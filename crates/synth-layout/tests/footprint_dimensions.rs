// SPDX-License-Identifier: Apache-2.0

//! The footprint size a part declares, derived from a footprint's text. The
//! fixtures are checked in, so none of this needs KiCad installed.

use synth_layout::kicad_footprint_loader::{
    courtyard_extent_from_text, derive_footprint_dimensions, derive_footprint_dimensions_from_text,
    pads_extent_from_text,
};

use std::f64::consts::SQRT_2;

const MARGIN: f64 = 0.25;

const SOT23: &str = include_str!("fixtures/footprints/sot23_like.kicad_mod");
const TO92_MIXED: &str = include_str!("fixtures/footprints/to92_mixed.kicad_mod");
const CIRCLE: &str = include_str!("fixtures/footprints/circle_courtyard.kicad_mod");
const POLY: &str = include_str!("fixtures/footprints/poly_courtyard.kicad_mod");
const NETTIE: &str = include_str!("fixtures/footprints/nettie_no_courtyard.kicad_mod");
const DEGENERATE: &str = include_str!("fixtures/footprints/degenerate_courtyard.kicad_mod");
const EMPTY: &str = include_str!("fixtures/footprints/empty.kicad_mod");
const ROTATED_PAD: &str = include_str!("fixtures/footprints/rotated_pad_no_courtyard.kicad_mod");
const ARC_EXTREMUM: &str = include_str!("fixtures/footprints/arc_courtyard_extremum.kicad_mod");
const ARC_CLOCKWISE: &str = include_str!("fixtures/footprints/arc_courtyard_clockwise.kicad_mod");

fn derived(text: &str) -> Result<(f64, f64), String> {
    derive_footprint_dimensions_from_text(text, MARGIN)
}

#[test]
fn the_margin_is_taken_off_the_courtyard_once() {
    // The courtyard is 3.86 x 3.4: the body plus the margin on each side.
    let (x0, y0, x1, y1) = courtyard_extent_from_text(SOT23).expect("a courtyard");
    assert!((x1 - x0 - 3.86).abs() < 1e-9 && (y1 - y0 - 3.4).abs() < 1e-9);
    assert_eq!(derived(SOT23), Ok((3.36, 2.9)));
}

#[test]
fn a_courtyard_drawn_with_a_line_and_an_arc_is_measured_whole() {
    // Its only line is horizontal, so reading lines alone gives a height of zero;
    // the arc carries the other 3.5 mm. The arc runs from (-2, -1.5) over (0, 2) to
    // (2, -1.5): its circle has radius 16.25 / 7 and reaches x = -2.32 and x = 2.32
    // on the way over, wider than the 4 mm its three points span.
    let (x0, y0, x1, y1) = courtyard_extent_from_text(TO92_MIXED).expect("a courtyard");
    let radius = 16.25 / 7.0;
    assert!((x1 - x0 - 2.0 * radius).abs() < 1e-6, "{x0} {x1}");
    assert!((y1 - y0 - 3.5).abs() < 1e-9, "{y0} {y1}");
    let (width, height) = derived(TO92_MIXED).expect("dimensions");
    // The derived size is rounded to a micron.
    assert!((width - (2.0 * radius - 0.5)).abs() < 1e-3, "{width}");
    assert!((height - 3.0).abs() < 1e-9, "{height}");
}

#[test]
fn a_circle_and_a_polygon_courtyard_are_measured() {
    assert_eq!(
        derived(CIRCLE),
        Ok((2.5, 2.5)),
        "a 1.5 mm radius is 3 mm across"
    );
    assert_eq!(
        derived(POLY),
        Ok((4.5, 2.5)),
        "5 x 3 less the margin on each side"
    );
}

#[test]
fn a_footprint_without_a_courtyard_takes_its_pads_extent() {
    assert_eq!(courtyard_extent_from_text(NETTIE), None);
    let (w, h) = pads_extent_from_text(NETTIE).expect("pads");
    assert!((w - 1.5).abs() < 1e-9 && (h - 0.5).abs() < 1e-9);
    assert_eq!(derived(NETTIE), Ok((1.5, 0.5)));
}

#[test]
fn a_degenerate_courtyard_falls_back_to_the_pads_not_to_zero() {
    // One line: a height of zero, which the registry refuses to load.
    assert_eq!(
        derived(DEGENERATE),
        Ok((3.0, 1.0)),
        "two 1 mm pads 2 mm apart"
    );
}

#[test]
fn a_footprint_with_nothing_to_measure_is_refused_with_a_reason() {
    let why = derived(EMPTY).expect_err("nothing to measure");
    assert!(
        why.contains("no courtyard") && why.contains("no pads"),
        "{why}"
    );
}

#[test]
fn an_unknown_footprint_is_refused_and_named() {
    let why = derive_footprint_dimensions("No_Such_Library:No_Such_Footprint", MARGIN)
        .expect_err("it cannot be read");
    assert!(why.contains("No_Such_Library:No_Such_Footprint"), "{why}");
    assert!(why.contains("spelling"), "{why}");
}

#[test]
fn an_arc_is_bounded_by_its_extremes_and_not_only_by_the_points_it_is_sampled_at() {
    // From -45 to 135 degrees through 45, radius 2 about the origin. The start,
    // middle and end lie at x = 1.414 at most, but the arc passes 0 degrees and
    // 90 degrees, so it reaches x = 2 and y = 2. Nothing on it goes below -1.414.
    let (x0, y0, x1, y1) = courtyard_extent_from_text(ARC_EXTREMUM).expect("a courtyard");
    let near = |a: f64, b: f64| (a - b).abs() < 1e-5;
    assert!(near(x1, 2.0) && near(y1, 2.0), "far side {x1} {y1}");
    assert!(
        near(x0, -SQRT_2) && near(y0, -SQRT_2),
        "near side {x0} {y0}"
    );
}

#[test]
fn an_arc_drawn_the_other_way_round_has_the_same_extent() {
    let (x0, y0, x1, y1) = courtyard_extent_from_text(ARC_CLOCKWISE).expect("a courtyard");
    let near = |a: f64, b: f64| (a - b).abs() < 1e-5;
    assert!(near(x1, 2.0) && near(y1, 2.0), "far side {x1} {y1}");
    assert!(
        near(x0, -SQRT_2) && near(y0, -SQRT_2),
        "near side {x0} {y0}"
    );
}

#[test]
fn a_rotated_pad_is_measured_turned_in_the_pad_fallback() {
    // A 0.5 x 2.0 mm pad turned 90 degrees covers 2.0 x 0.5 mm.
    let (w, h) = pads_extent_from_text(ROTATED_PAD).expect("a pad");
    assert!(
        (w - 2.0).abs() < 1e-9 && (h - 0.5).abs() < 1e-9,
        "{w} x {h}"
    );
    let turned_45 = ROTATED_PAD.replace("(at 0 0 90)", "(at 0 0 45)");
    let (w, h) = pads_extent_from_text(&turned_45).expect("a pad");
    // Half the width of a 0.5 x 2.0 pad turned 45 degrees is (0.25 + 1.0) / sqrt 2.
    assert!((w - 2.0 * (0.25 + 1.0) / SQRT_2).abs() < 1e-9, "{w}");
    assert!((h - w).abs() < 1e-9, "{h}");
    // An unturned pad keeps its own size, and so does a round one turned any way.
    let straight = ROTATED_PAD.replace("(at 0 0 90)", "(at 0 0)");
    assert_eq!(pads_extent_from_text(&straight), Some((0.5, 2.0)));
    let round = ROTATED_PAD.replace(
        "smd rect (at 0 0 90) (size 0.5 2.0)",
        "smd circle (at 0 0 45) (size 1.5 1.5)",
    );
    assert_eq!(pads_extent_from_text(&round), Some((1.5, 1.5)));
    // The derived size follows, with the margin taken off once.
    assert_eq!(derived(ROTATED_PAD), Ok((2.0, 0.5)));
}
