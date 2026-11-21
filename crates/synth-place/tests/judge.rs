// SPDX-License-Identifier: Apache-2.0

//! The placement judge on real boards.
//!
//! These tests use the public `place` → `judge` path, the same way a later
//! round would: a finished placement goes in, the acceptance numbers come
//! out, and nothing moves.

use std::path::{Path, PathBuf};

use synth_geometry::Rect;
use synth_ir::Board;
use synth_place::{PlacementJudgement, judge};

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
    let registry = synth_registry::load_user_overlay(
        registry,
        &workspace_root().join("crates/synth-place/tests/fixtures/parts"),
        false,
    )
    .expect("fixture parts")
    .registry;
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board lowered")
}

fn metric<'a>(report: &'a PlacementJudgement, key: &str) -> &'a synth_place::JudgeMetric {
    report
        .metrics
        .iter()
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("missing metric {key}: {:?}", report.metrics))
}

fn part_rect(board: &Board, placed: &synth_place::ComponentPlacement) -> Rect {
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
    let half = |mm: f64| synth_geometry::mm_to_nm(mm) / 2;
    Rect::from_center_half_extents(placed.center, half(width), half(height))
}

/// The exported placement: the default path plus the fit and trim, before the
/// compression pass. PR5 restores the RePlAce input these numbers came from.
fn exported_placement(board: &Board) -> synth_place::Placement {
    let mut placement = synth_place::place(board).expect("place");
    synth_place::fit_outline_with_passive_margin_mm(
        board,
        &mut placement,
        synth_place::tight_edge_margin_mm(board),
        synth_place::passive_edge_margin_mm(board),
    );
    synth_place::trim_outline_to_grid(board, &mut placement);
    placement
}

fn rtd() -> &'static (Board, synth_place::Placement) {
    static RTD: std::sync::OnceLock<(Board, synth_place::Placement)> = std::sync::OnceLock::new();
    RTD.get_or_init(|| {
        let board = load_board("crates/synth-place/tests/fixtures/rtd_afe.synth");
        let placement = exported_placement(&board);
        (board, placement)
    })
}

fn fpv() -> &'static (Board, synth_place::Placement) {
    static FPV: std::sync::OnceLock<(Board, synth_place::Placement)> = std::sync::OnceLock::new();
    FPV.get_or_init(|| {
        let board = load_board("crates/synth-place/tests/fixtures/fpv_fc.synth");
        let placement = exported_placement(&board);
        (board, placement)
    })
}

#[test]
fn judge_reports_the_round_numbers_without_moving_the_placement() {
    let (board, placement) = fpv();
    let before = placement.clone();

    let first = judge(board, placement);
    let second = judge(board, placement);

    assert_eq!(
        placement, &before,
        "the judge must not move the placement it is given"
    );
    assert_eq!(first, second, "the judge must be deterministic");
    assert!(
        (0.0..=100.0).contains(&first.score),
        "score {}",
        first.score
    );
    assert_eq!(
        first
            .metrics
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "outline_area",
            "largest_empty",
            "alignment",
            "ring",
            "area_efficiency",
            "docking",
            "usb_offset",
            "group_gap",
            "header_alignment",
            "header_parallel",
            "overlaps",
            "outside_outline",
            "hole_corner",
        ]
    );
    for row in &first.metrics {
        assert!(
            (row.weight * row.score.unwrap_or(0.0) - row.contribution).abs() < 1e-12,
            "{} contribution",
            row.key
        );
    }
    assert_eq!(
        metric(&first, "overlaps").measured,
        0.0,
        "the placed board has no overlaps"
    );
}

#[test]
#[ignore = "round report; prints the judge on the two experiment boards"]
fn round_report() {
    for (label, pair) in [("rtd", rtd()), ("fpv", fpv())] {
        let report = judge(&pair.0, &pair.1);
        println!(
            "{label}: {}",
            serde_json::to_string(&report).expect("judgement serializes")
        );
    }
}

#[test]
fn a_header_turned_out_of_the_row_fails_header_alignment() {
    let (board, placement) = fpv();
    assert_eq!(
        metric(&judge(board, placement), "header_alignment").measured,
        0.0,
        "the placed header row starts aligned"
    );
    let header = board
        .components
        .iter()
        .find(|component| component.refdes == "J_ESC")
        .expect("J_ESC is a header");
    let mut moved = placement.clone();
    let placed = moved
        .components
        .iter_mut()
        .find(|part| part.id == header.id)
        .expect("J_ESC is placed");
    placed.rotation = if placed.rotation == synth_geometry::Rotation::Ninety {
        synth_geometry::Rotation::Zero
    } else {
        synth_geometry::Rotation::Ninety
    };

    let report = judge(board, &moved);
    let row = metric(&report, "header_alignment");

    assert!(row.measured >= 2.9, "header spread {}", row.measured);
    assert!(row.failures >= 1);
    assert!(!report.passed);
}

#[test]
fn touching_courtyards_count_as_overlaps() {
    let (board, placement) = rtd();
    let mut moved = placement.clone();
    let h1 = moved
        .components
        .iter()
        .find(|part| board.component(part.id).is_some_and(|c| c.refdes == "H1"))
        .copied()
        .expect("H1");
    let index = moved
        .components
        .iter()
        .position(|part| board.component(part.id).is_some_and(|c| c.refdes == "H2"))
        .expect("H2");
    let h1_rect = part_rect(board, &h1);
    let h2_rect = part_rect(board, &moved.components[index]);
    moved.components[index].center.x_nm += h1_rect.max.x_nm - h2_rect.min.x_nm;
    moved.components[index].center.y_nm += i64::midpoint(h1_rect.min.y_nm, h1_rect.max.y_nm)
        - i64::midpoint(h2_rect.min.y_nm, h2_rect.max.y_nm);

    let report = judge(board, &moved);

    assert!(
        metric(&report, "overlaps").measured >= 1.0,
        "a shared boundary is a collision"
    );
    assert!(!report.passed);
}

#[test]
fn an_overlapping_placement_is_rejected() {
    let (board, placement) = rtd();
    let mut dirty = placement.clone();
    dirty.components[1].center = dirty.components[0].center;

    let report = judge(board, &dirty);

    assert!(metric(&report, "overlaps").measured >= 1.0);
    assert!(!report.passed);
    assert!(report.gate_failures >= 1);
}

#[test]
fn moving_the_group_capacitors_away_empties_the_ring() {
    let (board, placement) = rtd();
    let mut moved = placement.clone();
    for component in &board.components {
        if component.kind != "capacitor" {
            continue;
        }
        if let Some(placed) = moved
            .components
            .iter_mut()
            .find(|part| part.id == component.id)
        {
            placed.center.x_nm += synth_geometry::mm_to_nm(50.0);
        }
    }

    let clean = judge(board, placement);
    let report = judge(board, &moved);
    let row = metric(&report, "ring");

    assert!(metric(&clean, "ring").measured > 0.0);
    assert_eq!(row.measured, 0.0, "no group capacitor rings its chip");
    assert!(row.score.unwrap_or(1.0) < metric(&clean, "ring").score.unwrap_or(1.0));
}

#[test]
fn moving_one_review_group_onto_another_fails_the_group_gap() {
    let (board, placement) = rtd();
    let centroid = |group: &str| {
        let ids: Vec<_> = board
            .components
            .iter()
            .filter(|component| component.group.as_deref() == Some(group))
            .map(|component| component.id)
            .collect();
        let mut x = 0_i64;
        let mut y = 0_i64;
        let mut count = 0_i64;
        for placed in &placement.components {
            if ids.contains(&placed.id) {
                x += placed.center.x_nm;
                y += placed.center.y_nm;
                count += 1;
            }
        }
        (x / count, y / count)
    };
    let (from_x, from_y) = centroid("ADC");
    let (to_x, to_y) = centroid("MCU");
    let mut moved = placement.clone();
    for component in &board.components {
        if component.group.as_deref() != Some("ADC") {
            continue;
        }
        if let Some(placed) = moved
            .components
            .iter_mut()
            .find(|part| part.id == component.id)
        {
            placed.center.x_nm += to_x - from_x;
            placed.center.y_nm += to_y - from_y;
        }
    }

    let report = judge(board, &moved);
    let row = metric(&report, "group_gap");

    assert!(row.measured < 2.0, "group gap {}", row.measured);
    assert!(row.failures >= 1);
    assert!(!report.passed);
}

#[test]
fn a_docked_connector_moved_inward_is_undocked() {
    let (board, placement) = rtd();
    let connector = board
        .components
        .iter()
        .find(|component| component.refdes == "J2")
        .expect("J2 is the PT100 header");
    let mut moved = placement.clone();
    let placed = moved
        .components
        .iter_mut()
        .find(|part| part.id == connector.id)
        .expect("J2 is placed");
    placed.center.x_nm += synth_geometry::mm_to_nm(5.0);
    placed.center.y_nm += synth_geometry::mm_to_nm(5.0);

    let report = judge(board, &moved);
    let row = metric(&report, "docking");

    assert!(row.measured < 1.0, "docking ratio {}", row.measured);
    assert!(row.failures >= 1);
}

#[test]
fn a_mounting_hole_moved_off_its_corner_fails_hole_corner() {
    let (board, placement) = rtd();
    assert_eq!(
        metric(&judge(board, placement), "hole_corner").failures,
        0,
        "the placed board has its holes in the corners"
    );
    let hole = board
        .components
        .iter()
        .find(|component| component.kind == "mounting_hole")
        .expect("rtd_afe has mounting holes");
    let mut moved = placement.clone();
    let placed = moved
        .components
        .iter_mut()
        .find(|part| part.id == hole.id)
        .expect("the hole is placed");
    placed.center.x_nm += synth_geometry::mm_to_nm(6.0);

    let report = judge(board, &moved);
    let row = metric(&report, "hole_corner");

    assert!(row.measured > 5.5, "hole corner {}", row.measured);
    assert!(row.failures >= 1);
    assert!(!report.passed);
}

#[test]
fn a_usb_connector_off_the_edge_midpoint_fails() {
    let (board, placement) = rtd();
    let usb = board
        .components
        .iter()
        .find(|component| {
            component.refdes.to_ascii_lowercase().contains("usb")
                || component.part.as_ref().is_some_and(|part| {
                    part.kicad_footprint
                        .as_deref()
                        .is_some_and(|footprint| footprint.to_ascii_lowercase().contains("usb"))
                })
        })
        .expect("rtd_afe has a USB connector");
    let mut moved = placement.clone();
    let placed = moved
        .components
        .iter_mut()
        .find(|part| part.id == usb.id)
        .expect("the USB connector is placed");
    placed.center.x_nm += synth_geometry::mm_to_nm(5.0);

    let report = judge(board, &moved);
    let row = metric(&report, "usb_offset");

    assert!(row.measured >= 5.0, "usb offset {}", row.measured);
    assert!(row.failures >= 1);
    assert!(!report.passed);
}
