// SPDX-License-Identifier: Apache-2.0

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap, HashSet};
use synth_ir::NetId;

const GRID_STEP: f64 = 1.27; // 50 mil KiCad schematic grid step
const BEND_COST: f64 = 4.0;
const CROSSING_COST: f64 = 40.0;
/// Upper bound on `width_cells`/`height_cells` in [`SchematicGrid::new`].
///
/// `g_costs`/`came_from` are allocated eagerly at `width_cells *
/// height_cells * 5` entries; with no ceiling, a component placed at
/// an extreme, unvalidated position (e.g. an MCP `LayoutOp::MoveComponent`
/// call with `x_mm`/`y_mm` in the tens of thousands) balloons that
/// product into a multi-gigabyte allocation and can abort the process.
/// At `GRID_STEP`, `MAX_GRID_CELLS` per axis covers roughly a 1.5 m
/// square sheet — already far beyond any realistic schematic — while
/// keeping the worst-case allocation bounded to a couple hundred MB.
/// Positions beyond this range are clamped to the grid edge by
/// [`SchematicGrid::to_grid_pos`] rather than causing an OOM abort.
const MAX_GRID_CELLS: i32 = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    None,
    North,
    South,
    East,
    West,
}

impl Direction {
    fn delta(self) -> (i32, i32) {
        match self {
            Direction::None => (0, 0),
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::East => (1, 0),
            Direction::West => (-1, 0),
        }
    }

    /// Dense index (0..5) for use as the last dimension of the flat
    /// `g_costs`/`came_from` arrays in [`SchematicGrid::find_path`].
    fn index(self) -> usize {
        match self {
            Direction::None => 0,
            Direction::North => 1,
            Direction::South => 2,
            Direction::East => 3,
            Direction::West => 4,
        }
    }
}

#[derive(Debug, Clone)]
struct WireSegment {
    net_id: NetId,
    p1: (i32, i32),
    p2: (i32, i32),
    is_horizontal: bool,
}

pub struct SchematicGrid {
    min_x: f64,
    min_y: f64,
    width_cells: i32,
    height_cells: i32,
    blocked: BTreeSet<(i32, i32)>,
    /// The blocked cells that only reserve room for a part's Reference and
    /// Value text. [`Self::find_path`] may cross them when it is relaxed: the
    /// text moves clear of a wire afterwards, a body cannot.
    soft: BTreeSet<(i32, i32)>,
    wire_segments: Vec<WireSegment>,
    /// Indices into `wire_segments`, by grid row for the horizontal ones and
    /// by grid column for the vertical ones. A move only meets segments on
    /// its own row or column, so [`Self::find_path`] reads these two short
    /// lists instead of every segment on the page. Plain vectors, not a
    /// `HashMap`: an earlier map-based index lost to the flat scan in debug
    /// builds on hashing alone.
    horizontal_by_row: Vec<Vec<usize>>,
    vertical_by_column: Vec<Vec<usize>>,
    /// Segments whose row or column falls outside the grid. Always scanned.
    unindexed_segments: Vec<usize>,
    /// A* cost/backtrack bookkeeping, sized `width_cells *
    /// height_cells * 5` (5 = one entry per [`Direction`]).
    /// Allocated once in [`Self::new`] and reset (not reallocated)
    /// at the top of every [`Self::find_path`] call — reallocating
    /// these per net, per route, made routing a ~700ms bottleneck in
    /// debug builds on boards with a large bounding box (hundreds of
    /// thousands of cells × however many nets need routing).
    g_costs: Vec<f64>,
    came_from: Vec<Option<((i32, i32), Direction)>>,
    /// The `g_costs` / `came_from` entries the last search wrote. The next
    /// search resets only these, so a short path costs its own size and not
    /// the size of the page.
    touched: Vec<usize>,
}

#[derive(Copy, Clone, PartialEq)]
struct AStarNode {
    pos: (i32, i32),
    dir: Direction,
    g_cost: f64,
    f_cost: f64,
}

impl Eq for AStarNode {}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap on f_cost, tie-breaker on g_cost, then pos.0, pos.1, dir
        other
            .f_cost
            .partial_cmp(&self.f_cost)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                other
                    .g_cost
                    .partial_cmp(&self.g_cost)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| self.pos.0.cmp(&other.pos.0))
            .then_with(|| self.pos.1.cmp(&other.pos.1))
            .then_with(|| self.dir.cmp(&other.dir))
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl SchematicGrid {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        let padding = 20.0;
        let x0 = min_x - padding;
        let y0 = min_y - padding;
        let x1 = max_x + padding;
        let y1 = max_y + padding;

        let width_cells = (((x1 - x0) / GRID_STEP).ceil() as i32).clamp(50, MAX_GRID_CELLS);
        let height_cells = (((y1 - y0) / GRID_STEP).ceil() as i32).clamp(50, MAX_GRID_CELLS);
        let buffer_len = (width_cells as usize) * (height_cells as usize) * 5;

        Self {
            min_x: x0,
            min_y: y0,
            width_cells,
            height_cells,
            blocked: BTreeSet::new(),
            soft: BTreeSet::new(),
            wire_segments: Vec::new(),
            horizontal_by_row: vec![Vec::new(); height_cells.max(0) as usize],
            vertical_by_column: vec![Vec::new(); width_cells.max(0) as usize],
            unindexed_segments: Vec::new(),
            g_costs: vec![f64::INFINITY; buffer_len],
            came_from: vec![None; buffer_len],
            touched: Vec::new(),
        }
    }

    pub fn to_grid_pos(&self, x: f64, y: f64) -> (i32, i32) {
        let gx = ((x - self.min_x) / GRID_STEP).round() as i32;
        let gy = ((y - self.min_y) / GRID_STEP).round() as i32;
        (
            gx.clamp(0, self.width_cells - 1),
            gy.clamp(0, self.height_cells - 1),
        )
    }

    pub fn to_world_pos(&self, gx: i32, gy: i32) -> (f64, f64) {
        let x = self.min_x + (f64::from(gx)) * GRID_STEP;
        let y = self.min_y + (f64::from(gy)) * GRID_STEP;
        (
            (x / GRID_STEP).round() * GRID_STEP,
            (y / GRID_STEP).round() * GRID_STEP,
        )
    }

    #[allow(clippy::similar_names)]
    pub fn mark_obstacle_rect(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        let (gx1, gy1) = self.to_grid_pos(min_x, min_y);
        let (gx2, gy2) = self.to_grid_pos(max_x, max_y);

        let min_gx = gx1.min(gx2);
        let max_gx = gx1.max(gx2);
        let lo_gy = gy1.min(gy2);
        let hi_gy = gy1.max(gy2);

        for gx in min_gx..=max_gx {
            for gy in lo_gy..=hi_gy {
                self.blocked.insert((gx, gy));
                self.soft.remove(&(gx, gy));
            }
        }
    }

    /// Block a rectangle that only reserves room for text. A cell that is
    /// already blocked keeps its kind.
    #[allow(clippy::similar_names)]
    pub fn mark_soft_rect(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        let (gx1, gy1) = self.to_grid_pos(min_x, min_y);
        let (gx2, gy2) = self.to_grid_pos(max_x, max_y);
        for gx in gx1.min(gx2)..=gx1.max(gx2) {
            for gy in gy1.min(gy2)..=gy1.max(gy2) {
                if self.blocked.insert((gx, gy)) {
                    self.soft.insert((gx, gy));
                }
            }
        }
    }

    pub fn unmark_pos(&mut self, x: f64, y: f64) {
        let pos = self.to_grid_pos(x, y);
        self.blocked.remove(&pos);
        self.soft.remove(&pos);
    }

    /// Whether the cell covering `(x, y)` is currently blocked.
    pub fn is_blocked(&self, x: f64, y: f64) -> bool {
        let pos = self.to_grid_pos(x, y);
        self.blocked.contains(&pos)
    }

    pub fn register_wire_segment(&mut self, p1: (f64, f64), p2: (f64, f64), net_id: NetId) {
        let g1 = self.to_grid_pos(p1.0, p1.1);
        let g2 = self.to_grid_pos(p2.0, p2.1);
        if g1 == g2 {
            return;
        }

        let is_horiz = g1.1 == g2.1;
        let seg = WireSegment {
            net_id,
            p1: (g1.0.min(g2.0), g1.1.min(g2.1)),
            p2: (g1.0.max(g2.0), g1.1.max(g2.1)),
            is_horizontal: is_horiz,
        };
        let index = self.wire_segments.len();
        let slot = if seg.is_horizontal {
            usize::try_from(seg.p1.1)
                .ok()
                .and_then(|row| self.horizontal_by_row.get_mut(row))
        } else {
            usize::try_from(seg.p1.0)
                .ok()
                .and_then(|column| self.vertical_by_column.get_mut(column))
        };
        match slot {
            Some(list) => list.push(index),
            None => self.unindexed_segments.push(index),
        }
        self.wire_segments.push(seg);
    }

    /// Whether any segment of `points` (world mm) collinearly overlaps
    /// an already-registered wire segment belonging to a *different*
    /// net.
    ///
    /// [`Self::find_path`]'s A* search already refuses this ("Collinear
    /// overlap -> Blocked!" in the neighbor-expansion loop below), but
    /// the L-route fallback (`route::l_route_points`, used when A*
    /// fails) builds candidate paths independently of the grid's
    /// obstacle/segment bookkeeping and never consulted
    /// `wire_segments` at all. `score::segments_cross` also can't
    /// catch this case — it explicitly treats two parallel (both-
    /// horizontal or both-vertical) segments as never crossing, so a
    /// fallback path landing exactly on top of another net's wire
    /// previously had no check anywhere that would catch it, letting
    /// two unrelated nets render as one continuous overlapping line.
    pub fn path_overlaps_other_net_wire(&self, points: &[(f64, f64)], net_id: NetId) -> bool {
        for pair in points.windows(2) {
            let g1 = self.to_grid_pos(pair[0].0, pair[0].1);
            let g2 = self.to_grid_pos(pair[1].0, pair[1].1);
            if g1 == g2 {
                continue;
            }
            let is_horiz = g1.1 == g2.1;
            if !is_horiz && g1.0 != g2.0 {
                // Not axis-aligned (shouldn't happen for orthogonal
                // routes, but skip rather than false-positive on it).
                continue;
            }
            let (min_a, max_a) = if is_horiz {
                (g1.0.min(g2.0), g1.0.max(g2.0))
            } else {
                (g1.1.min(g2.1), g1.1.max(g2.1))
            };
            let line = if is_horiz { g1.1 } else { g1.0 };

            for seg in &self.wire_segments {
                if seg.net_id == net_id || seg.is_horizontal != is_horiz {
                    continue;
                }
                let seg_line = if is_horiz { seg.p1.1 } else { seg.p1.0 };
                if seg_line != line {
                    continue;
                }
                let (seg_min, seg_max) = if is_horiz {
                    (seg.p1.0, seg.p2.0)
                } else {
                    (seg.p1.1, seg.p2.1)
                };
                if max_a > seg_min && min_a < seg_max {
                    return true;
                }
            }
        }
        false
    }

    #[allow(clippy::unused_self)]
    fn manhattan_dist(&self, a: (i32, i32), b: (i32, i32)) -> f64 {
        f64::from((a.0 - b.0).abs() + (a.1 - b.1).abs())
    }

    #[allow(clippy::too_many_lines)]
    /// A shortest orthogonal path from `start_world` to `target_world`.
    ///
    /// `avoid` lists cells the path must not enter (the pin terminals of
    /// other nets). `relaxed` lets it cross the cells that only reserve room
    /// for a part's text, never a body.
    pub fn find_path(
        &mut self,
        start_world: (f64, f64),
        target_world: (f64, f64),
        net_id: NetId,
        relaxed: bool,
        avoid: &HashSet<(i32, i32)>,
    ) -> Option<Vec<(f64, f64)>> {
        let start = self.to_grid_pos(start_world.0, start_world.1);
        let target = self.to_grid_pos(target_world.0, target_world.1);

        if start == target {
            return Some(vec![start_world, target_world]);
        }

        let mut open_set = BinaryHeap::new();
        // Flat arrays indexed by (y * width + x) * 5 + dir, much
        // faster than BTreeMap for the dense cost bookkeeping A*
        // does. Reset in place (not reallocated) — see the doc
        // comment on the `g_costs`/`came_from` fields for why.
        for i in self.touched.drain(..) {
            self.g_costs[i] = f64::INFINITY;
            self.came_from[i] = None;
        }
        let width_cells = self.width_cells;
        let idx = |pos: (i32, i32), dir: Direction| {
            ((pos.1 as usize * width_cells as usize + pos.0 as usize) * 5) + dir.index()
        };

        let start_node = AStarNode {
            pos: start,
            dir: Direction::None,
            g_cost: 0.0,
            f_cost: self.manhattan_dist(start, target),
        };
        open_set.push(start_node);
        self.g_costs[idx(start, Direction::None)] = 0.0;
        self.touched.push(idx(start, Direction::None));

        let directions = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ];

        let mut best_target_state: Option<((i32, i32), Direction)> = None;

        while let Some(current) = open_set.pop() {
            let curr_key = (current.pos, current.dir);
            let recorded_g = self.g_costs[idx(current.pos, current.dir)];
            if current.g_cost > recorded_g + 1e-5 {
                continue;
            }

            if current.pos == target {
                best_target_state = Some(curr_key);
                break;
            }

            for &dir in &directions {
                let delta = dir.delta();
                let neighbor_pos = (current.pos.0 + delta.0, current.pos.1 + delta.1);

                if neighbor_pos.0 < 0
                    || neighbor_pos.0 >= self.width_cells
                    || neighbor_pos.1 < 0
                    || neighbor_pos.1 >= self.height_cells
                {
                    continue;
                }

                // Check obstacle collisions (allow start and target cells to be reached)
                if neighbor_pos != target
                    && neighbor_pos != start
                    && ((self.blocked.contains(&neighbor_pos)
                        && !(relaxed && self.soft.contains(&neighbor_pos)))
                        || avoid.contains(&neighbor_pos))
                {
                    continue;
                }

                // Check wire segment conflicts with existing nets
                let mut step_cost = 1.0;
                let move_is_horiz = dir == Direction::East || dir == Direction::West;
                let mut conflict = false;

                // Only the segments on this move's own line and on the line it
                // crosses can matter: collinear ones share the row (or column)
                // of the cell it leaves, crossing ones sit on the row (or
                // column) of the cell it enters.
                let (same_line, crossing_line): (&[usize], &[usize]) = if move_is_horiz {
                    (
                        usize::try_from(current.pos.1)
                            .ok()
                            .and_then(|row| self.horizontal_by_row.get(row))
                            .map_or(&[], Vec::as_slice),
                        usize::try_from(neighbor_pos.0)
                            .ok()
                            .and_then(|column| self.vertical_by_column.get(column))
                            .map_or(&[], Vec::as_slice),
                    )
                } else {
                    (
                        usize::try_from(current.pos.0)
                            .ok()
                            .and_then(|column| self.vertical_by_column.get(column))
                            .map_or(&[], Vec::as_slice),
                        usize::try_from(neighbor_pos.1)
                            .ok()
                            .and_then(|row| self.horizontal_by_row.get(row))
                            .map_or(&[], Vec::as_slice),
                    )
                };
                for &seg_index in same_line
                    .iter()
                    .chain(crossing_line)
                    .chain(&self.unindexed_segments)
                {
                    let seg = &self.wire_segments[seg_index];
                    if seg.net_id == net_id {
                        continue;
                    }
                    if seg.is_horizontal == move_is_horiz {
                        // Collinear overlap along same line segment -> Blocked!
                        if move_is_horiz && current.pos.1 == seg.p1.1 {
                            let min_x = current.pos.0.min(neighbor_pos.0);
                            let max_x = current.pos.0.max(neighbor_pos.0);
                            if max_x > seg.p1.0 && min_x < seg.p2.0 {
                                conflict = true;
                                break;
                            }
                        } else if !move_is_horiz && current.pos.0 == seg.p1.0 {
                            let min_y = current.pos.1.min(neighbor_pos.1);
                            let max_y = current.pos.1.max(neighbor_pos.1);
                            if max_y > seg.p1.1 && min_y < seg.p2.1 {
                                conflict = true;
                                break;
                            }
                        }
                    } else {
                        // Perpendicular wire crossing -> Crossing Penalty!
                        let cross_point = if move_is_horiz {
                            (neighbor_pos.0, current.pos.1)
                        } else {
                            (current.pos.0, neighbor_pos.1)
                        };
                        let in_seg = if seg.is_horizontal {
                            cross_point.1 == seg.p1.1
                                && cross_point.0 >= seg.p1.0
                                && cross_point.0 <= seg.p2.0
                        } else {
                            cross_point.0 == seg.p1.0
                                && cross_point.1 >= seg.p1.1
                                && cross_point.1 <= seg.p2.1
                        };
                        if in_seg {
                            step_cost += CROSSING_COST;
                        }
                    }
                }

                if conflict {
                    continue;
                }

                // Turn/Bend penalty
                if current.dir != Direction::None && current.dir != dir {
                    step_cost += BEND_COST;
                }

                let tentative_g = current.g_cost + step_cost;

                if tentative_g < self.g_costs[idx(neighbor_pos, dir)] {
                    self.g_costs[idx(neighbor_pos, dir)] = tentative_g;
                    self.came_from[idx(neighbor_pos, dir)] = Some(curr_key);
                    self.touched.push(idx(neighbor_pos, dir));

                    let h = self.manhattan_dist(neighbor_pos, target);
                    open_set.push(AStarNode {
                        pos: neighbor_pos,
                        dir,
                        g_cost: tentative_g,
                        f_cost: tentative_g + h,
                    });
                }
            }
        }

        let target_state = best_target_state?;

        // Reconstruct path
        let mut grid_path = Vec::new();
        let mut curr = target_state;
        while let Some(prev) = self.came_from[idx(curr.0, curr.1)] {
            grid_path.push(curr.0);
            curr = prev;
        }
        grid_path.push(start);
        grid_path.reverse();

        // Convert grid path to world coordinates & remove redundant inline points
        let mut world_pts = Vec::new();
        world_pts.push(start_world);

        for i in 1..grid_path.len().saturating_sub(1) {
            let p_prev = grid_path[i - 1];
            let p_curr = grid_path[i];
            let p_next = grid_path[i + 1];

            let dx1 = p_curr.0 - p_prev.0;
            let dy1 = p_curr.1 - p_prev.1;
            let dx2 = p_next.0 - p_curr.0;
            let dy2 = p_next.1 - p_curr.1;

            // Only add corner points where direction changes
            if (dx1, dy1) != (dx2, dy2) {
                world_pts.push(self.to_world_pos(p_curr.0, p_curr.1));
            }
        }

        world_pts.push(target_world);

        // Sanity check: deduplicate consecutive identical points
        let mut clean: Vec<(f64, f64)> = Vec::new();
        for &pt in &world_pts {
            if let Some(last) = clean.last()
                && (pt.0 - last.0).abs() < 0.01
                && (pt.1 - last.1).abs() < 0.01
            {
                continue;
            }
            clean.push(pt);
        }

        Some(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn grid() -> SchematicGrid {
        SchematicGrid::new(0.0, 0.0, 60.0, 60.0)
    }

    #[test]
    fn a_text_margin_walls_a_pin_in_until_it_is_relaxed() {
        let mut g = grid();
        g.mark_soft_rect(10.0, 10.0, 30.0, 30.0);
        let none = HashSet::new();
        let route = |g: &mut SchematicGrid, relaxed| {
            g.find_path((20.0, 20.0), (50.0, 20.0), NetId(0), relaxed, &none)
        };
        assert!(route(&mut g, false).is_none());
        assert!(route(&mut g, true).is_some());
    }

    #[test]
    fn a_body_is_never_relaxed() {
        let mut g = grid();
        g.mark_soft_rect(10.0, 10.0, 30.0, 30.0);
        g.mark_obstacle_rect(15.0, 15.0, 25.0, 25.0);
        let none = HashSet::new();
        let found = g.find_path((20.0, 20.0), (50.0, 20.0), NetId(0), true, &none);
        assert!(found.is_none());
    }

    #[test]
    fn a_path_keeps_off_the_cells_it_is_told_to_avoid() {
        let mut g = grid();
        let none = HashSet::new();
        let straight = g
            .find_path((10.0, 20.0), (50.0, 20.0), NetId(0), false, &none)
            .expect("open grid");
        assert_eq!(straight.len(), 2, "a straight run is two points");
        let avoid = HashSet::from([g.to_grid_pos(30.0, 20.0)]);
        let around = g
            .find_path((10.0, 20.0), (50.0, 20.0), NetId(0), false, &avoid)
            .expect("a way round");
        assert!(around.len() > 2, "{around:?}");
    }
}
