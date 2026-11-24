// SPDX-License-Identifier: Apache-2.0

//! Analytical (quadratic wirelength) placement seed.
//!
//! Opt-in seed behind [`crate::place_with_analytic_seed`]. It builds a
//! weighted clique model over the board's net endpoints, anchors the parts the
//! existing pipeline already treats as fixed, and solves the remaining centres
//! with a deterministic conjugate-gradient iteration. The output is a
//! [`RegionAssignment`] consumed by the same target-point search and legaliser
//! the CEM seed feeds; it is not a final [`crate::Placement`].
//!
//! `f64` is used inside the solve and converted to integer nanometres once at
//! the boundary. Pad offsets come from the same footprint lookup the router
//! and HPWL metric use, so a pin is the point the netlist names, not the body
//! centre.
//!
//! Scope is quadratic wirelength plus deterministic fan-out and routing-gap
//! projections. Density, WA/Nesterov, rotation optimisation and legalisation
//! stay out of this module: the prototype showed the density term costs final
//! HPWL on the reference toy board, so it is not in v1. The routing gap is a
//! second candidate, not a global rule; see the spaced entry point in
//! [`crate::place_with_analytic_seed_spaced`]. See
//! `docs/analytical-placement.md` for the model and the
//! `prototype/analytic-placement` branch for the feel test.

#![allow(clippy::cast_precision_loss)] // Board coordinates are far below 2^53 nm.

use std::collections::HashMap;

use synth_geometry::{Point, Rect, Rotation, mm_to_nm};
use synth_ir::{Board, ComponentId};

use crate::cem::RegionAssignment;
use crate::floorplan::FloorplanTarget;
use crate::{PadOffsetLookup, PinnedPlacements};

/// Fixed iteration budget. The residual may end the loop early only when it
/// is far below last-bit noise, so the output does not depend on a
/// data-dependent convergence path.
const CG_MAX_ITERATIONS: usize = 200;
const CG_TOLERANCE: f64 = 1e-9;

/// A tiny centre anchor keeps the linear system positive definite for parts
/// with no net and for subgraphs that touch no fixed part. It is negligible
/// against the unit net weights.
const CENTER_ANCHOR: f64 = 1e-5;

/// Extra routing gap for the spaced candidate, in millimetres.
///
/// The inflated courtyard already carries the placer's 1.5 mm margin. This
/// adds the channel the router needs on a dense board: without it
/// `feather_m4_express` packs to 260 mm of track against the default 389 mm
/// and loses two nets. The gap is a candidate, not a global rule: any nonzero
/// value also flips `secure_tracker` to 5-7 unrouted, so the unspaced
/// candidate stays available for the route loop to choose.
pub(crate) const ROUTING_GAP_MM: f64 = 2.0;

/// Bounded projection passes. The constraints converge in a few passes on
/// the reference boards; the cap stops a crowded board from cycling.
const SPACING_PASSES: usize = 8;

/// One placement instance in the linear system. The fields stay private to
/// the module; the unit tests build it directly.
struct Problem {
    free: Vec<bool>,
    anchors: Vec<Option<(f64, f64)>>,
    half: Vec<(f64, f64)>,
    dense: Vec<bool>,
    nets: Vec<Vec<(usize, (f64, f64))>>,
    usable: Rect,
    spacing_nm: f64,
}

/// Produce region hints from the analytical seed.
///
/// Anchor precedence mirrors the target-point search: sidecar-pinned parts,
/// then absolute `placement_hint`s, then floorplan targets. Parts covered by a
/// relative (`near`) hint are left free; their existing relative placement wins
/// at target selection.
#[allow(clippy::implicit_hasher)]
pub(crate) fn region_assign(
    board: &Board,
    usable: Rect,
    fp_targets: &HashMap<ComponentId, FloorplanTarget>,
    rotation_overrides: &HashMap<ComponentId, Rotation>,
    pinned: &PinnedPlacements,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
    pad_offsets: &PadOffsetLookup,
    spacing_mm: f64,
) -> RegionAssignment {
    let anchors = anchor_positions(board, usable, fp_targets, rotation_overrides, pinned);
    let mut ids = Vec::with_capacity(board.components.len());
    let mut index = HashMap::with_capacity(board.components.len());
    for (position, component) in board.components.iter().enumerate() {
        ids.push(component.id);
        index.insert(component.id, position);
    }

    let free: Vec<bool> = ids.iter().map(|id| !anchors.contains_key(id)).collect();
    let rotations: Vec<Rotation> = ids
        .iter()
        .map(|id| {
            anchors.get(id).map_or_else(
                || {
                    rotation_overrides
                        .get(id)
                        .copied()
                        .unwrap_or(Rotation::Zero)
                },
                |(_, rotation)| *rotation,
            )
        })
        .collect();
    let half: Vec<(f64, f64)> = ids
        .iter()
        .enumerate()
        .map(|(position, id)| {
            let (w_mm, h_mm) = courtyard_lookup.get(id).copied().unwrap_or((10.0, 10.0));
            rotated_half_extents(
                rotations[position],
                mm_to_nm(w_mm) as f64,
                mm_to_nm(h_mm) as f64,
            )
        })
        .collect();
    let anchor_points: Vec<Option<(f64, f64)>> = ids
        .iter()
        .map(|id| {
            anchors
                .get(id)
                .map(|(point, _)| (point.x_nm as f64, point.y_nm as f64))
        })
        .collect();
    let dense: Vec<bool> = ids
        .iter()
        .map(|id| crate::is_dense_part(board, *id))
        .collect();
    let nets = net_endpoints(board, &index, &rotations, pad_offsets);

    let problem = Problem {
        free,
        anchors: anchor_points,
        half,
        dense,
        nets,
        usable,
        spacing_nm: mm_to_nm(spacing_mm) as f64,
    };
    let solved = solve(&problem);
    let hints = ids
        .into_iter()
        .zip(solved)
        .collect::<HashMap<ComponentId, Point>>();
    RegionAssignment { hints }
}

/// The quadratic initial placement for the RePlAce solver: component ids,
/// centres, rotations and the free mask, in board order.
#[allow(clippy::implicit_hasher, clippy::too_many_arguments)]
pub(crate) fn initial_positions(
    board: &Board,
    usable: Rect,
    baseline: &crate::Placement,
    fp_targets: &HashMap<ComponentId, FloorplanTarget>,
    rotation_overrides: &HashMap<ComponentId, Rotation>,
    pinned: &PinnedPlacements,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
    pad_offsets: &PadOffsetLookup,
) -> (Vec<ComponentId>, Vec<Point>, Vec<Rotation>, Vec<bool>) {
    let anchors = hard_anchor_positions(
        board,
        usable,
        fp_targets,
        rotation_overrides,
        pinned,
        baseline,
    );
    let mut ids = Vec::with_capacity(board.components.len());
    let mut index = HashMap::with_capacity(board.components.len());
    for (position, component) in board.components.iter().enumerate() {
        ids.push(component.id);
        index.insert(component.id, position);
    }
    let free: Vec<bool> = ids.iter().map(|id| !anchors.contains_key(id)).collect();
    let rotations: Vec<Rotation> = ids
        .iter()
        .map(|id| {
            anchors.get(id).map_or_else(
                || {
                    rotation_overrides
                        .get(id)
                        .copied()
                        .unwrap_or(Rotation::Zero)
                },
                |(_, rotation)| *rotation,
            )
        })
        .collect();
    let half: Vec<(f64, f64)> = ids
        .iter()
        .enumerate()
        .map(|(position, id)| {
            let (w_mm, h_mm) = courtyard_lookup.get(id).copied().unwrap_or((10.0, 10.0));
            rotated_half_extents(
                rotations[position],
                mm_to_nm(w_mm) as f64,
                mm_to_nm(h_mm) as f64,
            )
        })
        .collect();
    let anchor_points: Vec<Option<(f64, f64)>> = ids
        .iter()
        .map(|id| {
            anchors
                .get(id)
                .map(|(point, _)| (point.x_nm as f64, point.y_nm as f64))
        })
        .collect();
    let dense: Vec<bool> = ids
        .iter()
        .map(|id| crate::is_dense_part(board, *id))
        .collect();
    let nets = net_endpoints(board, &index, &rotations, pad_offsets);
    let problem = Problem {
        free: free.clone(),
        anchors: anchor_points,
        half,
        dense,
        nets,
        usable,
        spacing_nm: 0.0,
    };
    let centres = solve(&problem);
    (ids, centres, rotations, free)
}

/// The truly mechanical anchors for a continuous placement.
///
/// Floorplan targets are hints the greedy scanner can move; freezing all of
/// them makes a different arrangement illegal before legalization starts.
/// Connectors keep the default placement's legal edge position (the baseline
/// already respects their hints), hard absolute hints anchor their part, and
/// sidecar pins win over both.
#[allow(clippy::implicit_hasher)]
fn hard_anchor_positions(
    board: &Board,
    usable: Rect,
    fp_targets: &HashMap<ComponentId, FloorplanTarget>,
    rotation_overrides: &HashMap<ComponentId, Rotation>,
    pinned: &PinnedPlacements,
    baseline: &crate::Placement,
) -> HashMap<ComponentId, (Point, Rotation)> {
    let mut anchors: HashMap<ComponentId, (Point, Rotation)> = pinned
        .iter()
        .map(|(id, (point, rotation))| (*id, (*point, *rotation)))
        .collect();
    let baseline_by_id: HashMap<ComponentId, (Point, Rotation)> = baseline
        .components
        .iter()
        .map(|placed| (placed.id, (placed.center, placed.rotation)))
        .collect();
    let no_relative_hints: HashMap<String, Rect> = HashMap::new();
    for component in &board.components {
        if anchors.contains_key(&component.id) {
            continue;
        }
        let hint = component.placement_hint.as_ref();
        let mechanical_edge = hint.is_some_and(|hint| hint.edge.is_some());
        let mechanical_kind = matches!(component.kind.as_str(), "connector" | "jack");
        let hard_region = hint.is_some_and(|hint| {
            hint.priority == synth_ir::PlacementPriority::Hard && hint.region.is_some()
        });
        if !mechanical_edge && !mechanical_kind && !hard_region {
            continue;
        }
        if mechanical_kind && let Some((point, rotation)) = baseline_by_id.get(&component.id) {
            anchors.insert(component.id, (*point, *rotation));
            continue;
        }
        let hint_target = hint.and_then(|hint| {
            let absolute = hint.edge.is_some()
                || (hint.priority == synth_ir::PlacementPriority::Hard && hint.region.is_some());
            absolute
                .then(|| crate::resolve_hint_target(hint, usable, &no_relative_hints).target)
                .flatten()
        });
        let target =
            hint_target.or_else(|| fp_targets.get(&component.id).map(|target| target.point));
        let Some(point) = target else {
            continue;
        };
        let rotation = fp_targets.get(&component.id).map_or_else(
            || {
                rotation_overrides
                    .get(&component.id)
                    .copied()
                    .unwrap_or(Rotation::Zero)
            },
            |target| target.rotation,
        );
        anchors.insert(component.id, (point, rotation));
    }
    anchors
}

/// Resolve the parts the existing pipeline already fixes, in the same
/// precedence order as the target-point search.
///
/// The greedy loop also has a child-module relative-offset branch and a
/// dense-connector edge rewrite that run later. Those are deliberately not
/// reproduced here: the seed is a hint, and the target search remains the
/// authority for them.
#[allow(clippy::implicit_hasher)]
fn anchor_positions(
    board: &Board,
    usable: Rect,
    fp_targets: &HashMap<ComponentId, FloorplanTarget>,
    rotation_overrides: &HashMap<ComponentId, Rotation>,
    pinned: &PinnedPlacements,
) -> HashMap<ComponentId, (Point, Rotation)> {
    let mut anchors: HashMap<ComponentId, (Point, Rotation)> = pinned
        .iter()
        .map(|(id, (point, rotation))| (*id, (*point, *rotation)))
        .collect();
    let no_relative_hints: HashMap<String, Rect> = HashMap::new();

    for component in &board.components {
        if anchors.contains_key(&component.id) {
            continue;
        }
        // A `near` hint is relative to a part that is placed during the
        // greedy loop. Anchoring it here would freeze the seed against a
        // target the search later ignores; leave it free instead.
        if component
            .placement_hint
            .as_ref()
            .is_some_and(|hint| hint.near.is_some())
        {
            continue;
        }
        let fp_target = fp_targets.get(&component.id);
        let rotation = fp_target.map_or_else(
            || {
                rotation_overrides
                    .get(&component.id)
                    .copied()
                    .unwrap_or(Rotation::Zero)
            },
            |target| target.rotation,
        );
        let hint_target = component.placement_hint.as_ref().and_then(|hint| {
            if hint.region.is_none() && hint.edge.is_none() {
                return None;
            }
            crate::resolve_hint_target(hint, usable, &no_relative_hints).target
        });

        if let Some(point) = hint_target {
            anchors.insert(component.id, (point, rotation));
        } else if let Some(target) = fp_target {
            anchors.insert(component.id, (target.point, target.rotation));
        }
    }

    anchors
}

/// Flatten the netlist into `(component index, rotated pad offset)` pins.
///
/// A net with one endpoint, or one whose components are all unknown, cannot
/// move anything and is dropped.
fn net_endpoints(
    board: &Board,
    index: &HashMap<ComponentId, usize>,
    rotations: &[Rotation],
    pad_offsets: &PadOffsetLookup,
) -> Vec<Vec<(usize, (f64, f64))>> {
    let mut nets = Vec::new();
    for net in &board.nets {
        if net.endpoints.len() < 2 {
            continue;
        }
        let pins: Vec<(usize, (f64, f64))> = net
            .endpoints
            .iter()
            .filter_map(|endpoint| {
                let &position = index.get(&endpoint.component)?;
                let (offset_x, offset_y) = pad_offsets
                    .lookup(endpoint.component, endpoint.pin.0 as usize)
                    .unwrap_or((0, 0));
                let (rotated_x, rotated_y) = rotations[position].rotate_offset(offset_x, offset_y);
                Some((position, (rotated_x as f64, rotated_y as f64)))
            })
            .collect();
        if pins.len() >= 2 {
            nets.push(pins);
        }
    }
    nets
}

/// Solve the quadratic wirelength problem and return one integer-nanometre
/// centre per component, in board order.
#[allow(clippy::cast_precision_loss)]
fn solve(problem: &Problem) -> Vec<Point> {
    let n = problem.anchors.len();
    let mut free_index = vec![None; n];
    let mut free_count = 0_usize;
    for (position, is_free) in problem.free.iter().copied().enumerate() {
        if is_free {
            free_index[position] = Some(free_count);
            free_count += 1;
        }
    }

    let mut matrix = vec![vec![0.0_f64; free_count]; free_count];
    let mut rhs_x = vec![0.0_f64; free_count];
    let mut rhs_y = vec![0.0_f64; free_count];

    for net in &problem.nets {
        let k = net.len();
        if k < 2 {
            continue;
        }
        let weight = 1.0 / (k - 1) as f64;
        for first in 0..k {
            for second in (first + 1)..k {
                let (first_component, first_offset) = net[first];
                let (second_component, second_offset) = net[second];
                if first_component == second_component {
                    continue;
                }
                match (free_index[first_component], free_index[second_component]) {
                    (Some(first_var), Some(second_var)) => {
                        matrix[first_var][first_var] += weight;
                        matrix[second_var][second_var] += weight;
                        matrix[first_var][second_var] -= weight;
                        matrix[second_var][first_var] -= weight;
                        rhs_x[first_var] += weight * (second_offset.0 - first_offset.0);
                        rhs_y[first_var] += weight * (second_offset.1 - first_offset.1);
                        rhs_x[second_var] += weight * (first_offset.0 - second_offset.0);
                        rhs_y[second_var] += weight * (first_offset.1 - second_offset.1);
                    }
                    (Some(first_var), None) => {
                        let (anchor_x, anchor_y) = problem.anchors[second_component]
                            .unwrap_or_else(|| problem_center(problem.usable));
                        matrix[first_var][first_var] += weight;
                        rhs_x[first_var] += weight * (anchor_x + second_offset.0 - first_offset.0);
                        rhs_y[first_var] += weight * (anchor_y + second_offset.1 - first_offset.1);
                    }
                    (None, Some(second_var)) => {
                        let (anchor_x, anchor_y) = problem.anchors[first_component]
                            .unwrap_or_else(|| problem_center(problem.usable));
                        matrix[second_var][second_var] += weight;
                        rhs_x[second_var] += weight * (anchor_x + first_offset.0 - second_offset.0);
                        rhs_y[second_var] += weight * (anchor_y + first_offset.1 - second_offset.1);
                    }
                    (None, None) => {}
                }
            }
        }
    }

    let center = problem_center(problem.usable);
    for variable in 0..free_count {
        matrix[variable][variable] += CENTER_ANCHOR;
        rhs_x[variable] += CENTER_ANCHOR * center.0;
        rhs_y[variable] += CENTER_ANCHOR * center.1;
    }

    let initial = vec![center.0; free_count];
    let solved_x = conjugate_gradient(&matrix, &rhs_x, &initial);
    let solved_y = conjugate_gradient(&matrix, &rhs_y, &initial);

    let mut centers = Vec::with_capacity(n);
    for (position, is_free) in problem.free.iter().copied().enumerate() {
        let (center_x, center_y) = match (is_free, problem.anchors[position], free_index[position])
        {
            (false, Some(anchor), _) => anchor,
            (true, _, Some(variable)) => (solved_x[variable], solved_y[variable]),
            _ => center,
        };
        if is_free {
            let mut point = Point::new(center_x.round() as i64, center_y.round() as i64);
            clamp_center(problem, position, &mut point);
            centers.push(point);
        } else {
            centers.push(Point::new(center_x.round() as i64, center_y.round() as i64));
        }
    }

    project_constraints(problem, &mut centers);
    centers
}

/// Apply the dense-ring and pairwise-spacing constraints together.
///
/// The dense ring keeps passives out of an MCU's fan-out; the pairwise gap
/// keeps routing channels between everything else. They can fight, so the
/// final pass is always the ring: a passive in a fan-out corridor is a harder
/// failure than a tight pair, and the legaliser handles the residual overlap.
fn project_constraints(problem: &Problem, centers: &mut [Point]) {
    for _ in 0..SPACING_PASSES {
        let ring_changed = push_out_of_dense_rings_once(problem, centers);
        let spacing_changed = if problem.spacing_nm > 0.0 {
            push_apart_once(problem, centers)
        } else {
            false
        };
        if !ring_changed && !spacing_changed {
            break;
        }
    }
    let _ = push_out_of_dense_rings_once(problem, centers);
}

/// Keep passives out of the fan-out ring around dense parts.
///
/// The greedy placer gives a part with at least [`crate::DENSE_PART_PINS`]
/// pins a [`crate::DENSE_PART_RING_MM`] ring so the router can escape its
/// pins. A pure wirelength solve pulls decoupling parts straight into that
/// ring, which is exactly the case the real-board A/B caught: lower HPWL,
/// more unrouted nets. One pass, deterministic order; the caller bounds the
/// repetition.
fn push_out_of_dense_rings_once(problem: &Problem, centers: &mut [Point]) -> bool {
    // The ring is added to the dense courtyard's width/height, so half of it
    // is the clearance per side. Using the full ring here doubles the gap and
    // spreads the board out.
    let ring_nm = mm_to_nm(crate::DENSE_PART_RING_MM / 2.0) as f64;
    let mut changed = false;
    for first in 0..centers.len() {
        if !problem.free[first] || problem.dense[first] {
            continue;
        }
        for second in 0..centers.len() {
            if first == second || !problem.dense[second] {
                continue;
            }
            changed |= push_pair(problem, centers, first, second, ring_nm, false);
        }
    }
    changed
}

/// Push free parts apart until every pair has the extra routing gap.
///
/// Free-free pairs split the penetration; a free-anchor pair moves only the
/// free part. Anchors are sidecar pins, absolute hints and floorplan targets,
/// so a mechanical constraint never moves for a channel.
fn push_apart_once(problem: &Problem, centers: &mut [Point]) -> bool {
    let mut changed = false;
    for first in 0..centers.len() {
        for second in (first + 1)..centers.len() {
            if !problem.free[first] && !problem.free[second] {
                continue;
            }
            changed |= push_pair(problem, centers, first, second, problem.spacing_nm, true);
        }
    }
    changed
}

/// Resolve one pair along its smaller penetration axis.
///
/// `split_free_pair` is the only difference between the fan-out ring and the
/// routing gap: the ring moves the passive away from a dense part and leaves
/// the dense part alone even when it is free, while two free parts share the
/// spacing push. The shift is split without losing the odd nanometre, and the
/// return value is true only when a point actually moved.
fn push_pair(
    problem: &Problem,
    centers: &mut [Point],
    first: usize,
    second: usize,
    gap_nm: f64,
    split_free_pair: bool,
) -> bool {
    let first_free = problem.free[first];
    let second_free = problem.free[second];
    if !first_free && !second_free {
        return false;
    }
    let (first_half_w, first_half_h) = problem.half[first];
    let (second_half_w, second_half_h) = problem.half[second];
    let need_x = first_half_w + second_half_w + gap_nm;
    let need_y = first_half_h + second_half_h + gap_nm;
    let dx = (centers[first].x_nm - centers[second].x_nm) as f64;
    let dy = (centers[first].y_nm - centers[second].y_nm) as f64;
    if dx.abs() >= need_x || dy.abs() >= need_y {
        return false;
    }

    let push_x = need_x - dx.abs();
    let push_y = need_y - dy.abs();
    let (axis_x, direction, magnitude) = if push_x <= push_y {
        (
            true,
            if dx >= 0.0 { 1_i64 } else { -1_i64 },
            push_x.round() as i64,
        )
    } else {
        (
            false,
            if dy >= 0.0 { 1_i64 } else { -1_i64 },
            push_y.round() as i64,
        )
    };

    let (first_shift, second_shift) = if split_free_pair && first_free && second_free {
        let half = magnitude / 2;
        (half, magnitude - half)
    } else if first_free {
        (magnitude, 0)
    } else {
        (0, magnitude)
    };

    let mut first_point = centers[first];
    let mut second_point = centers[second];
    if axis_x {
        first_point.x_nm += direction * first_shift;
        second_point.x_nm -= direction * second_shift;
    } else {
        first_point.y_nm += direction * first_shift;
        second_point.y_nm -= direction * second_shift;
    }
    if first_free {
        clamp_center(problem, first, &mut first_point);
    }
    if second_free {
        clamp_center(problem, second, &mut second_point);
    }
    let moved = first_point != centers[first] || second_point != centers[second];
    centers[first] = first_point;
    centers[second] = second_point;
    moved
}

fn clamp_center(problem: &Problem, position: usize, center: &mut Point) {
    let (half_w, half_h) = problem.half[position];
    center.x_nm = clamp_axis(
        center.x_nm as f64,
        problem.usable.min.x_nm as f64 + half_w,
        problem.usable.max.x_nm as f64 - half_w,
    )
    .round() as i64;
    center.y_nm = clamp_axis(
        center.y_nm as f64,
        problem.usable.min.y_nm as f64 + half_h,
        problem.usable.max.y_nm as f64 - half_h,
    )
    .round() as i64;
}

fn problem_center(usable: Rect) -> (f64, f64) {
    (
        (usable.min.x_nm + usable.width_nm() / 2) as f64,
        (usable.min.y_nm + usable.height_nm() / 2) as f64,
    )
}

/// Jacobi-preconditioned conjugate gradient. The diagonal preconditioner is
/// applied by scaling the initial residual; the matrix is small and sparse in
/// net degree, so the dense representation is deliberate and simple.
fn conjugate_gradient(matrix: &[Vec<f64>], rhs: &[f64], initial: &[f64]) -> Vec<f64> {
    let n = rhs.len();
    let mut solution = initial.to_vec();
    if n == 0 {
        return solution;
    }

    let mut residual = rhs.to_vec();
    for row in 0..n {
        residual[row] -= dot(&matrix[row], &solution);
    }

    // Jacobi preconditioner: z = M^-1 r, with the diagonal terms guaranteed
    // positive by the centre anchor.
    let mut preconditioned = residual.clone();
    for row in 0..n {
        let diagonal = matrix[row][row];
        if diagonal > 0.0 {
            preconditioned[row] /= diagonal;
        }
    }

    let mut direction = preconditioned.clone();
    let mut residual_dot = dot(&residual, &preconditioned);
    if dot(&residual, &residual).sqrt() <= CG_TOLERANCE {
        return solution;
    }

    for _ in 0..CG_MAX_ITERATIONS {
        let product: Vec<f64> = (0..n).map(|row| dot(&matrix[row], &direction)).collect();
        let denominator = dot(&direction, &product);
        if denominator.abs() <= f64::EPSILON {
            break;
        }
        let alpha = residual_dot / denominator;
        for row in 0..n {
            solution[row] += alpha * direction[row];
            residual[row] -= alpha * product[row];
        }
        if dot(&residual, &residual).sqrt() <= CG_TOLERANCE {
            break;
        }
        for row in 0..n {
            let diagonal = matrix[row][row];
            if diagonal > 0.0 {
                preconditioned[row] = residual[row] / diagonal;
            } else {
                preconditioned[row] = residual[row];
            }
        }
        let next_residual_dot = dot(&residual, &preconditioned);
        let beta = next_residual_dot / residual_dot;
        for row in 0..n {
            direction[row] = preconditioned[row] + beta * direction[row];
        }
        residual_dot = next_residual_dot;
    }

    solution
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn rotated_half_extents(rotation: Rotation, width_nm: f64, height_nm: f64) -> (f64, f64) {
    if rotation.swaps_extents() {
        (height_nm / 2.0, width_nm / 2.0)
    } else {
        (width_nm / 2.0, height_nm / 2.0)
    }
}

fn clamp_axis(value: f64, low: f64, high: f64) -> f64 {
    if low > high {
        low + (high - low) / 2.0
    } else {
        value.max(low).min(high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(
        free: Vec<bool>,
        anchors: Vec<Option<(f64, f64)>>,
        nets: Vec<Vec<(usize, (f64, f64))>>,
    ) -> Problem {
        let count = free.len();
        Problem {
            free,
            anchors,
            half: vec![(5_000_000.0, 5_000_000.0); count],
            dense: vec![false; count],
            nets,
            usable: Rect::new(Point::new(0, 0), Point::new(100_000_000, 100_000_000)),
            spacing_nm: 0.0,
        }
    }

    #[test]
    fn the_dense_ring_leaves_half_the_ring_as_clearance() {
        // The default placer inflates a dense courtyard by the full ring, so
        // the gap to a neighbour is half the ring per side. The analytical
        // projection must use the same clearance, not the full ring.
        let mut instance = problem(
            vec![false, true],
            vec![Some((50_000_000.0, 50_000_000.0)), None],
            Vec::new(),
        );
        instance.dense = vec![true, false];
        instance.half = vec![(2_000_000.0, 2_000_000.0), (1_000_000.0, 1_000_000.0)];

        let solved = solve(&instance);
        let gap = (solved[0].x_nm - solved[1].x_nm).abs() as f64 - 3_000_000.0;
        let expected = mm_to_nm(crate::DENSE_PART_RING_MM / 2.0) as f64;
        assert!(
            (gap - expected).abs() < 1.0,
            "dense ring gap {gap} nm, expected {expected} nm"
        );
    }

    #[test]
    fn a_chain_puts_the_middle_part_at_the_midpoint() {
        let instance = problem(
            vec![false, true, false],
            vec![
                Some((10_000_000.0, 50_000_000.0)),
                None,
                Some((90_000_000.0, 50_000_000.0)),
            ],
            vec![
                vec![(0, (0.0, 0.0)), (1, (0.0, 0.0))],
                vec![(1, (0.0, 0.0)), (2, (0.0, 0.0))],
            ],
        );

        let solved = solve(&instance);
        assert!(
            (solved[1].x_nm as f64 - 50_000_000.0).abs() < 100_000.0,
            "{:?}",
            solved[1]
        );
        assert!((solved[1].y_nm as f64 - 50_000_000.0).abs() < 100_000.0);
    }

    #[test]
    fn pad_offsets_move_the_body_not_the_centre() {
        let mut instance = problem(
            vec![false, true],
            vec![Some((10_000_000.0, 50_000_000.0)), None],
            vec![vec![(0, (5_000_000.0, 0.0)), (1, (-5_000_000.0, 0.0))]],
        );
        // Isolate the pad-offset effect from the routing gap: the solved
        // centres are 10 mm apart, which is already wider than 2 mm.
        instance.half = vec![(0.0, 0.0); 2];

        let solved = solve(&instance);
        assert!(
            (solved[1].x_nm as f64 - 20_000_000.0).abs() < 100_000.0,
            "{:?}",
            solved[1]
        );
    }

    #[test]
    fn a_netless_part_falls_back_to_the_usable_centre() {
        let instance = problem(vec![true], vec![None], Vec::new());
        let solved = solve(&instance);
        assert_eq!(solved[0], Point::new(50_000_000, 50_000_000));
    }

    #[test]
    fn two_free_parts_on_one_net_keep_the_spacing_gap() {
        let mut instance = problem(
            vec![true, true],
            vec![None, None],
            vec![vec![(0, (0.0, 0.0)), (1, (0.0, 0.0))]],
        );
        instance.spacing_nm = mm_to_nm(ROUTING_GAP_MM) as f64;
        let solved = solve(&instance);
        let needed = 2.0 * 5_000_000.0 + instance.spacing_nm;
        let dx = (solved[0].x_nm - solved[1].x_nm).abs() as f64;
        let dy = (solved[0].y_nm - solved[1].y_nm).abs() as f64;
        assert!(
            dx >= needed - 1.0 || dy >= needed - 1.0,
            "parts stayed {dx} x {dy} nm apart, need {needed} on one axis"
        );
    }

    #[test]
    fn spacing_moves_the_free_part_and_not_the_anchor() {
        let mut instance = problem(
            vec![false, true],
            vec![Some((10_000_000.0, 50_000_000.0)), None],
            vec![vec![(0, (0.0, 0.0)), (1, (0.0, 0.0))]],
        );
        instance.spacing_nm = mm_to_nm(ROUTING_GAP_MM) as f64;
        let solved = solve(&instance);
        assert_eq!(solved[0], Point::new(10_000_000, 50_000_000));
        let needed = 2.0 * 5_000_000.0 + instance.spacing_nm;
        let dx = (solved[0].x_nm - solved[1].x_nm).abs() as f64;
        let dy = (solved[0].y_nm - solved[1].y_nm).abs() as f64;
        assert!(
            dx >= needed - 1.0 || dy >= needed - 1.0,
            "free part stayed {dx} x {dy} nm from its anchor, need {needed}"
        );
    }

    #[test]
    fn already_separated_parts_do_not_move() {
        let mut instance = problem(
            vec![false, true],
            vec![Some((10_000_000.0, 50_000_000.0)), None],
            Vec::new(),
        );
        instance.spacing_nm = mm_to_nm(ROUTING_GAP_MM) as f64;
        let solved = solve(&instance);
        assert_eq!(solved[1], Point::new(50_000_000, 50_000_000));
    }

    #[test]
    fn the_solve_is_deterministic() {
        let instance = problem(
            vec![true, true],
            vec![None, None],
            vec![vec![(0, (0.0, 0.0)), (1, (0.0, 0.0))]],
        );
        assert_eq!(solve(&instance), solve(&instance));
    }
}
