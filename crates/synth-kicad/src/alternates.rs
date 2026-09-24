// SPDX-License-Identifier: Apache-2.0

//! Pin functions, shown through KiCad **pin alternates**.
//!
//! A part's pins are named for their package position (`PB6`, `GP0`),
//! but a schematic reads far better when the pin shows the function the
//! design actually drives through it (`I2C1_SCL`). KiCad models this
//! with pin *alternates*: the library symbol declares the possible
//! function names on a pin, and the placed instance selects one.
//!
//! The function in use is derived from the **net name** connected to the
//! pin — the same vocabulary the pin-mux ERC rules use
//! ([`synth_registry::PinCapability::from_net_name`]) — and only applied
//! when the pin's registry capabilities actually include that function.
//! A pin whose net does not name a function, or that cannot carry the
//! named one, keeps its package name (the mux rule reports the
//! mismatch separately).
//!
//! Two halves must agree:
//! - [`part_pin_alternates`] is the union of every function name used on
//!   a part's pins anywhere in the design, which the library symbol must
//!   declare ([`inject_alternates`] adds them to an embedded stock
//!   symbol).
//! - [`pin_function_alternates`] is the per-instance selection, whose
//!   names are a subset of that union by construction.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use synth_ir::{Board, Component, PinId};
use synth_registry::PinCapability;

/// Per-pin function names used anywhere in the design, keyed by part id
/// then pin number. The library symbol for each part must declare every
/// name in its set.
pub(crate) fn part_pin_alternates(
    board: &Board,
) -> BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let per_part = out.entry(part.id.0.clone()).or_default();
        for (number, name) in pin_function_alternates(board, component) {
            per_part.entry(number).or_default().insert(name);
        }
    }
    out
}

/// The function name to display on each connected pin of `component`,
/// keyed by pin number. Empty for a pin with no net, a net whose name
/// names no function, or a function the pin cannot carry.
pub(crate) fn pin_function_alternates(
    board: &Board,
    component: &Component,
) -> BTreeMap<String, String> {
    let Some(part) = component.part.as_ref() else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (idx, pin) in part.pins.iter().enumerate() {
        let Some((net_id, _)) = board
            .nets_containing(component.id, PinId(idx as u32))
            .next()
        else {
            continue;
        };
        let Some(net) = board.net(net_id) else {
            continue;
        };
        let Some(func) = PinCapability::from_net_name(&net.name) else {
            continue;
        };
        if !pin.capabilities.contains(&func) {
            continue;
        }
        out.insert(pin.number.0.clone(), alternate_name_for(&net.name, func));
    }
    out
}

/// Prefer the design's own function name (`I2C1_SCL`) when it is a legal
/// KiCad pin name; otherwise fall back to the canonical capability name
/// (`I2C_SCL`) so a bus member like `I2C0.sda` still renders cleanly.
fn alternate_name_for(net_name: &str, func: PinCapability) -> String {
    if is_kicad_identifier(net_name) {
        net_name.to_string()
    } else {
        func.canonical_name().to_string()
    }
}

fn is_kicad_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '.'))
}

/// Insert `(alternate "NAME" <type> line)` into the matching `(pin …)`
/// blocks of a KiCad library symbol's raw text.
///
/// Only pins named in `alts` are touched, and only when the alternate is
/// not already declared, so a symbol that needs no new function keeps
/// byte-identical text (no snapshot churn). KiCad's pin parser accepts
/// `alternate` anywhere inside the pin block, so the insertion is made
/// right after the pin's `(number …)` sub-block.
pub(crate) fn inject_alternates(text: &str, alts: &BTreeMap<String, BTreeSet<String>>) -> String {
    if alts.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 128);
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find("(pin ") {
        let start = cursor + rel;
        let Some(end) = matching_paren(text, start) else {
            break;
        };
        out.push_str(&text[cursor..start]);
        out.push_str(&rewrite_pin_block(&text[start..=end], alts));
        cursor = end + 1;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Append the missing alternates to one `(pin …)` block.
fn rewrite_pin_block(block: &str, alts: &BTreeMap<String, BTreeSet<String>>) -> String {
    let Some(number) = quoted_after(block, "(number \"") else {
        return block.to_string();
    };
    let Some(names) = alts.get(&number) else {
        return block.to_string();
    };
    // The pin's own electrical type (the token after `(pin `) is the
    // alternate's type — an alternate never changes how the pin drives.
    let Some(kind) = block
        .strip_prefix("(pin ")
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return block.to_string();
    };
    // Find the end of the `(number …)` sub-block to insert after.
    let Some(num_start) = block.find("(number \"") else {
        return block.to_string();
    };
    let Some(num_end) = matching_paren(block, num_start) else {
        return block.to_string();
    };
    let indent = line_indent(block, num_start);
    let mut insert = String::new();
    for name in names {
        if block.contains(&format!("(alternate \"{name}\"")) {
            continue;
        }
        insert.push('\n');
        insert.push_str(indent);
        let _ = write!(insert, "(alternate \"{name}\" {kind} line)");
    }
    if insert.is_empty() {
        return block.to_string();
    }
    let mut out = String::with_capacity(block.len() + insert.len());
    out.push_str(&block[..=num_end]);
    out.push_str(&insert);
    out.push_str(&block[num_end + 1..]);
    out
}

/// Index of the `)` matching the `(` at `open`, ignoring parentheses
/// inside double-quoted strings.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    for (offset, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'"' => in_string = !in_string,
            b'(' if !in_string => depth += 1,
            b')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn quoted_after(text: &str, key: &str) -> Option<String> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Leading whitespace of the line `text[at..]` sits on.
fn line_indent(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start().len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternate_name_prefers_the_design_function() {
        assert_eq!(
            alternate_name_for("I2C1_SCL", PinCapability::I2cScl),
            "I2C1_SCL"
        );
        assert_eq!(
            alternate_name_for("I2C0.sda", PinCapability::I2cSda),
            "I2C0.sda"
        );
        assert_eq!(
            alternate_name_for("I2C0/sda", PinCapability::I2cSda),
            "I2C_SDA"
        );
    }

    #[test]
    fn injects_alternate_after_the_number_block() {
        let text = "\t(symbol \"X\"\n\t\t(pin bidirectional line (at 0 0 0) (length 2.54)\n\t\t\t(name \"PB6\" (effects (font (size 1.27 1.27))))\n\t\t\t(number \"42\" (effects (font (size 1.27 1.27))))\n\t\t)\n\t)\n";
        let mut alts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        alts.entry("42".to_string())
            .or_default()
            .insert("I2C1_SCL".to_string());
        let out = inject_alternates(text, &alts);
        assert!(
            out.contains("(alternate \"I2C1_SCL\" bidirectional line)"),
            "{out}"
        );
        // Idempotent: a second pass adds nothing.
        assert_eq!(inject_alternates(&out, &alts), out);
    }

    #[test]
    fn leaves_untouched_pins_byte_identical() {
        let text = "(pin passive line (at 0 0 0) (length 1) (name \"1\") (number \"1\"))";
        let mut alts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        alts.entry("9".to_string())
            .or_default()
            .insert("NOPE".to_string());
        assert_eq!(inject_alternates(text, &alts), text);
        assert_eq!(inject_alternates(text, &BTreeMap::new()), text);
    }

    #[test]
    fn skips_an_alternate_the_symbol_already_declares() {
        let text = "(pin bidirectional line (at 0 0 0) (length 1) (name \"PB6\") (number \"42\") (alternate \"I2C1_SCL\" bidirectional line))";
        let mut alts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        alts.entry("42".to_string())
            .or_default()
            .insert("I2C1_SCL".to_string());
        assert_eq!(inject_alternates(text, &alts), text);
    }
}
