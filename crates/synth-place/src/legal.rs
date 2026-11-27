// SPDX-License-Identifier: Apache-2.0

//! A legal placement, or the evidence that none was found.
//!
//! The single-shot placer returns `E-SYNTH-PLACE-002` when its greedy packing
//! boxes itself in. That verdict says "this ordering failed on this canvas",
//! not "no legal placement exists": a hard hint may be the only blocker, and a
//! slightly larger canvas may admit the same parts. This module turns the
//! single shot into a bounded ladder:
//!
//! 1. the production placement ([`crate::place`]), hints as written;
//! 2. a fixed canvas — the declared outline, else the board estimate;
//! 3. the hint levels: as written, soft, dropped but connector edges kept,
//!    dropped everywhere (each level reports every hint it dropped);
//! 4. canvas growth in steps from that first fixed size, up to a ceiling.
//!
//! Every attempt is bounded by a wall-clock budget and an attempt count. The
//! budget is checked between attempts; a single solve cannot be interrupted,
//! so a board that places slowly can overrun it.
//!
//! [`crate::place`] keeps its single-shot contract: callers that need the
//! guarantee ask for it here.

use std::time::{Duration, Instant};

use synth_ir::{Board, PlacementPriority, PlacementRegion};

use crate::{PlaceError, Placement, place, place_with_dimensions};

/// How many sizes the fixed-canvas ladder grows through, as a factor of the
/// first fixed canvas. The last rung is the ceiling.
const CANVAS_GROWTH: [f64; 5] = [1.0, 1.2, 1.4, 1.7, 2.0];

/// No grown canvas exceeds this side, whatever the estimate says.
const MAX_CANVAS_SIDE_MM: f64 = 300.0;

/// Default wall-clock budget for [`place_legal`].
///
/// The CM4 carrier needs five 141 x 141 mm attempts before the 1.2x canvas
/// succeeds, so 30 s cut the ladder mid-way and made the verdict depend on
/// machine load. 60 s lets that ladder finish; the preflight is opt-in
/// (`--placement-preflight`), so the default `validate` never pays it.
pub const DEFAULT_LEGAL_BUDGET: Duration = Duration::from_secs(60);

/// Default attempt ceiling: one production solve plus six canvases at four
/// hint levels, so the full cross-product is reachable. Six is the most
/// `fixed_canvases` returns: a declared outline smaller than the estimate,
/// the estimate, then 1.2, 1.4, 1.7 and 2.0 times the estimate. The wall-clock
/// budget is the real bound on a slow board; this only stops a retry loop
/// that would never end.
pub const DEFAULT_LEGAL_ATTEMPTS: usize = 25;

/// What a caller will spend on the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegalOptions {
    /// Wall-clock budget, checked between attempts. `None` means unbounded.
    pub budget: Option<Duration>,
    /// Maximum number of solves, the production one included.
    pub max_attempts: usize,
    /// Grow the fixed canvas past the estimate when every hint level fails.
    pub grow_canvas: bool,
}

impl Default for LegalOptions {
    fn default() -> Self {
        Self {
            budget: Some(DEFAULT_LEGAL_BUDGET),
            max_attempts: DEFAULT_LEGAL_ATTEMPTS,
            grow_canvas: true,
        }
    }
}

/// Which hint level produced the placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintLevel {
    /// The hints exactly as the source wrote them.
    AsWritten,
    /// Every hard hint demoted to soft; the geometry is kept.
    Soft,
    /// Every hint dropped except a connector's edge, which stays hard.
    DroppedEdgesKept,
    /// Every hint dropped, connector edges included.
    DroppedAll,
}

impl HintLevel {
    fn label(self) -> &'static str {
        match self {
            Self::AsWritten => "as written",
            Self::Soft => "soft",
            Self::DroppedEdgesKept => "dropped (connector edges kept)",
            Self::DroppedAll => "dropped",
        }
    }
}

/// One hint the ladder had to relax, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedHint {
    pub refdes: String,
    /// The hint as the source wrote it.
    pub hint: String,
    /// The level that relaxed it.
    pub action: &'static str,
}

/// A legal placement and the evidence of what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct LegalPlacement {
    pub placement: Placement,
    pub level: HintLevel,
    /// Every hint relaxed to get here, in source order. Empty when the hints
    /// were honoured as written.
    pub dropped: Vec<DroppedHint>,
    /// The canvases tried, in order, in millimetres, each listed once however
    /// many hint levels ran on it. The production attempt that auto-fits has
    /// no entry of its own.
    pub canvases: Vec<(f64, f64)>,
    /// The canvas this placement solved on.
    pub final_canvas: (f64, f64),
    /// Solves attempted, the production one included.
    pub attempts: usize,
}

impl LegalPlacement {
    /// A one-line warning naming every relaxed hint, or `None` when nothing
    /// was relaxed. Callers that only print can use this directly.
    #[must_use]
    pub fn relaxation_warning(&self) -> Option<String> {
        let grew = self
            .canvases
            .first()
            .is_some_and(|first| *first != self.final_canvas);
        let growth = format!(
            "grew the board to {:.1} x {:.1} mm",
            self.final_canvas.0, self.final_canvas.1
        );
        if self.dropped.is_empty() {
            return grew.then(|| format!("placement {growth}"));
        }
        let hints = self
            .dropped
            .iter()
            .map(|dropped| format!("`{}` {}", dropped.refdes, dropped.hint))
            .collect::<Vec<_>>()
            .join("; ");
        Some(format!(
            "placement relaxed {} hint(s) to {}{}: {hints}",
            self.dropped.len(),
            self.level.label(),
            if grew {
                format!(" and {growth}")
            } else {
                String::new()
            }
        ))
    }
}

/// Why no placement was found inside the budget.
#[derive(Debug, Clone, PartialEq)]
pub struct LegalFailure {
    /// The part the last attempt could not place.
    pub refdes: String,
    /// Its hint as written, when the source gave it one.
    pub hint: Option<DroppedHint>,
    /// Every canvas tried, in millimetres.
    pub canvases: Vec<(f64, f64)>,
    /// The board the parts need, from the estimate.
    pub needed: (f64, f64),
    pub attempts: usize,
    /// True when the budget or attempt ceiling stopped the ladder before every
    /// combination was tried: absence of a placement is then unproven.
    pub budget_exhausted: bool,
}

/// Find a legal placement, relaxing hints and growing the canvas in a bounded
/// ladder.
///
/// The production solve runs first, so a board that already places is
/// returned unchanged. Only a failure moves down the ladder.
///
/// # Errors
///
/// [`LegalFailure`] when every attempt inside the budget failed. A failure
/// with `budget_exhausted` set is not proof that no legal placement exists.
#[allow(clippy::result_large_err)]
pub fn place_legal(board: &Board, options: &LegalOptions) -> Result<LegalPlacement, LegalFailure> {
    let started = Instant::now();
    let estimate = crate::estimate_board_size(board);
    let needed = (estimate.est_w_mm, estimate.est_h_mm);
    let mut canvases: Vec<(f64, f64)> = Vec::new();
    let mut attempts = 0_usize;

    let out_of_budget = |attempts: usize, started: Instant| {
        attempts >= options.max_attempts
            || options
                .budget
                .is_some_and(|budget| started.elapsed() >= budget)
    };

    // The production path first: a board that places as written must come out
    // exactly as it does today, and its auto-fitted canvas is the answer.
    attempts += 1;
    let production_start = Instant::now();
    let mut last_error = match place(board) {
        Ok(placement) => {
            rung_trace(
                "production",
                None,
                Some(attempts),
                production_start.elapsed(),
            );
            let width = synth_geometry::nm_to_mm(placement.board_outline.width_nm());
            let height = synth_geometry::nm_to_mm(placement.board_outline.height_nm());
            return Ok(LegalPlacement {
                placement,
                level: HintLevel::AsWritten,
                dropped: Vec::new(),
                canvases,
                final_canvas: (width, height),
                attempts,
            });
        }
        Err(error) => error,
    };
    rung_trace("production", None, None, production_start.elapsed());

    let largest_part = largest_courtyard_mm(board);
    for (width, height) in fixed_canvases(board, &needed, options.grow_canvas) {
        // A canvas that cannot hold the largest part in either rotation is not
        // a placement problem the solver can answer: `cem_region_assign`
        // clamps into an empty range and panics. Skip it, so the ladder ends
        // in a clean E-SYNTH-PLACE-002. It is still listed as ruled out.
        canvases.push((width, height));
        if !canvas_holds(width, height, largest_part) {
            continue;
        }
        for level in [
            HintLevel::AsWritten,
            HintLevel::Soft,
            HintLevel::DroppedEdgesKept,
            HintLevel::DroppedAll,
        ] {
            if out_of_budget(attempts, started) {
                return Err(failure(
                    board,
                    Some(&last_error),
                    canvases,
                    needed,
                    attempts,
                    true,
                ));
            }
            attempts += 1;
            let (relaxed, dropped) = relaxed_board(board, level);
            let rung_start = Instant::now();
            let result = place_with_dimensions(&relaxed, width, height);
            rung_trace(
                &format!("{width:.1}x{height:.1}"),
                Some(level.label()),
                Some(attempts),
                rung_start.elapsed(),
            );
            match result {
                Ok(placement) => {
                    return Ok(LegalPlacement {
                        placement,
                        level,
                        dropped,
                        canvases,
                        final_canvas: (width, height),
                        attempts,
                    });
                }
                Err(error) => last_error = error,
            }
        }
    }

    Err(failure(
        board,
        Some(&last_error),
        canvases,
        needed,
        attempts,
        false,
    ))
}

/// Print one rung's wall clock when `SYNTH_PLACE_RUNG_TRACE` is set.
fn rung_trace(stage: &str, level: Option<&str>, attempt: Option<usize>, elapsed: Duration) {
    if std::env::var_os("SYNTH_PLACE_RUNG_TRACE").is_some() {
        eprintln!(
            "rung attempt={} stage={stage} level={} {:.2}s",
            attempt.map_or_else(|| "-".to_string(), |n| n.to_string()),
            level.unwrap_or("-"),
            elapsed.as_secs_f64()
        );
    }
}

/// The long and short side of the largest part courtyard, millimetres.
fn largest_courtyard_mm(board: &Board) -> (f64, f64) {
    board
        .components
        .iter()
        .map(|component| {
            let (w_mm, h_mm) = component.part.as_ref().map_or_else(
                || crate::fallback_courtyard(&component.kind),
                synth_layout::pcb_courtyard_for_part,
            );
            (w_mm.max(h_mm), w_mm.min(h_mm))
        })
        .fold((0.0, 0.0), |(long, short): (f64, f64), (l, s)| {
            (long.max(l), short.max(s))
        })
}

/// Whether a `width` x `height` canvas can hold a part of `(long, short)`
/// sides in some rotation.
fn canvas_holds(width: f64, height: f64, (long, short): (f64, f64)) -> bool {
    width.max(height) >= long && width.min(height) >= short
}

/// The fixed canvases the ladder tries, in order, deduplicated.
fn fixed_canvases(board: &Board, needed: &(f64, f64), grow_canvas: bool) -> Vec<(f64, f64)> {
    let declared = board
        .outline
        .as_ref()
        .map(|outline| (outline.width.to_mm(), outline.height.to_mm()));
    let base = declared.unwrap_or(*needed);
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut push = |size: (f64, f64)| {
        // Millimetre-granular, so a declared 148 x 108 and the same estimate
        // do not become two identical solves.
        let key = (size.0.round(), size.1.round());
        if !out
            .iter()
            .any(|existing| (existing.0.round(), existing.1.round()) == key)
        {
            out.push(size);
        }
    };
    push(base);
    // The estimate and the growth steps are grown canvases. A declared
    // outline is authoritative (ADR 0001), so without `grow_canvas` it is the
    // only canvas the ladder tries.
    if grow_canvas {
        // A declared outline smaller than the estimate gets the estimate as
        // its first grown rung; one that is already larger keeps it.
        if needed.0 > base.0 || needed.1 > base.1 {
            push(*needed);
        }
        for factor in &CANVAS_GROWTH[1..] {
            push((
                (base.0 * factor).min(MAX_CANVAS_SIDE_MM),
                (base.1 * factor).min(MAX_CANVAS_SIDE_MM),
            ));
        }
    }
    out
}

/// The board at one hint level, plus every hint that level relaxed.
fn relaxed_board(board: &Board, level: HintLevel) -> (Board, Vec<DroppedHint>) {
    let mut relaxed = board.clone();
    let mut dropped = Vec::new();
    for component in &mut relaxed.components {
        let Some(hint) = component.placement_hint.as_ref() else {
            continue;
        };
        let description = describe_hint(hint);
        let keep = level == HintLevel::AsWritten
            || (level == HintLevel::DroppedEdgesKept && keep_connector_edge(&component.kind, hint));
        if keep {
            continue;
        }
        let action = if level == HintLevel::Soft {
            let mut soft = hint.clone();
            soft.priority = PlacementPriority::Soft;
            component.placement_hint = Some(soft);
            "made soft"
        } else {
            component.placement_hint = None;
            "dropped"
        };
        dropped.push(DroppedHint {
            refdes: component.refdes.clone(),
            hint: description,
            action,
        });
    }
    (relaxed, dropped)
}

/// A connector's edge is where the enclosure puts it, so it stays hard at the
/// `DroppedEdgesKept` level. Only `DroppedAll` relaxes it.
fn keep_connector_edge(kind: &str, hint: &synth_ir::PlacementConstraint) -> bool {
    matches!(kind, "connector" | "jack" | "receptacle")
        && hint.edge.is_some()
        && hint.near.is_none()
        && hint.region.is_none()
}

fn failure(
    board: &Board,
    last_error: Option<&PlaceError>,
    canvases: Vec<(f64, f64)>,
    needed: (f64, f64),
    attempts: usize,
    budget_exhausted: bool,
) -> LegalFailure {
    let refdes = match last_error {
        Some(PlaceError::NoLegalPosition { refdes, .. }) => refdes.clone(),
        Some(error) => error.to_string(),
        None => "board".to_string(),
    };
    let hint = board
        .components
        .iter()
        .find(|component| component.refdes == refdes)
        .and_then(|component| component.placement_hint.as_ref())
        .map(|hint| DroppedHint {
            refdes: refdes.clone(),
            hint: describe_hint(hint),
            action: "blocks",
        });
    LegalFailure {
        refdes,
        hint,
        canvases,
        needed,
        attempts,
        budget_exhausted,
    }
}

/// The hint's source form, for a report or a diagnostic.
#[must_use]
pub fn describe_hint(hint: &synth_ir::PlacementConstraint) -> String {
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
    if let Some(side) = &hint.side {
        parts.push(format!("side: {side:?}"));
    }
    parts.push(format!(
        "priority: {}",
        if hint.priority == PlacementPriority::Hard {
            "hard"
        } else {
            "soft"
        }
    ));
    format!("placement_hint {{ {} }}", parts.join(", "))
}

fn edge_name(edge: &synth_ir::PlacementEdge) -> &'static str {
    match edge {
        synth_ir::PlacementEdge::Top => "top",
        synth_ir::PlacementEdge::Bottom => "bottom",
        synth_ir::PlacementEdge::Left => "left",
        synth_ir::PlacementEdge::Right => "right",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A dense connector's hard `edge: top` forces it horizontal, which is
    /// wider than this portrait board. Dropping the hint lets the floorplan
    /// stand it up, so the hint is the only blocker.
    const TIGHT: &str = r#"board "tight_hints" {
        layers 2
        outline 40mm x 70mm
        component J1: connector "header_1x20" {
            placement_hint { edge: top priority: hard }
        }
        component U1: mcu "rp2350"
        component C1: capacitor "c_generic_0603"
        component C2: capacitor "c_generic_0603"
        component C3: capacitor "c_generic_0603"
        component R1: resistor "r_generic_0603"
        component R2: resistor "r_generic_0603"
        connect J1.p1 -> U1.vdd_io
        connect U1.gnd -> C1.p2
    }"#;

    fn board() -> Board {
        crate::tests::board_from_source(TIGHT, "tight_hints.synth")
    }

    #[test]
    fn a_placement_that_works_as_written_reports_no_relaxation() {
        let board = crate::tests::board_from_source(
            r#"board "easy" {
                layers 2
                component R1: resistor "r_generic_0603"
                component R2: resistor "r_generic_0603"
            }"#,
            "easy.synth",
        );
        let legal = place_legal(&board, &LegalOptions::default()).expect("place");
        assert_eq!(legal.level, HintLevel::AsWritten);
        assert!(legal.dropped.is_empty());
        assert!(legal.relaxation_warning().is_none());
        assert_eq!(legal.attempts, 1, "the production solve is enough");
    }

    #[test]
    fn a_canvas_that_is_too_short_grows_in_steps_and_reports_the_size() {
        let board = crate::tests::board_from_source(
            &TIGHT.replace("outline 40mm x 70mm", "outline 40mm x 60mm"),
            "tight_hints.synth",
        );
        let legal = place_legal(&board, &LegalOptions::default()).expect("the ladder places");
        assert_ne!(
            legal.final_canvas,
            (40.0, 60.0),
            "the declared canvas cannot hold the header standing up"
        );
        assert!(
            legal.final_canvas.0 >= 40.0 && legal.final_canvas.1 >= 60.0,
            "growth never shrinks the canvas: {:?}",
            legal.final_canvas
        );
        let warning = legal.relaxation_warning().expect("a warning");
        assert!(warning.contains("grew the board"), "{warning}");
        assert!(warning.contains("J1"), "{warning}");
    }

    #[test]
    fn each_canvas_is_listed_once_and_a_declared_outline_never_reads_as_grown() {
        let options = LegalOptions {
            grow_canvas: false,
            ..LegalOptions::default()
        };
        let legal = place_legal(&board(), &options).expect("the hint levels place");
        assert_eq!(legal.final_canvas, (40.0, 70.0));
        let mut seen = legal.canvases.clone();
        seen.dedup();
        assert_eq!(seen, legal.canvases, "a canvas is listed once per size");
        if let Some(warning) = legal.relaxation_warning() {
            assert!(!warning.contains("grew the board"), "{warning}");
        }
    }

    #[test]
    fn growing_past_the_declared_outline_warns_even_with_no_hint_dropped() {
        let board = crate::tests::board_from_source(
            r#"board "grown" {
                layers 2
                outline 40mm x 40mm
                component J1: connector "header_1x20"
                component R1: resistor "r_generic_0603"
            }"#,
            "grown.synth",
        );
        let legal = place_legal(&board, &LegalOptions::default()).expect("the ladder places");
        assert!(legal.dropped.is_empty());
        assert_ne!(legal.final_canvas, (40.0, 40.0));
        let warning = legal.relaxation_warning().expect("growth is a warning");
        assert!(warning.contains("grew the board"), "{warning}");
    }

    #[test]
    fn a_board_that_fits_no_canvas_fails_with_the_part_and_the_needed_size() {
        let board = crate::tests::board_from_source(
            r#"board "impossible" {
                layers 2
                outline 20mm x 20mm
                component J1: connector "header_1x20"
            }"#,
            "impossible.synth",
        );
        let failure = place_legal(&board, &LegalOptions::default()).expect_err("no canvas fits");
        assert_eq!(failure.refdes, "J1");
        assert!(!failure.budget_exhausted, "the ladder ran to the ceiling");
        assert!(!failure.canvases.is_empty());
        assert!(failure.hint.is_none(), "the source gives J1 no hint");
        assert!(
            failure.needed.0 > 0.0 && failure.needed.1 > 0.0,
            "{failure:?}"
        );
    }

    #[test]
    fn a_declared_outline_is_never_grown_without_grow_canvas() {
        let board = board();
        assert_eq!(
            fixed_canvases(&board, &(60.0, 90.0), false),
            vec![(40.0, 70.0)],
            "the estimate is a grown canvas and must not be tried"
        );
        let grown = fixed_canvases(&board, &(60.0, 90.0), true);
        assert_eq!(grown[0], (40.0, 70.0));
        assert!(grown.contains(&(60.0, 90.0)), "{grown:?}");
    }

    #[test]
    fn a_declared_outline_smaller_than_a_part_ends_in_a_clean_failure() {
        let board = crate::tests::board_from_source(
            &TIGHT.replace("outline 40mm x 70mm", "outline 10mm x 10mm"),
            "tiny.synth",
        );
        for grow_canvas in [false, true] {
            let options = LegalOptions {
                grow_canvas,
                ..LegalOptions::default()
            };
            // Must return, not panic in the solver's clamp.
            let failure = place_legal(&board, &options).expect_err("no canvas holds the parts");
            assert!(!failure.refdes.is_empty(), "{failure:?}");
        }
    }

    #[test]
    fn a_hard_hint_that_blocks_the_packing_is_relaxed_with_a_warning() {
        let board = board();
        let single = crate::place(&board);
        assert!(
            single.is_err(),
            "the fixture must fail as written, or it does not test the ladder"
        );
        println!("as written: {:?}", single.unwrap_err());

        let options = LegalOptions {
            grow_canvas: false,
            ..LegalOptions::default()
        };
        let legal = place_legal(&board, &options).expect("the ladder places");
        assert_eq!(
            legal.level,
            HintLevel::DroppedAll,
            "a connector edge stays hard until it is the only blocker"
        );
        assert_eq!(legal.final_canvas, (40.0, 70.0));
        assert_eq!(legal.placement.components.len(), board.components.len());
        assert_eq!(legal.dropped.len(), 1, "{:?}", legal.dropped);
        assert_eq!(legal.dropped[0].refdes, "J1");
        let warning = legal.relaxation_warning().expect("a warning");
        assert!(warning.contains("J1"), "{warning}");
        assert!(warning.contains("edge: top"), "{warning}");
        assert!(warning.contains("dropped"), "{warning}");
    }
}
