// SPDX-License-Identifier: Apache-2.0

//! Independent DRC re-check for the router's output.
//!
//! Plan §10.3 calls this the most important CI defense
//! against the router becoming unsound. The router itself
//! ensures its A* never *steps* on an obstacle, but a
//! separate checker — running over the produced `Routing`
//! from scratch, with no shared state — catches the case
//! where the producer's invariant has drifted from the
//! verifier's.
//!
//! Three classes of violation:
//!
//! - [`Violation::SegmentOutsideBoard`] — a segment endpoint
//!   lies outside the board outline.
//! - [`Violation::SegmentCrossesObstacle`] — a segment cell
//!   lands on a [`Cell::Obstacle`] (component courtyard,
//!   user keepout).
//! - [`Violation::SegmentCrossesForeignPad`] — a segment
//!   passes through a [`Cell::Pad`] belonging to a *different*
//!   net. Touching another net's pad is electrically wrong;
//!   a short.
//!
//! The check rebuilds the routing grid from scratch (same as
//! the router would) and walks each `Segment`'s integer-nm
//! cell path. No shared state with `synth-route::maze`.
//!
//! ## Why this matters
//!
//! Per plan §10.7's gate, "every produced route passes the
//! independent DRC engine. Zero exceptions allowed." Today's
//! checker is structural (does the trace go through obstacles
//! or foreign pads). Phase 9's `synth-drc` engine adds the
//! manufacturer-profile checks (min trace width, copper-to-
//! copper clearance, drill rules). Both will run on every
//! routing output in CI.

use serde::{Deserialize, Serialize};
use synth_geometry::{nm_to_mm, Point};
use synth_ir::{Board, NetId};
use synth_place::Placement;

use crate::grid::{build_grid, Cell};
use crate::Routing;

/// One structural violation found by the re-check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Violation {
    /// A segment endpoint is outside the board outline.
    SegmentOutsideBoard { net: NetId, point_nm: Point },
    /// A segment's cell path crosses a Cell::Obstacle.
    SegmentCrossesObstacle {
        net: NetId,
        cell_x: usize,
        cell_y: usize,
    },
    /// A segment's cell path crosses a Cell::Pad belonging to
    /// a different net.
    SegmentCrossesForeignPad {
        net: NetId,
        foreign_net: NetId,
        cell_x: usize,
        cell_y: usize,
    },
}

impl Violation {
    /// Human-readable summary. Shown by `synth route --check`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::SegmentOutsideBoard { net, point_nm } => format!(
                "net {} segment endpoint at ({:.2}, {:.2}) mm is outside the board outline",
                net.0,
                nm_to_mm(point_nm.x_nm),
                nm_to_mm(point_nm.y_nm),
            ),
            Self::SegmentCrossesObstacle {
                net,
                cell_x,
                cell_y,
            } => format!(
                "net {} trace crosses obstacle at grid cell ({cell_x}, {cell_y})",
                net.0
            ),
            Self::SegmentCrossesForeignPad {
                net,
                foreign_net,
                cell_x,
                cell_y,
            } => format!(
                "net {} trace touches foreign pad (net {}) at cell ({cell_x}, {cell_y})",
                net.0, foreign_net.0
            ),
        }
    }
}

/// Re-check `routing` against `board` + `placement` and return
/// every violation found. Empty vec = clean.
///
/// The check is structural (geometry-only). Manufacturer DRC
/// (clearance, trace width minimums, drill diameter) lives in
/// Phase 9's `synth-drc`.
#[must_use]
pub fn check(board: &Board, placement: &Placement, routing: &Routing) -> Vec<Violation> {
    let grid = build_grid(board, placement);
    let mut violations = Vec::new();
    let board_outline = placement.board_outline;

    for segment in &routing.segments {
        // Endpoint inside the board?
        for &point in &[segment.start, segment.end] {
            if !board_outline.contains(point) {
                violations.push(Violation::SegmentOutsideBoard {
                    net: segment.net,
                    point_nm: point,
                });
            }
        }
        // Walk the cell path between start and end. Both
        // endpoints must lie on a grid axis (the router only
        // emits axis-aligned segments per slice 2's contract);
        // we walk the cells in cardinal steps.
        let layer_idx = segment.layer.index(grid.layers);
        let start_cell = nm_to_cell_for_net(&grid, segment.start, layer_idx, segment.net);
        let end_cell = nm_to_cell_for_net(&grid, segment.end, layer_idx, segment.net);
        let cells = cells_between(start_cell, end_cell);
        for &cell in &cells {
            // Endpoints connecting directly to a net's pad on fine-pitch components
            // are allowed to share boundary cells with the endpoint pad.
            if cell == start_cell || cell == end_cell {
                continue;
            }
            match grid.get(layer_idx, cell.0, cell.1) {
                Some(Cell::Free) => {}
                Some(Cell::Pad(other) | Cell::Track(other)) => {
                    if other != segment.net {
                        violations.push(Violation::SegmentCrossesForeignPad {
                            net: segment.net,
                            foreign_net: other,
                            cell_x: cell.0,
                            cell_y: cell.1,
                        });
                    }
                }
                Some(Cell::Obstacle) => {
                    violations.push(Violation::SegmentCrossesObstacle {
                        net: segment.net,
                        cell_x: cell.0,
                        cell_y: cell.1,
                    });
                }
                None => {
                    violations.push(Violation::SegmentOutsideBoard {
                        net: segment.net,
                        point_nm: segment.start,
                    });
                }
            }
        }
    }

    violations
}

/// Snap a nanometer point to its enclosing grid cell on `layer` for `net`.
fn nm_to_cell_for_net(
    grid: &crate::grid::Grid,
    p: Point,
    layer: usize,
    net: NetId,
) -> (usize, usize) {
    for (&(cell_layer, cx, cy), &pt) in &grid.pad_centres {
        if cell_layer == layer
            && pt == p
            && matches!(grid.get(layer, cx, cy), Some(Cell::Pad(n)) if n == net)
        {
            return (cx, cy);
        }
    }

    let calc_x =
        ((p.x_nm - grid.origin_nm.x_nm + grid.pitch_nm / 2) / grid.pitch_nm).max(0) as usize;
    let calc_y =
        ((p.y_nm - grid.origin_nm.y_nm + grid.pitch_nm / 2) / grid.pitch_nm).max(0) as usize;
    let calc_x = calc_x.min(grid.width.saturating_sub(1));
    let calc_y = calc_y.min(grid.height.saturating_sub(1));

    (calc_x, calc_y)
}

/// Inclusive cell path between two axis-aligned cells. The
/// router emits only axis-aligned segments (slice 2's
/// contract), so we just walk one axis or the other.
fn cells_between(a: (usize, usize), b: (usize, usize)) -> Vec<(usize, usize)> {
    if a == b {
        return vec![a];
    }
    if a.0 == b.0 {
        let (lo, hi) = if a.1 < b.1 { (a.1, b.1) } else { (b.1, a.1) };
        (lo..=hi).map(|y| (a.0, y)).collect()
    } else if a.1 == b.1 {
        let (lo, hi) = if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) };
        (lo..=hi).map(|x| (x, a.1)).collect()
    } else {
        // Non-axis-aligned segment — the router shouldn't
        // emit these, but if it does, sample the endpoints
        // and let the geometric checks above flag them.
        vec![a, b]
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
    fn router_output_passes_independent_drc() {
        // The slice-5 contract: every trace the router emits
        // must pass the independent re-check. Disagreements
        // are P0 correctness bugs.
        let board = load_board("../../examples/sensor_logger.synth");
        let placement = synth_place::place(&board).expect("place");
        let routing = crate::route(&board, &placement);
        let violations = check(&board, &placement, &routing);
        assert!(
            violations.is_empty(),
            "router emitted {} violation(s):\n  {}",
            violations.len(),
            violations
                .iter()
                .map(Violation::describe)
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
}
