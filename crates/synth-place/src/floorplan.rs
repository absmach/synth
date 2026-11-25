// SPDX-License-Identifier: Apache-2.0

//! Boundary and edge connector floorplanner for human-quality PCB placement.
//!
//! Enforces human-like boundary anchoring:
//! - Edge connectors (USB-C, JST battery, headers) are anchored to outer board edges with outward rotation.
//! - MCUs and main processors are centered in the core interior.
//! - Regulators live near power input connectors.
//! - Sensors are grouped cleanly in peripheral sensor zones.

use std::collections::HashMap;
use synth_geometry::{Point, Rect, Rotation};
use synth_ir::{Board, ComponentId};
use synth_registry::MatingFace;

/// Target initial floorplan position and orientation for a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorplanTarget {
    pub point: Point,
    pub rotation: Rotation,
}

/// Board edge used when mapping a connector's physical mating face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardEdge {
    Top,
    Right,
    Bottom,
    Left,
}

/// Return the rotation that makes a footprint-local mating face point out of
/// the requested board edge. Keeping this mapping in one place prevents the
/// USB-C rule and agent validation from silently developing different angle
/// conventions.
#[must_use]
pub fn rotation_for_mating_edge(face: MatingFace, edge: BoardEdge) -> Rotation {
    let local = match face {
        MatingFace::Top => (0_i8, -1_i8),
        MatingFace::Right => (1, 0),
        MatingFace::Bottom => (0, 1),
        MatingFace::Left => (-1, 0),
    };
    let desired = match edge {
        BoardEdge::Top => (0, -1),
        BoardEdge::Right => (1, 0),
        BoardEdge::Bottom => (0, 1),
        BoardEdge::Left => (-1, 0),
    };
    match (local, desired) {
        ((x, y), (dx, dy)) if (x, y) == (dx, dy) => Rotation::Zero,
        ((x, y), (dx, dy)) if (y, -x) == (dx, dy) => Rotation::Ninety,
        ((x, y), (dx, dy)) if (-x, -y) == (dx, dy) => Rotation::OneEighty,
        _ => Rotation::TwoSeventy,
    }
}

/// A connector whose declared mating face does not point away from the
/// nearest board edge. This is kept in the placement crate so DRC, the agent
/// transition oracle, and preview/export consumers share one interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorOrientationIssue {
    pub refdes: String,
    pub edge: BoardEdge,
    pub expected: Rotation,
    pub actual: Rotation,
}

/// Check edge-mounted connectors against their physical mating-face metadata.
///
/// A connector is treated as edge-mounted when its centre lies within its own
/// courtyard half-extent (rotation aware) plus a short slack of an outline
/// edge. The slack absorbs the placer's own board-edge margin — edge-anchored
/// parts are inset by that margin on top of their courtyard depth — while
/// still excluding ordinary interior connectors. A fixed centre-distance
/// threshold cannot do this: a USB-C receptacle's courtyard half-depth
/// (~4.7 mm) plus the 4 mm edge margin already exceeds the old 8 mm constant,
/// so genuinely edge-mounted receptacles were silently skipped.
#[must_use]
pub fn connector_orientation_issues(
    board: &Board,
    outline: Rect,
    placements: &[crate::ComponentPlacement],
) -> Vec<ConnectorOrientationIssue> {
    edge_connectors(board, outline, placements)
        .into_iter()
        .filter(|found| found.placement.rotation != found.expected)
        .map(|found| ConnectorOrientationIssue {
            refdes: found.refdes,
            edge: found.edge,
            expected: found.expected,
            actual: found.placement.rotation,
        })
        .collect()
}

/// A connector whose mating plane sits off the board edge it opens through by
/// more than its footprint allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDepthIssue {
    pub refdes: String,
    pub edge: BoardEdge,
    /// Signed distance from the board edge to the mating plane, in nm.
    /// Positive means the plane sits inside the board (recessed); negative
    /// means it overhangs the edge.
    pub offset_nm: i64,
    /// Largest distance from the flush position the footprint allows, in nm.
    pub tolerance_nm: i64,
    /// Offset that puts the connector's courtyard flush with the edge, in nm.
    pub flush_nm: i64,
    /// How far to move the connector outward, toward and past `edge`, to put
    /// its mating plane in the allowed range, in nm. Negative moves it in.
    pub move_out_nm: i64,
    /// The part declares `mating_plane_offset_mm`, so the plane position is
    /// the registry's, not inferred from the courtyard.
    pub declared: bool,
    /// The allowed overhang range `[min, max]` past the edge, in nm, when
    /// the part declares one.
    pub overhang_range_nm: Option<(i64, i64)>,
}

/// A connector's declared mating plane in board coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatingPlane {
    /// The board side the face points out of after rotation.
    pub side: BoardEdge,
    /// The plane's coordinate on the axis normal to `side`, in nm: `x` for
    /// left/right, `y` for top/bottom.
    pub coord_nm: i64,
    /// Allowed overhang `[min, max]` past the edge, in nm.
    pub overhang_nm: (i64, i64),
}

impl MatingPlane {
    /// How far the plane sits past an edge of `outline` on its side, in nm.
    /// Positive overhangs, negative is recessed.
    #[must_use]
    pub fn overhang_past(&self, outline: Rect) -> i64 {
        match self.side {
            BoardEdge::Top => outline.min.y_nm - self.coord_nm,
            BoardEdge::Bottom => self.coord_nm - outline.max.y_nm,
            BoardEdge::Left => outline.min.x_nm - self.coord_nm,
            BoardEdge::Right => self.coord_nm - outline.max.x_nm,
        }
    }

    /// Where the board edge must sit for the plane to overhang it by the
    /// middle of its allowed range, on the plane's axis.
    #[must_use]
    pub fn target_edge_nm(&self) -> i64 {
        let target = i64::midpoint(self.overhang_nm.0, self.overhang_nm.1);
        match self.side {
            BoardEdge::Top | BoardEdge::Left => self.coord_nm + target,
            BoardEdge::Bottom | BoardEdge::Right => self.coord_nm - target,
        }
    }
}

/// The declared mating plane of a placed connector, if its part gives a
/// `mating_face` and a `mating_plane_offset_mm`. The placement's centre is
/// the courtyard centre; the offset is measured from the footprint origin.
#[must_use]
pub fn mating_plane(
    component: &synth_ir::Component,
    placement: &crate::ComponentPlacement,
) -> Option<MatingPlane> {
    let part = component.part.as_ref()?;
    let dims = part.footprint_dimensions.as_ref()?;
    let face = dims.mating_face?;
    let offset_nm = synth_geometry::mm_to_nm(dims.mating_plane_offset_mm?);
    let (lo, hi) = dims.mating_overhang_mm.map_or((0, 0), |[lo, hi]| {
        (synth_geometry::mm_to_nm(lo), synth_geometry::mm_to_nm(hi))
    });
    let (ux, uy) = match face {
        MatingFace::Top => (0_i64, -1_i64),
        MatingFace::Right => (1, 0),
        MatingFace::Bottom => (0, 1),
        MatingFace::Left => (-1, 0),
    };
    let (dx, dy) = placement.rotation.rotate_offset(ux, uy);
    let ((cx_mm, cy_mm), _) = synth_layout::pcb_courtyard_geometry_for_part(part);
    let (ox, oy) = placement.rotation.rotate_offset(
        synth_geometry::mm_to_nm(cx_mm),
        synth_geometry::mm_to_nm(cy_mm),
    );
    let origin_x = placement.center.x_nm - ox;
    let origin_y = placement.center.y_nm - oy;
    let (side, coord_nm) = match (dx, dy) {
        (0, -1) => (BoardEdge::Top, origin_y - offset_nm),
        (0, _) => (BoardEdge::Bottom, origin_y + offset_nm),
        (1, _) => (BoardEdge::Right, origin_x + offset_nm),
        _ => (BoardEdge::Left, origin_x - offset_nm),
    };
    Some(MatingPlane {
        side,
        coord_nm,
        overhang_nm: (lo, hi),
    })
}

/// Smallest depth tolerance, in mm. It only absorbs grid rounding for a part
/// that declares no courtyard margin.
pub const MIN_DEPTH_TOLERANCE_MM: f64 = 0.05;

/// Check that an edge-mounted connector's mating plane meets the board edge.
///
/// The plane is the connector body's face on its mating side. The courtyard is
/// centred on the placement and the part's `courtyard_margin_mm` is the rim it
/// adds around the body, so the plane sits one margin inside the courtyard
/// edge. A connector whose courtyard is flush with the edge therefore has its
/// plane one margin inside the board. The check allows the plane to be off
/// that flush position by one margin either way, and by
/// [`MIN_DEPTH_TOLERANCE_MM`] at least.
///
/// Only connectors that already face the right way are judged here. A
/// connector that faces inward is reported by [`connector_orientation_issues`].
#[must_use]
pub fn connector_depth_issues(
    board: &Board,
    outline: Rect,
    placements: &[crate::ComponentPlacement],
) -> Vec<ConnectorDepthIssue> {
    let mut issues = Vec::new();
    for found in edge_connectors(board, outline, placements) {
        if found.placement.rotation != found.expected {
            continue;
        }
        let min_tol = synth_geometry::mm_to_nm(MIN_DEPTH_TOLERANCE_MM);
        if let Some(component) = board.component(found.placement.id)
            && let Some(plane) = mating_plane(component, found.placement)
        {
            // The registry states where the plane is and how far past the
            // edge it may sit: judge that, not the courtyard.
            let overhang = plane.overhang_past(outline);
            let (lo, hi) = plane.overhang_nm;
            let move_out_nm = if overhang < lo - min_tol {
                lo - overhang
            } else if overhang > hi + min_tol {
                hi - overhang
            } else {
                continue;
            };
            issues.push(ConnectorDepthIssue {
                refdes: found.refdes,
                edge: found.edge,
                offset_nm: -overhang,
                tolerance_nm: min_tol,
                flush_nm: 0,
                move_out_nm,
                declared: true,
                overhang_range_nm: Some((lo, hi)),
            });
            continue;
        }
        let margin_nm = synth_geometry::mm_to_nm(found.margin_mm);
        let tolerance_nm = margin_nm.max(min_tol);
        let (distance, half_depth) = match found.edge {
            BoardEdge::Top => (found.placement.center.y_nm - outline.min.y_nm, found.half.1),
            BoardEdge::Bottom => (outline.max.y_nm - found.placement.center.y_nm, found.half.1),
            BoardEdge::Left => (found.placement.center.x_nm - outline.min.x_nm, found.half.0),
            BoardEdge::Right => (outline.max.x_nm - found.placement.center.x_nm, found.half.0),
        };
        let offset_nm = distance - (half_depth - margin_nm);
        if (offset_nm - margin_nm).abs() > tolerance_nm {
            issues.push(ConnectorDepthIssue {
                refdes: found.refdes,
                edge: found.edge,
                offset_nm,
                tolerance_nm,
                flush_nm: margin_nm,
                move_out_nm: offset_nm - margin_nm,
                declared: false,
                overhang_range_nm: None,
            });
        }
    }
    issues
}

/// An edge-mounted connector with the edge it opens through resolved.
struct EdgeConnector<'a> {
    refdes: String,
    placement: &'a crate::ComponentPlacement,
    edge: BoardEdge,
    expected: Rotation,
    /// Rotation-aware courtyard half extents `(x, y)`, in nm.
    half: (i64, i64),
    margin_mm: f64,
}

/// Every connector that declares a mating face and counts as edge-mounted,
/// with the edge it is judged against.
fn edge_connectors<'a>(
    board: &Board,
    outline: Rect,
    placements: &'a [crate::ComponentPlacement],
) -> Vec<EdgeConnector<'a>> {
    let mut found = Vec::new();
    let edge_slack = synth_geometry::mm_to_nm(6.0);
    for placement in placements {
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        let Some(face) = component
            .part
            .as_ref()
            .and_then(|part| part.footprint_dimensions.as_ref())
            .and_then(|dimensions| dimensions.mating_face)
        else {
            continue;
        };
        let ((_, _), (footprint_w, footprint_h)) = component.part.as_ref().map_or(
            ((0.0, 0.0), (10.0, 10.0)),
            synth_layout::pcb_courtyard_geometry_for_part,
        );
        let (half_x, half_y) = match placement.rotation {
            Rotation::Ninety | Rotation::TwoSeventy => (
                synth_geometry::mm_to_nm(footprint_h) / 2,
                synth_geometry::mm_to_nm(footprint_w) / 2,
            ),
            Rotation::Zero | Rotation::OneEighty => (
                synth_geometry::mm_to_nm(footprint_w) / 2,
                synth_geometry::mm_to_nm(footprint_h) / 2,
            ),
        };
        // Excess = how far the centre sits beyond the courtyard edge plus slack.
        // The nearest edge is the one with the smallest (signed) excess.
        let candidates = [
            (
                placement.center.y_nm - outline.min.y_nm,
                half_y + edge_slack,
                BoardEdge::Top,
            ),
            (
                outline.max.x_nm - placement.center.x_nm,
                half_x + edge_slack,
                BoardEdge::Right,
            ),
            (
                outline.max.y_nm - placement.center.y_nm,
                half_y + edge_slack,
                BoardEdge::Bottom,
            ),
            (
                placement.center.x_nm - outline.min.x_nm,
                half_x + edge_slack,
                BoardEdge::Left,
            ),
        ];
        // A hard `edge:` hint names the edge the connector is for, so it is judged
        // against that edge however far it sits from it. Without one, the nearest
        // edge counts, and only a connector that is close enough to be on it.
        let edge = if let Some(edge) = hard_hinted_edge(component) {
            edge
        } else {
            let Some(&(distance, tolerance, edge)) = candidates
                .iter()
                .min_by_key(|(distance, tolerance, _)| distance - tolerance)
            else {
                continue;
            };
            if distance < 0 || distance > tolerance {
                continue;
            }
            edge
        };
        found.push(EdgeConnector {
            refdes: component.refdes.clone(),
            placement,
            edge,
            expected: rotation_for_mating_edge(face, edge),
            half: (half_x, half_y),
            margin_mm: component
                .part
                .as_ref()
                .and_then(|part| part.footprint_dimensions.as_ref())
                .and_then(|dimensions| dimensions.courtyard_margin_mm)
                .unwrap_or(0.0),
        });
    }
    found
}

/// The edge a connector's hard `edge:` hint names, if it has one.
fn hard_hinted_edge(comp: &synth_ir::Component) -> Option<BoardEdge> {
    let hint = comp.placement_hint.as_ref()?;
    if hint.priority != synth_ir::PlacementPriority::Hard {
        return None;
    }
    hint.edge.as_ref()?;
    hinted_edge(comp)
}

/// The board edge a component was asked to sit on: an explicit `edge`, or the
/// edge implied by a `region`. `None` for a centre or unspecified request.
fn hinted_edge(comp: &synth_ir::Component) -> Option<BoardEdge> {
    let hint = comp.placement_hint.as_ref()?;
    if let Some(edge) = &hint.edge {
        return Some(match edge {
            synth_ir::PlacementEdge::Top => BoardEdge::Top,
            synth_ir::PlacementEdge::Right => BoardEdge::Right,
            synth_ir::PlacementEdge::Bottom => BoardEdge::Bottom,
            synth_ir::PlacementEdge::Left => BoardEdge::Left,
        });
    }
    match hint.region.as_ref()? {
        synth_ir::PlacementRegion::LeftEdge => Some(BoardEdge::Left),
        synth_ir::PlacementRegion::RightEdge => Some(BoardEdge::Right),
        synth_ir::PlacementRegion::TopEdge
        | synth_ir::PlacementRegion::TopLeft
        | synth_ir::PlacementRegion::TopRight => Some(BoardEdge::Top),
        synth_ir::PlacementRegion::BottomEdge
        | synth_ir::PlacementRegion::BottomLeft
        | synth_ir::PlacementRegion::BottomRight => Some(BoardEdge::Bottom),
        synth_ir::PlacementRegion::Centre => None,
    }
}

/// The outline edge nearest a point.
pub(crate) fn nearest_edge(point: Point, usable: Rect) -> BoardEdge {
    [
        (point.y_nm - usable.min.y_nm, BoardEdge::Top),
        (usable.max.x_nm - point.x_nm, BoardEdge::Right),
        (usable.max.y_nm - point.y_nm, BoardEdge::Bottom),
        (point.x_nm - usable.min.x_nm, BoardEdge::Left),
    ]
    .into_iter()
    .min_by_key(|(distance, _)| *distance)
    .map_or(BoardEdge::Left, |(_, edge)| edge)
}

/// The slot of every connector that shares its requested edge with another:
/// its index among them, in source order, and how many there are. Connectors
/// alone on their edge are absent, so they keep their single-connector
/// position. USB connectors place themselves and are not counted.
fn edge_connector_slots(board: &Board) -> HashMap<ComponentId, (usize, usize)> {
    let mut by_edge: HashMap<u8, Vec<ComponentId>> = HashMap::new();
    for comp in &board.components {
        let refdes = comp.refdes.to_lowercase();
        let usb = refdes.starts_with('j')
            && (refdes.contains("usb")
                || comp
                    .part
                    .as_ref()
                    .is_some_and(|p| p.id.as_str().contains("usb")))
            || comp.kind.to_lowercase().contains("usb");
        if usb {
            continue;
        }
        let jst = refdes.contains("jst")
            || comp
                .part
                .as_ref()
                .is_some_and(|p| p.id.as_str().contains("jst"));
        let edge = if jst {
            hinted_edge(comp).unwrap_or(BoardEdge::Left)
        } else if comp.kind.as_str() == "connector" {
            match hinted_edge(comp) {
                Some(edge)
                    if comp
                        .placement_hint
                        .as_ref()
                        .is_some_and(|h| h.edge.is_some()) =>
                {
                    edge
                }
                _ => continue,
            }
        } else {
            continue;
        };
        let key = match edge {
            BoardEdge::Left => 0,
            BoardEdge::Top => 1,
            BoardEdge::Right => 2,
            BoardEdge::Bottom => 3,
        };
        by_edge.entry(key).or_default().push(comp.id);
    }
    let mut slots = HashMap::new();
    for ids in by_edge.into_values().filter(|ids| ids.len() > 1) {
        let count = ids.len();
        for (index, id) in ids.into_iter().enumerate() {
            slots.insert(id, (index, count));
        }
    }
    slots
}

/// Compute target floorplan positions and rotations for connectors and macro ICs.
#[allow(clippy::too_many_lines, clippy::implicit_hasher)]
pub fn compute_floorplan_targets(
    board: &Board,
    usable: Rect,
    courtyard_lookup: &HashMap<ComponentId, (f64, f64)>,
) -> HashMap<ComponentId, FloorplanTarget> {
    let mut targets = HashMap::new();
    let min_x = usable.min.x_nm;
    let min_y = usable.min.y_nm;
    let max_x = usable.max.x_nm;
    let max_y = usable.max.y_nm;
    let width_nm = usable.width_nm();
    let height_nm = usable.height_nm();

    // Connectors hinted to the same edge each need their own slot along it.
    // Without this every connector on an edge was given the same point (or a
    // step counted across all edges), legalization then separated them by
    // pushing one inward, and an edge connector ended up off the edge. With
    // more than one on an edge they are spread evenly along it, in source
    // order; a lone connector keeps the position it always had.
    let slots = edge_connector_slots(board);
    let mut usb_count = 0_i64;
    // JST connectors already placed on each edge: left, right, top, bottom.
    let mut jst_per_edge = [0_i64; 4];
    let mut header_count = 0_i64;
    let mut sensor_count = 0_i64;
    for comp in &board.components {
        let (w_mm, h_mm) = courtyard_lookup
            .get(&comp.id)
            .copied()
            .unwrap_or((10.0, 10.0));
        let half_w = synth_geometry::mm_to_nm(w_mm) / 2;
        let half_h = synth_geometry::mm_to_nm(h_mm) / 2;

        let refdes_lower = comp.refdes.to_lowercase();
        let kind_lower = comp.kind.to_lowercase();

        let is_usb = refdes_lower.starts_with('j')
            && (refdes_lower.contains("usb")
                || comp
                    .part
                    .as_ref()
                    .is_some_and(|p| p.id.as_str().contains("usb")))
            || kind_lower.contains("usb");

        if is_usb {
            // USB-C connector anchored to its requested edge. The rotation
            // comes from physical footprint metadata, not a magic angle, so
            // a replacement USB footprint can declare a different unrotated
            // opening direction safely.
            //
            // NOTE: The pre-shift Y here is intentionally left at min_y + half_h; the
            // definitive edge-flush snap is applied deterministically AFTER the global
            // Y-shift in place_with_outline() using closed-form geometry.
            let requested_edge = comp
                .placement_hint
                .as_ref()
                .and_then(|hint| hint.edge.clone())
                .unwrap_or(synth_ir::PlacementEdge::Top);
            let board_edge = match requested_edge {
                synth_ir::PlacementEdge::Top => BoardEdge::Top,
                synth_ir::PlacementEdge::Right => BoardEdge::Right,
                synth_ir::PlacementEdge::Bottom => BoardEdge::Bottom,
                synth_ir::PlacementEdge::Left => BoardEdge::Left,
            };
            let rotation = comp
                .part
                .as_ref()
                .and_then(|part| part.footprint_dimensions.as_ref())
                .and_then(|dimensions| dimensions.mating_face)
                .map_or(Rotation::Zero, |face| {
                    rotation_for_mating_edge(face, board_edge)
                });
            let x = min_x + width_nm / 4 + (usb_count * synth_geometry::mm_to_nm(15.0));
            let y = min_y + half_h;
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation,
                },
            );
            usb_count += 1;
        } else if refdes_lower.contains("jst")
            || comp
                .part
                .as_ref()
                .is_some_and(|p| p.id.as_str().contains("jst"))
        {
            // A JST connector goes on the edge it asked for, left by default,
            // spread along that edge. Its rotation is set below from its
            // declared mating face, so the plug opens outward whichever edge
            // that is; `Ninety` is only the fallback for a part that declares
            // no face.
            let edge = hinted_edge(comp).unwrap_or(BoardEdge::Left);
            let inset = half_h + synth_geometry::mm_to_nm(1.5);
            let edge_index = match edge {
                BoardEdge::Left => 0,
                BoardEdge::Right => 1,
                BoardEdge::Top => 2,
                BoardEdge::Bottom => 3,
            };
            let step = jst_per_edge[edge_index] * synth_geometry::mm_to_nm(12.0);
            let (mut x, mut y) = match edge {
                BoardEdge::Left => (min_x + inset, min_y + height_nm / 3 + step),
                BoardEdge::Right => (max_x - inset, min_y + height_nm / 3 + step),
                BoardEdge::Top => (min_x + width_nm / 3 + step, min_y + inset),
                BoardEdge::Bottom => (min_x + width_nm / 3 + step, max_y - inset),
            };
            if let Some(&(index, count)) = slots.get(&comp.id) {
                let along = |len: i64, lo: i64| lo + len * (index as i64 + 1) / (count as i64 + 1);
                match edge {
                    BoardEdge::Left | BoardEdge::Right => y = along(height_nm, min_y),
                    BoardEdge::Top | BoardEdge::Bottom => x = along(width_nm, min_x),
                }
            }
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Ninety,
                },
            );
            jst_per_edge[edge_index] += 1;
        } else if comp.kind.as_str() == "connector" {
            // Honor an explicitly requested edge. The previous alternating
            // fallback ignored `edge: left/right`, so the order of components
            // in the source could silently put a header on the opposite side
            // of the board and turn a clean breakout into crossing fanout.
            let requested_edge = comp
                .placement_hint
                .as_ref()
                .and_then(|hint| hint.edge.clone());
            let (mut x, mut y, rot) = match requested_edge {
                Some(synth_ir::PlacementEdge::Left) => (
                    min_x + half_w + synth_geometry::mm_to_nm(1.0),
                    min_y + height_nm / 2,
                    Rotation::Zero,
                ),
                Some(synth_ir::PlacementEdge::Right) => (
                    max_x - half_w - synth_geometry::mm_to_nm(1.0),
                    min_y + height_nm / 2,
                    // Reverse the right-hand row so its pin order follows
                    // the RP2350's lower/right QFN GPIO sequence instead of
                    // forcing the breakout across the MCU.
                    Rotation::OneEighty,
                ),
                Some(synth_ir::PlacementEdge::Top) => (
                    min_x + width_nm / 2,
                    min_y + half_h + synth_geometry::mm_to_nm(1.0),
                    Rotation::Ninety,
                ),
                Some(synth_ir::PlacementEdge::Bottom) => (
                    min_x + width_nm / 2,
                    max_y - half_h - synth_geometry::mm_to_nm(1.0),
                    Rotation::Ninety,
                ),
                None if header_count % 2 == 0 => (
                    min_x + half_w + synth_geometry::mm_to_nm(1.0),
                    min_y + height_nm / 2,
                    Rotation::Zero,
                ),
                None => (
                    max_x - half_w - synth_geometry::mm_to_nm(1.0),
                    min_y + height_nm / 2,
                    Rotation::Zero,
                ),
            };
            if let (Some(&(index, count)), Some(edge)) = (slots.get(&comp.id), hinted_edge(comp)) {
                let along = |len: i64, lo: i64| lo + len * (index as i64 + 1) / (count as i64 + 1);
                match edge {
                    BoardEdge::Left | BoardEdge::Right => y = along(height_nm, min_y),
                    BoardEdge::Top | BoardEdge::Bottom => x = along(width_nm, min_x),
                }
            }
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: rot,
                },
            );
            header_count += 1;
        } else if kind_lower == "mcu"
            || kind_lower == "processor"
            || comp
                .part
                .as_ref()
                .is_some_and(|part| part.id.as_str().to_ascii_lowercase().contains("rp2350"))
            || (refdes_lower.starts_with("u2") && kind_lower != "regulator")
            || refdes_lower.contains("328p")
        {
            // Main MCU centered in core interior.
            let x = min_x + width_nm / 2;
            let y = min_y + height_nm / 2;
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Zero,
                },
            );
        } else if kind_lower == "regulator"
            || kind_lower == "power"
            || kind_lower == "charger"
            || refdes_lower.starts_with("u1")
            || refdes_lower.contains("ldo")
            || refdes_lower.contains("ams1117")
        {
            // Power regulator in Top-Left zone directly below USB
            let x = min_x + width_nm / 4;
            let y = min_y + height_nm / 3;
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Zero,
                },
            );
        } else if kind_lower == "sensor"
            || refdes_lower.starts_with("u3")
            || refdes_lower.starts_with("u4")
            || refdes_lower.contains("bmp")
            || refdes_lower.contains("bme")
        {
            // Sensors in Right zone, stacked vertically
            let x = min_x + (3 * width_nm) / 4;
            let y = min_y + height_nm / 3 + (sensor_count * synth_geometry::mm_to_nm(12.0));
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Zero,
                },
            );
            sensor_count += 1;
        } else if kind_lower == "memory"
            || kind_lower == "flash"
            || refdes_lower.starts_with("u5")
            || refdes_lower.contains("w25q")
        {
            // SPI Flash memory in Right zone, clear of MCU DIP footprint
            let x = min_x + (4 * width_nm) / 5;
            let y = min_y + (3 * height_nm) / 4;
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    // Keep the stock SOIC footprint in its native orientation.
                    // Rotating the inline footprint at the instance level
                    // changes the pad-axis interpretation in KiCad and can
                    // make adjacent 1.27 mm-pitch pads overlap. QSPI
                    // adjacency is handled by placement proximity, not by
                    // risking an invalid footprint orientation.
                    rotation: Rotation::Zero,
                },
            );
        } else if kind_lower == "switch" || refdes_lower.starts_with("sw") {
            // Tactile reset switch in Top-Left / Top-Center zone
            let x = min_x + width_nm / 2 - synth_geometry::mm_to_nm(10.0);
            let y = min_y + half_h + synth_geometry::mm_to_nm(2.0);
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Zero,
                },
            );
        }
    }

    // Every connector that declares which way its plug opens faces outward,
    // not only the USB receptacle. The edge is the one the source asked for,
    // or failing that the one nearest the connector's target. A part with no
    // declared face keeps whatever rotation its branch above chose.
    for comp in &board.components {
        if !matches!(comp.kind.as_str(), "connector" | "jack") {
            continue;
        }
        let Some(face) = comp
            .part
            .as_ref()
            .and_then(|part| part.footprint_dimensions.as_ref())
            .and_then(|dimensions| dimensions.mating_face)
        else {
            continue;
        };
        if let Some(target) = targets.get_mut(&comp.id) {
            let edge = hinted_edge(comp).unwrap_or_else(|| nearest_edge(target.point, usable));
            target.rotation = rotation_for_mating_edge(face, edge);
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    #[test]
    fn maps_usb_c_bottom_opening_to_top_edge() {
        assert_eq!(
            rotation_for_mating_edge(MatingFace::Bottom, BoardEdge::Top),
            Rotation::OneEighty
        );
    }

    /// A board of connectors on one edge, lowered from source, with a
    /// courtyard for each, so the floorplan is exercised as the placer calls it.
    fn connector_targets(
        parts: &[(&str, &str, &str)],
        usable_mm: (f64, f64),
    ) -> (Board, HashMap<ComponentId, FloorplanTarget>) {
        let mut components = String::new();
        for (refdes, part, edge) in parts {
            writeln!(
                components,
                "component {refdes}: connector \"{part}\" {{ placement_hint {{ edge: {edge} priority: hard }} }}"
            )
            .expect("write to a String");
        }
        let source = format!("board \"fp\" {{ layers 2 {components} }}");
        let parse = synth_parser::parse(&source, "fp.synth".to_string());
        let ast = parse.ast.as_ref().expect("parses");
        let registry =
            synth_registry::load_dir(&std::path::Path::new("../..").join("registry").join("parts"))
                .expect("registry");
        let board = synth_ir::lower(ast, &registry, "fp.synth")
            .board
            .expect("lowers");
        let lookup: HashMap<ComponentId, (f64, f64)> = board
            .components
            .iter()
            .map(|c| (c.id, (8.0, 6.0)))
            .collect();
        let usable = Rect::new(
            Point::new(0, 0),
            Point::new(
                synth_geometry::mm_to_nm(usable_mm.0),
                synth_geometry::mm_to_nm(usable_mm.1),
            ),
        );
        let targets = compute_floorplan_targets(&board, usable, &lookup);
        (board, targets)
    }

    /// Connectors hinted to one edge used to share one point, or a step counted
    /// across every edge, so legalization pushed one of them off the edge.
    #[test]
    fn connectors_hinted_to_one_edge_get_distinct_slots_along_it() {
        for part in ["jst_ph_3pin", "header_1x4"] {
            let (board, targets) = connector_targets(
                &[
                    ("J1", part, "right"),
                    ("J2", part, "right"),
                    ("J3", part, "right"),
                ],
                (80.0, 80.0),
            );
            let mut ys: Vec<i64> = board
                .components
                .iter()
                .map(|c| targets[&c.id].point.y_nm)
                .collect();
            ys.sort_unstable();
            for pair in ys.windows(2) {
                assert!(
                    pair[1] - pair[0] >= synth_geometry::mm_to_nm(10.0),
                    "{part}: slots too close or equal along the edge: {ys:?}"
                );
            }
            for c in &board.components {
                let x = targets[&c.id].point.x_nm;
                assert!(
                    x > synth_geometry::mm_to_nm(70.0),
                    "{part}: not on the right edge: {x}"
                );
            }
        }
    }

    /// The step counter was shared by every edge, so connectors on a second
    /// edge started where the first edge's count left off.
    #[test]
    fn connectors_on_different_edges_are_spread_independently() {
        let (board, targets) = connector_targets(
            &[
                ("J1", "jst_ph_3pin", "left"),
                ("J2", "jst_ph_3pin", "bottom"),
                ("J3", "jst_ph_3pin", "bottom"),
                ("J4", "jst_ph_3pin", "right"),
                ("J5", "jst_ph_3pin", "right"),
            ],
            (80.0, 80.0),
        );
        let at = |r: &str| {
            let id = board.components.iter().find(|c| c.refdes == r).expect(r).id;
            targets[&id].point
        };
        let mm = synth_geometry::nm_to_mm;
        assert!(
            (mm(at("J2").x_nm) - 80.0 / 3.0).abs() < 0.5,
            "J2 at {}",
            mm(at("J2").x_nm)
        );
        assert!(
            (mm(at("J3").x_nm) - 160.0 / 3.0).abs() < 0.5,
            "J3 at {}",
            mm(at("J3").x_nm)
        );
        assert!(
            (mm(at("J4").y_nm) - 80.0 / 3.0).abs() < 0.5,
            "J4 at {}",
            mm(at("J4").y_nm)
        );
        assert!(
            (mm(at("J5").y_nm) - 160.0 / 3.0).abs() < 0.5,
            "J5 at {}",
            mm(at("J5").y_nm)
        );
    }

    /// A lone connector on an edge keeps the position it has with nothing else
    /// on that edge, whatever sits on the edges before it in the source.
    #[test]
    fn a_lone_connector_on_each_edge_keeps_its_base_position() {
        let (board, targets) = connector_targets(
            &[
                ("J1", "jst_ph_3pin", "left"),
                ("J2", "jst_ph_3pin", "top"),
                ("J3", "jst_ph_3pin", "right"),
                ("J4", "jst_ph_3pin", "bottom"),
            ],
            (90.0, 60.0),
        );
        let at = |r: &str| {
            let id = board.components.iter().find(|c| c.refdes == r).expect(r).id;
            targets[&id].point
        };
        let mm = synth_geometry::nm_to_mm;
        assert!(
            (mm(at("J1").y_nm) - 20.0).abs() < 0.5,
            "left {}",
            mm(at("J1").y_nm)
        );
        assert!(
            (mm(at("J2").x_nm) - 30.0).abs() < 0.5,
            "top {}",
            mm(at("J2").x_nm)
        );
        assert!(
            (mm(at("J3").y_nm) - 20.0).abs() < 0.5,
            "right {}",
            mm(at("J3").y_nm)
        );
        assert!(
            (mm(at("J4").x_nm) - 30.0).abs() < 0.5,
            "bottom {}",
            mm(at("J4").x_nm)
        );
    }

    #[test]
    fn maps_each_local_face_to_each_edge() {
        let faces = [
            (MatingFace::Top, BoardEdge::Top),
            (MatingFace::Right, BoardEdge::Right),
            (MatingFace::Bottom, BoardEdge::Bottom),
            (MatingFace::Left, BoardEdge::Left),
        ];
        for (face, edge) in faces {
            assert_eq!(rotation_for_mating_edge(face, edge), Rotation::Zero);
        }
    }
}
