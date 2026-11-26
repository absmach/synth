// SPDX-License-Identifier: Apache-2.0

//! Round-22 pull fix: non-MCU groups slide toward the MCU until the 2 mm group
//! gap, stopping at any courtyard, hole keep-out or docked connector.

use std::path::{Path, PathBuf};

use synth_geometry::{Layer, Point, Rect, Rotation, mm_to_nm, nm_to_mm};
use synth_ir::Board;
use synth_place::{ComponentPlacement, Placement};

const SOURCE: &str = r#"
board "pull_groups" {
  layers 2

  group "MCU" {
    component U1: mcu "atmega328p"
  }
  group "SIDE" {
    component R1: resistor "r_generic_0603"
  }
  group "TOP" {
    component J2: connector "header_1x4"
    component R2: resistor "r_generic_0603"
  }
  component J1: connector "header_1x4" { placement_hint { edge: top priority: hard } }
}
"#;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load(source: &str) -> Board {
    let file = "pull_groups.synth".to_string();
    let parse = synth_parser::parse(source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ok");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    synth_ir::lower(&resolved.program, &registry, &file)
        .board
        .expect("board lowered")
}

fn placement_with(board: &Board, entries: &[(&str, f64, f64)]) -> Placement {
    Placement {
        board_outline: Rect::new(
            Point::new(0, 0),
            Point::new(mm_to_nm(100.0), mm_to_nm(100.0)),
        ),
        components: entries
            .iter()
            .map(|&(refdes, x_mm, y_mm)| {
                let component = board
                    .components
                    .iter()
                    .find(|component| component.refdes == refdes)
                    .unwrap_or_else(|| panic!("missing {refdes}"));
                ComponentPlacement {
                    id: component.id,
                    center: Point::new(mm_to_nm(x_mm), mm_to_nm(y_mm)),
                    rotation: Rotation::Zero,
                    layer: Layer::Top,
                }
            })
            .collect(),
    }
}

fn courtyard(board: &Board, placed: &ComponentPlacement) -> Rect {
    let component = board.component(placed.id).expect("component");
    let (width, height) = component.part.as_ref().map_or_else(
        || synth_place::fallback_courtyard(&component.kind),
        |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
    );
    let (width, height) = if placed.rotation.swaps_extents() {
        (height, width)
    } else {
        (width, height)
    };
    let half = |mm: f64| mm_to_nm(mm) / 2;
    Rect::from_center_half_extents(placed.center, half(width), half(height))
}

fn rect_of(board: &Board, placement: &Placement, refdes: &str) -> Rect {
    let placed = placement
        .components
        .iter()
        .find(|placed| {
            board
                .component(placed.id)
                .is_some_and(|c| c.refdes == refdes)
        })
        .unwrap_or_else(|| panic!("{refdes} not placed"));
    courtyard(board, placed)
}

fn gap_x(a: Rect, b: Rect) -> i64 {
    (b.min.x_nm - a.max.x_nm).max(a.min.x_nm - b.max.x_nm)
}

/// Shift `moving` along x until it is `gap_mm` from `target`.
fn set_gap_x(board: &Board, placement: &mut Placement, moving: &str, target: &str, gap_mm: f64) {
    let gap = gap_x(
        rect_of(board, placement, moving),
        rect_of(board, placement, target),
    );
    let delta = mm_to_nm(gap_mm) - gap;
    let right =
        rect_of(board, placement, moving).min.x_nm > rect_of(board, placement, target).max.x_nm;
    let index = placement
        .components
        .iter()
        .position(|placed| {
            board
                .component(placed.id)
                .is_some_and(|c| c.refdes == moving)
        })
        .expect("moving part placed");
    let center = placement.components[index].center;
    placement.components[index].center = Point::new(
        center.x_nm + if right { delta } else { -delta },
        center.y_nm,
    );
}

/// The tight outline a finished placement has, with `rim` around every part.
fn tight_outline(board: &Board, placement: &mut Placement, rim: i64) -> Rect {
    let mut min = Point::new(i64::MAX, i64::MAX);
    let mut max = Point::new(i64::MIN, i64::MIN);
    for placed in &placement.components {
        let rect = courtyard(board, placed);
        min.x_nm = min.x_nm.min(rect.min.x_nm);
        min.y_nm = min.y_nm.min(rect.min.y_nm);
        max.x_nm = max.x_nm.max(rect.max.x_nm);
        max.y_nm = max.y_nm.max(rect.max.y_nm);
    }
    Rect::new(
        Point::new(min.x_nm - rim, min.y_nm - rim),
        Point::new(max.x_nm + rim, max.y_nm + rim),
    )
}

#[test]
fn a_group_eight_mm_from_the_mcu_ends_two_mm_away() {
    let board = load(SOURCE);
    let mut placement = placement_with(&board, &[("U1", 30.0, 30.0), ("R1", 50.0, 20.0)]);
    set_gap_x(&board, &mut placement, "R1", "U1", 8.0);
    placement.board_outline = tight_outline(&board, &mut placement, mm_to_nm(1.5));
    let before = gap_x(
        rect_of(&board, &placement, "R1"),
        rect_of(&board, &placement, "U1"),
    );
    assert!(
        (nm_to_mm(before) - 8.0).abs() < 1e-3,
        "starts {before} nm away"
    );

    synth_place::pull_groups_inward(&board, &mut placement);

    let after = gap_x(
        rect_of(&board, &placement, "R1"),
        rect_of(&board, &placement, "U1"),
    );
    assert!(
        (nm_to_mm(after) - 2.0).abs() <= 1e-3,
        "group ended {} mm from the MCU",
        nm_to_mm(after)
    );
}

#[test]
fn a_group_blocked_by_a_connector_stops_at_the_connector() {
    let board = load(SOURCE);
    let mut placement = placement_with(
        &board,
        &[("U1", 30.0, 30.0), ("R1", 55.0, 6.0), ("J1", 42.0, 6.0)],
    );
    // Flush on the top wall, where the wall compression keeps it docked.
    let j1_index = placement
        .components
        .iter()
        .position(|placed| board.component(placed.id).is_some_and(|c| c.refdes == "J1"))
        .expect("J1 placed");
    let j1_lift = rect_of(&board, &placement, "J1").min.y_nm;
    placement.components[j1_index].center.y_nm -= j1_lift;
    set_gap_x(&board, &mut placement, "R1", "U1", 10.0);
    // The connector sits between the group and the MCU, so it is the first
    // obstacle on the pull path.
    set_gap_x(&board, &mut placement, "J1", "R1", 5.0);
    let mut min = Point::new(i64::MAX, i64::MAX);
    let mut max = Point::new(i64::MIN, i64::MIN);
    for placed in &placement.components {
        let rect = courtyard(&board, placed);
        min.x_nm = min.x_nm.min(rect.min.x_nm);
        min.y_nm = min.y_nm.min(rect.min.y_nm);
        max.x_nm = max.x_nm.max(rect.max.x_nm);
        max.y_nm = max.y_nm.max(rect.max.y_nm);
    }
    // Room on the right for the wall compression to follow the pulled group.
    placement.board_outline = Rect::new(
        Point::new(
            min.x_nm - mm_to_nm(5.0),
            rect_of(&board, &placement, "J1").min.y_nm,
        ),
        Point::new(max.x_nm + mm_to_nm(20.0), max.y_nm + mm_to_nm(5.0)),
    );
    let connector = rect_of(&board, &placement, "J1");

    synth_place::pull_groups_inward(&board, &mut placement);

    let blocked = gap_x(
        rect_of(&board, &placement, "R1"),
        rect_of(&board, &placement, "J1"),
    );
    assert!(
        (nm_to_mm(blocked) - 2.0).abs() <= 1e-3,
        "group ended {} mm from the connector",
        nm_to_mm(blocked)
    );
    let now = rect_of(&board, &placement, "J1");
    assert_eq!(
        (now.min.x_nm, now.max.x_nm),
        (connector.min.x_nm, connector.max.x_nm),
        "the blocking connector must not move along the pull axis"
    );
}

#[test]
fn a_group_already_two_mm_away_does_not_move() {
    let board = load(SOURCE);
    let mut placement = placement_with(&board, &[("U1", 30.0, 30.0), ("R1", 50.0, 20.0)]);
    set_gap_x(&board, &mut placement, "R1", "U1", 2.0);
    placement.board_outline = tight_outline(&board, &mut placement, mm_to_nm(1.5));
    let before = placement.clone();

    synth_place::pull_groups_inward(&board, &mut placement);

    assert_eq!(placement, before, "a group at its gap must not move");
}

#[test]
fn the_mcu_and_a_docked_connector_do_not_move() {
    let board = load(SOURCE);
    // TOP sits above the MCU, so its axis of separation is y. Its connector is
    // flush on the top wall: pulling the group down would take it off the wall.
    let mut placement = placement_with(
        &board,
        &[("U1", 30.0, 45.0), ("J2", 30.0, 4.0), ("R2", 30.0, 9.0)],
    );
    let outline = tight_outline(&board, &mut placement, mm_to_nm(1.5));
    placement.board_outline = Rect::new(
        Point::new(outline.min.x_nm, rect_of(&board, &placement, "J2").min.y_nm),
        outline.max,
    );
    let before = placement.clone();

    synth_place::pull_groups_inward(&board, &mut placement);

    assert_eq!(
        placement, before,
        "the MCU and the wall-docked connector must stay put"
    );
}
