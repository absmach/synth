// SPDX-License-Identifier: Apache-2.0

//! Keep reference labels off the holes and pads the router added copper to.
//!
//! The export places each part's reference label before routing, so it cannot
//! know where the router will put vias. A label that ends up over a via is
//! silkscreen over a drilled hole, which a fabricator's check reports as a
//! danger. This pass reads the routed board, and for every visible label that
//! sits on a via or a pad moves it the shortest distance to a spot that is
//! clear of every via, every pad and every other label, and is still on the
//! board. A label with no such spot within reach is left where it is and
//! hidden and counted, so the caller can say so.

// Board coordinates are nanometres well inside f64's exact range, and the
// counts and sample steps are small and positive.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::path::Path;

use synth_geometry::Rotation;

use crate::pcb_read::{mm_to_nm, PcbBoard, Point, NM_PER_MM};

/// Width of one character of a label at the 1 mm font the export uses.
const CHAR_WIDTH_MM: f64 = 0.9;
/// Height of a line of label text, with room for the stroke.
const TEXT_HEIGHT_MM: f64 = 1.3;
/// Silkscreen keeps this far from copper it must stay off.
const CLEARANCE_MM: f64 = 0.15;
/// The most silkscreen segments a label may cross when no spot is clear of them.
const MAX_CROSSED_SEGMENTS: usize = 4;
/// A label keeps this far from silkscreen it should not cross.
const SILK_CLEARANCE_MM: f64 = 0.1;
/// A label keeps this far inside the board edge.
const EDGE_CLEARANCE_MM: f64 = 0.2;
/// Distance between the spots tried, and how far a label may be moved.
const STEP_MM: f64 = 0.4;
const MAX_SHIFT_MM: f64 = 6.4;
/// Spacing of the points a label box is checked at.
const SAMPLE_MM: f64 = 0.3;

/// What a pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LabelMoves {
    /// Labels moved off a via or a pad.
    pub moved: usize,
    /// Labels on a via or a pad with no clear spot within reach, hidden so
    /// that no silkscreen is left over a hole.
    pub hidden: usize,
}

/// A label's box in board millimetres: `(min_x, min_y, max_x, max_y)`.
type Box2 = (f64, f64, f64, f64);

/// One visible reference label.
struct Label {
    reference: String,
    origin: (f64, f64),
    rotation: Rotation,
    /// Footprint-local offset and text angle, as written in the file.
    local: (f64, f64, f64),
    text_w: f64,
}

impl Label {
    fn centre(&self) -> (f64, f64) {
        let (dx, dy) = self
            .rotation
            .rotate_offset(mm_to_nm(self.local.0), mm_to_nm(self.local.1));
        (
            self.origin.0 + dx as f64 / NM_PER_MM,
            self.origin.1 + dy as f64 / NM_PER_MM,
        )
    }

    /// The footprint-local offset and angle after moving the label by (`dx`, `dy`) in board axes.
    fn moved_local(&self, dx: f64, dy: f64, angle: f64) -> (f64, f64, f64) {
        let inverse = match self.rotation {
            Rotation::Zero => Rotation::Zero,
            Rotation::Ninety => Rotation::TwoSeventy,
            Rotation::OneEighty => Rotation::OneEighty,
            Rotation::TwoSeventy => Rotation::Ninety,
        };
        let (lx, ly) = inverse.rotate_offset(mm_to_nm(dx), mm_to_nm(dy));
        (
            self.local.0 + lx as f64 / NM_PER_MM,
            self.local.1 + ly as f64 / NM_PER_MM,
            angle,
        )
    }

    /// Axis-aligned bounds of the text turned by `angle_deg`, centred on `at`.
    fn bounds_at(&self, at: (f64, f64), angle_deg: f64) -> Box2 {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        let w = self.text_w * cos.abs() + TEXT_HEIGHT_MM * sin.abs();
        let h = self.text_w * sin.abs() + TEXT_HEIGHT_MM * cos.abs();
        (
            at.0 - w / 2.0,
            at.1 - h / 2.0,
            at.0 + w / 2.0,
            at.1 + h / 2.0,
        )
    }
}

/// Move every reference label that sits on a via or a pad to a clear spot.
///
/// # Errors
/// Returns a description when the board cannot be read or written back.
pub fn keep_labels_off_copper(candidate: &Path) -> Result<LabelMoves, String> {
    let text = std::fs::read_to_string(candidate)
        .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
    let board = PcbBoard::parse(&text).map_err(|e| e.to_string())?;
    let (out, moves) = keep_labels_off_copper_in(&text, &board);
    if out != text {
        std::fs::write(candidate, out)
            .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
    }
    Ok(moves)
}

fn keep_labels_off_copper_in(text: &str, board: &PcbBoard) -> (String, LabelMoves) {
    keep_labels_off_copper_with(text, board, MAX_CROSSED_SEGMENTS, MAX_SHIFT_MM)
}

fn keep_labels_off_copper_with(
    text: &str,
    board: &PcbBoard,
    max_crossed: usize,
    reach_mm: f64,
) -> (String, LabelMoves) {
    let mut out = text.to_string();
    let mut moves = LabelMoves::default();
    let labels: Vec<Label> = board
        .footprints
        .iter()
        .filter_map(|fp| {
            let found = find_label(&out, &fp.reference)?;
            Some(Label {
                reference: fp.reference.clone(),
                origin: (fp.at.x_mm(), fp.at.y_mm()),
                rotation: quarter_turn(fp.rotation_deg),
                local: found.values,
                text_w: fp.reference.chars().count() as f64 * CHAR_WIDTH_MM,
            })
        })
        .collect();
    let scene = Scene {
        board,
        vias: board
            .vias
            .iter()
            .map(|v| {
                (
                    (v.at.x_mm(), v.at.y_mm()),
                    v.size_nm as f64 / NM_PER_MM / 2.0,
                )
            })
            .collect(),
        silk: silk_segments(text),
        edge: board_bounds(board),
    };
    let mut placed: Vec<Box2> = labels
        .iter()
        .map(|l| l.bounds_at(l.centre(), l.local.2))
        .collect();
    for (index, label) in labels.iter().enumerate() {
        if clear_of_copper(board, &scene.vias, placed[index]) {
            continue;
        }
        let others: Vec<Box2> = placed
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, b)| *b)
            .collect();
        let Some((dx, dy, angle)) = scene.find_spot(label, &others, max_crossed, reach_mm) else {
            if hide_label(&mut out, &label.reference) {
                moves.hidden += 1;
            }
            continue;
        };
        let new_local = label.moved_local(dx, dy, angle);
        if set_label(&mut out, &label.reference, new_local) {
            let centre = label.centre();
            placed[index] = label.bounds_at((centre.0 + dx, centre.1 + dy), angle);
            moves.moved += 1;
        }
    }
    (out, moves)
}

/// What a label may not sit on: the board's vias and pads, its silkscreen and its edge.
struct Scene<'a> {
    board: &'a PcbBoard,
    vias: Vec<((f64, f64), f64)>,
    silk: Vec<Seg>,
    edge: Option<Box2>,
}

impl Scene<'_> {
    /// The nearest spot clear of every via, pad and other label and on the
    /// board, trying the label as it is and turned a quarter before moving
    /// further out: one that crosses no silkscreen, or failing that the one
    /// that crosses the fewest segments, if that is only a line or two of a
    /// part's outline, since a label on a hole is the worse of the two. A spot
    /// clear of vias but not pads is never taken. `None` when there is none.
    fn find_spot(
        &self,
        label: &Label,
        others: &[Box2],
        max_crossed: usize,
        reach_mm: f64,
    ) -> Option<(f64, f64, f64)> {
        let centre = label.centre();
        let turned = if label.local.2.rem_euclid(180.0) < 45.0 {
            90.0
        } else {
            0.0
        };
        let candidates = || {
            candidate_shifts(reach_mm).flat_map(move |(dx, dy)| {
                [label.local.2, turned]
                    .into_iter()
                    .filter_map(move |angle| {
                        let b = label.bounds_at((centre.0 + dx, centre.1 + dy), angle);
                        let free = !others.iter().any(|o| boxes_touch(*o, b, CLEARANCE_MM))
                            && self.edge.is_none_or(|e| inside(e, b))
                            && clear_of_copper(self.board, &self.vias, b);
                        free.then(|| {
                            let crossed = self
                                .silk
                                .iter()
                                .filter(|s| segment_hits_box(**s, b, SILK_CLEARANCE_MM))
                                .count();
                            (dx, dy, angle, crossed)
                        })
                    })
            })
        };
        candidates()
            .find(|c| c.3 == 0)
            .or_else(|| {
                candidates().filter(|c| c.3 <= max_crossed).fold(
                    None,
                    |best: Option<(f64, f64, f64, usize)>, c| match best {
                        Some(b) if b.3 <= c.3 => Some(b),
                        _ => Some(c),
                    },
                )
            })
            .map(|(dx, dy, angle, _)| (dx, dy, angle))
    }
}

/// The shifts to try, nearest first: eight directions at each step out.
fn candidate_shifts(reach_mm: f64) -> impl Iterator<Item = (f64, f64)> {
    const DIRECTIONS: [(f64, f64); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
    ];
    let rings = (reach_mm / STEP_MM).round() as u32;
    std::iter::once((0.0, 0.0)).chain((1..=rings).flat_map(|ring| {
        let r = f64::from(ring) * STEP_MM;
        DIRECTIONS.iter().map(move |(ux, uy)| {
            let norm = ux.hypot(*uy);
            (r * ux / norm, r * uy / norm)
        })
    }))
}

/// True when no via comes within the clearance of the box.
fn clear_of_vias(vias: &[((f64, f64), f64)], b: Box2) -> bool {
    vias.iter().all(|&((vx, vy), r)| {
        let nx = vx.clamp(b.0, b.2);
        let ny = vy.clamp(b.1, b.3);
        (vx - nx).hypot(vy - ny) >= r + CLEARANCE_MM
    })
}

/// True when no via and no pad comes within the clearance of the box.
fn clear_of_copper(board: &PcbBoard, vias: &[((f64, f64), f64)], b: Box2) -> bool {
    let grow = CLEARANCE_MM;
    if !clear_of_vias(vias, b) {
        return false;
    }
    let pads: Vec<_> = board
        .pads()
        .filter(|p| {
            let reach = p.size_nm.0.max(p.size_nm.1) as f64 / NM_PER_MM;
            let (px, py) = (p.at.x_mm(), p.at.y_mm());
            px > b.0 - reach - grow
                && px < b.2 + reach + grow
                && py > b.1 - reach - grow
                && py < b.3 + reach + grow
        })
        .collect();
    if pads.is_empty() {
        return true;
    }
    let nx = ((b.2 - b.0) / SAMPLE_MM).ceil().max(1.0) as u32;
    let ny = ((b.3 - b.1) / SAMPLE_MM).ceil().max(1.0) as u32;
    for i in 0..=nx {
        for j in 0..=ny {
            let x = b.0 + (b.2 - b.0) * f64::from(i) / f64::from(nx);
            let y = b.1 + (b.3 - b.1) * f64::from(j) / f64::from(ny);
            let at = Point::new(mm_to_nm(x), mm_to_nm(y));
            if pads
                .iter()
                .any(|p| p.signed_distance_nm(at) < grow * NM_PER_MM)
            {
                return false;
            }
        }
    }
    true
}

fn boxes_touch(a: Box2, b: Box2, gap: f64) -> bool {
    a.0 < b.2 + gap && b.0 < a.2 + gap && a.1 < b.3 + gap && b.1 < a.3 + gap
}

fn inside(edge: Box2, b: Box2) -> bool {
    b.0 >= edge.0 + EDGE_CLEARANCE_MM
        && b.1 >= edge.1 + EDGE_CLEARANCE_MM
        && b.2 <= edge.2 - EDGE_CLEARANCE_MM
        && b.3 <= edge.3 - EDGE_CLEARANCE_MM
}

/// Bounds of the `Edge.Cuts` graphics, or `None` when the board has none.
fn board_bounds(board: &PcbBoard) -> Option<Box2> {
    let mut points = board
        .edge_cuts
        .iter()
        .flat_map(|(a, b)| [(a.x_mm(), a.y_mm()), (b.x_mm(), b.y_mm())]);
    let first = points.next()?;
    Some(points.fold(
        (first.0, first.1, first.0, first.1),
        |(x0, y0, x1, y1), (x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    ))
}

/// A silkscreen segment in board millimetres.
type Seg = ((f64, f64), (f64, f64));

/// Every silkscreen line, circle and arc of every footprint, as board-space
/// segments: the graphics a label must not be drawn across.
///
/// Read from the text, since the reader does not keep graphics. A circle
/// becomes a ring of segments and an arc the polyline through its three points.
fn silk_segments(text: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("(footprint") {
        let block = &rest[i..];
        let len = block_len(block).unwrap_or(block.len());
        let fp = &block[..len];
        rest = &block[len.min(block.len())..];
        let Some(at) = fp.find("(at ") else {
            continue;
        };
        let at_body = &fp[at + "(at".len()..];
        let Some(close) = at_body.find(')') else {
            continue;
        };
        let nums: Vec<f64> = at_body[..close]
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let (Some(&ox), Some(&oy)) = (nums.first(), nums.get(1)) else {
            continue;
        };
        let rotation = quarter_turn(nums.get(2).copied().unwrap_or(0.0));
        let to_board = |(x, y): (f64, f64)| {
            let (dx, dy) = rotation.rotate_offset(mm_to_nm(x), mm_to_nm(y));
            (ox + dx as f64 / NM_PER_MM, oy + dy as f64 / NM_PER_MM)
        };
        let mut scan = fp;
        while let Some(g) = scan.find("(fp_") {
            let graphic_text = &scan[g..];
            let end = block_len(graphic_text).unwrap_or(graphic_text.len());
            let graphic = &graphic_text[..end];
            scan = &graphic_text[end.max(1).min(graphic_text.len())..];
            if !(graphic.contains("\"F.SilkS\"") || graphic.contains("\"B.SilkS\"")) {
                continue;
            }
            let point = |key: &str| -> Option<(f64, f64)> {
                let k = graphic.find(key)?;
                let body = &graphic[k + key.len()..];
                let v: Vec<f64> = body[..body.find(')')?]
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                Some((*v.first()?, *v.get(1)?))
            };
            if graphic.starts_with("(fp_line") {
                if let (Some(a), Some(b)) = (point("(start"), point("(end")) {
                    out.push((to_board(a), to_board(b)));
                }
            } else if graphic.starts_with("(fp_circle") {
                if let (Some(c), Some(e)) = (point("(center"), point("(end")) {
                    let r = (e.0 - c.0).hypot(e.1 - c.1);
                    let ring: Vec<(f64, f64)> = (0..=16)
                        .map(|k| {
                            let a = f64::from(k) * std::f64::consts::TAU / 16.0;
                            (c.0 + r * a.cos(), c.1 + r * a.sin())
                        })
                        .collect();
                    out.extend(ring.windows(2).map(|w| (to_board(w[0]), to_board(w[1]))));
                }
            } else if graphic.starts_with("(fp_arc") {
                if let (Some(a), Some(m), Some(b)) = (point("(start"), point("(mid"), point("(end"))
                {
                    out.push((to_board(a), to_board(m)));
                    out.push((to_board(m), to_board(b)));
                }
            }
        }
    }
    out
}

/// Whether a segment passes through the box grown by `gap`.
fn segment_hits_box(seg: Seg, b: Box2, gap: f64) -> bool {
    let (x0, y0, x1, y1) = (b.0 - gap, b.1 - gap, b.2 + gap, b.3 + gap);
    let ((ax, ay), (bx, by)) = seg;
    // Liang-Barsky clip of the segment to the box.
    let (dx, dy) = (bx - ax, by - ay);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, ax - x0), (dx, x1 - ax), (-dy, ay - y0), (dy, y1 - ay)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// The quarter turn nearest a footprint angle in degrees.
fn quarter_turn(degrees: f64) -> Rotation {
    match (degrees / 90.0).round().rem_euclid(4.0) as i32 {
        1 => Rotation::Ninety,
        2 => Rotation::OneEighty,
        3 => Rotation::TwoSeventy,
        _ => Rotation::Zero,
    }
}

/// Where a visible reference label's `(at x y angle)` sits in the file.
struct Found {
    /// Byte range of the text between `(at` and its closing parenthesis.
    range: std::ops::Range<usize>,
    values: (f64, f64, f64),
}

/// The `(at ...)` of the visible `Reference` property of `reference`.
fn find_label(text: &str, reference: &str) -> Option<Found> {
    let mut from = 0;
    while let Some(i) = text[from..].find("\"Reference\"") {
        let start = from + i;
        from = start + "\"Reference\"".len();
        let rest = text[from..].trim_start();
        let Some(after_quote) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(close) = after_quote.find('"') else {
            continue;
        };
        if &after_quote[..close] != reference {
            continue;
        }
        let property_start = text[..start].rfind("(property")?;
        let property_end = property_start + block_len(&text[property_start..])?;
        let block = &text[property_start..property_end];
        if block.contains("(hide") || block.contains("hide yes") {
            return None;
        }
        let at = block.find("(at")?;
        let body_start = property_start + at + "(at".len();
        let body_end = body_start + text[body_start..].find(')')?;
        let numbers: Vec<f64> = text[body_start..body_end]
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        let (x, y) = (*numbers.first()?, *numbers.get(1)?);
        let angle = numbers.get(2).copied().unwrap_or(0.0);
        return Some(Found {
            range: body_start..body_end,
            values: (x, y, angle),
        });
    }
    None
}

/// Hide a reference label: the `(hide yes)` the export writes for a hidden one.
fn hide_label(text: &mut String, reference: &str) -> bool {
    let Some(found) = find_label(text, reference) else {
        return false;
    };
    let Some(start) = text[..found.range.start].rfind("(property") else {
        return false;
    };
    let Some(len) = block_len(&text[start..]) else {
        return false;
    };
    let end = start + len;
    // Before the `(effects` the export writes, or at the end of a property without one.
    let at = text[found.range.end..end]
        .find("(effects")
        .map_or(end - 1, |i| found.range.end + i);
    text.insert_str(at, "(hide yes)\n\t\t\t");
    true
}

fn set_label(text: &mut String, reference: &str, to: (f64, f64, f64)) -> bool {
    let Some(found) = find_label(text, reference) else {
        return false;
    };
    let replacement = format!(" {} {} {}", number(to.0), number(to.1), number(to.2));
    text.replace_range(found.range, &replacement);
    true
}

fn number(value: f64) -> String {
    let s = format!("{value:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// Length of the parenthesised block that starts `text`.
fn block_len(text: &str) -> Option<usize> {
    let mut depth = 0_i32;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// One resistor with its label 1 mm above it, a board edge round it, and
    /// `extra` after the footprint.
    fn board_text(extra: &str) -> String {
        format!(
            r#"(kicad_pcb
  (version 20260206)
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "N1")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (at 20 20)
    (property "Reference" "R1" (at 0 -2) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "N1"))
    (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "N1"))
  )
{extra}
  (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 0) (end 40 40) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 40) (end 0 40) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 40) (end 0 0) (layer "Edge.Cuts") (width 0.1))
)
"#
        )
    }

    fn run(text: &str) -> (String, LabelMoves) {
        let board = PcbBoard::parse(text).expect("parse");
        keep_labels_off_copper_in(text, &board)
    }

    fn label_at(text: &str) -> (f64, f64, f64) {
        find_label(text, "R1").expect("label").values
    }

    #[test]
    fn a_label_over_a_via_is_moved_clear_of_it() {
        let text = board_text(
            r#"  (via (at 20 18) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
        );
        let (out, moves) = run(&text);
        assert_eq!(
            moves,
            LabelMoves {
                moved: 1,
                hidden: 0
            }
        );
        let reread = PcbBoard::parse(&out).expect("still parses");
        let (x, y, angle) = label_at(&out);
        let label = Label {
            reference: "R1".to_string(),
            origin: (20.0, 20.0),
            rotation: Rotation::Zero,
            local: (x, y, angle),
            text_w: 2.0 * CHAR_WIDTH_MM,
        };
        let b = label.bounds_at(label.centre(), angle);
        assert!(
            clear_of_copper(&reread, &[((20.0, 18.0), 0.3)], b),
            "the label {b:?} still sits on the via"
        );
    }

    #[test]
    fn a_box_on_a_pad_clears_the_vias_but_not_the_copper() {
        let board = PcbBoard::parse(&board_text("")).expect("parse");
        // The resistor's pad 1 sits at board (19.175, 20).
        let on_pad = (18.9, 19.8, 19.5, 20.2);
        assert!(clear_of_vias(&[], on_pad));
        assert!(!clear_of_copper(&board, &[], on_pad));
        assert!(!clear_of_vias(&[((19.2, 20.0), 0.3)], on_pad));
    }

    #[test]
    fn a_label_keeps_off_a_neighbours_silkscreen_when_it_can() {
        // A capacitor above the resistor with a silkscreen line across the spot
        // the label would take, from (17, 17.5) to (23, 17.5).
        let capacitor = r#"  (footprint "Capacitor_SMD:C_0603_1608Metric"
    (layer "F.Cu")
    (at 20 15)
    (property "Reference" "C1" (at 0 -3) (layer "F.SilkS"))
    (fp_line (start -3 2.5) (end 3 2.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "N1"))
  )
  (via (at 20 18) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#;
        let text = board_text(capacitor);
        assert_eq!(silk_segments(&text), vec![((17.0, 17.5), (23.0, 17.5))]);
        let (out, moves) = run(&text);
        assert_eq!(moves.moved, 1);
        let (x, y, angle) = label_at(&out);
        let label = Label {
            reference: "R1".to_string(),
            origin: (20.0, 20.0),
            rotation: Rotation::Zero,
            local: (x, y, angle),
            text_w: 2.0 * CHAR_WIDTH_MM,
        };
        let b = label.bounds_at(label.centre(), angle);
        assert!(
            !segment_hits_box(((17.0, 17.5), (23.0, 17.5)), b, 0.0),
            "the label {b:?} sits on the capacitor's silkscreen line"
        );
    }

    #[test]
    fn a_circle_of_silkscreen_becomes_a_ring_of_segments_about_its_centre() {
        let text = r#"(kicad_pcb
  (footprint "Capacitor_THT:CP_Radial_D5.0mm_P2.00mm"
    (layer "F.Cu")
    (at 10 20 90)
    (fp_circle (center 1 0) (end 3.75 0) (layer "F.SilkS") (width 0.12))
  )
)"#;
        let ring = silk_segments(text);
        assert_eq!(ring.len(), 16);
        // Local centre (1, 0) turned a quarter lands at board (10, 19); radius 2.75.
        for ((x, y), _) in &ring {
            let r = (x - 10.0).hypot(y - 19.0);
            assert!(
                (r - 2.75).abs() < 1e-6,
                "point ({x}, {y}) is {r} from the centre"
            );
        }
    }

    #[test]
    fn a_segment_through_a_box_hits_it_and_one_beside_it_does_not() {
        let b = (0.0, 0.0, 2.0, 1.0);
        assert!(segment_hits_box(((-1.0, 0.5), (3.0, 0.5)), b, 0.0));
        assert!(!segment_hits_box(((-1.0, 2.0), (3.0, 2.0)), b, 0.0));
        assert!(segment_hits_box(((-1.0, 1.05), (3.0, 1.05)), b, 0.1));
    }

    #[test]
    fn a_label_on_clear_board_is_left_alone() {
        let text = board_text(
            r#"  (via (at 30 30) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
        );
        let (out, moves) = run(&text);
        assert_eq!(moves, LabelMoves::default());
        assert_eq!(out, text, "nothing to move, nothing rewritten");
    }

    #[test]
    fn a_label_with_no_clear_spot_is_hidden_and_counted() {
        // A carpet of vias over the whole area the label could move to.
        let mut vias = String::new();
        for i in 0..40 {
            for j in 0..40 {
                writeln!(
                    vias,
                    "  (via (at {} {}) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 1))",
                    f64::from(i) + 0.5,
                    f64::from(j) + 0.5
                )
                .expect("write to a string");
            }
        }
        let text = board_text(&vias);
        let (out, moves) = run(&text);
        assert_eq!(moves.moved, 0);
        assert_eq!(moves.hidden, 1);
        assert!(out.contains("(hide yes)"), "the label is hidden");
        assert!(
            find_label(&out, "R1").is_none(),
            "a hidden label is not touched again"
        );
    }

    #[test]
    fn a_label_of_a_turned_part_moves_in_the_parts_own_axes() {
        let text = board_text("").replace("(at 20 20)", "(at 20 20 90)");
        // Local (0, -2) on a part turned 90 degrees lands at board (20 - 2, 20)
        // ... a quarter turn maps local (x, y) to board (y, -x): (-2, 0).
        let via = r#"  (via (at 18 20) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#;
        let text = text.replace("\n\n  (gr_line", &format!("\n{via}\n  (gr_line"));
        let (out, moves) = run(&text);
        assert_eq!(
            moves.moved, 1,
            "the via at board (18, 20) is under the label"
        );
        assert_ne!(label_at(&out), label_at(&text));
    }

    #[test]
    fn a_hidden_label_is_not_moved() {
        let text = board_text(
            r#"  (via (at 20 18) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
        )
        .replace(
            r#"(at 0 -2) (layer "F.SilkS"))"#,
            r#"(at 0 -2) (layer "F.SilkS") (hide yes))"#,
        );
        let (out, moves) = run(&text);
        assert_eq!(moves, LabelMoves::default());
        assert_eq!(out, text);
    }
}
