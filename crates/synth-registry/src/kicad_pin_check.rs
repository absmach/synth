// SPDX-License-Identifier: Apache-2.0

//! Registry-load-time invariant: every pin number declared by a
//! `Part` with a `kicad_symbol` mapping must also exist in the
//! referenced KiCad stock symbol.
//!
//! Why: the schematic export and the (future) PCB export both emit
//! `(lib_id ...)` references pointing at KiCad's bundled library.
//! If the registry says `gnd` is pin 4 but KiCad's symbol puts GND
//! on pin 7, the schematic wires connect to a different physical
//! pad than the layout expects. Caught at fab time → catastrophic.
//! Caught at registry-load → trivial to fix.
//!
//! Implementation note: we don't parse KiCad's full s-expression
//! grammar. We scan the library text for `(symbol "<name>"`,
//! extract the balanced-paren block, follow `(extends "<parent>")`
//! when present, and collect every `(number "X" ...)`. ~80 LOC vs
//! a real parser.
//!
//! Graceful degradation: if KiCad isn't installed locally (CI
//! containers, fresh dev machines), `kicad_pin_numbers` returns
//! `None` and the verifier skips silently. The schematic export
//! falls back to a synthesized rectangle in that case too, so the
//! pipeline stays usable.

use std::collections::{BTreeMap, BTreeSet};

use crate::capability::ElectricalType;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Resolve the bundled-symbol directory by checking, in order:
///
/// 1. `KICAD_SYMBOL_DIR` env var (CI / custom installs).
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

/// Return the set of pin numbers declared by the named KiCad stock
/// symbol, following `(extends ...)` chains. `None` means the
/// library can't be located or the symbol doesn't exist — caller
/// treats either as "skip the check".
pub fn kicad_pin_numbers(lib_id: &str) -> Option<BTreeSet<String>> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let lib_path = dir.join(format!("{lib_name}.kicad_sym"));
    let text = std::fs::read_to_string(&lib_path).ok()?;
    let mut out = BTreeSet::new();
    if collect_pin_numbers(&text, sym_name, &mut out, 0) && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

/// Walk the symbol block accumulating pin numbers. Returns `true`
/// when the symbol was located (even if it has no pins of its own
/// and only `extends` another).
fn collect_pin_numbers(text: &str, sym_name: &str, out: &mut BTreeSet<String>, depth: u32) -> bool {
    if depth > 4 {
        // `extends` chains in KiCad practice are 1 or 2 hops max;
        // a longer chain is almost certainly a cycle or a parsing
        // mishap. Bail rather than recurse unbounded.
        return false;
    }
    let Some(block) = extract_symbol(text, sym_name) else {
        return false;
    };
    let mut pos = 0;
    while let Some(idx) = block[pos..].find("(number ") {
        let absolute = pos + idx + "(number ".len();
        if let Some(quote_start) = block[absolute..].find('"') {
            let abs_quote = absolute + quote_start + 1;
            if let Some(quote_end) = block[abs_quote..].find('"') {
                out.insert(block[abs_quote..abs_quote + quote_end].to_string());
            }
        }
        pos = absolute;
    }
    // Follow `(extends "parent")` if present.
    if let Some(parent) = extends_target(&block) {
        collect_pin_numbers(text, &parent, out, depth + 1);
    }
    true
}

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

fn extends_target(block: &str) -> Option<String> {
    let key = "(extends \"";
    let start = block.find(key)?;
    let after = &block[start + key.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Return each pin number in the named symbol together with the KiCad
/// electrical types carried by the pin(s) bearing that number, following
/// `(extends ...)` chains. `None` means the library or symbol could not be
/// located — the caller skips the check, exactly as for pin numbers.
///
/// A number can map to several types: KiCad stacks repeated grounds and
/// thermal pads under one number, marking one `power_in` and the rest
/// `passive`.
pub fn kicad_pin_types(lib_id: &str) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let text = std::fs::read_to_string(dir.join(format!("{lib_name}.kicad_sym"))).ok()?;
    let mut out = BTreeMap::new();
    if collect_pin_types(&text, sym_name, &mut out, 0) && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

fn collect_pin_types(
    text: &str,
    sym_name: &str,
    out: &mut BTreeMap<String, BTreeSet<String>>,
    depth: u32,
) -> bool {
    if depth > 4 {
        return false;
    }
    let Some(block) = extract_symbol(text, sym_name) else {
        return false;
    };
    // `(pin <electrical_type> <graphic_style> ... (number "N" ...))`. Taking
    // the whole pin by paren balance keeps the number with its own type.
    for pin in pin_blocks(&block) {
        let after = &pin["(pin ".len()..];
        let Some(kind_end) = after.find(char::is_whitespace) else {
            continue;
        };
        if let Some(number) = first_quoted_after(&pin, "(number ") {
            out.entry(number)
                .or_default()
                .insert(after[..kind_end].to_string());
        }
    }
    if let Some(parent) = extends_target(&block) {
        collect_pin_types(text, &parent, out, depth + 1);
    }
    true
}

fn first_quoted_after(block: &str, key: &str) -> Option<String> {
    let at = block.find(key)? + key.len();
    let open = block[at..].find('"')? + at + 1;
    let close = block[open..].find('"')? + open;
    Some(block[open..close].to_string())
}

/// Whether a registry pin's declared role can sit on a symbol pin carrying
/// `symbol_types`.
///
/// Only the two contradictions worth blocking a load for:
///
/// * a supply or ground pin landing on a symbol pin that drives or reads a
///   signal — the rail meets an output;
/// * a signal pin landing on a symbol supply pin — the driver meets a rail.
///
/// `passive`, `unspecified` and `free` agree with anything, because KiCad uses
/// them for stacked grounds, exposed pads and crystal terminals alike. A pin
/// the part declares `do_not_connect` is asserting that the pad stays
/// floating, which no symbol type contradicts. And a pin the part declares
/// `passive` is neither a rail nor a driver, so it is neither side of either
/// contradiction: `lmr33640` declares its internal-LDO `VCC` passive on
/// purpose, so that nothing treats a bias pin as a rail to load.
#[must_use]
pub fn roles_agree(declared: ElectricalType, symbol_types: &BTreeSet<String>) -> bool {
    const SYMBOL_SUPPLY: [&str; 2] = ["power_in", "power_out"];
    const AGREES_WITH_ANYTHING: [&str; 3] = ["passive", "unspecified", "free"];

    if matches!(
        declared,
        ElectricalType::DoNotConnect | ElectricalType::Passive
    ) {
        return true;
    }
    if symbol_types
        .iter()
        .any(|t| AGREES_WITH_ANYTHING.contains(&t.as_str()))
    {
        return true;
    }
    let symbol_is_supply = symbol_types
        .iter()
        .any(|t| SYMBOL_SUPPLY.contains(&t.as_str()));
    if declared.is_power() {
        symbol_is_supply
    } else {
        !symbol_is_supply
    }
}

/// Return each pin number together with the pin name(s) the symbol prints
/// there. Used to say *which* signal a mis-numbered pad actually carries,
/// which is the one fact that makes the diagnostic actionable.
pub fn kicad_pin_names(lib_id: &str) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let (lib_name, sym_name) = lib_id.split_once(':')?;
    let dir = bundled_dir()?;
    let text = std::fs::read_to_string(dir.join(format!("{lib_name}.kicad_sym"))).ok()?;
    let mut out = BTreeMap::new();
    if collect_pin_names(&text, sym_name, &mut out, 0) && !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

fn collect_pin_names(
    text: &str,
    sym_name: &str,
    out: &mut BTreeMap<String, BTreeSet<String>>,
    depth: u32,
) -> bool {
    if depth > 4 {
        return false;
    }
    let Some(block) = extract_symbol(text, sym_name) else {
        return false;
    };
    for pin in pin_blocks(&block) {
        if let (Some(number), Some(name)) = (
            first_quoted_after(&pin, "(number "),
            first_quoted_after(&pin, "(name "),
        ) {
            out.entry(number).or_default().insert(name);
        }
    }
    if let Some(parent) = extends_target(&block) {
        collect_pin_names(text, &parent, out, depth + 1);
    }
    true
}

/// Split a symbol block into its `(pin ...)` sub-blocks by paren balance.
fn pin_blocks(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search = 0;
    while let Some(idx) = block[search..].find("(pin ") {
        let start = search + idx;
        let mut depth = 0_i32;
        let mut end = start;
        for (i, b) in block.as_bytes().iter().enumerate().skip(start) {
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
        if end <= start {
            break;
        }
        out.push(block[start..end].to_string());
        search = start + "(pin ".len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_types_are_read_with_their_own_numbers() {
        let text = r#"(kicad_symbol_lib
    (symbol "Thing"
        (symbol "Thing_1_1"
            (pin power_in line (at 0 3.81 270) (length 2.794)
                (name "VDD" (effects (font (size 1.27 1.27))))
                (number "28" (effects (font (size 1.27 1.27))))
            )
            (pin output line (at 0 -3.81 90) (length 2.794)
                (name "TXN" (effects (font (size 1.27 1.27))))
                (number "1" (effects (font (size 1.27 1.27))))
            )
        )
    )
)"#;
        let mut out = BTreeMap::new();
        assert!(collect_pin_types(text, "Thing", &mut out, 0));
        assert_eq!(out["28"], BTreeSet::from(["power_in".to_string()]));
        assert_eq!(out["1"], BTreeSet::from(["output".to_string()]));
    }

    #[test]
    fn a_rail_declared_on_a_signal_pad_does_not_agree() {
        // The w5500 case: `vcc` sat on pad 1, which the symbol drives as TXN.
        let symbol = BTreeSet::from(["output".to_string()]);
        assert!(!roles_agree(ElectricalType::PowerInput, &symbol));
        assert!(!roles_agree(ElectricalType::GroundReference, &symbol));
        // and the converse: a signal pin on a supply pad.
        let supply = BTreeSet::from(["power_in".to_string()]);
        assert!(!roles_agree(ElectricalType::Input, &supply));
        assert!(!roles_agree(ElectricalType::Bidirectional, &supply));
    }

    #[test]
    fn roles_that_do_agree_are_left_alone() {
        let supply = BTreeSet::from(["power_in".to_string()]);
        assert!(roles_agree(ElectricalType::PowerInput, &supply));

        let signal = BTreeSet::from(["bidirectional".to_string()]);
        assert!(roles_agree(ElectricalType::Bidirectional, &signal));

        // KiCad stacks repeated grounds and thermal pads as `passive`, and
        // uses it for crystal terminals too, so it agrees with anything.
        let passive = BTreeSet::from(["passive".to_string()]);
        assert!(roles_agree(ElectricalType::PowerInput, &passive));
        assert!(roles_agree(ElectricalType::Output, &passive));

        // A number carrying several types agrees if any of them does.
        let stacked = BTreeSet::from(["power_in".to_string(), "passive".to_string()]);
        assert!(roles_agree(ElectricalType::PowerInput, &stacked));
    }

    #[test]
    fn a_passive_pin_agrees_with_every_symbol_type() {
        // Passive claims to be neither a rail nor a driver, so a symbol
        // supply pin under it is not a rail meeting a signal.
        for kind in ["power_in", "power_out", "output", "input"] {
            let symbol = BTreeSet::from([kind.to_string()]);
            assert!(
                roles_agree(ElectricalType::Passive, &symbol),
                "passive should agree with {kind}"
            );
        }
    }

    #[test]
    fn a_do_not_connect_pin_agrees_with_every_symbol_type() {
        // The part is asserting the pad stays floating. Nothing the symbol
        // says about it contradicts that.
        for kind in ["power_in", "output", "input", "no_connect"] {
            let symbol = BTreeSet::from([kind.to_string()]);
            assert!(
                roles_agree(ElectricalType::DoNotConnect, &symbol),
                "do_not_connect should agree with {kind}"
            );
        }
    }

    #[test]
    fn extract_pins_from_simple_block() {
        let text = r#"(kicad_symbol_lib
    (symbol "R"
        (symbol "R_1_1"
            (pin passive line (at 0 3.81 270) (length 2.794)
                (name "~" (effects (font (size 1.27 1.27))))
                (number "1" (effects (font (size 1.27 1.27))))
            )
            (pin passive line (at 0 -3.81 90) (length 2.794)
                (name "~" (effects (font (size 1.27 1.27))))
                (number "2" (effects (font (size 1.27 1.27))))
            )
        )
    )
)"#;
        let mut out = BTreeSet::new();
        assert!(collect_pin_numbers(text, "R", &mut out, 0));
        assert_eq!(out.iter().cloned().collect::<Vec<_>>(), vec!["1", "2"]);
    }

    #[test]
    fn extracted_set_follows_extends() {
        let text = r#"(kicad_symbol_lib
    (symbol "Base"
        (symbol "Base_1_1"
            (pin passive line (at 0 3.81 270)
                (number "1" (effects (font (size 1.27 1.27))))
            )
        )
    )
    (symbol "Derived"
        (extends "Base")
        (property "Reference" "U")
    )
)"#;
        let mut out = BTreeSet::new();
        assert!(collect_pin_numbers(text, "Derived", &mut out, 0));
        assert_eq!(out.iter().cloned().collect::<Vec<_>>(), vec!["1"]);
    }
}
