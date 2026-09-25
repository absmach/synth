// SPDX-License-Identifier: Apache-2.0

//! Net-class classification and the deterministic schematic palette
//! (schematic-quality plan Phase B1).
//!
//! The KiCad exporter writes these into `.kicad_pro`'s
//! `net_settings`, which is what makes KiCad colour wires *and*
//! labels by class automatically — the colour survives user edits and
//! shows up in the netlist UI, unlike per-wire `(stroke (color …))`.
//!
//! Classification reuses what the layouter already knows rather than
//! inventing a second vocabulary: [`synth_ir::infer_power_domains`]
//! decides Power/Ground topologically (so opaque `net_7` names still
//! classify correctly), and the pin-capability tokens behind
//! [`crate::pick_net_label`] decide the protocol classes (I²C, SPI,
//! UART, USB, Clock, Reset).
//!
//! The palette is fixed: the same class always gets the same hue on
//! every export, so `SCL` is amber in every design and a reader can
//! carry the convention between schematics.

use std::collections::BTreeMap;

use synth_ir::{Board, Net, NetId};
use synth_registry::PinCapability;

/// The nine semantic classes, in the order the exporter emits them.
/// `Default` is always last — KiCad assigns unlisted nets to it.
pub const NET_CLASSES: [&str; 9] = [
    "Power", "Ground", "I2C", "SPI", "UART", "USB", "Clock", "Reset", "Default",
];

/// Colour-blind-safe fixed palette (an Okabe–Ito-derived set): Power
/// vermillion, Ground charcoal, I²C blue, SPI bluish green, UART
/// reddish purple, USB sky blue, Clock orange, Reset red, Default
/// black. Deterministic per class name.
#[must_use]
pub fn net_class_color(class: &str) -> [u8; 3] {
    match class {
        "Power" => [0xD5, 0x5E, 0x00],
        "Ground" => [0x33, 0x33, 0x33],
        "I2C" => [0x00, 0x72, 0xB2],
        "SPI" => [0x00, 0x9E, 0x73],
        "UART" => [0xCC, 0x79, 0xA7],
        "USB" => [0x56, 0xB4, 0xE9],
        "Clock" => [0xE6, 0x9F, 0x00],
        "Reset" => [0xC0, 0x39, 0x2B],
        "Default" => [0x00, 0x00, 0x00],
        // Author-declared class with no fixed hue: hash into a
        // deterministic mid-tone so overflow still scales.
        other => hashed_color(other),
    }
}

/// Deterministic mid-tone for a class name outside the fixed palette:
/// FNV-1a → hue, fixed saturation/value, so names spread around the
/// wheel without ever colliding on a process-dependent hash.
#[must_use]
pub fn hashed_color(name: &str) -> [u8; 3] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // Hue 0..360; saturation 0.65, value 0.75.
    let hue = (hash % 360) as f64;
    hsv_to_rgb(hue, 0.65, 0.75)
}

/// HSV (hue in degrees, sat/val in 0..=1) to 8-bit RGB. Small and
/// exact enough for palette work; avoids pulling in a colour crate.
fn hsv_to_rgb(hue: f64, sat: f64, val: f64) -> [u8; 3] {
    let chroma = val * sat;
    let h_prime = hue / 60.0;
    let x = chroma * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = val - chroma;
    let to_u8 = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [to_u8(r1), to_u8(g1), to_u8(b1)]
}

/// One net's resolved class: the semantic class, or an author-declared
/// `netclass "…"` name when the net joined one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetClassAssignment {
    pub net: NetId,
    pub net_name: String,
    pub class: String,
    /// Fixed palette hue for a semantic class, hashed for a declared
    /// class without a `color` attribute.
    pub color: [u8; 3],
}

/// The semantic class of a single net, from its topology and pin
/// capabilities.
///
/// Power/Ground come from [`crate::classify_power_net`] — the very
/// rule the schematic power flags use — so a net drawn as a power
/// symbol is always coloured as Power/Ground, never as a signal.
/// Protocol classes come from the pin-capability vocabulary behind
/// [`crate::pick_net_label`].
#[must_use]
fn semantic_class(board: &Board, net: &Net) -> &'static str {
    // Topology first: a rail or ground is a rail or ground whatever
    // its name says.
    match crate::classify_power_net(board, net) {
        Some((crate::PowerFlagKind::Gnd, _)) => return "Ground",
        Some((crate::PowerFlagKind::Vcc, _)) => return "Power",
        None => {}
    }
    // Then the strongest protocol capability on any endpoint. A pin
    // can carry several; the first protocol hit wins, matching the
    // priority the label vocabulary uses.
    let mut found: Option<&'static str> = None;
    for ep in &net.endpoints {
        let Some(pin) = board.pin(ep.component, ep.pin) else {
            continue;
        };
        for cap in &pin.capabilities {
            let class = match cap {
                PinCapability::I2cSda | PinCapability::I2cScl => "I2C",
                PinCapability::SpiMosi
                | PinCapability::SpiMiso
                | PinCapability::SpiSck
                | PinCapability::SpiCs => "SPI",
                PinCapability::UartTx | PinCapability::UartRx => "UART",
                PinCapability::UsbDp
                | PinCapability::UsbDn
                | PinCapability::UsbVbus
                | PinCapability::UsbCc => "USB",
                PinCapability::ClockInput | PinCapability::ClockOutput => "Clock",
                PinCapability::Reset | PinCapability::BootMode => "Reset",
                _ => continue,
            };
            // First capability wins; do not let a later GPIO
            // downgrade an already-classified protocol net.
            found = Some(class);
            break;
        }
        if found.is_some() {
            break;
        }
    }
    found.unwrap_or("Default")
}

/// Classify every net in `board`, in ascending net order (deterministic).
///
/// An author-declared `netclass "…"` join wins over the semantic
/// class (the plan's precedence: one class per net, the author's
/// declaration first). Its colour comes from the declared class's
/// `color` attribute when present, else a hashed mid-tone.
#[must_use]
pub fn classify_nets(board: &Board) -> Vec<NetClassAssignment> {
    let mut declared_color: BTreeMap<&str, [u8; 3]> = BTreeMap::new();
    for nc in &board.netclasses {
        if let Some(rgb) = nc.color {
            declared_color.insert(nc.name.as_str(), rgb);
        }
    }
    let mut out: Vec<NetClassAssignment> = board
        .nets
        .iter()
        .map(|net| {
            let (class, color) = if let Some(name) = net.netclass.as_deref() {
                let color = declared_color
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| hashed_color(name));
                (name.to_string(), color)
            } else {
                let class = semantic_class(board, net);
                (class.to_string(), net_class_color(class))
            };
            NetClassAssignment {
                net: net.id,
                net_name: net.name.clone(),
                class,
                color,
            }
        })
        .collect();
    out.sort_by_key(|a| a.net);
    out
}

/// The full set of class names a board uses, fixed semantic classes
/// first (in [`NET_CLASSES`] order), then any author-declared class
/// names in declaration order. Deterministic; used to emit the
/// `.kicad_pro` `net_settings.classes[]` array.
#[must_use]
pub fn class_names(board: &Board, assignments: &[NetClassAssignment]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let used: std::collections::HashSet<&str> =
        assignments.iter().map(|a| a.class.as_str()).collect();
    for class in NET_CLASSES {
        if used.contains(class) && seen.insert(class.to_string()) {
            names.push(class.to_string());
        }
    }
    for a in assignments {
        if seen.insert(a.class.clone()) {
            names.push(a.class.clone());
        }
    }
    // Declared classes with no member net still deserve a row (the
    // declaration is design intent KiCad should show).
    for nc in &board.netclasses {
        if seen.insert(nc.name.clone()) {
            names.push(nc.name.clone());
        }
    }
    names
}

/// Colour of every net, keyed by id — the same hues
/// [`classify_nets`] resolves, in a form the schematic emitter can
/// look up per wire.
///
/// The exporter strokes each wire and label in its net's class hue
/// directly, *in addition to* writing `net_settings`. The project
/// block alone is not enough: KiCad applies a class colour by net
/// *name*, and a short local net that carries neither a power symbol
/// nor a label is auto-named (`Net-(U2-BOOT0)`) at load time, so no
/// assignment written ahead of time can reach it. An explicit stroke
/// reaches every net, including those.
#[must_use]
pub fn net_colors(board: &Board) -> BTreeMap<NetId, [u8; 3]> {
    classify_nets(board)
        .into_iter()
        .filter(|a| a.class != "Default")
        .map(|a| (a.net, a.color))
        .collect()
}

/// The names KiCad will know a net by, for `netclass_assignments`.
///
/// KiCad derives net names from the drawing, not from our IR, so the
/// IR name (`net_7`) never matches. The visible name comes from:
///
/// * a **power symbol** — a global net named by the symbol's value
///   (`+3V3`, `GND`, `VBUS`), with no sheet-path prefix; or
/// * a **local label** — the label text prefixed by the sheet path,
///   so `SDA` on the root sheet becomes `/SDA`.
///
/// Both spellings are returned for a labelled net (bare and
/// root-prefixed) because a net that is later moved onto a sub-sheet
/// keeps the bare form as a prefix match; deeper paths are covered by
/// the glob pattern the exporter emits alongside.
///
/// A net with neither a flag nor a label is auto-named by KiCad from
/// one of its pins and is deliberately absent here — [`net_colors`]
/// is what colours those.
#[must_use]
pub fn kicad_net_names(layout: &crate::Layout) -> BTreeMap<NetId, Vec<String>> {
    let mut out: BTreeMap<NetId, Vec<String>> = BTreeMap::new();
    // Power symbols win: a flagged net is global under the rail name
    // whatever else is drawn on it.
    for flag in &layout.power_flags {
        out.entry(flag.net).or_default().push(flag.label.clone());
    }
    for label in &layout.net_labels {
        if out.contains_key(&label.net) {
            continue;
        }
        out.entry(label.net)
            .or_insert_with(|| vec![format!("/{}", label.label), label.label.clone()]);
    }
    for names in out.values_mut() {
        names.sort();
        names.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_palette_is_deterministic_and_distinct() {
        assert_eq!(net_class_color("I2C"), net_class_color("I2C"));
        assert_ne!(net_class_color("I2C"), net_class_color("SPI"));
        assert_ne!(net_class_color("Power"), net_class_color("Ground"));
        // Every fixed class has a distinct hue.
        let mut seen = std::collections::HashSet::new();
        for class in NET_CLASSES {
            assert!(
                seen.insert(net_class_color(class)),
                "class {class} reuses a hue"
            );
        }
    }

    #[test]
    fn hashed_color_is_stable_across_calls() {
        assert_eq!(hashed_color("MY_CLASS"), hashed_color("MY_CLASS"));
        assert_ne!(hashed_color("A"), hashed_color("B"));
    }

    #[test]
    fn hsv_primaries_are_exact() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), [0, 0, 255]);
    }
}
