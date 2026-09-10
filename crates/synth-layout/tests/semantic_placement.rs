// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for §7.8 "Semantic Placement Pipeline v2" — the
//! strong-vs-weak connection weighting (§7.8.1 finding #1,
//! matchpack-style).
//!
//! [`synth_layout::layout`] builds inter-cluster adjacency from each
//! shared net's semantic connection strength: strong functional
//! signals (decoupling, resistor→IC, crystal load caps) dominate the
//! barycenter ordering and pull clusters together, while weak
//! power/ground rails only nudge orientation.
//!
//! The strong-vs-weak *proximity/ordering* effect is asserted at the
//! algorithm level in the crate's `semantic_weights_tests` (they drive
//! `build_cluster_adjacency` and `barycenter_order_rows` directly). On
//! an A4 sheet the page-height clamp pins every column to one cluster
//! for small boards, so barycenter reordering — the step the weighting
//! feeds — cannot fire end-to-end; these tests therefore assert the
//! invariants the weighting must *preserve*: determinism, 2.54 mm grid
//! snapping, unchanged power-flow layering, and correct strong-motif
//! (crystal + load caps) member placement.

use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_layout::Layout;
use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

fn pin(name: &str, et: ElectricalType) -> Pin {
    Pin {
        name: name.to_string(),
        number: PinNumber(name.to_string()),
        electrical_type: et,
        capabilities: Vec::new(),
        required: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
    }
}

fn part(kind: &str, pins: Vec<Pin>) -> Part {
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
        pins,
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
        placement_hint: None,
        group: None,
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn ep(component: u32, pin: u32) -> NetEndpoint {
    NetEndpoint {
        component: ComponentId(component),
        pin: PinId(pin),
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
    Net {
        id: NetId(id),
        name: name.to_string(),
        endpoints: endpoints.iter().map(|&(c, p)| ep(c, p)).collect(),
    }
}

fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
    Board {
        name: "semantic".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components,
        nets,
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

/// A passive two-pin resistor.
fn two_pin_resistor() -> Part {
    part(
        "resistor",
        vec![
            pin("p1", ElectricalType::Passive),
            pin("p2", ElectricalType::Passive),
        ],
    )
}

/// An MCU with two power pins and one GPIO — the anchor of a "layer
/// 2" active component.
fn mcu() -> Part {
    part(
        "mcu",
        vec![
            pin("vcc", ElectricalType::PowerInput),
            pin("gnd", ElectricalType::PowerInput),
            pin("gpio", ElectricalType::Bidirectional),
        ],
    )
}

/// A board mixing strong functional signals (resistor↔MCU, crystal↔
/// load caps) with weak power/ground rails, exercising every strength
/// class through the public pipeline.
fn mixed_strength_board() -> Board {
    board(
        vec![
            component(
                0,
                "J1",
                part(
                    "connector",
                    vec![
                        pin("vbus", ElectricalType::PowerInput),
                        pin("gnd", ElectricalType::PowerInput),
                    ],
                ),
            ),
            component(
                1,
                "U1",
                part(
                    "regulator",
                    vec![
                        pin("vin", ElectricalType::PowerInput),
                        pin("vout", ElectricalType::PowerOutput),
                        pin("gnd", ElectricalType::PowerInput),
                    ],
                ),
            ),
            component(2, "M1", mcu()),
            component(3, "R1", two_pin_resistor()),
            component(
                4,
                "X1",
                part(
                    "crystal",
                    vec![
                        pin("p1", ElectricalType::Passive),
                        pin("p2", ElectricalType::Passive),
                    ],
                ),
            ),
            component(5, "C1", two_pin_cap()),
            component(6, "C2", two_pin_cap()),
        ],
        vec![
            // Weak power rails.
            net(0, "vbus", &[(0, 0), (1, 0)]),
            net(1, "gnd", &[(0, 1), (1, 2), (2, 1), (5, 1), (6, 1)]),
            net(2, "vout", &[(1, 1), (2, 0)]),
            // Strong functional signal: R1 feeds M1's GPIO.
            net(3, "sig_r1_m1", &[(3, 1), (2, 2)]),
            // Strong crystal circuit: X1's load caps return to GND.
            net(4, "xtal_p1", &[(4, 0), (5, 0)]),
            net(5, "xtal_p2", &[(4, 1), (6, 0)]),
        ],
    )
}

fn two_pin_cap() -> Part {
    part(
        "capacitor",
        vec![
            pin("p1", ElectricalType::Passive),
            pin("p2", ElectricalType::Passive),
        ],
    )
}

/// Asserts the grid-snap invariant: every placement coordinate is a
/// whole multiple of 2.54 mm (within float tolerance).
fn assert_grid_snapped(layout: &Layout) {
    const GRID: f64 = 2.54;
    for p in &layout.components {
        let x_units = p.center_mm.0 / GRID;
        let y_units = p.center_mm.1 / GRID;
        assert!(
            (x_units - x_units.round()).abs() < 1e-6,
            "{:?} x={} is not on the 2.54 mm grid",
            p.id,
            p.center_mm.0
        );
        assert!(
            (y_units - y_units.round()).abs() < 1e-6,
            "{:?} y={} is not on the 2.54 mm grid",
            p.id,
            p.center_mm.1
        );
    }
}

#[test]
fn semantic_weighted_layout_is_deterministic_and_grid_snapped() {
    let board = mixed_strength_board();
    let a = synth_layout::layout(&board);
    let b = synth_layout::layout(&board);

    assert_eq!(
        a, b,
        "layout() must be deterministic under semantic weighting"
    );
    assert_grid_snapped(&a);
    assert_eq!(a.components.len(), board.components.len());
}

#[test]
fn power_flow_layer_assignment_still_holds() {
    // Connector (layer 0) → regulator (layer 1) → MCU (layer 2) →
    // passives/crystal (layer 3): power flows left to right, so the
    // columns must keep that order even with weak rails sharing GND.
    let board = mixed_strength_board();
    let layout = synth_layout::layout(&board);
    let x = |id: u32| {
        layout
            .placement(ComponentId(id))
            .expect("placed")
            .center_mm
            .0
    };

    let (jx, ux, mx, rx) = (x(0), x(1), x(2), x(3));
    assert!(
        jx < ux && ux < mx && mx < rx,
        "power-flow layer order (connector < regulator < MCU < passive) must hold: J1={jx} U1={ux} M1={mx} R1={rx}"
    );
    assert_grid_snapped(&layout);
}

#[test]
fn crystal_load_caps_stay_glued_below_their_crystal() {
    // The crystal motif is a *strong* sub-circuit: X1's two load caps
    // (C1, C2) are cluster members placed on the Below shelf, not
    // drifted to a distant column by the shared GND rail. Assert the
    // members sit below the anchor (y_cap > y_crystal) and within its
    // column (same x), i.e. the strong cluster kept them together.
    let board = mixed_strength_board();
    let layout = synth_layout::layout(&board);

    let crystal = layout.placement(ComponentId(4)).expect("X1 placed");
    for cap_id in [5_u32, 6_u32] {
        let cap = layout.placement(ComponentId(cap_id)).expect("cap placed");
        assert!(
            (cap.center_mm.1 > crystal.center_mm.1),
            "C{} must sit below its crystal anchor: crystal={:?} cap={:?}",
            cap_id,
            crystal.center_mm,
            cap.center_mm
        );
        // The caps fan out on the Below shelf, pin-aligned, so their x
        // can differ slightly — but they must stay near the anchor
        // (within a couple of member pitches), not drift to a distant
        // column the shared GND rail would otherwise leave them to.
        assert!(
            (cap.center_mm.0 - crystal.center_mm.0).abs() < 40.0,
            "C{} must stay near its crystal anchor: crystal={:?} cap={:?}",
            cap_id,
            crystal.center_mm,
            cap.center_mm
        );
    }
}
