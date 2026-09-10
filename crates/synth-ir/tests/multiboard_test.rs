// SPDX-License-Identifier: Apache-2.0

//! Sub-Phase 12c Multi-Board Co-Design Engine Integration Test.

use synth_diagnostics::Span;
use synth_ir::{Board, Component, ComponentId, InterBoardPinMapping, MultiBoardProject};

fn create_dummy_board(name: &str, connector_refdes: &str) -> Board {
    let comp = Component {
        id: ComponentId(0),
        refdes: connector_refdes.to_string(),
        kind: "connector".to_string(),
        part: None,
        value: None,
        placement_hint: None,
        group: None,
        source_span: Span::new(0, 0),
    };

    Board {
        name: name.to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components: vec![comp],
        nets: Vec::new(),
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: Span::new(0, 0),
    }
}

#[test]
fn test_multiboard_system_co_design_validation() {
    let mainboard = create_dummy_board("mainboard", "J1");
    let daughtercard = create_dummy_board("daughtercard", "J1");

    let mut project = MultiBoardProject::new("sensor_system");
    project.add_board(mainboard);
    project.add_board(daughtercard);

    project.add_mapping(InterBoardPinMapping {
        from_board: "mainboard".to_string(),
        from_refdes: "J1".to_string(),
        from_pin: "1".to_string(),
        to_board: "daughtercard".to_string(),
        to_refdes: "J1".to_string(),
        to_pin: "1".to_string(),
    });

    let result = project.validate_multiboard_system();

    println!("[Sub-Phase 12c] Multi-Board Validation Result: {result:?}");

    assert_eq!(result.total_mappings, 1);
    assert_eq!(result.matched_pins, 1);
    assert_eq!(result.matching_percentage, 100.0);
    assert!(
        result.is_clean,
        "Valid 2-board mapping must have zero diagnostics"
    );
}

#[test]
fn test_multiboard_system_detects_unknown_board() {
    let mainboard = create_dummy_board("mainboard", "J1");
    let mut project = MultiBoardProject::new("invalid_system");
    project.add_board(mainboard);

    project.add_mapping(InterBoardPinMapping {
        from_board: "mainboard".to_string(),
        from_refdes: "J1".to_string(),
        from_pin: "1".to_string(),
        to_board: "nonexistent_board".to_string(),
        to_refdes: "J1".to_string(),
        to_pin: "1".to_string(),
    });

    let result = project.validate_multiboard_system();
    assert!(!result.is_clean);
    assert_eq!(result.diagnostics[0].code, "E-SYNTH-MULTIBOARD-001");
}
