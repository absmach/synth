// SPDX-License-Identifier: Apache-2.0

//! Characterization tests for net naming — finding C1 groundwork.
//!
//! [`pick_net_label`] decides every rendered signal-net label string
//! and, through it, whether two distinct nets export as one silently
//! shorted KiCad net (same-named local labels on one sheet are
//! electrically merged). These tests pin down what the function does
//! *today* — including quirks — so the uniqueness fix in
//! `uniquify_net_labels` cannot silently shift naming semantics.
//!
//! The private helpers (`voltage_token`, `regulator_rail_label`,
//! `classify_power_flags`) are characterized by the in-module
//! `naming_tests` unit tests at the bottom of `lib.rs`, following the
//! repo convention of per-topic `#[cfg(test)]` modules there.

use synth_diagnostics::Span;
use synth_ir::{Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_layout::pick_net_label;
use synth_registry::{
    ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinCapability, PinNumber,
};

fn pin(name: &str, caps: &[PinCapability]) -> RegPin {
    RegPin {
        name: name.to_string(),
        number: PinNumber(name.to_string()),
        electrical_type: ElectricalType::Bidirectional,
        capabilities: caps.to_vec(),
        required: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
    }
}

fn part(kind: &str, pins: Vec<RegPin>) -> Part {
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
    }
}

fn board(components: Vec<Component>, nets: Vec<Net>) -> synth_ir::Board {
    synth_ir::Board {
        name: "test".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components,
        nets,
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: Span::new(0, 0),
    }
}

/// A minimal MCU with one pin; `caps` land on that pin.
fn mcu_with_pin(pin_name: &str, caps: &[PinCapability]) -> Component {
    component(0, "U1", part("mcu", vec![pin(pin_name, caps)]))
}

#[test]
fn capability_tokens_map_to_bare_names() {
    let cases: &[(&[PinCapability], &str)] = &[
        (&[PinCapability::I2cSda], "SDA"),
        (&[PinCapability::I2cScl], "SCL"),
        (&[PinCapability::SpiMosi], "MOSI"),
        (&[PinCapability::SpiMiso], "MISO"),
        (&[PinCapability::SpiSck], "SCK"),
        (&[PinCapability::SpiCs], "CS"),
        (&[PinCapability::UartTx], "TX"),
        (&[PinCapability::UartRx], "RX"),
        (&[PinCapability::Reset], "RESET"),
    ];
    for (caps, want) in cases {
        let b = board(vec![mcu_with_pin("x", caps)], vec![net(0, "n", &[(0, 0)])]);
        assert_eq!(
            pick_net_label(&b, &b.nets[0]).as_deref(),
            Some(*want),
            "capabilities {caps:?}"
        );
    }
}

#[test]
fn active_pin_without_caps_falls_back_to_uppercased_pin_name() {
    // An active-IC pin with no semantic capability yields its own
    // name uppercased — this is how two sensors' `int` pins both
    // become `INT` today (the C1 collision shape).
    let b = board(vec![mcu_with_pin("int", &[])], vec![net(0, "n", &[(0, 0)])]);
    assert_eq!(pick_net_label(&b, &b.nets[0]).as_deref(), Some("INT"));
}

#[test]
fn first_active_endpoint_short_circuits_the_scan() {
    // Quirk pinned: once an active-kind endpoint resolves to a pin,
    // its answer is returned immediately — even a bare uppercase
    // fallback — later endpoints are never consulted. U2's `sda`
    // capability is invisible behind U1's plain `int` pin.
    let u1 = mcu_with_pin("int", &[]);
    let u2 = component(
        1,
        "U2",
        part(
            "sensor",
            vec![pin("gnd", &[]), pin("sda", &[PinCapability::I2cSda])],
        ),
    );
    let b = board(vec![u1, u2], vec![net(0, "n", &[(0, 0), (1, 1)])]);
    assert_eq!(pick_net_label(&b, &b.nets[0]).as_deref(), Some("INT"));
}

#[test]
fn unresolvable_endpoints_are_skipped() {
    // Endpoints pointing at missing components or out-of-range pins
    // are skipped while scanning for an active-IC pin.
    let sensor = component(
        0,
        "U2",
        part("sensor", vec![pin("sda", &[PinCapability::I2cSda])]),
    );
    let b = board(
        vec![sensor],
        vec![
            // Component 9 does not exist.
            net(0, "missing_component", &[(9, 0), (0, 0)]),
            // Pin 3 is out of range for U2 but component 8 is also
            // missing, so the scan falls through to U2's real pin.
            net(1, "missing_then_real", &[(8, 3), (0, 0)]),
        ],
    );
    assert_eq!(pick_net_label(&b, &b.nets[0]).as_deref(), Some("SDA"));
    assert_eq!(pick_net_label(&b, &b.nets[1]).as_deref(), Some("SDA"));
}
#[test]
fn passive_only_net_falls_back_to_first_endpoint_pin_name() {
    // No active-IC endpoint anywhere: whatever endpoint is first
    // lends its uppercased pin name.
    let r1 = component(
        0,
        "R1",
        part("resistor", vec![pin("a", &[]), pin("b", &[])]),
    );
    let r2 = component(
        1,
        "R2",
        part("resistor", vec![pin("z", &[]), pin("y", &[])]),
    );
    let b = board(vec![r1, r2], vec![net(0, "sig", &[(0, 1), (1, 0)])]);
    assert_eq!(pick_net_label(&b, &b.nets[0]).as_deref(), Some("B"));
}

#[test]
fn net_without_endpoints_yields_none() {
    let b = board(vec![], vec![net(0, "dangling", &[])]);
    assert_eq!(pick_net_label(&b, &b.nets[0]), None);
}

// ----- Uniqueness property (plan 002 step 3) --------------------------------

fn passive_pin(name: &str) -> RegPin {
    pin(name, &[])
}

fn power_input_pin(name: &str) -> RegPin {
    RegPin {
        electrical_type: ElectricalType::PowerInput,
        ..pin(name, &[])
    }
}

/// Two I²C peripherals hanging off two separate buses of one MCU:
/// `U1.sda1/scl1 → U2`, `U1.sda2/scl2 → U3`, each bus with its own
/// pull-up so every signal net is multi-drop (≥3 endpoints) and gets
/// rendered as labels. Pre-fix, both buses' SDA nets render the bare
/// token `SDA` (and both SCL nets `SCL`) — the C1 short-circuit
/// shape, because KiCad merges same-named local labels electrically.
fn two_bus_board() -> synth_ir::Board {
    let u1 = component(
        0,
        "U1",
        part(
            "mcu",
            vec![
                power_input_pin("gnd"),
                power_input_pin("vdd"),
                pin("sda1", &[PinCapability::I2cSda]),
                pin("scl1", &[PinCapability::I2cScl]),
                pin("sda2", &[PinCapability::I2cSda]),
                pin("scl2", &[PinCapability::I2cScl]),
            ],
        ),
    );
    let sensor = |id: u32, refdes: &str| {
        component(
            id,
            refdes,
            part(
                "sensor",
                vec![
                    power_input_pin("gnd"),
                    power_input_pin("vdd"),
                    pin("sda", &[PinCapability::I2cSda]),
                    pin("scl", &[PinCapability::I2cScl]),
                ],
            ),
        )
    };
    let resistor = |id: u32, refdes: &str| {
        component(
            id,
            refdes,
            part("resistor", vec![passive_pin("p1"), passive_pin("p2")]),
        )
    };
    board(
        vec![
            u1,
            sensor(1, "U2"),
            sensor(2, "U3"),
            resistor(3, "R1"),
            resistor(4, "R2"),
            resistor(5, "R3"),
            resistor(6, "R4"),
        ],
        vec![
            // Ground and supply rails become power flags, not labels.
            net(0, "gnd", &[(0, 0), (1, 0), (2, 0)]),
            net(1, "vdd", &[(0, 1), (3, 1), (4, 1), (5, 1), (6, 1)]),
            // Bus 1: U1.sda1/scl1 ↔ U2, pull-ups R1/R2.
            net(2, "bus1_sda", &[(0, 2), (1, 2), (3, 0)]),
            net(3, "bus1_scl", &[(0, 3), (1, 3), (4, 0)]),
            // Bus 2: U1.sda2/scl2 ↔ U3, pull-ups R3/R4.
            net(4, "bus2_sda", &[(0, 4), (2, 2), (5, 0)]),
            net(5, "bus2_scl", &[(0, 5), (2, 3), (6, 0)]),
        ],
    )
}

#[test]
fn two_buses_never_share_a_rendered_label() {
    use std::collections::{HashMap, HashSet};
    use synth_layout::layout;

    let b = two_bus_board();
    let layout = layout(&b);

    // Distinct nets must not share a rendered label string — this is
    // the sheet-wide guarantee that keeps KiCad from silently merging
    // two buses into one net.
    let mut by_net: HashMap<NetId, HashSet<&str>> = HashMap::new();
    for label in &layout.net_labels {
        by_net
            .entry(label.net)
            .or_default()
            .insert(label.label.as_str());
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for (id, texts) in &by_net {
        assert_eq!(
            texts.len(),
            1,
            "net {id:?} must render one consistent label, got {texts:?}"
        );
        let text = *texts.iter().next().unwrap();
        assert!(
            seen.insert(text),
            "labels must be unique per net: {text:?} claimed twice ({by_net:?})"
        );
    }

    // Every bus net got labelled at all (multi-drop ⇒ label).
    for id in [NetId(2), NetId(3), NetId(4), NetId(5)] {
        assert!(by_net.contains_key(&id), "bus net {id:?} must be labelled");
    }

    // Deterministic disambiguation shape: the MCU supplies the tokens
    // for all four bus nets, so the lowest ordinal keeps the bare
    // token and the higher bus gains `_U1`.
    let text_of = |id: NetId| -> &str { by_net[&id].iter().next().copied().unwrap() };
    assert_eq!(text_of(NetId(2)), "SDA");
    assert_eq!(text_of(NetId(4)), "SDA_U1");
    assert_eq!(text_of(NetId(3)), "SCL");
    assert_eq!(text_of(NetId(5)), "SCL_U1");

    // Output is deterministic across runs.
    let again = synth_layout::layout(&b);
    assert_eq!(layout.net_labels, again.net_labels);
}
