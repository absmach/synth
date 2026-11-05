// SPDX-License-Identifier: Apache-2.0

//! Manufacturability findings read from an exported `.kicad_pcb`.
//!
//! The checks here mirror what a fab's own DFM tool reports on the same
//! board, graded the way that tool grades them: a `Danger` finding is one the
//! fab will reject, a `Warning` is borderline. They read the board file, not
//! the in-memory routing model, because silkscreen only exists in the file
//! the fab receives. Only straight silk strokes (`gr_line`, `fp_line`) are
//! read so far; arcs, circles, rectangles and text are not checked.

// Board coordinates are far below f64's 52-bit mantissa (a 100 mm board spans
// 1e8 nm), so the distance maths here loses nothing converting i64 to f64.
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeMap;

use crate::pcb_read::NM_PER_MM;
use crate::pcb_read::{Pad, PcbBoard, PcbParseError, Point, SilkLine};
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
    Ok(DfmReport {
        findings: silk_to_pad(&board, profile),
    })
}

/// Spacing between samples taken along a silk stroke, in nanometres. Small
/// against the 0.15 mm floors the rules check, so the error it adds is
/// negligible.
const SAMPLE_STEP_NM: f64 = 20_000.0;

/// Silkscreen printed too close to, or over, a pad on the same side of the
/// board. Ink on a pad stops solder wetting it.
fn silk_to_pad(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_silk_to_pad_nm as f64;
    let mut findings = Vec::new();
    for line in board.silk_lines() {
        let copper = if line.layer == "F.SilkS" {
            "F.Cu"
        } else {
            "B.Cu"
        };
        let half_width = line.width_nm as f64 / 2.0;
        for pad in board
            .pads()
            .filter(|p| p.layers.iter().any(|l| l == copper || l == "*.Cu"))
        {
            let (hx, hy) = (pad.size_nm.0 as f64 / 2.0, pad.size_nm.1 as f64 / 2.0);
            let reach = hx.hypot(hy) + half_width + required_nm;
            if distance_to_segment_nm(pad.at, line) > reach {
                continue;
            }
            let (distance, at) = nearest_sample(pad, line);
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

/// The smallest distance from the pad's outline to the stroke's centreline,
/// and the point on the stroke where it occurs. Negative inside the pad.
fn nearest_sample(pad: &Pad, line: &SilkLine) -> (f64, Point) {
    let (dx, dy) = (
        (line.end.x_nm - line.start.x_nm) as f64,
        (line.end.y_nm - line.start.y_nm) as f64,
    );
    let steps = ((dx.hypot(dy) / SAMPLE_STEP_NM).ceil() as i64).max(1);
    let mut best = (f64::INFINITY, line.start);
    for i in 0..=steps {
        let f = i as f64 / steps as f64;
        let p = Point::new(
            line.start.x_nm + (dx * f).round() as i64,
            line.start.y_nm + (dy * f).round() as i64,
        );
        let d = pad.signed_distance_nm(p);
        if d < best.0 {
            best = (d, p);
        }
    }
    best
}

fn distance_to_segment_nm(p: Point, line: &SilkLine) -> f64 {
    let (ax, ay) = (line.start.x_nm as f64, line.start.y_nm as f64);
    let (bx, by) = (line.end.x_nm as f64, line.end.y_nm as f64);
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

    /// A 0.12 mm silk line on `layer` from (x1, y1) to (x2, y2).
    fn silk(layer: &str, x1: f64, y1: f64, x2: f64, y2: f64) -> String {
        format!(
            r#"(gr_line (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.12) (type solid)) (layer "{layer}"))"#
        )
    }

    fn run(extra: &str) -> DfmReport {
        check_dfm(&board(extra), &ManufacturerProfile::jlc_standard()).expect("the board parses")
    }

    #[test]
    fn silk_well_clear_of_a_pad_is_not_reported() {
        // The line is 0.3 mm from the pad edge at x = 10.3; its own half
        // width is 0.06, so the gap is 0.24 mm against a 0.15 mm floor.
        let report = run(&silk("F.SilkS", 10.6, 19.0, 10.6, 21.0));
        assert!(report.findings.is_empty(), "{report:?}");
    }

    #[test]
    fn silk_closer_to_a_pad_than_the_floor_is_a_danger() {
        // 0.11 mm from the pad edge, less half the 0.12 mm line: 0.05 mm gap.
        let report = run(&silk("F.SilkS", 10.41, 19.0, 10.41, 21.0));
        assert_eq!(report.findings.len(), 1, "{report:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.rule, "silk_to_pad");
        assert_eq!(finding.severity, DfmSeverity::Danger);
        assert!((finding.measured_mm - 0.05).abs() < 0.005, "{finding:?}");
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
    (fp_line (start 1.4 -0.5) (end 1.4 0.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
  )
)
"#;
        let report = check_dfm(text, &ManufacturerProfile::jlc_standard()).expect("parses");
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert!((report.findings[0].measured_mm - 0.04).abs() < 0.005);
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
    (fp_line (start 1.4 -0.5) (end 1.4 0.5) (stroke (width 0.12) (type solid)) (layer "B.SilkS"))
  )
)
"#;
        let report = check_dfm(text, &ManufacturerProfile::jlc_standard()).expect("parses");
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert!((report.findings[0].measured_mm - 0.04).abs() < 0.005);
    }

    #[test]
    fn an_unreadable_board_is_an_error_not_a_clean_result() {
        assert!(check_dfm("(kicad_pcb (via", &ManufacturerProfile::jlc_standard()).is_err());
    }
}
