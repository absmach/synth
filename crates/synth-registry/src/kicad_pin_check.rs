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

use std::collections::BTreeSet;
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

#[cfg(test)]
mod tests {
    use super::*;

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
