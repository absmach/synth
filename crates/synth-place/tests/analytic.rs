// SPDX-License-Identifier: Apache-2.0

//! Analytical seed A/B against the current placement on real boards.
//!
//! The seed is a region-hint producer, so these tests use the public
//! `place_with_analytic_seed` entry point and assert the placement contract:
//! the same parts, inside the outline, no courtyard overlap, deterministic.
//! HPWL is printed, not asserted: the prototype showed HPWL alone is not the
//! gate; the router comparison lives in `synth-route`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use synth_geometry::{Rect, mm_to_nm};
use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(relative: &str) -> Board {
    let path = workspace_root().join(relative);
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

fn fixtures() -> [&'static str; 4] {
    [
        "fixtures/designs/ten_caps.synth",
        "fixtures/designs/secure_tracker.synth",
        "examples/sensor_logger/sensor_logger.synth",
        "fixtures/designs/feather_m4_express.synth",
    ]
}

type PlacementCandidate = fn(&Board) -> Result<synth_place::Placement, synth_place::PlaceError>;

type PlaceWithSidecar =
    fn(&Board, Option<&std::path::Path>) -> Result<synth_place::Placement, synth_place::PlaceError>;

fn candidates() -> [(&'static str, PlacementCandidate); 2] {
    [
        ("unspaced", synth_place::place_with_analytic_seed),
        ("spaced", synth_place::place_with_analytic_seed_spaced),
    ]
}

#[allow(clippy::similar_names)]
fn courtyard_rects(board: &Board, placement: &synth_place::Placement) -> Vec<Rect> {
    placement
        .components
        .iter()
        .map(|placed| {
            let (width_mm, height_mm) = board
                .component(placed.id)
                .and_then(|component| component.part.as_ref())
                .map_or((10.0, 10.0), |part| {
                    synth_layout::pcb_courtyard_geometry_for_part(part).1
                });
            let width_nm = mm_to_nm(width_mm);
            let height_nm = mm_to_nm(height_mm);
            let (half_w, half_h) = if placed.rotation.swaps_extents() {
                (height_nm / 2, width_nm / 2)
            } else {
                (width_nm / 2, height_nm / 2)
            };
            Rect::from_center_half_extents(placed.center, half_w, half_h)
        })
        .collect()
}

fn assert_legal(board: &Board, placement: &synth_place::Placement, label: &str) {
    assert_eq!(
        placement.components.len(),
        board.components.len(),
        "{label}: every component must be placed"
    );
    let rects = courtyard_rects(board, placement);
    let outline = placement.board_outline;
    for (placed, rect) in placement.components.iter().zip(&rects) {
        assert!(
            rect.min.x_nm >= outline.min.x_nm
                && rect.max.x_nm <= outline.max.x_nm
                && rect.min.y_nm >= outline.min.y_nm
                && rect.max.y_nm <= outline.max.y_nm,
            "{label}: {:?} courtyard outside the outline",
            placed.id
        );
    }
    for first in 0..rects.len() {
        for second in (first + 1)..rects.len() {
            assert!(
                !rects[first].intersects(&rects[second]),
                "{label}: {:?} overlaps {:?}",
                placement.components[first].id,
                placement.components[second].id
            );
        }
    }

    let review = synth_place::describe_placement(board, placement).visual_review;
    assert!(
        !review.requires_revision,
        "{label}: visual review requires revision: {:?}",
        review.findings
    );
}

#[test]
fn analytic_seed_places_real_boards_legally() {
    for (label, place) in candidates() {
        for fixture in fixtures() {
            let board = load_board(fixture);
            let placement = place(&board)
                .unwrap_or_else(|error| panic!("{fixture} ({label}): placement failed: {error:?}"));
            assert_legal(&board, &placement, &format!("{fixture} ({label})"));
        }
    }
}

#[test]
fn analytic_seed_is_deterministic() {
    for (label, place) in candidates() {
        for fixture in [
            "fixtures/designs/ten_caps.synth",
            "fixtures/designs/secure_tracker.synth",
        ] {
            let board = load_board(fixture);
            let first = place(&board).expect("first placement");
            let second = place(&board).expect("second placement");
            assert_eq!(
                first, second,
                "{fixture} ({label}): repeated placement must be identical"
            );
        }
    }
}

#[test]
fn analytic_seed_honours_an_absolute_sidecar_pin() {
    assert_sidecar_pin(
        synth_place::place_with_analytic_seed_and_sidecar,
        "unspaced",
    );
}

#[test]
fn spaced_analytic_seed_honours_an_absolute_sidecar_pin() {
    assert_sidecar_pin(
        synth_place::place_with_analytic_seed_spaced_and_sidecar,
        "spaced",
    );
}

fn assert_sidecar_pin(place: PlaceWithSidecar, label: &str) {
    let board = load_board("fixtures/designs/ten_caps.synth");
    let first = board.components[0].refdes.clone();
    let dir = std::env::temp_dir().join(format!(
        "synth-analytic-sidecar-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let sidecar = dir.join("ten_caps.placement.layout.toml");
    std::fs::write(
        &sidecar,
        format!("schema_version = 2\n\n[components.{first}]\nx = 20.0\ny = 20.0\nrotation = 0\n"),
    )
    .expect("write sidecar");

    let placement = place(&board, Some(&sidecar))
        .unwrap_or_else(|error| panic!("{label}: sidecar placement failed: {error:?}"));
    let placed = placement
        .components
        .iter()
        .find(|placed| placed.id == board.components[0].id)
        .expect("pinned component placed");
    assert!(
        (synth_geometry::nm_to_mm(placed.center.x_nm) - 20.0).abs() < 0.01
            && (synth_geometry::nm_to_mm(placed.center.y_nm) - 20.0).abs() < 0.01,
        "{label}: pinned {first} moved to ({}, {})",
        synth_geometry::nm_to_mm(placed.center.x_nm),
        synth_geometry::nm_to_mm(placed.center.y_nm)
    );
    assert_legal(
        &board,
        &placement,
        &format!("ten_caps ({label}) with a pinned sidecar"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analytic_seed_covers_the_same_parts_as_the_default_placement() {
    for fixture in fixtures() {
        let board = load_board(fixture);
        let default = synth_place::place(&board).expect("default placement");
        let analytic = synth_place::place_with_analytic_seed(&board).expect("analytic placement");
        let default_parts: BTreeSet<_> = default
            .components
            .iter()
            .map(|placed| (placed.id, placed.layer))
            .collect();
        let analytic_parts: BTreeSet<_> = analytic
            .components
            .iter()
            .map(|placed| (placed.id, placed.layer))
            .collect();
        assert_eq!(
            default_parts, analytic_parts,
            "{fixture}: component set changed"
        );
    }
}

#[test]
fn analytic_seed_hpwl_report() {
    for fixture in fixtures() {
        let board = load_board(fixture);
        let default = synth_place::place(&board).expect("default placement");
        let analytic = synth_place::place_with_analytic_seed(&board).expect("analytic placement");
        let spaced =
            synth_place::place_with_analytic_seed_spaced(&board).expect("spaced placement");
        let default_hpwl = synth_place::score::score_placement(&board, &default).hpwl_total_nm;
        let analytic_hpwl = synth_place::score::score_placement(&board, &analytic).hpwl_total_nm;
        let spaced_hpwl = synth_place::score::score_placement(&board, &spaced).hpwl_total_nm;
        println!(
            "{fixture}: default {:.1} mm HPWL, analytic {:.1} mm, spaced {:.1} mm",
            synth_geometry::nm_to_mm(default_hpwl),
            synth_geometry::nm_to_mm(analytic_hpwl),
            synth_geometry::nm_to_mm(spaced_hpwl)
        );
    }
}
