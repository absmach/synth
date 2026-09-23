// SPDX-License-Identifier: Apache-2.0

//! Multi-sheet planning and splitting (§P26).
//!
//! Small boards keep the historic single-sheet layout byte for byte.
//! A board splits only when it is *large* — its single-sheet content
//! no longer fits the biggest standard sheet (A2) — *and* it is
//! *splittable* — its components span at least two sheet boundaries
//! (`sheet` blocks or import files, which lower to implicit sheets).
//!
//! The split reuses the single-sheet pipeline instead of
//! re-clustering: [`split_layout`] partitions one finished [`Layout`]
//! per sheet, drops cross-sheet wires in favour of
//! [`HierarchicalLabel`] stubs, and recomputes captions, boxes,
//! legends, and notes per sheet. Positions translate rigidly, so the
//! per-sheet result stays deterministic. Component and net ids are
//! never remapped — every sheet shares the parent [`Board`], which is
//! what lets ERC, power inference, and the PCB flow stay
//! sheet-agnostic.

use std::collections::{HashMap, HashSet};

use synth_ir::{Board, ComponentId, NetId};

use crate::{
    annotate_groups, body_size_for_part, clamp_annotations_to_sheet, grow_sheet_to_fit,
    place_connector_legends, place_design_notes, HierarchicalLabel, Layout, SheetSize,
    BODY_FALLBACK_H, BODY_FALLBACK_W, PAGE_MARGIN,
};

/// One sheet's share of a board: `None` is the root sheet, `Some`
/// is a `sheet`-block or import-file boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetPartition {
    pub name: Option<String>,
    pub components: Vec<ComponentId>,
}

/// One laid-out sheet: its identity plus its page-local [`Layout`].
/// Positions are rebased per sheet; ids still address the parent
/// [`Board`].
#[derive(Debug, Clone, PartialEq)]
pub struct SheetLayout {
    pub name: Option<String>,
    pub layout: Layout,
}

/// Partition a board along sheet boundaries: components without a
/// `sheet` land on the root sheet, the rest group by sheet name in
/// first-seen component order. The root partition always comes first,
/// even when empty (a board whose components all live in sheets
/// still needs a root file for its sheet instances) — so exporters
/// can rely on `partitions[0]` being root. Empty named partitions
/// never occur.
///
/// # Panics
///
/// Never in practice: the internal `expect` guards a map entry this
/// function inserted on the line above.
pub fn plan_sheets(board: &Board) -> Vec<SheetPartition> {
    let mut order: Vec<Option<String>> = vec![None];
    let mut members: HashMap<Option<String>, Vec<ComponentId>> = HashMap::new();
    members.insert(None, Vec::new());
    for component in &board.components {
        let key = component.sheet.clone();
        if !members.contains_key(&key) {
            members.insert(key.clone(), Vec::new());
            order.push(key.clone());
        }
        members
            .get_mut(&key)
            .expect("partition exists")
            .push(component.id);
    }
    order
        .into_iter()
        .map(|name| SheetPartition {
            components: members.remove(&name).unwrap_or_default(),
            name,
        })
        .collect()
}

/// True when the laid-out content needs more room than A2, the
/// biggest standard sheet. Only then is a board *large* for §P26
/// purposes. Mirrors the bound computation in `grow_sheet_to_fit`
/// (components, wires, annotations, group boxes).
pub fn sheet_overflow(board: &Board, layout: &Layout) -> bool {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for placement in &layout.components {
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
        min_x = min_x.min(cx - bw / 2.0);
        max_x = max_x.max(cx + bw / 2.0);
        min_y = min_y.min(cy - bh / 2.0);
        max_y = max_y.max(cy + bh / 2.0);
    }
    for wire in &layout.wires {
        for &(x, y) in &wire.points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    for text in &layout.annotations {
        let (x, y) = text.at_mm;
        let width = text.text.chars().count() as f64 * text.size_mm * 0.72;
        min_x = min_x.min(x);
        max_x = max_x.max(x + width);
        min_y = min_y.min(y - text.size_mm);
        max_y = max_y.max(y);
    }
    for box_ in &layout.group_boxes {
        min_x = min_x.min(box_.min_mm.0);
        max_x = max_x.max(box_.max_mm.0);
        min_y = min_y.min(box_.min_mm.1);
        max_y = max_y.max(box_.max_mm.1);
    }
    if !min_x.is_finite() {
        return false;
    }
    let (need_w, need_h) = crate::sheet_needs(min_x, max_x, min_y, max_y);
    let (a2_w, a2_h) = SheetSize::A2.dims_mm();
    need_w > a2_w || need_h > a2_h
}

/// Lay out every sheet of a board: single-sheet layout as today when
/// the board is small or unsplittable, one [`SheetLayout`] per sheet
/// boundary otherwise. The caller renders one KiCad sheet per entry
/// (root first).
pub fn layout_sheets(board: &Board, global: Layout) -> Vec<SheetLayout> {
    if !sheet_overflow(board, &global) {
        return vec![SheetLayout {
            name: None,
            layout: global,
        }];
    }
    let partitions = plan_sheets(board);
    if partitions
        .iter()
        .filter(|p| !p.components.is_empty())
        .count()
        < 2
    {
        return vec![SheetLayout {
            name: None,
            layout: global,
        }];
    }
    split_layout(board, &global, &partitions)
}

/// Split one finished global [`Layout`] into per-sheet page-local
/// layouts.
///
/// Per partition: placements translate rigidly so the partition bbox
/// starts at the page margin; intra-sheet wires, junctions touching
/// kept wires, power flags, and local labels survive; cross-sheet
/// wires are dropped in favour of [`HierarchicalLabel`] stubs (power
/// nets need none — power symbols are global); captions, boxes,
/// legends, and notes are recomputed for the subset, and the page
/// regrows from A4.
fn split_layout(board: &Board, global: &Layout, partitions: &[SheetPartition]) -> Vec<SheetLayout> {
    let power_nets: HashSet<NetId> = global.power_flags.iter().map(|f| f.net).collect();
    // Net -> partitions containing at least one endpoint.
    let mut net_partitions: HashMap<NetId, Vec<usize>> = HashMap::new();
    for (pi, partition) in partitions.iter().enumerate() {
        let members: HashSet<ComponentId> = partition.components.iter().copied().collect();
        for net in &board.nets {
            if net.endpoints.iter().any(|e| members.contains(&e.component)) {
                net_partitions.entry(net.id).or_default().push(pi);
            }
        }
    }
    let cross_net: HashSet<NetId> = net_partitions
        .iter()
        .filter_map(|(net, parts)| (parts.len() > 1).then_some(*net))
        .collect();

    partitions
        .iter()
        .enumerate()
        .map(|(pi, partition)| {
            let members: HashSet<ComponentId> = partition.components.iter().copied().collect();
            // A wire survives only on its net's home sheet: nets
            // touching several partitions lose their wires everywhere
            // (hierarchical stubs take over) and so do wires whose
            // net never touches this partition.
            let keep_wire = |net: NetId| -> bool {
                net_partitions
                    .get(&net)
                    .is_some_and(|parts| parts.len() == 1 && parts[0] == pi)
            };
            // Rigid rebase offset from the subset's placed bodies and
            // kept wires so content starts at the page margin.
            let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
            for placement in global.components.iter().filter(|p| members.contains(&p.id)) {
                let (cx, cy) = placement.center_mm;
                let (bw, bh) = board
                    .component(placement.id)
                    .and_then(|c| c.part.as_ref())
                    .map_or((BODY_FALLBACK_W, BODY_FALLBACK_H), body_size_for_part);
                min_x = min_x.min(cx - bw / 2.0);
                min_y = min_y.min(cy - bh / 2.0);
            }
            for wire in global.wires.iter().filter(|w| keep_wire(w.net)) {
                for &(x, y) in &wire.points {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                }
            }
            let (dx, dy) = if min_x.is_finite() {
                // Snapped to the 1.27 mm connection grid: the global
                // layout is grid-aligned, and an arbitrary fractional
                // offset would push every wire end off-grid (KiCad
                // ERC `endpoint_off_grid` + lost pin connections).
                (
                    crate::route::snap_grid_127(PAGE_MARGIN - min_x),
                    crate::route::snap_grid_127(PAGE_MARGIN - min_y),
                )
            } else {
                (0.0, 0.0)
            };
            let shift = |(x, y): (f64, f64)| (x + dx, y + dy);

            let mut layout = Layout {
                components: global
                    .components
                    .iter()
                    .filter(|p| members.contains(&p.id))
                    .map(|p| {
                        let mut placed = *p;
                        placed.center_mm = shift(p.center_mm);
                        placed
                    })
                    .collect(),
                wires: global
                    .wires
                    .iter()
                    .filter(|w| keep_wire(w.net))
                    .map(|w| {
                        let mut routed = w.clone();
                        routed.points = w.points.iter().copied().map(shift).collect();
                        routed
                    })
                    .collect(),
                junctions: Vec::new(),
                power_flags: global
                    .power_flags
                    .iter()
                    .filter(|f| members.contains(&f.component))
                    .cloned()
                    .collect(),
                net_labels: global
                    .net_labels
                    .iter()
                    .filter(|l| members.contains(&l.component) && !cross_net.contains(&l.net))
                    .cloned()
                    .collect(),
                hierarchical_labels: Vec::new(),
                annotations: Vec::new(),
                group_boxes: Vec::new(),
                sheet_size: SheetSize::A4,
            };
            // Junctions survive only where kept wires still meet:
            // a dot left by a dropped cross-sheet wire would dangle.
            layout.junctions = global
                .junctions
                .iter()
                .filter(|&&(jx, jy)| {
                    let (jx, jy) = (jx + dx, jy + dy);
                    layout
                        .wires
                        .iter()
                        .flat_map(|w| w.points.windows(2))
                        .filter(|pair| pair.len() == 2)
                        .any(|pair| point_on_segment((jx, jy), pair[0], pair[1], 1e-6))
                })
                .map(|&(jx, jy)| (jx + dx, jy + dy))
                .collect();
            // Hierarchical stubs for cross-sheet signal nets: one per
            // in-partition endpoint, named by the shared net name.
            // Power nets are skipped — their symbols join globally.
            for net in &board.nets {
                if !cross_net.contains(&net.id) || power_nets.contains(&net.id) {
                    continue;
                }
                for endpoint in net
                    .endpoints
                    .iter()
                    .filter(|e| members.contains(&e.component))
                {
                    if board.pin(endpoint.component, endpoint.pin).is_none() {
                        continue;
                    }
                    layout.hierarchical_labels.push(HierarchicalLabel {
                        net: net.id,
                        component: endpoint.component,
                        pin: endpoint.pin,
                        // The shared net name is what joins the sheets
                        // in KiCad — identical text on every sheet the
                        // net touches, like local net labels.
                        label: net.name.clone(),
                    });
                }
            }
            // Captions, boxes, legends, and notes recomputed for the
            // subset (positions are page-local now).
            annotate_groups(board, &mut layout);
            place_connector_legends(board, &mut layout);
            place_sheet_notes(board, &mut layout, partition.name.as_deref());
            grow_sheet_to_fit(board, &mut layout);
            clamp_annotations_to_sheet(&mut layout);
            SheetLayout {
                name: partition.name.clone(),
                layout,
            }
        })
        .collect()
}

/// Design notes for one sheet: its own `notes` (by sheet name),
/// group notes whose group lands mostly here, and — on the root
/// sheet only — unscoped board notes. Mirrors `place_design_notes`
/// but scoped to a partition instead of the whole board.
fn place_sheet_notes(board: &Board, layout: &mut Layout, sheet: Option<&str>) {
    let scoped = Board {
        name: board.name.clone(),
        layers: board.layers,
        manufacturer: board.manufacturer.clone(),
        revision: board.revision.clone(),
        company: board.company.clone(),
        components: board.components.clone(),
        nets: board.nets.clone(),
        diff_pairs: board.diff_pairs.clone(),
        notes: board
            .notes
            .iter()
            .filter(|n| note_sheet(board, layout, n).as_deref() == sheet)
            .cloned()
            .collect(),
        keepouts: board.keepouts.clone(),
        netclasses: board.netclasses.clone(),
        source_span: board.source_span,
    };
    // Group boxes were recomputed for this subset already, so group
    // notes resolve against page-local geometry.
    place_design_notes(&scoped, layout);
}

/// Which sheet a note belongs on: its own sheet block first, else
/// the sheet holding most of its group's placed components, else
/// the root sheet.
fn note_sheet(board: &Board, layout: &Layout, note: &synth_ir::Note) -> Option<String> {
    if let Some(sheet) = note.sheet.clone() {
        return Some(sheet);
    }
    let placed: HashSet<ComponentId> = layout.components.iter().map(|p| p.id).collect();
    if let Some(group) = note.group.as_deref() {
        let mut counts: HashMap<Option<String>, usize> = HashMap::new();
        for component in &board.components {
            if component.group.as_deref() != Some(group) || !placed.contains(&component.id) {
                continue;
            }
            *counts.entry(component.sheet.clone()).or_default() += 1;
        }
        // Ties break to the root sheet deterministically: `None`
        // sorts before any `Some`.
        let mut ranked: Vec<(Option<String>, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if let Some((sheet, _)) = ranked.into_iter().next() {
            return sheet;
        }
    }
    None
}

/// True when point `p` lies on segment `a→b` within tolerance.
fn point_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64), eps: f64) -> bool {
    let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    if cross.abs() > eps {
        return false;
    }
    let dot = (p.0 - a.0) * (b.0 - a.0) + (p.1 - a.1) * (b.1 - a.1);
    if dot < -eps {
        return false;
    }
    let len_sq = (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2);
    dot <= len_sq + eps
}

#[cfg(test)]
mod tests {
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    use super::*;
    use crate::{ComponentPlacement, Rotation, WirePath};

    fn pin(name: &str) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: ElectricalType::Passive,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId("r".to_string()),
            kind: "resistor".to_string(),
            description: None,
            version: 0,
            lifecycle: Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: None,
            lcsc_pn: None,
            provenance: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
        }
    }

    fn component(id: u32, refdes: &str, sheet: Option<&str>, pins: Vec<RegPin>) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: "resistor".to_string(),
            part: Some(part(pins)),
            value: None,
            dnp: false,
            placement_hint: None,
            group: None,
            sheet: sheet.map(str::to_string),
            source_span: Span::new(0, 0),
        }
    }

    fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
        Net {
            id: NetId(id),
            name: name.to_string(),
            endpoints: endpoints
                .iter()
                .map(|&(c, p)| NetEndpoint {
                    component: ComponentId(c),
                    pin: PinId(p),
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: None,
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            name: "b".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            netclasses: vec![],
            source_span: Span::new(0, 0),
        }
    }

    fn placed(id: u32, x: f64, y: f64) -> ComponentPlacement {
        ComponentPlacement {
            id: ComponentId(id),
            center_mm: (x, y),
            rotation: Rotation::Zero,
        }
    }

    fn layout(placements: Vec<ComponentPlacement>, wires: Vec<WirePath>) -> Layout {
        Layout {
            components: placements,
            wires,
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            hierarchical_labels: Vec::new(),
            annotations: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    #[test]
    fn plan_orders_root_first_then_first_seen_sheets() {
        let two = || vec![pin("p1"), pin("p2")];
        let b = board(
            vec![
                component(0, "R1", None, two()),
                component(1, "R2", Some("Power"), two()),
                component(2, "R3", Some("Input"), two()),
                component(3, "R4", Some("Power"), two()),
            ],
            vec![],
        );
        let plan = plan_sheets(&b);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].name, None);
        assert_eq!(plan[0].components, vec![ComponentId(0)], "root partition");
        assert_eq!(plan[1].name.as_deref(), Some("Power"));
        assert_eq!(plan[2].name.as_deref(), Some("Input"));
    }

    #[test]
    fn split_labels_cross_nets_and_keeps_intra_wires() {
        let two = || vec![pin("p1"), pin("p2")];
        let b = board(
            vec![
                component(0, "R1", None, two()),
                component(1, "R2", Some("Power"), two()),
                component(2, "R3", Some("Power"), two()),
            ],
            vec![
                // Cross-sheet signal net: R1.p1 (root) + R2.p1.
                net(0, "SIG", &[(0, 0), (1, 0)]),
                // Intra-sheet net inside Power.
                net(1, "LOCAL", &[(1, 1), (2, 1)]),
            ],
        );
        let global = layout(
            vec![
                placed(0, 30.0, 30.0),
                placed(1, 100.0, 30.0),
                placed(2, 170.0, 30.0),
            ],
            vec![
                WirePath {
                    net: NetId(0),
                    points: vec![(30.0, 30.0), (100.0, 30.0)],
                    junctions: Vec::new(),
                },
                WirePath {
                    net: NetId(1),
                    points: vec![(100.0, 40.0), (170.0, 40.0)],
                    junctions: Vec::new(),
                },
            ],
        );
        let partitions = plan_sheets(&b);
        let sheets = split_layout(&b, &global, &partitions);
        assert_eq!(sheets.len(), 2);
        let root = &sheets[0];
        let power = &sheets[1];
        assert_eq!(root.name, None);
        assert_eq!(power.name.as_deref(), Some("Power"));
        // Cross net: no wires anywhere, hierarchical stubs on both.
        assert!(root.layout.wires.is_empty());
        assert_eq!(power.layout.wires.len(), 1);
        assert_eq!(root.layout.hierarchical_labels.len(), 1);
        assert_eq!(root.layout.hierarchical_labels[0].label, "SIG");
        assert_eq!(power.layout.hierarchical_labels.len(), 1);
        assert_eq!(power.layout.hierarchical_labels[0].label, "SIG");
        // Intra-sheet local labels survive only where placed... none
        // were set, but the kept wire proves the filter.
        assert!(power.layout.wires.iter().any(|w| w.net == NetId(1)));
        // Page-local rebase: content starts at the margin.
        for sheet in &sheets {
            for placement in &sheet.layout.components {
                assert!(
                    placement.center_mm.0 >= PAGE_MARGIN - 1e-6,
                    "rebased placement off-page: {placement:?}"
                );
            }
        }
    }

    #[test]
    fn power_nets_get_no_hierarchical_labels() {
        use synth_registry::ElectricalType as Et;
        let power_pin = |name: &str| RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: Et::PowerInput,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        };
        let b = board(
            vec![
                component(0, "U1", None, vec![power_pin("vcc"), pin("gnd")]),
                component(1, "U2", Some("Power"), vec![power_pin("vcc"), pin("gnd")]),
            ],
            vec![net(0, "VCC", &[(0, 0), (1, 0)])],
        );
        // Global flags as the pipeline would classify them.
        let mut global = layout(vec![placed(0, 30.0, 30.0), placed(1, 100.0, 30.0)], vec![]);
        global.power_flags = crate::classify_power_flags(&b);
        assert_eq!(global.power_flags.len(), 2);
        let partitions = plan_sheets(&b);
        let sheets = split_layout(&b, &global, &partitions);
        for sheet in &sheets {
            assert!(
                sheet.layout.hierarchical_labels.is_empty(),
                "power rails join globally, no stubs"
            );
            assert_eq!(sheet.layout.power_flags.len(), 1);
        }
    }
}
