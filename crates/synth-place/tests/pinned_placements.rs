// SPDX-License-Identifier: Apache-2.0

//! A pinned component is a constraint on the solver, not a correction to its
//! answer.
//!
//! The placer used to run its whole greedy search first and apply the
//! sidecar's overrides only on success, so a board the search could not pack
//! was refused however completely its floorplan was specified — the
//! overrides were never reached. These tests use a fixture from
//! `fixtures/place-errors/`, which exists precisely because the automatic
//! placer cannot place it, and assert that an author who pins every part gets
//! a placement.
//!
//! The pinned coordinates are a floorplan, not a fixed layout: after
//! legalisation the placer closes the bands no courtyard uses, keeping each
//! entry's rotation and the order of the pinned parts, so the finished board
//! is no larger than the floorplan the pins describe. A floorplan with a
//! `relative_to` entry keeps its coordinates exactly, because the band pass
//! moves one part at a time.

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

/// The extent the pinned grid describes, in millimetres.
fn grid_span(board: &Board) -> (f64, f64) {
    let slots: Vec<(f64, f64)> = (0..board.components.len()).map(grid_slot).collect();
    let min_x = slots.iter().map(|s| s.0).fold(f64::MAX, f64::min);
    let max_x = slots.iter().map(|s| s.0).fold(f64::MIN, f64::max);
    let min_y = slots.iter().map(|s| s.1).fold(f64::MAX, f64::min);
    let max_y = slots.iter().map(|s| s.1).fold(f64::MIN, f64::max);
    (max_x - min_x, max_y - min_y)
}

fn inside_outline(placement: &synth_place::Placement, point: synth_geometry::Point) -> bool {
    let outline = placement.board_outline;
    point.x_nm >= outline.min.x_nm
        && point.x_nm <= outline.max.x_nm
        && point.y_nm >= outline.min.y_nm
        && point.y_nm <= outline.max.y_nm
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

    // The pinned coordinates are a floorplan, not a fixed layout: the pinned
    // pass closes the empty bands between them, so the finished board is not
    // larger than the floorplan the pins describe. Every part keeps its pinned
    // rotation and sits inside the returned outline.
    let (span_w, span_h) = grid_span(&board);
    assert!(
        synth_geometry::nm_to_mm(placement.board_outline.width_nm()) <= span_w
            && synth_geometry::nm_to_mm(placement.board_outline.height_nm()) <= span_h,
        "the compacted floorplan must not exceed the pinned span ({span_w} x {span_h} mm)"
    );
    for component in &board.components {
        let got = placement
            .components
            .iter()
            .find(|p| p.id == component.id)
            .expect("component placed");
        assert_eq!(got.rotation, synth_geometry::Rotation::Zero);
        assert!(inside_outline(&placement, got.center));
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

    // The pinned coordinates are a floorplan: the band pass may slide the two
    // pinned parts toward the near edge, but it keeps their order and their
    // pinned rotations rather than packing them where it likes.
    assert!(
        synth_geometry::nm_to_mm(pinned.center.y_nm) <= 40.0
            && synth_geometry::nm_to_mm(rotated.center.y_nm) <= 55.0,
        "a band slide only moves toward the edge"
    );
    assert!(
        pinned.center.y_nm < rotated.center.y_nm,
        "the pinned order is kept"
    );
    assert!(inside_outline(&placement, pinned.center));
    assert!(inside_outline(&placement, rotated.center));
}

/// A relative entry is a pin too, and the outline must not overrule it.
///
/// Absolute entries reach the solver as constraints, so it sizes an outline
/// that contains them. A `relative_to` entry cannot: it is defined against an
/// anchor that has to be placed before it can be measured from, so it is
/// resolved in `apply_sidecar_overrides` after the outline is already fixed.
/// `legalize_sidecar_overrides` then judged it against that outline, found it
/// outside, and moved it to the nearest slot inside — and
/// `tighten_outline_after_sidecar` shrank the outline around the result, so
/// the board carried a coordinate the sidecar never asked for and said
/// nothing about it.
#[test]
fn a_relative_pin_outside_the_outline_is_not_quietly_moved() {
    let fixture = workspace_root()
        .join("examples")
        .join("env_logger")
        .join("env_logger.synth");
    let board = load_board(&fixture);

    let automatic = synth_place::place(&board).expect("the fixture places");
    let anchor = board.components[0].refdes.clone();
    let follower = board.components[1].refdes.clone();
    let anchor_at = automatic
        .components
        .iter()
        .find(|p| p.id == board.components[0].id)
        .expect("anchor placed")
        .center;

    // Far enough past the right edge that the outline the placer sized cannot
    // contain it, but close enough that a legal square exists out there.
    let span_mm = synth_geometry::nm_to_mm(
        automatic.board_outline.max.x_nm - automatic.board_outline.min.x_nm,
    );
    let dx = ((span_mm / 2.0 + 12.0) * 10.0).round() / 10.0;

    let dir = scratch("relative-outside");
    let sidecar = dir.join("env_logger.placement.layout.toml");
    fs::write(
        &sidecar,
        format!(
            "schema_version = 2\n\n[components.{anchor}]\nx = {:.1}\ny = {:.1}\nrotation = 0\n\n\
             [components.{follower}]\nrelative_to = \"{anchor}\"\ndx = {dx:.1}\ndy = 0.0\nrotation = 0\n",
            synth_geometry::nm_to_mm(anchor_at.x_nm),
            synth_geometry::nm_to_mm(anchor_at.y_nm),
        ),
    )
    .expect("write sidecar");

    let placement = synth_place::place_with_tuning_and_sidecar(
        &board,
        1.5,
        &std::collections::HashMap::new(),
        Some(&sidecar),
    )
    .expect("a board with a far relative entry still places");

    let anchor_final = placement
        .components
        .iter()
        .find(|p| p.id == board.components[0].id)
        .expect("anchor placed")
        .center;
    let got = placement
        .components
        .iter()
        .find(|p| p.id == board.components[1].id)
        .expect("follower placed")
        .center;
    let want_x = synth_geometry::nm_to_mm(anchor_final.x_nm) + dx;
    assert!(
        (synth_geometry::nm_to_mm(got.x_nm) - want_x).abs() < 0.001,
        "{follower} was placed {dx:.1} mm from {anchor}, which puts it at \
         {want_x:.3}, but it landed at {:.3}: the outline overruled the sidecar",
        synth_geometry::nm_to_mm(got.x_nm),
    );
}

/// The public API is the one the MCP tools place through, so it has to carry
/// the constraint too.
///
/// `place_with_sidecar` ran the automatic search first and applied the
/// sidecar to its answer, which is the bug the rest of this file is about,
/// left on the entry point that most needs it: interactive placement refused
/// a board the search could not pack before the author's coordinates were
/// read. Same fixture as the first test, through the other door.
#[test]
fn the_public_sidecar_api_places_a_fully_pinned_board_the_search_refuses() {
    let fixture = workspace_root()
        .join("fixtures")
        .join("place-errors")
        .join("no_position_01.synth");
    let board = load_board(&fixture);

    assert!(
        synth_place::place(&board).is_err(),
        "fixture is only meaningful while the automatic placer refuses it"
    );

    let dir = scratch("public-api");
    let sidecar = dir.join("no_position_01.placement.layout.toml");
    grid_sidecar(&board, &sidecar);

    let placement = synth_place::place_with_sidecar(&board, Some(&sidecar))
        .expect("a fully pinned floorplan must place through the public API too");

    assert_eq!(placement.components.len(), board.components.len());
    let (span_w, span_h) = grid_span(&board);
    assert!(
        synth_geometry::nm_to_mm(placement.board_outline.width_nm()) <= span_w
            && synth_geometry::nm_to_mm(placement.board_outline.height_nm()) <= span_h,
        "the compacted floorplan must not exceed the pinned span ({span_w} x {span_h} mm)"
    );
    for component in &board.components {
        let got = placement
            .components
            .iter()
            .find(|p| p.id == component.id)
            .expect("component placed");
        assert!(inside_outline(&placement, got.center));
    }
}
