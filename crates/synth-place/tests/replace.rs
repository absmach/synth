// SPDX-License-Identifier: Apache-2.0

//! RePlAce placement contract on real boards.

#![allow(clippy::similar_names)] // Paired w/h in mm and nm are intentional.

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
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board")
}

fn courtyard_rects(board: &Board, placement: &synth_place::Placement) -> Vec<Rect> {
    placement
        .components
        .iter()
        .map(|placed| {
            let (w_mm, h_mm) = board
                .component(placed.id)
                .and_then(|component| component.part.as_ref())
                .map_or((10.0, 10.0), |part| {
                    synth_layout::pcb_courtyard_geometry_for_part(part).1
                });
            let w_nm = mm_to_nm(w_mm);
            let h_nm = mm_to_nm(h_mm);
            let (half_w, half_h) = if placed.rotation.swaps_extents() {
                (h_nm / 2, w_nm / 2)
            } else {
                (w_nm / 2, h_nm / 2)
            };
            Rect::from_center_half_extents(placed.center, half_w, half_h)
        })
        .collect()
}

fn assert_legal(board: &Board, placement: &synth_place::Placement, label: &str) {
    let rects = courtyard_rects(board, placement);
    let outline = placement.board_outline;
    for (placed, rect) in placement.components.iter().zip(&rects) {
        assert!(
            rect.min.x_nm >= outline.min.x_nm
                && rect.max.x_nm <= outline.max.x_nm
                && rect.min.y_nm >= outline.min.y_nm
                && rect.max.y_nm <= outline.max.y_nm,
            "{label}: {:?} outside the outline",
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
        "{label}: visual review: {:?}",
        review.findings
    );
}

#[test]
fn replace_places_real_boards_legally() {
    for fixture in [
        "examples/sensor_logger/sensor_logger.synth",
        "fixtures/designs/secure_tracker.synth",
    ] {
        let board = load_board(fixture);
        let default = synth_place::place(&board).expect("default");
        let replaced = synth_place::place_with_replace(&board).expect("replace");
        assert_legal(&board, &replaced, fixture);
        let default_parts: std::collections::BTreeSet<_> = default
            .components
            .iter()
            .map(|placed| (placed.id, placed.layer))
            .collect();
        let replaced_parts: std::collections::BTreeSet<_> = replaced
            .components
            .iter()
            .map(|placed| (placed.id, placed.layer))
            .collect();
        assert_eq!(default_parts, replaced_parts, "{fixture}: component set");
    }
}

#[test]
fn replace_is_deterministic() {
    let board = load_board("fixtures/designs/secure_tracker.synth");
    let first = synth_place::place_with_replace(&board).expect("first");
    let second = synth_place::place_with_replace(&board).expect("second");
    assert_eq!(
        first, second,
        "repeated RePlAce placement must be identical"
    );
}
