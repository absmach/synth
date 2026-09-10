// SPDX-License-Identifier: Apache-2.0

//! Read KiCad bundled footprint files (`*.kicad_mod`) and extract
//! the geometric primitives the placer / router need.
//!
//! Footprints live at
//! `<bundled_dir>/<lib>.pretty/<name>.kicad_mod` on a stock install.
//! `<bundled_dir>` is `/usr/share/kicad/footprints` on Linux,
//! `/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints`
//! on macOS, or whatever `KICAD_FOOTPRINT_DIR` overrides it to.
//!
//! Phase 7 slice 1C exposes two helpers:
//!
//! - [`courtyard_bbox`] — the F.CrtYd / B.CrtYd rectangle that the
//!   placer must keep clear of every other component. Slice 2's
//!   constraint solver uses this for collision avoidance.
//! - [`pads`] — pad number, centre, size, layers. Phase 8 (routing)
//!   needs this for net assignment to pads and trace endpoints.
//!
//! ## Parsing approach
//!
//! `*.kicad_mod` files are s-expressions but we don't pull in a
//! parser. The geometry we need lives in flat `(fp_rect ...)`,
//! `(fp_line ...)`, and `(pad ...)` blocks with predictable
//! field order. A scan-and-extract approach (find the block,
//! pull the named `(start ...)` / `(end ...)` / `(at ...)` /
//! `(size ...)` sub-fields) is ~150 LOC and matches the s-exp
//! scanner already used by `kicad_lib_loader` for symbols.
//!
//! Returns `None` when the bundled footprint directory isn't
//! installed locally (CI without KiCad). Callers fall back to
//! the symbol-bbox approximation in
//! [`super::body_size_for_part`].

use std::path::PathBuf;
use std::sync::OnceLock;

/// Resolve the bundled-footprint directory.
fn bundled_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        // Tier-2 user footprints (Phase 15, R15.4): generated `.kicad_mod`
        // files written by `synth part import lcsc` land here and take
        // precedence over system/bundled KiCad libraries.
        if let Ok(env) = std::env::var("SYNTH_USER_FOOTPRINT_DIR") {
            let p = PathBuf::from(env);
            if p.is_dir() {
                return Some(p);
            }
        }
        if let Ok(env) = std::env::var("KICAD_FOOTPRINT_DIR") {
            let p = PathBuf::from(env);
            if p.is_dir() {
                return Some(p);
            }
        }
        for candidate in [
            "/usr/share/kicad/footprints",
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints",
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

fn read_footprint(lib_id: &str) -> Option<String> {
    let (lib, name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let path = dir
        .join(format!("{lib}.pretty"))
        .join(format!("{name}.kicad_mod"));
    std::fs::read_to_string(&path).ok()
}

/// Resolve only the Tier-2 user footprint directory (set via
/// `SYNTH_USER_FOOTPRINT_DIR`). Returns `None` when unset or not a directory.
pub fn user_footprint_dir() -> Option<PathBuf> {
    let env = std::env::var("SYNTH_USER_FOOTPRINT_DIR").ok()?;
    let p = PathBuf::from(env);
    p.is_dir().then_some(p)
}

/// Enumerate every `<lib>.pretty` directory under the user footprint dir
/// (Tier-2 imports from `synth part import-lcsc`), paired with its library
/// nickname. The exporter registers these in the project's `fp-lib-table` so
/// inlined footprint references resolve without a "library not found" warning.
pub fn user_footprint_libs() -> Vec<(String, PathBuf)> {
    let Some(dir) = user_footprint_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("pretty") {
                        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                            out.push((name.to_string(), path));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Top-level child heads that the PCB emitter overrides on the
/// instance and must therefore strip from the embedded body.
const INSTANCE_OVERRIDES: &[&str] = &[
    // Metadata KiCad supplies itself.
    "version",
    "generator",
    "generator_version",
    // The footprint's own `(layer "F.Cu")` clashes with the
    // placement layer we set per instance (Top/Bottom).
    "layer",
    // Documentation strings — keep the file small; pcbnew
    // doesn't need them on instances.
    "descr",
    "tags",
];

/// Property names that the PCB instance always overrides.
const PROPERTY_OVERRIDES: &[&str] = &[
    "Reference",
    "Value",
    "Footprint",
    "Datasheet",
    "Description",
];

/// Read the named footprint's `.kicad_mod` and return the
/// **inner body** — everything between `(footprint "<name>"` and
/// the final close paren, *minus* the children the PCB emitter
/// sets per-instance (`INSTANCE_OVERRIDES` heads and
/// `PROPERTY_OVERRIDES` property names).
///
/// The returned text is meant to be wrapped as a `Sexp::Raw` and
/// embedded inside a `(footprint "Lib:Name" ...)` instance in
/// the exporter's `.kicad_pcb`. The instance supplies its own
/// header (lib_id, layer, uuid, at, Reference, Value); this body
/// contributes the geometry (`fp_line`, `pad`, `model`, ...).
///
/// Without this, KiCad's DRC flags `lib_footprint_mismatch` for
/// every instance because the file references a footprint
/// definition that isn't cached inline. With it, the .kicad_pcb
/// is self-contained the same way our .kicad_sch is.
///
/// Returns `None` when the bundled footprint directory isn't
/// installed locally or the file is missing — the caller falls
/// back to instance-only emission and pcbnew shows a
/// "no body cached" warning rather than failing to open.
pub fn inline_body(lib_id: &str) -> Option<String> {
    let text = read_footprint(lib_id)?;
    let text = text
        .replace("${KICAD10_3DMODEL_DIR}", "/usr/share/kicad/3dmodels")
        .replace("${KICAD8_3DMODEL_DIR}", "/usr/share/kicad/3dmodels")
        .replace("${KICAD7_3DMODEL_DIR}", "/usr/share/kicad/3dmodels")
        .replace("${KICAD6_3DMODEL_DIR}", "/usr/share/kicad/3dmodels");
    let body_start = footprint_body_start(&text)?;
    let body_end = find_matching_close(&text, body_start)?;
    let body = &text[body_start..body_end];
    Some(filter_overrides(body))
}

/// Offset just past the closing quote of the outer
/// `(footprint "<name>"` — i.e. the first byte at which top-level
/// children start.
fn footprint_body_start(text: &str) -> Option<usize> {
    let open = text.find("(footprint")?;
    let after_kw = open + "(footprint".len();
    let rest = &text[after_kw..];
    let q1 = rest.find('"')? + 1;
    let q2 = rest[q1..].find('"')? + q1 + 1;
    Some(after_kw + q2)
}

/// Given an offset that sits inside an open `(footprint ...)`,
/// return the offset of the matching close paren.
fn find_matching_close(text: &str, from: usize) -> Option<usize> {
    let mut depth = 1_i32;
    for (i, b) in text.as_bytes().iter().enumerate().skip(from) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Walk top-level children of `body` and emit only those that
/// aren't in [`INSTANCE_OVERRIDES`] (by head atom) and aren't a
/// [`PROPERTY_OVERRIDES`] property.
fn filter_overrides(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if bytes[i] != b'(' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let Some(end) = find_matching_close(body, i + 1) else {
            out.push_str(&body[i..]);
            break;
        };
        let block = &body[i..=end];
        if should_keep_block(block) {
            out.push_str(block);
        }
        i = end + 1;
    }
    out
}

fn should_keep_block(block: &str) -> bool {
    let head = block
        .trim_start_matches('(')
        .split(|c: char| c.is_ascii_whitespace() || c == '(' || c == ')')
        .next()
        .unwrap_or("");
    if INSTANCE_OVERRIDES.contains(&head) {
        return false;
    }
    if head == "property" {
        let after = &block["(property".len()..];
        if let Some(q1) = after.find('"') {
            let rest = &after[q1 + 1..];
            if let Some(q2) = rest.find('"') {
                let name = &rest[..q2];
                if PROPERTY_OVERRIDES.contains(&name) {
                    return false;
                }
            }
        }
    }
    true
}

/// Return `(width_mm, height_mm)` of the footprint's courtyard
/// (the keep-out rectangle the placer must respect). Reads
/// `(fp_rect ...)` blocks on layer `F.CrtYd`; if a footprint
/// draws its courtyard with multiple `(fp_line ...)` segments
/// instead, the bbox of the line endpoints is returned. Returns
/// `None` when:
/// - KiCad isn't installed locally;
/// - the footprint file doesn't exist;
/// - the footprint has no `F.CrtYd` geometry (rare; almost every
///   KiCad-bundled footprint declares one).
///
/// Return `(center_offset_x_mm, center_offset_y_mm, width_mm, height_mm)` of the footprint's courtyard.
pub fn courtyard_rect(lib_id: &str) -> Option<(f64, f64, f64, f64)> {
    let text = read_footprint(lib_id)?;
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut found = false;

    let mut pos = 0;
    while let Some(block_start) = find_next_block(&text, pos, &["(fp_rect", "(fp_line"]) {
        let Some(block_end) = balanced_paren_end(&text, block_start) else {
            break;
        };
        let block = &text[block_start..block_end];
        if block_contains_layer(block, "F.CrtYd") {
            if block.starts_with("(fp_rect") {
                if let Some((sx, sy)) = xy_after(block, "(start ") {
                    if let Some((ex, ey)) = xy_after(block, "(end ") {
                        min_x = min_x.min(sx).min(ex);
                        max_x = max_x.max(sx).max(ex);
                        min_y = min_y.min(sy).min(ey);
                        max_y = max_y.max(sy).max(ey);
                        found = true;
                    }
                }
            } else if block.starts_with("(fp_line") {
                if let Some((sx, sy)) = xy_after(block, "(start ") {
                    min_x = min_x.min(sx);
                    max_x = max_x.max(sx);
                    min_y = min_y.min(sy);
                    max_y = max_y.max(sy);
                    found = true;
                }
                if let Some((ex, ey)) = xy_after(block, "(end ") {
                    min_x = min_x.min(ex);
                    max_x = max_x.max(ex);
                    min_y = min_y.min(ey);
                    max_y = max_y.max(ey);
                    found = true;
                }
            }
        }
        pos = block_end;
    }
    if !found {
        return None;
    }
    let w = max_x - min_x;
    let h = max_y - min_y;
    let cx = f64::midpoint(min_x, max_x);
    let cy = f64::midpoint(min_y, max_y);
    Some((cx, cy, w, h))
}

/// Return `(width_mm, height_mm)` of the footprint's courtyard.
pub fn courtyard_bbox(lib_id: &str) -> Option<(f64, f64)> {
    courtyard_rect(lib_id).map(|(_, _, w, h)| (w, h))
}

/// Which copper layers a pad's copper exists on.
///
/// SMD pads name exactly one Cu layer (`F.Cu` or `B.Cu`);
/// through-hole pads use the `*.Cu` wildcard. The router must
/// only treat a pad as a routable endpoint on layers where its
/// copper actually exists — a trace ending on `B.Cu` beneath an
/// F.Cu-only SMD pad is dangling copper, which native KiCad DRC
/// flags as `track_dangling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadCopperLayers {
    /// Copper on F.Cu only.
    #[default]
    Front,
    /// Copper on B.Cu only.
    Back,
    /// Copper on both layers (through-hole, or an explicit
    /// `*.Cu` wildcard).
    Both,
    /// No copper layers (e.g. non-plated through hole `np_thru_hole`).
    None,
}

impl PadCopperLayers {
    /// True when the pad has copper on F.Cu.
    #[must_use]
    pub fn includes_front(self) -> bool {
        matches!(self, Self::Front | Self::Both)
    }

    /// True when the pad has copper on B.Cu.
    #[must_use]
    pub fn includes_back(self) -> bool {
        matches!(self, Self::Back | Self::Both)
    }

    /// True when the pad has copper on any layer.
    #[must_use]
    pub fn has_copper(self) -> bool {
        matches!(self, Self::Front | Self::Back | Self::Both)
    }
}

/// A single pad on a footprint. Slice 1C carries the minimum the
/// router needs: pad number (matches `Pin::number` in the
/// registry), centre offset relative to the footprint origin,
/// pad rectangle size, and the copper layer set. Drill diameter,
/// shape variants, and solder-mask expansions land in later slices.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pad {
    pub number: String,
    /// Centre offset from the footprint origin, in millimetres.
    pub center_mm: (f64, f64),
    /// Pad rectangle size in millimetres.
    pub size_mm: (f64, f64),
    /// Copper layers this pad exists on.
    pub copper_layers: PadCopperLayers,
    /// True when this pad is a non-plated through hole (NPTH mechanical mounting hole).
    pub is_npth: bool,
}

/// A parsed footprint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Footprint {
    pub lib_id: String,
    pub pads: Vec<Pad>,
}

/// Parse all pads in a footprint file. Returns `None`
/// if the footprint file can't be located. Pads appear in the
/// order they're listed in the `.kicad_mod` file (matches the
/// datasheet pin order in KiCad's curated libraries).
pub fn pads(lib_id: &str) -> Option<Vec<Pad>> {
    let text = read_footprint(lib_id)?;
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(block_start) = find_next_block(&text, pos, &["(pad "]) {
        let Some(block_end) = balanced_paren_end(&text, block_start) else {
            break;
        };
        let block = &text[block_start..block_end];
        if let Some(pad) = parse_pad(block) {
            out.push(pad);
        }
        pos = block_end;
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn parse_pad(block: &str) -> Option<Pad> {
    // Pad header: `(pad "<number>" <type> <shape>`. Pull the
    // quoted number first.
    let after_pad = block.strip_prefix("(pad ")?;
    let after_open_quote = after_pad.strip_prefix('"')?;
    let close = after_open_quote.find('"')?;
    let number = after_open_quote[..close].to_string();

    // Pad type is the token between the number and the shape:
    // `smd`, `thru_hole`, `np_thru_hole`, or `connect`.
    let after_number = &after_open_quote[close + 1..];
    let pad_type = after_number
        .split(|c: char| c.is_ascii_whitespace())
        .find(|t| !t.is_empty())
        .unwrap_or("");

    let is_npth = pad_type.contains("np_thru_hole");
    let copper_layers = parse_copper_layers(block, pad_type);

    // Stencil paste-only apertures (no copper, not NPTH) are not electrical or physical pads.
    if !is_npth && !copper_layers.has_copper() {
        return None;
    }

    let center = xy_after(block, "(at ").unwrap_or((0.0, 0.0));
    let size = xy_after(block, "(size ").unwrap_or((0.0, 0.0));
    Some(Pad {
        number,
        center_mm: center,
        size_mm: size,
        copper_layers,
        is_npth,
    })
}

/// Derive the copper layer set from a pad block's `(layers ...)`
/// field. Through-hole pads default to both layers; everything
/// else defaults to F.Cu when the field is missing (rare in the
/// curated libraries).
fn parse_copper_layers(block: &str, pad_type: &str) -> PadCopperLayers {
    if pad_type.contains("np_thru_hole") {
        return PadCopperLayers::None;
    }
    let Some(layers_idx) = block.find("(layers ") else {
        return if pad_type.contains("thru_hole") {
            PadCopperLayers::Both
        } else {
            PadCopperLayers::Front
        };
    };
    let after = &block[layers_idx..];
    let close = after.find(')').unwrap_or(after.len());
    let field = &after[..close];
    let front = field.contains("\"F.Cu\"") || field.contains("\"*.Cu\"");
    let back = field.contains("\"B.Cu\"") || field.contains("\"*.Cu\"");
    match (front, back) {
        (true, true) => PadCopperLayers::Both,
        (true, false) => PadCopperLayers::Front,
        (false, true) => PadCopperLayers::Back,
        // No Cu layer named at all (e.g. paste-only, npth, or a malformed
        // block): fall back to the pad-type heuristic.
        (false, false) => {
            if pad_type.contains("thru_hole") {
                PadCopperLayers::Both
            } else {
                PadCopperLayers::None
            }
        }
    }
}

/// Find the byte offset of the next occurrence of any of the
/// given prefixes in `text` starting at `from`. None when no
/// prefix is found.
fn find_next_block(text: &str, from: usize, prefixes: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for prefix in prefixes {
        if let Some(idx) = text[from..].find(prefix) {
            let absolute = from + idx;
            best = Some(best.map_or(absolute, |b| b.min(absolute)));
        }
    }
    best
}

/// Given a byte offset pointing at an opening `(`, return the
/// offset *one past* the matching close paren.
fn balanced_paren_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return None;
    }
    let mut depth = 0_i32;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// True when `block` contains `(layer "<target>")`.
fn block_contains_layer(block: &str, target: &str) -> bool {
    let needle = format!("(layer \"{target}\")");
    block.contains(&needle)
}

/// Parse the first `<key>X Y ...)` after `key` and return `(X, Y)`.
fn xy_after(text: &str, key: &str) -> Option<(f64, f64)> {
    let idx = text.find(key)?;
    let after = &text[idx + key.len()..];
    let mut tokens = after.split_whitespace();
    let x = tokens.next()?.parse::<f64>().ok()?;
    let y_token = tokens.next()?.trim_end_matches(')');
    let y = y_token.parse::<f64>().ok()?;
    Some((x, y))
}

/// Synthesize a fallback pad layout for a registry part that has no
/// bundled KiCad footprint (`kicad_footprint` is `None`).
///
/// This is the geometry the exporter emits when it can't reference a
/// real `.kicad_mod` (e.g. a brand-new module like the ESP32-C61-MINI-1
/// that has no official KiCad library yet), and — crucially — the
/// **same** geometry the router stamps when `pads()` returns `None`.
/// Keeping both sides in lock-step means the exported copper lands
/// exactly where the maze router planned traces, so the part connects
/// instead of leaving dangling/via-dangling copper.
///
/// Pads are laid out in two columns (left/right) straddling the part's
/// `footprint_dimensions` rectangle, one pad per IR pin, named by the
/// pin's `number` so the net assignment lookup matches the schematic.
/// The pitch is derived from the body height so every pin fits with a
/// safe gap (no solder-mask bridge between adjacent pads).
pub fn synth_part_pads(part: &synth_registry::Part) -> Option<Vec<Pad>> {
    if part.pins.is_empty() {
        return None;
    }
    let (w_mm, h_mm) = part.footprint_dimensions.as_ref().map_or_else(
        || default_synth_dims(part.pins.len()),
        |d| (d.width_mm, d.height_mm),
    );
    let n = part.pins.len();
    let left = n.div_ceil(2);
    let right = n - left;
    let pad_w = 0.6_f64;
    let pad_h = 0.45_f64;
    let inset = (w_mm / 2.0).min(0.9_f64);
    let pitch_l = if left > 1 {
        h_mm / (left as f64 + 1.0)
    } else {
        0.0
    };
    let pitch_r = if right > 1 {
        h_mm / (right as f64 + 1.0)
    } else {
        0.0
    };
    let mut pads = Vec::with_capacity(n);
    for (i, pin) in part.pins.iter().enumerate() {
        let (x, y) = if i < left {
            let y = if left > 1 {
                -h_mm / 2.0 + (i as f64 + 1.0) * pitch_l
            } else {
                0.0
            };
            (-(w_mm / 2.0 - inset), y)
        } else {
            let j = i - left;
            let y = if right > 1 {
                -h_mm / 2.0 + (j as f64 + 1.0) * pitch_r
            } else {
                0.0
            };
            (w_mm / 2.0 - inset, y)
        };
        pads.push(Pad {
            number: pin.number.0.clone(),
            center_mm: (x, y),
            size_mm: (pad_w, pad_h),
            copper_layers: PadCopperLayers::Front,
            is_npth: false,
        });
    }
    Some(pads)
}

/// Default body size for a part with no explicit `footprint_dimensions`:
/// tiny for 2-pin passives, small for up to 8 pins, otherwise a tall
/// dual-column strip (matches the heuristic in `pcb.rs`).
fn default_synth_dims(pin_count: usize) -> (f64, f64) {
    if pin_count <= 2 {
        (2.0, 2.0)
    } else if pin_count <= 8 {
        (3.0, 3.0)
    } else {
        (7.62, 36.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn courtyard_bbox_from_fp_rect() {
        let block = r#"(footprint "test"
    (fp_rect
        (start -1.48 -0.73)
        (end 1.48 0.73)
        (stroke (width 0.05) (type solid))
        (fill no)
        (layer "F.CrtYd")
    )
)"#;
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut pos = 0;
        while let Some(start) = find_next_block(block, pos, &["(fp_rect"]) {
            let end = balanced_paren_end(block, start).unwrap();
            let b = &block[start..end];
            if block_contains_layer(b, "F.CrtYd") {
                let (sx, sy) = xy_after(b, "(start ").unwrap();
                let (ex, ey) = xy_after(b, "(end ").unwrap();
                min_x = min_x.min(sx).min(ex);
                max_x = max_x.max(sx).max(ex);
                min_y = min_y.min(sy).min(ey);
                max_y = max_y.max(sy).max(ey);
            }
            pos = end;
        }
        assert!((max_x - min_x - 2.96).abs() < 0.0001);
        assert!((max_y - min_y - 1.46).abs() < 0.0001);
    }

    #[test]
    fn balanced_paren_handles_nested() {
        let s = "((a b) (c (d e)) f)";
        assert_eq!(balanced_paren_end(s, 0), Some(s.len()));
    }

    #[test]
    fn parse_pad_extracts_number_at_size() {
        let block = r#"(pad "1" smd roundrect
    (at -0.825 0)
    (size 0.8 0.95)
    (layers "F.Cu" "F.Mask" "F.Paste")
    (roundrect_rratio 0.25)
)"#;
        let pad = parse_pad(block).unwrap();
        assert_eq!(pad.number, "1");
        assert!((pad.center_mm.0 - -0.825).abs() < 1e-9);
        assert!((pad.center_mm.1).abs() < 1e-9);
        assert_eq!(pad.size_mm, (0.8, 0.95));
        assert_eq!(pad.copper_layers, PadCopperLayers::Front);
    }

    #[test]
    fn parse_pad_detects_copper_layers() {
        let smd_front = r#"(pad "1" smd roundrect
    (at 0 0)
    (size 1 1)
    (layers "F.Cu" "F.Mask" "F.Paste")
)"#;
        let smd_back = r#"(pad "1" smd roundrect
    (at 0 0)
    (size 1 1)
    (layers "B.Cu" "B.Mask" "B.Paste")
)"#;
        let tht = r#"(pad "1" thru_hole circle
    (at 0 0)
    (size 1.5 1.5)
    (drill 0.8)
    (layers "*.Cu" "*.Mask")
)"#;
        let no_layers_field = r#"(pad "1" thru_hole circle
    (at 0 0)
    (size 1.5 1.5)
)"#;
        let no_cu_named = r#"(pad "1" smd rect
    (at 0 0)
    (size 1 1)
    (layers "F.Mask")
)"#;
        assert_eq!(
            parse_pad(smd_front).unwrap().copper_layers,
            PadCopperLayers::Front
        );
        assert_eq!(
            parse_pad(smd_back).unwrap().copper_layers,
            PadCopperLayers::Back
        );
        assert_eq!(parse_pad(tht).unwrap().copper_layers, PadCopperLayers::Both);
        assert_eq!(
            parse_pad(no_layers_field).unwrap().copper_layers,
            PadCopperLayers::Both
        );
        // Paste/mask-only block without copper or NPTH is ignored.
        assert!(parse_pad(no_cu_named).is_none());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Smoke-test that exercises the real bundled-footprint
    /// reader against KiCad's stock library. Skipped silently
    /// when KiCad isn't installed (CI without bundled
    /// footprints).
    #[test]
    fn courtyard_bbox_matches_known_footprints() {
        let cases = [
            ("Resistor_SMD:R_0603_1608Metric", 2.96, 1.46),
            ("Package_DIP:DIP-28_W7.62mm", 9.73, 36.08),
        ];
        for (lib_id, expected_w, expected_h) in cases {
            let Some((w, h)) = courtyard_bbox(lib_id) else {
                eprintln!("skipping {lib_id}: footprint not installed");
                continue;
            };
            assert!(
                (w - expected_w).abs() < 0.15,
                "{lib_id}: w {w:.3} vs expected {expected_w:.3}"
            );
            assert!(
                (h - expected_h).abs() < 0.15,
                "{lib_id}: h {h:.3} vs expected {expected_h:.3}"
            );
        }
    }
}
