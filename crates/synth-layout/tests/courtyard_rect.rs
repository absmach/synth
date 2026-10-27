// SPDX-License-Identifier: Apache-2.0

//! The courtyard the placer keeps parts apart by. The fixtures are checked in,
//! so none of this needs KiCad installed.

use synth_layout::kicad_footprint_loader::courtyard_rect_from_text;

const SOT23: &str = include_str!("fixtures/footprints/sot23_like.kicad_mod");
const TO92_MIXED: &str = include_str!("fixtures/footprints/to92_mixed.kicad_mod");
const CIRCLE: &str = include_str!("fixtures/footprints/circle_courtyard.kicad_mod");
const POLY: &str = include_str!("fixtures/footprints/poly_courtyard.kicad_mod");
const NETTIE: &str = include_str!("fixtures/footprints/nettie_no_courtyard.kicad_mod");

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_circle_courtyard_has_the_size_of_its_circle() {
    // A radial capacitor or a mounting hole: read as no courtyard before, so the
    // placer fell back to a few millimetres and parts landed inside the part.
    let (cx, cy, w, h) = courtyard_rect_from_text(CIRCLE).expect("a courtyard");
    assert!(close(cx, 0.0) && close(cy, 0.0));
    assert!(close(w, 3.0) && close(h, 3.0), "{w} x {h}");
}

#[test]
fn a_polygon_courtyard_is_measured() {
    let (_, _, w, h) = courtyard_rect_from_text(POLY).expect("a courtyard");
    assert!(close(w, 5.0) && close(h, 3.0), "{w} x {h}");
}

#[test]
fn a_line_and_an_arc_are_measured_together_and_off_centre() {
    // The only line is horizontal, so lines alone give a height of zero.
    let (cx, cy, w, h) = courtyard_rect_from_text(TO92_MIXED).expect("a courtyard");
    assert!(close(w, 4.0) && close(h, 3.5), "{w} x {h}");
    assert!(close(cx, 0.0), "{cx}");
    assert!(
        cy > -1.5 && cy < 1.0,
        "the box is centred on what is drawn: {cy}"
    );
}

#[test]
fn a_rectangle_of_lines_is_unchanged_and_no_courtyard_is_none() {
    let (_, _, w, h) = courtyard_rect_from_text(SOT23).expect("a courtyard");
    assert!(close(w, 3.86) && close(h, 3.4), "{w} x {h}");
    assert!(courtyard_rect_from_text(NETTIE).is_none());
}
