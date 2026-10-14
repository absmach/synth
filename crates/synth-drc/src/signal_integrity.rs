// SPDX-License-Identifier: Apache-2.0

//! Signal-integrity rules that read the **routed** copper, not the declared
//! geometry.
//!
//! `synth-validate` already checks a design's declared trace geometry against
//! an impedance target at compile time (`E-SYNTH-IMPEDANCE-*`). These rules are
//! the independent, post-route counterpart: they take whatever copper the
//! router actually produced — re-derived from the `Routing` shapes, never from
//! the router's own statistics — and check it against the same physical
//! models. A trace the exporter derived but the router ignored is exactly the
//! failure this catches.
//!
//! Both rules only fire on a design that asked for the constraint: an
//! impedance rule needs an `impedance` target and a stackup, a skew rule needs
//! a declared `max_skew`. A design that declares neither is unaffected, so the
//! release gate's zero-violation bar is unchanged for boards that never opted
//! in.

use std::collections::BTreeMap;

use synth_geometry::{mm_to_nm, nm_to_mm, Layer, Point, StackupProfile};
use synth_ir::{Board, Length, Net, NetId};
use synth_pcb::{Routing, Segment};

use crate::profile::ManufacturerProfile;
use crate::Violation;

/// IPC-2221 external-layer ampacity: the width a trace needs to carry
/// `current_ua` with a `delta_t_c` rise over `thickness_nm` copper.
///
/// `A = (I / (k·ΔT^0.44))^(1/0.725)` gives the cross-section in mil², divided
/// by the copper thickness in mil to get the width. `k = 0.048` is the
/// external-layer constant. The result is a conservative starting point, not
/// a thermal simulation.
/// The constructor is public so the exporter can size a `current` net's trace
/// for the router, not only check it afterwards.
pub fn required_width_nm(current_ua: i64, thickness_nm: i64, delta_t_c: f64) -> i64 {
    #[allow(clippy::cast_precision_loss)]
    let current_a = current_ua as f64 / 1_000_000.0;
    let area_mil2 = (current_a / (0.048 * delta_t_c.powf(0.44))).powf(1.0 / 0.725);
    #[allow(clippy::cast_precision_loss)]
    let thickness_mil = (thickness_nm as f64 / 25_400.0).max(0.1);
    let width_mil = area_mil2 / thickness_mil;
    (width_mil * 25_400.0).round() as i64
}

/// Fractional deviation past which a routed impedance is a blocking error.
///
/// Matches the compile-time `E-SYNTH-IMPEDANCE-001` threshold. There is no
/// warning tier here because a [`Violation`] carries no severity: any
/// violation counts against the zero-DRC gate, so a rule must only fire when
/// the copper is genuinely wrong.
const ERROR_DEVIATION: f64 = 0.20;

/// The routed width and layer that carry the most copper for `net`.
///
/// A net's impedance is set by the trace that dominates it, so the longest
/// total length wins; ties break toward the lower layer index then the
/// narrower width, so the choice is deterministic for a given routing.
fn dominant_routing(routing: &Routing, net: NetId) -> Option<(Layer, i64)> {
    let mut by_key: BTreeMap<(Layer, i64), i64> = BTreeMap::new();
    for segment in routing.segments.iter().filter(|s| s.net == net) {
        let length_nm = (segment.end.x_nm - segment.start.x_nm).abs()
            + (segment.end.y_nm - segment.start.y_nm).abs();
        *by_key.entry((segment.layer, segment.width_nm)).or_default() += length_nm;
    }
    let mut entries: Vec<((Layer, i64), i64)> = by_key.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.first().map(|(key, _)| *key)
}

fn segments_for_net(routing: &Routing, net: NetId) -> Vec<synth_geometry::Point> {
    routing
        .segments
        .iter()
        .filter(|s| s.net == net)
        .flat_map(|s| [s.start, s.end])
        .collect()
}

/// Controlled-impedance copper check. Code: `E-SYNTH-DRC-012`.
///
/// For every `diff_pair` with an `impedance` target, estimate the impedance
/// the **routed** trace width gives on the layer it was routed on and compare
/// it with the target. Single-ended targets (a pair whose two halves are the
/// same net) use the microstrip/stripline model for the routed layer; a true
/// pair uses the coupled-microstrip model and the declared (or manufacturer
/// minimum) gap. An inner-layer pair returns no estimate from the coupled
/// model and is skipped rather than guessed at.
///
/// A leg with no copper is skipped: an unrouted net is `E-SYNTH-ROUTE-001`'s
/// concern, not this rule's.
///
/// Both a `diff_pair`'s `impedance` (differential, or single-ended when its
/// two halves are the same net) and a `netclass`'s `impedance` (single-ended)
/// feed this rule.
#[must_use]
pub fn check_controlled_impedance(
    board: &Board,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: &StackupProfile,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (positive, negative, _, impedance) in board.impedance_legs() {
        let partner = (positive.id != negative.id).then_some(negative);
        if let Some(v) = impedance_violation(
            board,
            routing,
            profile,
            stackup,
            positive,
            partner,
            impedance.to_ohms(),
        ) {
            violations.push(v);
        }
    }
    for class in &board.netclasses {
        let Some(impedance) = class.impedance else {
            continue;
        };
        for net in member_nets(board, class) {
            if let Some(v) = impedance_violation(
                board,
                routing,
                profile,
                stackup,
                net,
                None,
                impedance.to_ohms(),
            ) {
                violations.push(v);
            }
        }
    }
    violations
}

/// One controlled net's routed impedance check, or `None` when it is on
/// target or cannot be estimated.
fn impedance_violation(
    board: &Board,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: &StackupProfile,
    positive: &Net,
    partner: Option<&Net>,
    target: f64,
) -> Option<Violation> {
    let (layer, width_nm) = dominant_routing(routing, positive.id)?;
    if let Some(partner) = partner {
        // A pair is only meaningfully checked once both halves carry copper.
        dominant_routing(routing, partner.id)?;
    }
    let is_pair = partner.is_some();
    let actual = if is_pair {
        let gap_nm = board
            .declared_clearance(positive)
            .map_or(profile.min_copper_clearance_nm, |l| l.0);
        stackup.coupled_zdiff(layer, width_nm, gap_nm)?
    } else {
        stackup.z0(layer, width_nm)?.z0_ohms
    };
    let deviation = (actual - target).abs() / target.max(1.0);
    if deviation <= ERROR_DEVIATION {
        return None;
    }

    let subject = partner.map_or_else(
        || format!("net `{}`", positive.name),
        |p| format!("pair `{}`/`{}`", positive.name, p.name),
    );
    let expected = if is_pair {
        stackup
            .coupled_for_z0(
                layer,
                target,
                profile.min_trace_width_nm,
                profile.min_copper_clearance_nm,
            )
            .ok()
            .map(|g| format!("about {:.3} mm wide", g.width_mm))
    } else {
        stackup
            .width_for_z0(layer, target, profile.min_trace_width_nm)
            .ok()
            .map(|w| format!("about {:.3} mm wide", w.to_mm()))
    };
    let mut nets = vec![positive.id];
    nets.extend(partner.map(|p| p.id));
    let pos_mm = routing
        .segments
        .iter()
        .find(|s| s.net == positive.id)
        .map(|s| (nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm)));
    Some(Violation {
        code: "E-SYNTH-DRC-012".to_string(),
        message: format!(
            "{subject} is routed {:.3} mm wide on {}; that gives {actual:.1} ohm, \
             {:.0} percent off the {target:.1} ohm target{}",
            nm_to_mm(width_nm),
            layer.name(),
            deviation * 100.0,
            expected.map_or(String::new(), |e| format!(
                " (the target needs {e} on this stackup)"
            )),
        ),
        witness: segments_for_net(routing, positive.id),
        nets,
        components: Vec::new(),
        pos_mm,
        suggested_override: None,
    })
}

/// Nets joined to `class` by a `class "…"` declaration.
fn member_nets<'a>(
    board: &'a Board,
    class: &'a synth_ir::NetClass,
) -> impl Iterator<Item = &'a Net> {
    board
        .nets
        .iter()
        .filter(move |n| n.netclass.as_deref() == Some(class.name.as_str()))
}

/// Ampacity check. Code: `E-SYNTH-DRC-015`.
///
/// For every `netclass` that declares a `current`, the routed width of each
/// member net must reach the IPC-2221 width for that current over the layer's
/// copper thickness (the board stackup's when one is available, else 1 oz).
/// Only classes that declare a current are checked, so this never fires on a
/// design that has not asked for it.
#[must_use]
pub fn check_ampacity(
    board: &Board,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: Option<&StackupProfile>,
) -> Vec<Violation> {
    #[allow(clippy::cast_precision_loss)] // milli-degrees → degrees
    let delta_t_c = profile.ampacity_delta_t_milli_c as f64 / 1000.0;
    let mut violations = Vec::new();
    for class in &board.netclasses {
        let Some(current) = class.current else {
            continue;
        };
        for net in member_nets(board, class) {
            let Some((layer, width_nm)) = dominant_routing(routing, net.id) else {
                continue;
            };
            let thickness_nm = stackup
                .and_then(|s| s.geometry(layer))
                .map_or_else(|| mm_to_nm(0.035), |g| g.copper_thickness.0);
            let required_nm = required_width_nm(current.0, thickness_nm, delta_t_c);
            if width_nm >= required_nm {
                continue;
            }
            violations.push(Violation {
                code: "E-SYNTH-DRC-015".to_string(),
                message: format!(
                    "net `{}` is routed {:.3} mm wide on {}; carrying {:.2} A needs \
                     about {:.3} mm at a {} C rise",
                    net.name,
                    nm_to_mm(width_nm),
                    layer.name(),
                    current.to_amps(),
                    nm_to_mm(required_nm),
                    delta_t_c,
                ),
                witness: segments_for_net(routing, net.id),
                nets: vec![net.id],
                components: Vec::new(),
                pos_mm: routing
                    .segments
                    .iter()
                    .find(|s| s.net == net.id)
                    .map(|s| (nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
                suggested_override: None,
            });
        }
    }
    violations
}

/// Differential-pair length-match check. Code: `E-SYNTH-DRC-014`.
///
/// Sums the routed copper on each half of every `diff_pair` that declares a
/// `max_skew` and flags a pair whose halves differ by more than that. Computed
/// from the emitted segments, so it holds regardless of what a router reported
/// about its own length matching. A half with no copper is skipped.
#[must_use]
pub fn check_diff_pair_skew(board: &Board, routing: &Routing) -> Vec<Violation> {
    let mut violations = Vec::new();
    for pair in &board.diff_pairs {
        let Some(max_skew) = pair.max_skew else {
            continue;
        };
        let [(_, Some(positive)), (_, Some(negative))] = board.diff_pair_legs(pair) else {
            continue;
        };
        if positive.id == negative.id {
            continue;
        }
        let positive_len = routing.length_for_net(positive.id);
        let negative_len = routing.length_for_net(negative.id);
        if positive_len == 0 || negative_len == 0 {
            continue;
        }
        let skew = (positive_len - negative_len).abs();
        if skew <= max_skew.0 {
            continue;
        }
        violations.push(Violation {
            code: "E-SYNTH-DRC-014".to_string(),
            message: format!(
                "pair `{}`/`{}` is length-matched to {:.3} mm, longer than the {:.3} mm \
                 maximum skew",
                positive.name,
                negative.name,
                nm_to_mm(skew),
                Length(max_skew.0).to_mm(),
            ),
            witness: Vec::new(),
            nets: vec![positive.id, negative.id],
            components: Vec::new(),
            pos_mm: None,
            suggested_override: None,
        });
    }
    violations
}

/// The strongest coupled run between one pair of nets:
/// `(overlap_nm, edge_gap_nm, segment_a, segment_b, layer)`.
type CoupledRun = (i64, i64, Segment, Segment, Layer);

fn min_max(a: i64, b: i64) -> (i64, i64) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Edge (crosstalk) coupling between two axis-aligned segments that run
/// parallel, or `None` if they are not parallel, not close enough to couple,
/// or do not overlap. Returns `(edge_gap_nm, overlap_nm)`.
///
/// Traces the maze router emits are Manhattan, so the parallel test is done on
/// the axis-aligned cases; two perpendicular traces are handled by the
/// clearance rule, not here.
fn parallel_run(a: &Segment, b: &Segment, coupling_nm: i64) -> Option<(i64, i64)> {
    if a.layer != b.layer {
        return None;
    }
    let both_horizontal = a.start.y_nm == a.end.y_nm && b.start.y_nm == b.end.y_nm;
    let both_vertical = a.start.x_nm == a.end.x_nm && b.start.x_nm == b.end.x_nm;
    let (gap, overlap) = if both_horizontal {
        let gap = (a.start.y_nm - b.start.y_nm).abs() - (a.width_nm + b.width_nm) / 2;
        let (a_lo, a_hi) = min_max(a.start.x_nm, a.end.x_nm);
        let (b_lo, b_hi) = min_max(b.start.x_nm, b.end.x_nm);
        (gap, a_hi.min(b_hi) - a_lo.max(b_lo))
    } else if both_vertical {
        let gap = (a.start.x_nm - b.start.x_nm).abs() - (a.width_nm + b.width_nm) / 2;
        let (a_lo, a_hi) = min_max(a.start.y_nm, a.end.y_nm);
        let (b_lo, b_hi) = min_max(b.start.y_nm, b.end.y_nm);
        (gap, a_hi.min(b_hi) - a_lo.max(b_lo))
    } else {
        return None;
    };
    (gap >= 0 && gap <= coupling_nm && overlap > 0).then_some((gap, overlap))
}

/// Edge-coupling (crosstalk) check. Code: `W-SYNTH-DRC-013` — a **warning**.
///
/// Two foreign nets whose traces run parallel on the same layer, closer than
/// one dielectric height (`StackupProfile`), for more than the profile's
/// `crosstalk_run_multiple` dielectric heights are flagged as a coupling risk.
/// The mutual inductance and capacitance of a long close run couple more energy
/// than a short one, so run length gates the warning, and the multiple is a
/// profile knob rather than a hard-coded constant.
///
/// Advisory because how much coupling is acceptable depends on the signalling
/// scheme and cannot be judged from geometry alone. At most one warning is
/// emitted per net pair, for its worst run, so a dense board does not flood.
#[must_use]
pub fn check_edge_coupling(
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: &StackupProfile,
) -> Vec<Violation> {
    let run_multiple = profile.crosstalk_run_multiple;
    // Per net pair (ordered), the strongest coupled run.
    let mut worst: BTreeMap<(NetId, NetId), CoupledRun> = BTreeMap::new();
    for i in 0..routing.segments.len() {
        for j in (i + 1)..routing.segments.len() {
            let a = &routing.segments[i];
            let b = &routing.segments[j];
            if a.net == b.net {
                continue;
            }
            let Some(height_nm) = stackup.geometry(a.layer).map(|g| g.dielectric_height.0) else {
                continue;
            };
            let Some((gap, overlap)) = parallel_run(a, b, height_nm) else {
                continue;
            };
            if overlap < run_multiple * height_nm {
                continue;
            }
            let key = if a.net.0 <= b.net.0 {
                (a.net, b.net)
            } else {
                (b.net, a.net)
            };
            let entry = worst.entry(key).or_insert((overlap, gap, *a, *b, a.layer));
            if overlap > entry.0 {
                *entry = (overlap, gap, *a, *b, a.layer);
            }
        }
    }
    worst
        .into_iter()
        .map(
            |((first, second), (overlap, gap, a, _b, layer))| Violation {
                code: "W-SYNTH-DRC-013".to_string(),
                message: format!(
                    "nets {} and {} run parallel for {:.1} mm on {} {:.3} mm apart, within a \
                 dielectric height; risk of crosstalk",
                    first.0,
                    second.0,
                    nm_to_mm(overlap),
                    layer.name(),
                    nm_to_mm(gap),
                ),
                witness: vec![a.start, a.end],
                nets: vec![first, second],
                components: Vec::new(),
                pos_mm: Some((nm_to_mm(a.start.x_nm), nm_to_mm(a.start.y_nm))),
                suggested_override: None,
            },
        )
        .collect()
}

/// Return-path via check. Code: `W-SYNTH-DRC-016` — a **warning**.
///
/// A via that takes a signal to another layer needs a ground return via close
/// by: return current follows the signal's reference plane, and when the signal
/// changes layers that plane is interrupted, so the return has to jump too. A
/// signal via with no ground via within the stackup's thickest dielectric is a
/// discontinuity a fast edge radiates from.
///
/// Advisory because the current path is only inferred from geometry and whether
/// a discontinuity matters depends on the edge rate. Needs a declared stackup
/// (the distance is a property of the dielectric), like the other
/// signal-integrity rules; skipped without one.
#[must_use]
pub fn check_return_path_vias(
    board: &Board,
    routing: &Routing,
    stackup: &StackupProfile,
) -> Vec<Violation> {
    let domains = synth_ir::infer_power_domains(board);
    // Topology first, then the name/pin heuristic the exporter uses for plane
    // detection: the registry types ground pins `power_input`, not
    // `ground_reference`, so the domain map alone misses most grounds.
    let is_ground = |net_id: NetId| {
        board.net(net_id).is_some_and(|net| {
            domains
                .get(net_id)
                .is_some_and(synth_ir::PowerDomainKind::is_ground)
                || net_name_or_pin_is_ground(board, net)
        })
    };
    if !board.nets.iter().any(|net| is_ground(net.id)) {
        // No ground net at all: the discontinuity is the missing plane, which
        // is the design's, not this via's.
        return Vec::new();
    }
    let ground_vias: Vec<Point> = routing
        .vias
        .iter()
        .filter(|via| is_ground(via.net))
        .map(|via| via.at)
        .collect();
    let radius_nm = stackup
        .layers
        .iter()
        .map(|layer| layer.dielectric_height.0)
        .max()
        .unwrap_or_else(|| mm_to_nm(1.6));
    let mut out = Vec::new();
    for via in &routing.vias {
        if is_ground(via.net) {
            continue;
        }
        let has_return = ground_vias.iter().any(|ground| {
            (ground.x_nm - via.at.x_nm).abs() <= radius_nm
                && (ground.y_nm - via.at.y_nm).abs() <= radius_nm
        });
        if !has_return {
            out.push(Violation {
                code: "W-SYNTH-DRC-016".to_string(),
                message: format!(
                    "net {} changes layers at ({:.2}, {:.2}) mm with no ground return via within \
                     {:.2} mm, so the return current has to jump reference planes",
                    via.net.0,
                    nm_to_mm(via.at.x_nm),
                    nm_to_mm(via.at.y_nm),
                    nm_to_mm(radius_nm),
                ),
                witness: vec![via.at],
                nets: vec![via.net],
                components: Vec::new(),
                pos_mm: Some((nm_to_mm(via.at.x_nm), nm_to_mm(via.at.y_nm))),
                suggested_override: None,
            });
        }
    }
    out
}

/// Whether a net is ground by name (`gnd`, `vss`, `0v`) or by the name of a
/// connected pin. Mirrors the exporter's plane-net classifier, so a return via
/// is judged against the same ground the planes are built from.
pub(crate) fn net_name_or_pin_is_ground(board: &Board, net: &Net) -> bool {
    fn named(text: &str) -> bool {
        let text = text.to_ascii_lowercase();
        text.contains("gnd") || text.contains("vss") || text == "0v"
    }
    named(&net.name)
        || net.endpoints.iter().any(|endpoint| {
            board
                .component(endpoint.component)
                .and_then(|component| component.part.as_ref())
                .and_then(|part| part.pins.get(endpoint.pin.0 as usize))
                .is_some_and(|pin| named(&pin.name))
        })
}

/// Copper resistivity, ohm-metres.
const COPPER_RESISTIVITY: f64 = 1.68e-8;

/// Power-integrity check. Code: `W-SYNTH-DRC-017` — a **warning**.
///
/// A net that declares a `current` has the resistance of its routed copper
/// summed from the track geometry (length, width, and the layer's copper
/// thickness from the stackup) and turned into an IR drop `I·R`. Above the
/// budget a rail sags under load. Advisory: the sum treats a net's tracks in
/// series, so a net with parallel paths is over-estimated, and the budget is a
/// default rather than the design's own. Needs a declared stackup for the
/// copper thickness, like the other physics rules.
#[must_use]
#[allow(clippy::cast_precision_loss)] // nm → f64 metres; magnitudes are far below 2^53
pub fn check_ir_drop(
    board: &Board,
    routing: &Routing,
    profile: &ManufacturerProfile,
    stackup: &StackupProfile,
) -> Vec<Violation> {
    let budget_mv = profile.ir_drop_budget_mv as f64;
    let mut violations = Vec::new();
    for class in &board.netclasses {
        let Some(current) = class.current else {
            continue;
        };
        // A plane carries the current as a sheet, so summing the routed traces
        // would wildly over-estimate the drop; the breakout resistance is the
        // planner's to bound, not this rule's.
        if class.plane {
            continue;
        }
        for net in member_nets(board, class) {
            let mut resistance_ohms = 0.0_f64;
            for segment in routing.segments.iter().filter(|s| s.net == net.id) {
                let thickness_nm = stackup
                    .geometry(segment.layer)
                    .map_or_else(|| mm_to_nm(0.035), |geometry| geometry.copper_thickness.0);
                // Euclidean length, not Manhattan: a diagonal track is up to
                // ~41% shorter than abs(dx)+abs(dy), so Manhattan overestimates
                // its resistance and can raise a false W-SYNTH-DRC-017.
                let dx_nm = (segment.end.x_nm - segment.start.x_nm) as f64;
                let dy_nm = (segment.end.y_nm - segment.start.y_nm) as f64;
                let length_m = dx_nm.hypot(dy_nm) / 1e9;
                let area_m2 = (segment.width_nm as f64 / 1e9) * (thickness_nm as f64 / 1e9);
                if area_m2 > 0.0 {
                    resistance_ohms += COPPER_RESISTIVITY * length_m / area_m2;
                }
            }
            if resistance_ohms <= 0.0 {
                continue;
            }
            let drop_mv = current.to_amps() * resistance_ohms * 1000.0;
            if drop_mv <= budget_mv {
                continue;
            }
            violations.push(Violation {
                code: "W-SYNTH-DRC-017".to_string(),
                message: format!(
                    "net `{}` drops about {drop_mv:.0} mV carrying {:.2} A over {resistance_ohms:.3} \
                     ohm of copper, above the {budget_mv:.0} mV budget",
                    net.name,
                    current.to_amps(),
                ),
                witness: segments_for_net(routing, net.id),
                nets: vec![net.id],
                components: Vec::new(),
                pos_mm: routing
                    .segments
                    .iter()
                    .find(|s| s.net == net.id)
                    .map(|s| (nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
                suggested_override: None,
            });
        }
    }
    violations
}

/// Routed-length budget check. Code: `E-SYNTH-DRC-018`.
///
/// A net whose `netclass` declares `max_length` must not exceed it once routed.
/// This is the length half of a propagation-delay budget: on a known stackup
/// delay is proportional to length, so a length target is the same statement as
/// a delay target without a separate time unit. Only classes that declare a
/// length are checked.
#[must_use]
pub fn check_net_length(board: &Board, routing: &Routing) -> Vec<Violation> {
    let mut violations = Vec::new();
    for class in &board.netclasses {
        let Some(max_length) = class.max_length else {
            continue;
        };
        for net in member_nets(board, class) {
            let routed_nm = routing.length_for_net(net.id);
            if routed_nm <= max_length.0 {
                continue;
            }
            violations.push(Violation {
                code: "E-SYNTH-DRC-018".to_string(),
                message: format!(
                    "net `{}` is routed {:.1} mm, longer than the {:.1} mm the class allows",
                    net.name,
                    nm_to_mm(routed_nm),
                    max_length.to_mm(),
                ),
                witness: segments_for_net(routing, net.id),
                nets: vec![net.id],
                components: Vec::new(),
                pos_mm: routing
                    .segments
                    .iter()
                    .find(|s| s.net == net.id)
                    .map(|s| (nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
                suggested_override: None,
            });
        }
    }
    violations
}
