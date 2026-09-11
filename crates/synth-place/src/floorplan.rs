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
/// Connectors are considered edge-mounted when their centre is within 8 mm of
/// an outline edge. The tolerance covers normal courtyard depth while avoiding
/// false positives for ordinary interior connectors.
#[must_use]
pub fn connector_orientation_issues(
    board: &Board,
    outline: Rect,
    placements: &[crate::ComponentPlacement],
) -> Vec<ConnectorOrientationIssue> {
    let mut issues = Vec::new();
    let edge_tolerance = synth_geometry::mm_to_nm(8.0);
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
        let distances = [
            (placement.center.y_nm - outline.min.y_nm, BoardEdge::Top),
            (outline.max.x_nm - placement.center.x_nm, BoardEdge::Right),
            (outline.max.y_nm - placement.center.y_nm, BoardEdge::Bottom),
            (placement.center.x_nm - outline.min.x_nm, BoardEdge::Left),
        ];
        let Some(&(distance, edge)) = distances.iter().min_by_key(|(distance, _)| *distance) else {
            continue;
        };
        if distance < 0 || distance > edge_tolerance {
            continue;
        }
        let expected = rotation_for_mating_edge(face, edge);
        if placement.rotation != expected {
            issues.push(ConnectorOrientationIssue {
                refdes: component.refdes.clone(),
                edge,
                expected,
                actual: placement.rotation,
            });
        }
    }
    issues
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
    let width_nm = usable.width_nm();
    let height_nm = usable.height_nm();

    let mut usb_count = 0_i64;
    let mut jst_count = 0_i64;
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
            // USB-C connector anchored to the Top Edge, left-aligned. The
            // rotation comes from physical footprint metadata, not a magic
            // angle, so a replacement USB footprint can declare a different
            // unrotated opening direction safely.
            //
            // NOTE: The pre-shift Y here is intentionally left at min_y + half_h; the
            // definitive edge-flush snap is applied deterministically AFTER the global
            // Y-shift in place_with_outline() using closed-form geometry.
            let x = min_x + width_nm / 4 + (usb_count * synth_geometry::mm_to_nm(15.0));
            let y = min_y + half_h;
            let mating_face = comp
                .part
                .as_ref()
                .and_then(|p| p.footprint_dimensions.as_ref())
                .and_then(|d| d.mating_face)
                .unwrap_or(MatingFace::Bottom);
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: rotation_for_mating_edge(mating_face, BoardEdge::Top),
                },
            );
            usb_count += 1;
        } else if refdes_lower.contains("jst")
            || comp
                .part
                .as_ref()
                .is_some_and(|p| p.id.as_str().contains("jst"))
        {
            // JST battery connector on Left Edge, facing left (-X) with Rotation::Ninety
            let x = min_x + half_h + synth_geometry::mm_to_nm(1.5);
            let y = min_y + height_nm / 3 + (jst_count * synth_geometry::mm_to_nm(12.0));
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::Ninety,
                },
            );
            jst_count += 1;
        } else if comp.kind.as_str() == "connector" {
            // General pin header on Left or Right edge
            let (x, rot) = if header_count % 2 == 0 {
                (
                    min_x + half_w + synth_geometry::mm_to_nm(1.0),
                    Rotation::Zero,
                )
            } else {
                (
                    max_x - half_w - synth_geometry::mm_to_nm(1.0),
                    Rotation::Zero,
                )
            };
            let y = min_y + height_nm / 4 + ((header_count / 2) * synth_geometry::mm_to_nm(15.0));
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
            || refdes_lower.starts_with("u2")
            || refdes_lower.contains("328p")
        {
            // Main MCU centered in core interior
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

    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_usb_c_bottom_opening_to_top_edge() {
        assert_eq!(
            rotation_for_mating_edge(MatingFace::Bottom, BoardEdge::Top),
            Rotation::OneEighty
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
