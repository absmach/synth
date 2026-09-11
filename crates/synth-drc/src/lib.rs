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

mod geometry;
mod profile;
mod rules;

use serde::{Deserialize, Serialize};
use synth_geometry::Point;
use synth_ir::{Board, NetId};
use synth_place::Placement;
use synth_route::Routing;

pub use profile::{ManufacturerProfile, ProfileError};
pub use rules::run_kicad_cli_drc;

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
}

impl DrcReport {
    /// True when no violations were found.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
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
    DrcReport {
        violations,
        profile_name: profile.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_ir::Board;

    fn load_board(path: &str) -> Board {
        let source = std::fs::read_to_string(path).expect("read fixture");
        let file = path.to_string();
        let parse = synth_parser::parse(&source, file.clone());
        let ast = parse.ast.as_ref().expect("parse");
        let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
        let registry = synth_registry::load_dir(&registry_dir).expect("registry");
        let loader = synth_ir::FsImportLoader {
            root: std::path::PathBuf::from("../.."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        let lowered = synth_ir::lower(&resolved.program, &registry, &file);
        lowered.board.expect("board")
    }

    #[test]
    fn sensor_logger_drc_runs_and_produces_report() {
        // Slice 1A contract: the engine runs end-to-end and
        // produces a structured report against a real
        // (Board, Placement, Routing) triple.
        //
        // This used to rely on an incidental, router-quality-
        // dependent clearance gap (adjacent traces forced to 0 mm
        // separation on a tight grid) to prove the DRC engine
        // isn't a no-op. That's fragile: a router improvement
        // (e.g. wider pin-escape carving letting A* pick
        // better-spaced paths) can legitimately make a design
        // fully clean, which isn't a DRC-engine bug. So the
        // violation here is now injected deterministically —
        // same technique as `geometry::flags_trace_running_
        // through_a_component_courtyard` — independent of
        // whatever quality the router happens to produce.
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let mut routing = synth_route::route(&board, &placement);

        let comp = placement
            .components
            .first()
            .expect("at least one component");
        routing.segments.push(synth_route::Segment {
            net: NetId(999_999),
            layer: synth_geometry::Layer::Top,
            start: comp.center,
            end: synth_geometry::Point::new(
                comp.center.x_nm + synth_geometry::mm_to_nm(0.01),
                comp.center.y_nm,
            ),
            width_nm: synth_geometry::mm_to_nm(0.15),
        });

        let profile = ManufacturerProfile::jlc_standard();
        let report = check(&board, &placement, &routing, &profile);
        assert_eq!(report.profile_name, "jlc-standard");
        println!("UNROUTED NETS (count {}):", routing.unrouted_nets.len());
        for u in &routing.unrouted_nets {
            println!(
                "  Net {:?} ({}) from {:?} to {:?}",
                u.net, u.net_name, u.source_pad_nm, u.target_pad_nm
            );
        }
        println!("VIOLATIONS (count {}):", report.violations.len());
        for v in &report.violations {
            println!(
                "  {} {} at {:?} components: {:?}",
                v.code, v.message, v.pos_mm, v.components
            );
        }
        // The engine MUST surface the violation injected above;
        // that's the whole point of the independent check. A
        // clean report here would indicate the engine isn't
        // actually doing anything.
        assert!(
            !report.violations.is_empty(),
            "DRC engine should detect a trace routed through a component body"
        );
        for v in &report.violations {
            assert!(
                v.code.starts_with("E-SYNTH-DRC-"),
                "violation code {} must use the canonical prefix",
                v.code
            );
        }
    }

    #[test]
    fn report_is_deterministic() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = synth_route::route(&board, &placement);
        let profile = ManufacturerProfile::jlc_standard();
        let a = check(&board, &placement, &routing, &profile);
        let b = check(&board, &placement, &routing, &profile);
        assert_eq!(a, b);
    }

    #[test]
    fn iot_sensor_board_drc() {
        let board = load_board("../../fixtures/designs/iot_sensor_board.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = synth_route::route(&board, &placement);
        let profile = ManufacturerProfile::jlc_standard();
        let report = check(&board, &placement, &routing, &profile);
        for v in &report.violations {
            eprintln!("IOT DRC VIOLATION: {} {}", v.code, v.message);
        }
        assert!(
            report.violations.is_empty(),
            "iot_sensor_board must pass Synth DRC with 0 violations, found: {:?}",
            report.violations
        );
    }

    #[test]
    fn env_logger_drc() {
        let board = load_board("../../examples/env_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = synth_route::route(&board, &placement);
        let profile = ManufacturerProfile::jlc_standard();
        let report = check(&board, &placement, &routing, &profile);
        for v in &report.violations {
            eprintln!("ENV DRC VIOLATION: {} {}", v.code, v.message);
        }
        assert!(
            report.violations.is_empty(),
            "env_logger must pass Synth DRC with 0 violations, found: {:?}",
            report.violations
        );
    }
}
