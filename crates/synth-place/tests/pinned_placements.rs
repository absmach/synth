// SPDX-License-Identifier: Apache-2.0

//! A pinned component is a constraint on the solver, not a correction to its
//! answer.
//!
//! The placer used to run its whole greedy search first and apply the
//! sidecar's overrides only on success, so a board the search could not pack
//! was refused however completely its floorplan was specified — the
//! overrides were never reached. These tests use a fixture from
//! `fixtures/place-errors/`, which exists precisely because the automatic
//! placer cannot place it, and assert that an author who says where every
//! part goes gets that placement.

use std::fs;
use std::path::{Path, PathBuf};

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(path: &Path) -> Board {
    let source = fs::read_to_string(path).expect("read fixture");
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

/// A scratch directory of this test's own, so the two tests cannot collide
/// and nothing is left in the source tree.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "synth-place-pinned-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A 30 mm grid: wider than any courtyard in the fixture, so the floorplan is
/// trivially legal and the test is about whether it is honoured, not about
/// whether it is any good.
fn grid_slot(index: usize) -> (f64, f64) {
    let column = u8::try_from(index % 5).expect("column fits");
    let row = u8::try_from(index / 5).expect("row fits");
    (
        20.0 + f64::from(column) * 30.0,
        20.0 + f64::from(row) * 30.0,
    )
}

fn grid_sidecar(board: &Board, path: &Path) {
    use std::fmt::Write as _;
    let mut out = String::from("schema_version = 2\n");
    for (i, component) in board.components.iter().enumerate() {
        let (x, y) = grid_slot(i);
        write!(
            out,
            "\n[components.{}]\nx = {x:.1}\ny = {y:.1}\nrotation = 0\n",
            component.refdes
        )
        .expect("write to String");
    }
    fs::write(path, out).expect("write sidecar");
}

#[test]
fn a_fixture_the_automatic_placer_refuses_places_when_every_part_is_pinned() {
    let fixture = workspace_root()
        .join("fixtures")
        .join("place-errors")
        .join("no_position_01.synth");
    let board = load_board(&fixture);

    // The premise. This fixture is in the failure corpus because the greedy
    // search cannot pack it; if that ever stops being true the test below
    // proves nothing, so assert it rather than assume it.
    assert!(
        synth_place::place(&board).is_err(),
        "fixture is only meaningful while the automatic placer refuses it"
    );

    let dir = scratch("no-position");
    let sidecar = dir.join("no_position_01.placement.layout.toml");
    grid_sidecar(&board, &sidecar);

    let placement = synth_place::place_with_tuning_and_sidecar(
        &board,
        1.5,
        &std::collections::HashMap::new(),
        Some(&sidecar),
    )
    .expect("a fully pinned floorplan must place");

    assert_eq!(
        placement.components.len(),
        board.components.len(),
        "every component is placed"
    );

    for (i, component) in board.components.iter().enumerate() {
        let (want_x, want_y) = grid_slot(i);
        let got = placement
            .components
            .iter()
            .find(|p| p.id == component.id)
            .expect("component placed");
        assert!(
            (synth_geometry::nm_to_mm(got.center.x_nm) - want_x).abs() < 0.001
                && (synth_geometry::nm_to_mm(got.center.y_nm) - want_y).abs() < 0.001,
            "{} should sit where it was pinned: wanted ({want_x}, {want_y}), got ({}, {})",
            component.refdes,
            synth_geometry::nm_to_mm(got.center.x_nm),
            synth_geometry::nm_to_mm(got.center.y_nm),
        );
    }
}

#[test]
fn pinning_some_parts_still_leaves_the_rest_to_the_placer() {
    // The documented workflow is "use a placement sidecar only to fix
    // specific parts". Pinning two of them must not require pinning the rest,
    // and the two must land where they were put.
    let fixture = workspace_root()
        .join("examples")
        .join("env_logger")
        .join("env_logger.synth");
    let board = load_board(&fixture);

    let dir = scratch("env-logger");
    let sidecar = dir.join("env_logger.placement.layout.toml");
    let first = board.components[0].refdes.clone();
    let second = board.components[1].refdes.clone();
    fs::write(
        &sidecar,
        format!(
            "schema_version = 2\n\n[components.{first}]\nx = 12.0\ny = 40.0\nrotation = 0\n\n\
             [components.{second}]\nx = 12.0\ny = 55.0\nrotation = 90\n"
        ),
    )
    .expect("write sidecar");

    let placement = synth_place::place_with_tuning_and_sidecar(
        &board,
        1.5,
        &std::collections::HashMap::new(),
        Some(&sidecar),
    )
    .expect("a partially pinned board still places");

    assert_eq!(placement.components.len(), board.components.len());

    let pinned = placement
        .components
        .iter()
        .find(|p| p.id == board.components[0].id)
        .expect("pinned component placed");
    assert!(
        (synth_geometry::nm_to_mm(pinned.center.x_nm) - 12.0).abs() < 0.001
            && (synth_geometry::nm_to_mm(pinned.center.y_nm) - 40.0).abs() < 0.001,
        "the pinned part moved"
    );

    let rotated = placement
        .components
        .iter()
        .find(|p| p.id == board.components[1].id)
        .expect("pinned component placed");
    assert_eq!(
        rotated.rotation,
        synth_geometry::Rotation::Ninety,
        "a pinned rotation is the pinned one, not a floorplan target"
    );
}
