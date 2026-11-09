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
use crate::pcb_read::{
    entry_is_copper, layer_covers, Hole, Pad, PcbBoard, PcbParseError, Point, Via,
};
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
    /// Silkscreen curves and legacy arcs the reader does not understand.
    pub unchecked_silk_graphics: usize,
    /// Tracks missing a start, mid or end point: not measured.
    pub unread_tracks: usize,
    /// `custom` and `trapezoid` pads, measured by their bounding box.
    pub approximated_pads: usize,
    /// Rules the profile publishes no figure for: not run, so a clean report
    /// says nothing about them.
    pub unchecked_rules: Vec<&'static str>,
}

impl DfmReport {
    /// Danger findings, the ones the fab will reject. They block a release
    /// export only when the caller passes `--dfm-gate`; otherwise they are
    /// recorded.
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
    // A zero floor means the profile has no published figure: skip the rule
    // rather than grade it against another fab's number.
    let rules: [(&'static str, i64, RuleFn); 5] = [
        ("silk_to_pad", profile.min_silk_to_pad_nm, silk_to_pad),
        ("silk_to_hole", profile.min_silk_to_hole_nm, silk_to_hole),
        (
            "silk_line_width",
            profile.min_silk_width_nm,
            silk_line_width,
        ),
        ("pad_to_pad", profile.min_pad_to_pad_nm, pad_to_pad),
        ("pth_to_trace", profile.min_pth_to_trace_nm, pth_to_trace),
    ];
    let mut findings = Vec::new();
    let mut unchecked_rules = Vec::new();
    for (name, floor_nm, rule) in rules {
        if floor_nm > 0 {
            findings.extend(rule(&board, profile));
        } else {
            unchecked_rules.push(name);
        }
    }
    Ok(DfmReport {
        findings,
        unchecked_silk_text: board.silk_text_count(),
        unchecked_silk_graphics: board.silk_unread_count(),
        unread_tracks: board.track_unread,
        approximated_pads: board.approximated_pad_count(),
        unchecked_rules,
    })
}

type RuleFn = fn(&PcbBoard, &ManufacturerProfile) -> Vec<DfmFinding>;

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
        for pad in board.pads() {
            let reach = pad.reach_nm() + half_width + required_nm;
            let closest = shape
                .strokes
                .iter()
                .filter(|stroke| distance_to_segment_nm(pad.at, stroke) <= reach)
                .map(|stroke| nearest_sample(pad, stroke))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            // Ink filling a shape that covers any part of the pad is on it.
            let covered = shape.covers(
                std::iter::once(pad.at).chain(pad.outline_points_nm(SAMPLE_STEP_NM)),
                |p| pad.copper_distance_nm(p) <= 0.0,
            );
            let Some((distance, at)) = closest.or(covered.then_some((0.0, pad.at))) else {
                continue;
            };
            if !pad.copper_on(copper) {
                continue;
            }
            let gap_nm = if covered {
                0.0
            } else {
                (distance - half_width).max(0.0)
            };
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
/// goes through the board, so ink on the back reaches it too. A slot is the
/// stadium around its long axis, not a circle of its longer dimension. One
/// finding per shape and hole.
fn silk_to_hole(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_silk_to_hole_nm as f64;
    let holes: Vec<(Hole, String)> = board
        .vias
        .iter()
        .filter(|via| via.drill_nm > 0)
        .map(|via| {
            let hole = Hole {
                a: via.at,
                b: via.at,
                radius_nm: via.drill_nm as f64 / 2.0,
            };
            (
                hole,
                format!("via at ({:.3}, {:.3}) mm", via.at.x_mm(), via.at.y_mm()),
            )
        })
        .chain(
            board
                .pads()
                .filter_map(|pad| Some((pad.hole()?, format!("pad {}", pad.id())))),
        )
        .collect();
    let mut findings = Vec::new();
    for shape in board.silk_shapes() {
        let half_width = shape.width_nm as f64 / 2.0;
        for (hole, label) in &holes {
            let closest = shape
                .strokes
                .iter()
                .map(|stroke| segment_distance_nm((hole.a, hole.b), *stroke))
                .min_by(f64::total_cmp);
            let covered = shape.covers(hole_probes(hole), |p| {
                distance_to_segment_nm(p, &(hole.a, hole.b)) <= hole.radius_nm
            });
            let Some(distance) = closest.or(covered.then_some(0.0)) else {
                continue;
            };
            let gap_nm = if covered {
                0.0
            } else {
                (distance - hole.radius_nm - half_width).max(0.0)
            };
            if gap_nm < required_nm {
                findings.push(DfmFinding {
                    rule: "silk_to_hole",
                    severity: DfmSeverity::Danger,
                    message: format!(
                        "silkscreen {:.3} mm from the hole of {label}, under the {:.3} mm minimum",
                        gap_nm / NM_PER_MM,
                        required_nm / NM_PER_MM
                    ),
                    at_mm: (hole.a.x_mm(), hole.a.y_mm()),
                    measured_mm: gap_nm / NM_PER_MM,
                    required_mm: required_nm / NM_PER_MM,
                });
            }
        }
    }
    findings
}

/// Points on a hole to test against filled ink: both ends and eight points
/// around each end's rim.
fn hole_probes(hole: &Hole) -> Vec<Point> {
    let mut probes = vec![hole.a, hole.b];
    for end in [hole.a, hole.b] {
        for k in 0..8 {
            let angle = f64::from(k) * std::f64::consts::FRAC_PI_4;
            probes.push(Point::new(
                end.x_nm
                    .saturating_add((hole.radius_nm * angle.cos()).round() as i64),
                end.y_nm
                    .saturating_add((hole.radius_nm * angle.sin()).round() as i64),
            ));
        }
    }
    probes
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

/// Pads and vias closer than the fab can etch apart, on the same side of the
/// board, whatever their nets: the Gerbers the fab reads carry none, so two
/// copper objects of one net are graded too. Same-net copper closer than
/// the merge tolerance is one object and is left alone. A mounting hole
/// carries no copper and is skipped.
fn pad_to_pad(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_pad_to_pad_nm as f64;
    // A via is copper on every layer it spans, so to the fab it is one more
    // round pad.
    let via_pads: Vec<Pad> = board.vias.iter().map(via_as_pad).collect();
    let pads: Vec<&Pad> = board
        .pads()
        .filter(|p| p.kind != "np_thru_hole" && p.layers.iter().any(|l| entry_is_copper(l)))
        .chain(via_pads.iter())
        .collect();
    let mut findings = Vec::new();
    for (i, a) in pads.iter().enumerate() {
        for b in &pads[i + 1..] {
            let (dx, dy) = (
                a.at.x_nm.saturating_sub(b.at.x_nm) as f64,
                a.at.y_nm.saturating_sub(b.at.y_nm) as f64,
            );
            if dx.hypot(dy) > a.reach_nm() + b.reach_nm() + required_nm {
                continue;
            }
            if !share_copper_layer(a, b) {
                continue;
            }
            let (gap_nm, at) = pad_gap(a, b);
            let (net_a, net_b) = (connected_net(a), connected_net(b));
            // Copper that touches is one object once etched, whatever its net
            // status; only two real, different nets make that a short.
            let different_nets = net_a.is_some() && net_b.is_some() && net_a != net_b;
            if gap_nm == 0.0 && !different_nets {
                continue;
            }
            // Copper of one net this close is one object once etched.
            if net_a.is_some() && net_a == net_b && gap_nm < profile.copper_merge_below_nm as f64 {
                continue;
            }
            if let Some((severity, bound_nm)) = spacing_grade(gap_nm, profile) {
                findings.push(DfmFinding {
                    rule: "pad_to_pad",
                    severity,
                    message: format!(
                        "pads {} and {} are {:.3} mm apart, under the {:.3} mm minimum",
                        a.id(),
                        b.id(),
                        gap_nm / NM_PER_MM,
                        bound_nm / NM_PER_MM
                    ),
                    at_mm: (at.x_mm(), at.y_mm()),
                    measured_mm: gap_nm / NM_PER_MM,
                    required_mm: bound_nm / NM_PER_MM,
                });
            }
        }
    }
    findings
}

/// A via's copper as a round pad, so spacing rules can treat it like one.
fn via_as_pad(via: &Via) -> Pad {
    Pad {
        footprint: "via".to_string(),
        number: format!("{:.3},{:.3}", via.at.x_mm(), via.at.y_mm()),
        kind: "via".to_string(),
        shape: "circle".to_string(),
        at: via.at,
        size_nm: (via.size_nm, via.size_nm),
        angle_deg: 0.0,
        corner_ratio: 0.0,
        drill_nm: (via.drill_nm, via.drill_nm),
        drill_offset_nm: (0, 0),
        bbox: None,
        layers: via.layers.clone(),
        net: via.net,
        net_name: via.net_name.clone(),
    }
}

/// How JLCDFM grades a copper spacing: Danger below the danger bound, Warning
/// below the floor, nothing above it. Returns the grade and the bound crossed.
fn spacing_grade(gap_nm: f64, profile: &ManufacturerProfile) -> Option<(DfmSeverity, f64)> {
    let floor = profile.min_pad_to_pad_nm as f64;
    // A gap that meets the fab's own floor is never a Danger.
    let danger = (profile.pad_to_pad_danger_nm as f64).min(floor);
    if gap_nm < danger {
        Some((DfmSeverity::Danger, danger))
    } else if gap_nm < floor {
        Some((DfmSeverity::Warning, floor))
    } else {
        None
    }
}

/// The pad's net, or `None` when it has none or sits on KiCad's net 0, which
/// means unconnected: two unconnected pads are separate copper, not one net.
fn connected_net(pad: &Pad) -> Option<u32> {
    pad.net.filter(|n| *n != 0)
}

/// Whether two pads have copper on a common layer; `*.Cu` is every layer.
fn share_copper_layer(a: &Pad, b: &Pad) -> bool {
    a.layers
        .iter()
        .filter(|l| entry_is_copper(l))
        .any(|la| b.layers.iter().any(|lb| layer_covers(lb, la)))
}

/// The gap between two pads' outlines and a point on it: the nearest an
/// outline sample of either pad comes to the other pad. Zero if they overlap.
fn pad_gap(a: &Pad, b: &Pad) -> (f64, Point) {
    let mut best = (f64::INFINITY, a.at);
    for (from, to) in [(a, b), (b, a)] {
        for p in from.outline_points_nm(SAMPLE_STEP_NM) {
            let d = to.copper_distance_nm(p);
            if d < best.0 {
                best = (d, p);
            }
        }
    }
    (best.0.max(0.0), best.1)
}

/// A trace of another net close to a plated hole's edge: drilling wanders,
/// and a trace under the wall shorts or breaks. Plated holes are through-hole
/// pad drills and via barrels; a hole goes through every layer, so a trace on
/// any layer counts. One finding per hole, for its closest trace; the hole's
/// own net may run up to it. Arc tracks are measured as sampled chords.
/// Graded a Warning, not a Danger, so it never blocks a release. Measured
/// against JLCDFM on one board before vias and arcs were read, it found 0
/// items where JLCDFM found 58 (known not to match; not re-measured since).
fn pth_to_trace(board: &PcbBoard, profile: &ManufacturerProfile) -> Vec<DfmFinding> {
    let required_nm = profile.min_pth_to_trace_nm as f64;
    let via_pads: Vec<Pad> = board.vias.iter().map(via_as_pad).collect();
    let mut findings = Vec::new();
    for (pad, label) in board
        .pads()
        .filter(|p| p.kind == "thru_hole")
        .map(|p| (p, format!("pad {}", p.id())))
        .chain(
            via_pads
                .iter()
                .map(|p| (p, format!("via at ({})", p.number))),
        )
    {
        let Some(hole) = pad.hole() else {
            continue;
        };
        let own_net = connected_net(pad);
        let closest = board
            .segments
            .iter()
            .chain(&board.arc_chords)
            .filter(|s| !(own_net.is_some() && own_net == s.net))
            .map(|s| {
                let d = segment_distance_nm((hole.a, hole.b), (s.start, s.end));
                (d - hole.radius_nm - s.width_nm as f64 / 2.0).max(0.0)
            })
            .min_by(f64::total_cmp);
        let Some(gap_nm) = closest else {
            continue;
        };
        if gap_nm < required_nm {
            findings.push(DfmFinding {
                rule: "pth_to_trace",
                severity: DfmSeverity::Warning,
                message: format!(
                    "a trace is {:.3} mm from the plated hole of {label}, under the {:.3} mm minimum",
                    gap_nm / NM_PER_MM,
                    required_nm / NM_PER_MM
                ),
                at_mm: (hole.a.x_mm(), hole.a.y_mm()),
                measured_mm: gap_nm / NM_PER_MM,
                required_mm: required_nm / NM_PER_MM,
            });
        }
    }
    findings
}

/// Most samples taken along one stroke, so a malformed huge stroke cannot loop forever.
const MAX_STROKE_SAMPLES: i64 = 100_000;

/// The smallest distance from the pad's outline to a stroke's centreline,
/// and the point on the stroke where it occurs. Negative inside the pad.
fn nearest_sample(pad: &Pad, stroke: &(Point, Point)) -> (f64, Point) {
    let (start, end) = *stroke;
    let (dx, dy) = (
        end.x_nm.saturating_sub(start.x_nm) as f64,
        end.y_nm.saturating_sub(start.y_nm) as f64,
    );
    let steps = ((dx.hypot(dy) / SAMPLE_STEP_NM).ceil() as i64).clamp(1, MAX_STROKE_SAMPLES);
    let mut best = (f64::INFINITY, start);
    for i in 0..=steps {
        let f = i as f64 / steps as f64;
        let p = Point::new(
            start.x_nm.saturating_add((dx * f).round() as i64),
            start.y_nm.saturating_add((dy * f).round() as i64),
        );
        let d = pad.copper_distance_nm(p);
        if d < best.0 {
            best = (d, p);
        }
    }
    best
}

/// The smallest distance between two segments; zero if they cross.
fn segment_distance_nm(a: (Point, Point), b: (Point, Point)) -> f64 {
    let side = |p: Point, q: Point, r: Point| {
        ((q.x_nm - p.x_nm) as f64) * ((r.y_nm - p.y_nm) as f64)
            - ((q.y_nm - p.y_nm) as f64) * ((r.x_nm - p.x_nm) as f64)
    };
    let crosses = side(a.0, a.1, b.0) * side(a.0, a.1, b.1) < 0.0
        && side(b.0, b.1, a.0) * side(b.0, b.1, a.1) < 0.0;
    if crosses {
        return 0.0;
    }
    [
        distance_to_segment_nm(a.0, &b),
        distance_to_segment_nm(a.1, &b),
        distance_to_segment_nm(b.0, &a),
        distance_to_segment_nm(b.1, &a),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
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
  (net 2 "SIG")
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

    /// A one-pad SMD footprint of the given shape and square size on `layer`.
    fn pad_at(x: f64, y: f64, net: u32, shape: &str, size: f64, layer: &str) -> String {
        let net_name = match net {
            0 => "",
            1 => "GND",
            _ => "SIG",
        };
        format!(
            r#"(footprint "Test:P" (layer "{layer}") (at {x} {y})
    (pad "1" smd {shape} (at 0 0) (size {size} {size})
      (layers "{layer}" "{}" "{}") (net {net} "{net_name}")))"#,
            layer.replace("Cu", "Paste"),
            layer.replace("Cu", "Mask")
        )
    }

    #[test]
    fn pads_of_different_nets_under_the_danger_bound_are_a_danger() {
        // The base pad spans x 9.7..10.3; this one spans 10.37..10.97: 0.07 mm.
        let report = run(&pad_at(10.67, 20.0, 2, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.severity, DfmSeverity::Danger);
        assert!((finding.measured_mm - 0.07).abs() < 0.005, "{finding:?}");
        assert!((finding.required_mm - 0.10).abs() < 1e-9, "{finding:?}");
    }

    #[test]
    fn pads_between_the_danger_bound_and_the_floor_are_a_warning() {
        // JLCDFM's scale: Danger below 0.10 mm, Warning to 0.15 mm.
        let report = run(&pad_at(10.72, 20.0, 2, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
        let finding = &report.findings[0];
        assert_eq!(finding.severity, DfmSeverity::Warning);
        assert!((finding.measured_mm - 0.12).abs() < 0.005, "{finding:?}");
        assert!((finding.required_mm - 0.15).abs() < 1e-9, "{finding:?}");
        assert_eq!(report.danger_count(), 0);
    }

    #[test]
    fn pads_with_room_between_them_are_not_reported() {
        let report = run(&pad_at(10.9, 20.0, 2, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&report, "pad_to_pad"), 0, "{report:?}");
    }

    #[test]
    fn separate_copper_is_graded_whatever_its_net_like_a_fab_sees_it() {
        // A pad and a pad of its own net 0.09 mm apart: two etched objects
        // with a sliver between them. JLCDFM reads Gerbers, which have no
        // nets, and calls this a Danger.
        let report = run(&pad_at(10.69, 20.0, 1, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
        assert_eq!(report.findings[0].severity, DfmSeverity::Danger);
        // At 0.12 mm it is a Warning.
        let warn = run(&pad_at(10.72, 20.0, 1, "rect", 0.6, "F.Cu"));
        assert_eq!(warn.findings[0].severity, DfmSeverity::Warning, "{warn:?}");
    }

    #[test]
    fn same_net_copper_closer_than_the_merge_tolerance_is_one_object() {
        // 0.05 mm: the fab's copper runs together, so there is no gap to grade.
        let merged = run(&pad_at(10.65, 20.0, 1, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&merged, "pad_to_pad"), 0, "{merged:?}");
        // The same distance between different nets is a near short, not a merge.
        let short = run(&pad_at(10.65, 20.0, 2, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&short, "pad_to_pad"), 1, "{short:?}");
        assert_eq!(short.findings[0].severity, DfmSeverity::Danger);
    }

    #[test]
    fn pads_on_opposite_sides_do_not_share_copper() {
        let report = run(&pad_at(10.7, 20.0, 2, "rect", 0.6, "B.Cu"));
        assert_eq!(count(&report, "pad_to_pad"), 0, "{report:?}");
    }

    #[test]
    fn diagonal_round_pads_are_measured_on_their_curved_outline() {
        // Centres 0.707 mm apart, radius 0.3 each: a 0.107 mm gap. Their
        // bounding boxes overlap, so a box-to-box measure would say zero.
        let report = run(&format!(
            "{}\n  {}",
            pad_at(20.0, 20.0, 1, "circle", 0.6, "F.Cu"),
            pad_at(20.5, 20.5, 2, "circle", 0.6, "F.Cu")
        ));
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
        let finding = report.findings.iter().find(|f| f.rule == "pad_to_pad");
        assert!((finding.expect("finding").measured_mm - 0.107).abs() < 0.005);
    }

    #[test]
    fn a_via_is_copper_too_and_may_not_crowd_a_pad_of_another_net() {
        // A 0.4 mm via of net 2 centred 0.55 mm from the base pad's centre: its
        // copper edge is 0.05 mm from the pad edge.
        let via = |x: f64, net: u32| {
            format!(
                r#"(via (at {x} 20) (size 0.4) (drill 0.2) (layers "F.Cu" "B.Cu") (net {net}))"#
            )
        };
        let near = run(&via(10.55, 2));
        assert_eq!(count(&near, "pad_to_pad"), 1, "{near:?}");
        let finding = near.findings.iter().find(|f| f.rule == "pad_to_pad");
        assert!((finding.expect("finding").measured_mm - 0.05).abs() < 0.005);
        // Room to spare is fine. Its own net is merged when it is that close,
        // and graded once there is a real gap (0.08 mm: a Danger).
        assert_eq!(count(&run(&via(10.55, 1)), "pad_to_pad"), 0);
        assert_eq!(count(&run(&via(11.2, 2)), "pad_to_pad"), 0);
        let own_net = run(&via(10.58, 1));
        assert_eq!(count(&own_net, "pad_to_pad"), 1, "{own_net:?}");
        assert_eq!(own_net.findings[0].severity, DfmSeverity::Danger);
    }

    #[test]
    fn two_unconnected_pads_are_separate_copper_not_one_net() {
        // KiCad's net 0 is "no net". The base pad is on net 1, so give both
        // pads their own distinct pins on net 0.
        let report = run(&format!(
            "{}\n  {}",
            pad_at(20.0, 20.0, 0, "rect", 0.6, "F.Cu"),
            pad_at(20.7, 20.0, 0, "rect", 0.6, "F.Cu")
        ));
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
    }

    #[test]
    fn a_through_hole_pad_meets_an_smd_pad_on_the_side_it_shares() {
        // The SMD pad lists only F.Cu; the through-hole pad lists `*.Cu`.
        let thru = r#"(footprint "Test:T" (layer "F.Cu") (at 10.7 20)
    (pad "1" thru_hole rect (at 0 0) (size 0.6 0.6) (drill 0.3)
      (layers "*.Cu" "*.Mask") (net 2 "SIG")))"#;
        let report = run(thru);
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
    }

    /// A plated 1.0 mm hole in a 1.6 mm pad at (20, 20), net 1.
    const PTH: &str = r#"(footprint "Test:Thru" (layer "F.Cu") (at 20 20)
    (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 1.0)
      (layers "*.Cu" "*.Mask") (net 1 "GND")))"#;

    fn trace(layer: &str, y: f64, net: u32) -> String {
        format!(
            r#"(segment (start 19 {y}) (end 21 {y}) (width 0.2) (layer "{layer}") (net {net}))"#
        )
    }

    #[test]
    fn a_trace_too_close_to_a_plated_hole_is_an_advisory_warning() {
        // Trace centre 0.75 mm from the hole centre, less its 0.5 mm radius
        // and the trace's 0.1 mm half width: a 0.15 mm gap against 0.28 mm.
        let report = run(&format!("{PTH}\n  {}", trace("F.Cu", 20.75, 2)));
        assert_eq!(count(&report, "pth_to_trace"), 1, "{report:?}");
        let finding = report.findings.iter().find(|f| f.rule == "pth_to_trace");
        let finding = finding.expect("finding");
        // Known not to match JLCDFM, so it never blocks a release.
        assert_eq!(finding.severity, DfmSeverity::Warning);
        assert_eq!(report.danger_count(), 0);
        assert!((finding.measured_mm - 0.15).abs() < 0.005, "{finding:?}");
        assert!((finding.required_mm - 0.28).abs() < 1e-9, "{finding:?}");
    }

    fn arc_trace(mid_y: f64, net: u32) -> String {
        format!(
            r#"(arc (start 19 21) (mid 20 {mid_y}) (end 21 21) (width 0.2) (layer "F.Cu") (net {net}))"#
        )
    }

    #[test]
    fn an_arc_track_too_close_to_a_plated_hole_is_reported() {
        // The arc's crown is 0.75 mm from the hole centre: a 0.15 mm gap.
        let report = run(&format!("{PTH}\n  {}", arc_trace(20.75, 2)));
        assert_eq!(count(&report, "pth_to_trace"), 1, "{report:?}");
        let finding = report.findings.iter().find(|f| f.rule == "pth_to_trace");
        assert!((finding.expect("finding").measured_mm - 0.15).abs() < 0.005);
    }

    fn pth_gap_of(arc: &str) -> f64 {
        let report = run(&format!("{PTH}\n  {arc}"));
        assert_eq!(count(&report, "pth_to_trace"), 1, "{report:?}");
        let finding = report.findings.iter().find(|f| f.rule == "pth_to_trace");
        finding.expect("finding").measured_mm
    }

    #[test]
    fn arc_direction_does_not_change_the_measured_distance() {
        // Crown 0.75 mm above the hole centre; same arc written both ways.
        let a =
            r#"(arc (start 19 21) (mid 20 20.75) (end 21 21) (width 0.2) (layer "F.Cu") (net 2))"#;
        let b =
            r#"(arc (start 21 21) (mid 20 20.75) (end 19 21) (width 0.2) (layer "F.Cu") (net 2))"#;
        assert!((pth_gap_of(a) - pth_gap_of(b)).abs() < 1e-9);
        assert!((pth_gap_of(b) - 0.15).abs() < 0.005);
    }

    #[test]
    fn an_arc_sweeping_over_180_degrees_is_measured_along_the_long_way() {
        // Circle centre (20, 21.5), radius 0.75: 225 degrees from the east
        // point through the crown (20, 20.75) to the south-west.
        let major = r#"(arc (start 20.75 21.5) (mid 20 20.75) (end 19.4697 22.0303) (width 0.2) (layer "F.Cu") (net 2))"#;
        assert!((pth_gap_of(major) - 0.15).abs() < 0.005);
        // The same endpoints with the mid on the far side never nears the hole.
        let far = r#"(arc (start 20.75 21.5) (mid 20 22.25) (end 19.4697 22.0303) (width 0.2) (layer "F.Cu") (net 2))"#;
        let report = run(&format!("{PTH}\n  {far}"));
        assert_eq!(count(&report, "pth_to_trace"), 0, "{report:?}");
    }

    #[test]
    fn collinear_arc_points_are_measured_as_a_straight_track() {
        let line = r#"(arc (start 19 20.75) (mid 20 20.75) (end 21 20.75) (width 0.2) (layer "F.Cu") (net 2))"#;
        assert!((pth_gap_of(line) - 0.15).abs() < 0.005);
    }

    #[test]
    fn malformed_tracks_are_counted_not_dropped() {
        let report = run(
            r#"(arc (start 19 21) (end 21 21) (width 0.2) (layer "F.Cu") (net 2))
  (segment (start 19 21) (width 0.2) (layer "F.Cu") (net 2))"#,
        );
        assert_eq!(report.unread_tracks, 2, "{report:?}");
        assert_eq!(run(&arc_trace(20.75, 2)).unread_tracks, 0);
    }

    #[test]
    fn an_arc_track_on_the_holes_own_net_or_far_away_is_not_reported() {
        let own = run(&format!("{PTH}\n  {}", arc_trace(20.75, 1)));
        assert_eq!(count(&own, "pth_to_trace"), 0, "{own:?}");
        let far = run(&format!("{PTH}\n  {}", arc_trace(22.0, 2)));
        assert_eq!(count(&far, "pth_to_trace"), 0, "{far:?}");
    }

    #[test]
    fn a_trace_with_room_around_the_hole_is_not_reported() {
        let report = run(&format!("{PTH}\n  {}", trace("F.Cu", 21.0, 2)));
        assert_eq!(count(&report, "pth_to_trace"), 0, "{report:?}");
    }

    #[test]
    fn the_holes_own_net_may_run_up_to_it() {
        let report = run(&format!("{PTH}\n  {}", trace("F.Cu", 20.75, 1)));
        assert_eq!(count(&report, "pth_to_trace"), 0, "{report:?}");
    }

    #[test]
    fn a_plated_hole_goes_through_so_a_back_trace_counts_and_is_reported_once() {
        let report = run(&format!(
            "{PTH}\n  {}\n  {}",
            trace("B.Cu", 20.75, 2),
            trace("F.Cu", 19.25, 2)
        ));
        assert_eq!(count(&report, "pth_to_trace"), 1, "one finding per hole");
    }

    #[test]
    fn the_merge_tolerance_splits_a_same_net_gap_either_side_of_it() {
        // Base pad right edge 10.3 mm. 0.065 mm: merged. 0.075 mm: graded.
        assert_eq!(
            count(
                &run(&pad_at(10.665, 20.0, 1, "rect", 0.6, "F.Cu")),
                "pad_to_pad"
            ),
            0
        );
        let graded = run(&pad_at(10.675, 20.0, 1, "rect", 0.6, "F.Cu"));
        assert_eq!(count(&graded, "pad_to_pad"), 1, "{graded:?}");
    }

    #[test]
    fn the_pieces_of_one_pin_are_separate_copper_and_graded_too() {
        // A split pin: two pieces 0.1 mm apart. The fab sees two objects.
        let split = r#"(footprint "Test:EP" (layer "F.Cu") (at 30 30)
    (property "Reference" "U9" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "GND"))
    (pad "1" smd rect (at 0.7 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "GND")))"#;
        let report = run(split);
        assert_eq!(count(&report, "pad_to_pad"), 1, "{report:?}");
        assert_eq!(report.findings[0].severity, DfmSeverity::Warning);
    }

    #[test]
    fn a_via_on_its_own_pad_is_one_object_but_on_another_nets_pad_it_is_a_short() {
        let via_on_pad = |net: u32| {
            format!(r#"(via (at 10 20) (size 0.4) (drill 0.2) (layers "F.Cu" "B.Cu") (net {net}))"#)
        };
        assert_eq!(count(&run(&via_on_pad(1)), "pad_to_pad"), 0);
        let short = run(&via_on_pad(2));
        assert_eq!(count(&short, "pad_to_pad"), 1, "{short:?}");
        assert_eq!(short.findings[0].severity, DfmSeverity::Danger);
    }

    fn pad_text(body: &str) -> String {
        format!(r#"(footprint "Test:X" (layer "F.Cu") (at 20 20) {body})"#)
    }

    #[test]
    fn a_non_plated_hole_is_a_silk_to_hole_finding_only() {
        let npth = pad_text(
            r#"(pad "" np_thru_hole circle (at 0 0) (size 1.0 1.0) (drill 1.0) (layers "*.Cu" "*.Mask"))"#,
        );
        let report = run(&format!(
            "{npth}\n  {}",
            silk("F.SilkS", 20.6, 19.0, 20.6, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_hole"), 1, "{report:?}");
        assert_eq!(count(&report, "silk_to_pad"), 0, "{report:?}");
    }

    #[test]
    fn a_solid_filled_silk_shape_over_a_pad_is_a_danger() {
        // 5 mm square around the 0.6 mm pad: every stroke is far from it.
        let rect = |fill: &str| {
            format!(
                r#"(gr_rect (start 7.5 17.5) (end 12.5 22.5) (stroke (width 0.15) (type solid)) (fill {fill}) (layer "F.SilkS"))"#
            )
        };
        assert_eq!(count(&run(&rect("none")), "silk_to_pad"), 0);
        assert_eq!(count(&run(&rect("solid")), "silk_to_pad"), 1);
    }

    #[test]
    fn a_solid_filled_silk_shape_over_a_hole_is_a_danger() {
        let fill = r#"(gr_rect (start 17.5 17.5) (end 22.5 22.5) (stroke (width 0.15) (type solid)) (fill solid) (layer "F.SilkS"))"#;
        let report = run(&format!("{PTH}\n  {fill}"));
        assert_eq!(count(&report, "silk_to_hole"), 1, "{report:?}");
    }

    #[test]
    fn a_slot_is_a_stadium_not_a_circle_of_its_longer_side() {
        // 0.6 x 2.0 mm slot along x. A line 0.8 mm above its centre line is
        // 0.8 - 0.3 - 0.075 = 0.425 mm clear: fine. A circle of the 2.0 mm
        // dimension would have said zero.
        let slot = pad_text(
            r#"(pad "1" thru_hole oval (at 0 0) (size 3 1.6) (drill oval 2.0 0.6) (layers "*.Cu" "*.Mask") (net 1 "GND"))"#,
        );
        let across = run(&format!(
            "{slot}\n  {}",
            silk("F.SilkS", 19.0, 19.2, 21.0, 19.2)
        ));
        assert_eq!(count(&across, "silk_to_hole"), 0, "{across:?}");
        // Past the rounded end along the axis: 1.3 mm from the centre is 0.3 mm
        // beyond the slot end, still 0.225 mm clear of the stroke half-width.
        let along = run(&format!(
            "{slot}\n  {}",
            silk("F.SilkS", 21.3, 19.0, 21.3, 21.0)
        ));
        assert_eq!(count(&along, "silk_to_hole"), 0, "{along:?}");
        let close = run(&format!(
            "{slot}\n  {}",
            silk("F.SilkS", 21.2, 19.0, 21.2, 21.0)
        ));
        assert_eq!(count(&close, "silk_to_hole"), 1, "{close:?}");
    }

    #[test]
    fn an_offset_drill_leaves_the_hole_at_the_pad_and_moves_the_copper() {
        // KiCad: the hole stays at (20, 20); the 3 mm copper is centred at (21, 20).
        let offset = pad_text(
            r#"(pad "1" thru_hole circle (at 0 0) (size 3 3) (drill 1.0 (offset 1.0 0)) (layers "*.Cu" "*.Mask") (net 1 "GND"))"#,
        );
        let run_with = |line: String| run(&format!("{offset}\n  {line}"));
        // Hole edge at x = 20.5: a line at 20.6 is 0.1 - 0.075 clear of it.
        let hole = run_with(silk("F.SilkS", 20.6, 19.0, 20.6, 21.0));
        assert_eq!(count(&hole, "silk_to_hole"), 1, "{hole:?}");
        // Where the hole would be if it moved with the offset: nothing there.
        let moved = run_with(silk("F.SilkS", 21.6, 19.0, 21.6, 21.0));
        assert_eq!(count(&moved, "silk_to_hole"), 0, "{moved:?}");
        // Copper's right edge is at 22.5; centred on `at` it would end at 21.5.
        let copper = run_with(silk("F.SilkS", 22.6, 19.0, 22.6, 21.0));
        assert_eq!(count(&copper, "silk_to_pad"), 1, "{copper:?}");
    }

    #[test]
    fn a_rotated_offset_drill_moves_the_copper_with_the_pad() {
        // Pad turned 90 degrees: the (1, 0) offset lands at (0, -1), so the
        // copper is centred at (20, 19) and the hole stays at (20, 20).
        let rotated = pad_text(
            r#"(pad "1" thru_hole circle (at 0 0 90) (size 3 3) (drill 1.0 (offset 1.0 0)) (layers "*.Cu" "*.Mask") (net 1 "GND"))"#,
        );
        // Copper top edge at y = 17.5: a line at 17.4 is 0.1 - 0.075 clear.
        let near = run(&format!(
            "{rotated}\n  {}",
            silk("F.SilkS", 19.0, 17.4, 21.0, 17.4)
        ));
        assert_eq!(count(&near, "silk_to_pad"), 1, "{near:?}");
        // Unrotated, the copper would reach x = 22.5.
        let unrotated_spot = run(&format!(
            "{rotated}\n  {}",
            silk("F.SilkS", 22.6, 19.0, 22.6, 21.0)
        ));
        assert_eq!(
            count(&unrotated_spot, "silk_to_pad"),
            0,
            "{unrotated_spot:?}"
        );
    }

    #[test]
    fn a_fab_with_no_dfm_floors_skips_those_rules_and_says_so() {
        // Silk over the pad, a thin line, and a second pad 0.07 mm away: each
        // would be a finding under the JLC floors.
        let extra = format!(
            "{}\n  {}\n  {}",
            silk("F.SilkS", 10.0, 19.0, 10.0, 21.0),
            silk("F.SilkS", 12.0, 19.0, 12.0, 21.0),
            pad_at(10.67, 20.0, 2, "rect", 0.6, "F.Cu")
        );
        let text = board(&format!("{PTH}\n  {extra}"));
        for profile in [
            ManufacturerProfile::pcbway_standard(),
            ManufacturerProfile::oshpark_4layer(),
        ] {
            let report = check_dfm(&text, &profile).expect("parses");
            for rule in [
                "silk_to_pad",
                "silk_to_hole",
                "silk_line_width",
                "pad_to_pad",
            ] {
                assert_eq!(count(&report, rule), 0, "{} {rule}", profile.name);
                assert!(
                    report.unchecked_rules.contains(&rule),
                    "{} {rule}",
                    profile.name
                );
            }
            assert!(!report.unchecked_rules.contains(&"pth_to_trace"));
        }
    }

    #[test]
    fn a_custom_pad_is_measured_by_its_primitives_and_counted_as_approximate() {
        let custom = pad_text(
            r#"(pad "1" smd custom (at 0 0) (size 0.2 0.2) (layers "F.Cu") (net 1 "GND")
      (primitives (gr_poly (pts (xy -1 -1) (xy 3 -1) (xy 3 1) (xy -1 1)) (width 0) (fill yes)))"#,
        ) + ")";
        // The anchor is 0.2 mm; the copper reaches x = 23. Silk over it at x = 22.
        let report = run(&format!(
            "{custom}\n  {}",
            silk("F.SilkS", 22.0, 19.0, 22.0, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_pad"), 1, "{report:?}");
        assert_eq!(report.approximated_pads, 1);
    }

    #[test]
    fn silk_curves_and_legacy_arcs_are_counted_not_dropped() {
        let report = run(
            r#"(gr_curve (pts (xy 5 5) (xy 6 6) (xy 7 6) (xy 8 5)) (stroke (width 0.15) (type solid)) (layer "F.SilkS"))
  (gr_arc (start 5 5) (end 6 5) (angle 90) (stroke (width 0.15) (type solid)) (layer "F.SilkS"))"#,
        );
        assert_eq!(report.unchecked_silk_graphics, 2, "{report:?}");
        assert_eq!(run("").unchecked_silk_graphics, 0);
    }

    #[test]
    fn an_f_and_b_pad_meets_a_pad_on_either_outer_layer() {
        let both = r#"(footprint "Test:FB" (layer "F.Cu") (at 10.7 20)
    (pad "1" smd rect (at 0 0) (size 0.6 0.6) (layers "F&B.Cu") (net 2 "SIG")))"#;
        assert_eq!(count(&run(both), "pad_to_pad"), 1);
        // And silk on the front sees it.
        let report = run(&format!(
            "{both}\n  {}",
            silk("F.SilkS", 10.9, 19.0, 10.9, 21.0)
        ));
        assert_eq!(count(&report, "silk_to_pad"), 1, "{report:?}");
    }

    #[test]
    fn a_net_zero_trace_is_not_the_net_of_a_net_zero_hole() {
        let unconnected = PTH.replace(r#"(net 1 "GND")"#, r#"(net 0 "")"#);
        let report = run(&format!("{unconnected}\n  {}", trace("F.Cu", 20.75, 0)));
        assert_eq!(count(&report, "pth_to_trace"), 1, "{report:?}");
    }

    #[test]
    fn a_profile_with_no_published_floor_skips_the_rule_and_says_so() {
        let mut profile = ManufacturerProfile::jlc_standard();
        profile.min_pad_to_pad_nm = 0;
        let text = board(&pad_at(10.67, 20.0, 2, "rect", 0.6, "F.Cu"));
        let report = check_dfm(&text, &profile).expect("parses");
        assert_eq!(count(&report, "pad_to_pad"), 0);
        assert_eq!(report.unchecked_rules, vec!["pad_to_pad"]);
    }

    #[test]
    fn an_unreadable_board_is_an_error_not_a_clean_result() {
        assert!(check_dfm("(kicad_pcb (via", &ManufacturerProfile::jlc_standard()).is_err());
    }

    #[test]
    fn overlapping_unconnected_pads_are_one_object_not_a_zero_gap() {
        // A split NC pin: two overlapping pads on net 0.
        let report = run(&format!(
            "{}\n  {}",
            pad_at(20.0, 20.0, 0, "rect", 0.6, "F.Cu"),
            pad_at(20.3, 20.0, 0, "rect", 0.6, "F.Cu")
        ));
        assert_eq!(count(&report, "pad_to_pad"), 0, "{report:?}");
        // Overlapping copper of two real, different nets is a short.
        let short = run(&format!(
            "{}\n  {}",
            pad_at(20.0, 20.0, 1, "rect", 0.6, "F.Cu"),
            pad_at(20.3, 20.0, 2, "rect", 0.6, "F.Cu")
        ));
        assert_eq!(count(&short, "pad_to_pad"), 1, "{short:?}");
        assert_eq!(short.findings[0].severity, DfmSeverity::Danger);
    }

    #[test]
    fn a_gap_that_meets_the_profile_floor_is_never_a_danger() {
        let mut profile = ManufacturerProfile::jlc_standard();
        profile.min_pad_to_pad_nm = 80_000;
        profile.pad_to_pad_danger_nm = 100_000;
        assert_eq!(spacing_grade(90_000.0, &profile), None);
        assert_eq!(
            spacing_grade(70_000.0, &profile),
            Some((DfmSeverity::Danger, 80_000.0))
        );
    }

    #[test]
    fn a_via_barrel_close_to_another_nets_trace_is_a_finding() {
        // Via drill 0.2 at (12, 20); a 0.2 mm trace of net 2 along y = 20.3:
        // 0.3 - 0.1 - 0.1 = 0.1 mm clear, under the 0.28 mm floor.
        let trace = trace_at(12.0, 20.3, 2);
        let report = run(&format!("{VIA}\n  {trace}"));
        assert_eq!(count(&report, "pth_to_trace"), 1, "{report:?}");
        // The via's own net may run up to it.
        let own = run(&format!("{VIA}\n  {}", trace_at(12.0, 20.3, 1)));
        assert_eq!(count(&own, "pth_to_trace"), 0, "{own:?}");
    }

    fn trace_at(x: f64, y: f64, net: u32) -> String {
        format!(
            r#"(segment (start {} {y}) (end {} {y}) (width 0.2) (layer "F.Cu") (net {net}))"#,
            x - 1.0,
            x + 1.0
        )
    }

    #[test]
    fn a_trapezoid_pad_reaches_its_wide_end() {
        // 1 x 1 mm with rect_delta (0 0.4): the corners reach y = +-0.7.
        let trap = pad_text(
            r#"(pad "1" smd trapezoid (at 0 0) (size 1 1) (rect_delta 0.4 0) (layers "F.Cu") (net 1 "GND"))"#,
        );
        // y = 20.76 is 0.06 - 0.075 from the wide corner: on it. Measured
        // from size alone the pad ends at 20.5 and this reads 0.185 clear.
        let report = run(&format!(
            "{trap}\n  {}",
            silk("F.SilkS", 19.0, 20.76, 21.0, 20.76)
        ));
        assert_eq!(count(&report, "silk_to_pad"), 1, "{report:?}");
    }

    #[test]
    fn silk_polygon_arcs_are_followed() {
        // The arc bulges left to x = 10.4, 0.1 mm from the pad's right edge
        // (10.3). Its chord stays at x = 12 and would read as far away.
        let report = run(
            r#"(gr_poly (pts (xy 12 19) (arc (start 12 19) (mid 10.4 20) (end 12 21)) (xy 13 21)) (stroke (width 0.15) (type solid)) (fill no) (layer "F.SilkS"))"#,
        );
        assert_eq!(count(&report, "silk_to_pad"), 1, "{report:?}");
    }
}
