// SPDX-License-Identifier: Apache-2.0

//! Deterministic DRC engine per plan §11.
//!
//! Phase 9. Validates a `(Board, Placement, Routing)` triple
//! against a `ManufacturerProfile` and returns a `DrcReport`
//! containing zero or more structured `Violation`s. Same
//! compiler-correctness discipline as earlier phases:
//!
//! - **Deterministic.** Same triple + profile → byte-identical
//!   report. No `HashMap` iteration in the hot path; integer-nm
//!   arithmetic only.
//! - **Independent of the router.** A DRC engine that shares
//!   state with the producer can't catch the producer's bugs.
//!   The DRC code path rebuilds every check from scratch
//!   against the IR shapes — same approach as
//!   `synth_route::drc`, just with manufacturer-profile rules.
//! - **Machine-readable diagnostics.** Each violation carries a
//!   stable `E-SYNTH-DRC-*` code (slice 5 catalogue) and a
//!   polygon location so the browser preview can highlight the
//!   offending geometry exactly.
//!
//! ## Phased rollout
//!
//! - **Slice 1A — foundation (shipped).** Crate skeleton, the
//!   `DrcReport` / `Violation` / `ManufacturerProfile` IR, TOML
//!   profile loader, and three starter rules:
//!   `MinTraceWidth`, `MinCopperClearance`,
//!   `CopperToEdgeClearance`.
//! - **Slice 1B — via-aware rules (this slice).** Drill
//!   diameter (`E-SYNTH-DRC-004`), annular ring
//!   (`E-SYNTH-DRC-005`), drill-to-copper (`E-SYNTH-DRC-006`).
//!   These fire on `Routing::vias` and stay quiet on the
//!   current single-layer router output; they're wired so
//!   slice 2.x of routing can land vias against a profile-aware
//!   DRC engine.
//! - **Slice 1B.1 — copper-to-component clearance (shipped).**
//!   `E-SYNTH-DRC-007`: every segment must clear every
//!   component's courtyard and every foreign pad, checked via
//!   continuous rectangle geometry recomputed from `Board` +
//!   `Placement` — deliberately not sharing `synth_route`'s
//!   grid, so a bug in the grid can't hide from both. Landed
//!   from a prior-art scan of pcbflow/cuflow, which verify
//!   clearance via an independent geometric method (polygon
//!   buffering) rather than trusting the same code path that
//!   produced the routes. See `geometry.rs`.
//! - **Slice 1C.** Solder-mask sliver and silkscreen overlap.
//! - **Slice 1D.** Courtyard overlap (depends on placement
//!   geometry, not routing).
//! - **Slice 2 — manufacturer profile catalog.** Seed TOMLs
//!   for JLC (already shipped in 1A), PCBWay, OSHPark.
//! - **Slice 3 — Gerber / drill / STEP export.** Wraps
//!   `kicad-cli pcb export gerbers` and `kicad-cli pcb export
//!   step` per §11.2 ("we do not write Gerbers by hand for V1").
//! - **Slice 4 — BOM enrichment.** Registry-keyed LCSC numbers,
//!   stock levels (cached), substitution suggestions.
//! - **Slice 5 — `E-SYNTH-DRC-*` diagnostic catalogue** with
//!   polygon location attribution and patch primitives.

#![forbid(unsafe_code)]
#![allow(clippy::cast_possible_wrap, clippy::similar_names)]

mod emc;
mod geometry;
pub mod kicad_cli;
mod profile;
mod rules;
mod signal_integrity;
/// The IPC-2221 width a `current` needs. Re-exported so the exporter can size
/// a high-current net's trace for the router.
pub use signal_integrity::required_width_nm;
pub use signal_integrity::{ground_net_names, is_ground_name};

use serde::{Deserialize, Serialize};
use synth_geometry::Point;
use synth_ir::{Board, NetId};
use synth_pcb::Routing;
use synth_place::Placement;

pub use profile::{ManufacturerProfile, ProfileError};
pub use rules::{replay, run_kicad_cli_drc, DrcCounts, NativeDrcOutcome, DRC_STAGE};
/// Re-exported so a DRC caller can resolve and hand in a board's physical
/// stack without depending on `synth-geometry` directly.
pub use synth_geometry::StackupProfile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedOverride {
    pub refdes: String,
    pub delta_x_mm: f64,
    pub delta_y_mm: f64,
    pub rotation_deg: u32,
}

/// One DRC violation. Carries the stable diagnostic code, a
/// human description, and a witness — either a point pair
/// (e.g., two trace segments too close) or a single coordinate
/// (e.g., trace outside copper-to-edge clearance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    /// Stable `E-SYNTH-DRC-*` code. Slice 5 promotes each to a
    /// full `Diagnostic` payload.
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Geometric witness — a list of points the violation
    /// involves. UI tooling renders these as polygons /
    /// highlights / arrows.
    pub witness: Vec<Point>,
    /// Net(s) involved, if applicable.
    #[serde(default)]
    pub nets: Vec<NetId>,
    /// Component refdes(s) involved, if applicable.
    #[serde(default)]
    pub components: Vec<String>,
    /// Position witness in mm (x, y), if available.
    #[serde(default)]
    pub pos_mm: Option<(f64, f64)>,
    /// Suggested sidecar placement override to resolve this DRC violation.
    #[serde(default)]
    pub suggested_override: Option<SuggestedOverride>,
}

/// Full DRC report. Slice 1A keeps it flat; future slices may
/// add per-layer / per-net summary statistics for the browser
/// preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrcReport {
    pub violations: Vec<Violation>,
    /// Profile this report was produced against. Embedded so
    /// downstream consumers can verify they're comparing
    /// apples to apples.
    pub profile_name: String,
    /// The board's physical stack the report was evaluated against, when the
    /// caller supplied one ([`check_with_stackup`]). `None` from the plain
    /// [`check`], whose output is therefore byte-identical to before stackup
    /// awareness existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stackup: Option<StackupProfile>,
}

impl Violation {
    /// True when this violation blocks a release.
    ///
    /// A [`Violation`] carries no severity field; the project's stable
    /// convention is that an `E-` code is an error and a `W-` code is a
    /// warning, so severity is read from the code. Advisory rules (edge
    /// coupling, say) emit `W-SYNTH-DRC-*` and never gate.
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        self.code.starts_with("E-")
    }
}

impl DrcReport {
    /// True when no *blocking* violations were found.
    ///
    /// Warning-severity violations (`W-SYNTH-DRC-*`) are advisory and do not
    /// make a report unclean; callers that want the full list read
    /// [`DrcReport::violations`].
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.violations.iter().all(|v| !v.is_blocking())
    }

    /// Count of blocking violations.
    #[must_use]
    pub fn blocking_count(&self) -> usize {
        self.violations.iter().filter(|v| v.is_blocking()).count()
    }
}

/// Run every V1 DRC rule against the supplied artifacts.
///
/// Slice 1A wired three trace / clearance / edge rules; slice
/// 1B adds the via-aware trio. Subsequent slices wire the
/// remaining §11.3 set (mask sliver, silk overlap, courtyard).
#[must_use]
pub fn check(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    profile: &ManufacturerProfile,
) -> DrcReport {
    evaluate(board, placement, routing, profile, None)
}

/// [`check`], additionally recording the board's resolved physical stackup on
/// the report.
///
/// The stackup does not change which violations fire yet — the impedance,
/// crosstalk and ampacity rules that consume it land separately — but carrying
/// it here is what lets those rules resolve a trace against the real
/// dielectric geometry instead of a fixed default. Use
/// [`StackupProfile::for_board`] to obtain the argument, so a board that
/// declares no stackup still gets a standard profile rather than none.
#[must_use]
pub fn check_with_stackup(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: &StackupProfile,
) -> DrcReport {
    evaluate(board, placement, routing, profile, Some(stackup.clone()))
}

/// The routed signal/power-integrity rules: the ones `kicad-cli` DRC cannot
/// see. Impedance, crosstalk, differential skew, ampacity, return path, IR drop
/// and net length all read the copper the router produced.
///
/// Geometric clearance (min width, copper clearance, shorts, copper-to-edge)
/// is deliberately **not** here: the route validation runs `kicad-cli pcb drc`
/// on the same board, so repeating it would double-gate the same finding. This
/// is the set to run on a routed board whose clearance kicad-cli has already
/// judged.
#[must_use]
pub fn check_signal_integrity(
    board: &Board,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: Option<&StackupProfile>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    if let Some(stackup) = stackup {
        violations.extend(signal_integrity::check_controlled_impedance(
            board, routing, profile, stackup,
        ));
        violations.extend(signal_integrity::check_edge_coupling(
            routing, profile, stackup,
        ));
        violations.extend(signal_integrity::check_return_path_vias(
            board, routing, stackup,
        ));
        violations.extend(signal_integrity::check_ir_drop(
            board, routing, profile, stackup,
        ));
    }
    violations.extend(signal_integrity::check_diff_pair_skew(board, routing));
    violations.extend(signal_integrity::check_ampacity(
        board, routing, profile, stackup,
    ));
    violations.extend(signal_integrity::check_net_length(board, routing));
    violations.extend(signal_integrity::check_group_skew(board, routing));
    violations.extend(signal_integrity::check_layer_pins(board, routing));
    violations.extend(signal_integrity::check_via_current(board, routing));
    violations
}

/// A blocking finding that the routed board could not be read, so the
/// signal/power-integrity rules never ran.
///
/// A delivered board that is missing, unreadable or malformed must not look
/// clean. A gate that swallows the read error cannot tell "checked, nothing
/// found" from "never checked" — the exact failure the routed SI/PI pass
/// exists to prevent. Callers that read the routed copper back from disk emit
/// this in place of the (empty) rule output, so it flows through the same
/// blocking path as a real violation and downgrades the release manifest.
///
/// Code: `E-SYNTH-DRC-022`.
#[must_use]
pub fn unreadable_routed_board(path: &std::path::Path, error: &str) -> Violation {
    Violation {
        code: "E-SYNTH-DRC-022".to_string(),
        message: format!(
            "the routed board `{}` could not be read ({error}); signal/power integrity \
             was not checked",
            path.display()
        ),
        witness: Vec::new(),
        nets: Vec::new(),
        components: Vec::new(),
        pos_mm: None,
        suggested_override: None,
    }
}

fn evaluate(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: Option<StackupProfile>,
) -> DrcReport {
    let mut violations = Vec::new();
    violations.extend(rules::check_min_trace_width(routing, profile));
    violations.extend(rules::check_min_copper_clearance(routing, profile));
    violations.extend(rules::check_copper_to_edge(routing, placement, profile));
    violations.extend(rules::check_min_drill_diameter(routing, profile));
    violations.extend(rules::check_min_annular_ring(routing, profile));
    violations.extend(rules::check_min_drill_to_copper(routing, profile));
    violations.extend(rules::check_courtyard_overlap(board, placement));
    violations.extend(rules::check_connector_orientation(board, placement));
    violations.extend(rules::check_silkscreen_overlap(board, placement));
    violations.extend(rules::check_soldermask_sliver(routing, profile));
    violations.extend(geometry::check_copper_to_component_clearance(
        board, placement, routing, profile,
    ));
    violations.extend(geometry::check_shorts(board, placement, routing));
    violations.extend(emc::check_emc_placement(board, placement));
    // Routed signal integrity (E-SYNTH-DRC-012 / 014). The impedance rule
    // needs a stackup and is skipped without one, preserving the documented
    // "no stackup, impedance not verified" contract; the skew rule is
    // geometry-only.
    if let Some(stackup) = stackup.as_ref() {
        violations.extend(signal_integrity::check_controlled_impedance(
            board, routing, profile, stackup,
        ));
        violations.extend(signal_integrity::check_edge_coupling(
            routing, profile, stackup,
        ));
        violations.extend(signal_integrity::check_return_path_vias(
            board, routing, stackup,
        ));
        violations.extend(signal_integrity::check_ir_drop(
            board, routing, profile, stackup,
        ));
    }
    violations.extend(signal_integrity::check_diff_pair_skew(board, routing));
    violations.extend(signal_integrity::check_net_length(board, routing));
    violations.extend(signal_integrity::check_group_skew(board, routing));
    violations.extend(signal_integrity::check_layer_pins(board, routing));
    violations.extend(signal_integrity::check_via_current(board, routing));
    violations.extend(signal_integrity::check_ampacity(
        board,
        routing,
        profile,
        stackup.as_ref(),
    ));
    DrcReport {
        violations,
        profile_name: profile.name.clone(),
        stackup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_ir::Board;

    fn board_from_source(source: &str) -> Board {
        let file = "ground.synth".to_string();
        let parse = synth_parser::parse(source, file.clone());
        let ast = parse.ast.as_ref().expect("parse");
        let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
        let registry = synth_registry::load_dir(&registry_dir).expect("registry");
        let loader = synth_ir::FsImportLoader {
            root: std::path::PathBuf::from("../.."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        synth_ir::lower(&resolved.program, &registry, &file)
            .board
            .expect("board")
    }

    /// A ground that its name does not give away, which is what a repair pass
    /// that sees only the board file must still recognise.
    #[test]
    fn a_net_on_ground_pins_is_ground_whatever_it_is_called() {
        let board = board_from_source(
            r#"board "returns" { layers 2
                component U1: mcu "rp2350"
                component C1: capacitor "c_generic_0603" value "100nF"
                net "POWER_RETURN" { U1.gnd C1.p2 }
                net "SIGNAL" { U1.gp0 C1.p1 }
            }"#,
        );
        let ground = ground_net_names(&board);
        assert!(ground.iter().any(|n| n == "POWER_RETURN"), "{ground:?}");
        assert!(!ground.iter().any(|n| n == "SIGNAL"), "{ground:?}");
    }

