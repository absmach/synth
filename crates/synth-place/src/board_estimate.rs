// SPDX-License-Identifier: Apache-2.0

//! The board size a design's parts need.
//!
//! A declared outline is authoritative (ADR 0001): placement solves inside it
//! and never fits it. That is right for a mating constraint and wrong when the
//! size is a guess, where it leaves a large empty board. The estimate sums the
//! parts' courtyards, divides by the fill a fitted board lands on and adds the
//! rim the exporter would fit, so it names the size the same parts would get
//! without the statement.
//!
//! It also seeds the legal-placement ladder ([`crate::place_legal`]): when a
//! fixed canvas fails, the estimate is the first size worth retrying.

use synth_ir::Board;

/// Courtyard area ÷ the pre-rim board area a fitted outline lands on.
///
/// The placer's own target is 0.45, but that is the canvas it packs into; the
/// fitted board also spends area on routing channels. The rim is added after
/// this ratio is applied. Calibrated on
/// four boards: sensor_logger, secure_tracker and feather_m4_express (square
/// estimates, all within 20% of the fitted area) and the gateway (120 parts,
/// 1.5:1 declared aspect): its export fit is 157.4 x 113.4 mm with the hints
/// as written, and softening the only span-fixing hints changes nothing. 0.26
/// puts the gateway estimate at 144.2 x 96.1 mm (13 859 mm2), 22% under that
/// fit, without overfitting any one board. 0.32 was the first guess and missed
/// the gateway by 37%.
const ESTIMATE_FILL_RATIO: f64 = 0.26;

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
                || crate::fallback_courtyard(&component.kind),
                synth_layout::pcb_courtyard_for_part,
            );
            w_mm * h_mm
        })
        .sum();

    let side_mm = (total_courtyard_mm2 / ESTIMATE_FILL_RATIO).sqrt();
    let rim_mm = crate::tight_edge_margin_mm(board);
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
