// SPDX-License-Identifier: Apache-2.0

//! Reference-label placement for the PCB export.
//!
//! A part's reference label used to sit at one fixed offset from the part,
//! whatever was already there: it could land on a neighbour, on another label,
//! or past the board edge, and it sat well clear of the part. [`place_labels`]
//! puts it right against the courtyard on the first free side and falls back
//! to that offset only when no side is free.

use std::collections::HashMap;

use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rect, Rotation};
use synth_ir::ComponentId;

/// Gap between a footprint's dimensions and its label, the long-standing default.
pub(crate) const DEFAULT_MARGIN_MM: f64 = 3.5;
/// Size of a reference label's font, in millimetres. A little under the 1 mm
/// KiCad starts from, so a label takes less of a crowded board.
pub(crate) const LABEL_FONT_MM: f64 = 0.8;
/// Stroke of a label's letters; KiCad's default is a fifth of the size, which
/// reads heavy on a small label.
pub(crate) const LABEL_STROKE_MM: f64 = 0.12;
/// Width of one character at that font.
const CHAR_WIDTH_MM: f64 = LABEL_FONT_MM;
/// Height of a line of text, with room for the stroke.
const TEXT_HEIGHT_MM: f64 = LABEL_FONT_MM * 1.3;
/// A label keeps this far from a neighbour's courtyard: a footprint draws its
/// silkscreen on and just outside the courtyard, and the fab's silkscreen
/// clearance applies to text and outline alike.
const NEIGHBOUR_CLEARANCE_MM: f64 = 0.3;
/// Gap between a part's courtyard and an alternative label position.
const SIDE_GAP_MM: f64 = 0.35;
/// A label keeps this far inside the board edge.
const EDGE_CLEARANCE_MM: f64 = 0.2;

/// A side of a part's courtyard, in board axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Above,
    Below,
    Left,
    Right,
}

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
    /// Side the part file prefers for the label, tried before the default.
    pub side: Option<Side>,
    /// The part file asks for no label on the silkscreen.
    pub hidden: bool,
}

/// A design's say over one label, from the placement sidecar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LabelOverride {
    /// Label centre relative to the courtyard centre, in board axes.
    pub offset_mm: Option<(f64, f64)>,
    pub angle_deg: f64,
    /// Overrides the part file's `hidden` when set.
    pub hide: Option<bool>,
    /// A hard offset is used as given (kept on the board); a soft one is tried first.
    pub hard: bool,
}

/// Where a reference label goes: a footprint-local offset and a text angle.
/// The angle is in board axes, which is how KiCad reads a footprint text's
/// angle, so it does not change when the part is rotated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LabelPos {
    pub local_x_mm: f64,
    pub local_y_mm: f64,
    pub angle_deg: f64,
    /// Emit the label hidden.
    pub hidden: bool,
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
        hidden: false,
    }
}

/// Width of a part's label in nanometres.
#[allow(clippy::cast_precision_loss)] // a designator is a handful of characters
fn text_width_nm(refdes: &str) -> i64 {
    mm_to_nm(refdes.chars().count() as f64 * CHAR_WIDTH_MM)
}

/// Choose a label position for every part.
///
/// A label the design or the part file hides takes no room. A hard sidecar
/// offset is used as given, skipping the collision checks but pulled back
/// inside the board. Otherwise each part takes the first candidate
/// that fits: a soft sidecar offset, the part file's preferred side, then
/// above, below, left and right of its courtyard, then the default. A
/// candidate fits when it stays inside the board, clear of every other part's
/// courtyard and clear of the labels already placed. A part with no fitting
/// candidate takes the first candidate that at least stays on the board, clear
/// of the labels already placed; only when none does is the default pulled
/// inside the board.
pub(crate) fn place_labels(
    parts: &[LabelPart],
    outline: Rect,
    overrides: &HashMap<ComponentId, LabelOverride>,
) -> HashMap<ComponentId, LabelPos> {
    let mut placed = HashMap::new();
    let mut taken: Vec<Rect> = Vec::new();
    for part in parts {
        let design = overrides.get(&part.id);
        let default = default_label(part);
        if design.and_then(|o| o.hide).unwrap_or(part.hidden) {
            placed.insert(
                part.id,
                LabelPos {
                    hidden: true,
                    ..default
                },
            );
            continue;
        }
        let hard = design.is_some_and(|o| o.hard);
        let from_design = design.and_then(|o| {
            let (dx, dy) = o.offset_mm?;
            let c = part.courtyard;
            let centre = Point::new(
                (c.min.x_nm + c.max.x_nm) / 2 + mm_to_nm(dx),
                (c.min.y_nm + c.max.y_nm) / 2 + mm_to_nm(dy),
            );
            Some(local_label(part, centre, o.angle_deg))
        });
        let others = || {
            parts
                .iter()
                .filter(|o| o.id != part.id)
                .map(|o| grown(o.courtyard, NEIGHBOUR_CLEARANCE_MM))
        };
        // How much of a candidate's box sits on neighbours' courtyards and
        // already placed labels, in square nanometres: zero means it fits.
        let clash = |pos: LabelPos| -> i128 {
            let b = label_box(part, pos);
            others()
                .map(|c| overlap_area(&c, &b))
                .chain(taken.iter().map(|t| overlap_area(t, &b)))
                .sum()
        };
        let pos = match from_design {
            Some(forced) if hard => pulled_inside(part, outline, forced),
            _ => {
                let candidates: Vec<LabelPos> = from_design
                    .into_iter()
                    .chain(part.side.map(|s| side_candidate(part, s)))
                    .chain(
                        [Side::Above, Side::Below, Side::Left, Side::Right]
                            .map(|s| side_candidate(part, s)),
                    )
                    .chain(std::iter::once(default))
                    .chain(extra_candidates(part))
                    .collect();
                let on_board = |p: &LabelPos| inside(outline, label_box(part, *p));
                // The first candidate that is on the board and clear of everything,
                // in the order of preference; failing that the one on the board
                // that clashes least, so a crowded spot is the least crowded one.
                candidates
                    .iter()
                    .copied()
                    .find(|p| on_board(p) && clash(*p) == 0)
                    .or_else(|| {
                        candidates
                            .iter()
                            .copied()
                            .filter(on_board)
                            .min_by_key(|p| clash(*p))
                    })
                    .unwrap_or_else(|| pulled_inside(part, outline, default))
            }
        };
        taken.push(label_box(part, pos));
        placed.insert(part.id, pos);
    }
    placed
}

/// A horizontal label just outside one side of the courtyard.
fn side_candidate(part: &LabelPart, side: Side) -> LabelPos {
    let width = text_width_nm(&part.refdes);
    let height = mm_to_nm(TEXT_HEIGHT_MM);
    let gap = mm_to_nm(SIDE_GAP_MM);
    let court = part.courtyard;
    let mid_x = (court.min.x_nm + court.max.x_nm) / 2;
    let mid_y = (court.min.y_nm + court.max.y_nm) / 2;
    let (x, y) = match side {
        Side::Above => (mid_x, court.min.y_nm - gap - height / 2),
        Side::Below => (mid_x, court.max.y_nm + gap + height / 2),
        Side::Left => (court.min.x_nm - gap - width / 2, mid_y),
        Side::Right => (court.max.x_nm + gap + width / 2, mid_y),
    };
    local_label(part, Point::new(x, y), 0.0)
}

/// `rect` grown by `by_mm` on every side.
fn grown(rect: Rect, by_mm: f64) -> Rect {
    let by = mm_to_nm(by_mm);
    Rect::new(
        Point::new(rect.min.x_nm - by, rect.min.y_nm - by),
        Point::new(rect.max.x_nm + by, rect.max.y_nm + by),
    )
}

/// The area two boxes share, in square nanometres; zero when they do not meet.
fn overlap_area(a: &Rect, b: &Rect) -> i128 {
    let w = (a.max.x_nm.min(b.max.x_nm) - a.min.x_nm.max(b.min.x_nm)).max(0);
    let h = (a.max.y_nm.min(b.max.y_nm) - a.min.y_nm.max(b.min.y_nm)).max(0);
    i128::from(w) * i128::from(h)
}

/// More places than the four sides: the label slid along each side, set on the
/// corners, and turned a quarter beside a tall part, for a part whose four
/// plain sides are all taken.
fn extra_candidates(part: &LabelPart) -> Vec<LabelPos> {
    let width = text_width_nm(&part.refdes);
    let height = mm_to_nm(TEXT_HEIGHT_MM);
    let gap = mm_to_nm(SIDE_GAP_MM);
    let c = part.courtyard;
    let (mid_x, mid_y) = ((c.min.x_nm + c.max.x_nm) / 2, (c.min.y_nm + c.max.y_nm) / 2);
    let mut out = Vec::new();
    // Along a side: shifted toward either end by a third of the length, and by
    // half a text width past the corner.
    for k in [-1_i64, 1] {
        let slide_x = k * ((c.max.x_nm - c.min.x_nm) / 3 + width / 4);
        let slide_y = k * ((c.max.y_nm - c.min.y_nm) / 3 + height);
        out.push(local_label(
            part,
            Point::new(mid_x + slide_x, c.min.y_nm - gap - height / 2),
            0.0,
        ));
        out.push(local_label(
            part,
            Point::new(mid_x + slide_x, c.max.y_nm + gap + height / 2),
            0.0,
        ));
        out.push(local_label(
            part,
            Point::new(c.min.x_nm - gap - width / 2, mid_y + slide_y),
            0.0,
        ));
        out.push(local_label(
            part,
            Point::new(c.max.x_nm + gap + width / 2, mid_y + slide_y),
            0.0,
        ));
    }
    // Corners.
    for (x, y) in [
        (c.min.x_nm - gap - width / 2, c.min.y_nm - gap - height / 2),
        (c.max.x_nm + gap + width / 2, c.min.y_nm - gap - height / 2),
        (c.min.x_nm - gap - width / 2, c.max.y_nm + gap + height / 2),
        (c.max.x_nm + gap + width / 2, c.max.y_nm + gap + height / 2),
    ] {
        out.push(local_label(part, Point::new(x, y), 0.0));
    }
    // Turned a quarter beside the part, where a long label would reach a neighbour
    // across.
    out.push(local_label(
        part,
        Point::new(c.min.x_nm - gap - height / 2, mid_y),
        90.0,
    ));
    out.push(local_label(
        part,
        Point::new(c.max.x_nm + gap + height / 2, mid_y),
        90.0,
    ));
    out
}

/// A label at `angle_deg`, centred on a board-coordinate point.
fn local_label(part: &LabelPart, centre: Point, angle_deg: f64) -> LabelPos {
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
        angle_deg,
        hidden: false,
    }
}

/// `pos` moved the shortest distance that puts its box inside the board.
///
/// A hard offset skips the collision checks but not the board edge. A box
/// larger than the board ends up centred on it.
fn pulled_inside(part: &LabelPart, outline: Rect, pos: LabelPos) -> LabelPos {
    let b = label_box(part, pos);
    let edge = mm_to_nm(EDGE_CLEARANCE_MM);
    let shift = |board_lo: i64, board_hi: i64, box_lo: i64, box_hi: i64| {
        let back = (board_hi - edge - box_hi).min(0);
        let forward = (board_lo + edge - box_lo).max(0);
        back + forward
    };
    let dx = shift(outline.min.x_nm, outline.max.x_nm, b.min.x_nm, b.max.x_nm);
    let dy = shift(outline.min.y_nm, outline.max.y_nm, b.min.y_nm, b.max.y_nm);
    if dx == 0 && dy == 0 {
        return pos;
    }
    let centre = Point::new(
        (b.min.x_nm + b.max.x_nm) / 2 + dx,
        (b.min.y_nm + b.max.y_nm) / 2 + dy,
    );
    local_label(part, centre, pos.angle_deg)
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
    // Axis-aligned bounds of the text rectangle turned by `angle_deg`.
    let (text_w, text_h) = (nm_to_mm(text_width_nm(&part.refdes)), TEXT_HEIGHT_MM);
    let (sin, cos) = pos.angle_deg.to_radians().sin_cos();
    let w = mm_to_nm(text_w * cos.abs() + text_h * sin.abs());
    let h = mm_to_nm(text_w * sin.abs() + text_h * cos.abs());
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
            side: None,
            hidden: false,
        }
    }

    fn place(parts: &[LabelPart], outline: Rect) -> HashMap<ComponentId, LabelPos> {
        place_labels(parts, outline, &HashMap::new())
    }

    fn outline() -> Rect {
        Rect::new(mm(0.0, 0.0), mm(40.0, 40.0))
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersects(&b)
    }

    #[test]
    fn a_label_with_room_sits_next_to_its_part() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let placed = place(std::slice::from_ref(&part), outline());
        assert_eq!(placed[&part.id], side_candidate(&part, Side::Above));
    }

    #[test]
    fn a_label_with_every_side_blocked_falls_back_to_the_default() {
        let part = resistor(1, "R1", 20.0, 20.0);
        // One neighbour over each side spot, none over the default spot 4 mm above.
        let blockers = [
            (2, 20.0, 18.5, 4.0, 1.4),
            (3, 20.0, 21.5, 4.0, 1.4),
            (4, 17.7, 20.0, 2.0, 2.0),
            (5, 22.3, 20.0, 2.0, 2.0),
        ];
        let mut parts = vec![part.clone()];
        for (id, cx, cy, w, h) in blockers {
            let mut blocker = resistor(id, "U1", cx, cy);
            blocker.courtyard = rect(cx, cy, w, h);
            parts.push(blocker);
        }
        let placed = place(&parts, outline());
        assert_eq!(placed[&part.id], default_label(&part));
    }

    /// When every side is blocked and the default leaves the board, the
    /// fallback still has to stay on the board and clear of the labels
    /// already placed — not just take the first crowded side.
    #[test]
    fn a_fallback_label_avoids_the_labels_already_placed() {
        let mut part = resistor(2, "R2", 20.0, 2.0);
        part.courtyard = rect(20.0, 2.0, 2.0, 1.0);

        // Its hard label sits exactly over the below-side candidate.
        let marker = resistor(9, "M1", 20.0, 2.0);
        let hard = LabelOverride {
            hard: true,
            ..soft((0.0, 1.5))
        };

        // Courtyards over the left and right spots; the above candidate is
        // already past the top edge and the default is further out.
        let mut left = resistor(4, "U2", 17.7, 2.0);
        left.courtyard = rect(17.7, 2.0, 2.0, 2.0);
        let mut right = resistor(5, "U3", 22.3, 2.0);
        right.courtyard = rect(22.3, 2.0, 2.0, 2.0);

        let parts = [marker.clone(), part.clone(), left, right];
        let placed = place_labels(&parts, outline(), &overrides(9, hard));
        let pos = placed[&part.id];
        let b = label_box(&part, pos);
        let marker_box = label_box(&marker, placed[&marker.id]);

        // Every plain side is blocked. The fallback takes the spot that touches
        // the least, which is no worse than the left side it used to take.
        let clash = |p: LabelPos| -> i128 {
            let b = label_box(&part, p);
            parts
                .iter()
                .filter(|o| o.id != part.id)
                .map(|o| overlap_area(&grown(o.courtyard, NEIGHBOUR_CLEARANCE_MM), &b))
                .sum()
        };
        assert!(
            clash(pos) <= clash(side_candidate(&part, Side::Left)),
            "the fallback {pos:?} clashes more than the left side"
        );
        assert!(inside(outline(), b), "fallback label {b:?} off the board");
        assert!(
            !overlaps(b, marker_box),
            "fallback label {b:?} overlaps the placed label {marker_box:?}"
        );
    }

    #[test]
    fn a_label_keeps_clear_of_the_margin_round_a_neighbours_courtyard() {
        let part = resistor(1, "R1", 20.0, 20.0);
        // A neighbour whose courtyard ends 0.15 mm above where the label would
        // start: not overlapping, but inside the clearance a silkscreen needs.
        let label_top = 20.0 - 0.5 - SIDE_GAP_MM - TEXT_HEIGHT_MM;
        let mut neighbour = resistor(2, "U1", 20.0, label_top - 0.15 - 0.5);
        neighbour.courtyard = rect(20.0, label_top - 0.15 - 0.5, 6.0, 1.0);
        let parts = [part.clone(), neighbour.clone()];
        let placed = place(&parts, outline());
        let b = label_box(&part, placed[&part.id]);
        assert!(
            !overlaps(b, grown(neighbour.courtyard, NEIGHBOUR_CLEARANCE_MM)),
            "label {b:?} sits inside the neighbour's silkscreen margin"
        );
        assert_ne!(placed[&part.id], side_candidate(&part, Side::Above));
    }

    #[test]
    fn the_label_font_is_a_little_under_a_millimetre() {
        assert!((LABEL_FONT_MM - 0.8).abs() < 1e-9);
        // The placer measures labels at the size the export writes.
        let part = resistor(1, "R12", 20.0, 20.0);
        let b = label_box(&part, side_candidate(&part, Side::Above));
        let width = nm_to_mm(b.max.x_nm - b.min.x_nm);
        assert!((width - 3.0 * LABEL_FONT_MM).abs() < 1e-6, "{width}");
    }

    #[test]
    fn a_label_that_lands_on_a_neighbour_moves_to_a_free_side() {
        let part = resistor(1, "R1", 20.0, 20.0);
        // A wide neighbour covers the spot just above the part.
        let mut neighbour = resistor(2, "U1", 20.0, 18.4);
        neighbour.courtyard = rect(20.0, 18.4, 8.0, 1.8);
        let parts = [part.clone(), neighbour.clone()];
        let placed = place(&parts, outline());
        let b = label_box(&part, placed[&part.id]);
        assert!(
            !overlaps(b, neighbour.courtyard),
            "label {b:?} on the neighbour"
        );
        assert!(
            b.min.y_nm >= part.courtyard.max.y_nm,
            "label {b:?} not moved below the part"
        );
        assert!(inside(outline(), b), "label {b:?} off the board");
    }

    #[test]
    fn a_label_past_the_board_edge_moves_inside() {
        // 2 mm from the top edge, so the default label (4 mm above) is off the board.
        let part = resistor(1, "R1", 20.0, 2.0);
        let placed = place(std::slice::from_ref(&part), outline());
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
        let placed = place(&parts, outline());
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
        let placed = place(std::slice::from_ref(&part), outline());
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
                let placed = place(std::slice::from_ref(&part), outline());
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

    fn soft(offset: (f64, f64)) -> LabelOverride {
        LabelOverride {
            offset_mm: Some(offset),
            angle_deg: 0.0,
            hide: None,
            hard: false,
        }
    }

    fn overrides(id: u32, o: LabelOverride) -> HashMap<ComponentId, LabelOverride> {
        HashMap::from([(ComponentId(id), o)])
    }

    #[test]
    fn a_preferred_side_is_used_when_it_is_free() {
        let mut part = resistor(1, "R1", 20.0, 20.0);
        part.side = Some(Side::Below);
        let pos = place(std::slice::from_ref(&part), outline())[&part.id];
        let b = label_box(&part, pos);
        assert!(
            b.min.y_nm >= part.courtyard.max.y_nm,
            "label {b:?} not below the part"
        );
    }

    #[test]
    fn a_blocked_preferred_side_falls_back_to_a_free_one() {
        let mut part = resistor(1, "R1", 20.0, 20.0);
        part.side = Some(Side::Below);
        let mut neighbour = resistor(2, "U1", 20.0, 22.0);
        neighbour.courtyard = rect(20.0, 22.0, 8.0, 2.0);
        let parts = [part.clone(), neighbour.clone()];
        let b = label_box(&part, place(&parts, outline())[&part.id]);
        assert!(
            !overlaps(b, neighbour.courtyard),
            "label {b:?} on the neighbour"
        );
        assert!(inside(outline(), b));
    }

    #[test]
    fn a_soft_offset_is_used_when_it_fits() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let parts = std::slice::from_ref(&part);
        let pos = place_labels(parts, outline(), &overrides(1, soft((0.0, 3.0))))[&part.id];
        let b = label_box(&part, pos);
        let (mid_x, mid_y) = ((b.min.x_nm + b.max.x_nm) / 2, (b.min.y_nm + b.max.y_nm) / 2);
        assert_eq!((mid_x, mid_y), (mm_to_nm(20.0), mm_to_nm(23.0)));
    }

    #[test]
    fn a_soft_offset_that_collides_falls_back_to_auto() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let mut neighbour = resistor(2, "U1", 20.0, 23.0);
        neighbour.courtyard = rect(20.0, 23.0, 8.0, 2.0);
        let parts = [part.clone(), neighbour.clone()];
        let pos = place_labels(&parts, outline(), &overrides(1, soft((0.0, 3.0))))[&part.id];
        let b = label_box(&part, pos);
        assert!(
            !overlaps(b, neighbour.courtyard),
            "label {b:?} on the neighbour"
        );
    }

    #[test]
    fn a_hard_offset_is_used_even_when_it_collides() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let mut neighbour = resistor(2, "U1", 20.0, 23.0);
        neighbour.courtyard = rect(20.0, 23.0, 8.0, 2.0);
        let parts = [part.clone(), neighbour];
        let hard = LabelOverride {
            hard: true,
            ..soft((0.0, 3.0))
        };
        let pos = place_labels(&parts, outline(), &overrides(1, hard))[&part.id];
        let b = label_box(&part, pos);
        let mid = ((b.min.x_nm + b.max.x_nm) / 2, (b.min.y_nm + b.max.y_nm) / 2);
        assert_eq!(mid, (mm_to_nm(20.0), mm_to_nm(23.0)));
    }

    #[test]
    fn a_soft_offset_at_an_arbitrary_angle_is_checked_with_its_rotated_extent() {
        let part = resistor(1, "R1", 20.0, 20.0);
        // The label, turned 45 degrees, reaches up to y = 21.83; unturned it
        // would stop at y = 22.35 and clear this neighbour.
        let mut neighbour = resistor(2, "U1", 20.0, 21.5);
        neighbour.courtyard = rect(20.0, 21.5, 8.0, 1.0);
        let parts = [part.clone(), neighbour.clone()];
        let turned = LabelOverride {
            angle_deg: 45.0,
            ..soft((0.0, 3.0))
        };
        let pos = place_labels(&parts, outline(), &overrides(1, turned))[&part.id];
        let b = label_box(&part, pos);
        assert!(
            !overlaps(b, neighbour.courtyard),
            "label {b:?} on the neighbour"
        );
        assert_ne!(pos.local_y_mm, 3.0, "the turned offset was kept");
    }

    #[test]
    fn a_sidecar_angle_is_a_board_angle_whatever_the_part_rotation() {
        // KiCad reads a footprint text's angle in board axes, so a rotated
        // part must not add its own rotation to the angle the design asked for.
        let mut part = resistor(1, "R1", 20.0, 20.0);
        part.rotation = Rotation::Ninety;
        part.courtyard = rect(20.0, 20.0, 1.0, 2.0);
        for (angle_deg, reads_wide) in [(0.0, true), (90.0, false)] {
            let turned = LabelOverride {
                angle_deg,
                ..soft((0.0, 4.0))
            };
            let placed = place_labels(
                std::slice::from_ref(&part),
                outline(),
                &overrides(1, turned),
            );
            let pos = placed[&part.id];
            assert_eq!(pos.angle_deg, angle_deg, "the angle was changed");
            let b = label_box(&part, pos);
            assert_eq!(
                b.max.x_nm - b.min.x_nm > b.max.y_nm - b.min.y_nm,
                reads_wide,
                "label {b:?} has the wrong orientation at {angle_deg} degrees"
            );
        }
    }

    #[test]
    fn a_hard_offset_past_the_board_edge_is_pulled_back_inside() {
        let part = resistor(1, "R1", 20.0, 20.0);
        let hard = LabelOverride {
            hard: true,
            ..soft((0.0, -25.0))
        };
        let pos =
            place_labels(std::slice::from_ref(&part), outline(), &overrides(1, hard))[&part.id];
        let b = label_box(&part, pos);
        assert!(inside(outline(), b), "label {b:?} off the board");
        assert_eq!(
            (b.min.x_nm + b.max.x_nm) / 2,
            mm_to_nm(20.0),
            "only the offending axis moves"
        );
    }

    #[test]
    fn a_label_is_hidden_by_the_part_file_unless_the_design_says_otherwise() {
        let mut part = resistor(1, "R1", 20.0, 20.0);
        part.hidden = true;
        assert!(place(std::slice::from_ref(&part), outline())[&part.id].hidden);
        let show = LabelOverride {
            offset_mm: None,
            angle_deg: 0.0,
            hide: Some(false),
            hard: false,
        };
        let shown = place_labels(std::slice::from_ref(&part), outline(), &overrides(1, show));
        assert!(!shown[&part.id].hidden);
        let hide = LabelOverride {
            hide: Some(true),
            ..show
        };
        let plain = resistor(1, "R1", 20.0, 20.0);
        let hidden = place_labels(std::slice::from_ref(&plain), outline(), &overrides(1, hide));
        assert!(hidden[&plain.id].hidden);
    }

    #[test]
    fn when_no_label_fits_the_default_is_pulled_inside() {
        // A 6 mm board cannot hold the 10 mm label anywhere: the default is
        // the only candidate left, and it is pulled to the edge clearance.
        let tiny = Rect::new(mm(0.0, 0.0), mm(6.0, 6.0));
        let part = resistor(1, "R_LONGNAME", 3.0, 3.0);
        let placed = place(std::slice::from_ref(&part), tiny);
        assert_ne!(placed[&part.id], default_label(&part));
        let b = label_box(&part, placed[&part.id]);
        assert_eq!(
            b.min.y_nm,
            mm_to_nm(0.2),
            "the sprawling default gives way to the board edge"
        );
        assert_eq!((b.min.x_nm + b.max.x_nm) / 2, mm_to_nm(3.0));
    }
}
