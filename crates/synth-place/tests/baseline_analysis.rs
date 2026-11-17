// SPDX-License-Identifier: Apache-2.0

//! Baseline placement analysis tests (Phase 0).
//!
//! Verifies:
//! - Analysis does not mutate placement or board.
//! - Repeated analysis is byte-deterministic.
//! - Baseline metrics for representative designs.

use synth_place::{analyze_placement, place};

fn load_board(path: &str) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: std::path::PathBuf::from("../.."),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    synth_ir::lower(&resolved.program, &registry, &file)
        .board
        .expect("board")
}

#[test]
fn test_baseline_env_logger() {
    let board = load_board("../../examples/env_logger/env_logger.synth");
    let placement = place(&board).expect("place env_logger");
    let placement_clone = placement.clone();

    let report1 = analyze_placement(&board, &placement);
    let report2 = analyze_placement(&board, &placement);

    // Byte-deterministic
    assert_eq!(report1, report2);
    // Unchanged placement
    assert_eq!(placement, placement_clone);

    // Baseline validation
    assert_eq!(report1.component_count, 20);
    assert_eq!(report1.geometric.courtyard_overlaps, 0);
    assert_eq!(report1.geometric.out_of_board_components, 0);
    assert!(report1.connectivity.estimated_wirelength_mm > 0.0);

    let report_text = format!("{}", report1);
    println!("=== ENV_LOGGER BASELINE REPORT ===\n{}", report_text);
    assert!(report_text.contains("PCB PLACEMENT ANALYSIS"));
}

#[test]
fn test_baseline_sensor_logger() {
    let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
    let placement = place(&board).expect("place sensor_logger");
    let placement_clone = placement.clone();

    let report = analyze_placement(&board, &placement);
    assert_eq!(placement, placement_clone);

    assert_eq!(report.component_count, 31);
    assert_eq!(report.geometric.courtyard_overlaps, 0);
    assert_eq!(report.geometric.out_of_board_components, 0);
    assert!(report.connectivity.estimated_wirelength_mm > 0.0);

    let report_text = format!("{}", report);
    println!("=== SENSOR_LOGGER BASELINE REPORT ===\n{}", report_text);
}

#[test]
fn test_baseline_ethernet_phy() {
    let board = load_board("../../fixtures/designs/ethernet_phy.synth");
    let placement = place(&board).expect("place ethernet_phy");
    let placement_clone = placement.clone();

    let report = analyze_placement(&board, &placement);
    assert_eq!(placement, placement_clone);

    assert_eq!(report.component_count, 19);
    assert_eq!(report.geometric.courtyard_overlaps, 0);
    assert_eq!(report.geometric.out_of_board_components, 0);
    assert!(report.connectivity.estimated_wirelength_mm > 0.0);

    let report_text = format!("{}", report);
    println!("=== ETHERNET_PHY BASELINE REPORT ===\n{}", report_text);
}
