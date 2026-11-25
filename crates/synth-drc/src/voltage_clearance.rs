// SPDX-License-Identifier: Apache-2.0

//! Voltage-aware clearance (`E-SYNTH-DRC-025`).
//!
//! The flat profile minimum (`E-SYNTH-DRC-002`, `kicad-cli`'s clearance rule)
//! is the spacing a fab can etch. It says nothing about the spacing a voltage
//! difference needs. This rule gives every net a working voltage, looks up the
//! spacing the difference needs in the profile's table (IPC-2221B Table 6-1 by
//! default) and measures the routed board against it:
//!
//! - copper to copper of a different net, on every copper layer, including
//!   filled zones and inner layers;
//! - copper to drilled hole walls, on every layer the hole passes through. A
//!   hole with no net (a mounting hole, an NPTH) counts as chassis at 0 V;
//! - copper to the board edge, also taken as chassis at 0 V.
//!
//! A rail that reaches the board through a connector, with no fuse or eFuse
//! in the way, is unprotected and gets the profile's extra margin.
//!
//! The rule reports only where the voltage asks for more than the flat
//! minimum the other rules already enforce. A board with no voltage
//! information therefore gets no finding from it, and a low-voltage board
//! (3.3 V, 5 V USB) needs nothing past the fab floor.
//!
//! The geometry is read from the `.kicad_pcb`, because zone fills, inner
//! layers and hole walls exist only there. A zone that has not been filled
//! yet has no copper to measure; the exported `.kicad_dru` carries the same
//! spacing as KiCad rules ([`kicad_rules`]), so KiCad's own fill keeps it.

// Board coordinates are far below f64's mantissa; see `dfm.rs`.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use std::collections::BTreeMap;

use synth_geometry::nm_to_mm;
use synth_ir::Board;

use crate::pcb_read::{PcbBoard, Point};
use crate::{ManufacturerProfile, Violation};

/// Rule code.
pub const CODE: &str = "E-SYNTH-DRC-025";

/// Component kinds whose power pins bring a rail onto the board from outside.
const INPUT_KINDS: &[&str] = &["connector", "jack", "terminal_block", "terminal"];

/// Sample spacing along a pad outline, in nanometres.
const PAD_OUTLINE_STEP_NM: f64 = 25_000.0;

/// The working voltage of one net, as the rule uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetVoltage {
    /// Nominal voltage in millivolts.
    pub mv: i64,
    /// The rail enters from a connector with no fuse in the way.
    pub unprotected: bool,
}

/// Every net with a known working voltage, by name.
///
/// A netclass `voltage` wins over the inferred rail voltage. A net is
/// unprotected when its class says `unprotected`, or when it is a rail with
/// a power pin on a connector and its class does not say `protected`.
#[must_use]
pub fn net_voltages(board: &Board) -> BTreeMap<String, NetVoltage> {
    let domains = synth_ir::infer_power_domains(board);
    let mut out = BTreeMap::new();
    for net in &board.nets {
        let class = net
            .netclass
            .as_deref()
            .and_then(|name| board.netclasses.iter().find(|c| c.name == name));
        let declared = class.and_then(|c| c.voltage).map(synth_ir::Voltage::to_v);
        let inferred = domains
            .get(net.id)
            .and_then(synth_ir::PowerDomainKind::nominal_voltage);
        let Some(volts) = declared.or(inferred) else {
            continue;
        };
        let is_rail = volts.abs() > 0.0;
        let from_connector = net.endpoints.iter().any(|ep| {
            board.component(ep.component).is_some_and(|c| {
                INPUT_KINDS.contains(&c.kind.as_str())
                    && board.pin(ep.component, ep.pin).is_some_and(|p| {
                        matches!(
                            p.electrical_type,
                            synth_registry::ElectricalType::PowerInput
                                | synth_registry::ElectricalType::PowerOutput
                                | synth_registry::ElectricalType::Passive
                        )
                    })
            })
        });
        // A fuse splits the rail: the net between the connector and the fuse
        // touches the connector and stays unprotected, and the net past the
        // fuse does not touch it, so it is protected without saying so.
        let unprotected = match class.and_then(|c| c.protected) {
            Some(protected) => !protected,
            None => is_rail && from_connector,
        };
        out.insert(
            net.name.clone(),
            NetVoltage {
                mv: (volts * 1000.0).round() as i64,
                unprotected,
            },
        );
    }
    out
}

/// The spacing two conductors need, in nanometres: the table figure for
/// their difference, plus the unprotected margin when either is an
/// unprotected rail at or above the profile's threshold.
#[must_use]
pub fn required_nm(
    profile: &ManufacturerProfile,
    a: NetVoltage,
    b: NetVoltage,
    internal: bool,
) -> (i64, i64) {
    let rules = &profile.voltage_clearance;
    let table = rules.table_nm(a.mv - b.mv, internal);
    let hot = |v: NetVoltage| v.unprotected && v.mv.abs() >= rules.unprotected_min_mv;
    let margin = if hot(a) || hot(b) {
        rules.unprotected_margin_nm
    } else {
        0
    };
    (table + margin, margin)
}

/// The chassis: a hole with no net and the board edge.
const CHASSIS: NetVoltage = NetVoltage {
    mv: 0,
    unprotected: false,
};

/// What a measured item is, which picks the flat floor it is compared past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Copper,
    Hole,
    Edge,
}

impl Kind {
    fn floor_nm(self, profile: &ManufacturerProfile) -> i64 {
        match self {
            Self::Copper => profile.min_copper_clearance_nm,
            Self::Hole => profile.min_drill_to_copper_nm,
            Self::Edge => profile.min_copper_to_edge_nm,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Copper => "copper",
            Self::Hole => "the hole wall",
            Self::Edge => "the board edge",
        }
    }
}

/// One measured shape: straight edges swept by a radius.
struct Item {
    net: Option<String>,
    layer: String,
    kind: Kind,
    label: String,
    edges: Vec<(Point, Point)>,
    radius_nm: f64,
    bbox: (Point, Point),
}

fn bbox_of(edges: &[(Point, Point)], radius_nm: f64) -> (Point, Point) {
    let r = radius_nm.ceil() as i64;
    let mut min = Point::new(i64::MAX, i64::MAX);
    let mut max = Point::new(i64::MIN, i64::MIN);
    for (a, b) in edges {
        for p in [a, b] {
            min.x_nm = min.x_nm.min(p.x_nm - r);
            min.y_nm = min.y_nm.min(p.y_nm - r);
            max.x_nm = max.x_nm.max(p.x_nm + r);
            max.y_nm = max.y_nm.max(p.y_nm + r);
        }
    }
    (min, max)
}

fn closed(points: &[Point]) -> Vec<(Point, Point)> {
    (0..points.len())
        .map(|i| (points[i], points[(i + 1) % points.len()]))
        .collect()
}

fn item(
    net: Option<String>,
    layer: &str,
    kind: Kind,
    label: String,
    edges: Vec<(Point, Point)>,
    radius_nm: f64,
) -> Item {
    let bbox = bbox_of(&edges, radius_nm);
    Item {
        net,
        layer: layer.to_string(),
        kind,
        label,
        edges,
        radius_nm,
        bbox,
    }
}

fn copper_layers(pcb: &PcbBoard) -> Vec<String> {
    let mut layers: Vec<String> = pcb
        .layers
        .iter()
        .filter(|l| l.ends_with(".Cu") && !l.starts_with('*'))
        .cloned()
        .collect();
    if layers.is_empty() {
        layers = vec!["F.Cu".to_string(), "B.Cu".to_string()];
    }
    layers
}

fn is_internal(layer: &str) -> bool {
    layer.starts_with("In")
}

#[allow(clippy::too_many_lines)]
fn collect(pcb: &PcbBoard) -> Vec<Item> {
    let layers = copper_layers(pcb);
    // The net table first: the reader fills `net_name` with the code's
    // digits when an item writes only `(net 3)`.
    let name = |code: Option<u32>, named: Option<&String>| -> Option<String> {
        code.and_then(|c| pcb.nets.get(&c).cloned())
            .or_else(|| named.cloned())
            .filter(|n| !n.is_empty())
    };
    let mut items = Vec::new();
    for s in pcb.segments.iter().chain(&pcb.arc_chords) {
        items.push(item(
            name(s.net, s.net_name.as_ref()),
            &s.layer,
            Kind::Copper,
            "track".to_string(),
            vec![(s.start, s.end)],
            s.width_nm as f64 / 2.0,
        ));
    }
    for v in &pcb.vias {
        for layer in v.layers.iter().filter(|l| layers.contains(l)) {
            items.push(item(
                name(v.net, v.net_name.as_ref()),
                layer,
                Kind::Copper,
                "via".to_string(),
                vec![(v.at, v.at)],
                v.size_nm as f64 / 2.0,
            ));
        }
        // A via's barrel is a hole through every layer it spans.
    }
    for fp in &pcb.footprints {
        for pad in &fp.pads {
            let net = name(pad.net, pad.net_name.as_ref());
            let outline = pad.outline_points_nm(PAD_OUTLINE_STEP_NM);
            for layer in layers.iter().filter(|l| pad.copper_on(l)) {
                items.push(item(
                    net.clone(),
                    layer,
                    Kind::Copper,
                    format!("pad {}", pad.id()),
                    closed(&outline),
                    0.0,
                ));
            }
            // A plated hole with a net is measured by its pad's copper. One
            // with no net (an NPTH, a mounting hole) is chassis, and its wall
            // runs through every layer, inner ones included.
            let netless = pad.kind == "np_thru_hole" || net.is_none();
            if let Some(hole) = pad.hole().filter(|_| netless) {
                for layer in &layers {
                    items.push(item(
                        None,
                        layer,
                        Kind::Hole,
                        format!("hole of {}", pad.id()),
                        vec![(hole.a, hole.b)],
                        hole.radius_nm,
                    ));
                }
            }
        }
    }
    for zone in &pcb.zones {
        let net = name(zone.net, zone.net_name.as_ref());
        for (layer, polygons) in &zone.fills {
            for polygon in polygons.iter().filter(|p| p.len() >= 3) {
                items.push(item(
                    net.clone(),
                    layer,
                    Kind::Copper,
                    "zone fill".to_string(),
                    closed(polygon),
                    0.0,
                ));
            }
        }
    }
    if !pcb.edge_cuts.is_empty() {
        for layer in &layers {
            items.push(item(
                None,
                layer,
                Kind::Edge,
                "board edge".to_string(),
                pcb.edge_cuts.clone(),
                0.0,
            ));
        }
    }
    items
}

/// Distance between two segments and the midpoint of their closest points.
fn segment_gap(a: (Point, Point), b: (Point, Point)) -> (f64, Point) {
    let candidates = [
        (a.0, closest(b, a.0)),
        (a.1, closest(b, a.1)),
        (closest(a, b.0), b.0),
        (closest(a, b.1), b.1),
    ];
    let mut best = (f64::INFINITY, a.0);
    for (p, q) in candidates {
        let d = ((p.x_nm - q.x_nm) as f64).hypot((p.y_nm - q.y_nm) as f64);
        if d < best.0 {
            best = (
                d,
                Point::new(i64::midpoint(p.x_nm, q.x_nm), i64::midpoint(p.y_nm, q.y_nm)),
            );
        }
    }
    if segments_cross(a, b) {
        return (0.0, best.1);
    }
    best
}

fn closest(s: (Point, Point), p: Point) -> Point {
    let (ax, ay) = (s.0.x_nm as f64, s.0.y_nm as f64);
    let (dx, dy) = ((s.1.x_nm - s.0.x_nm) as f64, (s.1.y_nm - s.0.y_nm) as f64);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x_nm as f64 - ax) * dx + (p.y_nm as f64 - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    Point::new((ax + t * dx).round() as i64, (ay + t * dy).round() as i64)
}

fn segments_cross(a: (Point, Point), b: (Point, Point)) -> bool {
    let orient = |p: Point, q: Point, r: Point| -> i128 {
        i128::from(q.x_nm - p.x_nm) * i128::from(r.y_nm - p.y_nm)
            - i128::from(q.y_nm - p.y_nm) * i128::from(r.x_nm - p.x_nm)
    };
    let d1 = orient(a.0, a.1, b.0);
    let d2 = orient(a.0, a.1, b.1);
    let d3 = orient(b.0, b.1, a.0);
    let d4 = orient(b.0, b.1, a.1);
    (d1 > 0) != (d2 > 0) && (d3 > 0) != (d4 > 0) && d1 != 0 && d2 != 0 && d3 != 0 && d4 != 0
}

fn boxes_apart(a: (Point, Point), b: (Point, Point), by: i64) -> bool {
    a.1.x_nm + by < b.0.x_nm
        || b.1.x_nm + by < a.0.x_nm
        || a.1.y_nm + by < b.0.y_nm
        || b.1.y_nm + by < a.0.y_nm
}

/// Smallest gap between two items, and where, when it is under `limit_nm`.
fn gap(a: &Item, b: &Item, limit_nm: i64) -> Option<(f64, Point)> {
    if boxes_apart(a.bbox, b.bbox, limit_nm) {
        return None;
    }
    let pad = limit_nm + (a.radius_nm + b.radius_nm).ceil() as i64;
    let mut best: Option<(f64, Point)> = None;
    for ea in &a.edges {
        let ba = bbox_of(std::slice::from_ref(ea), 0.0);
        if boxes_apart(ba, b.bbox, pad) {
            continue;
        }
        for eb in &b.edges {
            let bb = bbox_of(std::slice::from_ref(eb), 0.0);
            if boxes_apart(ba, bb, pad) {
                continue;
            }
            let (d, at) = segment_gap(*ea, *eb);
            let d = (d - a.radius_nm - b.radius_nm).max(0.0);
            if best.is_none_or(|(known, _)| d < known) {
                best = Some((d, at));
            }
        }
    }
    best.filter(|(d, _)| *d < limit_nm as f64)
}

fn show_v(v: NetVoltage) -> String {
    let volts = v.mv as f64 / 1000.0;
    let s = format!("{volts:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if v.unprotected {
        format!("{s} V, unprotected")
    } else {
        format!("{s} V")
    }
}

/// `(hot net, other net or kind label, layer)`.
type Key = (String, String, String);
/// The closest gap found for a key: distance, where, required spacing, the
/// margin part of it, and the two items.
type Worst = (f64, Point, i64, i64, usize, usize);

/// Measure the routed board against the voltage-derived spacing.
///
/// One violation per pair of nets (or net and hole/edge) per layer, at the
/// closest point found.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_voltage_clearance(
    board: &Board,
    pcb: &PcbBoard,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    let voltages = net_voltages(board);
    if voltages.is_empty() {
        return Vec::new();
    }
    let voltage_of = |item: &Item| -> Option<NetVoltage> {
        match &item.net {
            Some(n) => voltages.get(n).copied(),
            None => Some(CHASSIS),
        }
    };
    let items = collect(pcb);
    // An item is hot when, against chassis, its voltage needs more than some
    // floor. Only hot items start a comparison, which keeps the pairs few.
    let lowest_floor = [Kind::Copper, Kind::Hole, Kind::Edge]
        .iter()
        .map(|k| k.floor_nm(profile))
        .min()
        .unwrap_or(0);
    let hot_nets: std::collections::BTreeSet<&str> = voltages
        .iter()
        .filter(|(_, v)| {
            [false, true].into_iter().any(|internal| {
                std::iter::once(CHASSIS)
                    .chain(voltages.values().copied())
                    .any(|o| required_nm(profile, **v, o, internal).0 > lowest_floor)
            })
        })
        .map(|(name, _)| name.as_str())
        .collect();
    let hot: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            it.kind == Kind::Copper && it.net.as_deref().is_some_and(|n| hot_nets.contains(n))
        })
        .map(|(i, _)| i)
        .collect();
    let net_ids: BTreeMap<&str, synth_ir::NetId> =
        board.nets.iter().map(|n| (n.name.as_str(), n.id)).collect();
    let mut worst: BTreeMap<Key, Worst> = BTreeMap::new();
    for &i in &hot {
        let a = &items[i];
        let Some(va) = voltage_of(a) else { continue };
        let internal = is_internal(&a.layer);
        for (j, b) in items.iter().enumerate() {
            if i == j || b.layer != a.layer || b.net == a.net {
                continue;
            }
            // An unknown net is measured as 0 V: the conservative difference
            // against a known rail.
            let vb = voltage_of(b).unwrap_or(CHASSIS);
            // Two hot copper items are measured once, from the lower index.
            if b.kind == Kind::Copper && hot.binary_search(&j).is_ok() && j < i {
                continue;
            }
            let (required, margin) = required_nm(profile, va, vb, internal);
            if required <= b.kind.floor_nm(profile) {
                continue;
            }
            let Some((d, at)) = gap(a, b, required) else {
                continue;
            };
            let other = match (&b.net, b.kind) {
                (Some(n), _) => n.clone(),
                (None, Kind::Edge) => "<edge>".to_string(),
                (None, _) => format!("<{}>", b.label),
            };
            let key = (a.net.clone().unwrap_or_default(), other, a.layer.clone());
            if worst.get(&key).is_none_or(|known| d < known.0) {
                worst.insert(key, (d, at, required, margin, i, j));
            }
        }
    }
    worst
        .into_iter()
        .map(|((net_a, _other, layer), (d, at, required, margin, i, j))| {
            let a = &items[i];
            let b = &items[j];
            let va = voltage_of(a).unwrap_or(CHASSIS);
            let vb = voltage_of(b).unwrap_or(CHASSIS);
            let internal = is_internal(&layer);
            let table = required - margin;
            let column = if internal {
                "B1 internal"
            } else if profile.voltage_clearance.coating == crate::Coating::Polymer {
                "B4 coated external"
            } else {
                "B2 external"
            };
            let other = match (&b.net, b.kind) {
                (Some(n), Kind::Copper) => format!(
                    "net `{n}` ({}, {})",
                    voltages.get(n).map_or_else(|| "voltage unknown, taken as 0 V".to_string(), |v| show_v(*v)),
                    b.label
                ),
                (Some(n), _) => format!("{} of net `{n}` ({})", b.kind.noun(), show_v(vb)),
                (None, Kind::Edge) => "the board edge (chassis, 0 V)".to_string(),
                (None, _) => format!("{} (no net, chassis 0 V)", b.label),
            };
            let margin_text = if margin > 0 {
                format!(" + {:.2} mm unprotected-rail margin", nm_to_mm(margin))
            } else {
                String::new()
            };
            let mut nets = Vec::new();
            nets.extend(net_ids.get(net_a.as_str()).copied());
            if let Some(n) = &b.net {
                nets.extend(net_ids.get(n.as_str()).copied());
            }
            Violation {
                code: CODE.to_string(),
                message: format!(
                    "net `{net_a}` ({}, {}) is {:.3} mm from {other} on {layer} at ({:.3}, {:.3}) mm; \
                     a {:.1} V difference needs {:.3} mm ({:.3} mm IPC-2221B {column}{margin_text})",
                    show_v(va),
                    a.label,
                    d / 1e6,
                    at.x_mm(),
                    at.y_mm(),
                    (va.mv - vb.mv).abs() as f64 / 1000.0,
                    nm_to_mm(required),
                    nm_to_mm(table),
                ),
                witness: vec![synth_geometry::Point::new(at.x_nm, at.y_nm)],
                nets,
                components: Vec::new(),
                pos_mm: Some((at.x_mm(), at.y_mm())),
                suggested_override: None,
            }
        })
        .collect()
}

/// KiCad custom rules for the spacing each voltage net needs against chassis
/// and against a 0 V conductor, outer and inner layers separately. Only nets
/// whose requirement is past the flat floors get a rule, so a board with no
/// such net exports exactly as before.
#[must_use]
pub fn kicad_rules(board: &Board, profile: &ManufacturerProfile) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (name, v) in net_voltages(board) {
        let safe = name.replace('\'', "");
        for (layer, internal) in [("outer", false), ("inner", true)] {
            let (required, _) = required_nm(profile, v, CHASSIS, internal);
            let mm = nm_to_mm(required);
            if required > profile.min_copper_clearance_nm {
                let _ = writeln!(
                    out,
                    "(rule \"voltage {safe} clearance {layer}\"\n  (layer {layer})\n  (constraint clearance (min {mm}mm))\n  (condition \"A.NetName == '{safe}' && A.NetName != B.NetName\"))"
                );
            }
            if required > profile.min_drill_to_copper_nm {
                let _ = writeln!(
                    out,
                    "(rule \"voltage {safe} hole {layer}\"\n  (layer {layer})\n  (constraint hole_clearance (min {mm}mm))\n  (condition \"A.NetName == '{safe}'\"))"
                );
            }
            if required > profile.min_copper_to_edge_nm {
                let _ = writeln!(
                    out,
                    "(rule \"voltage {safe} edge {layer}\"\n  (layer {layer})\n  (constraint edge_clearance (min {mm}mm))\n  (condition \"A.NetName == '{safe}'\"))"
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTOR: &str = r#"
id = "t_dc_jack"
kind = "connector"

[[pins]]
name = "vin"
number = "1"
electrical_type = "power_output"

[[pins]]
name = "gnd"
number = "2"
electrical_type = "ground_reference"
"#;

    const LOAD: &str = r#"
id = "t_load"
kind = "ic"

[[pins]]
name = "vin"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "gnd"
number = "2"
electrical_type = "ground_reference"
"#;

    fn board(class: &str) -> Board {
        let registry = synth_registry::load_from_strs(&[
            ("t_dc_jack.synth.toml", CONNECTOR),
            ("t_load.synth.toml", LOAD),
        ])
        .expect("test parts must load");
        let src = format!(
            r#"board "v" {{
  layers 4
  {class}
  component J1: connector "t_dc_jack"
  component U1: ic "t_load"
  power "VIN" 12v {{ J1.vin U1.vin }}
  net "GND" {{ J1.gnd U1.gnd }}
}}"#
        );
        let parse = synth_parser::parse(&src, "v.synth".to_string());
        assert!(
            parse.diagnostics.iter().all(|d| !d.severity.is_blocking()),
            "{:?}",
            parse.diagnostics
        );
        synth_ir::lower(parse.ast.as_ref().expect("ast"), &registry, "v.synth")
            .board
            .expect("board")
    }

    /// A 12 V through-hole pad (2 mm round, 1 mm drill) at (10, 10), an
    /// inner-layer GND fill whose left side is `gap_mm` from the pad's copper,
    /// and an optional mounting hole.
    fn pcb(gap_mm: f64, extra: &str) -> PcbBoard {
        let x0 = 11.0 + gap_mm;
        let text = format!(
            r#"(kicad_pcb
  (version 20260206)
  (generator "test")
  (layers (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "In2.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "VIN")
  (net 2 "GND")
  (footprint "Conn:Jack"
    (layer "F.Cu")
    (at 10 10)
    (property "Reference" "J1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "VIN"))
  )
  {extra}
  (zone (net 2) (net_name "GND") (layer "In1.Cu")
    (polygon (pts (xy {x0} 5) (xy 30 5) (xy 30 15) (xy {x0} 15)))
    (filled_polygon (layer "In1.Cu") (pts (xy {x0} 5) (xy 30 5) (xy 30 15) (xy {x0} 15))))
  (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 0) (end 40 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 30) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#
        );
        PcbBoard::parse(&text).expect("board parses")
    }

    fn run(class: &str, gap_mm: f64, extra: &str) -> Vec<Violation> {
        check_voltage_clearance(
            &board(class),
            &pcb(gap_mm, extra),
            &ManufacturerProfile::jlc_standard(),
        )
    }

    #[test]
    fn ipc_table_rows() {
        let rules = crate::VoltageClearanceRules::default();
        assert_eq!(rules.table_nm(12_000, true), 50_000);
        assert_eq!(rules.table_nm(12_000, false), 100_000);
        assert_eq!(rules.table_nm(48_000, false), 600_000);
        assert_eq!(rules.table_nm(400_000, true), 250_000);
        // Past 500 V: 0.005 mm/V external, so 1000 V needs 5 mm.
        assert_eq!(rules.table_nm(1_000_000, false), 5_000_000);
    }

    #[test]
    fn a_connector_rail_is_unprotected_and_ground_is_not() {
        let v = net_voltages(&board(""));
        assert_eq!(
            v["VIN"],
            NetVoltage {
                mv: 12_000,
                unprotected: true
            }
        );
        assert_eq!(v["GND"].mv, 0);
        assert!(!v["GND"].unprotected);
    }

    /// The acceptance case: a 12 V connector rail with an inner GND pour
    /// 0.1 mm away. B1 gives 0.05 mm, the unprotected margin 0.2 mm, so
    /// 0.25 mm is needed.
    #[test]
    fn an_inner_pour_close_to_an_unprotected_12v_pin_is_flagged() {
        let v = run("", 0.1, "");
        let pour: Vec<_> = v
            .iter()
            .filter(|v| v.message.contains("net `GND`"))
            .collect();
        assert_eq!(pour.len(), 1, "{v:?}");
        let m = &pour[0].message;
        assert!(m.contains("net `VIN` (12 V, unprotected"), "{m}");
        assert!(m.contains("0.100 mm"), "{m}");
        assert!(m.contains("In1.Cu"), "{m}");
        assert!(m.contains("needs 0.250 mm"), "{m}");
        assert_eq!(pour[0].code, CODE);
    }

    #[test]
    fn raising_the_clearance_clears_it() {
        let v = run("", 0.3, "");
        assert!(v.iter().all(|v| !v.message.contains("net `GND`")), "{v:?}");
    }

    #[test]
    fn a_protected_rail_needs_only_the_table() {
        // B1 at 12 V is 0.05 mm, under the 0.127 mm flat floor: nothing to add.
        let class = r#"netclass "FUSED" { protected }"#;
        let mut b = board(class);
        for net in &mut b.nets {
            if net.name == "VIN" {
                net.netclass = Some("FUSED".to_string());
            }
        }
        let v = check_voltage_clearance(&b, &pcb(0.1, ""), &ManufacturerProfile::jlc_standard());
        assert!(v.is_empty(), "{v:?}");
    }

    /// A mounting hole wall 0.22 mm from the 12 V pad clears the 0.2 mm
    /// hole floor, but the voltage needs 0.25 mm on inner layers, so both
    /// inner layers report it.
    #[test]
    fn mounting_hole_clearance_is_enforced_on_inner_layers() {
        // Pad copper edge at x = 11; hole wall at 11.22 (centre 12.82, r 1.6).
        let hole = r#"(footprint "MountingHole:MountingHole_3.2mm_M3"
    (layer "F.Cu") (at 12.82 10)
    (property "Reference" "H1" (at 0 -1) (layer "F.SilkS"))
    (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))"#;
        let v = run("", 5.0, hole);
        let inner: Vec<_> = v
            .iter()
            .filter(|v| v.message.contains("hole of H1") && v.message.contains(" on In"))
            .collect();
        assert_eq!(inner.len(), 2, "{v:?}");
        assert!(
            inner.iter().all(|v| v.message.contains("needs 0.250 mm")),
            "{inner:?}"
        );
    }

    #[test]
    fn a_board_without_voltage_information_gets_nothing() {
        let registry =
            synth_registry::load_from_strs(&[("t_load.synth.toml", LOAD)]).expect("parts");
        let src = r#"board "v" { layers 4 component U1: ic "t_load" net "A" { U1.vin } net "B" { U1.gnd } }"#;
        let parse = synth_parser::parse(src, "v.synth".to_string());
        let b = synth_ir::lower(parse.ast.as_ref().expect("ast"), &registry, "v.synth")
            .board
            .expect("board");
        let v = check_voltage_clearance(&b, &pcb(0.01, ""), &ManufacturerProfile::jlc_standard());
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn the_export_rules_name_the_hot_net() {
        let rules = kicad_rules(&board(""), &ManufacturerProfile::jlc_standard());
        assert!(
            rules.contains("(rule \"voltage VIN clearance inner\""),
            "{rules}"
        );
        assert!(rules.contains("(min 0.25mm)"), "{rules}");
        assert!(!rules.contains("GND"), "{rules}");
    }
}
