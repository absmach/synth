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

/// Target initial floorplan position and orientation for a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloorplanTarget {
    pub point: Point,
    pub rotation: Rotation,
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
            // USB-C connector anchored to the Top Edge, left-aligned.
            //
            // Rotation::OneEighty: the footprint origin is inside the board body.
            // With 180° rotation, the pads (which sit at y_local = -4.045 mm in the
            // unrotated footprint) are flipped to y_local = +4.045 mm — facing the
            // board interior. The receptacle opening (the physical port the cable
            // enters) is therefore at the negative-Y side of the rotated footprint,
            // pointing OUTWARD toward the top Edge.Cuts. That is the only orientation
            // that makes cable insertion physically possible when the board is mounted
            // with the top edge facing the user.
            //
            // NOTE: The pre-shift Y here is intentionally left at min_y + half_h; the
            // definitive edge-flush snap is applied deterministically AFTER the global
            // Y-shift in place_with_outline() using closed-form geometry.
            let x = min_x + width_nm / 4 + (usb_count * synth_geometry::mm_to_nm(15.0));
            let y = min_y + half_h;
            targets.insert(
                comp.id,
                FloorplanTarget {
                    point: Point::new(x, y),
                    rotation: Rotation::OneEighty,
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
