// SPDX-License-Identifier: Apache-2.0

//! Physical footprint pad geometry and side classification.
//!
//! Phase 1 (Enhancement Specification §6): Make the placement engine aware of
//! actual physical IC pin locations, dimensions, and side orientation on the board.
//!
//! Grounded in the physical footprint (KiCad `*.kicad_mod` or registry part definitions),
//! not schematic symbol conventions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rotation};
use synth_ir::{Board, Component, ComponentId, PinId};
use synth_layout::{kicad_footprint_loader, pcb_courtyard_geometry_for_part};

/// Cardinal and spatial classification of a pad relative to its component center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PadSide {
    North,
    South,
    East,
    West,
    Center,
    Corner,
    Ambiguous,
}

impl PadSide {
    /// Return human-readable label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::North => "North",
            Self::South => "South",
            Self::East => "East",
            Self::West => "West",
            Self::Center => "Center",
            Self::Corner => "Corner",
            Self::Ambiguous => "Ambiguous",
        }
    }

    /// Rotate this side according to the component's board rotation.
    ///
    /// Synth/KiCad convention: board Y increases downward.
    /// A positive 90 deg rotation (counter-clockwise on screen) maps (x, y) -> (y, -x).
    /// North (negative Y) -> West (negative X).
    /// East (positive X) -> North (negative Y).
    /// South (positive Y) -> East (positive X).
    /// West (negative X) -> South (positive Y).
    #[must_use]
    pub fn rotate(self, rotation: Rotation) -> Self {
        match self {
            Self::Center | Self::Ambiguous | Self::Corner => self,
            Self::North => match rotation {
                Rotation::Zero => Self::North,
                Rotation::Ninety => Self::West,
                Rotation::OneEighty => Self::South,
                Rotation::TwoSeventy => Self::East,
            },
            Self::South => match rotation {
                Rotation::Zero => Self::South,
                Rotation::Ninety => Self::East,
                Rotation::OneEighty => Self::North,
                Rotation::TwoSeventy => Self::West,
            },
            Self::East => match rotation {
                Rotation::Zero => Self::East,
                Rotation::Ninety => Self::North,
                Rotation::OneEighty => Self::West,
                Rotation::TwoSeventy => Self::South,
            },
            Self::West => match rotation {
                Rotation::Zero => Self::West,
                Rotation::Ninety => Self::South,
                Rotation::OneEighty => Self::East,
                Rotation::TwoSeventy => Self::North,
            },
        }
    }

    /// Whether this is one of the four cardinal perimeter sides.
    #[must_use]
    pub fn is_cardinal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::East | Self::West)
    }
}

/// A physical pad on a footprint, with local and board-relative coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhysicalPad {
    /// Pad designator/number (e.g. "1", "A1", "EP").
    pub number: String,
    /// Associated pin name in the design if resolved (e.g. "VDD", "TXD").
    pub pin_name: Option<String>,
    /// Pad center offset from the footprint origin in millimeters.
    pub local_origin_mm: (f64, f64),
    /// Pad center offset relative to the courtyard center in nanometers (rotation 0).
    pub rel_courtyard_nm: Point,
    /// Pad size in millimeters (width, height).
    pub size_mm: (f64, f64),
    /// Spatial classification relative to the unrotated component body.
    pub local_side: PadSide,
    /// Electrical pin type if known (e.g. "power", "input", "output", "bidirectional").
    pub electrical_type: Option<String>,
    /// True if pad is non-plated through hole.
    pub is_npth: bool,
}

impl PhysicalPad {
    /// Calculate the pad's board-level coordinate given the placed component's
    /// courtyard center and rotation.
    #[must_use]
    pub fn board_point(&self, courtyard_center: Point, rotation: Rotation) -> Point {
        let (rx, ry) = rotation.rotate_offset(self.rel_courtyard_nm.x_nm, self.rel_courtyard_nm.y_nm);
        Point::new(courtyard_center.x_nm + rx, courtyard_center.y_nm + ry)
    }

    /// Return the pad's side relative to the board after component rotation.
    #[must_use]
    pub fn board_side(&self, rotation: Rotation) -> PadSide {
        self.local_side.rotate(rotation)
    }
}

/// Component footprint pad geometry container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentPadGeometry {
    pub component_id: ComponentId,
    pub refdes: String,
    pub part_id: Option<String>,
    /// Footprint width and height in nanometers.
    pub courtyard_width_nm: i64,
    pub courtyard_height_nm: i64,
    /// Origin offset from the courtyard center in nanometers.
    pub origin_offset_nm: Point,
    /// All physical pads belonging to this component.
    pub pads: Vec<PhysicalPad>,
}

impl ComponentPadGeometry {
    /// Find a pad by its pin number / designator.
    #[must_use]
    pub fn find_by_number(&self, number: &str) -> Option<&PhysicalPad> {
        self.pads.iter().find(|p| p.number == number)
    }

    /// Find a pad by its logical pin name.
    #[must_use]
    pub fn find_by_pin_name(&self, name: &str) -> Option<&PhysicalPad> {
        self.pads
            .iter()
            .find(|p| p.pin_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }

    /// Find a pad by logical [`PinId`] in the component's part definition.
    #[must_use]
    pub fn find_by_pin_id(&self, component: &Component, pin_id: PinId) -> Option<&PhysicalPad> {
        if let Some(part) = &component.part {
            if let Some(pin) = part.pins.get(pin_id.0 as usize) {
                if let Some(pad) = self.find_by_number(&pin.number.0) {
                    return Some(pad);
                }
                if let Some(pad) = self.find_by_pin_name(&pin.name) {
                    return Some(pad);
                }
            }
        }
        let num_str = format!("{}", pin_id.0 + 1);
        if let Some(pad) = self.find_by_number(&num_str) {
            return Some(pad);
        }
        let num_str0 = format!("{}", pin_id.0);
        self.find_by_number(&num_str0)
    }

    /// Enumerate pads on a specific local side.
    #[must_use]
    pub fn pads_on_local_side(&self, side: PadSide) -> Vec<&PhysicalPad> {
        self.pads.iter().filter(|p| p.local_side == side).collect()
    }

    /// Distribution count of pads across sides.
    #[must_use]
    pub fn side_distribution(&self) -> BTreeMap<PadSide, usize> {
        let mut dist = BTreeMap::new();
        for pad in &self.pads {
            *dist.entry(pad.local_side).or_insert(0) += 1;
        }
        dist
    }
}

/// Classify a pad coordinate relative to courtyard dimensions into a [`PadSide`].
///
/// `dx_nm` and `dy_nm` are offsets from the courtyard center in nanometers.
/// In board/KiCad conventions, Y increases downward (negative is North).
#[must_use]
pub fn classify_pad_side(dx_nm: i64, dy_nm: i64, width_nm: i64, height_nm: i64) -> PadSide {
    if width_nm <= 0 || height_nm <= 0 {
        return PadSide::Ambiguous;
    }

    let half_w = (width_nm as f64 / 2.0).max(1.0);
    let half_h = (height_nm as f64 / 2.0).max(1.0);

    let norm_x = dx_nm as f64 / half_w;
    let norm_y = dy_nm as f64 / half_h;

    let abs_x = norm_x.abs();
    let abs_y = norm_y.abs();

    // Central thermal / ground pads
    if abs_x <= 0.35 && abs_y <= 0.35 {
        return PadSide::Center;
    }

    // Corner threshold: pads situated near diagonal extremities
    if abs_x >= 0.75 && abs_y >= 0.75 && (abs_x - abs_y).abs() < 0.20 {
        return PadSide::Corner;
    }

    // Dominant side determination
    if abs_y > abs_x {
        if norm_y < -0.15 {
            PadSide::North
        } else if norm_y > 0.15 {
            PadSide::South
        } else {
            PadSide::Ambiguous
        }
    } else if abs_x > abs_y {
        if norm_x > 0.15 {
            PadSide::East
        } else if norm_x < -0.15 {
            PadSide::West
        } else {
            PadSide::Ambiguous
        }
    } else {
        PadSide::Ambiguous
    }
}

/// Extract physical pad geometry for a single component on the board.
#[must_use]
pub fn extract_component_pad_geometry(component: &Component) -> ComponentPadGeometry {
    let (origin_x_mm, origin_y_mm, w_mm, h_mm) = component.part.as_ref().map_or_else(
        || (0.0, 0.0, 5.0, 5.0),
        |part| {
            let (origin, (w, h)) = pcb_courtyard_geometry_for_part(part);
            (origin.0, origin.1, w, h)
        },
    );

    let origin_offset_nm = Point::new(mm_to_nm(origin_x_mm), mm_to_nm(origin_y_mm));
    let width_nm = mm_to_nm(w_mm);
    let height_nm = mm_to_nm(h_mm);

    let pins = component
        .part
        .as_ref()
        .map_or(&[][..], |p| p.pins.as_slice());

    let footprint_pads = component
        .part
        .as_ref()
        .and_then(|p| p.kicad_footprint.as_deref())
        .and_then(kicad_footprint_loader::pads);

    let mut pads = Vec::new();

    if let Some(loaded_pads) = footprint_pads {
        for pad in loaded_pads {
            let rel_x = mm_to_nm(pad.center_mm.0) - origin_offset_nm.x_nm;
            let rel_y = mm_to_nm(pad.center_mm.1) - origin_offset_nm.y_nm;

            let side = classify_pad_side(rel_x, rel_y, width_nm, height_nm);

            let matching_pin = pins.iter().find(|p| p.number.0 == pad.number);
            let pin_name = matching_pin.map(|p| p.name.clone());
            let electrical_type = matching_pin.map(|p| format!("{:?}", p.electrical_type));

            pads.push(PhysicalPad {
                number: pad.number,
                pin_name,
                local_origin_mm: pad.center_mm,
                rel_courtyard_nm: Point::new(rel_x, rel_y),
                size_mm: pad.size_mm,
                local_side: side,
                electrical_type,
                is_npth: pad.is_npth,
            });
        }
    } else {
        // Fallback synthetic geometry for components without loaded footprint files
        let pad_count = pins.len();
        for (idx, pin) in pins.iter().enumerate() {
            let (rel_x, rel_y, side) = if pad_count == 2 {
                if idx == 0 {
                    (-width_nm / 3, 0, PadSide::West)
                } else {
                    (width_nm / 3, 0, PadSide::East)
                }
            } else {
                // Synthesize 4-side perimeter distribution
                let side_idx = idx % 4;
                match side_idx {
                    0 => (0, -height_nm / 3, PadSide::North),
                    1 => (width_nm / 3, 0, PadSide::East),
                    2 => (0, height_nm / 3, PadSide::South),
                    _ => (-width_nm / 3, 0, PadSide::West),
                }
            };

            pads.push(PhysicalPad {
                number: pin.number.0.clone(),
                pin_name: Some(pin.name.clone()),
                local_origin_mm: (nm_to_mm(rel_x), nm_to_mm(rel_y)),
                rel_courtyard_nm: Point::new(rel_x, rel_y),
                size_mm: (0.5, 0.5),
                local_side: side,
                electrical_type: Some(format!("{:?}", pin.electrical_type)),
                is_npth: false,
            });
        }
    }

    ComponentPadGeometry {
        component_id: component.id,
        refdes: component.refdes.clone(),
        part_id: component.part.as_ref().map(|p| p.id.0.clone()),
        courtyard_width_nm: width_nm,
        courtyard_height_nm: height_nm,
        origin_offset_nm,
        pads,
    }
}

/// Extract physical pad geometries for all components on the board.
#[must_use]
pub fn extract_all_pad_geometries(board: &Board) -> BTreeMap<ComponentId, ComponentPadGeometry> {
    let mut map = BTreeMap::new();
    for comp in &board.components {
        map.insert(comp.id, extract_component_pad_geometry(comp));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_side_classification_cardinal() {
        let w = mm_to_nm(10.0);
        let h = mm_to_nm(10.0);

        // North: negative Y
        assert_eq!(classify_pad_side(0, mm_to_nm(-4.0), w, h), PadSide::North);
        // South: positive Y
        assert_eq!(classify_pad_side(0, mm_to_nm(4.0), w, h), PadSide::South);
        // East: positive X
        assert_eq!(classify_pad_side(mm_to_nm(4.0), 0, w, h), PadSide::East);
        // West: negative X
        assert_eq!(classify_pad_side(mm_to_nm(-4.0), 0, w, h), PadSide::West);
    }

    #[test]
    fn test_pad_side_classification_center_thermal() {
        let w = mm_to_nm(10.0);
        let h = mm_to_nm(10.0);

        assert_eq!(classify_pad_side(0, 0, w, h), PadSide::Center);
        assert_eq!(classify_pad_side(mm_to_nm(0.5), mm_to_nm(0.5), w, h), PadSide::Center);
    }

    #[test]
    fn test_pad_side_rotation_transform() {
        // North rotated 90 deg -> West
        assert_eq!(PadSide::North.rotate(Rotation::Ninety), PadSide::West);
        // North rotated 180 deg -> South
        assert_eq!(PadSide::North.rotate(Rotation::OneEighty), PadSide::South);
        // North rotated 270 deg -> East
        assert_eq!(PadSide::North.rotate(Rotation::TwoSeventy), PadSide::East);

        // East rotated 90 deg -> North
        assert_eq!(PadSide::East.rotate(Rotation::Ninety), PadSide::North);
        // South rotated 90 deg -> East
        assert_eq!(PadSide::South.rotate(Rotation::Ninety), PadSide::East);
        // West rotated 90 deg -> South
        assert_eq!(PadSide::West.rotate(Rotation::Ninety), PadSide::South);
    }

    #[test]
    fn test_physical_pad_board_coordinate_transform() {
        let pad = PhysicalPad {
            number: "1".to_string(),
            pin_name: Some("TXD".to_string()),
            local_origin_mm: (2.0, 0.0),
            rel_courtyard_nm: Point::new(mm_to_nm(2.0), 0),
            size_mm: (0.4, 0.8),
            local_side: PadSide::East,
            electrical_type: None,
            is_npth: false,
        };

        let center = Point::new(mm_to_nm(50.0), mm_to_nm(50.0));

        // At rotation 0: (50 + 2, 50) = (52, 50)
        let pt0 = pad.board_point(center, Rotation::Zero);
        assert_eq!(pt0.x_nm, mm_to_nm(52.0));
        assert_eq!(pt0.y_nm, mm_to_nm(50.0));

        // At rotation 90: (x, y) -> (y, -x) = (0, -2) => (50, 48)
        let pt90 = pad.board_point(center, Rotation::Ninety);
        assert_eq!(pt90.x_nm, mm_to_nm(50.0));
        assert_eq!(pt90.y_nm, mm_to_nm(48.0));
    }

    #[test]
    fn test_qfp_four_pad_rows() {
        let w = mm_to_nm(12.0);
        let h = mm_to_nm(12.0);
        // North row
        assert_eq!(classify_pad_side(0, mm_to_nm(-5.0), w, h), PadSide::North);
        // South row
        assert_eq!(classify_pad_side(0, mm_to_nm(5.0), w, h), PadSide::South);
        // East row
        assert_eq!(classify_pad_side(mm_to_nm(5.0), 0, w, h), PadSide::East);
        // West row
        assert_eq!(classify_pad_side(mm_to_nm(-5.0), 0, w, h), PadSide::West);
    }

    #[test]
    fn test_soic_two_pad_rows() {
        let w = mm_to_nm(6.0);
        let h = mm_to_nm(10.0);
        // West row (pins 1..4)
        assert_eq!(classify_pad_side(mm_to_nm(-2.5), mm_to_nm(-3.0), w, h), PadSide::West);
        assert_eq!(classify_pad_side(mm_to_nm(-2.5), mm_to_nm(3.0), w, h), PadSide::West);
        // East row (pins 5..8)
        assert_eq!(classify_pad_side(mm_to_nm(2.5), mm_to_nm(-3.0), w, h), PadSide::East);
        assert_eq!(classify_pad_side(mm_to_nm(2.5), mm_to_nm(3.0), w, h), PadSide::East);
    }

    #[test]
    fn test_qfn_with_exposed_thermal_pad() {
        let w = mm_to_nm(5.0);
        let h = mm_to_nm(5.0);
        // Center thermal pad
        assert_eq!(classify_pad_side(0, 0, w, h), PadSide::Center);
        assert_eq!(classify_pad_side(mm_to_nm(0.2), mm_to_nm(0.2), w, h), PadSide::Center);
        // Perimeter pads
        assert_eq!(classify_pad_side(0, mm_to_nm(-2.2), w, h), PadSide::North);
        assert_eq!(classify_pad_side(0, mm_to_nm(2.2), w, h), PadSide::South);
        assert_eq!(classify_pad_side(mm_to_nm(2.2), 0, w, h), PadSide::East);
        assert_eq!(classify_pad_side(mm_to_nm(-2.2), 0, w, h), PadSide::West);
    }

    #[test]
    fn test_asymmetric_connector_origin() {
        let courtyard_center = Point::new(mm_to_nm(40.0), mm_to_nm(30.0));
        let origin_offset_nm = Point::new(mm_to_nm(-10.0), mm_to_nm(-5.0));
        let pad_local_mm = (0.0, 0.0);
        let rel_x = mm_to_nm(pad_local_mm.0) - origin_offset_nm.x_nm;
        let rel_y = mm_to_nm(pad_local_mm.1) - origin_offset_nm.y_nm;

        let pad = PhysicalPad {
            number: "1".to_string(),
            pin_name: Some("VBUS".to_string()),
            local_origin_mm: pad_local_mm,
            rel_courtyard_nm: Point::new(rel_x, rel_y),
            size_mm: (1.0, 2.0),
            local_side: PadSide::West,
            electrical_type: None,
            is_npth: false,
        };

        let pt0 = pad.board_point(courtyard_center, Rotation::Zero);
        assert_eq!(pt0.x_nm, mm_to_nm(50.0));
        assert_eq!(pt0.y_nm, mm_to_nm(35.0));

        let pt180 = pad.board_point(courtyard_center, Rotation::OneEighty);
        assert_eq!(pt180.x_nm, mm_to_nm(30.0));
        assert_eq!(pt180.y_nm, mm_to_nm(25.0));
        assert_eq!(pad.board_side(Rotation::OneEighty), PadSide::East);
    }

    #[test]
    fn test_rotation_quadrants() {
        let pad_north = PhysicalPad {
            number: "1".to_string(),
            pin_name: None,
            local_origin_mm: (0.0, -5.0),
            rel_courtyard_nm: Point::new(0, mm_to_nm(-5.0)),
            size_mm: (0.5, 0.5),
            local_side: PadSide::North,
            electrical_type: None,
            is_npth: false,
        };

        assert_eq!(pad_north.board_side(Rotation::Zero), PadSide::North);
        assert_eq!(pad_north.board_side(Rotation::Ninety), PadSide::West);
        assert_eq!(pad_north.board_side(Rotation::OneEighty), PadSide::South);
        assert_eq!(pad_north.board_side(Rotation::TwoSeventy), PadSide::East);
    }
}
