// SPDX-License-Identifier: Apache-2.0

//! A placement hint can carry a rotation (absmach/synth#118). The placement
//! review recommends `rotate_connector_parallel_to_edge` with a suggested
//! angle, and before this only a layout override could apply it.

use std::path::{Path, PathBuf};

use synth_geometry::Rotation;
use synth_ir::Board;
use synth_place::ExternalHint;

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
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads");
    synth_ir::lower(parse.ast.as_ref().expect("parse ok"), &registry, &file)
        .board
        .expect("board lowered")
}

fn rotation_of(board: &Board, placement: &synth_place::Placement, refdes: &str) -> Rotation {
    let id = board
        .components
        .iter()
        .find(|c| c.refdes == refdes)
        .expect("component")
        .id;
    placement
        .components
        .iter()
        .find(|p| p.id == id)
        .expect("placed")
        .rotation
}

fn degrees(rotation: Rotation) -> u32 {
    match rotation {
        Rotation::Zero => 0,
        Rotation::Ninety => 90,
        Rotation::OneEighty => 180,
        Rotation::TwoSeventy => 270,
    }
}

#[test]
fn a_hint_rotation_is_the_rotation_the_connector_gets() {
    let board = load_board("examples/placement_and_diff_pair/placement_and_diff_pair.synth");
    let edge = ExternalHint {
        component: Some("J1".into()),
        edge: Some("left".into()),
        priority: Some("hard".into()),
        ..ExternalHint::default()
    };
    let (unturned, _) = synth_place::place_with_hints(&board, std::slice::from_ref(&edge))
        .expect("places with the edge hint");
    // Half a turn keeps the courtyard's extents, so the request is legal
    // wherever the placer put the connector; the edge-facing pass would
    // otherwise turn it back.
    let wanted = (degrees(rotation_of(&board, &unturned, "J1")) + 180) % 360;

    let turned_hint = ExternalHint {
        rotation: Some(wanted),
        ..edge
    };
    let (turned, _) = synth_place::place_with_hints(&board, &[turned_hint])
        .expect("places with the rotated hint");
    assert_eq!(degrees(rotation_of(&board, &turned, "J1")), wanted);
}

#[test]
fn a_hint_without_a_rotation_places_as_before() {
    let board = load_board("examples/placement_and_diff_pair/placement_and_diff_pair.synth");
    let hint = ExternalHint {
        component: Some("J1".into()),
        edge: Some("left".into()),
        priority: Some("hard".into()),
        ..ExternalHint::default()
    };
    let (with_hint, _) =
        synth_place::place_with_hints(&board, &[hint]).expect("places with the hint");
    let (again, _) = synth_place::place_with_hints(
        &board,
        &[ExternalHint {
            rotation: None,
            component: Some("J1".into()),
            edge: Some("left".into()),
            priority: Some("hard".into()),
            ..ExternalHint::default()
        }],
    )
    .expect("places again");
    assert_eq!(with_hint, again, "no rotation leaves the placement alone");
}

/// Following the advice is a sidecar write. The external review states each
/// suggested position in the sidecar's space, the footprint origin, so a part
/// written where it is told stays put rather than shifting by its courtyard
/// offset.
#[test]
fn the_external_review_suggests_positions_as_footprint_origins() {
    let board = load_board("examples/placement_and_diff_pair/placement_and_diff_pair.synth");
    let placement = synth_place::place(&board).expect("places");
    let internal = synth_place::describe_placement(&board, &placement);
    let external = synth_place::describe_placement_external(&board, &placement);
    for (inside, outside) in internal
        .visual_review
        .recommended_actions
        .iter()
        .zip(&external.visual_review.recommended_actions)
    {
        let (Some(x), Some(y)) = (inside.suggested_x_mm, inside.suggested_y_mm) else {
            assert_eq!(outside.suggested_x_mm, None);
            continue;
        };
        let component = board
            .components
            .iter()
            .find(|c| c.refdes == inside.refdes)
            .expect("component");
        let rotation = match inside.suggested_rotation_deg {
            Some(90) => Rotation::Ninety,
            Some(180) => Rotation::OneEighty,
            Some(270) => Rotation::TwoSeventy,
            Some(_) => Rotation::Zero,
            None => rotation_of(&board, &placement, &inside.refdes),
        };
        let origin = synth_place::footprint_origin(
            component.part.as_ref(),
            &synth_place::ComponentPlacement {
                id: component.id,
                center: synth_geometry::Point::new(
                    synth_geometry::mm_to_nm(x),
                    synth_geometry::mm_to_nm(y),
                ),
                rotation,
                layer: synth_geometry::Layer::Top,
            },
        );
        assert_eq!(
            (outside.suggested_x_mm, outside.suggested_y_mm),
            (
                Some(synth_geometry::nm_to_mm(origin.x_nm)),
                Some(synth_geometry::nm_to_mm(origin.y_nm))
            ),
            "{}",
            inside.refdes
        );
    }
}
