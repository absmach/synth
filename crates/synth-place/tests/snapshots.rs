// SPDX-License-Identifier: Apache-2.0

//! Bit-exact regression tests for `synth-place`. Snapshot is the
//! `Placement` of `examples/sensor_logger.synth` rendered as a
//! short, human-readable summary. Updates land via
//! `cargo insta review` when the algorithm intentionally changes.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use synth_geometry::nm_to_mm;
use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(rel: &str) -> Board {
    let path = workspace_root().join(rel);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ok");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry loads");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board lowered")
}

fn render_placement_snapshot(board: &Board, placement: &synth_place::Placement) -> String {
    let mut body = String::new();
    writeln!(
        body,
        "board_outline_mm: {:.2} x {:.2}\n",
        nm_to_mm(placement.board_outline.width_nm()),
        nm_to_mm(placement.board_outline.height_nm()),
    )
    .unwrap();
    for p in &placement.components {
        let refdes = board
            .components
            .iter()
            .find(|c| c.id == p.id)
            .map_or_else(|| format!("#{}", p.id.0), |c| c.refdes.clone());
        writeln!(
            body,
            "{:>4}  ({:7.2}, {:7.2}) mm  rot={:?} layer={:?}",
            refdes,
            nm_to_mm(p.center.x_nm),
            nm_to_mm(p.center.y_nm),
            p.rotation,
            p.layer,
        )
        .unwrap();
    }
    body
}

#[test]
fn sensor_logger_placement_snapshot() {
    let board = load_board("examples/sensor_logger.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("sensor_logger_placement", snapshot);
}

#[test]
fn secure_tracker_placement_snapshot() {
    let board = load_board("fixtures/designs/secure_tracker.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("secure_tracker_placement", snapshot);
}

#[test]
fn feather_m4_express_placement_snapshot() {
    let board = load_board("fixtures/designs/feather_m4_express.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("feather_m4_express_placement", snapshot);
}

#[test]
fn usb_c_orientation_is_derived_and_wrong_override_is_detected() {
    let board = load_board("fixtures/designs/iot_sensor_board.synth");
    let placement = synth_place::place(&board).expect("place");
    let usb = board
        .components
        .iter()
        .find(|component| component.refdes == "J1")
        .expect("USB-C component");
    let usb_placement = placement
        .components
        .iter()
        .find(|component| component.id == usb.id)
        .expect("USB-C placement");
    assert_eq!(usb_placement.rotation, synth_geometry::Rotation::OneEighty);
    assert!(synth_place::floorplan::connector_orientation_issues(
        &board,
        placement.board_outline,
        &placement.components,
    )
    .is_empty());

    let mut incorrect = placement.clone();
    incorrect
        .components
        .iter_mut()
        .find(|component| component.id == usb.id)
        .expect("USB-C placement")
        .rotation = synth_geometry::Rotation::Zero;
    let issues = synth_place::floorplan::connector_orientation_issues(
        &board,
        incorrect.board_outline,
        &incorrect.components,
    );
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].refdes, "J1");
    assert_eq!(issues[0].expected, synth_geometry::Rotation::OneEighty);
}
