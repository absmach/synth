// SPDX-License-Identifier: Apache-2.0

//! Inter-board mapping checks that go beyond board/component existence:
//! the mapped pins must exist, carry the same signal name, cover every pin
//! of the connector exactly once, and sit in compatible voltage domains.

use serde_json::json;
use synth_diagnostics::Span;
use synth_ir::units::Voltage;
use synth_ir::{
    Board, Component, ComponentId, InterBoardPinMapping, MultiBoardProject, Net, NetEndpoint,
    NetId, PinId,
};
use synth_registry::Part;

fn connector(refdes: &str, pins: &[(&str, &str)]) -> Component {
    let pins: Vec<serde_json::Value> = pins
        .iter()
        .map(|(name, number)| {
            json!({
                "name": name,
                "number": number,
                "electrical_type": "passive"
            })
        })
        .collect();
    let part: Part = serde_json::from_value(json!({
        "id": "test_connector",
        "kind": "connector",
        "pins": pins
    }))
    .expect("test connector part");
    Component {
        id: ComponentId(0),
        refdes: refdes.to_string(),
        kind: "connector".to_string(),
        part: Some(part),
        value: None,
        dnp: false,
        properties: std::collections::BTreeMap::new(),
        placement_hint: None,
        escape: None,
        group: None,
        sheet: None,
        source_span: Span::new(0, 0),
    }
}

fn board(name: &str, component: Component, nets: Vec<(&str, Option<f64>, Vec<PinId>)>) -> Board {
    let nets = nets
        .into_iter()
        .enumerate()
        .map(|(id, (name, voltage, pins))| Net {
            id: NetId(id as u32),
            name: name.to_string(),
            auto_named: false,
            endpoints: pins
                .into_iter()
                .map(|pin| NetEndpoint {
                    component: ComponentId(0),
                    pin,
                    source_span: Span::new(0, 0),
                })
                .collect(),
            netclass: None,
            voltage: voltage.map(Voltage::from_v),
        })
        .collect();
    Board {
        outline: None,
        schematic_overflow: None,
        schematic_paper: None,
        groups: Vec::new(),
        legends: false,
        name: name.to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        components: vec![component],
        nets,
        diff_pairs: Vec::new(),
        notes: vec![],
        keepouts: Vec::new(),
        stackup: None,
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        variants: vec![],
        required_processes: std::collections::BTreeSet::new(),
        source_span: Span::new(0, 0),
    }
}

fn mapping(from: &str, to: &str) -> InterBoardPinMapping {
    InterBoardPinMapping {
        from_board: "control".to_string(),
        from_refdes: "J1".to_string(),
        from_pin: from.to_string(),
        to_board: "power".to_string(),
        to_refdes: "J1".to_string(),
        to_pin: to.to_string(),
    }
}

/// Four pins, 1..4, with matching names and voltage domains on both boards.
fn pair() -> (Board, Board) {
    let control = board(
        "control",
        connector(
            "J1",
            &[
                ("PWM_UH", "1"),
                ("PWM_UL", "2"),
                ("+3V3", "3"),
                ("GND", "4"),
            ],
        ),
        vec![
            ("PWM_UH", Some(3.3), vec![PinId(0)]),
            ("PWM_UL", Some(3.3), vec![PinId(1)]),
            ("+3V3", Some(3.3), vec![PinId(2)]),
            ("GND", Some(0.0), vec![PinId(3)]),
        ],
    );
    let power = board(
        "power",
        connector(
            "J1",
            &[
                ("PWM_UH", "1"),
                ("PWM_UL", "2"),
                ("+3V3", "3"),
                ("GND", "4"),
            ],
        ),
        vec![
            ("PWM_UH", Some(3.3), vec![PinId(0)]),
            ("PWM_UL", Some(3.3), vec![PinId(1)]),
            ("+3V3", Some(3.3), vec![PinId(2)]),
            ("GND", Some(0.0), vec![PinId(3)]),
        ],
    );
    (control, power)
}

fn project(control: Board, power: Board, mappings: Vec<InterBoardPinMapping>) -> MultiBoardProject {
    let mut project = MultiBoardProject::new("stack");
    project.add_board(control);
    project.add_board(power);
    for mapping in mappings {
        project.add_mapping(mapping);
    }
    project
}

fn codes(result: &synth_ir::MultiBoardValidationResult) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn a_complete_matching_mapping_is_clean() {
    let (control, power) = pair();
    let result = project(
        control,
        power,
        vec![
            mapping("1", "1"),
            mapping("2", "2"),
            mapping("3", "3"),
            mapping("4", "4"),
        ],
    )
    .validate_multiboard_system();

    assert_eq!(result.total_mappings, 4);
    assert_eq!(result.matched_pins, 4);
    assert_eq!(result.matching_percentage, 100.0);
    assert!(result.is_clean, "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn a_pin_that_does_not_exist_is_reported() {
    let (control, power) = pair();
    let result = project(
        control,
        power,
        vec![
            mapping("1", "1"),
            mapping("2", "2"),
            mapping("3", "3"),
            mapping("4", "4"),
            mapping("99", "99"),
        ],
    )
    .validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-002"),
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert_eq!(result.matched_pins, 4);
}

#[test]
fn swapped_pins_are_reported_as_a_signal_mismatch() {
    let (control, power) = pair();
    let result = project(
        control,
        power,
        vec![
            mapping("1", "2"),
            mapping("2", "1"),
            mapping("3", "3"),
            mapping("4", "4"),
        ],
    )
    .validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-003"),
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert_eq!(result.matching_percentage, 50.0);
}

#[test]
fn a_missing_mapping_is_reported() {
    let (control, power) = pair();
    let result = project(
        control,
        power,
        vec![mapping("1", "1"), mapping("2", "2"), mapping("3", "3")],
    )
    .validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-004"),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn a_pin_mapped_twice_is_reported() {
    let (control, power) = pair();
    let result = project(
        control,
        power,
        vec![
            mapping("1", "1"),
            mapping("1", "2"),
            mapping("3", "3"),
            mapping("4", "4"),
        ],
    )
    .validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-004"),
        "diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn a_voltage_domain_mismatch_is_reported() {
    let control = board(
        "control",
        connector("J1", &[("POWER", "1")]),
        vec![("POWER", Some(12.0), vec![PinId(0)])],
    );
    let power = board(
        "power",
        connector("J1", &[("POWER", "1")]),
        vec![("POWER", Some(3.3), vec![PinId(0)])],
    );
    let result = project(control, power, vec![mapping("1", "1")]).validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-005"),
        "diagnostics: {:?}",
        result.diagnostics
    );
    assert_ne!(result.matching_percentage, 100.0);
}

#[test]
fn a_ground_pin_mapped_to_a_rail_is_reported() {
    let (control, mut power) = pair();
    // Move power pin 4 (GND) onto the 3.3 V rail, keeping the name so only
    // the domain check can see the fault.
    power.nets[2].endpoints.push(NetEndpoint {
        component: ComponentId(0),
        pin: PinId(3),
        source_span: Span::new(0, 0),
    });
    power.nets[3].endpoints.clear();

    let result = project(
        control,
        power,
        vec![
            mapping("1", "1"),
            mapping("2", "2"),
            mapping("3", "3"),
            mapping("4", "4"),
        ],
    )
    .validate_multiboard_system();

    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-005"),
        "diagnostics: {:?}",
        result.diagnostics
    );
}
