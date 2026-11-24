// SPDX-License-Identifier: Apache-2.0

//! Estimate the board a design's parts need, and warn when a declared
//! `outline` is much larger than that.
//!
//! A declared outline is authoritative (ADR 0001): placement solves inside it
//! and never fits it. That is right for a mating constraint and wrong when the
//! size is a guess, where it leaves a large empty board. The estimate sums the
//! parts' courtyards, divides by the fill a fitted board lands on and adds the
//! rim the exporter would fit, so it names the size the same parts would get
//! without the statement.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Patch, PatchKind, Severity};
use synth_ir::{
    Board, Component, PlacementConstraint, PlacementEdge, PlacementPriority, PlacementRegion,
};

/// A declared outline this many times larger (by area) than the estimate is
/// reported. 1.5x leaves room for a real mechanical or mating constraint while
/// catching a size that was never computed from the parts.
const OVERSIZE_AREA_FACTOR: f64 = 1.5;

/// Stable code for a hard placement hint that is not a physical constraint.
const HINT_CODE: &str = "W-SYNTH-BOARD-005";

/// Courtyard area ÷ the pre-rim board area a fitted outline lands on.
///
/// The placer's own target is 0.45, but that is the canvas it packs into; the
/// fitted board also spends area on routing channels. The rim is added after
/// this ratio is applied. Calibrated on
/// four boards: sensor_logger, secure_tracker and feather_m4_express (square
/// estimates, all within 20% of the fitted area) and the gateway (120 parts,
/// 1.5:1 declared aspect): its export fit is 157.4 x 113.4 mm with the hard
/// hints as written and 118.0 x 115.5 mm with the non-physical hard hints
/// soft. 0.26 puts the gateway estimate 23% under the first and within 2% of
/// the second, without overfitting any one board. 0.32 was the first guess and
/// missed the gateway by 37%.
const ESTIMATE_FILL_RATIO: f64 = 0.26;

/// Stable code for both the oversized-outline warning and the estimate info.
const BOARD_ESTIMATE_CODE: &str = "W-SYNTH-BOARD-004";

/// The board size a design's own components need, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardEstimate {
    /// Sum of the parts' courtyard areas, mm².
    pub total_courtyard_mm2: f64,
    /// Estimated board area including the fitted rim, mm².
    pub est_area_mm2: f64,
    /// Estimated board width including the fitted rim, mm.
    pub est_w_mm: f64,
    /// Estimated board height including the fitted rim, mm.
    pub est_h_mm: f64,
}

/// Estimate the board the components need: their courtyard area at the fill a
/// fitted board lands on, plus the tight rim the export fits round them. The
/// area is the gate; a declared outline reshapes the width and height to its
/// aspect ratio, and a board without one is reported square.
#[must_use]
#[allow(clippy::similar_names)]
pub fn estimate_board_size(board: &Board) -> BoardEstimate {
    let total_courtyard_mm2: f64 = board
        .components
        .iter()
        .map(|component| {
            let (w_mm, h_mm) = component.part.as_ref().map_or_else(
                || synth_place::fallback_courtyard(&component.kind),
                synth_layout::pcb_courtyard_for_part,
            );
            w_mm * h_mm
        })
        .sum();

    let side_mm = (total_courtyard_mm2 / ESTIMATE_FILL_RATIO).sqrt();
    let rim_mm = synth_place::tight_edge_margin_mm(board);
    let square_side_mm = side_mm + 2.0 * rim_mm;
    let est_area_mm2 = square_side_mm * square_side_mm;
    let (est_w_mm, est_h_mm) =
        board
            .outline
            .as_ref()
            .map_or((square_side_mm, square_side_mm), |outline| {
                let aspect = outline.width.to_mm() / outline.height.to_mm();
                if aspect.is_finite() && aspect > 0.0 {
                    (
                        (est_area_mm2 * aspect).sqrt(),
                        (est_area_mm2 / aspect).sqrt(),
                    )
                } else {
                    (square_side_mm, square_side_mm)
                }
            });

    BoardEstimate {
        total_courtyard_mm2,
        est_area_mm2,
        est_w_mm,
        est_h_mm,
    }
}

/// Report the estimate, and warn when the declared outline is far larger.
#[must_use]
#[allow(clippy::similar_names)]
pub fn board_size_diagnostics(board: &Board, file: &str) -> Vec<Diagnostic> {
    let estimate = estimate_board_size(board);
    let Some(outline) = board.outline.as_ref() else {
        return vec![
            DiagnosticBuilder::new(BOARD_ESTIMATE_CODE, Severity::Info, "board size estimate")
                .location(Location::from_span(file.to_string(), board.source_span))
                .found(format!(
                    "parts need about {:.1} x {:.1} mm ({:.0} mm2)",
                    estimate.est_w_mm, estimate.est_h_mm, estimate.est_area_mm2,
                ))
                .message(format!(
                    "the parts need about {:.1} x {:.1} mm at a square aspect; \
                     placement fits the outline to them",
                    estimate.est_w_mm, estimate.est_h_mm,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{BOARD_ESTIMATE_CODE}"))
                .build(),
        ];
    };

    // A stated reason is the author's answer to this warning (a mechanical or
    // mating constraint); do not repeat it.
    if outline.reason.is_some() {
        return Vec::new();
    }

    let declared_w_mm = outline.width.to_mm();
    let declared_h_mm = outline.height.to_mm();
    let declared_area_mm2 = declared_w_mm * declared_h_mm;
    if declared_area_mm2 <= OVERSIZE_AREA_FACTOR * estimate.est_area_mm2 {
        return Vec::new();
    }

    let ratio = declared_area_mm2 / estimate.est_area_mm2;
    vec![
        DiagnosticBuilder::new(
            BOARD_ESTIMATE_CODE,
            Severity::Warning,
            "declared outline is far larger than the parts need",
        )
        .location(Location::from_span(file.to_string(), outline.source_span))
        .expected(format!(
            "a declared outline no more than {OVERSIZE_AREA_FACTOR}x the estimated area \
             ({:.1} x {:.1} mm, {:.0} mm2)",
            estimate.est_w_mm, estimate.est_h_mm, estimate.est_area_mm2,
        ))
        .found(format!(
            "outline {declared_w_mm:.1} x {declared_h_mm:.1} mm ({declared_area_mm2:.0} mm2, {ratio:.1}x the estimate)"
        ))
        .message(format!(
            "declared outline {declared_w_mm:.1} x {declared_h_mm:.1} mm is {ratio:.1}x \
             the estimated area the parts need ({:.1} x {:.1} mm); remove the outline or \
             add a stated reason",
            estimate.est_w_mm, estimate.est_h_mm,
        ))
        .suggested_fix(Patch {
            confidence: 0.8,
            rationale: Some(
                "remove the declared outline; placement then fits the board to its parts".to_string(),
            ),
            patch_consequence_preview: None,
            kind: PatchKind::DeleteRange {
                range: outline.source_span,
            },
        })
        .explanation_url(format!("synth.docs/diagnostics/{BOARD_ESTIMATE_CODE}"))
        .build(),
    ]
}

/// Report hard placement hints that are not physical constraints.
///
/// A connector edge or a mounting-hole corner is physical: the enclosure fixes
/// it. A hard `near`, `region` or `edge` on anything else pins a part without a
/// mechanical reason and blocks the compact placement. Measured on the gateway:
/// 157.4 x 113.4 mm with the hints as written, 118.0 x 115.5 mm with the 90
/// non-physical hard hints made soft (the hints' board is 31% larger by area).
#[must_use]
#[allow(clippy::similar_names)]
pub fn hard_hint_diagnostics(board: &Board, file: &str) -> Vec<Diagnostic> {
    let estimate = estimate_board_size(board);
    let (board_w_mm, board_h_mm) = board_rect(board, &estimate);
    let mut out = Vec::new();
    for component in &board.components {
        let Some(hint) = component.placement_hint.as_ref() else {
            continue;
        };
        if hint.priority != PlacementPriority::Hard || is_physical(component, hint) {
            continue;
        }
        let span = forced_span(board, component, board_w_mm, board_h_mm, 8).map_or_else(
            String::new,
            |span| {
                if span.via_near {
                    format!(
                        "it follows a part pinned {:.1} mm from the far side; ",
                        span.mm
                    )
                } else {
                    format!("it pins the part {:.1} mm from the far side; ", span.mm)
                }
            },
        );
        let description = describe_hint(hint);
        out.push(
            DiagnosticBuilder::new(
                HINT_CODE,
                Severity::Warning,
                "hard placement hint on a non-physical part",
            )
            .location(Location::from_span(file.to_string(), component.source_span))
            .expected(
                "a soft placement hint unless the part is physically pinned by a connector \
                 edge or a mounting hole",
            )
            .found(format!(
                "`{}` ({}) {description}",
                component.refdes, component.kind,
            ))
            .message(format!(
                "`{}` ({}) sets {description}, which is not a physical constraint; {span}\
                 the parts need about {:.1} x {:.1} mm. Make it soft unless it is \
                 physical (a connector edge or a mounting hole).",
                component.refdes, component.kind, estimate.est_w_mm, estimate.est_h_mm,
            ))
            .explanation_url(format!("synth.docs/diagnostics/{HINT_CODE}"))
            .build(),
        );
    }
    out
}

/// The rectangle a hint's span is measured on: the declared outline when
/// there is one, else the estimate.
fn board_rect(board: &Board, estimate: &BoardEstimate) -> (f64, f64) {
    board
        .outline
        .as_ref()
        .map_or((estimate.est_w_mm, estimate.est_h_mm), |outline| {
            (outline.width.to_mm(), outline.height.to_mm())
        })
}

/// The edge of a board-edge part and a mounting hole are the two hints a
/// mechanical constraint fixes; every other hard hint is advisory.
fn is_physical(component: &Component, hint: &PlacementConstraint) -> bool {
    component.kind == "mounting_hole"
        || (crate::BOARD_EDGE_KINDS.contains(&component.kind.as_str()) && hint.edge.is_some())
}

/// The distance a hard hint pins its cluster from the opposite board side,
/// and whether it only reaches that anchor through a `near` chain.
struct ForcedSpan {
    mm: f64,
    via_near: bool,
}

/// How far the hint pins a part from the opposite side, following a hard
/// `near` chain to the hint that fixes the cluster's position.
#[allow(clippy::similar_names)]
fn forced_span(
    board: &Board,
    component: &Component,
    board_w_mm: f64,
    board_h_mm: f64,
    depth: usize,
) -> Option<ForcedSpan> {
    let hint = component.placement_hint.as_ref()?;
    if hint.priority != PlacementPriority::Hard {
        return None;
    }
    if let Some(span_mm) = hint_span(hint, board_w_mm, board_h_mm) {
        return Some(ForcedSpan {
            mm: span_mm,
            via_near: false,
        });
    }
    let target_refdes = hint.near.as_deref()?;
    if depth == 0 {
        return None;
    }
    let target = board
        .components
        .iter()
        .find(|candidate| candidate.refdes == target_refdes)?;
    forced_span(board, target, board_w_mm, board_h_mm, depth - 1).map(|span| ForcedSpan {
        via_near: true,
        ..span
    })
}

#[allow(clippy::similar_names)]
fn hint_span(hint: &PlacementConstraint, board_w_mm: f64, board_h_mm: f64) -> Option<f64> {
    if let Some(edge) = &hint.edge {
        return Some(match edge {
            PlacementEdge::Left | PlacementEdge::Right => board_w_mm,
            PlacementEdge::Top | PlacementEdge::Bottom => board_h_mm,
        });
    }
    hint.region
        .as_ref()
        .map(|region| region_span(region, board_w_mm, board_h_mm))
}

#[allow(clippy::similar_names)]
fn region_span(region: &PlacementRegion, board_w_mm: f64, board_h_mm: f64) -> f64 {
    match region {
        PlacementRegion::TopLeft
        | PlacementRegion::TopRight
        | PlacementRegion::BottomLeft
        | PlacementRegion::BottomRight => board_w_mm.hypot(board_h_mm),
        PlacementRegion::Centre => (board_w_mm / 2.0).hypot(board_h_mm / 2.0),
        PlacementRegion::TopEdge | PlacementRegion::BottomEdge => board_h_mm,
        PlacementRegion::LeftEdge | PlacementRegion::RightEdge => board_w_mm,
    }
}

fn describe_hint(hint: &PlacementConstraint) -> String {
    let mut parts = Vec::new();
    if let Some(edge) = &hint.edge {
        parts.push(format!("edge: {}", edge_name(edge)));
    }
    if let Some(region) = &hint.region {
        parts.push(format!("region: {}", region_name(region)));
    }
    if let Some(near) = &hint.near {
        parts.push(format!("near: \"{near}\""));
    }
    parts.push("priority: hard".to_string());
    format!("placement_hint {{ {} }}", parts.join(", "))
}

fn edge_name(edge: &PlacementEdge) -> &'static str {
    match edge {
        PlacementEdge::Top => "top",
        PlacementEdge::Bottom => "bottom",
        PlacementEdge::Left => "left",
        PlacementEdge::Right => "right",
    }
}

fn region_name(region: &PlacementRegion) -> &'static str {
    match region {
        PlacementRegion::TopLeft => "top_left",
        PlacementRegion::TopRight => "top_right",
        PlacementRegion::BottomLeft => "bottom_left",
        PlacementRegion::BottomRight => "bottom_right",
        PlacementRegion::Centre => "centre",
        PlacementRegion::TopEdge => "top_edge",
        PlacementRegion::BottomEdge => "bottom_edge",
        PlacementRegion::LeftEdge => "left_edge",
        PlacementRegion::RightEdge => "right_edge",
    }
}
