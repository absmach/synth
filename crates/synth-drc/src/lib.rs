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
pub use signal_integrity::{ground_net_names, is_ground_name, return_path_critical_net_names};

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

    /// A layer change only costs a fast edge a loop, so the nets a missing return
    /// via must hold a board back for are the ones that carry one: a clock, a
    /// differential pin, a switching node. A plain GPIO line is not one.
    #[test]
    fn only_fast_nets_are_critical_for_a_return_via() {
        let board = board_from_source(
            r#"board "returns" { layers 2
                component U1: mcu "rp2350"
                component X1: crystal "osc_smd_25mhz"
                component L1: inductor "l_generic_1210" value "10uH"
                component C1: capacitor "c_generic_0603" value "100nF"
                component R1: resistor "r_generic_0603" value "10k"
                net "CLK_OUT" { X1.out R1.p1 }
                net "SW_NODE" { L1.p1 C1.p1 }
                net "SLOW_GPIO" { U1.gp0 R1.p2 }
            }"#,
        );
        let critical = return_path_critical_net_names(&board);
        assert!(critical.iter().any(|n| n == "CLK_OUT"), "{critical:?}");
        assert!(critical.iter().any(|n| n == "SW_NODE"), "{critical:?}");
        assert!(!critical.iter().any(|n| n == "SLOW_GPIO"), "{critical:?}");
    }

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
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
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
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = synth_route::route(&board, &placement);
        let profile = ManufacturerProfile::jlc_standard();
        let a = check(&board, &placement, &routing, &profile);
        let b = check(&board, &placement, &routing, &profile);
        assert_eq!(a, b);
    }

    #[test]
    fn declared_stackup_resolves_and_a_declared_profile_preserves_violations() {
        let board = load_board("../../fixtures/designs/stackup_4layer.synth");
        let profile = StackupProfile::declared(&board).expect("declared stackup");
        assert_eq!(profile.copper_count(), 4);
        assert_eq!(profile.source, synth_geometry::StackupSource::Declared);

        let placement = synth_place::place(&board).expect("place");
        let routing = synth_route::route(&board, &placement);
        let fab = ManufacturerProfile::jlc_standard();
        let plain = check(&board, &placement, &routing, &fab);
        let with_stackup = check_with_stackup(&board, &placement, &routing, &fab, &profile);
        // Phase 1 threads the stackup through without changing which rules
        // fire, so the two reports can only differ in the recorded stackup.
        assert_eq!(plain.violations, with_stackup.violations);
        assert!(plain.stackup.is_none());
        assert_eq!(with_stackup.stackup.as_ref(), Some(&profile));
    }

    #[test]
    fn a_board_without_a_stackup_substitutes_a_standard_profile() {
        let board = load_board("../../examples/env_logger/env_logger.synth");
        assert!(
            StackupProfile::declared(&board).is_none(),
            "env_logger must not declare a stackup for this test to be meaningful"
        );
        let profile = StackupProfile::for_board(&board);
        assert_eq!(profile.source, synth_geometry::StackupSource::Default);
        assert_eq!(profile.copper_count(), board.layers as usize);
    }

    fn four_layer_stackup() -> synth_ir::Stackup {
        use synth_ir::{DielectricConstant, Length, StackupLayer};
        let span = synth_diagnostics::Span::new(0, 0);
        let copper = |t: f64| StackupLayer::Copper {
            thickness: Length::from_mm(t),
            source_span: span,
        };
        let dielectric = |t: f64, er: f64| StackupLayer::Insulator {
            thickness: Length::from_mm(t),
            er: DielectricConstant((er * 1_000_000.0) as i64),
            material: Some("FR4".to_string()),
            source_span: span,
        };
        synth_ir::Stackup {
            layers: vec![
                copper(0.035),
                dielectric(0.2104, 4.4),
                copper(0.0152),
                dielectric(1.065, 4.6),
                copper(0.0152),
                dielectric(0.2104, 4.4),
                copper(0.035),
            ],
            source_span: span,
        }
    }

    fn board_with_controlled_pair() -> Board {
        let mut board =
            load_board("../../examples/placement_and_diff_pair/placement_and_diff_pair.synth");
        board.stackup = Some(four_layer_stackup());
        board
    }

    fn straight(net: NetId, width_nm: i64, y_nm: i64, len_nm: i64) -> synth_pcb::Segment {
        synth_pcb::Segment {
            net,
            layer: synth_geometry::Layer::Top,
            start: synth_geometry::Point::new(0, y_nm),
            end: synth_geometry::Point::new(len_nm, y_nm),
            width_nm,
        }
    }

    #[test]
    fn a_routed_width_matching_the_target_passes_and_a_wrong_one_blocks() {
        let board = board_with_controlled_pair();
        let stackup = StackupProfile::for_board(&board);
        let (positive, negative, _, _) = board.impedance_legs().next().expect("a controlled pair");

        // The width the stackup demands for 90 ohm on the outer layer, with
        // the manufacturer-minimum gap the rule falls back to.
        let pair = stackup
            .coupled_for_z0(
                synth_geometry::Layer::Top,
                90.0,
                synth_geometry::mm_to_nm(0.127),
                synth_geometry::mm_to_nm(0.127),
            )
            .expect("reachable target");
        let good_width = synth_geometry::mm_to_nm(pair.width_mm);

        let routed = |width_nm: i64| Routing {
            segments: vec![
                straight(positive.id, width_nm, 0, 4_000_000),
                straight(positive.id, width_nm, 400_000, 4_000_000),
                straight(negative.id, width_nm, 200_000, 4_000_000),
                straight(negative.id, width_nm, 600_000, 4_000_000),
            ],
            ..Routing::default()
        };
        let profile = ManufacturerProfile::jlc_standard();

        let clean = super::signal_integrity::check_controlled_impedance(
            &board,
            &routed(good_width),
            &profile,
            &stackup,
        );
        assert!(clean.is_empty(), "on-target width must pass: {clean:?}");

        let wrong = super::signal_integrity::check_controlled_impedance(
            &board,
            &routed(good_width / 3),
            &profile,
            &stackup,
        );
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        assert_eq!(wrong[0].code, "E-SYNTH-DRC-012");
    }

    #[test]
    fn impedance_is_not_checked_without_a_stackup() {
        let mut board = board_with_controlled_pair();
        board.stackup = None;
        let (positive, negative, _, _) = board.impedance_legs().next().expect("a controlled pair");
        let routing = Routing {
            segments: vec![
                straight(positive.id, 50_000, 0, 4_000_000),
                straight(negative.id, 50_000, 200_000, 4_000_000),
            ],
            ..Routing::default()
        };
        // `check` is the documented "no stackup, not verified" path: it never
        // runs the impedance rule, so a board that declares no stackup cannot
        // be blocked by it.
        let report = check(
            &board,
            &synth_place::place(&board).expect("place"),
            &routing,
            &ManufacturerProfile::jlc_standard(),
        );
        assert!(report.stackup.is_none());
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.code == "E-SYNTH-DRC-012"),
            "impedance must not be checked without a stackup"
        );
    }

    #[test]
    fn a_pair_over_its_declared_skew_blocks() {
        let mut board = board_with_controlled_pair();
        board.diff_pairs[0].max_skew = Some(synth_ir::Length::from_mm(0.15));
        let (positive, negative, _, _) = board.impedance_legs().next().expect("a controlled pair");

        let routed = |positive_len: i64, negative_len: i64| Routing {
            segments: vec![
                straight(positive.id, 127_000, 0, positive_len),
                straight(negative.id, 127_000, 200_000, negative_len),
            ],
            ..Routing::default()
        };

        assert!(
            super::signal_integrity::check_diff_pair_skew(&board, &routed(5_000_000, 5_100_000))
                .is_empty(),
            "0.1 mm skew is within the 0.15 mm budget"
        );
        let over =
            super::signal_integrity::check_diff_pair_skew(&board, &routed(5_000_000, 5_400_000));
        assert_eq!(over.len(), 1, "{over:?}");
        assert_eq!(over[0].code, "E-SYNTH-DRC-014");
    }

    #[test]
    fn an_undersized_trace_on_a_declared_current_blocks() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.nets[0].netclass = Some("PWR".to_string());
        board.netclasses.push(synth_ir::NetClass {
            name: "PWR".to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: None,
            current: Some(synth_ir::Current(1_000_000)),
            via_diameter: None,
            via_drill: None,
            plane: false,
            max_length: None,
            max_skew: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        });
        let net = board.nets[0].id;
        let routed = |width_nm: i64| Routing {
            segments: vec![straight(net, width_nm, 0, 5_000_000)],
            ..Routing::default()
        };

        let profile = ManufacturerProfile::jlc_standard();
        let thin = super::signal_integrity::check_ampacity(
            &board,
            &routed(synth_geometry::mm_to_nm(0.2)),
            &profile,
            None,
        );
        assert_eq!(thin.len(), 1, "{thin:?}");
        assert_eq!(thin[0].code, "E-SYNTH-DRC-015");
        assert!(
            super::signal_integrity::check_ampacity(
                &board,
                &routed(synth_geometry::mm_to_nm(1.0)),
                &profile,
                None
            )
            .is_empty(),
            "1 mm carries 1 A comfortably"
        );
    }

    #[test]
    fn a_single_ended_netclass_impedance_is_checked_against_the_route() {
        let mut board = board_with_controlled_pair();
        board.nets[0].netclass = Some("RF".to_string());
        board.netclasses.push(synth_ir::NetClass {
            name: "RF".to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: Some(synth_ir::Impedance(50_000)),
            current: None,
            via_diameter: None,
            via_drill: None,
            plane: false,
            max_length: None,
            max_skew: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        });
        let stackup = StackupProfile::for_board(&board);
        let net = board.nets[0].id;
        let good_width = stackup
            .width_for_z0(
                synth_geometry::Layer::Top,
                50.0,
                synth_geometry::mm_to_nm(0.127),
            )
            .expect("reachable")
            .0;
        let routed = |width_nm: i64| Routing {
            segments: vec![straight(net, width_nm, 0, 5_000_000)],
            ..Routing::default()
        };
        let profile = ManufacturerProfile::jlc_standard();

        assert!(
            super::signal_integrity::check_controlled_impedance(
                &board,
                &routed(good_width),
                &profile,
                &stackup
            )
            .is_empty(),
            "a routed width matching the class's 50 ohm target must pass"
        );
        let off = super::signal_integrity::check_controlled_impedance(
            &board,
            &routed(synth_geometry::mm_to_nm(0.05)),
            &profile,
            &stackup,
        );
        assert_eq!(off.len(), 1, "{off:?}");
        assert_eq!(off[0].code, "E-SYNTH-DRC-012");
    }

    #[test]
    fn a_long_close_parallel_run_warns_but_does_not_block() {
        let stackup = StackupProfile::default_for_layers(4);
        let near = Routing {
            segments: vec![
                straight(NetId(1), 50_000, 0, 5_000_000),
                straight(NetId(2), 50_000, 200_000, 5_000_000),
            ],
            ..Routing::default()
        };
        let warnings = super::signal_integrity::check_edge_coupling(
            &near,
            &ManufacturerProfile::jlc_standard(),
            &stackup,
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0].code, "W-SYNTH-DRC-013");
        assert!(!warnings[0].is_blocking());

        // A warning-only report is clean for the release gate.
        let report = DrcReport {
            violations: warnings,
            profile_name: "test".to_string(),
            stackup: None,
        };
        assert!(report.is_clean());
        assert_eq!(report.blocking_count(), 0);

        // Far enough apart to be outside a dielectric height: no warning.
        let far = Routing {
            segments: vec![
                straight(NetId(1), 50_000, 0, 5_000_000),
                straight(NetId(2), 50_000, 1_000_000, 5_000_000),
            ],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_edge_coupling(
            &far,
            &ManufacturerProfile::jlc_standard(),
            &stackup
        )
        .is_empty());
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
    fn outline_check_uses_the_rotated_footprint_extents() {
        let board = load_board("../../fixtures/designs/iot_sensor_board.synth");
        let usb = board
            .components
            .iter()
            .find(|component| component.refdes == "J1")
            .expect("USB-C component");
        let placed = |rotation| synth_place::Placement {
            board_outline: synth_geometry::Rect::new(
                synth_geometry::Point::new(0, 0),
                synth_geometry::Point::new(20_000_000, 20_000_000),
            ),
            components: vec![synth_place::ComponentPlacement {
                id: usb.id,
                center: synth_geometry::Point::new(4_000_000, 10_000_000),
                rotation,
                layer: synth_geometry::Layer::Top,
            }],
        };
        assert_eq!(
            rules::check_silkscreen_overlap(&board, &placed(synth_geometry::Rotation::Zero)).len(),
            1
        );
        assert!(
            rules::check_silkscreen_overlap(&board, &placed(synth_geometry::Rotation::Ninety))
                .is_empty()
        );
    }

    #[test]
    fn env_logger_drc() {
        let board = load_board("../../examples/env_logger/env_logger.synth");
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

    #[test]
    fn a_signal_via_needs_a_nearby_ground_return_via() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.stackup = Some(four_layer_stackup());
        let stackup = StackupProfile::for_board(&board);
        let ground = board
            .nets
            .iter()
            .find(|net| super::signal_integrity::net_name_or_pin_is_ground(&board, net))
            .expect("env_logger must have a ground net")
            .id;
        let signal = board
            .nets
            .iter()
            .find(|net| !super::signal_integrity::net_name_or_pin_is_ground(&board, net))
            .expect("env_logger must have a signal net")
            .id;

        let via = |net: NetId, x: i64, y: i64| synth_pcb::Via {
            net,
            at: synth_geometry::Point::new(x, y),
            drill_nm: 300_000,
            pad_diameter_nm: 600_000,
        };

        // A signal via with no ground via anywhere near it.
        let lonely = Routing {
            vias: vec![via(signal, 0, 0)],
            ..Routing::default()
        };
        let flagged = super::signal_integrity::check_return_path_vias(&board, &lonely, &stackup);
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].code, "W-SYNTH-DRC-016");
        assert!(!flagged[0].is_blocking());

        // A ground return via within the stackup's thickest dielectric.
        let returned = Routing {
            vias: vec![via(signal, 0, 0), via(ground, 500_000, 0)],
            ..Routing::default()
        };
        assert!(
            super::signal_integrity::check_return_path_vias(&board, &returned, &stackup).is_empty()
        );
    }

    #[test]
    fn a_rail_dropping_more_than_the_budget_is_warned() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.stackup = Some(four_layer_stackup());
        board.nets[0].netclass = Some("PWR".to_string());
        board.netclasses.push(synth_ir::NetClass {
            name: "PWR".to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: None,
            current: Some(synth_ir::Current(1_000_000)),
            plane: false,
            via_diameter: None,
            via_drill: None,
            max_length: None,
            max_skew: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        });
        let stackup = StackupProfile::for_board(&board);
        let net = board.nets[0].id;

        // 100 mm of 0.2 mm track carrying 1 A drops a few hundred mV.
        let long = Routing {
            segments: vec![straight(net, 200_000, 0, 100_000_000)],
            ..Routing::default()
        };
        let flagged = super::signal_integrity::check_ir_drop(
            &board,
            &long,
            &ManufacturerProfile::jlc_standard(),
            &stackup,
        );
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].code, "W-SYNTH-DRC-017");
        assert!(!flagged[0].is_blocking());

        // 5 mm of 1 mm track drops a few mV.
        let short = Routing {
            segments: vec![straight(net, 1_000_000, 0, 5_000_000)],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_ir_drop(
            &board,
            &short,
            &ManufacturerProfile::jlc_standard(),
            &stackup
        )
        .is_empty());

        // A 45-degree diagonal on the same copper: 12 mm by 12 mm is 16.97 mm
        // of track (about 41 mV, within budget). Manhattan distance — abs(dx) +
        // abs(dy) = 24 mm, about 58 mV — would falsely warn.
        let diagonal = Routing {
            segments: vec![synth_pcb::Segment {
                net,
                layer: synth_geometry::Layer::Top,
                start: synth_geometry::Point::new(0, 0),
                end: synth_geometry::Point::new(12_000_000, 12_000_000),
                width_nm: 200_000,
            }],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_ir_drop(
            &board,
            &diagonal,
            &ManufacturerProfile::jlc_standard(),
            &stackup
        )
        .is_empty());
    }

    #[test]
    fn a_net_longer_than_its_class_allows_is_flagged() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.nets[0].netclass = Some("LEN".to_string());
        board.netclasses.push(synth_ir::NetClass {
            name: "LEN".to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: None,
            current: None,
            plane: false,
            via_diameter: None,
            via_drill: None,
            max_length: Some(synth_ir::Length::from_mm(10.0)),
            max_skew: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        });
        let net = board.nets[0].id;
        let long = Routing {
            segments: vec![straight(net, 127_000, 0, 50_000_000)],
            ..Routing::default()
        };
        let flagged = super::signal_integrity::check_net_length(&board, &long);
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].code, "E-SYNTH-DRC-018");
        let short = Routing {
            segments: vec![straight(net, 127_000, 0, 5_000_000)],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_net_length(&board, &short).is_empty());
    }

    fn bus_class(name: &str) -> synth_ir::NetClass {
        synth_ir::NetClass {
            name: name.to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: None,
            current: None,
            plane: false,
            max_length: None,
            max_skew: Some(synth_ir::Length::from_mm(1.0)),
            via_diameter: None,
            via_drill: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        }
    }

    #[test]
    fn a_matched_bus_with_too_much_skew_is_flagged() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.netclasses.push(bus_class("BUS"));
        board.nets[0].netclass = Some("BUS".to_string());
        board.nets[1].netclass = Some("BUS".to_string());
        let routing = Routing {
            segments: vec![
                straight(board.nets[0].id, 127_000, 0, 10_000_000),
                straight(board.nets[1].id, 127_000, 1_000_000, 20_000_000),
            ],
            ..Routing::default()
        };
        let flagged = super::signal_integrity::check_group_skew(&board, &routing);
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].code, "E-SYNTH-DRC-019");
        let matched = Routing {
            segments: vec![
                straight(board.nets[0].id, 127_000, 0, 10_000_000),
                straight(board.nets[1].id, 127_000, 1_000_000, 10_500_000),
            ],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_group_skew(&board, &matched).is_empty());
    }

    #[test]
    fn too_much_current_per_via_is_flagged() {
        let mut board = load_board("../../examples/env_logger/env_logger.synth");
        board.nets[0].netclass = Some("PWR".to_string());
        board.netclasses.push(synth_ir::NetClass {
            name: "PWR".to_string(),
            trace_width: None,
            clearance: None,
            color: None,
            impedance: None,
            current: Some(synth_ir::Current(3_000_000)),
            plane: false,
            max_length: None,
            max_skew: None,
            via_diameter: None,
            via_drill: None,
            source_span: synth_diagnostics::Span::new(0, 0),
        });
        let net = board.nets[0].id;
        let via = |x: i64| synth_pcb::Via {
            net,
            at: synth_geometry::Point::new(x, 0),
            drill_nm: 300_000,
            pad_diameter_nm: 600_000,
        };
        let one = Routing {
            vias: vec![via(0)],
            ..Routing::default()
        };
        let flagged = super::signal_integrity::check_via_current(&board, &one);
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].code, "W-SYNTH-DRC-020");
        let two = Routing {
            vias: vec![via(0), via(1_000_000)],
            ..Routing::default()
        };
        assert!(super::signal_integrity::check_via_current(&board, &two).is_empty());
    }
}
