// SPDX-License-Identifier: Apache-2.0

//! Full RePlAce-style global placement: weighted-average wirelength plus
//! electrostatic density, optimised with Nesterov's accelerated gradient.
//!
//! The output is a continuous [`Placement`]; the caller legalises it with the
//! displacement-minimising legalizer in `lib.rs`. This module is deliberately
//! independent of the CEM seed and of the quadratic hint path; it only borrows
//! the quadratic initial placement and the PCB pad-offset lookups.

#![allow(
    // The solve is f64 and converts to nanometres at the boundary; board
    // counts and coordinates are far below 2^53.
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // Paired x/y, w/h identifiers and a tight iterative kernel.
    clippy::similar_names,
    clippy::many_single_char_names,
    // (a + b) / 2 over board coordinates cannot overflow an f64.
    clippy::manual_midpoint
)]

use std::collections::HashMap;

use synth_geometry::{Layer, Point, Rect, Rotation, mm_to_nm, nm_to_mm};
use synth_ir::{Board, ComponentId};

use crate::analytic::initial_positions;
use crate::floorplan::FloorplanTarget;
use crate::replace_params::{DensityModel, RePlAceParams};
use crate::{ComponentPlacement, PadOffsetLookup, PinnedPlacements, Placement};

/// Continuous RePlAce output plus the frozen mask aligned with its components.
pub(crate) struct ReplacePlacement {
    pub placement: Placement,
    /// True for an anchored component.
    pub frozen: Vec<bool>,
}

struct Pin {
    component: usize,
    offset: (f64, f64),
}

#[allow(clippy::implicit_hasher, clippy::too_many_arguments)]
pub(crate) fn continuous_replace(
    board: &Board,
    outline: Rect,
    usable: Rect,
    baseline: &Placement,
    fp_targets: &HashMap<ComponentId, FloorplanTarget>,
    rotation_overrides: &HashMap<ComponentId, Rotation>,
    pinned: &PinnedPlacements,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
    pad_offsets: &PadOffsetLookup,
    params: RePlAceParams,
) -> ReplacePlacement {
    let (ids, centres, rotations, free) = initial_positions(
        board,
        usable,
        baseline,
        fp_targets,
        rotation_overrides,
        pinned,
        courtyard_lookup,
        pad_offsets,
    );
    let index: HashMap<ComponentId, usize> = ids
        .iter()
        .enumerate()
        .map(|(position, id)| (*id, position))
        .collect();
    let nets = build_nets(
        board,
        &index,
        &rotations,
        pad_offsets,
        params.max_net_degree,
    );
    let sizes: Vec<(f64, f64)> = ids
        .iter()
        .map(|id| courtyard_lookup.get(id).copied().unwrap_or((10.0, 10.0)))
        .collect();
    let frozen: Vec<bool> = free.iter().map(|is_free| !is_free).collect();
    let mut positions: Vec<(f64, f64)> = centres
        .iter()
        .map(|point| (nm_to_mm(point.x_nm), nm_to_mm(point.y_nm)))
        .collect();
    let auto_fit = board.outline.is_none();
    let edges: Vec<f64> = ids
        .iter()
        .map(|id| {
            board.component(*id).map_or(0.5, |component| {
                let inset = if auto_fit {
                    crate::auto_fit_courtyard_edge_inset_nm(component)
                } else {
                    crate::courtyard_edge_inset_nm(component)
                };
                nm_to_mm(inset)
            })
        })
        .collect();
    let bounds = Bounds {
        min_x: nm_to_mm(usable.min.x_nm),
        min_y: nm_to_mm(usable.min.y_nm),
        max_x: nm_to_mm(usable.max.x_nm),
        max_y: nm_to_mm(usable.max.y_nm),
    };
    for (position, slot) in positions.iter_mut().enumerate() {
        *slot = clamp_position(
            *slot, position, &sizes, &rotations, &edges, &bounds, &frozen,
        );
    }

    let mut current = positions.clone();
    let total_area: f64 = sizes.iter().map(|(w, h)| w * h).sum();
    let grid = DensityGrid::new(bounds, params.bin_count);
    let mut lambda_scale: Option<f64> = None;
    for stage in 0..params.stages {
        let stage_index = i32::try_from(stage).unwrap_or(0);
        let gamma =
            (params.gamma_start_mm * params.gamma_decay.powi(stage_index)).max(params.gamma_end_mm);
        let mut stage_lambda: Option<f64> = None;
        let mut t = 1.0_f64;
        let mut previous = current.clone();
        let self_forces: Vec<(f64, f64)> = if params.density_model == DensityModel::Poisson {
            (0..current.len())
                .map(|index| grid.self_force(current[index], sizes[index], total_area))
                .collect()
        } else {
            Vec::new()
        };
        for _ in 0..params.iterations_per_stage {
            let t_next = 0.5 * (1.0 + (1.0 + 4.0 * t * t).sqrt());
            let momentum = ((t - 1.0) / t_next).max(0.0);
            let mut momentum_positions = current.clone();
            for position in 0..momentum_positions.len() {
                if frozen[position] {
                    continue;
                }
                let dx = (current[position].0 - previous[position].0) * momentum;
                let dy = (current[position].1 - previous[position].1) * momentum;
                momentum_positions[position] = (current[position].0 + dx, current[position].1 + dy);
                momentum_positions[position] = clamp_position(
                    momentum_positions[position],
                    position,
                    &sizes,
                    &rotations,
                    &edges,
                    &bounds,
                    &frozen,
                );
            }
            let mut gradient = vec![(0.0_f64, 0.0_f64); momentum_positions.len()];
            let _ = wa_gradient(
                &nets,
                &momentum_positions,
                &frozen,
                gamma,
                params.net_weight,
                &mut gradient,
            );
            let density = match params.density_model {
                DensityModel::Bin => grid.bin_density_gradient(
                    &momentum_positions,
                    &sizes,
                    &frozen,
                    params.target_density,
                ),
                DensityModel::Poisson => grid.density_gradient(
                    &momentum_positions,
                    &sizes,
                    &frozen,
                    total_area,
                    &self_forces,
                ),
            };
            if lambda_scale.is_none() {
                // gpl sets the first density weight from the ratio of the two
                // gradient norms, so lambda_start is dimensionless.
                let norm = |values: &[(f64, f64)]| -> f64 {
                    values
                        .iter()
                        .zip(&frozen)
                        .filter(|(_, is_frozen)| !**is_frozen)
                        .map(|(force, _)| force.0.abs() + force.1.abs())
                        .sum()
                };
                let density_norm = norm(&density);
                lambda_scale = Some(if density_norm > 1.0e-12 {
                    norm(&gradient) / density_norm
                } else {
                    1.0
                });
            }
            let lambda = *stage_lambda.get_or_insert_with(|| {
                params.lambda_start
                    * lambda_scale.unwrap_or(1.0)
                    * params.lambda_growth.powi(stage_index)
            });
            overlap_gradient(
                &momentum_positions,
                &sizes,
                &rotations,
                &frozen,
                params.legalizer_gap_mm,
                params.overlap_weight * params.lambda_growth.powi(stage_index),
                &mut gradient,
            );
            if params.compact_weight > 0.0 {
                compact_gradient(
                    &momentum_positions,
                    &sizes,
                    &frozen,
                    params.compact_weight,
                    &mut gradient,
                );
            }
            if params.coverage_weight > 0.0 {
                coverage_gradient(
                    &momentum_positions,
                    &sizes,
                    &rotations,
                    &frozen,
                    params.coverage_weight,
                    &mut gradient,
                );
            }
            for position in 0..gradient.len() {
                if frozen[position] {
                    continue;
                }
                gradient[position].0 += lambda * density[position].0;
                gradient[position].1 += lambda * density[position].1;
            }
            let mut next = momentum_positions.clone();
            for position in 0..next.len() {
                if frozen[position] {
                    continue;
                }
                let dx = (gradient[position].0 * params.step_mm)
                    .clamp(-params.max_step_mm, params.max_step_mm);
                let dy = (gradient[position].1 * params.step_mm)
                    .clamp(-params.max_step_mm, params.max_step_mm);
                next[position] = (
                    momentum_positions[position].0 - dx,
                    momentum_positions[position].1 - dy,
                );
                next[position] = clamp_position(
                    next[position],
                    position,
                    &sizes,
                    &rotations,
                    &edges,
                    &bounds,
                    &frozen,
                );
            }
            previous = current;
            current = next;
            t = t_next;
        }
    }

    let layers: HashMap<ComponentId, Layer> = baseline
        .components
        .iter()
        .map(|placed| (placed.id, placed.layer))
        .collect();
    let components = ids
        .iter()
        .enumerate()
        .map(|(position, id)| ComponentPlacement {
            id: *id,
            center: Point::new(mm_to_nm(current[position].0), mm_to_nm(current[position].1)),
            rotation: rotations[position],
            layer: layers.get(id).copied().unwrap_or(Layer::Top),
        })
        .collect();
    ReplacePlacement {
        placement: Placement {
            board_outline: outline,
            components,
        },
        frozen,
    }
}

/// Gradient of the summed pairwise courtyard overlap area (each part grown by
/// `gap`), scaled by `weight`. Two parts at the same spot are pushed apart
/// along x by index so the result stays deterministic.
fn overlap_gradient(
    positions: &[(f64, f64)],
    sizes: &[(f64, f64)],
    rotations: &[Rotation],
    frozen: &[bool],
    gap: f64,
    weight: f64,
    gradient: &mut [(f64, f64)],
) {
    let extent = |i: usize| {
        let (w, h) = if rotations[i].swaps_extents() {
            (sizes[i].1, sizes[i].0)
        } else {
            sizes[i]
        };
        (w + gap, h + gap)
    };
    for a in 0..positions.len() {
        for b in (a + 1)..positions.len() {
            if frozen[a] && frozen[b] {
                continue;
            }
            let (wa, ha) = extent(a);
            let (wb, hb) = extent(b);
            let dx = positions[a].0 - positions[b].0;
            let dy = positions[a].1 - positions[b].1;
            let ox = (wa + wb) / 2.0 - dx.abs();
            let oy = (ha + hb) / 2.0 - dy.abs();
            if ox <= 0.0 || oy <= 0.0 {
                continue;
            }
            let sign_x = if dx.abs() < 1.0e-9 { 1.0 } else { dx.signum() };
            let sign_y = if dy.abs() < 1.0e-9 { 1.0 } else { dy.signum() };
            // d(ox*oy)/dxa = -sign(dx)*oy; descent moves a along +sign(dx).
            let (gx, gy) = (-sign_x * oy * weight, -sign_y * ox * weight);
            if !frozen[a] {
                gradient[a].0 += gx;
                gradient[a].1 += gy;
            }
            if !frozen[b] {
                gradient[b].0 -= gx;
                gradient[b].1 -= gy;
            }
        }
    }
}

/// Envelope compaction gradient. Each free part is pulled toward its nearest
/// strictly larger free part, so a decoupling passive hugs its IC instead of
/// being dragged across the board to one global centroid (the support-distance
/// gate measures exactly that cap-to-IC separation). The largest parts fall
/// back to the free-part centroid, which still closes the outer envelope.
///
/// "Larger" is the (courtyard area, index) order key, which increases along
/// every attraction edge, so the pull cannot form a cycle.
fn compact_gradient(
    positions: &[(f64, f64)],
    sizes: &[(f64, f64)],
    frozen: &[bool],
    weight: f64,
    gradient: &mut [(f64, f64)],
) {
    let mut sum = (0.0_f64, 0.0_f64);
    let mut count = 0_usize;
    for (position, is_frozen) in positions.iter().zip(frozen) {
        if *is_frozen {
            continue;
        }
        sum.0 += position.0;
        sum.1 += position.1;
        count += 1;
    }
    if count == 0 {
        return;
    }
    let mean = (sum.0 / count as f64, sum.1 / count as f64);
    for index in 0..positions.len() {
        if frozen[index] {
            continue;
        }
        let key = (sizes[index].0 * sizes[index].1, index);
        let mut nearest: Option<usize> = None;
        let mut best = f64::INFINITY;
        for other in 0..positions.len() {
            if other == index || frozen[other] {
                continue;
            }
            if (sizes[other].0 * sizes[other].1, other) <= key {
                continue;
            }
            let dx = positions[index].0 - positions[other].0;
            let dy = positions[index].1 - positions[other].1;
            let distance = dx * dx + dy * dy;
            if distance < best {
                best = distance;
                nearest = Some(other);
            }
        }
        let target = nearest.map_or(mean, |other| positions[other]);
        gradient[index].0 += weight * (positions[index].0 - target.0);
        gradient[index].1 += weight * (positions[index].1 - target.1);
    }
}

/// Coverage (dead-space) gradient. The score marks a cell dead when its centre
/// sits farther than three millimetres from every courtyard, so the empty
/// corner between the extreme parts is waste even when the fitted outline
/// already contains it. Each dead cell pulls on its nearest free parts,
/// weighted by inverse squared distance: a hole is wider than one part's
/// three-millimetre halo, so a winner-take-all pull can only ever move a
/// single part into it and leaves the rest of the hole dead.
fn coverage_gradient(
    positions: &[(f64, f64)],
    sizes: &[(f64, f64)],
    rotations: &[Rotation],
    frozen: &[bool],
    weight: f64,
    gradient: &mut [(f64, f64)],
) {
    const REACH_MM: f64 = 3.0;
    const CELL_MM: f64 = 2.0;
    let extent = |index: usize| {
        if rotations[index].swaps_extents() {
            (sizes[index].1, sizes[index].0)
        } else {
            sizes[index]
        }
    };
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (index, position) in positions.iter().enumerate() {
        let (w, h) = extent(index);
        min.0 = min.0.min(position.0 - w / 2.0);
        min.1 = min.1.min(position.1 - h / 2.0);
        max.0 = max.0.max(position.0 + w / 2.0);
        max.1 = max.1.max(position.1 + h / 2.0);
    }
    let mut pull = vec![(0.0_f64, 0.0_f64); positions.len()];
    let mut x = min.0 + CELL_MM / 2.0;
    while x < max.0 {
        let mut y = min.1 + CELL_MM / 2.0;
        while y < max.1 {
            let covered = positions.iter().enumerate().any(|(index, position)| {
                let (w, h) = extent(index);
                (x - position.0).abs() <= w / 2.0 + REACH_MM
                    && (y - position.1).abs() <= h / 2.0 + REACH_MM
            });
            if !covered {
                // The cell is wider than one part; hand its pull to the few
                // nearest free parts so a whole cluster can move into a hole.
                const NEAREST_PARTS: usize = 3;
                let mut nearest: Vec<(f64, usize)> = positions
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !frozen[*index])
                    .map(|(index, position)| {
                        ((x - position.0).powi(2) + (y - position.1).powi(2), index)
                    })
                    .collect();
                nearest.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                let nearest = &nearest[..nearest.len().min(NEAREST_PARTS)];
                let total: f64 = nearest
                    .iter()
                    .map(|(distance_sq, _)| 1.0 / distance_sq.max(1.0e-9))
                    .sum();
                for (distance_sq, index) in nearest {
                    let distance = distance_sq.sqrt().max(1.0e-9);
                    let share = (1.0 / distance_sq.max(1.0e-9)) / total;
                    pull[*index].0 += share * (x - positions[*index].0) / distance;
                    pull[*index].1 += share * (y - positions[*index].1) / distance;
                }
            }
            y += CELL_MM;
        }
        x += CELL_MM;
    }
    for (index, force) in pull.iter().enumerate() {
        if frozen[index] {
            continue;
        }
        let magnitude = force.0.hypot(force.1);
        if magnitude > 1.0e-9 {
            gradient[index].0 += weight * force.0 / magnitude;
            gradient[index].1 += weight * force.1 / magnitude;
        }
    }
}

/// Relocate small free parts into dead pockets inside the current envelope.
///
/// The score counts a one-millimetre cell dead when no courtyard comes within
/// three millimetres of it. A concave corner beside a tall part cannot be
/// filled by [`coverage_gradient`]: the pocket's nearest part is the tall part
/// itself, which cannot move into its own corner, while the passives that could
/// fill it are held on their nets. This pass moves one small part at a time and
/// only when the move covers strictly more dead cells than it uncovers, the
/// destination stays inside the current courtyard envelope (so the fitted
/// outline cannot grow) and the destination stays legal. Deterministic.
pub(crate) fn fill_dead_pockets(
    placement: &mut Placement,
    sizes: &[(f64, f64)],
    coverage: &[(f64, f64)],
    frozen: &[bool],
    gap_mm: f64,
    usable: Rect,
) {
    const CELL_MM: f64 = 1.0;
    const REACH_MM: f64 = 3.0;
    const MAX_PART_MM: f64 = 5.0;
    // Each move into a fresh pocket covers its whole courtyard+3 mm halo
    // (~40-60 cells), but a hole wider than one halo needs several parts.
    // The loop already stops when no move gains MIN_GAIN cells, so this is
    // only a safety bound, not a budget: 6 left 143 mm2 dead on sensor.
    const MAX_MOVES: usize = 20;
    const MIN_GAIN: i32 = 3;

    let count = placement.components.len();
    if count < 2 {
        return;
    }
    let swapped: Vec<bool> = placement
        .components
        .iter()
        .map(|placed| placed.rotation.swaps_extents())
        .collect();
    let mut centres: Vec<(f64, f64)> = placement
        .components
        .iter()
        .map(|placed| (nm_to_mm(placed.center.x_nm), nm_to_mm(placed.center.y_nm)))
        .collect();
    // Coverage is what the exported board actually draws. A part whose KiCad
    // footprint is not installed exports no courtyard, so the score treats the
    // space beside it as dead; using its declared size as covered hides that
    // hole from this pass forever.
    let mut half = vec![(0.0_f64, 0.0_f64); count];
    let mut extent_half = half.clone();
    let mut legal_half = half.clone();
    for index in 0..count {
        let (w, h) = sizes[index];
        let (w, h) = if swapped[index] { (h, w) } else { (w, h) };
        extent_half[index] = (w / 2.0, h / 2.0);
        let gap = if frozen[index] { 0.0 } else { gap_mm };
        legal_half[index] = ((w + gap) / 2.0, (h + gap) / 2.0);
        let (w, h) = coverage[index];
        let (w, h) = if swapped[index] { (h, w) } else { (w, h) };
        half[index] = (w / 2.0, h / 2.0);
    }
    let usable_min = (nm_to_mm(usable.min.x_nm), nm_to_mm(usable.min.y_nm));
    let usable_max = (nm_to_mm(usable.max.x_nm), nm_to_mm(usable.max.y_nm));
    let envelope = {
        let mut min = (f64::INFINITY, f64::INFINITY);
        let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for index in 0..count {
            min.0 = min.0.min(centres[index].0 - extent_half[index].0);
            min.1 = min.1.min(centres[index].1 - extent_half[index].1);
            max.0 = max.0.max(centres[index].0 + extent_half[index].0);
            max.1 = max.1.max(centres[index].1 + extent_half[index].1);
        }
        (min, max)
    };
    let mut moved = vec![false; count];
    for _ in 0..MAX_MOVES {
        let dead = dead_cells(&centres, &half, envelope, CELL_MM, REACH_MM);
        if dead.len() < MIN_GAIN as usize {
            break;
        }
        // Deepest pockets first: cells with the most dead neighbours.
        let mut ranked: Vec<(usize, usize)> = (0..dead.len())
            .map(|cell| {
                let centre = dead[cell];
                let neighbours = dead
                    .iter()
                    .filter(|other| {
                        (other.0 - centre.0).abs() <= 2.0 * REACH_MM
                            && (other.1 - centre.1).abs() <= 2.0 * REACH_MM
                    })
                    .count();
                (cell, neighbours)
            })
            .collect();
        ranked.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then(dead[left.0].1.total_cmp(&dead[right.0].1))
                .then(dead[left.0].0.total_cmp(&dead[right.0].0))
        });
        let mut best: Option<(i32, usize, (f64, f64))> = None;
        for (target, _) in ranked.iter().take(3) {
            let centre = dead[*target];
            for part in 0..count {
                // A part with no coverage exports no courtyard, so moving it
                // cannot cover a dead cell; leave it where its nets put it.
                let (w, h) = (2.0 * extent_half[part].0, 2.0 * extent_half[part].1);
                if frozen[part] || moved[part] || w.max(h) > MAX_PART_MM || half[part] == (0.0, 0.0)
                {
                    continue;
                }
                for offset_x in -3..=3 {
                    for offset_y in -3..=3 {
                        let candidate = (
                            centre.0 + f64::from(offset_x),
                            centre.1 + f64::from(offset_y),
                        );
                        let (hw, hh) = legal_half[part];
                        if candidate.0 - hw < envelope.0.0
                            || candidate.0 + hw > envelope.1.0
                            || candidate.1 - hh < envelope.0.1
                            || candidate.1 + hh > envelope.1.1
                            || candidate.0 - hw < usable_min.0
                            || candidate.0 + hw > usable_max.0
                            || candidate.1 - hh < usable_min.1
                            || candidate.1 + hh > usable_max.1
                        {
                            continue;
                        }
                        if (0..count).any(|other| {
                            other != part
                                && (candidate.0 - centres[other].0).abs() < hw + legal_half[other].0
                                && (candidate.1 - centres[other].1).abs() < hh + legal_half[other].1
                        }) {
                            continue;
                        }
                        let gain = dead_gain(
                            &centres, &half, part, candidate, envelope, CELL_MM, REACH_MM,
                        );
                        if gain >= MIN_GAIN && best.is_none_or(|(current, _, _)| gain > current) {
                            best = Some((gain, part, candidate));
                        }
                    }
                }
            }
            if best.is_some_and(|(gain, _, _)| gain >= 40) {
                break;
            }
        }
        let Some((_, part, candidate)) = best else {
            break;
        };
        centres[part] = candidate;
        moved[part] = true;
    }
    for (index, placed) in placement.components.iter_mut().enumerate() {
        placed.center = Point::new(mm_to_nm(centres[index].0), mm_to_nm(centres[index].1));
    }
}

/// Uncovered cell centres on a `cell_mm` grid inside `envelope`.
fn dead_cells(
    centres: &[(f64, f64)],
    half: &[(f64, f64)],
    envelope: ((f64, f64), (f64, f64)),
    cell_mm: f64,
    reach_mm: f64,
) -> Vec<(f64, f64)> {
    let mut dead = Vec::new();
    let mut y = envelope.0.1 + cell_mm / 2.0;
    while y < envelope.1.1 {
        let mut x = envelope.0.0 + cell_mm / 2.0;
        while x < envelope.1.0 {
            if !cell_covers(centres, half, (x, y), usize::MAX, reach_mm) {
                dead.push((x, y));
            }
            x += cell_mm;
        }
        y += cell_mm;
    }
    dead
}

fn cell_covers(
    centres: &[(f64, f64)],
    half: &[(f64, f64)],
    cell: (f64, f64),
    skip: usize,
    reach_mm: f64,
) -> bool {
    centres.iter().enumerate().any(|(index, position)| {
        index != skip
            && (cell.0 - position.0).abs() <= half[index].0 + reach_mm
            && (cell.1 - position.1).abs() <= half[index].1 + reach_mm
    })
}

/// Change in covered cells if part `part` moved from its current centre to
/// `candidate`, counting only cells on the same grid as [`dead_cells`].
fn dead_gain(
    centres: &[(f64, f64)],
    half: &[(f64, f64)],
    part: usize,
    candidate: (f64, f64),
    envelope: ((f64, f64), (f64, f64)),
    cell_mm: f64,
    reach_mm: f64,
) -> i32 {
    let old = centres[part];
    let (hw, hh) = half[part];
    let min_x = old.0.min(candidate.0) - hw - reach_mm - cell_mm;
    let max_x = old.0.max(candidate.0) + hw + reach_mm + cell_mm;
    let min_y = old.1.min(candidate.1) - hh - reach_mm - cell_mm;
    let max_y = old.1.max(candidate.1) + hh + reach_mm + cell_mm;
    let mut gain = 0_i32;
    let mut y = ((min_y - envelope.0.1) / cell_mm).floor() * cell_mm + envelope.0.1 + cell_mm / 2.0;
    while y < max_y {
        let mut x =
            ((min_x - envelope.0.0) / cell_mm).floor() * cell_mm + envelope.0.0 + cell_mm / 2.0;
        while x < max_x {
            let cell = (x, y);
            if cell.0 > envelope.0.0
                && cell.1 > envelope.0.1
                && cell.0 < envelope.1.0
                && cell.1 < envelope.1.1
                && !cell_covers(centres, half, cell, part, reach_mm)
            {
                let before = (cell.0 - old.0).abs() <= hw + reach_mm
                    && (cell.1 - old.1).abs() <= hh + reach_mm;
                let after = (cell.0 - candidate.0).abs() <= hw + reach_mm
                    && (cell.1 - candidate.1).abs() <= hh + reach_mm;
                gain += i32::from(after) - i32::from(before);
            }
            x += cell_mm;
        }
        y += cell_mm;
    }
    gain
}

fn build_nets(
    board: &Board,
    index: &HashMap<ComponentId, usize>,
    rotations: &[Rotation],
    pad_offsets: &PadOffsetLookup,
    max_degree: usize,
) -> Vec<Vec<Pin>> {
    let mut nets = Vec::new();
    for net in &board.nets {
        if net.endpoints.len() < 2 || net.endpoints.len() > max_degree {
            continue;
        }
        let pins: Vec<Pin> = net
            .endpoints
            .iter()
            .filter_map(|endpoint| {
                let &component = index.get(&endpoint.component)?;
                let (offset_x, offset_y) = pad_offsets
                    .lookup(endpoint.component, endpoint.pin.0 as usize)
                    .unwrap_or((0, 0));
                let (rotated_x, rotated_y) = rotations[component].rotate_offset(offset_x, offset_y);
                Some(Pin {
                    component,
                    offset: (nm_to_mm(rotated_x), nm_to_mm(rotated_y)),
                })
            })
            .collect();
        if pins.len() >= 2 {
            nets.push(pins);
        }
    }
    nets
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

#[allow(clippy::too_many_arguments)]
fn clamp_position(
    position: (f64, f64),
    index: usize,
    sizes: &[(f64, f64)],
    rotations: &[Rotation],
    edges: &[f64],
    bounds: &Bounds,
    frozen: &[bool],
) -> (f64, f64) {
    if frozen[index] {
        return position;
    }
    let (width, height) = sizes[index];
    let (half_w, half_h) = if rotations[index].swaps_extents() {
        (height / 2.0, width / 2.0)
    } else {
        (width / 2.0, height / 2.0)
    };
    let inset = edges[index];
    let min_x = bounds.min_x + half_w + inset;
    let max_x = bounds.max_x - half_w - inset;
    let min_y = bounds.min_y + half_h + inset;
    let max_y = bounds.max_y - half_h - inset;
    let x = if min_x > max_x {
        (min_x + max_x) / 2.0
    } else {
        position.0.clamp(min_x, max_x)
    };
    let y = if min_y > max_y {
        (min_y + max_y) / 2.0
    } else {
        position.1.clamp(min_y, max_y)
    };
    (x, y)
}

/// Weighted-average wirelength and its gradient. Returns the objective value;
/// the gradient is accumulated into `gradient`.
fn wa_gradient(
    nets: &[Vec<Pin>],
    positions: &[(f64, f64)],
    frozen: &[bool],
    gamma: f64,
    net_weight: f64,
    gradient: &mut [(f64, f64)],
) -> f64 {
    let gamma = gamma.max(1.0e-9);
    let mut objective = 0.0;
    for net in nets {
        if net.len() < 2 {
            continue;
        }
        for axis in 0..2 {
            let values: Vec<f64> = if axis == 0 {
                net.iter()
                    .map(|pin| positions[pin.component].0 + pin.offset.0)
                    .collect()
            } else {
                net.iter()
                    .map(|pin| positions[pin.component].1 + pin.offset.1)
                    .collect()
            };
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let p: Vec<f64> = values
                .iter()
                .map(|value| ((value - max) / gamma).exp())
                .collect();
            let q: Vec<f64> = values
                .iter()
                .map(|value| ((min - value) / gamma).exp())
                .collect();
            let sum_p: f64 = p.iter().sum();
            let sum_q: f64 = q.iter().sum();
            if sum_p <= 0.0 || sum_q <= 0.0 {
                continue;
            }
            let weighted_p: f64 = p.iter().zip(&values).map(|(w, value)| w * value).sum();
            let weighted_q: f64 = q.iter().zip(&values).map(|(w, value)| w * value).sum();
            let average_p = weighted_p / sum_p;
            let average_q = weighted_q / sum_q;
            objective += net_weight * (average_p - average_q);
            for (pin_index, pin) in net.iter().enumerate() {
                if frozen[pin.component] {
                    continue;
                }
                let p_i = p[pin_index] / sum_p;
                let q_i = q[pin_index] / sum_q;
                let derivative = p_i * (1.0 + (values[pin_index] - average_p) / gamma)
                    - q_i * (1.0 - (values[pin_index] - average_q) / gamma);
                if axis == 0 {
                    gradient[pin.component].0 += net_weight * derivative;
                } else {
                    gradient[pin.component].1 += net_weight * derivative;
                }
            }
        }
    }
    objective
}

/// Electrostatic density on a square bin grid. The charge is the component
/// area spread by the piecewise-quadratic kernel; the potential solves the
/// Neumann Poisson equation with a direct deterministic DCT.
struct DensityGrid {
    m: usize,
    min_x: f64,
    min_y: f64,
    bin_w: f64,
    bin_h: f64,
    omega: Vec<f64>,
    cos: Vec<Vec<f64>>,
    sin: Vec<Vec<f64>>,
}

impl DensityGrid {
    fn new(bounds: Bounds, m: usize) -> Self {
        let m = m.max(8);
        let bin_w = (bounds.max_x - bounds.min_x) / m as f64;
        let bin_h = (bounds.max_y - bounds.min_y) / m as f64;
        let omega: Vec<f64> = (0..m)
            .map(|u| std::f64::consts::TAU * u as f64 / m as f64)
            .collect();
        let cos = (0..m)
            .map(|u| (0..m).map(|l| (omega[u] * l as f64).cos()).collect())
            .collect();
        let sin = (0..m)
            .map(|u| (0..m).map(|l| (omega[u] * l as f64).sin()).collect())
            .collect();
        Self {
            m,
            min_x: bounds.min_x,
            min_y: bounds.min_y,
            bin_w: bin_w.max(1.0e-6),
            bin_h: bin_h.max(1.0e-6),
            omega,
            cos,
            sin,
        }
    }

    /// Add one component's area to the charge grid, normalised so the kernel
    /// sums to the courtyard area.
    fn add_charge(&self, charge: &mut [f64], position: (f64, f64), size: (f64, f64)) {
        let m = self.m;
        let (width, height) = size;
        let area = width * height;
        if area <= 0.0 {
            return;
        }
        let cx = (position.0 - self.min_x) / self.bin_w - 0.5;
        let cy = (position.1 - self.min_y) / self.bin_h - 0.5;
        let kernel_w = width.max(self.bin_w);
        let kernel_h = height.max(self.bin_h);
        let span_x = (kernel_w / self.bin_w).ceil() as i64 + 1;
        let span_y = (kernel_h / self.bin_h).ceil() as i64 + 1;
        let mut weights = Vec::new();
        let mut sum = 0.0;
        for dx in -span_x..=span_x {
            for dy in -span_y..=span_y {
                let l = cx.round() as i64 + dx;
                let j = cy.round() as i64 + dy;
                if l < 0 || j < 0 || l >= m as i64 || j >= m as i64 {
                    continue;
                }
                let theta_x = kernel((l as f64 + 0.5 - cx).abs() * self.bin_w, kernel_w);
                let theta_y = kernel((j as f64 + 0.5 - cy).abs() * self.bin_h, kernel_h);
                let weight = theta_x * theta_y;
                if weight > 0.0 {
                    weights.push((l as usize, j as usize, weight));
                    sum += weight;
                }
            }
        }
        if sum <= 0.0 {
            let l = (cx.round().clamp(0.0, (m - 1) as f64)) as usize;
            let j = (cy.round().clamp(0.0, (m - 1) as f64)) as usize;
            charge[l * m + j] += area;
        } else {
            for (l, j, weight) in weights {
                charge[l * m + j] += area * weight / sum;
            }
        }
    }

    fn charge(&self, positions: &[(f64, f64)], sizes: &[(f64, f64)], total_area: f64) -> Vec<f64> {
        let m = self.m;
        let mut charge = vec![0.0_f64; m * m];
        for (position, size) in positions.iter().zip(sizes) {
            self.add_charge(&mut charge, *position, *size);
        }
        let uniform = total_area / (m * m) as f64;
        for value in &mut charge {
            *value -= uniform;
        }
        charge
    }

    /// Potential gradient grids from a zero-mean charge grid.
    fn field(&self, charge: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let m = self.m;
        let mut row = vec![0.0_f64; m * m];
        for u in 0..m {
            for j in 0..m {
                let mut sum = 0.0;
                for l in 0..m {
                    sum += self.cos[u][l] * charge[l * m + j];
                }
                row[u * m + j] = sum;
            }
        }
        let mut rho_hat = vec![0.0_f64; m * m];
        for v in 0..m {
            for u in 0..m {
                let mut sum = 0.0;
                for j in 0..m {
                    sum += self.cos[v][j] * row[u * m + j];
                }
                rho_hat[u * m + v] = sum / (m * m) as f64;
            }
        }
        let mut psi_hat = vec![0.0_f64; m * m];
        for u in 0..m {
            for v in 0..m {
                if u == 0 && v == 0 {
                    continue;
                }
                psi_hat[u * m + v] = rho_hat[u * m + v]
                    / (self.omega[u] * self.omega[u] + self.omega[v] * self.omega[v]);
            }
        }
        let mut for_x = vec![0.0_f64; m * m];
        let mut for_y = vec![0.0_f64; m * m];
        for u in 0..m {
            for j in 0..m {
                let mut inner_x = 0.0;
                let mut inner_y = 0.0;
                for v in 0..m {
                    let value = psi_hat[u * m + v];
                    inner_x += value * self.cos[v][j];
                    inner_y += value * -self.omega[v] * self.sin[v][j];
                }
                for_x[u * m + j] = inner_x;
                for_y[u * m + j] = inner_y;
            }
        }
        for l in 0..m {
            for j in 0..m {
                let mut dx = 0.0;
                let mut dy = 0.0;
                for u in 0..m {
                    dx += -self.omega[u] * self.sin[u][l] * for_x[u * m + j];
                    dy += self.cos[u][l] * for_y[u * m + j];
                }
                for_x[l * m + j] = dx / self.bin_w;
                for_y[l * m + j] = dy / self.bin_h;
            }
        }
        (for_x, for_y)
    }

    /// The force a component's own charge produces at its centre. It is
    /// subtracted from the total field so a symmetric kernel does not move
    /// itself (the discrete DCT leaves a small self-force).
    fn self_force(&self, position: (f64, f64), size: (f64, f64), total_area: f64) -> (f64, f64) {
        if total_area <= 0.0 {
            return (0.0, 0.0);
        }
        let m = self.m;
        let mut charge = vec![0.0_f64; m * m];
        self.add_charge(&mut charge, position, size);
        let (dx, dy) = self.field(&charge);
        let (gx, gy) = self.sample(&dx, &dy, position);
        let area = size.0 * size.1;
        (area * gx / total_area, area * gy / total_area)
    }

    fn density_gradient(
        &self,
        positions: &[(f64, f64)],
        sizes: &[(f64, f64)],
        frozen: &[bool],
        total_area: f64,
        self_forces: &[(f64, f64)],
    ) -> Vec<(f64, f64)> {
        let mut gradient = vec![(0.0_f64, 0.0_f64); positions.len()];
        if total_area <= 0.0 {
            return gradient;
        }
        let charge = self.charge(positions, sizes, total_area);
        let (dx, dy) = self.field(&charge);
        for index in 0..positions.len() {
            if frozen[index] {
                continue;
            }
            let (gx, gy) = self.sample(&dx, &dy, positions[index]);
            let area = sizes[index].0 * sizes[index].1;
            gradient[index] = (
                area * gx / total_area - self_forces[index].0,
                area * gy / total_area - self_forces[index].1,
            );
        }
        // The Neumann field must have no net force; a residual global drift
        // from the discrete solve is removed so it cannot translate the board.
        let free_count = frozen.iter().filter(|&&is_frozen| !is_frozen).count();
        if free_count > 0 {
            let mean_x: f64 = gradient
                .iter()
                .zip(frozen)
                .filter(|(_, is_frozen)| !**is_frozen)
                .map(|(force, _)| force.0)
                .sum::<f64>()
                / free_count as f64;
            let mean_y: f64 = gradient
                .iter()
                .zip(frozen)
                .filter(|(_, is_frozen)| !**is_frozen)
                .map(|(force, _)| force.1)
                .sum::<f64>()
                / free_count as f64;
            for (force, is_frozen) in gradient.iter_mut().zip(frozen) {
                if !*is_frozen {
                    force.0 -= mean_x;
                    force.1 -= mean_y;
                }
            }
        }
        // The DCT potential is in charge units; normalise the free-force
        // magnitude so lambda is a dimensionless balance against WA.
        let max_force = gradient
            .iter()
            .zip(frozen)
            .filter(|(_, is_frozen)| !**is_frozen)
            .map(|(force, _)| force.0.abs().max(force.1.abs()))
            .fold(0.0_f64, f64::max);
        if max_force > 1.0e-12 {
            for (force, is_frozen) in gradient.iter_mut().zip(frozen) {
                if !*is_frozen {
                    force.0 /= max_force;
                    force.1 /= max_force;
                }
            }
        }
        gradient
    }

    /// Bin-overlap density gradient (research note Option A). No self-force:
    /// the kernel's derivative is zero at its own centre, and the penalty is
    /// a local function of the bin coverage.
    fn bin_density_gradient(
        &self,
        positions: &[(f64, f64)],
        sizes: &[(f64, f64)],
        frozen: &[bool],
        target_density: f64,
    ) -> Vec<(f64, f64)> {
        let m = self.m;
        let target = target_density * self.bin_w * self.bin_h;
        let mut coverage = vec![0.0_f64; m * m];
        for (position, size) in positions.iter().zip(sizes) {
            let (support, norm) = self.kernel_support(*position, *size);
            if norm <= 0.0 {
                continue;
            }
            let area = size.0 * size.1;
            for (l, j, theta_x, theta_y) in support {
                coverage[l * m + j] += area * theta_x * theta_y / norm;
            }
        }
        let mut gradient = vec![(0.0_f64, 0.0_f64); positions.len()];
        for index in 0..positions.len() {
            if frozen[index] {
                continue;
            }
            let (support, norm) = self.kernel_support(positions[index], sizes[index]);
            if norm <= 0.0 {
                continue;
            }
            let area = sizes[index].0 * sizes[index].1;
            let scale = area / norm;
            let kernel_w = sizes[index].0.max(self.bin_w);
            let kernel_h = sizes[index].1.max(self.bin_h);
            let mut force_x = 0.0;
            let mut force_y = 0.0;
            for (l, j, theta_x, theta_y) in support {
                let center_x = self.min_x + (l as f64 + 0.5) * self.bin_w;
                let center_y = self.min_y + (j as f64 + 0.5) * self.bin_h;
                let dx = positions[index].0 - center_x;
                let dy = positions[index].1 - center_y;
                let derivative_x = kernel_derivative(dx.abs(), kernel_w) * dx.signum();
                let derivative_y = kernel_derivative(dy.abs(), kernel_h) * dy.signum();
                let excess = (coverage[l * m + j] - target).max(0.0);
                force_x += -2.0 * excess * scale * derivative_x * theta_y;
                force_y += -2.0 * excess * scale * theta_x * derivative_y;
            }
            gradient[index] = (force_x, force_y);
        }
        let max_force = gradient
            .iter()
            .zip(frozen)
            .filter(|(_, is_frozen)| !**is_frozen)
            .map(|(force, _)| force.0.abs().max(force.1.abs()))
            .fold(0.0_f64, f64::max);
        if max_force > 1.0e-12 {
            for (force, is_frozen) in gradient.iter_mut().zip(frozen) {
                if !*is_frozen {
                    force.0 /= max_force;
                    force.1 /= max_force;
                }
            }
        }
        gradient
    }

    fn kernel_support(
        &self,
        position: (f64, f64),
        size: (f64, f64),
    ) -> (Vec<(usize, usize, f64, f64)>, f64) {
        let m = self.m;
        let (width, height) = size;
        let cx = (position.0 - self.min_x) / self.bin_w - 0.5;
        let cy = (position.1 - self.min_y) / self.bin_h - 0.5;
        let kernel_w = width.max(self.bin_w);
        let kernel_h = height.max(self.bin_h);
        let span_x = (kernel_w / self.bin_w).ceil() as i64 + 1;
        let span_y = (kernel_h / self.bin_h).ceil() as i64 + 1;
        let mut support = Vec::new();
        let mut norm = 0.0;
        for dx in -span_x..=span_x {
            for dy in -span_y..=span_y {
                let l = cx.round() as i64 + dx;
                let j = cy.round() as i64 + dy;
                if l < 0 || j < 0 || l >= m as i64 || j >= m as i64 {
                    continue;
                }
                let theta_x = kernel((l as f64 + 0.5 - cx).abs() * self.bin_w, kernel_w);
                let theta_y = kernel((j as f64 + 0.5 - cy).abs() * self.bin_h, kernel_h);
                let weight = theta_x * theta_y;
                if weight > 0.0 {
                    support.push((l as usize, j as usize, theta_x, theta_y));
                    norm += weight;
                }
            }
        }
        (support, norm)
    }

    fn sample(&self, grid_x: &[f64], grid_y: &[f64], position: (f64, f64)) -> (f64, f64) {
        let m = self.m;
        let x = (position.0 - self.min_x) / self.bin_w - 0.5;
        let y = (position.1 - self.min_y) / self.bin_h - 0.5;
        let x = x.clamp(0.0, (m - 1) as f64);
        let y = y.clamp(0.0, (m - 1) as f64);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(m - 1);
        let y1 = (y0 + 1).min(m - 1);
        let tx = x - x0 as f64;
        let ty = y - y0 as f64;
        let lerp = |grid: &[f64]| {
            let a = grid[x0 * m + y0] * (1.0 - tx) + grid[x1 * m + y0] * tx;
            let b = grid[x0 * m + y1] * (1.0 - tx) + grid[x1 * m + y1] * tx;
            a * (1.0 - ty) + b * ty
        };
        (lerp(grid_x), lerp(grid_y))
    }
}

fn kernel_derivative(distance: f64, width: f64) -> f64 {
    let width = width.max(1.0e-9);
    if distance <= width / 2.0 {
        -4.0 * distance / (width * width)
    } else if distance <= width {
        4.0 * (distance - width) / (width * width)
    } else {
        0.0
    }
}

fn kernel(distance: f64, width: f64) -> f64 {
    let width = width.max(1.0e-9);
    if distance <= width / 2.0 {
        1.0 - 2.0 * distance * distance / (width * width)
    } else if distance <= width {
        2.0 * (distance - width) * (distance - width) / (width * width)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_pin_net() -> Vec<Pin> {
        vec![
            Pin {
                component: 0,
                offset: (0.0, 0.0),
            },
            Pin {
                component: 1,
                offset: (0.0, 0.0),
            },
        ]
    }

    #[test]
    fn wa_gradient_matches_finite_difference() {
        let nets = vec![two_pin_net()];
        let positions = [(10.0, 10.0), (20.0, 15.0)];
        let frozen = [false, false];
        let mut gradient = vec![(0.0, 0.0); 2];
        let baseline = wa_gradient(&nets, &positions, &frozen, 5.0, 1.0, &mut gradient);
        let step = 1.0e-4;
        for index in 0..2 {
            for axis in 0..2 {
                let mut moved = positions;
                if axis == 0 {
                    moved[index].0 += step;
                } else {
                    moved[index].1 += step;
                }
                let mut scratch = vec![(0.0, 0.0); 2];
                let value = wa_gradient(&nets, &moved, &frozen, 5.0, 1.0, &mut scratch);
                let finite = (value - baseline) / step;
                let analytic = if axis == 0 {
                    gradient[index].0
                } else {
                    gradient[index].1
                };
                assert!(
                    (finite - analytic).abs() < 1.0e-3,
                    "component {index} axis {axis}: finite {finite}, analytic {analytic}"
                );
            }
        }
    }

    #[test]
    fn compact_pulls_small_parts_toward_a_larger_neighbour() {
        let positions = [(0.0, 0.0), (10.0, 0.0)];
        let sizes = [(2.0, 2.0), (2.0, 2.0)];
        let frozen = [false, false];
        let mut gradient = vec![(0.0, 0.0); 2];
        compact_gradient(&positions, &sizes, &frozen, 0.05, &mut gradient);
        // Equal courtyards: the higher index counts as larger, so part 1 is
        // drawn toward part 0; the largest falls back to the centroid.
        assert!(
            gradient[1].0 > 0.0,
            "part 1 must be pulled toward part 0, got {gradient:?}"
        );
        assert!(
            gradient[0].0 < 0.0,
            "the centroid fallback must pull the largest part inward, got {gradient:?}"
        );
    }

    #[test]
    fn coverage_pulls_parts_toward_an_empty_side() {
        // Two 5 mm parts 40 mm apart leave a long dead strip between them; each
        // must be pulled into that strip, from its own side.
        let positions = [(0.0, 0.0), (40.0, 0.0)];
        let sizes = [(5.0, 5.0), (5.0, 5.0)];
        let rotations = [Rotation::Zero, Rotation::Zero];
        let frozen = [false, false];
        let mut gradient = vec![(0.0, 0.0); 2];
        coverage_gradient(&positions, &sizes, &rotations, &frozen, 0.5, &mut gradient);
        assert!(
            gradient[0].0 > 0.0,
            "left part must move right, got {gradient:?}"
        );
        assert!(
            gradient[1].0 < 0.0,
            "right part must move left, got {gradient:?}"
        );
    }

    #[test]
    fn coverage_distributes_a_wide_hole_over_its_neighbours() {
        // A 35 mm dead strip between the middle and right part: the far-left
        // part is never the nearest one, but it must still feel the hole so a
        // cluster, not a single part, drifts into it.
        let positions = [(0.0, 0.0), (10.0, 0.0), (55.0, 0.0)];
        let sizes = [(5.0, 5.0); 3];
        let rotations = [Rotation::Zero; 3];
        let frozen = [false, false, false];
        let mut gradient = vec![(0.0, 0.0); 3];
        coverage_gradient(&positions, &sizes, &rotations, &frozen, 0.5, &mut gradient);
        assert!(
            gradient[0].0 > 0.0 && gradient[1].0 > 0.0 && gradient[2].0 < 0.0,
            "every part must be pulled into the strip, got {gradient:?}"
        );
    }

    #[test]
    fn coverage_is_silent_when_the_board_is_full() {
        // A 6 mm gap keeps every cell within the 3 mm reach of one of the parts.
        let positions = [(0.0, 0.0), (11.0, 0.0)];
        let sizes = [(5.0, 5.0), (5.0, 5.0)];
        let rotations = [Rotation::Zero, Rotation::Zero];
        let frozen = [false, false];
        let mut gradient = vec![(0.0, 0.0); 2];
        coverage_gradient(&positions, &sizes, &rotations, &frozen, 0.5, &mut gradient);
        assert_eq!(gradient, vec![(0.0, 0.0); 2]);
    }

    #[test]
    fn density_has_no_self_force() {
        let bounds = Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 20.0,
            max_y: 20.0,
        };
        let grid = DensityGrid::new(bounds, 32);
        let positions = [(10.0, 10.0)];
        let sizes = [(2.0, 2.0)];
        let frozen = [false];
        let self_forces = vec![grid.self_force(positions[0], sizes[0], 4.0)];
        let gradient = grid.density_gradient(&positions, &sizes, &frozen, 4.0, &self_forces);
        assert!(
            gradient[0].0.abs() < 1.0e-6 && gradient[0].1.abs() < 1.0e-6,
            "single part must have no self-force, got {:?}",
            gradient[0]
        );
    }

    #[test]
    fn density_pushes_two_parts_apart() {
        let bounds = Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 20.0,
            max_y: 20.0,
        };
        let grid = DensityGrid::new(bounds, 32);
        let positions = [(9.0, 10.0), (11.0, 10.0)];
        let sizes = [(2.0, 2.0), (2.0, 2.0)];
        let frozen = [false, false];
        let total_area = 8.0;
        let self_forces: Vec<(f64, f64)> = positions
            .iter()
            .zip(&sizes)
            .map(|(position, size)| grid.self_force(*position, *size, total_area))
            .collect();
        let gradient = grid.density_gradient(&positions, &sizes, &frozen, total_area, &self_forces);
        assert!(
            gradient[0].0 < 0.0 && gradient[1].0 > 0.0,
            "parts must be pushed apart, got {gradient:?}"
        );
    }

    #[test]
    fn dead_pocket_fill_moves_a_passive_into_a_concave_corner() {
        let placed = |id: u32, x: f64, y: f64| ComponentPlacement {
            id: ComponentId(id),
            center: Point::new(mm_to_nm(x), mm_to_nm(y)),
            rotation: Rotation::Zero,
            layer: Layer::Top,
        };
        // A bar and a post form an L; beyond the bar's end the cells sit more
        // than three millimetres from every courtyard.
        let mut placement = Placement {
            board_outline: Rect::new(Point::new(0, 0), Point::new(mm_to_nm(40.0), mm_to_nm(40.0))),
            components: vec![
                placed(0, 20.0, 5.0),
                placed(1, 12.0, 20.0),
                placed(2, 19.0, 5.5),
            ],
        };
        let sizes = [(20.0, 2.0), (4.0, 20.0), (2.0, 1.0)];
        let frozen = [false, false, false];
        let usable = Rect::new(Point::new(0, 0), Point::new(mm_to_nm(40.0), mm_to_nm(40.0)));
        fill_dead_pockets(&mut placement, &sizes, &sizes, &frozen, 0.5, usable);
        let moved = placement.components[2].center;
        // The pocket is the L's interior: clear of the post (x > 14) and below
        // the bar (y > 6). The pass picks the nearest dead cell that gains the
        // most coverage, not one fixed coordinate.
        assert!(
            nm_to_mm(moved.x_nm) > 14.0 && nm_to_mm(moved.y_nm) > 6.0,
            "passive should fill the pocket, got ({:.1}, {:.1})",
            nm_to_mm(moved.x_nm),
            nm_to_mm(moved.y_nm)
        );
    }
}
