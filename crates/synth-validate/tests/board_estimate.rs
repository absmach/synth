// SPDX-License-Identifier: Apache-2.0

//! `synth validate` estimates the board a design's parts need and warns when a
//! declared `outline` is far larger than that estimate.

use std::path::{Path, PathBuf};
use synth_diagnostics::Severity;
use synth_geometry::nm_to_mm;
use synth_ir::board::BoardOutline;
use synth_ir::{Board, Length};
use synth_validate::board_estimate::{board_size_diagnostics, estimate_board_size};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn registry() -> synth_registry::Registry {
    synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads")
}

fn lower(source: &str, file: &str) -> Board {
    let parse = synth_parser::parse(source, file.to_string());
    let ast = parse.ast.as_ref().expect("parse ok");
    synth_ir::lower(ast, &registry(), file)
        .board
        .expect("board lowered")
}

fn gateway_board() -> Board {
    let path = workspace_root()
        .join("fixtures")
        .join("designs")
        .join("gateway_estimate.synth");
    let source = std::fs::read_to_string(&path).expect("read gateway fixture");
    lower(&source, "gateway_estimate.synth")
}

/// A five-part board that places quickly; the one live placement in this
/// file.
fn dense_usb_board() -> Board {
    let path = workspace_root()
        .join("fixtures")
        .join("designs")
        .join("dense_usb_design.synth");
    let source = std::fs::read_to_string(&path).expect("read dense usb fixture");
    lower(&source, "dense_usb_design.synth")
}

/// Fitted dimensions `synth_place::place` chose for the calibration boards on
/// 2026-11-24 with stock KiCad footprints. Placement has its own tests; these
/// recorded fits pin the estimator's accuracy without re-solving four real
/// boards on every run.
const SENSOR_LOGGER_FIT_MM: (f64, f64) = (65.15, 44.25);
const SECURE_TRACKER_FIT_MM: (f64, f64) = (63.40, 59.30);
const FEATHER_M4_FIT_MM: (f64, f64) = (71.21, 47.98);
const GATEWAY_ESTIMATE_FIT_MM: (f64, f64) = (70.63, 64.94);

#[test]
fn a_declared_outline_far_larger_than_the_parts_need_warns() {
    let board = gateway_board();
    let estimate = estimate_board_size(&board);
    assert!(
        estimate.est_area_mm2 > 0.0 && 180.0 * 120.0 > 1.5 * estimate.est_area_mm2,
        "fixture must be oversized for its parts: {estimate:?}"
    );

    let diagnostics = board_size_diagnostics(&board, "gateway_estimate.synth");
    let warning = diagnostics
        .iter()
        .find(|d| d.code == "W-SYNTH-BOARD-004" && d.severity == Severity::Warning)
        .unwrap_or_else(|| panic!("no board-size warning: {diagnostics:?}"));

    let message = warning.message.as_deref().unwrap_or_default();
    assert!(message.contains("180.0 x 120.0"), "{message}");
    assert!(message.contains("estimated"), "{message}");
    assert!(message.contains("remove the outline"), "{message}");
    let expected = warning.expected.as_deref().unwrap_or_default();
    assert!(expected.contains("estimated area"), "{expected}");
    assert!(warning.found.is_some(), "{warning:?}");
    assert!(!warning.suggested_fixes.is_empty(), "{warning:?}");
}

#[test]
fn a_board_without_an_outline_reports_the_estimate_as_info() {
    let board = dense_usb_board();

    let diagnostics = board_size_diagnostics(&board, "dense_usb_design.synth");
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Warning),
        "{diagnostics:?}"
    );
    let info = diagnostics
        .iter()
        .find(|d| d.code == "W-SYNTH-BOARD-004")
        .unwrap_or_else(|| panic!("no board-size info: {diagnostics:?}"));
    assert_eq!(info.severity, Severity::Info);

    let estimate = estimate_board_size(&board);
    let placed = synth_place::place(&board).expect("the fixture's parts fit a fitted board");
    let fitted_w = nm_to_mm(placed.board_outline.max.x_nm - placed.board_outline.min.x_nm);
    let fitted_h = nm_to_mm(placed.board_outline.max.y_nm - placed.board_outline.min.y_nm);
    for (estimated, fitted, axis) in [
        (estimate.est_w_mm, fitted_w, "width"),
        (estimate.est_h_mm, fitted_h, "height"),
    ] {
        let off = (estimated - fitted).abs() / fitted;
        assert!(
            off <= 0.25,
            "estimated {axis} {estimated:.2} mm is {:.1}% off the fitted {fitted:.2} mm",
            off * 100.0
        );
    }
}

/// Both checks belong to the ERC registry, not only to the two hand-wired
/// validate entry points.
#[test]
fn the_erc_registry_reports_the_board_size_and_hint_rules() {
    let board = lower(
        r#"board "registry" {
            layers 2
            outline 180mm x 120mm
            component U1: ic "stm32g030k8_lqfp32" {
                placement_hint { region centre priority: hard }
            }
        }"#,
        "registry.synth",
    );
    let codes: Vec<String> = synth_validate::run_erc(&board, "registry.synth")
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(
        codes.iter().any(|code| code == "W-SYNTH-BOARD-004"),
        "{codes:?}"
    );
    assert!(
        codes.iter().any(|code| code == "W-SYNTH-BOARD-005"),
        "{codes:?}"
    );
}

#[test]
fn a_stated_reason_suppresses_the_oversized_outline_warning() {
    let board = lower(
        r#"board "reason" {
            layers 2
            outline 180mm x 120mm reason "fits the enclosure"
            component R1: resistor "r_generic_0603"
        }"#,
        "reason.synth",
    );
    assert_eq!(
        board
            .outline
            .as_ref()
            .and_then(|outline| outline.reason.as_deref()),
        Some("fits the enclosure")
    );
    let diagnostics = board_size_diagnostics(&board, "reason.synth");
    assert!(
        diagnostics.iter().all(|d| d.code != "W-SYNTH-BOARD-004"),
        "{diagnostics:?}"
    );
}

#[test]
fn a_tight_outline_is_not_warned_about() {
    let mut board = gateway_board();
    board.outline = None;
    let estimate = estimate_board_size(&board);
    board.outline = Some(BoardOutline {
        width: Length::from_mm(estimate.est_w_mm * 1.2),
        height: Length::from_mm(estimate.est_h_mm * 1.2),
        reason: None,
        source_span: board.source_span,
    });

    let diagnostics = board_size_diagnostics(&board, "tight.synth");
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Warning),
        "{diagnostics:?}"
    );
}

#[test]
fn empty_one_part_missing_courtyard_and_mounting_holes_do_not_panic() {
    let empty = lower("board \"empty\" { layers 2 }", "empty.synth");
    let empty_estimate = estimate_board_size(&empty);
    assert_eq!(empty_estimate.total_courtyard_mm2, 0.0);
    assert!(empty_estimate.est_w_mm.is_finite() && empty_estimate.est_w_mm > 0.0);
    assert!(empty_estimate.est_h_mm.is_finite() && empty_estimate.est_h_mm > 0.0);

    let one = lower(
        "board \"one\" { layers 2 component R1: resistor \"r_generic_0603\" }",
        "one.synth",
    );
    let one_estimate = estimate_board_size(&one);
    assert!(one_estimate.total_courtyard_mm2 > 0.0);
    assert!(one_estimate.est_area_mm2 > one_estimate.total_courtyard_mm2);

    let missing = lower(
        "board \"missing\" { layers 2 component U1: ic \"no_such_part_abc\" }",
        "missing.synth",
    );
    assert!(missing.components[0].part.is_none());
    let missing_estimate = estimate_board_size(&missing);
    // fallback_courtyard("ic") is 12 x 12 mm.
    assert!(
        (missing_estimate.total_courtyard_mm2 - 144.0).abs() < 1e-9,
        "{missing_estimate:?}"
    );

    let holes = lower(
        "board \"holes\" { layers 2 component H1: mounting_hole \"mounting_hole_m2_5\" }",
        "holes.synth",
    );
    let holes_estimate = estimate_board_size(&holes);
    assert!(holes_estimate.total_courtyard_mm2 > 0.0);
    assert!(holes_estimate.est_w_mm.is_finite());
}

#[test]
fn the_estimate_is_deterministic() {
    let board = gateway_board();
    assert_eq!(estimate_board_size(&board), estimate_board_size(&board));
}

/// The estimate's area must track the fitted outline on boards that already
/// have no `outline` statement: those are the sizes placement fits from the
/// same courtyards. The estimate is square here while the fitted board is
/// wide, so the area is the comparison the gate uses.
#[test]
fn the_estimate_matches_the_fitted_outline_on_existing_boards() {
    let mut seen = 0;
    let mut failures = Vec::new();
    for (design, (fitted_w, fitted_h)) in [
        (
            "examples/sensor_logger/sensor_logger.synth",
            SENSOR_LOGGER_FIT_MM,
        ),
        (
            "fixtures/designs/secure_tracker.synth",
            SECURE_TRACKER_FIT_MM,
        ),
        (
            "fixtures/designs/feather_m4_express.synth",
            FEATHER_M4_FIT_MM,
        ),
    ] {
        let path = workspace_root().join(design);
        let source = std::fs::read_to_string(&path).expect("read design");
        let board = lower(&source, design);
        let estimate = estimate_board_size(&board);
        let fitted_area = fitted_w * fitted_h;
        println!(
            "{design}: estimate {:.1} x {:.1} ({:.0} mm2), fitted {:.1} x {:.1} ({:.0} mm2), \
             off {:.1}% / {:.1}%",
            estimate.est_w_mm,
            estimate.est_h_mm,
            estimate.est_area_mm2,
            fitted_w,
            fitted_h,
            fitted_area,
            (estimate.est_w_mm - fitted_w).abs() / fitted_w * 100.0,
            (estimate.est_h_mm - fitted_h).abs() / fitted_h * 100.0,
        );
        let area_off = (estimate.est_area_mm2 - fitted_area).abs() / fitted_area;
        if area_off > 0.25 {
            failures.push(format!(
                "{design}: estimated area {:.0} mm2 is {:.1}% off the fitted {:.0} mm2 \
                 ({fitted_w:.1} x {fitted_h:.1})",
                estimate.est_area_mm2,
                area_off * 100.0,
                fitted_area
            ));
        }
        seen += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(seen, 3);
}

/// A declared 3:1 rectangle gets a 3:1 estimate of the same area, so a long
/// thin board is not judged against a square.
#[test]
fn a_declared_aspect_reshapes_the_estimate_without_changing_its_area() {
    let board = lower(
        r#"board "wide" {
            layers 2
            outline 120mm x 40mm
            component R1: resistor "r_generic_0603"
            component R2: resistor "r_generic_0603"
            component C1: capacitor "c_generic_0603"
        }"#,
        "wide.synth",
    );
    let shaped = estimate_board_size(&board);
    let mut square_board = board.clone();
    square_board.outline = None;
    let square = estimate_board_size(&square_board);

    assert!(
        (shaped.est_area_mm2 - square.est_area_mm2).abs() < 1e-6,
        "the reshape must keep the area: {} vs {}",
        shaped.est_area_mm2,
        square.est_area_mm2
    );
    assert!(
        (shaped.est_w_mm / shaped.est_h_mm - 3.0).abs() < 1e-6,
        "estimated aspect {:.3} is not 3:1",
        shaped.est_w_mm / shaped.est_h_mm
    );
}

/// The warning threshold compares areas, so the reshape must not change
/// whether the oversized outline is reported.
#[test]
fn the_area_gate_is_unchanged_by_the_aspect() {
    let board = gateway_board();
    let shaped = estimate_board_size(&board);
    let mut square_board = board.clone();
    square_board.outline = None;
    let square = estimate_board_size(&square_board);

    assert!((shaped.est_area_mm2 - square.est_area_mm2).abs() < 1e-6);
    assert!(
        board_size_diagnostics(&board, "gateway_estimate.synth")
            .iter()
            .any(|d| d.code == "W-SYNTH-BOARD-004" && d.severity == Severity::Warning)
    );
}

/// The gateway-shaped fixture's estimate, at its 1.5:1 declared aspect, must
/// stay within 25% of the board the same parts fit without the outline.
#[test]
fn the_gateway_fixture_estimate_tracks_its_fitted_board() {
    let board = gateway_board();
    let estimate = estimate_board_size(&board);
    assert!(
        (estimate.est_w_mm / estimate.est_h_mm - 1.5).abs() < 1e-6,
        "gateway fixture aspect {:.3}",
        estimate.est_w_mm / estimate.est_h_mm
    );

    let (fitted_w, fitted_h) = GATEWAY_ESTIMATE_FIT_MM;
    let fitted_area = fitted_w * fitted_h;
    let off = (estimate.est_area_mm2 - fitted_area).abs() / fitted_area;
    assert!(
        off <= 0.25,
        "estimate {:.0} mm2 is {:.1}% off the fitted {:.0} mm2 ({fitted_w:.1} x {fitted_h:.1})",
        estimate.est_area_mm2,
        off * 100.0,
        fitted_area
    );
}

#[test]
fn the_info_says_the_estimate_is_at_a_square_aspect() {
    let mut board = gateway_board();
    board.outline = None;
    let diagnostics = board_size_diagnostics(&board, "square.synth");
    let info = diagnostics
        .iter()
        .find(|d| d.code == "W-SYNTH-BOARD-004")
        .expect("board-size info");
    assert!(
        info.message
            .as_deref()
            .unwrap_or_default()
            .contains("square aspect"),
        "{:?}",
        info.message
    );
}

/// A hand-built outline with no height (the parser rejects those) falls back
/// to the square estimate instead of dividing by zero.
#[test]
fn a_zero_height_outline_falls_back_to_a_square_estimate() {
    let mut board = gateway_board();
    board.outline = Some(BoardOutline {
        width: Length::from_mm(60.0),
        height: Length(0),
        reason: None,
        source_span: board.source_span,
    });
    let estimate = estimate_board_size(&board);
    assert!(
        (estimate.est_w_mm - estimate.est_h_mm).abs() < 1e-9,
        "{estimate:?}"
    );
    assert!(estimate.est_area_mm2 > 0.0);
}
