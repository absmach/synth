// SPDX-License-Identifier: Apache-2.0

//! Placement geometry, checked before a board is exported.
//!
//! `synth validate` used to judge the source and the netlist only: a design
//! whose parts do not pack could pass with zero blocking diagnostics and then
//! fail an hour later in `synth place`. This runs the real placer through the
//! bounded ladder in [`synth_place::place_legal`] and reports what it found:
//!
//! - the board that was placed, and the size it took;
//! - the hints the placer had to relax and every canvas it tried;
//! - a placed board that needs visual revision;
//! - the placement sidecar's own result, beside a from-scratch one, so a
//!   stale or poor floorplan is visible up front.
//!
//! Severity is the contract: an error only when the ladder exhausted every
//! canvas and hint level inside the budget, so "no legal placement" is proven.
//! A budget cut-off is a warning, because the absence of a placement was not
//! established.

use std::path::Path;
use std::time::Duration;

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_ir::Board;
use synth_place::{
    LegalFailure, LegalOptions, LegalPlacement, Placement, judge, place_legal, place_with_sidecar,
};

/// The success/report code: info when the board places as written, warning
/// when the placement needed a relaxed hint, a larger canvas, a visual
/// revision, or a sidecar.
pub const PREFLIGHT_CODE: &str = "W-SYNTH-PLACE-004";

/// The failure code, reused from the placer: the preflight reports the same
/// "no legal position" verdict, with the canvas and hint evidence attached.
pub const NO_PLACEMENT_CODE: &str = "E-SYNTH-PLACE-002";

/// How much a preflight may spend, and whether to compare a sidecar against a
/// from-scratch placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreflightOptions {
    pub budget: Duration,
    pub max_attempts: usize,
    /// When a placement sidecar exists, also prove what a from-scratch
    /// placement would do. Off makes the preflight a sidecar check only.
    pub compare_sidecar: bool,
}

impl Default for PreflightOptions {
    fn default() -> Self {
        Self {
            budget: synth_place::legal::DEFAULT_LEGAL_BUDGET,
            max_attempts: synth_place::legal::DEFAULT_LEGAL_ATTEMPTS,
            compare_sidecar: true,
        }
    }
}

/// The default `synth validate` note for a board with hard placement hints.
///
/// It does not run the placer: it says the opt-in preflight exists, because a
/// hard hint is exactly where a board can be unplaceable while the cheap
/// checks are clean.
#[must_use]
pub fn preflight_hint(board: &Board, file: &str) -> Option<Diagnostic> {
    let has_hard = board.components.iter().any(|component| {
        component
            .placement_hint
            .as_ref()
            .is_some_and(|hint| hint.priority == synth_ir::PlacementPriority::Hard)
    });
    if !has_hard {
        return None;
    }
    Some(
        DiagnosticBuilder::new(
            PREFLIGHT_CODE,
            Severity::Info,
            "placement preflight not run",
        )
        .location(Location::from_span(file.to_string(), board.source_span))
        .message(
            "this board has hard placement hints; run `synth validate --placement-preflight` to test that it places",
        )
        .explanation_url(format!("synth.docs/diagnostics/{PREFLIGHT_CODE}"))
        .build(),
    )
}

/// Run the placement preflight and turn its result into diagnostics.
///
/// `design` is the source path, used to resolve the placement sidecar.
#[must_use]
pub fn placement_preflight(
    board: &Board,
    file: &str,
    design: &Path,
    options: &PreflightOptions,
) -> Vec<Diagnostic> {
    let sidecar = synth_place_sidecar(design);
    if let Some(path) = sidecar {
        return sidecar_then_scratch(board, file, &path, options);
    }
    scratch_only(board, file, options)
}

/// The placement sidecar for a design, if it has one.
fn synth_place_sidecar(design: &Path) -> Option<std::path::PathBuf> {
    synth_layout::placement_sidecar_path(design)
}

/// The preflight for a design that has no file on disk (MCP `source` without
/// `file_path`): no sidecar lookup, so a stray `<name>.placement.layout.toml`
/// in the server's working directory is never picked up.
pub fn placement_preflight_unsaved(
    board: &Board,
    file: &str,
    options: &PreflightOptions,
) -> Vec<Diagnostic> {
    scratch_only(board, file, options)
}

fn scratch_only(board: &Board, file: &str, options: &PreflightOptions) -> Vec<Diagnostic> {
    let legal_options = LegalOptions {
        budget: Some(options.budget),
        max_attempts: options.max_attempts,
        // A declared outline is authoritative (ADR 0001): `synth place` and
        // the export never grow it, so the preflight must not either.
        grow_canvas: board.outline.is_none(),
    };
    match place_legal(board, &legal_options) {
        Ok(legal) => vec![legal_diagnostic(board, file, &legal, options.budget)],
        Err(failure) => vec![failure_diagnostic(board, file, &failure, options.budget)],
    }
}

fn sidecar_then_scratch(
    board: &Board,
    file: &str,
    sidecar: &Path,
    options: &PreflightOptions,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let pinned_parts =
        synth_layout::sidecar::SidecarLayout::load_from_file(sidecar).map_or(0, |loaded| {
            board
                .components
                .iter()
                .filter(|component| loaded.components.contains_key(&component.refdes))
                .count()
        });
    let missing = board.components.len().saturating_sub(pinned_parts);

    match place_with_sidecar(board, Some(sidecar)) {
        Ok(placement) => {
            let mut builder = report_builder(
                "placement sidecar",
                if missing > 0 || needs_revision(board, &placement) {
                    Severity::Warning
                } else {
                    Severity::Info
                },
                board,
                file,
                &format!(
                    "the sidecar pins {pinned_parts} of {} parts",
                    board.components.len()
                ),
            )
            .message(format!(
                "the placement sidecar gives {};{}",
                metrics(board, &placement),
                if missing > 0 {
                    format!(
                        " {missing} part(s) are missing from the sidecar and were solved from scratch"
                    )
                } else {
                    String::new()
                }
            ));
            if needs_revision(board, &placement) {
                builder = builder.expected(
                    "a sidecar placement that does not require visual revision; run the placement review and apply its moves",
                );
            }
            out.push(builder.build());
        }
        Err(error) => out.push(
            DiagnosticBuilder::new(
                PREFLIGHT_CODE,
                Severity::Warning,
                "the placement sidecar does not place",
            )
            .location(Location::from_span(file.to_string(), board.source_span))
            .message(format!(
                "the placement sidecar {} could not be applied: {error}",
                sidecar.display()
            ))
            .expected(
                "a sidecar the placer can apply, or remove the file for a from-scratch placement",
            )
            .explanation_url(format!("synth.docs/diagnostics/{PREFLIGHT_CODE}"))
            .build(),
        ),
    }

    if options.compare_sidecar {
        let legal_options = LegalOptions {
            budget: Some(options.budget),
            max_attempts: options.max_attempts,
            // A declared outline is authoritative (ADR 0001): `synth place` and
            // the export never grow it, so the preflight must not either.
            grow_canvas: board.outline.is_none(),
        };
        match place_legal(board, &legal_options) {
            Ok(legal) => out.push(legal_diagnostic(board, file, &legal, options.budget)),
            Err(failure) => out.push(failure_diagnostic(board, file, &failure, options.budget)),
        }
    }
    out
}

fn report_builder(
    title: &str,
    severity: Severity,
    board: &Board,
    file: &str,
    found: &str,
) -> DiagnosticBuilder {
    let span = board
        .outline
        .as_ref()
        .map_or(board.source_span, |outline| outline.source_span);
    DiagnosticBuilder::new(PREFLIGHT_CODE, severity, title)
        .location(Location::from_span(file.to_string(), span))
        .found(found.to_string())
        .explanation_url(format!("synth.docs/diagnostics/{PREFLIGHT_CODE}"))
}

fn legal_diagnostic(
    board: &Board,
    file: &str,
    legal: &LegalPlacement,
    budget: Duration,
) -> Diagnostic {
    let relaxed = legal.relaxation_warning();
    // The preflight's warning line is the ladder having to relax something.
    // A placement that is legal but still needs a visual revision is a
    // finding for the review, not a placement failure: it stays info, with
    // the revision named, so a clean design keeps its `ok:` line.
    let severity = if relaxed.is_some() {
        Severity::Warning
    } else {
        Severity::Info
    };
    let title = if relaxed.is_some() {
        "placement needs a relaxed hint or a larger canvas"
    } else {
        "placement preflight"
    };
    let found = format!(
        "{} on {:.1} x {:.1} mm after {} solve(s)",
        metrics(board, &legal.placement),
        legal.final_canvas.0,
        legal.final_canvas.1,
        legal.attempts,
    );
    let mut message = format!(
        "the placer returns a legal placement: {} ({} attempt(s), {}s budget)",
        metrics(board, &legal.placement),
        legal.attempts,
        budget.as_secs(),
    );
    if let Some(warning) = &relaxed {
        let _ = std::fmt::Write::write_fmt(&mut message, format_args!("; {warning}"));
    }
    if legal.canvases.len() > 1 {
        let canvases = legal
            .canvases
            .iter()
            .map(|(w, h)| format!("{w:.1} x {h:.1}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ =
            std::fmt::Write::write_fmt(&mut message, format_args!("; canvases tried: {canvases}"));
    }
    if needs_revision(board, &legal.placement) {
        message.push_str("; the visual review still requires revision");
    }
    let mut builder = report_builder(title, severity, board, file, &found).message(message);
    if relaxed.is_some() || needs_revision(board, &legal.placement) {
        builder = builder
            .expected("a placement that honours the source hints and needs no visual revision");
    }
    builder.build()
}

fn failure_diagnostic(
    board: &Board,
    file: &str,
    failure: &LegalFailure,
    budget: Duration,
) -> Diagnostic {
    if failure.budget_exhausted {
        return unproven_diagnostic(board, file, failure, budget);
    }
    let canvases = failure
        .canvases
        .iter()
        .map(|(w, h)| format!("{w:.1} x {h:.1}"))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = failure.hint.as_ref().map_or_else(
        || "no hint".to_string(),
        |dropped| format!("its `{}`", dropped.hint),
    );
    let span = board
        .components
        .iter()
        .find(|component| component.refdes == failure.refdes)
        .map_or(board.source_span, |component| component.source_span);
    DiagnosticBuilder::new(
        NO_PLACEMENT_CODE,
        Severity::Error,
        format!("No legal placement for {}", failure.refdes),
    )
    .location(Location::from_span(file.to_string(), span))
    .message(format!(
        "the placer found no position for `{}` on any canvas ({}; {} attempt(s)); {hint} \
         blocks it, and the parts need about {:.1} x {:.1} mm",
        failure.refdes,
        if canvases.is_empty() {
            "the declared outline and the estimate".to_string()
        } else {
            canvases
        },
        failure.attempts,
        failure.needed.0,
        failure.needed.1,
    ))
    .expected(format!(
        "a canvas of at least {:.1} x {:.1} mm, or a soft/removed hint on `{}`",
        failure.needed.0, failure.needed.1, failure.refdes,
    ))
    .found(format!(
        "no legal placement on {} canvas(es) in {} attempt(s)",
        failure.canvases.len(),
        failure.attempts,
    ))
    .explanation_url(format!("synth.docs/diagnostics/{NO_PLACEMENT_CODE}"))
    .build()
}

fn unproven_diagnostic(
    board: &Board,
    file: &str,
    failure: &LegalFailure,
    budget: Duration,
) -> Diagnostic {
    report_builder(
        "placement preflight did not finish",
        Severity::Warning,
        board,
        file,
        &format!(
            "stopped after {} attempt(s) on {} canvas(es)",
            failure.attempts,
            failure.canvases.len()
        ),
    )
    .message(format!(
        "the placement preflight reached its {}s budget before `{}` could be placed or \
         ruled out; the board may still place, but this run did not prove it. \
         Re-run with a longer budget or without the budget for a verdict",
        budget.as_secs(),
        failure.refdes,
    ))
    .expected("a placement preflight that finishes inside its budget")
    .build()
}

/// The one-line size/density read shared by every success report.
fn metrics(board: &Board, placement: &Placement) -> String {
    let judgement = judge(board, placement);
    format!(
        "{:.1} x {:.1} mm ({:.0} mm2), dead area {:.1}%, {} gate failure(s)",
        synth_geometry::nm_to_mm(placement.board_outline.width_nm()),
        synth_geometry::nm_to_mm(placement.board_outline.height_nm()),
        synth_geometry::nm_to_mm(placement.board_outline.width_nm())
            * synth_geometry::nm_to_mm(placement.board_outline.height_nm()),
        judgement.dead_area_ratio * 100.0,
        judgement.gate_failures,
    )
}

fn needs_revision(board: &Board, placement: &Placement) -> bool {
    synth_place::describe_placement(board, placement)
        .visual_review
        .requires_revision
}
