// SPDX-License-Identifier: Apache-2.0

//! Stage A — routing grid construction per plan §10.2.
//!
//! The grid is the search space the Lee maze + A* router
//! (slice 2) expands over. It's a 3D structure addressed by
//! `(layer, x, y)`: `LAYERS` deep (Top + Bottom in V1),
//! `width × height` cells per layer, integer-nanometer
//! coordinates snapped to a uniform pitch.
//!
//! Cells take one of three states:
//!
//! - [`Cell::Free`] — routable. A trace may enter, leave, or
//!   pass through this cell.
//! - [`Cell::Obstacle`] — blocked. A component courtyard
//!   (inflated by clearance), a keepout polygon, or a pad
//!   from a *different* net. The router never assigns a trace
//!   cell to this state.
//! - [`Cell::Pad`] — a copper pad belonging to a
//!   specific net. The router uses these as wavefront sources
//!   and sinks; a trace on net N may enter / leave through a
//!   `Pad(N)` cell but not through a `Pad(M)` cell where
//!   `M ≠ N`.
//!
//! ## Resolution
//!
//! Plan §10.2 says the grid resolution is "chosen per design
//! from minimum pitch in the netlist". Slice 1C uses a fixed
//! `0.5 mm` resolution — fine enough for DIP-28 (2.54 mm
//! pitch) and SOIC-8 (1.27 mm), coarse enough that a typical
//! 100 × 80 mm 2-layer board has ~64 000 cells (manageable
//! for Lee's BFS). Slice 2.x adapts per design.
//!
//! ## Determinism
//!
//! All construction is deterministic by component / pad
//! iteration order. No `HashMap` writes in the build hot
//! path; cell writes happen via deterministic loops over
//! board.components in `ComponentId` order, then over
//! footprint pads in the order the `.kicad_mod` file lists
//! them.

use serde::{Deserialize, Serialize};
use synth_geometry::{mm_to_nm, Point, Rect};
use synth_ir::{Board, NetId};
use synth_layout::kicad_footprint_loader;
use synth_place::Placement;

/// Compute adaptive routing grid resolution based on component pitch.
/// Returns 0.254 mm (10 mil) for fine-pitch ICs (LQFP, QFN, USB-C) or 0.5 mm for standard parts.
#[must_use]
pub fn compute_adaptive_grid_pitch(board: &Board) -> f64 {
    for comp in &board.components {
        if comp.kind == "connector" {
            return 0.254;
        }
        let Some(part) = comp.part.as_ref() else {
            continue;
        };
        if part.kicad_footprint.as_deref().is_some_and(|fp| {
            fp.contains("QFN")
                || fp.contains("DFN")
                || fp.contains("LQFP")
                || fp.contains("TQFP")
                || fp.contains("USB")
        }) {
            return 0.254;
        }
    }
    0.5
}

/// Routing grid resolution in millimetres. Slice 1C uses a
/// fixed 0.5 mm pitch; slice 2.x will adapt per design based
/// on the netlist's minimum pitch. 0.5 mm keeps dense boards
/// routable (the ESP32-C61 module's 22 pins escape cleanly); the
/// cost is that a mandatory 0.6 mm via can sit only one cell from a
/// foreign track, so via-to-track `clearance` is enforced by the
/// A* halo penalties in `maze.rs` rather than by raw grid spacing.
/// A coarser 0.6 mm pitch would satisfy clearance geometrically but
/// starves routing on dense boards (open circuits).
pub const ROUTING_GRID_MM: f64 = 0.5;

/// Clearance margin added to every courtyard before stamping
/// it as an obstacle, in millimetres. The placer already keeps
/// component courtyards apart (Phase 7 slice 2's hard
/// invariant), so this clearance only buys *trace-to-courtyard*
/// breathing room. Zero is the most permissive value the
/// router will see; slice 2.x reads a per-class clearance from
/// the manufacturer profile.
///
/// NOTE: raising this above 0 collides with the router's own
/// DRC-passing contract on dense boards (verified by
/// `router_output_passes_independent_drc`): the 0.5 mm routing
/// grid cannot honour a 0.127 mm courtyard clearance without
/// congestion (tracks collide / short). The real fix for
/// trace-to-courtyard clearance is a finer routing grid
/// (slice 2.x), not a larger obstacle here. Keep this at 0 until
/// then.
pub const OBSTACLE_CLEARANCE_MM: f64 = 0.0;

/// Number of routing layers in V1 default (Top + Bottom).
pub const LAYERS: usize = 2;

/// Cell state in the routing grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cell {
    /// Routable cell.
    Free,
    /// Blocked: courtyard + clearance, or keepout.
    Obstacle,
    /// Copper pad — a wavefront source / sink for the
    /// embedded net id.
    Pad(NetId),
    /// Routed track copper belonging to `NetId`. Blocks every
    /// *other* net's A* (so traces don't cross or run clearance-
    ///violatingly parallel), but the owning net may still traverse
    /// and change layers on its own `Track` cells when chaining
    /// legs. This is what makes the exported board respect
    /// trace-to-trace clearance and avoid `tracks_crossing`.
    Track(NetId),
}

/// 3D routing grid. Storage is row-major within each layer,
/// layers stacked: `cells[(layer * height + y) * width + x]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub layers: usize,
    /// Top-left corner of cell `(0, 0)` in nanometers,
    /// snapped to the routing pitch. Cell `(x, y)`'s centre
    /// is at `origin_nm + (x * pitch_nm, y * pitch_nm)`.
    pub origin_nm: Point,
    pub pitch_nm: i64,
    pub cells: Vec<Cell>,
    /// Centroids for pad cells, keyed by (layer, x, y).
    pub pad_centres: std::collections::HashMap<(usize, usize, usize), Point>,
    /// Exact physical bounding boxes of all pads on the board with their owning net
    /// (or NetId(u32::MAX) for unnetted copper pads). Used by the router for exact
    /// Euclidean via-to-pad clearance checks to eliminate KiCad hole_clearance and copper clearance violations.
    pub pads: Vec<(NetId, Rect)>,
    /// Exact centres and hole radii of all NPTH mechanical mounting holes on the board.
    /// Used by maze routing to enforce KiCad hole_clearance and hole_to_hole constraints.
    pub npth_holes: Vec<(Point, i64)>,
    /// Exact board outline bounding box. Used for edge clearance checks.
    pub board_outline: Rect,
}

impl Grid {
    /// Returns the cell at `(layer, x, y)`, or `None` when
    /// out of bounds.
    #[must_use]
    pub fn get(&self, layer: usize, x: usize, y: usize) -> Option<Cell> {
        if layer >= self.layers || x >= self.width || y >= self.height {
            return None;
        }
        Some(self.cells[self.idx(layer, x, y)])
    }

    /// Overwrite the cell at `(layer, x, y)`. Out-of-bounds writes
    /// are ignored. Used to stamp routed `Track` copper so later
    /// nets treat it as an obstacle.
    pub fn set(&mut self, layer: usize, x: usize, y: usize, cell: Cell) {
        if layer >= self.layers || x >= self.width || y >= self.height {
            return;
        }
        let i = self.idx(layer, x, y);
        self.cells[i] = cell;
    }

    /// Map a nanometre point to its enclosing grid cell, clamped to
    /// the board bounds. Used to rasterise routed segments back onto
    /// the grid during rip-up-reroute.
    pub fn cell_of_point(&self, p: Point) -> (usize, usize) {
        let x = ((p.x_nm - self.origin_nm.x_nm) / self.pitch_nm).clamp(0, self.width as i64 - 1)
            as usize;
        let y = ((p.y_nm - self.origin_nm.y_nm) / self.pitch_nm).clamp(0, self.height as i64 - 1)
            as usize;
        (x, y)
    }

    /// Index helper. Caller must check bounds.
    fn idx(&self, layer: usize, x: usize, y: usize) -> usize {
        (layer * self.height + y) * self.width + x
    }

    /// Centre of cell `(x, y)` on `layer` in nanometers. When
    /// the cell is covered by a physical pad on that layer, the
    /// exact pad centroid is returned so traces terminate on the
    /// real copper shape instead of the coarse grid point.
    #[must_use]
    pub fn cell_centre(&self, layer: usize, x: usize, y: usize) -> Point {
        if let Some(p) = self.pad_centres.get(&(layer, x, y)) {
            *p
        } else {
            Point::new(
                self.origin_nm.x_nm + (x as i64) * self.pitch_nm,
                self.origin_nm.y_nm + (y as i64) * self.pitch_nm,
            )
        }
    }

    /// Count cells in a given state. Used by tests + the slice
    /// 2 router's coverage telemetry.
    #[must_use]
    pub fn count(&self, predicate: impl Fn(Cell) -> bool) -> usize {
        self.cells.iter().copied().filter(|c| predicate(*c)).count()
    }
}

/// Build a routing grid for `board` against `placement`.
///
/// Steps:
///
/// 1. Compute grid dimensions: `placement.board_outline`
///    rounded out to the routing pitch.
/// 2. Initialise every cell to [`Cell::Free`] on both layers.
/// 3. For each placed component, stamp its courtyard rect
///    (inflated by `OBSTACLE_CLEARANCE_MM`) as
///    [`Cell::Obstacle`] on both layers, then stamp every
///    pad's covered cells as [`Cell::Pad`] (which overrides
///    the obstacle stamp). Pads not in the IR netlist stay
///    as obstacles.
///
/// `synth_layout::kicad_footprint_loader` provides the
/// per-footprint courtyard and pad geometry; the slice-1B
/// pad-net lookup logic is mirrored here so the grid
/// recognises which pads belong to which net.
#[must_use]
pub fn build_grid(board: &Board, placement: &Placement) -> Grid {
    build_grid_with_clearance(board, placement, synth_geometry::mm_to_nm(0.127))
}

/// Build routing grid with custom courtyard/obstacle clearance.
#[must_use]
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn build_grid_with_clearance(board: &Board, placement: &Placement, clearance_nm: i64) -> Grid {
    use std::collections::HashMap;
    use synth_geometry::Rotation;
    use synth_ir::ComponentId;

    let pitch_mm = compute_adaptive_grid_pitch(board);
    let pitch_nm = mm_to_nm(pitch_mm);

    // Grid origin = board outline's min, snapped down to the
    // pitch grid so cell (0,0) is at or just outside the
    // board's top-left.
    let snap_down = |v: i64| -> i64 { (v / pitch_nm) * pitch_nm };
    let snap_up = |v: i64| -> i64 { ((v + pitch_nm - 1) / pitch_nm) * pitch_nm };
    let origin_nm = Point::new(
        snap_down(placement.board_outline.min.x_nm),
        snap_down(placement.board_outline.min.y_nm),
    );
    let max_nm = Point::new(
        snap_up(placement.board_outline.max.x_nm),
        snap_up(placement.board_outline.max.y_nm),
    );
    let width = ((max_nm.x_nm - origin_nm.x_nm) / pitch_nm) as usize;
    let height = ((max_nm.y_nm - origin_nm.y_nm) / pitch_nm) as usize;
    let layers = (board.layers as usize).clamp(2, 4);
    let total_cells = layers * width * height;

    let mut grid = Grid {
        width,
        height,
        layers,
        origin_nm,
        pitch_nm,
        cells: vec![Cell::Free; total_cells],
        pad_centres: std::collections::HashMap::new(),
        pads: Vec::new(),
        npth_holes: Vec::new(),
        board_outline: placement.board_outline,
    };

    // Step (0): Edge clearance protection — stamp perimeter cells within
    // 0.5 mm of Edge.Cuts as Obstacle. Use 1 cell (0.5 mm) rather than 2
    // so edge-mounted connectors (e.g. USB-C at x≈3 mm, UART header at
    // x≈3 mm) retain pad escape corridors.
    // Keep one grid cell of routing clearance from Edge.Cuts.  Trace/pad
    // clearance is enforced by the pad keep-outs and the final DRC; adding
    // the full trace radius here removes connector escape corridors on the
    // Family A boards (notably env_logger's net_8) and makes A* search a
    // largely unsatisfiable grid.  Edge-mounted pads remain carveable while
    // generated copper is still checked independently by DRC.
    let edge_margin_cells = ((mm_to_nm(0.55) + pitch_nm - 1) / pitch_nm) as usize;
    for l in 0..layers {
        for y in 0..height {
            for x in 0..width {
                if x < edge_margin_cells
                    || x >= width.saturating_sub(edge_margin_cells)
                    || y < edge_margin_cells
                    || y >= height.saturating_sub(edge_margin_cells)
                {
                    let idx = grid.idx(l, x, y);
                    grid.cells[idx] = Cell::Obstacle;
                }
            }
        }
    }

    // Slice 1B's logic, mirrored: build a
    // `(component_id, pad_number_str) → net_id` lookup so we
    // can stamp Pad(net) cells with the correct net.
    let mut pad_net_lookup: HashMap<(ComponentId, String), NetId> = HashMap::new();
    for net in &board.nets {
        for endpoint in &net.endpoints {
            let Some(component) = board.component(endpoint.component) else {
                continue;
            };
            let Some(part) = component.part.as_ref() else {
                continue;
            };
            let Some(pin) = part.pins.get(endpoint.pin.0 as usize) else {
                continue;
            };
            pad_net_lookup.insert((component.id, pin.number.0.clone()), net.id);
        }
    }

    let mut mirror_additions = Vec::new();
    for ((comp, pin), net_id) in &pad_net_lookup {
        if pin == "A1" || pin == "B12" {
            mirror_additions.push(((*comp, "A12".to_string()), *net_id));
            mirror_additions.push(((*comp, "B1".to_string()), *net_id));
            mirror_additions.push(((*comp, "B12".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH1".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH2".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH3".to_string()), *net_id));
            mirror_additions.push(((*comp, "SH4".to_string()), *net_id));
        } else if pin == "A4" || pin == "B9" {
            mirror_additions.push(((*comp, "A9".to_string()), *net_id));
            mirror_additions.push(((*comp, "B4".to_string()), *net_id));
            mirror_additions.push(((*comp, "B9".to_string()), *net_id));
        }
    }
    for (k, v) in mirror_additions {
        pad_net_lookup.entry(k).or_insert(v);
    }

    let mut npth_cells = std::collections::HashSet::new();
    let mut connector_cells = std::collections::HashSet::new();
    let mut pad_comp_centers: std::collections::HashMap<(usize, usize, usize), Point> =
        std::collections::HashMap::new();
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(placement) = placement.components.iter().find(|p| p.id == component.id) else {
            continue;
        };

        // Step (a): courtyard → Obstacle, on both layers.
        let ((court_cx_mm, court_cy_mm), (court_w_mm, court_h_mm)) =
            synth_layout::pcb_courtyard_geometry_for_part(part);
        let half_w_nm = mm_to_nm(court_w_mm) / 2 + clearance_nm;
        let half_h_nm = mm_to_nm(court_h_mm) / 2 + clearance_nm;
        let (rotated_half_w, rotated_half_h) = match placement.rotation {
            Rotation::Zero | Rotation::OneEighty => (half_w_nm, half_h_nm),
            Rotation::Ninety | Rotation::TwoSeventy => (half_h_nm, half_w_nm),
        };
        let (rot_cx, rot_cy) = placement
            .rotation
            .rotate_offset(mm_to_nm(court_cx_mm), mm_to_nm(court_cy_mm));
        let court_center = placement.center;
        let courtyard_rect =
            Rect::from_center_half_extents(court_center, rotated_half_w, rotated_half_h);

        let origin_x_nm = placement.center.x_nm - rot_cx;
        let origin_y_nm = placement.center.y_nm - rot_cy;

        // Step (b): per-pad → Pad(net). Override courtyard
        // obstacles where pads lie.
        let loaded_pads: Option<Vec<kicad_footprint_loader::Pad>> = part
            .kicad_footprint
            .as_deref()
            .and_then(kicad_footprint_loader::pads)
            .or_else(|| kicad_footprint_loader::synth_part_pads(part));

        // Courtyards represent the physical body of components mounted on the top layer.
        // Inner and bottom layers remain free for routing. Through-hole pins penetrate
        // all layers and are stamped individually on every layer they occupy in step (b).
        stamp_rect_layer(&mut grid, courtyard_rect, Cell::Obstacle, 0);

        if let Some(pads) = loaded_pads {
            for pad in pads {
                let (pad_x_mm, pad_y_mm) = pad.center_mm;
                let (pad_w_mm, pad_h_mm) = pad.size_mm;
                // KiCad's file convention: Ninety maps local
                // (x, y) to (y, -x). Shared helper keeps the
                // router's pad positions identical to what
                // pcbnew renders from the exported file.
                let (rot_x, rot_y) = placement
                    .rotation
                    .rotate_offset(mm_to_nm(pad_x_mm), mm_to_nm(pad_y_mm));
                let (rot_w_nm, rot_h_nm) = if placement.rotation.swaps_extents() {
                    (mm_to_nm(pad_h_mm), mm_to_nm(pad_w_mm))
                } else {
                    (mm_to_nm(pad_w_mm), mm_to_nm(pad_h_mm))
                };
                let pad_centre = Point::new(origin_x_nm + rot_x, origin_y_nm + rot_y);
                let pad_rect =
                    Rect::from_center_half_extents(pad_centre, rot_w_nm / 2, rot_h_nm / 2);
                let maybe_net = pad_net_lookup
                    .get(&(component.id, pad.number.clone()))
                    .copied();

                if let Some(net_id) = maybe_net {
                    grid.pads.push((net_id, pad_rect));
                    let cell = Cell::Pad(net_id);
                    let (x0, x1, y0, y1) = grid_range_for_rect(
                        pad_rect,
                        grid.origin_nm,
                        grid.pitch_nm,
                        grid.width,
                        grid.height,
                        true,
                    );
                    for layer in 0..grid.layers {
                        let has_copper = match layer {
                            0 => pad.copper_layers.includes_front(),
                            _ => pad.copper_layers.includes_back(),
                        };
                        if !has_copper {
                            continue;
                        }
                        stamp_rect_layer(&mut grid, pad_rect, cell, layer);
                        stamp_rect_pad_centres(&mut grid, pad_rect, pad_centre, layer);
                        let is_mcu_or_ic = component.kind == "mcu"
                            || component
                                .part
                                .as_ref()
                                .is_some_and(|p| p.pins.len() > 8 && component.kind != "connector");
                        let is_connector =
                            component.kind == "connector" || component.kind == "jack";
                        if is_connector {
                            for y in y0..=y1 {
                                for x in x0..=x1 {
                                    connector_cells.insert((layer, x, y));
                                }
                            }
                        }
                        if is_mcu_or_ic {
                            for y in y0..=y1 {
                                for x in x0..=x1 {
                                    pad_comp_centers.insert((layer, x, y), placement.center);
                                }
                            }
                        }
                    }
                } else if pad.is_npth {
                    // NPTH mechanical mounting holes penetrate all layers and require 0.25 mm hole clearance.
                    let hole_radius_nm = rot_w_nm / 2;
                    grid.npth_holes.push((pad_centre, hole_radius_nm));
                    // KiCad board setup hole clearance is 0.250 mm.
                    // Stamping obstacle at hole_radius + 0.260 mm + 0.130 mm ensures no track center
                    // can route closer than 0.260 mm to the hole edge.
                    let total_radius_nm = hole_radius_nm + mm_to_nm(0.260) + mm_to_nm(0.130);
                    let obstacle_rect = Rect::from_center_half_extents(
                        pad_centre,
                        total_radius_nm,
                        total_radius_nm,
                    );
                    let (x0, x1, y0, y1) = grid_range_for_rect(
                        obstacle_rect,
                        grid.origin_nm,
                        grid.pitch_nm,
                        grid.width,
                        grid.height,
                        true,
                    );
                    let r_sq = total_radius_nm * total_radius_nm;
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            let cell_centre = grid.cell_centre(0, x, y);
                            let dx = cell_centre.x_nm - pad_centre.x_nm;
                            let dy = cell_centre.y_nm - pad_centre.y_nm;
                            if dx * dx + dy * dy <= r_sq {
                                for layer in 0..grid.layers {
                                    let idx = grid.idx(layer, x, y);
                                    grid.cells[idx] = Cell::Obstacle;
                                    npth_cells.insert((layer, x, y));
                                }
                            }
                        }
                    }
                } else {
                    // Unnetted copper pad (e.g. unused pin on connector/MCU).
                    // Stamp as foreign pad NetId(u32::MAX) on its exact copper rectangle (no inflation),
                    // ensuring it is unroutable without blocking adjacent active pads.
                    grid.pads.push((NetId(u32::MAX), pad_rect));
                    let cell = Cell::Pad(NetId(u32::MAX));
                    for layer in 0..grid.layers {
                        let has_copper = match layer {
                            0 => pad.copper_layers.includes_front(),
                            _ => pad.copper_layers.includes_back(),
                        };
                        if has_copper {
                            stamp_rect_layer(&mut grid, pad_rect, cell, layer);
                        }
                    }
                }
            }
        } else {
            // Fallback for environments where KiCad footprint files aren't installed locally (e.g. CI runner).
            // Stamp synthetic pads for each pin on `part` around the component's position.
            for (pin_idx, pin) in part.pins.iter().enumerate() {
                if let Some(&net_id) = pad_net_lookup.get(&(component.id, pin.number.0.clone())) {
                    let offset_x_mm = (pin_idx as f64) * ROUTING_GRID_MM;
                    let pad_centre = Point::new(origin_x_nm + mm_to_nm(offset_x_mm), origin_y_nm);
                    let pad_rect = Rect::from_center_half_extents(
                        pad_centre,
                        mm_to_nm(0.3) / 2,
                        mm_to_nm(0.3) / 2,
                    );
                    grid.pads.push((net_id, pad_rect));
                    for layer in 0..grid.layers {
                        stamp_rect_layer(&mut grid, pad_rect, Cell::Pad(net_id), layer);
                        stamp_rect_pad_centres(&mut grid, pad_rect, pad_centre, layer);
                    }
                }
            }
        }
    }

    // Step (c): Pin escape fanout: walk outward from every Pad(net) cell through Obstacle cells
    // converting them to Free, stopping as soon as Free space, another pad, or foreign pad adjacency is reached.
    carve_pin_escapes(
        &mut grid,
        &pad_comp_centers,
        &npth_cells,
        &connector_cells,
        edge_margin_cells,
    );

    grid
}

/// Carve a pin-escape corridor from every Pad cell outward in
/// each cardinal direction: walk through Obstacle cells
/// converting them to Free until we hit a non-Obstacle cell
/// (Free, Pad, or board edge). The standard PCB routing
/// "fanout" technique — without it, A* expansion is stuck on
/// pad cells surrounded by inflated-courtyard obstacles.
///
/// Stops at any cell that's already Pad (other net's pad) or
/// Free; never crosses a board edge. Pads of a *different*
/// net are NOT carved through — the courtyard rule still
/// protects them. NPTH mounting holes are strictly protected.
fn carve_pin_escapes(
    grid: &mut Grid,
    pad_comp_centers: &std::collections::HashMap<(usize, usize, usize), Point>,
    npth_cells: &std::collections::HashSet<(usize, usize, usize)>,
    connector_cells: &std::collections::HashSet<(usize, usize, usize)>,
    edge_margin_cells: usize,
) {
    let max_carve_depth = (mm_to_nm(12.0) / grid.pitch_nm) as usize;
    let stride_y = grid.width;
    let stride_layer = grid.width * grid.height;
    let snapshot = grid.cells.clone();
    for layer in 0..grid.layers {
        let base = layer * stride_layer;
        for y in 0..grid.height {
            for x in 0..grid.width {
                let Cell::Pad(owning_net) = snapshot[base + y * stride_y + x] else {
                    continue;
                };
                if owning_net == NetId(u32::MAX) {
                    continue;
                }

                let is_conn_pad = connector_cells.contains(&(layer, x, y));

                // For fine-pitch ICs/MCUs, pads are arranged along package edges.
                // Carve fanout in any direction that does NOT step inward towards the chip center.
                // This prevents carving inward across the chip body while still allowing outward
                // and tangential breakout corridors if a direct perpendicular escape is blocked.
                let mut dir_buf = [(0_i32, 0_i32); 4];
                let mut dir_count = 0;
                let directions: &[(i32, i32)] =
                    if let Some(center) = pad_comp_centers.get(&(layer, x, y)) {
                        let pad_centre = grid
                            .pad_centres
                            .get(&(layer, x, y))
                            .copied()
                            .unwrap_or_else(|| {
                                Point::new(
                                    grid.origin_nm.x_nm + (x as i64) * grid.pitch_nm,
                                    grid.origin_nm.y_nm + (y as i64) * grid.pitch_nm,
                                )
                            });
                        let vx = pad_centre.x_nm - center.x_nm;
                        let vy = pad_centre.y_nm - center.y_nm;
                        for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                            let inward = i64::from(dx) * (-vx) + i64::from(dy) * (-vy);
                            if inward <= 0 {
                                dir_buf[dir_count] = (dx, dy);
                                dir_count += 1;
                            }
                        }
                        &dir_buf[..dir_count]
                    } else {
                        &[(1, 0), (-1, 0), (0, 1), (0, -1)]
                    };

                for &(dx, dy) in directions {
                    let mut depth = 0_usize;
                    let mut nx = x as i32 + dx;
                    let mut ny = y as i32 + dy;
                    while depth < max_carve_depth {
                        if nx < edge_margin_cells as i32
                            || ny < edge_margin_cells as i32
                            || nx >= (grid.width.saturating_sub(edge_margin_cells)) as i32
                            || ny >= (grid.height.saturating_sub(edge_margin_cells)) as i32
                        {
                            break;
                        }
                        let cx = nx as usize;
                        let cy = ny as usize;
                        let idx = base + cy * stride_y + cx;

                        if npth_cells.contains(&(layer, cx, cy)) {
                            break;
                        }

                        match grid.cells[idx] {
                            Cell::Obstacle => {
                                grid.cells[idx] = Cell::Free;
                            }
                            _ => break,
                        }

                        // For dense connectors (like USB-C), allow a narrow 1-2 cell lateral flare
                        // outside the pad so traces can step aside from neighboring via shadows.
                        if is_conn_pad && depth >= 1 {
                            for (pdx, pdy) in [(-dy, dx), (dy, -dx)] {
                                for pstep in 1..=2 {
                                    let px = cx as i32 + pdx * pstep;
                                    let py = cy as i32 + pdy * pstep;
                                    if px < edge_margin_cells as i32
                                        || py < edge_margin_cells as i32
                                        || px
                                            >= (grid.width.saturating_sub(edge_margin_cells)) as i32
                                        || py
                                            >= (grid.height.saturating_sub(edge_margin_cells))
                                                as i32
                                    {
                                        break;
                                    }
                                    let pcx = px as usize;
                                    let pcy = py as usize;
                                    let pidx = base + pcy * stride_y + pcx;
                                    if npth_cells.contains(&(layer, pcx, pcy)) {
                                        break;
                                    }
                                    if is_adjacent_to_foreign_pad(grid, layer, pcx, pcy, owning_net)
                                    {
                                        break;
                                    }
                                    match grid.cells[pidx] {
                                        Cell::Obstacle => {
                                            grid.cells[pidx] = Cell::Free;
                                        }
                                        Cell::Free => {}
                                        _ => break,
                                    }
                                }
                            }
                        }

                        nx += dx;
                        ny += dy;
                        depth += 1;
                    }
                }
            }
        }
    }
}

pub fn is_adjacent_to_foreign_pad(
    grid: &Grid,
    layer: usize,
    x: usize,
    y: usize,
    owning_net: NetId,
) -> bool {
    let stride_y = grid.width;
    let base = layer * grid.height * grid.width;
    for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && ny >= 0 && nx < grid.width as i32 && ny < grid.height as i32 {
            let idx = base + (ny as usize) * stride_y + (nx as usize);
            if let Cell::Pad(n) = grid.cells[idx] {
                if n != owning_net {
                    return true;
                }
            }
        }
    }
    false
}

pub fn is_adjacent_to_foreign_track(
    grid: &Grid,
    layer: usize,
    x: usize,
    y: usize,
    owning_net: NetId,
) -> bool {
    let stride_y = grid.width;
    let base = layer * grid.height * grid.width;
    for dx in -1_i32..=1 {
        for dy in -1_i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && ny >= 0 && nx < grid.width as i32 && ny < grid.height as i32 {
                let idx = base + (ny as usize) * stride_y + (nx as usize);
                if let Cell::Track(n) = grid.cells[idx] {
                    if n != owning_net {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Stamp every cell inside `rect_nm` with `cell` on every
/// layer. Boundary-inclusive (a cell whose centre lies on
/// the rect edge is stamped).
fn _stamp_rect(grid: &mut Grid, rect_nm: Rect, cell: Cell) {
    for layer in 0..grid.layers {
        stamp_rect_layer(grid, rect_nm, cell, layer);
    }
}

fn grid_range_for_rect(
    rect_nm: Rect,
    origin: Point,
    pitch: i64,
    width: usize,
    height: usize,
    is_pad: bool,
) -> (usize, usize, usize, usize) {
    let (x0, x1, y0, y1) = if is_pad {
        let to_grid_min = |v: i64, o: i64| -> i64 {
            let diff = v - o;
            if diff <= 0 {
                0
            } else {
                (diff + pitch - 1) / pitch
            }
        };
        let to_grid_max = |v: i64, o: i64| -> i64 {
            let diff = v - o;
            if diff <= 0 {
                0
            } else {
                diff / pitch
            }
        };
        let mut x0 = to_grid_min(rect_nm.min.x_nm, origin.x_nm);
        let mut x1 = to_grid_max(rect_nm.max.x_nm, origin.x_nm);
        let mut y0 = to_grid_min(rect_nm.min.y_nm, origin.y_nm);
        let mut y1 = to_grid_max(rect_nm.max.y_nm, origin.y_nm);
        if x0 > x1 {
            let mid = ((rect_nm.min.x_nm + rect_nm.max.x_nm) / 2 - origin.x_nm + pitch / 2) / pitch;
            x0 = mid;
            x1 = mid;
        }
        if y0 > y1 {
            let mid = ((rect_nm.min.y_nm + rect_nm.max.y_nm) / 2 - origin.y_nm + pitch / 2) / pitch;
            y0 = mid;
            y1 = mid;
        }
        (x0, x1, y0, y1)
    } else {
        let to_grid_min = |v: i64, o: i64| -> i64 { (v - o) / pitch };
        let to_grid_max = |v: i64, o: i64| -> i64 { (v - o + pitch - 1) / pitch };
        (
            to_grid_min(rect_nm.min.x_nm, origin.x_nm),
            to_grid_max(rect_nm.max.x_nm, origin.x_nm),
            to_grid_min(rect_nm.min.y_nm, origin.y_nm),
            to_grid_max(rect_nm.max.y_nm, origin.y_nm),
        )
    };
    (
        (x0.max(0) as usize).min(width.saturating_sub(1)),
        (x1.max(0) as usize).min(width.saturating_sub(1)),
        (y0.max(0) as usize).min(height.saturating_sub(1)),
        (y1.max(0) as usize).min(height.saturating_sub(1)),
    )
}

fn stamp_rect_layer(grid: &mut Grid, rect_nm: Rect, cell: Cell, layer: usize) {
    let is_pad = matches!(cell, Cell::Pad(_));
    let (x0, x1, y0, y1) = grid_range_for_rect(
        rect_nm,
        grid.origin_nm,
        grid.pitch_nm,
        grid.width,
        grid.height,
        is_pad,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            let idx = grid.idx(layer, x, y);
            let existing = grid.cells[idx];
            match (existing, cell) {
                (Cell::Pad(_), Cell::Obstacle) => {}
                (Cell::Pad(n1), Cell::Pad(n2)) if n1 != n2 => {
                    grid.cells[idx] = Cell::Obstacle;
                }
                _ => grid.cells[idx] = cell,
            }
        }
    }
}

fn stamp_rect_pad_centres(grid: &mut Grid, rect_nm: Rect, pad_centre: Point, layer: usize) {
    let (x0, x1, y0, y1) = grid_range_for_rect(
        rect_nm,
        grid.origin_nm,
        grid.pitch_nm,
        grid.width,
        grid.height,
        true,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            grid.pad_centres.insert((layer, x, y), pad_centre);
        }
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
    fn grid_is_deterministic() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let a = build_grid(&board, &placement);
        let b = build_grid(&board, &placement);
        assert_eq!(a, b);
    }

    #[test]
    fn grid_has_obstacles_and_pads_and_free_cells() {
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let grid = build_grid(&board, &placement);

        let free = grid.count(|c| matches!(c, Cell::Free));
        let obstacles = grid.count(|c| matches!(c, Cell::Obstacle));
        let pads = grid.count(|c| matches!(c, Cell::Pad(_)));

        assert!(free > 0, "routing channels exist");
        assert!(obstacles > 0, "courtyards block some cells");
        assert!(pads > 0, "pads stamped at expected positions");
        assert_eq!(
            free + obstacles + pads,
            grid.cells.len(),
            "every cell has exactly one state"
        );
    }

    #[test]
    fn pad_cells_are_set_for_every_endpoint() {
        // For each net endpoint, there must be at least one
        // Pad(net) cell in the grid pointing at the right
        // net id. Without this invariant, slice 2's Lee maze
        // would have no source / sink to expand from for
        // entire nets.
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let grid = build_grid(&board, &placement);

        for net in &board.nets {
            if net.endpoints.is_empty() {
                continue;
            }
            let count = grid.count(|c| c == Cell::Pad(net.id));
            assert!(
                count > 0,
                "net {:?} ({}) has {} endpoints but no Pad({:?}) cells",
                net.id,
                net.name,
                net.endpoints.len(),
                net.id
            );
        }
    }
}
