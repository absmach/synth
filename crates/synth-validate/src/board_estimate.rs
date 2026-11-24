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
use synth_ir::Board;

/// A declared outline this many times larger (by area) than the estimate is
/// reported. 1.5x leaves room for a real mechanical or mating constraint while
/// catching a size that was never computed from the parts.
const OVERSIZE_AREA_FACTOR: f64 = 1.5;

/// Courtyard area ÷ board area the fitted outline lands on.
///
/// The placer's own target is 0.45, but that is the canvas it packs into; the
/// fitted board also carries routing channels and the edge rim, so its fill
/// fills 26-29% on the reference boards (sensor_logger 756/2883 mm²,
/// secure_tracker 994/3760, feather_m4_express 977/3417). 0.32 reproduces
/// those fitted sizes to within 12% of area; using 0.45 underestimates them by
/// about a third.
const ESTIMATE_FILL_RATIO: f64 = 0.32;

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

/// Estimate the square board the components need: their courtyard area at the
/// fill a fitted board lands on, plus the tight rim the export fits round them.
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
    let est_w_mm = side_mm + 2.0 * rim_mm;
    let est_h_mm = side_mm + 2.0 * rim_mm;

    BoardEstimate {
        total_courtyard_mm2,
        est_area_mm2: est_w_mm * est_h_mm,
        est_w_mm,
        est_h_mm,
    }
}

/// Report the estimate, and warn when the declared outline is far larger.
#[must_use]
#[allow(clippy::similar_names)]
pub fn board_size_diagnostics(board: &Board, file: &str) -> Vec<Diagnostic> {
    let estimate = estimate_board_size(board);
    let Some(outline) = board.outline else {
        return vec![
            DiagnosticBuilder::new(BOARD_ESTIMATE_CODE, Severity::Info, "board size estimate")
                .location(Location::from_span(file.to_string(), board.source_span))
                .found(format!(
                    "parts need about {:.1} x {:.1} mm ({:.0} mm2)",
                    estimate.est_w_mm, estimate.est_h_mm, estimate.est_area_mm2,
                ))
                .message(format!(
                    "the parts need about {:.1} x {:.1} mm at the fitted fill; \
                 placement fits the outline to them",
                    estimate.est_w_mm, estimate.est_h_mm,
                ))
                .explanation_url(format!("synth.docs/diagnostics/{BOARD_ESTIMATE_CODE}"))
                .build(),
        ];
    };

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
            "a declared outline within {OVERSIZE_AREA_FACTOR}x the estimated {:.1} x {:.1} mm ({:.0} mm2)",
            estimate.est_w_mm, estimate.est_h_mm, estimate.est_area_mm2,
        ))
        .found(format!(
            "outline {declared_w_mm:.1} x {declared_h_mm:.1} mm ({declared_area_mm2:.0} mm2, {ratio:.1}x the estimate)"
        ))
        .message(format!(
            "declared outline {declared_w_mm:.1} x {declared_h_mm:.1} mm is {ratio:.1}x \
             the estimated {:.1} x {:.1} mm the parts need; remove the outline or \
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
