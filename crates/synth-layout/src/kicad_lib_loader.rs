// SPDX-License-Identifier: Apache-2.0

//! Load stock KiCad symbol definitions from the user's KiCad install
//! and return them as raw text suitable for embedding in our
//! exported schematic's `(lib_symbols ...)` block.
//!
//! ## Why we embed
//!
//! KiCad's convention is that every `(lib_id "X:Y")` referenced in a
//! schematic has a matching `(symbol "X:Y" ...)` definition embedded
//! in the schematic's `(lib_symbols ...)` block. Without that
//! embedded definition, KiCad renders a `?U?` placeholder regardless
//! of whether the user's global sym-lib-table can resolve the
//! library — the embedded block is the authoritative cache.
//!
//! ## How we load
//!
//! KiCad ships its bundled symbol libraries at well-known paths;
//! Linux puts them under `/usr/share/kicad/symbols/`. We don't
//! parse the s-expression — we scan for `(symbol "<name>"` and
//! return the balanced-paren block as a raw string, with the
//! outer symbol name rewritten from `"<name>"` to `"<lib>:<name>"`
//! to match KiCad's lib_symbols naming convention.
//!
//! If the bundled library can't be found (e.g. CI without KiCad
//! installed), `load_symbol` returns `None` and the caller falls
//! back to a synthesized rectangle. That keeps the exporter usable
//! anywhere; visual fidelity in KiCad simply requires KiCad to be
//! installed locally, which it almost certainly is on the
//! developer's machine that opens the schematic.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Resolve the bundled-symbol directory by checking, in order:
///
/// 1. `KICAD_SYMBOL_DIR` environment variable (escape hatch for
///    custom installs / CI overrides).
/// 2. `/usr/share/kicad/symbols` (Linux distro default).
/// 3. `/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols`
///    (macOS).
fn bundled_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(env) = std::env::var("KICAD_SYMBOL_DIR") {
            let p = PathBuf::from(env);
            if p.is_dir() {
                return Some(p);
            }
        }
        for candidate in [
            "/usr/share/kicad/symbols",
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols",
        ] {
            let p = PathBuf::from(candidate);
            if p.is_dir() {
                return Some(p);
            }
        }
        None
    })
    .clone()
}

/// Given a KiCad `lib_id` of the form `"Lib:Symbol"`, return the
/// embedded-friendly s-expression text for that symbol, with the
/// outer name rewritten to include the library prefix (so the
/// resulting block can be dropped straight into `(lib_symbols ...)`).
///
/// Returns `None` if:
/// - the `lib_id` doesn't contain `:` (not a stock reference);
/// - the bundled symbol directory isn't found on disk;
/// - the named library file doesn't exist;
/// - the symbol can't be located inside the library file.
///
/// On any of these the caller should fall back to a synthesized
/// rectangle so the export still succeeds.
pub fn load_symbol(lib_id: &str) -> Option<String> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let lib_path = dir.join(format!("{lib_name}.kicad_sym"));
    let text = std::fs::read_to_string(&lib_path).ok()?;

    let mut chain = Vec::new();
    let mut curr = sym_name.to_string();
    let mut visited = std::collections::HashSet::new();
    while visited.insert(curr.clone()) {
        if let Some(block) = extract_symbol(&text, &curr) {
            let target = extends_target(&block);
            chain.push((curr.clone(), block));
            if let Some(t) = target {
                curr = t;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if chain.is_empty() {
        return None;
    }

    let (main_name, main_block) = &chain[0];
    let mut extra_sub_symbols = Vec::new();
    for (name, block) in chain.iter().skip(1) {
        let mut pos = 0;
        let child_needle = format!("(symbol \"{name}_");
        while let Some(idx) = block[pos..].find(&child_needle) {
            let abs_start = pos + idx;
            if let Some(sub_block) = extract_balanced_sub_block(&block[abs_start..]) {
                let old_prefix = format!("(symbol \"{name}_");
                let new_prefix = format!("(symbol \"{main_name}_");
                let renamed = sub_block.replace(&old_prefix, &new_prefix);
                extra_sub_symbols.push(renamed);
                pos = abs_start + sub_block.len();
            } else {
                pos = abs_start + child_needle.len();
            }
        }
    }

    let mut main_rewritten = rewrite_outer_name(main_block, lib_id);
    main_rewritten = remove_extends_line(&main_rewritten);

    if !extra_sub_symbols.is_empty() {
        if let Some(last_paren) = main_rewritten.rfind(')') {
            let mut insert_str = String::new();
            for sub in extra_sub_symbols {
                insert_str.push_str("\n\t\t");
                insert_str.push_str(&sub);
            }
            main_rewritten.insert_str(last_paren, &insert_str);
        }
    }

    Some(main_rewritten)
}

fn remove_extends_line(text: &str) -> String {
    if let Some(start) = text.find("(extends \"") {
        if let Some(end) = text[start..].find('\n') {
            let mut s = text.to_string();
            s.drain(start..=start + end);
            return s;
        }
    }
    text.to_string()
}

fn extract_balanced_sub_block(text: &str) -> Option<String> {
    let mut depth = 0_i32;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(text[..=i].to_string());
            }
        }
    }
    None
}

/// Vertical pitch between the units of one multi-unit symbol, in mm.
/// The placer keeps one placement per package, so the units are drawn
/// as a stack anchored at that placement; the router and the schematic
/// exporter must agree on this offset or the wires miss the pins.
pub const UNIT_PITCH_MM: f64 = 12.7;

/// Offset of unit `unit` (1-based) from the package anchor, in KiCad
/// sheet coordinates (y grows downward, so units stack downward).
#[must_use]
pub fn unit_offset_mm(unit: u32) -> (f64, f64) {
    (0.0, f64::from(unit.saturating_sub(1)) * UNIT_PITCH_MM)
}

/// Sheet-space offset of the unit a pin belongs to, or `(0, 0)` for a
/// single-unit symbol (or a pin with no declared unit). The router and
/// the schematic exporter must both apply this or the wires miss the
/// pins of units 2..N.
#[must_use]
pub fn pin_unit_offset(lib_id: &str, number: &str) -> (f64, f64) {
    symbol_units(lib_id)
        .and_then(|(map, count)| {
            if count > 1 {
                map.get(number).copied()
            } else {
                None
            }
        })
        .map_or((0.0, 0.0), unit_offset_mm)
}

/// Physical pin number -> KiCad unit number (1-based) for a stock
/// symbol, plus the number of units it declares. A single-unit symbol
/// yields an empty map and a count of 1; `None` when the symbol can't
/// be found.
///
/// KiCad declares units as sub-symbols named `<Symbol>_<unit>_<style>`
/// (`LM2904_1_1`, `LM2904_2_1`, `LM2904_3_1`). Unit `0` is common to
/// every unit; its pins are attributed to unit 1 here, which is where
/// the router draws them.
pub fn symbol_units(lib_id: &str) -> Option<(std::collections::BTreeMap<String, u32>, u32)> {
    /// Pin number -> unit, plus the unit count, for one `lib_id`.
    type Units = (std::collections::BTreeMap<String, u32>, u32);
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Option<Units>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = guard.get(lib_id) {
        return hit.clone();
    }
    let computed = symbol_units_uncached(lib_id);
    guard.insert(lib_id.to_string(), computed.clone());
    computed
}

fn symbol_units_uncached(lib_id: &str) -> Option<(std::collections::BTreeMap<String, u32>, u32)> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let text = std::fs::read_to_string(dir.join(format!("{lib_name}.kicad_sym"))).ok()?;
    symbol_units_from_source(&text, sym_name)
}

/// [`symbol_units`] against already-loaded library text, following
/// `(extends …)` chains within it.
#[must_use]
pub fn symbol_units_from_source(
    text: &str,
    sym_name: &str,
) -> Option<(std::collections::BTreeMap<String, u32>, u32)> {
    let mut chain = Vec::new();
    let mut curr = sym_name.to_string();
    let mut visited = HashSet::new();
    while visited.insert(curr.clone()) {
        let Some(block) = extract_symbol(text, &curr) else {
            break;
        };
        let target = extends_target(&block);
        chain.push(block);
        match target {
            Some(t) => curr = t,
            None => break,
        }
    }
    if chain.is_empty() {
        return None;
    }
    let mut map = std::collections::BTreeMap::new();
    let mut seen = HashSet::new();
    let mut max_unit = 0_u32;
    // Subclass first: its unit assignment for a shared pin number wins.
    for block in &chain {
        collect_unit_pins(block, &mut map, &mut seen, &mut max_unit);
    }
    Some((map, max_unit.max(1)))
}

fn collect_unit_pins(
    block: &str,
    map: &mut std::collections::BTreeMap<String, u32>,
    seen: &mut HashSet<String>,
    max_unit: &mut u32,
) {
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(symbol \"") {
        let abs = pos + idx;
        let Some(name) = find_quoted_value(&block[abs..], "(symbol \"") else {
            pos = abs + 9;
            continue;
        };
        let Some(unit) = unit_of_sub_symbol(&name) else {
            pos = abs + 9;
            continue;
        };
        let Some(sub) = extract_balanced_sub_block(&block[abs..]) else {
            pos = abs + 9;
            continue;
        };
        let len = sub.len();
        if unit > 0 {
            *max_unit = (*max_unit).max(unit);
        }
        let draw_unit = if unit == 0 { 1 } else { unit };
        for number in pin_numbers(&sub) {
            if seen.insert(number.clone()) {
                map.insert(number, draw_unit);
            }
        }
        pos = abs + len;
    }
}

/// The `<unit>` in a sub-symbol name `<Symbol>_<unit>_<style>`.
fn unit_of_sub_symbol(name: &str) -> Option<u32> {
    let mut parts = name.rsplitn(3, '_');
    let style = parts.next()?;
    let unit = parts.next()?;
    if style.is_empty() || !style.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    unit.parse::<u32>().ok()
}

fn pin_numbers(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(number \"") {
        let abs = pos + idx;
        if let Some(n) = find_quoted_value(&block[abs..], "(number \"") {
            out.push(n);
        }
        pos = abs + 9;
    }
    out
}

/// Return map of pin number (and pin name) -> (local_x_mm, local_y_mm, angle_deg)
/// for a stock KiCad symbol. Follows `(extends ...)` chains.
///
/// Results are cached per `lib_id`: the routing passes in `synth-layout`
/// call this for every pin of every component while carving approach
/// corridors, and re-reading + re-parsing the (large) library files from
/// disk each time pushed `synth layout` far past its latency budget.
type PinPositionsCache =
    std::sync::Mutex<std::collections::HashMap<String, Option<Vec<(String, (f64, f64, f64))>>>>;

pub fn pin_positions(lib_id: &str) -> Option<std::collections::HashMap<String, (f64, f64, f64)>> {
    static CACHE: OnceLock<PinPositionsCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = guard.get(lib_id) {
        return hit
            .as_ref()
            .map(|entries| entries.iter().cloned().collect());
    }

    let computed = pin_positions_uncached(lib_id).map(|map| {
        let mut entries: Vec<(String, (f64, f64, f64))> = map.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    });
    guard.insert(lib_id.to_string(), computed.clone());
    computed.map(|entries| entries.into_iter().collect())
}

/// One physical pin parsed from a stock KiCad symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalPin {
    /// Physical pin number as printed in the datasheet (`"1"`, `"7"`).
    pub number: String,
    /// Pin name as written in the symbol (`"VDD"`, `"PA0"`).
    pub name: String,
    /// KiCad electrical type keyword (`power_in`, `power_out`,
    /// `input`, `output`, `bidirectional`, ...).
    pub electrical_type: String,
    /// Local coordinates in mm within the symbol frame (y-up).
    pub x: f64,
    pub y: f64,
}

/// Return the *full* physical pin inventory of a stock KiCad symbol:
/// every pin declared in the `(pin <electrical> <shape> ...) <unit>`
/// blocks, with its electrical type and local position. Follows
/// `(extends ...)` chains. `None` when the bundled library or symbol
/// can't be located.
///
/// This differs from [`pin_positions`]: it includes electrical types
/// and *every* physical pin — not just those named in the registry —
/// which is what the schematic exporter needs to (a) fan out power
/// nets to every physical power leg and (b) emit `(no_connect)`
/// markers for every remaining un-mapped pin so KiCad ERC stays
/// clean. Results are cached per `lib_id`, mirroring `pin_positions`.
type PhysicalPinCache =
    std::sync::Mutex<std::collections::HashMap<String, Option<Vec<PhysicalPin>>>>;

pub fn physical_pins(lib_id: &str) -> Option<Vec<PhysicalPin>> {
    static CACHE: OnceLock<PhysicalPinCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = guard.get(lib_id) {
        return hit.clone();
    }

    let computed = physical_pins_uncached(lib_id);
    guard.insert(lib_id.to_string(), computed.clone());
    computed
}

fn physical_pins_uncached(lib_id: &str) -> Option<Vec<PhysicalPin>> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let lib_path = dir.join(format!("{lib_name}.kicad_sym"));
    let text = std::fs::read_to_string(&lib_path).ok()?;
    physical_pins_from_source(&text, sym_name)
}

/// Extract the full physical pin inventory of a symbol named
/// `sym_name` from an already-loaded `.kicad_sym` file's raw text,
/// following `(extends ...)` chains within that same text. Shared by
/// the bundled-library lookup above and by [`crate::kicad_zip`],
/// which loads `.kicad_sym` text from a SnapEDA/UltraLibrarian export
/// zip instead of the system KiCad install.
#[must_use]
pub fn physical_pins_from_source(text: &str, sym_name: &str) -> Option<Vec<PhysicalPin>> {
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut curr = sym_name.to_string();
    let mut visited = std::collections::HashSet::new();
    while visited.insert(curr.clone()) {
        let block = extract_symbol(text, &curr)?;
        let target = extends_target(&block);
        chain.push((curr.clone(), block));
        if let Some(t) = target {
            curr = t;
        } else {
            break;
        }
    }

    let mut pins: Vec<PhysicalPin> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Walk the inheritance chain from the subclass down to the
    // parent: pins in a subclass override same-numbered pins in the
    // parent, so process the root first, then walk up.
    for (_, block) in &chain {
        collect_physical_pins(block, &mut pins, &mut seen);
    }

    if pins.is_empty() {
        None
    } else {
        Some(pins)
    }
}

/// Find the name of the first top-level `(symbol "Name" ...)` block
/// in a standalone `.kicad_sym` file's text. Single-part export files
/// (SnapEDA, UltraLibrarian, KiCad's own "Save symbol as new library")
/// always open with exactly one such block before any nested
/// `Name_0_1`/`Name_1_1` sub-unit symbols, so the first match is
/// always the part itself.
#[must_use]
pub fn first_symbol_name(text: &str) -> Option<String> {
    let start = text.find("(symbol \"")?;
    find_quoted_value(&text[start..], "(symbol \"")
}

fn collect_physical_pins(block: &str, out: &mut Vec<PhysicalPin>, seen: &mut HashSet<String>) {
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(pin ") {
        let absolute = pos + idx;
        let rest = &block[absolute..];
        // A pin block is `(pin <electrical> <shape> (at ...) ...)`.
        let end_idx = rest.find("\n  )").unwrap_or(rest.len());
        let pin_block = &rest[..end_idx];
        let number = find_quoted_value(pin_block, "(number \"");
        let name = find_quoted_value(pin_block, "(name \"");
        let xy_angle = find_xy_angle(pin_block, "(at ");
        let electrical = electrical_type_of(pin_block);
        if let (Some(num), Some(name), Some((x, y, _)), Some(et)) =
            (number, name, xy_angle, electrical)
        {
            if seen.insert(num.clone()) {
                out.push(PhysicalPin {
                    number: num,
                    name,
                    electrical_type: et,
                    x,
                    y,
                });
            }
        }
        pos = absolute + "(pin ".len();
    }
}

/// Read the `power_in` / `bidirectional` / ... token that follows the
/// `(pin ` opening. KiCad writes `(pin power_in line (at ...) ...)`.
fn electrical_type_of(pin_block: &str) -> Option<String> {
    let head = pin_block.strip_prefix("(pin ")?;
    let head = head.trim_start();
    let token_end = head.find(char::is_whitespace).unwrap_or(head.len());
    let token = &head[..token_end];
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn pin_positions_uncached(
    lib_id: &str,
) -> Option<std::collections::HashMap<String, (f64, f64, f64)>> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let lib_path = dir.join(format!("{lib_name}.kicad_sym"));
    let text = std::fs::read_to_string(&lib_path).ok()?;
    pin_positions_inner(&text, sym_name)
}

fn pin_positions_inner(
    text: &str,
    sym_name: &str,
) -> Option<std::collections::HashMap<String, (f64, f64, f64)>> {
    let block = extract_symbol(text, sym_name)?;
    let mut map = std::collections::HashMap::new();

    if let Some(target) = extends_target(&block) {
        if let Some(target_map) = pin_positions_inner(text, &target) {
            map.extend(target_map);
        }
    }

    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(pin ") {
        let absolute = pos + idx;
        let rest = &block[absolute..];

        // Find end of this (pin ...) block
        let end_idx = rest.find("\n  )").unwrap_or(rest.len());
        let pin_block = &rest[..end_idx];

        let xy_angle = find_xy_angle(pin_block, "(at ");
        let number = find_quoted_value(pin_block, "(number \"");
        let name = find_quoted_value(pin_block, "(name \"");

        if let Some((x, y, angle)) = xy_angle {
            if let Some(num) = number {
                map.insert(num.clone(), (x, y, angle));
            }
            if let Some(n) = name {
                if n != "~" && !n.is_empty() {
                    map.insert(n.to_lowercase(), (x, y, angle));
                }
            }
        }

        pos = absolute + "(pin ".len();
    }

    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn find_xy_angle(text: &str, key: &str) -> Option<(f64, f64, f64)> {
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let mut tokens = after.split_whitespace();
    let x = tokens.next()?.parse::<f64>().ok()?;
    let y = tokens.next()?.parse::<f64>().ok()?;
    let angle_token = tokens.next()?.trim_end_matches(')');
    let angle = angle_token.parse::<f64>().unwrap_or(0.0);
    Some((x, y, angle))
}

fn find_quoted_value(text: &str, key: &str) -> Option<String> {
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Return the body bbox `(width_mm, height_mm)` of a KiCad stock
/// symbol by parsing its `(rectangle (start ...) (end ...))` shapes.
/// Used by cluster placement to size the grid around the real
/// rendered footprint instead of the synthesized-rectangle estimate.
///
/// Follows `(extends "OtherName")` chains: if the symbol only
/// extends another, recursively resolve the target in the same
/// library file. Returns `None` if the symbol can't be located or
/// has no rectangle geometry; falls back to a pin-extent-based
/// estimate when no rectangle is present (rare for body symbols).
type BodyBboxCache = std::sync::Mutex<std::collections::HashMap<String, Option<(f64, f64)>>>;

pub fn body_bbox(lib_id: &str) -> Option<(f64, f64)> {
    static CACHE: OnceLock<BodyBboxCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(hit) = guard.get(lib_id) {
        return *hit;
    }
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let lib_path = dir.join(format!("{lib_name}.kicad_sym"));
    let text = std::fs::read_to_string(&lib_path).ok()?;
    let computed = body_bbox_inner(&text, sym_name);
    guard.insert(lib_id.to_string(), computed);
    computed
}

fn body_bbox_inner(text: &str, sym_name: &str) -> Option<(f64, f64)> {
    let block = extract_symbol(text, sym_name)?;
    if let Some(target) = extends_target(&block) {
        if let Some(bbox) = body_bbox_inner(text, &target) {
            return Some(bbox);
        }
    }
    rect_bbox(&block).or_else(|| pin_extent_bbox(&block))
}

fn extends_target(block: &str) -> Option<String> {
    let key = "(extends \"";
    let start = block.find(key)?;
    let after = &block[start + key.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Hull of every `(rectangle (start X Y) (end X Y))` in the symbol.
fn rect_bbox(block: &str) -> Option<(f64, f64)> {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut found_any = false;
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(rectangle") {
        let absolute = pos + idx;
        let rest = &block[absolute..];
        if let (Some((sx, sy)), Some((ex, ey))) = (find_xy(rest, "(start "), find_xy(rest, "(end "))
        {
            x_min = x_min.min(sx).min(ex);
            x_max = x_max.max(sx).max(ex);
            y_min = y_min.min(sy).min(ey);
            y_max = y_max.max(sy).max(ey);
            found_any = true;
        }
        pos = absolute + "(rectangle".len();
    }
    if !found_any {
        return None;
    }
    Some((x_max - x_min, y_max - y_min))
}

/// Fallback: derive bbox from the spread of pin connection points.
/// Pins live at `(pin <type> <shape> (at X Y ANGLE) ...)`. The body
/// must enclose every pin, so this is a conservative estimate when
/// the symbol has no `rectangle`.
fn pin_extent_bbox(block: &str) -> Option<(f64, f64)> {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut found_any = false;
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(pin ") {
        let absolute = pos + idx;
        let rest = &block[absolute..];
        if let Some((x, y)) = find_xy(rest, "(at ") {
            x_min = x_min.min(x);
            x_max = x_max.max(x);
            y_min = y_min.min(y);
            y_max = y_max.max(y);
            found_any = true;
        }
        pos = absolute + "(pin ".len();
    }
    if !found_any {
        return None;
    }
    Some((x_max - x_min, y_max - y_min))
}

fn find_xy(text: &str, key: &str) -> Option<(f64, f64)> {
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let mut tokens = after.split_whitespace();
    let x = tokens.next()?.parse::<f64>().ok()?;
    let y_token = tokens.next()?.trim_end_matches(')');
    let y = y_token.parse::<f64>().ok()?;
    Some((x, y))
}

/// Scan `text` for `(symbol "<name>"` and return the balanced-paren
/// block as a string, including the outermost parens.
fn extract_symbol(text: &str, name: &str) -> Option<String> {
    let needle = format!("(symbol \"{name}\"");
    let start = text.find(&needle)?;
    let mut depth = 0_i32;
    let bytes = text.as_bytes();
    let mut end = start;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(text[start..end].to_string())
}

/// Rewrite the outermost `(symbol "<old>" ...)` head to use
/// `<lib_id>` instead of `<old>`. KiCad's lib_symbols expects the
/// fully-qualified name; the source files store just the symbol
/// name.
fn rewrite_outer_name(block: &str, new_name: &str) -> String {
    let Some(quote_start) = block.find('"') else {
        return block.to_string();
    };
    let Some(quote_end) = block[quote_start + 1..]
        .find('"')
        .map(|i| quote_start + 1 + i)
    else {
        return block.to_string();
    };
    let mut out = String::with_capacity(block.len() + new_name.len());
    out.push_str(&block[..=quote_start]);
    out.push_str(new_name);
    out.push_str(&block[quote_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_balanced_block() {
        let text = r#"(kicad_symbol_lib
	(symbol "R"
		(property "Value" "R")
		(symbol "R_0_1"
			(rectangle (start 0 0) (end 1 1))
		)
	)
	(symbol "C" (property "Value" "C"))
)
"#;
        let r = extract_symbol(text, "R").expect("found R");
        assert!(r.starts_with("(symbol \"R\""));
        assert!(r.ends_with(')'));
        // Balanced — extracted exactly one symbol, not the second
        // one ("C") and not the enclosing library.
        assert!(!r.contains("(symbol \"C\""));
    }

    #[test]
    fn rewrite_keeps_inner_unchanged() {
        let block = "(symbol \"R\"\n\t(property \"Value\" \"R\")\n)";
        let rewritten = rewrite_outer_name(block, "Device:R");
        assert!(rewritten.starts_with("(symbol \"Device:R\""));
        // Inner property value is the second `"R"` and must be
        // untouched.
        assert!(rewritten.contains("\"Value\" \"R\""));
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn parses_units_from_a_dual_opamp() {
        let text = r#"
    (symbol "LM2904"
        (extends "Opamp_Dual")
        (symbol "LM2904_1_1"
            (pin output line (at 7.62 0 180) (length 2.54)
                (name "~" (effects (font (size 1.27 1.27))))
                (number "1" (effects (font (size 1.27 1.27)))))
        )
        (symbol "LM2904_2_1"
            (pin output line (at 7.62 0 180) (length 2.54)
                (name "~" (effects (font (size 1.27 1.27))))
                (number "7" (effects (font (size 1.27 1.27)))))
        )
        (symbol "LM2904_3_1"
            (pin power_in line (at 0 -7.62 90) (length 3.81)
                (name "V+" (effects (font (size 1.27 1.27))))
                (number "8" (effects (font (size 1.27 1.27)))))
        )
    )
"#;
        let (map, count) = symbol_units_from_source(text, "LM2904").expect("units");
        assert_eq!(count, 3);
        assert_eq!(map.get("1"), Some(&1));
        assert_eq!(map.get("7"), Some(&2));
        assert_eq!(map.get("8"), Some(&3));
    }

    #[test]
    fn single_unit_symbol_reports_one_unit() {
        let text = r#"
    (symbol "R"
        (symbol "R_0_1" (rectangle (start -1 1) (end 1 -1)))
        (symbol "R_1_1"
            (pin passive line (at 0 2.54 270) (length 1.27)
                (name "~" (effects (font (size 1.27 1.27))))
                (number "1" (effects (font (size 1.27 1.27)))))
        )
    )
"#;
        let (map, count) = symbol_units_from_source(text, "R").expect("units");
        assert_eq!(count, 1);
        assert_eq!(map.get("1"), Some(&1));
    }

    #[test]
    fn real_lm358_is_a_three_unit_symbol() {
        // Skipped when KiCad's bundled libraries aren't installed.
        let Some((map, count)) = symbol_units("Amplifier_Operational:LM358") else {
            return;
        };
        assert_eq!(count, 3, "LM358 has two amplifier units plus a power unit");
        assert_eq!(map.get("1"), Some(&1), "OUT_A");
        assert_eq!(map.get("7"), Some(&2), "OUT_B");
        assert_eq!(map.get("8"), Some(&3), "V+ power unit");
    }

    #[test]
    fn unit_offset_stacks_downward() {
        assert_eq!(unit_offset_mm(1), (0.0, 0.0));
        assert_eq!(unit_offset_mm(2), (0.0, UNIT_PITCH_MM));
    }
}
