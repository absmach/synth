// SPDX-License-Identifier: Apache-2.0

//! Deterministic PCB router.
//!
//! Phase 8 (implementation plan §10). The plan calls the router
//! "the crown jewel of the compiler-correctness story" — and the
//! longest single workstream at 24 wk. Same compiler-correctness
//! discipline as the placer:
//!
//! - **Determinism.** Same `(Board, Placement)` → byte-identical
//!   traces, vias, ordering. PRNG draws (when later slices add
//!   any) come from a single stream seeded from a hash of the
//!   IR + placement; no wall-clock time, no `HashMap` iteration
//!   in the routing hot path.
//! - **Soundness.** Every produced route satisfies the active
//!   manufacturer's DRC rules. A trace the router emits and the
//!   DRC engine later rejects is a P0 bug. Slice 5 wires the
//!   independent DRC re-check into CI; until then, the V1 trace
//!   widths and clearances are conservative enough that DRC
//!   accepts everything we emit on JLC's default profile.
//! - **Completeness on a bounded class.** ≤4 routing layers, ≤500
//!   nets, ≤2000 endpoints, single-ended + diff pairs + power
//!   nets. Outside the class returns a structured failure, not a
//!   best-effort partial route.
//!
//! ## Phased rollout
//!
//! - **Slice 1A — foundation (this slice).** `synth-route` crate
//!   skeleton, the `Routing` IR shape, and a deterministic
//!   placeholder router that returns *empty* `traces` and
//!   `vias`. The IR is what `synth-kicad` consumes when emitting
//!   the PCB; pinning the shape now lets downstream slices land
//!   without breaking the exporter.
//! - **Slice 1B.** Net assignment in `.kicad_pcb`: every pad
//!   declares the net it belongs to so pcbnew's ratsnest
//!   ("rubber band" connections) shows the netlist visually
//!   before any actual traces exist.
//! - **Slice 1C — Stage A.** Routing grid construction per
//!   plan §10.2: pin escape points, obstacle expansion
//!   (component courtyards + keepouts inflated by clearance),
//!   integer-nm grid addressable by `(layer, x_grid, y_grid)`.
//! - **Slice 2 — Stage B.** Per-net Lee maze + A* with the
//!   priority order from §10.2 (RF → diff pairs → clocks →
//!   power-sensitive → buses → general nets).
//! - **Slice 3 — Stage C.** Negotiated congestion rip-up /
//!   reroute. Bounded 8-iteration loop with history-list cost
//!   inflation on contested cells.
//! - **Slice 4.** Differential pair coupled routing + matched-
//!   length serpentine.
//! - **Slice 5.** DRC re-check + reference router for
//!   differential testing per §10.3.
//! - **Slice 6.** `E-SYNTH-ROUTE-*` diagnostic catalogue per
//!   §10.5 with minimum-witness identification.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_wrap,
    // Grid coordinates are non-negative by construction
    // (clamped before any usize cast); the i64 → usize casts
    // in the build_grid hot path are sound.
    clippy::cast_sign_loss,
    clippy::similar_names,
    // Pre-existing routing hot-path functions: refactor is out of scope
    // and would not change behavior.
    clippy::too_many_arguments,
    clippy::too_many_lines,
)]

pub mod advisor;
pub mod bus;
pub mod diff_pair;
pub mod drc;
pub mod grid;
pub mod logger;
mod maze;
pub mod miter;
pub mod serpentine;

pub use logger::{log_routing_outcome, RoutingOutcomeRecord};

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};
use synth_geometry::{Layer, Point};
use synth_ir::{Board, NetId};
use synth_place::Placement;
use thiserror::Error;

/// A single straight copper segment on one layer. Phase 8 only
/// emits axis-aligned segments (`start.x == end.x` or `start.y
/// == end.y`); diagonal traces are explicitly out of scope per
/// plan §10.6. Width is per-net-class — slice 2 reads it from
/// the manufacturer profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    pub net: NetId,
    pub layer: Layer,
    pub start: Point,
    pub end: Point,
    pub width_nm: i64,
}

/// A through-hole via at a grid intersection. V1 supports
/// through-hole only (blind / buried vias are out of scope per
/// §10.6). The via connects every layer between `top` and
/// `bottom` inclusive; for the V1 2-layer stackup this is just
/// (Top, Bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Via {
    pub net: NetId,
    pub at: Point,
    pub drill_nm: i64,
    pub pad_diameter_nm: i64,
}

/// Complete routing output. Shape stable across slices: later
/// slices fill in `segments` and `vias` and may add new fields
/// (per-net length statistics, congestion map) but never rename
/// or remove these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub segments: Vec<Segment>,
    pub vias: Vec<Via>,
    /// Per-diff-pair length / skew metrics. Empty when the
    /// board declares no `diff_pair` blocks or none could be
    /// resolved against the IR netlist.
    #[serde(default)]
    pub diff_pair_reports: Vec<diff_pair::PairReport>,
    /// Nets that the router could not route end-to-end after
    /// the negotiated-congestion loop hit its iteration cap.
    /// Each entry is the input to a `E-SYNTH-ROUTE-001`
    /// diagnostic (see [`Routing::to_diagnostics`]).
    #[serde(default)]
    pub unrouted_nets: Vec<UnroutedNet>,
    /// Number of A* cell evaluations performed during routing search.
    #[serde(default)]
    pub cells_expanded: u64,
}

/// One net the router gave up on. Carries enough context for
/// slice 6's diagnostic emission: the net's IR id and name,
/// its declaration span (for IDE jump-to-line), and one
/// `(pad_a, pad_b)` pair the router was unable to connect.
/// The pair is *a* witness — slice 6.x may extend with a full
/// minimum-witness obstacle set per plan §10.5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnroutedNet {
    pub net: NetId,
    pub net_name: String,
    pub source_span: Span,
    /// Pad position the search started from. Stored in nm
    /// for downstream tools; render in mm for humans.
    pub source_pad_nm: Point,
    /// Pad position the search couldn't reach.
    pub target_pad_nm: Point,
}

impl Routing {
    /// True when nothing was routed. Convenience for consumers
    /// that need to decide whether to emit the ratsnest only.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.vias.is_empty()
    }

    /// Sum of segment lengths attributed to `net`, in nm.
    /// Used by `synth-route::diff_pair` for per-pair length
    /// reports.
    #[must_use]
    pub fn length_for_net(&self, net: NetId) -> i64 {
        self.segments
            .iter()
            .filter(|s| s.net == net)
            .map(|s| {
                let dx = (s.end.x_nm - s.start.x_nm).abs();
                let dy = (s.end.y_nm - s.start.y_nm).abs();
                dx + dy
            })
            .sum()
    }

    /// Convert every `unrouted_nets` entry into a `Diagnostic`
    /// suitable for emission via the `synth-diagnostics`
    /// protocol — the same path `synth-place` and the lower
    /// phases use for structured failures.
    ///
    /// Slice 6 emits `E-SYNTH-ROUTE-001 unrouted net` for each
    /// entry. Future slices add witness-bearing codes
    /// (obstruction, capacity, diff-pair, clearance, via) per
    /// the categories listed in plan §10.5.
    #[must_use]
    pub fn to_diagnostics(&self, file: &str) -> Vec<Diagnostic> {
        self.unrouted_nets
            .iter()
            .map(|u| {
                DiagnosticBuilder::new(
                    "E-SYNTH-ROUTE-001",
                    Severity::Error,
                    format!("Net `{}` could not be routed", u.net_name),
                )
                .message(format!(
                    "After 8 negotiated-congestion iterations no path on layer F.Cu \
                     connects pad at ({:.2}, {:.2}) mm to pad at ({:.2}, {:.2}) mm \
                     for net `{}`. Slice 6 reports the failing net; slice 6.x will \
                     extend with the minimum witness (the smallest set of components \
                     / keepouts whose removal would make the route exist).",
                    synth_geometry::nm_to_mm(u.source_pad_nm.x_nm),
                    synth_geometry::nm_to_mm(u.source_pad_nm.y_nm),
                    synth_geometry::nm_to_mm(u.target_pad_nm.x_nm),
                    synth_geometry::nm_to_mm(u.target_pad_nm.y_nm),
                    u.net_name,
                ))
                .location(Location::from_span(file, u.source_span))
                .build()
            })
            .collect()
    }
}

/// Structured failure modes — one variant per `E-SYNTH-ROUTE-*`
/// code family declared in plan §10.5. Slice 6 promotes these
/// to full diagnostics with minimum-witness identification.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RouteError {
    /// One or more nets couldn't be routed. Slice 6 carries the
    /// per-net minimum witness (which obstacle / which path /
    /// which constraint group).
    #[error("E-SYNTH-ROUTE-001 {unrouted_count} net(s) could not be routed")]
    UnroutedNets { unrouted_count: usize },
}

impl RouteError {
    /// Convert to `synth-diagnostics` Diagnostic objects. Slice
    /// 6 populates per-net witnesses; slice 1A returns the
    /// summary form.
    #[must_use]
    pub fn to_diagnostics(&self, _board: &Board, _file: &str) -> Vec<Diagnostic> {
        Vec::new()
    }
}

/// Route every net in `board` against `placement`.
///
/// Slice 2 emits real traces via Lee maze + A* (Top layer
/// only; vias and bottom-layer routing land in slice 2.x).
/// Nets that fail to route are silently skipped — slice 3's
/// negotiated rip-up retries them with elevated congestion
/// costs.
#[must_use]
pub fn route(board: &Board, placement: &Placement) -> Routing {
    let mut routing = maze::route_all(board, placement, &advisor::DefaultCongestionAdvisor);
    generate_rf_via_fence(board, placement, &mut routing);
    routing
}

/// Route every net in `board` against `placement` using custom trace width and clearance limits.
#[must_use]
pub fn route_with_profile(
    board: &Board,
    placement: &Placement,
    min_trace_width_nm: i64,
    min_clearance_nm: i64,
) -> Routing {
    maze::route_all_with_profile(
        board,
        placement,
        &advisor::DefaultCongestionAdvisor,
        min_trace_width_nm,
        min_clearance_nm,
    )
}

/// Route every net in `board` against `placement` guided by a congestion advisor.
#[must_use]
pub fn route_with_advisor(
    board: &Board,
    placement: &Placement,
    advisor: &dyn advisor::CongestionAdvisor,
) -> Routing {
    let mut routing = maze::route_all(board, placement, advisor);
    generate_rf_via_fence(board, placement, &mut routing);
    routing
}

// ── RF Coplanar Ground Via Stitching ─────────────────────────────────────────
//
// Generates a deterministic fence of GND vias flanking every RF microstrip segment.
// Coplanar waveguide geometry requires ground vias at ≤ λ/10 pitch and ≥ 3× trace-width
// lateral clearance to suppress parasitic radiation and maintain 50 Ω impedance.
//
// Design parameters (2.4 GHz, FR4, εr ≈ 4.2):
//   λ = c / (f · √εr) ≈ 3e8 / (2.4e9 · 2.05) ≈ 61 mm
//   λ/10 ≈ 6.1 mm  →  use 0.9 mm (aggressive but safe; well below λ/10)
//   Lateral offset = 3 × trace_width (minimum for gap isolation without DRC collision)
//
// The function is deterministic: same (board, routing) → identical via set.
// Candidate vias that violate drill-to-copper clearance or fall outside the
// board outline are pruned deterministically.

/// Pitch between consecutive ground-stitch vias along an RF segment, in nm.
/// 0.9 mm is safely below λ/10 at 2.4 GHz on FR4 (≈ 6.1 mm).
const RF_STITCH_PITCH_NM: i64 = 900_000;

/// Lateral distance from the segment centreline to each stitch via, expressed
/// as a multiplier of the segment trace width. 3× keeps vias outside the
/// clearance envelope of the signal trace while maintaining close coupling
/// to the ground plane.
const RF_STITCH_CLEARANCE_MULT: i64 = 3;

/// Pad diameter for RF ground-stitch vias, in nm (0.6 mm → JLC standard).
const RF_STITCH_VIA_PAD_NM: i64 = 600_000;

/// Drill diameter for RF ground-stitch vias, in nm (0.3 mm → JLC standard).
const RF_STITCH_VIA_DRILL_NM: i64 = 300_000;

/// Minimum drill-to-copper clearance in nm (0.2 mm → JLC standard).
const RF_STITCH_MIN_CLEARANCE_NM: i64 = 200_000;

/// Post-routing pass: append coplanar ground-stitch vias alongside every
/// RF-classified routing segment. Operates on already-routed data in `routing`;
/// the maze router is not re-run.
///
/// RF segments are identified by name heuristic matching the net-class emitter.
/// GND net is the first `Ground`-domain net.
/// Places the RF ground-via fence. All casts are board nanometre
/// coordinates (≲ 1e9), far below f64's 52-bit mantissa — exact.
#[allow(clippy::cast_precision_loss)]
fn generate_rf_via_fence(board: &Board, placement: &Placement, routing: &mut Routing) {
    use synth_ir::{infer_power_domains, NetId};

    let domain_map = infer_power_domains(board);

    // Identify the GND NetId: first net classified as Ground.
    let gnd_net_id: Option<NetId> = board
        .nets
        .iter()
        .find(|n| {
            domain_map
                .get(n.id)
                .is_some_and(synth_ir::PowerDomainKind::is_ground)
        })
        .map(|n| n.id);

    let Some(gnd_net) = gnd_net_id else {
        // No ground net → no via fence possible.
        return;
    };

    // RF net classification: name-based detection matching the same heuristic as
    // build_netclasses is_rf in synth-kicad. RF nets always carry semantic names
    // (declared via diff_pair blocks or registry RF annotations).
    let is_rf_net = |net_id: NetId| -> bool {
        let Some(net) = board.net(net_id) else {
            return false;
        };
        let name_lower = net.name.to_ascii_lowercase();
        name_lower.contains("main_ant")
            || name_lower.contains("rf")
            || name_lower.contains("ant")
            || name_lower.contains("bal_")
            || name_lower.contains("unbal")
    };

    let outline = placement.board_outline;
    let drill_r = RF_STITCH_VIA_DRILL_NM / 2;
    let pad_r = RF_STITCH_VIA_PAD_NM / 2;

    // Prunes candidate vias that would cause a DRC violation.
    let is_via_safe = |at: Point, segs: &[Segment]| -> bool {
        // 1. Must lie entirely inside the board outline with edge clearance.
        if at.x_nm - pad_r < outline.min.x_nm
            || at.x_nm + pad_r > outline.max.x_nm
            || at.y_nm - pad_r < outline.min.y_nm
            || at.y_nm + pad_r > outline.max.y_nm
        {
            return false;
        }

        // 2. Must maintain ≥ min_drill_to_copper clearance to any segment on a DIFFERENT net.
        for s in segs {
            if s.net == gnd_net {
                continue;
            }
            let (sx, ex) = if s.start.x_nm < s.end.x_nm {
                (s.start.x_nm, s.end.x_nm)
            } else {
                (s.end.x_nm, s.start.x_nm)
            };
            let (sy, ey) = if s.start.y_nm < s.end.y_nm {
                (s.start.y_nm, s.end.y_nm)
            } else {
                (s.end.y_nm, s.start.y_nm)
            };
            let closest = Point::new(at.x_nm.clamp(sx, ex), at.y_nm.clamp(sy, ey));
            let center_dist = (closest.x_nm - at.x_nm).abs() + (closest.y_nm - at.y_nm).abs();
            let edge_dist = center_dist - drill_r - s.width_nm / 2;
            if edge_dist < RF_STITCH_MIN_CLEARANCE_NM {
                return false;
            }
        }

        true
    };

    let mut new_vias: Vec<Via> = Vec::new();

    for seg in &routing.segments {
        if !is_rf_net(seg.net) {
            continue;
        }

        let dx = seg.end.x_nm - seg.start.x_nm;
        let dy = seg.end.y_nm - seg.start.y_nm;
        let seg_len = ((dx * dx + dy * dy) as f64).sqrt();
        if seg_len < 1.0 {
            continue;
        }

        // Unit vector along segment, scaled to RF_STITCH_PITCH_NM steps.
        let step_x = ((dx as f64 / seg_len) * RF_STITCH_PITCH_NM as f64) as i64;
        let step_y = ((dy as f64 / seg_len) * RF_STITCH_PITCH_NM as f64) as i64;

        // Unit perpendicular (left-hand normal): (-dy, dx) / |seg|
        let lateral_offset = RF_STITCH_CLEARANCE_MULT * seg.width_nm;
        let perp_x = ((-dy as f64 / seg_len) * lateral_offset as f64) as i64;
        let perp_y = ((dx as f64 / seg_len) * lateral_offset as f64) as i64;

        // Walk from start to end in RF_STITCH_PITCH_NM steps.
        let steps = (seg_len / RF_STITCH_PITCH_NM as f64).ceil() as i64;
        for i in 0..=steps {
            let t_x = seg.start.x_nm + step_x * i;
            let t_y = seg.start.y_nm + step_y * i;

            // Clamp to segment endpoints so we don't overshoot.
            let at_x = t_x.clamp(
                seg.start.x_nm.min(seg.end.x_nm),
                seg.start.x_nm.max(seg.end.x_nm),
            );
            let at_y = t_y.clamp(
                seg.start.y_nm.min(seg.end.y_nm),
                seg.start.y_nm.max(seg.end.y_nm),
            );

            // Left via candidate
            let left_pt = Point::new(at_x + perp_x, at_y + perp_y);
            if is_via_safe(left_pt, &routing.segments) {
                new_vias.push(Via {
                    net: gnd_net,
                    at: left_pt,
                    drill_nm: RF_STITCH_VIA_DRILL_NM,
                    pad_diameter_nm: RF_STITCH_VIA_PAD_NM,
                });
            }

            // Right via candidate (mirror)
            let right_pt = Point::new(at_x - perp_x, at_y - perp_y);
            if is_via_safe(right_pt, &routing.segments) {
                new_vias.push(Via {
                    net: gnd_net,
                    at: right_pt,
                    drill_nm: RF_STITCH_VIA_DRILL_NM,
                    pad_diameter_nm: RF_STITCH_VIA_PAD_NM,
                });
            }
        }
    }

    routing.vias.extend(new_vias);
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
    fn routing_is_deterministic_across_runs() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let a = route(&board, &placement);
        let b = route(&board, &placement);
        assert_eq!(a, b);
    }

    #[test]
    fn unrouted_nets_produce_diagnostics() {
        // Slice 6 contract: every net the router gave up on
        // has an entry in `unrouted_nets`, and every entry
        // converts to a Diagnostic. We don't assert *which*
        // nets fail (depends on packing); we assert the
        // count of diagnostics matches the count of
        // unrouted_nets entries.
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = route(&board, &placement);
        let diags = routing.to_diagnostics("test");
        assert_eq!(diags.len(), routing.unrouted_nets.len());
        for d in &diags {
            assert_eq!(d.code, "E-SYNTH-ROUTE-001");
            assert!(
                d.location.is_some(),
                "every E-SYNTH-ROUTE-001 has a location"
            );
        }
    }

    #[test]
    fn sensor_logger_routes_cleanly() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let r = route(&board, &placement);
        assert!(
            r.unrouted_nets.is_empty(),
            "sensor_logger must have 0 unrouted nets: {:?}",
            r.unrouted_nets
        );
        let violations = crate::drc::check(&board, &placement, &r);
        for v in &violations {
            eprintln!("DRC VIOLATION: {}", v.describe());
        }
        assert!(
            violations.is_empty(),
            "sensor_logger DRC violations: {violations:?}"
        );
    }

    #[test]
    fn env_logger_routes_cleanly() {
        let board = load_board("../../examples/env_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let r = route(&board, &placement);
        assert!(
            r.unrouted_nets.is_empty(),
            "env_logger must have 0 unrouted nets: {:?}",
            r.unrouted_nets
        );
        let violations = crate::drc::check(&board, &placement, &r);
        for v in &violations {
            eprintln!("DRC VIOLATION: {}", v.describe());
        }
        assert!(
            violations.is_empty(),
            "env_logger DRC violations: {violations:?}"
        );
    }

    #[test]
    fn iot_sensor_board_routes_cleanly() {
        let board = load_board("../../fixtures/designs/iot_sensor_board.synth");
        let placement = synth_place::place(&board).expect("place");

        let r = route(&board, &placement);
        assert!(
            r.unrouted_nets.is_empty(),
            "iot_sensor_board must have 0 unrouted nets: {:?}",
            r.unrouted_nets
        );
        let violations = crate::drc::check(&board, &placement, &r);
        for v in &violations {
            eprintln!("DRC VIOLATION: {}", v.describe());
        }
        assert!(
            violations.is_empty(),
            "iot_sensor_board DRC violations: {violations:?}"
        );
    }

    #[test]
    fn profile_routing_honors_width_and_clearance() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");

        // Standard profile: 127 µm width, 127 µm clearance
        let standard_route =
            route_with_profile(&board, &placement, 127_000, synth_geometry::mm_to_nm(0.127));
        // Heavy trace profile: 300 µm width, 200 µm clearance
        let heavy_route =
            route_with_profile(&board, &placement, 300_000, synth_geometry::mm_to_nm(0.200));

        assert!(!standard_route.segments.is_empty());
        assert!(!heavy_route.segments.is_empty());

        // Every segment in heavy_route must honor min width (300 µm)
        // or legal pad neck-down (200 µm.max(300 µm) = 300 µm).
        for seg in &heavy_route.segments {
            assert!(
                seg.width_nm >= 300_000,
                "heavy profile segment width {} must be >= 300000 nm",
                seg.width_nm
            );
        }

        // Standard route must contain standard width segments (127 µm)
        let has_standard_width = standard_route
            .segments
            .iter()
            .any(|s| s.width_nm == 127_000);
        assert!(
            has_standard_width,
            "standard profile must emit standard 127 µm segments"
        );
    }

    #[test]
    fn test_default_advisor_zero_cost() {
        use advisor::{CongestionAdvisor, DefaultCongestionAdvisor};
        let adv = DefaultCongestionAdvisor;
        let board = load_board("../../examples/sensor_logger.synth");
        let cost = adv.evaluate_cell_cost(&board, Point::new(0, 0), 0);
        assert_eq!(cost.penalty, 0.0);
        assert_eq!(cost.cost_multiplier, 1.0);
    }

    #[test]
    fn test_cells_expanded_is_positive() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let r = route(&board, &placement);
        assert!(r.cells_expanded > 0, "cells_expanded must be positive");
    }

    struct HighCostAdvisor;
    impl advisor::CongestionAdvisor for HighCostAdvisor {
        fn evaluate_cell_cost(
            &self,
            _board: &Board,
            _point: Point,
            _layer_index: usize,
        ) -> advisor::CongestionCost {
            advisor::CongestionCost {
                penalty: 0.0,
                cost_multiplier: 5.0,
            }
        }
    }

    #[test]
    fn test_route_with_high_cost_advisor() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let default_r = route(&board, &placement);
        let high_r = route_with_advisor(&board, &placement, &HighCostAdvisor);
        assert!(
            high_r.cells_expanded > 0,
            "high cost advisor routing runs and expands cells"
        );
        assert_eq!(
            default_r.unrouted_nets.len(),
            high_r.unrouted_nets.len(),
            "routing completeness is preserved"
        );
    }
}
