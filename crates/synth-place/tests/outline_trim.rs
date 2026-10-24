// SPDX-License-Identifier: Apache-2.0

//! The export trims a fitted outline onto the shape score's 1 mm cell grid.
//!
//! The score counts dead cells over a grid anchored at the Edge.Cuts bbox
//! minimum, so an outline ending a fraction past a whole millimetre counts a
//! strip of air. The trim removes that strip without eating the 0.5 mm reserve
//! the fit left around the parts, and a connector docked to an edge owns that
//! edge.

use synth_geometry::{mm_to_nm, Point, Rect, Rotation};

fn board_from(src: &str) -> synth_ir::Board {
    let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let parsed = synth_parser::parse(src, "inline.synth");
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
    synth_ir::lower(&parsed.ast.unwrap(), &registry, "inline.synth")
        .board
        .expect("board")
}

fn placement(
    board: &synth_ir::Board,
    refdes: &str,
    center_mm: (f64, f64),
    outline_max_mm: f64,
) -> synth_place::Placement {
    let component = board
        .components
        .iter()
        .find(|component| component.refdes == refdes)
        .expect("component in board");
    synth_place::Placement {
        board_outline: Rect::new(
            Point::new(0, 0),
            Point::new(mm_to_nm(outline_max_mm), mm_to_nm(outline_max_mm)),
        ),
        components: vec![synth_place::ComponentPlacement {
            id: component.id,
            center: Point::new(mm_to_nm(center_mm.0), mm_to_nm(center_mm.1)),
            rotation: Rotation::Zero,
            layer: synth_geometry::Layer::Top,
        }],
    }
}

fn courtyard_geometry(board: &synth_ir::Board, refdes: &str) -> (f64, f64) {
    let component = board
        .components
        .iter()
        .find(|component| component.refdes == refdes)
        .expect("component in board");
    let part = component.part.as_ref().expect("resolved part");
    synth_layout::pcb_courtyard_geometry_for_part(part).1
}

#[test]
fn trailing_edges_trim_to_just_under_a_whole_millimetre() {
    let board = board_from(
        r#"board "trim" {
            layers 2
            component U1: mcu "rp2350"
        }"#,
    );
    let mut placement = placement(&board, "U1", (20.0, 20.0), 30.4);
    let before = placement.board_outline;

    synth_place::trim_outline_to_grid(&board, &mut placement);

    // 30.4 mm leaves a 0.4 mm strip, so the trim lands on 29.99 mm: the cell
    // grid then counts 29 whole columns instead of 30.
    assert_eq!(placement.board_outline.max.x_nm, mm_to_nm(30.0) - 10_000);
    assert_eq!(placement.board_outline.max.y_nm, mm_to_nm(30.0) - 10_000);
    assert_eq!(placement.board_outline.min, before.min);
}

#[test]
fn the_trim_keeps_half_a_millimetre_to_the_last_courtyard() {
    let board = board_from(
        r#"board "trim" {
            layers 2
            component U1: mcu "rp2350"
        }"#,
    );
    // The courtyard reaches 30.1 mm; the next whole millimetre is 30.7 mm, so
    // pulling the edge under it would leave only 0.1 mm instead of the reserve.
    let mut placement = placement(&board, "U1", (26.35, 26.35), 30.7);
    let before = placement.board_outline;

    synth_place::trim_outline_to_grid(&board, &mut placement);

    assert_eq!(placement.board_outline, before, "the 0.5 mm reserve wins");
}

#[test]
fn a_flush_connector_pins_its_edge_against_the_trim() {
    let board = board_from(
        r#"board "trim" {
            layers 2
            component J1: connector "header_1x3" {
                placement_hint { edge: right  priority: hard }
            }
        }"#,
    );
    let (width_mm, _height_mm) = courtyard_geometry(&board, "J1");
    let mut placement = placement(&board, "J1", (30.0 - width_mm / 2.0, 15.0), 30.0);
    synth_place::fit_outline_with_margin_mm(&board, &mut placement, 1.5);
    let fitted = placement.board_outline;

    synth_place::trim_outline_to_grid(&board, &mut placement);

    assert_eq!(
        placement.board_outline.max.x_nm, fitted.max.x_nm,
        "the connector's declared edge must not move"
    );
    let courtyard_max_x = placement.components[0].center.x_nm + mm_to_nm(width_mm / 2.0);
    assert!(
        fitted.max.x_nm - courtyard_max_x <= mm_to_nm(0.5),
        "the fitted edge keeps the connector reach"
    );
}
