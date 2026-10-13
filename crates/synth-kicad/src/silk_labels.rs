// SPDX-License-Identifier: Apache-2.0

//! Reference-label placement for the PCB export.
//!
//! A part's reference label used to sit at one fixed offset from the part,
//! whatever was already there: it could land on a neighbour, on another label,
//! or past the board edge. [`place_labels`] keeps that offset when it is free
//! and otherwise tries the other three sides, so a board with no conflict
//! exports exactly as before.

use std::collections::HashMap;

use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rect, Rotation};
use synth_ir::ComponentId;

/// Gap between a footprint's dimensions and its label, the long-standing default.
pub(crate) const DEFAULT_MARGIN_MM: f64 = 3.5;
/// Width of one character at the 1 mm font the export uses.
const CHAR_WIDTH_MM: f64 = 1.0;
/// Height of a line of text, with room for the stroke.
const TEXT_HEIGHT_MM: f64 = 1.3;
/// Gap between a part's courtyard and an alternative label position.
const SIDE_GAP_MM: f64 = 0.3;
/// A label keeps this far inside the board edge.
const EDGE_CLEARANCE_MM: f64 = 0.2;

/// What the label placer needs to know about one part.
#[derive(Debug, Clone)]
pub(crate) struct LabelPart {
    pub id: ComponentId,
    pub refdes: String,
    /// Footprint origin in board coordinates.
    pub origin: Point,
    pub rotation: Rotation,
    /// Courtyard in board coordinates.
    pub courtyard: Rect,
    /// Half of the unrotated footprint width and height, in millimetres.
    pub half_extent_mm: (f64, f64),
}

/// Where a reference label goes: a footprint-local offset and a text angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LabelPos {
    pub local_x_mm: f64,
    pub local_y_mm: f64,
    pub angle_deg: f64,
}

/// The fixed offset the export has always used for a part's label.
pub(crate) fn default_label(part: &LabelPart) -> LabelPos {
    let (half_w, half_h) = part.half_extent_mm;
    let m = DEFAULT_MARGIN_MM;
    let (dx, dy, angle_deg) = match part.rotation {
        Rotation::Zero => (0.0, -(half_h + m), 0.0),
        Rotation::Ninety => (-(half_w + m), 0.0, -90.0),
        Rotation::OneEighty => (0.0, half_h + m, 180.0),
        Rotation::TwoSeventy => (half_w + m, 0.0, 90.0),
    };
    LabelPos {
        local_x_mm: dx,
        local_y_mm: dy,
        angle_deg,
    }
}

/// Width of a part's label in nanometres.
#[allow(clippy::cast_precision_loss)] // a designator is a handful of characters
fn text_width_nm(refdes: &str) -> i64 {
    mm_to_nm(refdes.chars().count() as f64 * CHAR_WIDTH_MM)
}

/// Choose a label position for every part.
///
/// Parts are visited in order, each taking the first candidate that fits:
/// the default, then above, below, left and right of its courtyard. A
/// candidate fits when it stays inside the board, clear of every other part's
/// courtyard and clear of the labels already placed. A part with no fitting
/// candidate keeps the default.
pub(crate) fn place_labels(parts: &[LabelPart], outline: Rect) -> HashMap<ComponentId, LabelPos> {
    let mut placed = HashMap::new();
    let mut taken: Vec<Rect> = Vec::new();
    for part in parts {
        let others = || {
            parts
                .iter()
                .filter(|o| o.id != part.id)
                .map(|o| o.courtyard)
        };
        let fits = |pos: LabelPos| {
            let b = label_box(part, pos);
            inside(outline, b)
                && !others().any(|c| c.intersects(&b))
                && !taken.iter().any(|t| t.intersects(&b))
        };
        let default = default_label(part);
        let pos = std::iter::once(default)
            .chain(side_candidates(part))
            .find(|&p| fits(p))
            .unwrap_or(default);
        taken.push(label_box(part, pos));
        placed.insert(part.id, pos);
    }
    placed
}

/// Horizontal-text positions above, below, left and right of the courtyard.
fn side_candidates(part: &LabelPart) -> [LabelPos; 4] {
    let w = text_width_nm(&part.refdes);
    let h = mm_to_nm(TEXT_HEIGHT_MM);
    let gap = mm_to_nm(SIDE_GAP_MM);
    let c = part.courtyard;
    let mid_x = (c.min.x_nm + c.max.x_nm) / 2;
    let mid_y = (c.min.y_nm + c.max.y_nm) / 2;
    [
        (mid_x, c.min.y_nm - gap - h / 2),
        (mid_x, c.max.y_nm + gap + h / 2),
        (c.min.x_nm - gap - w / 2, mid_y),
        (c.max.x_nm + gap + w / 2, mid_y),
    ]
    .map(|(x, y)| local_label(part, Point::new(x, y)))
}

/// A horizontal label centred on a board-coordinate point.
fn local_label(part: &LabelPart, centre: Point) -> LabelPos {
    let inverse = match part.rotation {
        Rotation::Zero => Rotation::Zero,
        Rotation::Ninety => Rotation::TwoSeventy,
        Rotation::OneEighty => Rotation::OneEighty,
        Rotation::TwoSeventy => Rotation::Ninety,
    };
    let (x, y) = inverse.rotate_offset(
        centre.x_nm - part.origin.x_nm,
        centre.y_nm - part.origin.y_nm,
    );
    LabelPos {
        local_x_mm: nm_to_mm(x),
        local_y_mm: nm_to_mm(y),
        angle_deg: 0.0,
    }
}

/// True when `inner` lies within `outer`, clear of its edge.
fn inside(outer: Rect, inner: Rect) -> bool {
    let edge = mm_to_nm(EDGE_CLEARANCE_MM);
    inner.min.x_nm >= outer.min.x_nm + edge
        && inner.min.y_nm >= outer.min.y_nm + edge
        && inner.max.x_nm <= outer.max.x_nm - edge
        && inner.max.y_nm <= outer.max.y_nm - edge
}

/// Board-coordinate box a label occupies.
fn label_box(part: &LabelPart, pos: LabelPos) -> Rect {
    let (dx, dy) = part
        .rotation
        .rotate_offset(mm_to_nm(pos.local_x_mm), mm_to_nm(pos.local_y_mm));
    let centre = Point::new(part.origin.x_nm + dx, part.origin.y_nm + dy);
    let w = text_width_nm(&part.refdes);
    let h = mm_to_nm(TEXT_HEIGHT_MM);
    let (w, h) = if pos.angle_deg.abs() == 90.0 {
        (h, w)
    } else {
        (w, h)
    };
    Rect::new(
        Point::new(centre.x_nm - w / 2, centre.y_nm - h / 2),
        Point::new(centre.x_nm + w / 2, centre.y_nm + h / 2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(x: f64, y: f64) -> Point {
        Point::new(mm_to_nm(x), mm_to_nm(y))
    }

    fn rect(cx: f64, cy: f64, w: f64, h: f64) -> Rect {
        Rect::new(
            mm(cx - w / 2.0, cy - h / 2.0),
            mm(cx + w / 2.0, cy + h / 2.0),
        )
    }

    /// A 2 x 1 mm part at (`cx`, `cy`), unrotated.
    fn resistor(id: u32, refdes: &str, cx: f64, cy: f64) -> LabelPart {
        LabelPart {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            origin: mm(cx, cy),
            rotation: Rotation::Zero,
            courtyard: rect(cx, cy, 2.0, 1.0),
            half_extent_mm: (1.0, 0.5),
        }
    }

    fn outline() -> Rect {
        Rect::new(mm(0.0, 0.0), mm(40.0, 40.0))
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersects(&b)
    }

    #[test]
    fn a_label_with_room_keeps_the_default_position() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let placed = place_labels(std::slice::from_ref(&part), outline());
        assert_eq!(placed[&part.id], default_label(&part));
    }

    #[test]
    fn a_label_that_lands_on_a_neighbour_moves_to_a_free_side() {
        let part = resistor(1, "R1", 20.0, 20.0);
        // The default label sits 4 mm above the part; a wide neighbour covers it.
        let mut neighbour = resistor(2, "U1", 20.0, 16.0);
        neighbour.courtyard = rect(20.0, 16.0, 8.0, 2.0);
        let parts = [part.clone(), neighbour.clone()];
        let placed = place_labels(&parts, outline());
        let pos = placed[&part.id];
        assert_ne!(pos, default_label(&part), "the default is covered");
        let b = label_box(&part, pos);
        assert!(
            !overlaps(b, neighbour.courtyard),
            "label {b:?} on the neighbour"
        );
        assert!(inside(outline(), b), "label {b:?} off the board");
    }

    #[test]
    fn a_label_past_the_board_edge_moves_inside() {
        // 2 mm from the top edge, so the default label (4 mm above) is off the board.
        let part = resistor(1, "R1", 20.0, 2.0);
        let placed = place_labels(std::slice::from_ref(&part), outline());
        let pos = placed[&part.id];
        assert_ne!(pos, default_label(&part));
        assert!(inside(outline(), label_box(&part, pos)));
    }

    #[test]
    fn two_labels_do_not_overlap_each_other() {
        // 3 mm apart, so their six-character default labels collide.
        let a = resistor(1, "R_TNOW", 20.0, 20.0);
        let b = resistor(2, "R9_TNO", 23.0, 20.0);
        let parts = [a.clone(), b.clone()];
        let placed = place_labels(&parts, outline());
        let box_a = label_box(&a, placed[&a.id]);
        let box_b = label_box(&b, placed[&b.id]);
        assert!(
            !overlaps(box_a, box_b),
            "labels {box_a:?} and {box_b:?} overlap"
        );
    }

    #[test]
    fn a_rotated_part_gets_a_label_inside_the_board() {
        // Rotated a quarter turn, its default label sits below the part: here
        // that is past the bottom edge.
        let mut part = resistor(1, "J2", 20.0, 38.0);
        part.rotation = Rotation::Ninety;
        part.courtyard = rect(20.0, 38.0, 1.0, 2.0);
        let placed = place_labels(std::slice::from_ref(&part), outline());
        let pos = placed[&part.id];
        assert_ne!(pos, default_label(&part));
        assert!(inside(outline(), label_box(&part, pos)));
    }

    #[test]
    fn a_label_is_on_the_board_and_clear_of_its_own_part_at_every_rotation() {
        let spots = [
            (2.0, 20.0),
            (38.0, 20.0),
            (20.0, 2.0),
            (20.0, 38.0),
            (3.0, 3.0),
            (37.0, 37.0),
        ];
        for rotation in [
            Rotation::Zero,
            Rotation::Ninety,
            Rotation::OneEighty,
            Rotation::TwoSeventy,
        ] {
            for (cx, cy) in spots {
                let mut part = resistor(1, "J2", cx, cy);
                part.rotation = rotation;
                if rotation.swaps_extents() {
                    part.courtyard = rect(cx, cy, 1.0, 2.0);
                }
                let placed = place_labels(std::slice::from_ref(&part), outline());
                let b = label_box(&part, placed[&part.id]);
                assert!(
                    inside(outline(), b),
                    "{rotation:?} at ({cx}, {cy}): {b:?} off the board"
                );
                assert!(
                    !overlaps(b, part.courtyard),
                    "{rotation:?} at ({cx}, {cy}): {b:?} on its own part"
                );
            }
        }
    }

    #[test]
    fn when_nothing_fits_the_default_is_kept() {
        // A 6 mm board cannot hold the label anywhere.
        let tiny = Rect::new(mm(0.0, 0.0), mm(6.0, 6.0));
        let part = resistor(1, "R_LONGNAME", 3.0, 3.0);
        let placed = place_labels(std::slice::from_ref(&part), tiny);
        assert_eq!(placed[&part.id], default_label(&part));
    }
}
