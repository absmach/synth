// SPDX-License-Identifier: Apache-2.0

//! The flags of one net on two pins that face each other.
//!
//! Each flag hangs off a stub straight out of its pin and runs away from it.
//! Pins that face each other across less than two flag lengths put their flags
//! box over box, and the net's name cannot be read. One flag has to move off
//! its pin's row.

use std::collections::HashMap;

use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_layout::route::{net_label_plan, pin_terminal_xy};
use synth_layout::{default_placer, layout_with_overrides, ComponentPlacement, LabelSpot, Layout};
use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

/// Flag box in mm: 1.12 per character plus 2.9, and 2.65 tall.
const FLAG_CHAR_MM: f64 = 1.12;
const FLAG_PAD_MM: f64 = 2.9;
const FLAG_H_MM: f64 = 2.65;
/// Boxes that overlap by less than this only touch.
const TOUCH_MM: f64 = 0.2;
const NET: &str = "RS485_A_BUS";

fn pin(name: &str) -> Pin {
    Pin {
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

fn part(kind: &str, pins: &[&str]) -> Part {
    Part {
        id: PartId(kind.to_string()),
        kind: kind.to_string(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        provenance: None,
        pins: pins.iter().map(|name| pin(name)).collect(),
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
    }
}

fn component(id: u32, refdes: &str, p: Part) -> Component {
    Component {
        id: ComponentId(id),
        refdes: refdes.to_string(),
        kind: p.kind.clone(),
        part: Some(p),
        value: None,
        dnp: false,
        properties: std::collections::BTreeMap::new(),
        placement_hint: None,
        escape: None,
        group: None,
        sheet: None,
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

/// R1, a three-pin connector J1 and a second connector J2 on one net, which
/// three endpoints get a flag each.
fn board() -> Board {
    let endpoint = |component: u32, pin: u32| NetEndpoint {
        component: ComponentId(component),
        pin: PinId(pin),
        source_span: synth_diagnostics::Span::new(0, 0),
    };
    Board {
        required_processes: std::collections::BTreeSet::new(),
        schematic_overflow: None,
        schematic_paper: None,
        groups: Vec::new(),
        legends: false,
        name: "head_on".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        components: vec![
            component(0, "R1", part("resistor", &["p1", "p2"])),
            component(1, "J1", part("connector", &["p1", "p2", "p3"])),
            component(2, "J2", part("connector", &["p1", "p2"])),
        ],
        nets: vec![
            Net {
                id: NetId(0),
                name: NET.to_string(),
                auto_named: false,
                endpoints: vec![endpoint(0, 1), endpoint(1, 0), endpoint(2, 0)],
                netclass: None,
                voltage: None,
            },
            // On J1's next pin, so a rise off pin 1 has a neighbour to run into.
            Net {
                id: NetId(1),
                name: "RS485_B_BUS".to_string(),
                auto_named: false,
                endpoints: vec![endpoint(0, 0), endpoint(1, 1), endpoint(2, 1)],
                netclass: None,
                voltage: None,
            },
        ],
        diff_pairs: Vec::new(),
        notes: vec![],
        keepouts: Vec::new(),
        stackup: None,
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        variants: vec![],
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn tip(board: &Board, layout: &Layout, component: u32, pin: u32) -> (f64, f64, f64, f64) {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    pin_terminal_xy(board, ComponentId(component), PinId(pin), &placements).expect("pin")
}

fn move_to(layout: &mut Layout, component: u32, x: f64, y: f64) {
    let placement = layout
        .components
        .iter_mut()
        .find(|p| p.id == ComponentId(component))
        .expect("placed");
    placement.center_mm = (x, y);
}

/// R1's pin 2 and J1's pin 1 face each other, 11.43 mm apart on one row, with
/// J2 far away.
fn head_on(board: &Board) -> Layout {
    head_on_apart(board, 11.43)
}

/// [`head_on`] with the two pins `gap` mm apart.
fn head_on_apart(board: &Board, gap: f64) -> Layout {
    layout_with_overrides(board, &default_placer(), &|layout| {
        move_to(layout, 0, 50.0, 50.0);
        move_to(layout, 1, 100.0, 50.0);
        move_to(layout, 2, 50.0, 150.0);
        let (r_x, r_y, ..) = tip(board, layout, 0, 1);
        let (j_x, j_y, ..) = tip(board, layout, 1, 0);
        let (cx, cy) = layout.components[1].center_mm;
        move_to(layout, 1, cx + r_x + gap - j_x, cy + r_y - j_y);
    })
}

type Rect = (f64, f64, f64, f64);

/// What one net label draws: its flag box and the boxes of its wire.
struct Drawn {
    component: ComponentId,
    pin: PinId,
    flag: Rect,
    wire: Vec<Rect>,
}

/// Every net label of `layout` as drawn, with the spot it was given.
#[allow(clippy::cast_precision_loss)] // a net name is a handful of characters
fn drawn(board: &Board, layout: &Layout) -> Vec<Drawn> {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    layout
        .net_labels
        .iter()
        .map(|label| {
            let plan = net_label_plan(board, label.component, label.pin, &placements, label.spot)
                .expect("plan");
            let width = label.label.chars().count() as f64 * FLAG_CHAR_MM + FLAG_PAD_MM;
            let (x, y) = plan.anchor;
            let (x0, x1) = if plan.faces_right {
                (x, x + width)
            } else {
                (x - width, x)
            };
            let wire = plan
                .wire
                .windows(2)
                .map(|seg| {
                    (
                        seg[0].0.min(seg[1].0),
                        seg[0].1.min(seg[1].1),
                        seg[0].0.max(seg[1].0),
                        seg[0].1.max(seg[1].1),
                    )
                })
                .collect();
            Drawn {
                component: label.component,
                pin: label.pin,
                flag: (x0, y - FLAG_H_MM / 2.0, x1, y + FLAG_H_MM / 2.0),
                wire,
            }
        })
        .collect()
}

fn overlap(a: Rect, b: Rect, tolerance: f64) -> bool {
    a.2.min(b.2) - a.0.max(b.0) > tolerance && a.3.min(b.3) - a.1.max(b.1) > tolerance
}

/// Whether the boxes share a point, as two wires that meet do.
fn meet(a: Rect, b: Rect) -> bool {
    a.2.min(b.2) >= a.0.max(b.0) && a.3.min(b.3) >= a.1.max(b.1)
}

fn on(labels: &[Drawn], component: u32, pin: u32) -> &Drawn {
    labels
        .iter()
        .find(|l| l.component == ComponentId(component) && l.pin == PinId(pin))
        .expect("label")
}

#[test]
fn flags_of_pins_that_face_each_other_do_not_overlap() {
    let board = board();
    let layout = head_on(&board);
    // The pins face each other, so at their default spots the flags overlap.
    let (r, j) = (tip(&board, &layout, 0, 1), tip(&board, &layout, 1, 0));
    assert!(r.2 > 0.0 && j.2 < 0.0, "pins face {r:?} and {j:?}");
    let mut by_default = layout.clone();
    for label in &mut by_default.net_labels {
        label.spot = LabelSpot::default();
    }
    let at_default = drawn(&board, &by_default);
    assert!(
        overlap(
            on(&at_default, 0, 1).flag,
            on(&at_default, 1, 0).flag,
            TOUCH_MM
        ),
        "the setup is not head-on"
    );
    let placed = drawn(&board, &layout);
    for (i, a) in placed.iter().enumerate() {
        for b in &placed[i + 1..] {
            assert!(
                !overlap(a.flag, b.flag, TOUCH_MM),
                "flags {:?} and {:?} overlap",
                a.flag,
                b.flag
            );
        }
    }
}

/// Wires that meet join their nets in KiCad. A rise off J1's pin 1 ends at the
/// column where pin 2's stub ends.
#[test]
fn no_label_wire_meets_the_wire_of_another_label() {
    let board = board();
    let layout = head_on(&board);
    let placed = drawn(&board, &layout);
    for (i, a) in placed.iter().enumerate() {
        for b in &placed[i + 1..] {
            for (wa, wb) in a
                .wire
                .iter()
                .flat_map(|wa| b.wire.iter().map(move |wb| (wa, wb)))
            {
                assert!(!meet(*wa, *wb), "wires {wa:?} and {wb:?} meet");
            }
        }
    }
}

/// Two nets that carry the same name: the one that loses it gets a suffix.
/// Its spot must be the one it gets when the board already holds the suffixed
/// name, so the flag is placed at the width it is drawn at. Swept over name
/// lengths, since only some put the widened flag in reach of a neighbour.
#[test]
fn spots_are_chosen_for_the_renamed_flags() {
    for (len, gap) in (1..=24).flat_map(|len| (0..40).map(move |g| (len, 4.0 + f64::from(g) * 0.5)))
    {
        let name = "N".repeat(len);
        let mut clashing = board();
        clashing.nets[0].name = name.clone();
        clashing.nets[1].name = name;
        let renamed = head_on_apart(&clashing, gap);
        assert!(
            renamed.net_labels.iter().any(|l| l.label.len() > len),
            "no label was renamed at length {len}"
        );
        let mut settled = clashing.clone();
        for net in &mut settled.nets {
            let label = renamed.net_labels.iter().find(|l| l.net == net.id);
            net.name = label.map_or(net.name.clone(), |l| l.label.clone());
        }
        let expected = head_on_apart(&settled, gap);
        let spots = |layout: &Layout| -> Vec<_> {
            layout
                .net_labels
                .iter()
                .map(|l| (l.net, l.pin, l.spot))
                .collect()
        };
        assert_eq!(spots(&renamed), spots(&expected), "length {len}, gap {gap}");
    }
}
