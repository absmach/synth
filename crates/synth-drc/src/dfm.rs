// SPDX-License-Identifier: Apache-2.0

//! Manufacturability findings read from an exported `.kicad_pcb`.
//!
//! The checks here mirror what a fab's own DFM tool reports on the same
//! board, graded the way that tool grades them: a `Danger` finding is one the
//! fab will reject, a `Warning` is borderline. They read the board file, not
//! the in-memory routing model, because silkscreen only exists in the file
//! the fab receives. Silk lines, arcs, circles, rectangles and polygons are
//! measured; text is counted but not measured, since there is no glyph
//! geometry to check.

// Board coordinates are far below f64's 52-bit mantissa (a 100 mm board spans
// 1e8 nm), so the distance maths here loses nothing converting i64 to f64.
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeMap;

use crate::pcb_read::NM_PER_MM;
use crate::pcb_read::{Pad, PcbBoard, PcbParseError, Point};
use crate::ManufacturerProfile;

/// How a fab's DFM tool would grade a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DfmSeverity {
    /// The fab rejects or cannot build this.
    Danger,
    /// Borderline: buildable, but at risk.
    Warning,
}

/// One manufacturability finding.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DfmFinding {
    /// Stable rule name, e.g. `silk_to_pad`.
    pub rule: &'static str,
    pub severity: DfmSeverity,
    pub message: String,
    /// Board location in millimetres.
    pub at_mm: (f64, f64),
    /// The clearance found, in millimetres.
    pub measured_mm: f64,
    /// The clearance the profile requires, in millimetres.
    pub required_mm: f64,
}

/// Every finding for one board.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct DfmReport {
    pub findings: Vec<DfmFinding>,
    /// Visible silkscreen text the checks could not measure. A clean report
    /// does not cover these.
    pub unchecked_silk_text: usize,
}

impl DfmReport {
    /// Findings the fab will reject. These block a release.
    #[must_use]
    pub fn danger_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == DfmSeverity::Danger)
            .count()
    }

    /// Finding counts per rule and severity, in a stable order.
    #[must_use]
    pub fn counts(&self) -> BTreeMap<(&'static str, DfmSeverity), usize> {
        let mut counts = BTreeMap::new();
        for f in &self.findings {
            *counts.entry((f.rule, f.severity)).or_insert(0) += 1;
        }
        counts
    }
}

/// Run every DFM rule over the text of a `.kicad_pcb`.
///
/// # Errors
///
/// Returns the parse error when the text is not a readable board. An
/// unreadable board is never reported as clean.
pub fn check_dfm(
    pcb_text: &str,
    profile: &ManufacturerProfile,
) -> Result<DfmReport, PcbParseError> {
    let board = PcbBoard::parse(pcb_text)?;
    let mut findings = silk_to_pad(&board, profile);
    findings.extend(silk_to_hole(&board, profile));
    findings.extend(silk_line_width(&board, profile));
    Ok(DfmReport {
        findings,
        unchecked_silk_text: board.silk_text_count(),
    })
}

/// Spacing between samples taken along a silk stroke, in nanometres. Small
/// against the 0.15 mm floors the rules check, so the error it adds is
/// negligible.
const SAMPLE_STEP_NM: f64 = 20_000.0;

/// Silkscreen printed too close to, or over, a pad on the same side of the
/// board. Ink on a pad stops solder wetting it. One finding per shape and pad,
/// at the closest point, however many strokes trace the shape.
fn silk_to_pad(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_silk_to_pad_nm as f64;
    let mut findings = Vec::new();
    for shape in board.silk_shapes() {
        let copper = if shape.layer == "F.SilkS" {
            "F.Cu"
        } else {
            "B.Cu"
        };
        let half_width = shape.width_nm as f64 / 2.0;
        for pad in board
            .pads()
            .filter(|p| p.layers.iter().any(|l| l == copper || l == "*.Cu"))
        {
            let (hx, hy) = (pad.size_nm.0 as f64 / 2.0, pad.size_nm.1 as f64 / 2.0);
            let reach = hx.hypot(hy) + half_width + required_nm;
            let closest = shape
                .strokes
                .iter()
                .filter(|stroke| distance_to_segment_nm(pad.at, stroke) <= reach)
                .map(|stroke| nearest_sample(pad, stroke))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((distance, at)) = closest else {
                continue;
            };
            let gap_nm = (distance - half_width).max(0.0);
            if gap_nm < required_nm {
                findings.push(DfmFinding {
                    rule: "silk_to_pad",
                    severity: DfmSeverity::Danger,
                    message: format!(
                        "silkscreen {:.3} mm from pad {}, under the {:.3} mm minimum",
                        gap_nm / NM_PER_MM,
                        pad.id(),
                        required_nm / NM_PER_MM
                    ),
                    at_mm: (at.x_mm(), at.y_mm()),
                    measured_mm: gap_nm / NM_PER_MM,
                    required_mm: required_nm / NM_PER_MM,
                });
            }
        }
    }
    findings
}

/// Silkscreen printed too close to a drilled hole, on either side: a hole
/// goes through the board, so ink on the back reaches it too. The hole is a
/// circle of the pad's larger drill dimension. One finding per shape and hole.
fn silk_to_hole(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_silk_to_hole_nm as f64;
    let holes: Vec<(Point, f64, String)> = board
        .vias
        .iter()
        .filter(|via| via.drill_nm > 0)
        .map(|via| {
            let label = format!("via at ({:.3}, {:.3}) mm", via.at.x_mm(), via.at.y_mm());
            (via.at, via.drill_nm as f64 / 2.0, label)
        })
        .chain(
            board
                .pads()
                .filter(|pad| pad.drill_nm.0 > 0 || pad.drill_nm.1 > 0)
                .map(|pad| {
                    let radius = pad.drill_nm.0.max(pad.drill_nm.1) as f64 / 2.0;
                    (pad.at, radius, format!("pad {}", pad.id()))
                }),
        )
        .collect();
    let mut findings = Vec::new();
    for shape in board.silk_shapes() {
        let half_width = shape.width_nm as f64 / 2.0;
        for (centre, radius, label) in &holes {
            let closest = shape
                .strokes
                .iter()
                .map(|stroke| distance_to_segment_nm(*centre, stroke))
                .min_by(f64::total_cmp);
            let Some(distance) = closest else {
                continue;
            };
            let gap_nm = (distance - radius - half_width).max(0.0);
            if gap_nm < required_nm {
                findings.push(DfmFinding {
                    rule: "silk_to_hole",
                    severity: DfmSeverity::Danger,
                    message: format!(
                        "silkscreen {:.3} mm from the hole of {label}, under the {:.3} mm minimum",
                        gap_nm / NM_PER_MM,
                        required_nm / NM_PER_MM
                    ),
                    at_mm: (centre.x_mm(), centre.y_mm()),
                    measured_mm: gap_nm / NM_PER_MM,
                    required_mm: required_nm / NM_PER_MM,
                });
            }
        }
    }
    findings
}

/// Silkscreen drawn thinner than the fab prints reliably. JLCDFM grades this a
/// Warning, so it never blocks. A shape with no stated width is skipped
/// rather than guessed thin. One finding per shape.
fn silk_line_width(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_silk_width_nm;
    board
        .silk_shapes()
        .filter(|shape| shape.width_nm > 0 && shape.width_nm < required_nm)
        .filter_map(|shape| {
            let (start, _) = shape.strokes.first()?;
            Some(DfmFinding {
                rule: "silk_line_width",
                severity: DfmSeverity::Warning,
                message: format!(
                    "silkscreen line {:.3} mm wide, under the {:.3} mm minimum",
                    shape.width_nm as f64 / NM_PER_MM,
                    required_nm as f64 / NM_PER_MM
                ),
                at_mm: (start.x_mm(), start.y_mm()),
                measured_mm: shape.width_nm as f64 / NM_PER_MM,
                required_mm: required_nm as f64 / NM_PER_MM,
            })
        })
        .collect()
}

/// The smallest distance from the pad's outline to a stroke's centreline,
/// and the point on the stroke where it occurs. Negative inside the pad.
fn nearest_sample(pad: &Pad, stroke: &(Point, Point)) -> (f64, Point) {
    let (start, end) = *stroke;
    let (dx, dy) = (
        (end.x_nm - start.x_nm) as f64,
        (end.y_nm - start.y_nm) as f64,
    );
    let steps = ((dx.hypot(dy) / SAMPLE_STEP_NM).ceil() as i64).max(1);
    let mut best = (f64::INFINITY, start);
    for i in 0..=steps {
        let f = i as f64 / steps as f64;
        let p = Point::new(
            start.x_nm + (dx * f).round() as i64,
            start.y_nm + (dy * f).round() as i64,
        );
        let d = pad.signed_distance_nm(p);
        if d < best.0 {
            best = (d, p);
        }
    }
    best
}

fn distance_to_segment_nm(p: Point, stroke: &(Point, Point)) -> f64 {
    let (start, end) = *stroke;
    let (ax, ay) = (start.x_nm as f64, start.y_nm as f64);
    let (bx, by) = (end.x_nm as f64, end.y_nm as f64);
    let (px, py) = (p.x_nm as f64, p.y_nm as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let f = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    (px - (ax + f * dx)).hypot(py - (ay + f * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with one 0.6 mm square SMD pad at (10, 20) on `F.Cu`, plus the
    /// given extra items (silkscreen graphics) before the closing paren.
    fn board(extra: &str) -> String {
        format!(
            r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "GND")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd rect (at 0 0) (size 0.6 0.6)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))
  )
  {extra}
)
"#
        )
    }

    /// A 0.15 mm silk line on `layer` from (x1, y1) to (x2, y2).
    fn silk(layer: &str, x1: f64, y1: f64, x2: f64, y2: f64) -> String {
        format!(
            r#"(gr_line (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.15) (type solid)) (layer "{layer}"))"#
        )
    }

    fn run(extra: &str) -> DfmReport {
        check_dfm(&board(extra), &ManufacturerProfile::jlc_standard()).expect("the board parses")
    }

    #[test]
    fn silk_well_clear_of_a_pad_is_not_reported() {
        // The line is 0.3 mm from the pad edge at x = 10.3; its own half
        // width is 0.075, so the gap is 0.225 mm against a 0.15 mm floor.
        let report = run(&silk("F.SilkS", 10.6, 19.0, 10.6, 21.0));
        assert!(report.findings.is_empty(), "{report:?}");
    }

    #[test]
    fn silk_closer_to_a_pad_than_the_floor_is_a_danger() {
        // 0.11 mm from the pad edge, less half the 0.15 mm line: 0.035 mm gap.
        let report = run(&silk("F.SilkS", 10.41, 19.0, 10.41, 21.0));
        assert_eq!(report.findings.len(), 1, "{report:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.rule, "silk_to_pad");
        assert_eq!(finding.severity, DfmSeverity::Danger);
        assert!((finding.measured_mm - 0.035).abs() < 0.005, "{finding:?}");
        assert!((finding.required_mm - 0.15).abs() < 1e-9, "{finding:?}");
        assert_eq!(report.danger_count(), 1);
    }

    #[test]
    fn silk_printed_across_a_pad_has_no_gap() {
        let report = run(&silk("F.SilkS", 9.0, 20.0, 11.0, 20.0));
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert!(report.findings[0].measured_mm.abs() < 1e-9);
    }

    #[test]
    fn silk_on_the_other_side_of_the_board_does_not_touch_the_pad() {
        let report = run(&silk("B.SilkS", 10.0, 19.0, 10.0, 21.0));
        assert!(report.findings.is_empty(), "{report:?}");
    }

    #[test]
    fn footprint_silk_is_placed_like_the_footprints_pads() {
        // Footprint rotated 90 degrees at (30, 20). Its pad at local (1, 0)
        // lands at board (30, 19); its silk line at local x = 1.4 lands on
        // y = 18.6, 0.1 mm from the pad edge. A reader that rotated the pad
        // but not the silk would leave the line far away and find nothing.
        let text = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
  (net 0 "")
  (net 1 "GND")
  (footprint "Test:Rotated"
    (layer "F.Cu")
    (at 30 20 90)
    (pad "1" smd rect (at 1 0) (size 0.6 0.6)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))
    (fp_line (start 1.4 -0.5) (end 1.4 0.5) (stroke (width 0.15) (type solid)) (layer "F.SilkS"))
  )
)
"#;
        let report = check_dfm(text, &ManufacturerProfile::jlc_standard()).expect("parses");
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert!((report.findings[0].measured_mm - 0.025).abs() < 0.005);
    }

    #[test]
    fn a_bottom_side_footprints_silk_is_checked_against_its_own_pads() {
        // Footprint on the back at (30, 20): pad at local (1, 0), silk 0.1 mm
        // from its edge, both on the back. A front-side pad must not see it.
        let text = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal))
  (net 0 "")
  (net 1 "GND")
  (footprint "Test:Back"
    (layer "B.Cu")
    (at 30 20)
    (pad "1" smd rect (at 1 0) (size 0.6 0.6)
      (layers "B.Cu" "B.Paste" "B.Mask") (net 1 "GND"))
    (fp_line (start 1.4 -0.5) (end 1.4 0.5) (stroke (width 0.15) (type solid)) (layer "B.SilkS"))
  )
)
"#;
        let report = check_dfm(text, &ManufacturerProfile::jlc_standard()).expect("parses");
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert!((report.findings[0].measured_mm - 0.025).abs() < 0.005);
    }

    fn silk_to_pad_count(report: &DfmReport) -> usize {
        report
            .findings
            .iter()
            .filter(|f| f.rule == "silk_to_pad")
            .count()
    }

    #[test]
    fn a_silk_rectangle_edge_too_close_to_a_pad_is_a_danger() {
        // Left edge at x = 10.41: 0.11 mm from the pad edge, 0.035 mm clear of
        // the 0.15 mm stroke. The other three edges are far from the pad.
        let report = run(
            r#"(gr_rect (start 10.41 19) (end 11 21) (stroke (width 0.15) (type solid)) (fill no) (layer "F.SilkS"))"#,
        );
        assert_eq!(silk_to_pad_count(&report), 1, "{report:?}");
    }

    #[test]
    fn a_silk_circle_too_close_to_a_pad_is_a_danger() {
        // Centre (11, 20), radius 0.69: its leftmost point is 0.01 mm from the
        // pad edge, so the stroke touches the pad.
        let report = run(
            r#"(gr_circle (center 11 20) (end 11.69 20) (stroke (width 0.15) (type solid)) (fill no) (layer "F.SilkS"))"#,
        );
        assert_eq!(silk_to_pad_count(&report), 1, "{report:?}");
        assert!(report.findings[0].measured_mm < 0.005, "{report:?}");
    }

    #[test]
    fn a_silk_arc_too_close_to_a_pad_is_a_danger_and_its_far_side_is_not() {
        // Arc on the left half of a circle centred (11, 20), radius 0.59: its
        // leftmost point is 0.11 mm from the pad edge, a 0.035 mm gap.
        let near = run(
            r#"(gr_arc (start 11 19.41) (mid 10.41 20) (end 11 20.59) (stroke (width 0.15) (type solid)) (layer "F.SilkS"))"#,
        );
        assert_eq!(silk_to_pad_count(&near), 1, "{near:?}");
        assert!(
            (near.findings[0].measured_mm - 0.035).abs() < 0.01,
            "{near:?}"
        );
        // The same circle's right half is 0.89 mm away.
        let far = run(
            r#"(gr_arc (start 11 19.41) (mid 11.59 20) (end 11 20.59) (stroke (width 0.15) (type solid)) (layer "F.SilkS"))"#,
        );
        assert_eq!(silk_to_pad_count(&far), 0, "{far:?}");
    }

    fn count(report: &DfmReport, rule: &str) -> usize {
        report.findings.iter().filter(|f| f.rule == rule).count()
    }

    /// A 0.2 mm via at (12, 20), well away from the board's one pad.
    const VIA: &str = r#"(via (at 12 20) (size 0.4) (drill 0.2) (layers "F.Cu" "B.Cu") (net 1))"#;

    #[test]
    fn silk_too_close_to_a_via_drill_is_a_danger() {
        // Line at x = 12.3: 0.30 mm from the hole centre, less the 0.10 mm
        // hole radius and the 0.075 mm half stroke: a 0.125 mm gap.
        let report = run(&format!(
            "{VIA}\n{}",
            silk("F.SilkS", 12.3, 19.0, 12.3, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_hole"), 1, "{report:?}");
        let finding = report.findings.iter().find(|f| f.rule == "silk_to_hole");
        assert!((finding.expect("finding").measured_mm - 0.125).abs() < 0.005);
    }

    #[test]
    fn silk_clear_of_a_via_drill_is_not_reported() {
        let report = run(&format!(
            "{VIA}\n{}",
            silk("F.SilkS", 12.4, 19.0, 12.4, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_hole"), 0, "{report:?}");
    }

    #[test]
    fn a_hole_goes_through_the_board_so_silk_on_the_back_counts_too() {
        let report = run(&format!(
            "{VIA}\n{}",
            silk("B.SilkS", 12.3, 19.0, 12.3, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_hole"), 1, "{report:?}");
    }

    #[test]
    fn silk_across_a_through_hole_pads_drill_is_a_danger() {
        let text = board(&format!(
            r#"(footprint "Test:Thru" (layer "F.Cu") (at 20 20)
    (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 1.0)
      (layers "*.Cu" "*.Mask") (net 1 "GND")))
  {}"#,
            silk("F.SilkS", 19.0, 20.0, 21.0, 20.0)
        ));
        let report = check_dfm(&text, &ManufacturerProfile::jlc_standard()).expect("parses");
        assert_eq!(count(&report, "silk_to_hole"), 1, "{report:?}");
        let hole = report
            .findings
            .iter()
            .find(|f| f.rule == "silk_to_hole")
            .expect("a hole finding");
        assert!(hole.measured_mm < 0.005, "{hole:?}");
    }

    #[test]
    fn a_thin_silk_line_is_a_warning_not_a_danger() {
        // JLCDFM grades 0.12 mm silk a Warning; the floor is 0.15 mm.
        let report = run(
            r#"(gr_line (start 5 5) (end 7 5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))"#,
        );
        assert_eq!(count(&report, "silk_line_width"), 1, "{report:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.severity, DfmSeverity::Warning);
        assert!((finding.measured_mm - 0.12).abs() < 1e-9, "{finding:?}");
        assert!((finding.required_mm - 0.15).abs() < 1e-9, "{finding:?}");
        assert_eq!(report.danger_count(), 0);
    }

    #[test]
    fn silk_at_the_width_floor_is_not_reported() {
        let report = run(&silk("F.SilkS", 5.0, 5.0, 7.0, 5.0));
        assert_eq!(count(&report, "silk_line_width"), 0, "{report:?}");
    }

    #[test]
    fn a_thin_curve_is_one_finding_not_one_per_chord() {
        let report = run(
            r#"(gr_circle (center 5 5) (end 6 5) (stroke (width 0.12) (type solid)) (fill no) (layer "F.SilkS"))"#,
        );
        assert_eq!(count(&report, "silk_line_width"), 1, "{report:?}");
    }

    #[test]
    fn silk_with_no_stated_width_is_not_guessed_thin() {
        let report = run(r#"(gr_line (start 5 5) (end 7 5) (layer "F.SilkS"))"#);
        assert_eq!(count(&report, "silk_line_width"), 0, "{report:?}");
    }

    #[test]
    fn silk_text_is_counted_as_unchecked_not_silently_passed() {
        // The base board's footprint carries one visible reference designator
        // on the silk layer; text has no stroke geometry here to measure.
        assert_eq!(run("").unchecked_silk_text, 1);
        let with_board_text = run(
            r#"(gr_text "REV A" (at 5 5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))"#,
        );
        assert_eq!(with_board_text.unchecked_silk_text, 2);
    }

    #[test]
    fn text_off_the_silk_layers_and_hidden_text_are_not_counted() {
        let report = run(
            r#"(gr_text "NOTE" (at 5 5) (layer "F.Fab") (effects (font (size 1 1))))
  (gr_text "HIDDEN" (at 6 5) (layer "F.SilkS") (effects (font (size 1 1)) (hide yes)))"#,
        );
        assert_eq!(report.unchecked_silk_text, 1, "only the reference counts");
    }

    #[test]
    fn text_marked_not_hidden_is_still_counted() {
        let report = run(
            r#"(gr_text "SHOWN" (at 5 5) (layer "F.SilkS") (effects (font (size 1 1)) (hide no)))"#,
        );
        assert_eq!(report.unchecked_silk_text, 2, "the reference and this text");
    }

    #[test]
    fn an_unreadable_board_is_an_error_not_a_clean_result() {
        assert!(check_dfm("(kicad_pcb (via", &ManufacturerProfile::jlc_standard()).is_err());
    }
}
