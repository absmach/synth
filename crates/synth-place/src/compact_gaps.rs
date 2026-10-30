// SPDX-License-Identifier: Apache-2.0

//! Closes the gaps in a finished layout to a target spacing.
//!
//! Scaling a layout inward shrinks every gap by the same share, so a lane that
//! is 40 mm wide stays wide while a pair of capacitors 0.3 mm apart is pushed
//! into each other. This pass works on the gaps themselves: each part slides
//! toward the left, then toward the top, until the next part or wall in that
//! direction is one `gap` away, and no gap that is already tighter than the
//! target is opened. What is left is a layout in which no part has more than
//! about `gap` of empty space in front of it, however the lanes ran before.
//!
//! The order of parts along each axis is kept, so the circuits stay beside the
//! parts they were placed beside. Hard-hinted parts stay where they are.
//! Connectors keep to their edge: the rows on the left and top edges are walls,
//! and the rows on the right and bottom edges are set against the compacted
//! parts afterwards, so every connector is still the extreme part on its side.
//! Mounting holes follow the new outline's corners.

use super::{
    courtyard_envelope, courtyard_rect_for_placement, courtyard_sizes, fallback_courtyard,
    fit_outline_with_margin_mm, flush_edge_index, is_dense_part, is_hard_hinted,
    tight_edge_margin_mm, total_hpwl_nm, trim_outline_to_grid, Board, ComponentPlacement,
    Placement, Point, Rect,
};
use synth_geometry::{mm_to_nm, nm_to_mm};

/// The spacing a compaction closes the layout to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapSpacing {
    /// Clear space between two courtyards, in millimetres.
    pub gap_mm: f64,
    /// The same next to a part with many pins, which needs room for its traces
    /// to leave.
    pub dense_gap_mm: f64,
}

/// A part's courtyard size. The placement point is the courtyard's centre (the
/// exporter works the footprint origin back from it), so the size is all a
/// part needs.
#[derive(Debug, Clone, Copy)]
struct Geom {
    size: (f64, f64),
}

/// The courtyard of the stock M2.5 mounting-hole footprint is a circle of
/// 5.5 mm; the footprint loader reads a circle as no courtyard and falls back
/// to the 2.5 mm hole, which would let a part sit inside the hole's keep-out.
const MOUNTING_HOLE_COURTYARD_MM: f64 = 5.6;

fn geometry(board: &Board, placement: &Placement) -> Vec<Geom> {
    placement
        .components
        .iter()
        .map(|placed| {
            let component = board.component(placed.id);
            let (w, h) = component.map_or((0.0, 0.0), |c| {
                c.part.as_ref().map_or_else(
                    || fallback_courtyard(&c.kind),
                    |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
                )
            });
            if component.is_some_and(|c| c.kind.as_str() == "mounting_hole") {
                Geom {
                    size: (
                        w.max(MOUNTING_HOLE_COURTYARD_MM),
                        h.max(MOUNTING_HOLE_COURTYARD_MM),
                    ),
                }
            } else {
                Geom { size: (w, h) }
            }
        })
        .collect()
}

fn geom_rect(placed: &ComponentPlacement, geom: &Geom) -> Rect {
    courtyard_rect_for_placement(placed, geom.size.0, geom.size.1, None, 0)
}

/// How far apart, across the axis, two parts' extents may be and still be
/// taken to face each other. The slide must not pass a part that merely clears
/// another by less than a courtyard's rounding.
const FACING_TOLERANCE_MM: f64 = 1.0;

/// What a part may do in a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Slides.
    Free,
    /// Hard-hinted: stays.
    Fixed,
    /// A mounting hole: follows the outline's corners at the end.
    Hole,
    /// A connector flush with the edge of that index (left 0, top 1, right 2,
    /// bottom 3).
    Edge(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn low(self, r: &Rect) -> i64 {
        match self {
            Axis::X => r.min.x_nm,
            Axis::Y => r.min.y_nm,
        }
    }

    fn high(self, r: &Rect) -> i64 {
        match self {
            Axis::X => r.max.x_nm,
            Axis::Y => r.max.y_nm,
        }
    }

    /// The other axis' extents overlap, so the two parts face each other along
    /// this one.
    fn faces(self, a: &Rect, b: &Rect) -> bool {
        let other = match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        };
        let tol = mm_to_nm(FACING_TOLERANCE_MM);
        other.low(a) - tol < other.high(b) && other.low(b) - tol < other.high(a)
    }

    /// The edge index of the connector row that walls this axis off at its low
    /// side (left for x, top for y) and the one set against the parts at its
    /// high side (right for x, bottom for y).
    fn edges(self) -> (usize, usize) {
        match self {
            Axis::X => (0, 2),
            Axis::Y => (1, 3),
        }
    }
}

fn shifted(placement: &mut Placement, index: usize, axis: Axis, delta_nm: i64) {
    let centre = &mut placement.components[index].center;
    match axis {
        Axis::X => centre.x_nm += delta_nm,
        Axis::Y => centre.y_nm += delta_nm,
    }
}

/// Which way a slide goes along its axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toward {
    Low,
    High,
}

/// What ties parts together: they slide as one unit, so a circuit keeps its
/// shape instead of scattering across the board.
struct Ties {
    /// Pairs the circuit recognition binds (a decoupling capacitor to its IC,
    /// a load capacitor to its crystal).
    bound: Vec<(usize, usize)>,
    /// For each net that is not a rail and joins few parts, the parts on it.
    small_nets: Vec<Vec<usize>>,
}

/// A net with more parts than this is a rail or a bus, not a local connection.
const SMALL_NET_PARTS: usize = 4;

fn ties(board: &Board, placement: &Placement) -> Ties {
    let index: std::collections::HashMap<synth_ir::ComponentId, usize> = placement
        .components
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();
    let mut bound = Vec::new();
    for cluster in synth_ir::recognize_clusters(board) {
        let Some(&anchor) = index.get(&cluster.anchor) else {
            continue;
        };
        for member in &cluster.members {
            if let Some(&m) = index.get(&member.component) {
                bound.push((anchor, m));
            }
        }
    }
    let domains = synth_ir::infer_power_domains(board);
    let small_nets = board
        .nets
        .iter()
        .filter(|net| {
            !domains
                .get(net.id)
                .is_some_and(|d| d.is_rail() || d.is_ground())
                && !named_for_ground(&net.name)
        })
        .filter_map(|net| {
            let mut parts: Vec<usize> = net
                .endpoints
                .iter()
                .filter_map(|e| index.get(&e.component).copied())
                .collect();
            parts.sort_unstable();
            parts.dedup();
            (2..=SMALL_NET_PARTS)
                .contains(&parts.len())
                .then_some(parts)
        })
        .collect();
    Ties { bound, small_nets }
}

/// A net named for ground (`gnd`, `vss`, `0v` in any case), the rule the DRC and
/// the exporter share.
fn named_for_ground(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("gnd") || name.contains("vss") || name == "0v"
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The unit each part belongs to: parts the circuit recognition binds, and
/// parts that touch (within `reach_nm`) and share a small net.
fn units(rects: &[Rect], ties: &Ties, reach_nm: i64) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..rects.len()).collect();
    let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    };
    for &(a, b) in &ties.bound {
        join(&mut parent, a, b);
    }
    let near = |a: &Rect, b: &Rect| {
        let dx = (a.min.x_nm - b.max.x_nm)
            .max(b.min.x_nm - a.max.x_nm)
            .max(0);
        let dy = (a.min.y_nm - b.max.y_nm)
            .max(b.min.y_nm - a.max.y_nm)
            .max(0);
        dx <= reach_nm && dy <= reach_nm
    };
    for net in &ties.small_nets {
        for (i, &a) in net.iter().enumerate() {
            for &b in &net[i + 1..] {
                if near(&rects[a], &rects[b]) {
                    join(&mut parent, a, b);
                }
            }
        }
    }
    (0..rects.len()).map(|i| find(&mut parent, i)).collect()
}

/// One pass along `axis`, every free unit sliding toward `toward` until the next
/// part or wall is one gap away.
fn slide_pass(
    placement: &mut Placement,
    geom: &[Geom],
    roles: &[Role],
    dense: &[bool],
    spacing: GapSpacing,
    axis: Axis,
    toward: Toward,
    ties: &Ties,
    rim_mm: f64,
) -> bool {
    let pitch_nm = mm_to_nm(0.5);
    let original: Vec<Rect> = placement
        .components
        .iter()
        .zip(geom)
        .map(|(p, g)| geom_rect(p, g))
        .collect();
    // Coordinates seen so that every slide goes toward smaller values.
    let low_v = |r: &Rect| match toward {
        Toward::Low => axis.low(r),
        Toward::High => -axis.high(r),
    };
    let high_v = |r: &Rect| match toward {
        Toward::Low => axis.high(r),
        Toward::High => -axis.low(r),
    };
    let (low_edge, high_edge) = axis.edges();
    let movable = |i: usize| match roles[i] {
        Role::Free => true,
        // A row at an end of this axis comes inward, and only inward; a row
        // along it slides like any part.
        Role::Edge(edge) if edge == low_edge => toward == Toward::High,
        Role::Edge(edge) if edge == high_edge => toward == Toward::Low,
        Role::Edge(_) => true,
        Role::Fixed | Role::Hole => false,
    };
    let wall = original
        .iter()
        .enumerate()
        .filter(|(i, _)| roles[*i] != Role::Hole)
        .map(|(_, r)| low_v(r))
        .min()
        .unwrap_or(0);
    // The outline is fitted a rim away from every part that is not a flush
    // connector, so a part may not slide to the line the connectors' faces make:
    // it would push the outline out by the rim on that side. Where the connectors
    // set the extreme, ordinary parts stop a rim short of it.
    let connector_wall = original
        .iter()
        .enumerate()
        .filter(|(i, _)| matches!(roles[*i], Role::Edge(_)))
        .map(|(_, r)| low_v(r))
        .min();
    let free_wall = match connector_wall {
        Some(face) if face <= wall => face + mm_to_nm(rim_mm),
        _ => wall,
    };
    let reach = mm_to_nm(spacing.gap_mm.max(2.0));
    let mut unit_of = units(&original, ties, reach);
    // A connector row moves inward as one body, so its connectors stay flush
    // with each other and with whatever is tied to them.
    for edge in [low_edge, high_edge] {
        let row: Vec<usize> = (0..roles.len())
            .filter(|&i| roles[i] == Role::Edge(edge))
            .collect();
        if let Some((&first, rest)) = row.split_first() {
            let target = unit_of[first];
            for &i in rest {
                let from = unit_of[i];
                for u in &mut unit_of {
                    if *u == from {
                        *u = target;
                    }
                }
            }
        }
    }
    let mut members: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, u) in unit_of.iter().enumerate() {
        members.entry(*u).or_default().push(i);
    }
    let mut order: Vec<(i64, usize)> = members
        .iter()
        .filter(|(_, m)| m.iter().all(|&i| movable(i)))
        .map(|(u, m)| {
            (
                m.iter().map(|&i| low_v(&original[i])).min().unwrap_or(0),
                *u,
            )
        })
        .collect();
    order.sort_unstable();
    let mut current = original.clone();
    let mut moved = false;
    for (_, unit) in order {
        let group = &members[&unit];
        let mut room = i64::MAX;
        let carries_row = group.iter().any(|&p| matches!(roles[p], Role::Edge(_)));
        for &p in group {
            let mut bound = if carries_row { wall } else { free_wall };
            for q in 0..original.len() {
                if unit_of[q] == unit
                    || high_v(&original[q]) > low_v(&original[p])
                    || !axis.faces(&original[q], &original[p])
                {
                    continue;
                }
                let want = if dense[p] || dense[q] {
                    spacing.dense_gap_mm
                } else {
                    spacing.gap_mm
                };
                let had = low_v(&original[p]) - high_v(&original[q]);
                let between = mm_to_nm(want).min(had);
                let behind = match toward {
                    Toward::Low => axis.high(&current[q]),
                    Toward::High => -axis.low(&current[q]),
                };
                bound = bound.max(behind + between);
            }
            room = room.min(low_v(&original[p]) - bound);
        }
        let slide = room.max(0) / pitch_nm * pitch_nm;
        let sign = match toward {
            Toward::Low => -1,
            Toward::High => 1,
        };
        if slide > 0 {
            let delta = sign * slide;
            for &p in group {
                shifted(placement, p, axis, delta);
                match axis {
                    Axis::X => {
                        current[p].min.x_nm += delta;
                        current[p].max.x_nm += delta;
                    }
                    Axis::Y => {
                        current[p].min.y_nm += delta;
                        current[p].max.y_nm += delta;
                    }
                }
            }
            moved = true;
        }
    }
    moved
}

/// Closes the gaps of `placement` to `spacing` and fits the outline to the
/// result. `None` when the layout has no courtyards.
#[must_use]
pub fn compact_by_gaps(
    board: &Board,
    placement: &Placement,
    spacing: GapSpacing,
) -> Option<Placement> {
    let mut candidate = placement.clone();
    let sizes = courtyard_sizes(board, &candidate);
    courtyard_envelope(&candidate, &sizes)?;
    let geom = geometry(board, &candidate);
    let roles: Vec<Role> = candidate
        .components
        .iter()
        .map(|p| {
            let component = board.component(p.id);
            if let Some(edge) = flush_edge_index(board, p.id) {
                Role::Edge(edge)
            } else if component.is_some_and(|c| c.kind.as_str() == "mounting_hole") {
                Role::Hole
            } else if component.is_some_and(is_hard_hinted) {
                Role::Fixed
            } else {
                Role::Free
            }
        })
        .collect();
    let dense: Vec<bool> = candidate
        .components
        .iter()
        .map(|p| is_dense_part(board, p.id))
        .collect();
    let original_outline = candidate.board_outline;
    let edge_gaps = edge_gaps_nm(&candidate, &geom, &roles, original_outline);
    let hole_offsets = hole_corner_offsets(&candidate, &roles, original_outline);
    let parts_before = parts_envelope(&candidate, &geom, &roles)?;

    let ties = ties(board, &candidate);
    let rim_mm = tight_edge_margin_mm(board);
    // Fill the space toward the right and the bottom first, then close it back
    // against the top-left walls, where the connector rows on the left and the
    // top are flush; the other two rows are seated afterwards.
    let schedule = [
        (Axis::X, Toward::High),
        (Axis::Y, Toward::High),
        (Axis::X, Toward::Low),
        (Axis::Y, Toward::Low),
    ];
    for round in 0..6 {
        let mut moved = false;
        for (axis, toward) in schedule {
            if round >= 1 && toward == Toward::High {
                continue;
            }
            moved |= slide_pass(
                &mut candidate,
                &geom,
                &roles,
                &dense,
                spacing,
                axis,
                toward,
                &ties,
                rim_mm,
            );
        }
        if round >= 1 && !moved {
            break;
        }
    }
    seat_holes(
        board,
        &mut candidate,
        &geom,
        &roles,
        &hole_offsets,
        parts_before,
    );
    fit_outline_with_margin_mm(board, &mut candidate, tight_edge_margin_mm(board));
    trim_outline_to_grid(board, &mut candidate);
    snap_edge_connectors(&mut candidate, &geom, &roles, &edge_gaps);
    Some(candidate)
}

/// How far each flush connector's face sat from the edge it is flush with.
fn edge_gaps_nm(placement: &Placement, geom: &[Geom], roles: &[Role], outline: Rect) -> Vec<i64> {
    placement
        .components
        .iter()
        .zip(geom)
        .zip(roles)
        .map(|((p, g), role)| match role {
            Role::Edge(edge) => edge_gap_nm(&geom_rect(p, g), outline, *edge),
            _ => 0,
        })
        .collect()
}

/// The distance from the face of `rect` to the outline edge of that index.
fn edge_gap_nm(rect: &Rect, outline: Rect, edge: usize) -> i64 {
    match edge {
        0 => rect.min.x_nm - outline.min.x_nm,
        1 => rect.min.y_nm - outline.min.y_nm,
        2 => outline.max.x_nm - rect.max.x_nm,
        _ => outline.max.y_nm - rect.max.y_nm,
    }
}

/// A part that holds a connector edge of the outline away from its connectors.
#[derive(Debug, Clone)]
pub(crate) struct HoldingOutline {
    pub id: crate::ComponentId,
    /// The edge index: left 0, top 1, right 2, bottom 3.
    pub edge: usize,
    /// How far the edge's flush connectors sit inside it because of this part.
    pub connector_gap_mm: f64,
    /// How far inward the part has to move for the outline to close up.
    pub move_in_mm: f64,
}

/// A connector that is flush with an edge has to sit on it, but the outline is
/// fitted a rim away from every other part, so a passive standing within that
/// rim of the connectors' edge pushes the outline out and leaves the
/// connectors short of it. Lists those parts, with the inward move that lets
/// the edge return to the connectors.
pub(crate) fn parts_holding_outline(board: &Board, placement: &Placement) -> Vec<HoldingOutline> {
    let mut fitted = placement.clone();
    let rim_mm = tight_edge_margin_mm(board);
    fit_outline_with_margin_mm(board, &mut fitted, rim_mm);
    let outline = fitted.board_outline;
    let geom = geometry(board, &fitted);
    let rects: Vec<Rect> = fitted
        .components
        .iter()
        .zip(&geom)
        .map(|(p, g)| geom_rect(p, g))
        .collect();
    let flush: Vec<Option<usize>> = fitted
        .components
        .iter()
        .map(|p| flush_edge_index(board, p.id))
        .collect();
    let mut found = Vec::new();
    for edge in 0..4 {
        let Some(connector_gap) = (0..rects.len())
            .filter(|&i| flush[i] == Some(edge))
            .map(|i| edge_gap_nm(&rects[i], outline, edge))
            .min()
        else {
            continue;
        };
        let held_mm = nm_to_mm(connector_gap) - SNAP_GAP_MM;
        if held_mm < MIN_HELD_MM {
            continue;
        }
        for (j, part) in fitted.components.iter().enumerate() {
            let is_hole = board
                .component(part.id)
                .is_some_and(|c| c.kind.as_str() == "mounting_hole");
            if flush[j].is_some() || is_hole {
                continue;
            }
            let reach = nm_to_mm(edge_gap_nm(&rects[j], outline, edge));
            if reach <= rim_mm + RIM_SLACK_MM {
                found.push(HoldingOutline {
                    id: part.id,
                    edge,
                    connector_gap_mm: nm_to_mm(connector_gap),
                    move_in_mm: (held_mm * 10.0).ceil() / 10.0,
                });
            }
        }
    }
    found
}

/// The gap a flush connector keeps to its edge when nothing holds it away.
const SNAP_GAP_MM: f64 = 0.25;

/// A connector edge counts as held off when its connectors sit this far further in.
const MIN_HELD_MM: f64 = 0.3;

/// A part this near the rim distance from the edge is the one that sets it.
const RIM_SLACK_MM: f64 = 0.1;

/// Fitting the outline round the corner holes can leave it further out than
/// a connector row, so each flush connector returns to the gap it had to its
/// edge, unless another part is in the way.
fn snap_edge_connectors(placement: &mut Placement, geom: &[Geom], roles: &[Role], gaps: &[i64]) {
    let outline = placement.board_outline;
    for i in 0..placement.components.len() {
        let Role::Edge(edge) = roles[i] else { continue };
        let rect = geom_rect(&placement.components[i], &geom[i]);
        let excess = edge_gap_nm(&rect, outline, edge) - gaps[i];
        if excess <= 0 {
            continue;
        }
        let (axis, delta) = match edge {
            0 => (Axis::X, -excess),
            1 => (Axis::Y, -excess),
            2 => (Axis::X, excess),
            _ => (Axis::Y, excess),
        };
        let mut moved = rect;
        match axis {
            Axis::X => {
                moved.min.x_nm += delta;
                moved.max.x_nm += delta;
            }
            Axis::Y => {
                moved.min.y_nm += delta;
                moved.max.y_nm += delta;
            }
        }
        let swept = Rect::new(
            Point::new(
                rect.min.x_nm.min(moved.min.x_nm),
                rect.min.y_nm.min(moved.min.y_nm),
            ),
            Point::new(
                rect.max.x_nm.max(moved.max.x_nm),
                rect.max.y_nm.max(moved.max.y_nm),
            ),
        );
        let blocked = (0..placement.components.len()).any(|j| {
            j != i && {
                let other = geom_rect(&placement.components[j], &geom[j]);
                other.min.x_nm < swept.max.x_nm
                    && other.max.x_nm > swept.min.x_nm
                    && other.min.y_nm < swept.max.y_nm
                    && other.max.y_nm > swept.min.y_nm
            }
        });
        if !blocked {
            shifted(placement, i, axis, delta);
        }
    }
}

/// Each hole's nearest corner of the original outline, and the hole's offset
/// from it.
fn hole_corner_offsets(
    placement: &Placement,
    roles: &[Role],
    outline: Rect,
) -> Vec<Option<(bool, bool, i64, i64)>> {
    placement
        .components
        .iter()
        .zip(roles)
        .map(|(p, role)| {
            (*role == Role::Hole).then(|| {
                let right = p.center.x_nm > (outline.min.x_nm + outline.max.x_nm) / 2;
                let bottom = p.center.y_nm > (outline.min.y_nm + outline.max.y_nm) / 2;
                let corner_x = if right {
                    outline.max.x_nm
                } else {
                    outline.min.x_nm
                };
                let corner_y = if bottom {
                    outline.max.y_nm
                } else {
                    outline.min.y_nm
                };
                (
                    right,
                    bottom,
                    p.center.x_nm - corner_x,
                    p.center.y_nm - corner_y,
                )
            })
        })
        .collect()
}

/// The box around every part but the holes.
fn parts_envelope(placement: &Placement, geom: &[Geom], roles: &[Role]) -> Option<Rect> {
    placement
        .components
        .iter()
        .zip(geom)
        .zip(roles)
        .filter(|(_, role)| **role != Role::Hole)
        .map(|((p, g), _)| geom_rect(p, g))
        .reduce(|a, b| {
            Rect::new(
                Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
                Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
            )
        })
}

/// Moves each hole in with the faces it sits on, and clears it of the parts.
///
/// The holes stay put during the slides, so parts compact round them; a hole
/// on the right or the bottom then follows that face inward by as much as the
/// face moved. Compaction can still have filled the spot it comes to, so a
/// hole that does not clear is put in the free spot nearest it that keeps the
/// original inset from the outline's edge, and only when there is none inside
/// does it go outside, growing the outline.
fn seat_holes(
    board: &Board,
    placement: &mut Placement,
    geom: &[Geom],
    roles: &[Role],
    offsets: &[Option<(bool, bool, i64, i64)>],
    before: Rect,
) {
    let parts: Vec<Rect> = placement
        .components
        .iter()
        .zip(geom)
        .zip(roles)
        .filter(|(_, role)| **role != Role::Hole)
        .map(|((p, g), _)| geom_rect(p, g))
        .collect();
    let Some(envelope) = parts_envelope(placement, geom, roles) else {
        return;
    };
    let margin = mm_to_nm(tight_edge_margin_mm(board));
    let pitch_nm = mm_to_nm(0.5);
    let outer = Rect::new(
        Point::new(envelope.min.x_nm - margin, envelope.min.y_nm - margin),
        Point::new(envelope.max.x_nm + margin, envelope.max.y_nm + margin),
    );
    let face_moved_x = (before.max.x_nm - envelope.max.x_nm).max(0) / pitch_nm * pitch_nm;
    let face_moved_y = (before.max.y_nm - envelope.max.y_nm).max(0) / pitch_nm * pitch_nm;
    let mut taken: Vec<Rect> = Vec::new();
    for (index, offset) in offsets.iter().enumerate() {
        let Some((right, bottom, dx, dy)) = offset else {
            continue;
        };
        let (inset_x, inset_y) = (dx.abs(), dy.abs());
        let half_w = mm_to_nm(geom[index].size.0) / 2;
        let half_h = mm_to_nm(geom[index].size.1) / 2;
        let hole_rect = |at: Point| Rect::from_center_half_extents(at, half_w, half_h);
        let clear = |at: Point, taken: &[Rect]| -> bool {
            let hole = hole_rect(at);
            parts.iter().chain(taken.iter()).all(|r| {
                hole.max.x_nm <= r.min.x_nm
                    || hole.min.x_nm >= r.max.x_nm
                    || hole.max.y_nm <= r.min.y_nm
                    || hole.min.y_nm >= r.max.y_nm
            })
        };
        let here = placement.components[index].center;
        let ideal = Point::new(
            here.x_nm - if *right { face_moved_x } else { 0 },
            here.y_nm - if *bottom { face_moved_y } else { 0 },
        );
        let mut at = clear(ideal, &taken).then_some(ideal);
        if at.is_none() {
            // The free spot nearest the ideal one that keeps the original inset.
            let (lo_x, hi_x) = if *right {
                (outer.min.x_nm, outer.max.x_nm - inset_x)
            } else {
                (outer.min.x_nm + inset_x, outer.max.x_nm)
            };
            let (lo_y, hi_y) = if *bottom {
                (outer.min.y_nm, outer.max.y_nm - inset_y)
            } else {
                (outer.min.y_nm + inset_y, outer.max.y_nm)
            };
            let mut spots: Vec<Point> = Vec::new();
            let mut x = lo_x / pitch_nm * pitch_nm;
            while x <= hi_x {
                let mut y = lo_y / pitch_nm * pitch_nm;
                while y <= hi_y {
                    spots.push(Point::new(x, y));
                    y += pitch_nm;
                }
                x += pitch_nm;
            }
            let distance = |p: &Point| (p.x_nm - ideal.x_nm).abs() + (p.y_nm - ideal.y_nm).abs();
            spots.sort_by_key(distance);
            at = spots.iter().copied().find(|p| clear(*p, &taken));
        }
        if at.is_none() {
            // Nothing inside is free: walk outward from the corner.
            let (step_x, step_y) = (if *right { 1 } else { -1 }, if *bottom { 1 } else { -1 });
            let mut p = ideal;
            for _ in 0..400 {
                if clear(p, &taken) {
                    at = Some(p);
                    break;
                }
                p = Point::new(p.x_nm + step_x * pitch_nm, p.y_nm + step_y * pitch_nm);
            }
        }
        let at = at.unwrap_or(ideal);
        taken.push(hole_rect(at));
        placement.components[index].center = at;
    }
}

/// A part's courtyard rectangle as the compaction sees it, for the developer probe.
#[doc(hidden)]
#[must_use]
pub fn courtyard_rect_probe(placed: &ComponentPlacement, size: (f64, f64)) -> Rect {
    geom_rect(placed, &Geom { size })
}

/// Total estimated wire length of a placement, for the developer probe.
#[doc(hidden)]
#[must_use]
pub fn hpwl_probe(board: &Board, placement: &Placement) -> f64 {
    synth_geometry::nm_to_mm(total_hpwl_nm(board, placement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place;

    fn load_board(path: &str) -> Board {
        let source = std::fs::read_to_string(path).expect("read fixture");
        let parse = synth_parser::parse(&source, path.to_string());
        let ast = parse.ast.as_ref().expect("parse");
        let registry =
            synth_registry::load_dir(&std::path::Path::new("../..").join("registry").join("parts"))
                .expect("registry");
        let loader = synth_ir::FsImportLoader {
            root: std::path::PathBuf::from("../.."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, path);
        synth_ir::lower(&resolved.program, &registry, path)
            .board
            .expect("board")
    }

    fn rects(board: &Board, placement: &Placement) -> Vec<Rect> {
        let geom = geometry(board, placement);
        placement
            .components
            .iter()
            .zip(&geom)
            .map(|(p, g)| geom_rect(p, g))
            .collect()
    }

    /// A layout with the slack an agent's pinned layout carries.
    fn spread(board: &Board) -> Placement {
        let mut placement = place(board).expect("place");
        for placed in &mut placement.components {
            placed.center = Point::new(placed.center.x_nm * 14 / 10, placed.center.y_nm * 14 / 10);
        }
        crate::tighten_outline_after_sidecar(board, &mut placement);
        placement
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.min.x_nm < b.max.x_nm
            && b.min.x_nm < a.max.x_nm
            && a.min.y_nm < b.max.y_nm
            && b.min.y_nm < a.max.y_nm
    }

    #[test]
    fn closing_the_gaps_shrinks_the_board_and_overlaps_nothing() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let before = spread(&board);
        let spacing = GapSpacing {
            gap_mm: 2.0,
            dense_gap_mm: 2.0,
        };
        let after = compact_by_gaps(&board, &before, spacing).expect("compacts");
        assert!(
            area_of(&after) < area_of(&before) * 0.97,
            "{} -> {}",
            area_of(&after),
            area_of(&before)
        );
        let rects = rects(&board, &after);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!overlap(a, b), "courtyards overlap after compaction");
            }
        }
    }

    #[test]
    fn a_flush_connector_stays_as_close_to_its_edge_as_it_was() {
        let board = load_board("../../fixtures/designs/edge_jst_connectors.synth");
        let before = spread(&board);
        let spacing = GapSpacing {
            gap_mm: 2.0,
            dense_gap_mm: 2.0,
        };
        let after = compact_by_gaps(&board, &before, spacing).expect("compacts");
        let (rects_before, rects_after) = (rects(&board, &before), rects(&board, &after));
        let mut checked = 0;
        for (i, p) in before.components.iter().enumerate() {
            let Some(edge) = flush_edge_index(&board, p.id) else {
                continue;
            };
            let was = edge_gap_nm(&rects_before[i], before.board_outline, edge);
            let is = edge_gap_nm(&rects_after[i], after.board_outline, edge);
            assert!(
                is <= was,
                "connector {i} sits {is} nm from edge {edge}, was {was}"
            );
            checked += 1;
        }
        assert!(checked > 0, "the board has no flush connectors");
    }

    #[test]
    fn a_passive_inside_the_rim_of_a_connector_edge_is_reported_and_clears_when_moved() {
        let board = load_board("../../fixtures/designs/edge_jst_connectors.synth");
        let mut placement = spread(&board);
        let after = compact_by_gaps(
            &board,
            &placement,
            GapSpacing {
                gap_mm: 2.0,
                dense_gap_mm: 2.0,
            },
        )
        .expect("compacts");
        placement = after;
        let rects_now = rects(&board, &placement);
        let by_refdes = |r: &str| {
            placement
                .components
                .iter()
                .position(|p| board.component(p.id).is_some_and(|c| c.refdes == r))
                .expect(r)
        };
        let (j3, c1) = (by_refdes("J3"), by_refdes("C1"));
        let half = (rects_now[c1].max.y_nm - rects_now[c1].min.y_nm) / 2;
        // C1 stands with its far side level with the bottom connector's face.
        placement.components[c1].center.y_nm = rects_now[j3].max.y_nm - half;
        let held = parts_holding_outline(&board, &placement);
        let id = placement.components[c1].id;
        let hit = held.iter().find(|h| h.id == id).expect("C1 reported");
        assert_eq!(hit.edge, 3);
        assert!(hit.move_in_mm > 0.5, "{}", hit.move_in_mm);

        placement.components[c1].center.y_nm -= mm_to_nm(4.0);
        assert!(
            parts_holding_outline(&board, &placement)
                .iter()
                .all(|h| h.id != id),
            "C1 no longer holds the outline once it is 4 mm in"
        );
    }

    fn area_of(placement: &Placement) -> f64 {
        crate::area_mm2(&placement.board_outline)
    }

    #[test]
    fn a_hard_hinted_part_does_not_move() {
        let board = load_board("../../examples/sensor_logger/sensor_logger.synth");
        let before = spread(&board);
        let spacing = GapSpacing {
            gap_mm: 2.0,
            dense_gap_mm: 3.5,
        };
        let after = compact_by_gaps(&board, &before, spacing).expect("compacts");
        for (a, b) in before.components.iter().zip(&after.components) {
            let component = board.component(a.id).unwrap();
            if is_hard_hinted(component)
                && flush_edge_index(&board, a.id).is_none()
                && component.kind.as_str() != "mounting_hole"
            {
                assert_eq!(a.center, b.center, "{} moved", component.refdes);
            }
        }
    }
}
