// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the in-house schematic aesthetic ERC engine
//! (`synth_kicad::schem_erc`, `E-SYNTH-SCHEM-001..004`).
//!
//! Each rule is exercised with a synthetic bad layout (it fires) and a
//! synthetic good layout (it stays silent), matching the twin-pair
//! methodology from §5.2.3 of the implementation plan. All inputs are
//! constructed directly as `synth_layout::Layout` / `synth_ir::Board`
//! values — the engine is pure and needs no KiCad installation.

use synth_diagnostics::{EntityRef, Severity, Span};
use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_kicad::schem_erc::check;
use synth_layout::{ComponentPlacement, NetLabel, PowerFlag, PowerFlagKind, Rotation, SheetSize};
use synth_registry::{Lifecycle, Part, PartId, Pin, PinNumber, RequiredDecoupling};

fn empty_board() -> Board {
    Board {
        name: "test".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components: Vec::new(),
        nets: Vec::new(),
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: Span::new(0, 0),
    }
}

fn layout(
    components: Vec<ComponentPlacement>,
    wires: Vec<synth_layout::WirePath>,
    power_flags: Vec<PowerFlag>,
    net_labels: Vec<NetLabel>,
) -> synth_layout::Layout {
    synth_layout::Layout {
        components,
        wires,
        junctions: Vec::new(),
        power_flags,
        net_labels,
        annotations: Vec::new(),
        sheet_size: SheetSize::A4,
    }
}

fn placement(id: ComponentId, x: f64, y: f64, rotation: Rotation) -> ComponentPlacement {
    ComponentPlacement {
        id,
        center_mm: (x, y),
        rotation,
    }
}

fn wire(net: NetId, points: Vec<(f64, f64)>) -> synth_layout::WirePath {
    synth_layout::WirePath {
        net,
        points,
        junctions: Vec::new(),
    }
}

fn flag(component: ComponentId, pin: PinId, kind: PowerFlagKind, label: &str) -> PowerFlag {
    PowerFlag {
        net: NetId(0),
        component,
        pin,
        kind,
        label: label.to_string(),
    }
}

/// A minimal two-pin part with a `kind`, used to give components a
/// resolvable part without pulling in the registry.
fn part(kind: &str) -> Part {
    Part {
        id: PartId(format!("part_{kind}")),
        kind: kind.to_string(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        pins: vec![
            Pin {
                name: "1".to_string(),
                number: PinNumber("1".to_string()),
                electrical_type: synth_registry::ElectricalType::Passive,
                capabilities: Vec::new(),
                required: false,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            },
            Pin {
                name: "2".to_string(),
                number: PinNumber("2".to_string()),
                electrical_type: synth_registry::ElectricalType::Passive,
                capabilities: Vec::new(),
                required: false,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            },
        ],
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
        provenance: None,
    }
}

// ----- E-SYNTH-SCHEM-001 -----------------------------------------------------
#[test]
fn schem_001_fires_on_inverted_gnd_symbol() {
    // GND on pin 1, Ninety puts pin 1 on Top → GND points up.
    let layout = layout(
        vec![placement(ComponentId(0), 10.0, 10.0, Rotation::Ninety)],
        Vec::new(),
        vec![flag(ComponentId(0), PinId(1), PowerFlagKind::Gnd, "GND")],
        Vec::new(),
    );
    let violations = check(&layout, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-001");
    assert_eq!(violations[0].severity, Severity::Warning);
}

#[test]
fn schem_001_stays_silent_on_correctly_oriented_gnd() {
    // GND on pin 1 at TwoSeventy puts pin 1 on Bottom → GND points down.
    let layout = layout(
        vec![placement(ComponentId(0), 10.0, 10.0, Rotation::TwoSeventy)],
        Vec::new(),
        vec![flag(ComponentId(0), PinId(1), PowerFlagKind::Gnd, "GND")],
        Vec::new(),
    );
    assert!(check(&layout, &empty_board()).is_empty());
}

// ----- E-SYNTH-SCHEM-002 -----------------------------------------------------

#[test]
fn schem_002_fires_on_six_crossings() {
    let mut wires = vec![wire(NetId(0), vec![(0.0, 5.0), (100.0, 5.0)])];
    for i in 1u32..=6 {
        let x = f64::from(i) * 5.0;
        wires.push(wire(NetId(i), vec![(x, 0.0), (x, 10.0)]));
    }
    let layout = layout(Vec::new(), wires, Vec::new(), Vec::new());
    let violations = check(&layout, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-002");
    assert!(violations[0].message.as_deref().unwrap().contains('6'));
}

#[test]
fn schem_002_stays_silent_on_two_crossings() {
    let layout = layout(
        Vec::new(),
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (10.0, 5.0)]),
            wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
            wire(NetId(2), vec![(8.0, 0.0), (8.0, 10.0)]),
        ],
        Vec::new(),
        Vec::new(),
    );
    assert!(check(&layout, &empty_board()).is_empty());
}

// ----- E-SYNTH-SCHEM-003 -----------------------------------------------------

fn decoupling_board() -> Board {
    let mut ic_part = part("mcu");
    ic_part.required_decoupling = vec![RequiredDecoupling {
        net: "VCC".to_string(),
        value: "100n".to_string(),
        count: 1,
        max_distance_mm: None,
    }];
    let mut cap_part = part("capacitor");
    cap_part.required_decoupling = Vec::new();

    Board {
        name: "dec".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components: vec![
            Component {
                id: ComponentId(0),
                refdes: "U1".to_string(),
                kind: "mcu".to_string(),
                part: Some(ic_part),
                value: None,
                placement_hint: None,
                group: None,
                source_span: Span::new(0, 0),
            },
            Component {
                id: ComponentId(1),
                refdes: "C1".to_string(),
                kind: "capacitor".to_string(),
                part: Some(cap_part),
                value: None,
                placement_hint: None,
                group: None,
                source_span: Span::new(0, 0),
            },
        ],
        nets: vec![Net {
            id: NetId(0),
            name: "VCC".to_string(),
            endpoints: vec![
                NetEndpoint {
                    component: ComponentId(0),
                    pin: PinId(0),
                    source_span: Span::new(0, 0),
                },
                NetEndpoint {
                    component: ComponentId(1),
                    pin: PinId(0),
                    source_span: Span::new(0, 0),
                },
            ],
        }],
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: Span::new(0, 0),
    }
}

#[test]
fn schem_003_fires_on_cap_far_from_ic() {
    let board = decoupling_board();
    let layout = layout(
        vec![
            placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
            placement(ComponentId(1), 100.0, 10.0, Rotation::Zero),
        ],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let violations = check(&layout, &board);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-003");
    assert!(violations[0]
        .entities
        .iter()
        .any(|e| matches!(e, EntityRef::Component { id } if id == "U1")));
}

#[test]
fn schem_003_stays_silent_on_cap_near_ic() {
    let board = decoupling_board();
    let layout = layout(
        vec![
            placement(ComponentId(0), 10.0, 10.0, Rotation::Zero),
            placement(ComponentId(1), 14.0, 10.0, Rotation::Zero),
        ],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    assert!(check(&layout, &board).is_empty());
}

// ----- E-SYNTH-SCHEM-004 -----------------------------------------------------

#[test]
fn schem_004_fires_on_long_explicit_net() {
    let layout = layout(
        Vec::new(),
        vec![wire(NetId(0), vec![(0.0, 0.0), (150.0, 0.0)])],
        Vec::new(),
        Vec::new(),
    );
    let violations = check(&layout, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-004");
    assert!(violations[0].message.as_deref().unwrap().contains("net 0"));
}

#[test]
fn schem_004_stays_silent_on_short_net() {
    let layout = layout(
        Vec::new(),
        vec![wire(NetId(0), vec![(0.0, 0.0), (50.0, 0.0)])],
        Vec::new(),
        Vec::new(),
    );
    assert!(check(&layout, &empty_board()).is_empty());
}

#[test]
fn schem_004_stays_silent_on_label_truncated_net() {
    // A long signal that was truncated to net labels has no wire, so it
    // must not fire — label truncation is exactly the fix the rule
    // rewards.
    let layout = layout(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![NetLabel {
            net: NetId(0),
            component: ComponentId(0),
            pin: PinId(0),
            label: "LONG_SIG".to_string(),
        }],
    );
    assert!(check(&layout, &empty_board()).is_empty());
}

// ----- E-SYNTH-SCHEM-005 -----------------------------------------------------

#[test]
fn schem_005_fires_on_four_line_node() {
    // Same-net plus-cross at (5, 5): a 3-point polyline contributes 2
    // segment-ends, two stubs one each → degree 4.
    let mut l = layout(
        Vec::new(),
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
            wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
            wire(NetId(0), vec![(5.0, 10.0), (5.0, 5.0)]),
        ],
        Vec::new(),
        Vec::new(),
    );
    l.junctions = vec![(5.0, 5.0)];
    let violations = check(&l, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-005");
    assert_eq!(violations[0].severity, Severity::Warning);
}

#[test]
fn schem_005_stays_silent_on_three_line_t_node() {
    let mut l = layout(
        Vec::new(),
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
            wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
        ],
        Vec::new(),
        Vec::new(),
    );
    l.junctions = vec![(5.0, 5.0)];
    assert!(check(&l, &empty_board()).is_empty());
}

// ----- E-SYNTH-SCHEM-006 -----------------------------------------------------

#[test]
fn schem_006_fires_on_junction_dot_touching_foreign_wire() {
    // Net 0 owns a T junction at (5, 5); net 1's wire passes straight
    // through the same point.
    let mut l = layout(
        Vec::new(),
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
            wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
            wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
        ],
        Vec::new(),
        Vec::new(),
    );
    l.junctions = vec![(5.0, 5.0)];
    let violations = check(&l, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-006");
}

#[test]
fn schem_006_stays_silent_when_foreign_wire_stops_short() {
    let mut l = layout(
        Vec::new(),
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
            wire(NetId(0), vec![(5.0, 0.0), (5.0, 5.0)]),
            wire(NetId(1), vec![(20.0, 0.0), (20.0, 10.0)]),
        ],
        Vec::new(),
        Vec::new(),
    );
    l.junctions = vec![(5.0, 5.0)];
    assert!(check(&l, &empty_board()).is_empty());
}

// ----- E-SYNTH-SCHEM-007 -----------------------------------------------------

#[test]
fn schem_007_fires_on_content_past_the_sheet_edge() {
    // A4 landscape is 297 × 210 mm; x=320 overflows.
    let layout = layout(
        vec![placement(ComponentId(0), 320.0, 50.0, Rotation::Zero)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let violations = check(&layout, &empty_board());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, "E-SYNTH-SCHEM-007");
}

#[test]
fn schem_007_stays_silent_when_content_fits_the_sheet() {
    let layout = layout(
        vec![placement(ComponentId(0), 280.0, 200.0, Rotation::Zero)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    assert!(check(&layout, &empty_board()).is_empty());
}

// ----- aggregate -------------------------------------------------------------

#[test]
fn all_rules_fire_together_in_order() {
    // Build a layout that trips every rule at once, and assert the
    // diagnostics arrive in rule order (001 → 002 → … → 007).
    let board = decoupling_board();
    let mut l = layout(
        vec![
            // IC rotated so its GND flag points up → 001.
            placement(ComponentId(0), 10.0, 10.0, Rotation::Ninety),
            // Cap 90 mm away → 003.
            placement(ComponentId(1), 100.0, 10.0, Rotation::Zero),
            // Beyond the A4 sheet edge (297 mm) → 007.
            placement(ComponentId(2), 350.0, 50.0, Rotation::Zero),
        ],
        vec![
            wire(NetId(0), vec![(0.0, 5.0), (150.0, 5.0)]), // long → 004
            wire(NetId(1), vec![(5.0, 0.0), (5.0, 10.0)]),
            wire(NetId(2), vec![(10.0, 0.0), (10.0, 10.0)]),
            wire(NetId(3), vec![(15.0, 0.0), (15.0, 10.0)]),
            wire(NetId(4), vec![(20.0, 0.0), (20.0, 10.0)]),
            wire(NetId(5), vec![(25.0, 0.0), (25.0, 10.0)]),
            wire(NetId(6), vec![(30.0, 0.0), (30.0, 10.0)]),
            // Same-net 4-way node at (5, 5) → 005.
            wire(NetId(1), vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)]),
            wire(NetId(1), vec![(5.0, 10.0), (5.0, 5.0)]),
            wire(NetId(1), vec![(5.0, 5.0), (5.0, 15.0)]),
            // A different net's wire passing straight through the
            // same node → 006.
            wire(NetId(7), vec![(5.0, 0.0), (5.0, 10.0)]),
        ],
        // GND on pin 1 at Ninety → pin 1 on Top → points up → 001.
        vec![flag(ComponentId(0), PinId(1), PowerFlagKind::Gnd, "GND")],
        Vec::new(),
    );
    l.junctions = vec![(5.0, 5.0)];
    let violations = check(&l, &board);
    let codes: Vec<&str> = violations.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            "E-SYNTH-SCHEM-001",
            "E-SYNTH-SCHEM-002",
            "E-SYNTH-SCHEM-003",
            "E-SYNTH-SCHEM-004",
            "E-SYNTH-SCHEM-005",
            "E-SYNTH-SCHEM-006",
            "E-SYNTH-SCHEM-007",
        ]
    );
}
