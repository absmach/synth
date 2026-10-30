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
//!
//! A part's outline can cross a via just as a label can. The same pass cuts
//! the part of any silkscreen line that comes within the clearance of a via
//! or a pad out of it, and drops the pieces too short to see, so no
//! silkscreen is left over a hole or too close to copper. Silkscreen strokes
//! and text thinner than a fabricator can print are drawn at the minimum.

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
/// Silkscreen keeps this far from copper it must stay off: JLCPCB's 0.15 mm
/// minimum plus 0.1 mm for the text strokes and pad shapes a box check rounds.
const CLEARANCE_MM: f64 = 0.25;
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

/// The shortest piece of an outline line worth keeping after a cut.
const MIN_PIECE_MM: f64 = 0.25;
/// The thinnest silkscreen stroke or text drawn: JLCPCB prints 0.15 mm and its
/// DFM check warns at exactly that, so strokes go a little wider.
const MIN_SILK_WIDTH_MM: f64 = 0.18;
/// Spacing of the points an outline line is checked at against pads.
const PAD_SAMPLE_MM: f64 = 0.04;

/// What a pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LabelMoves {
    /// Labels moved off a via or a pad.
    pub moved: usize,
    /// Labels on a via or a pad with no clear spot within reach, hidden so
    /// that no silkscreen is left over a hole.
    pub hidden: usize,
    /// Silkscreen outline lines cut short where they crossed a via.
    pub trimmed: usize,
    /// Silkscreen strokes and texts drawn thinner than the minimum, widened.
    pub widened: usize,
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
    let (out, widened) = widen_silkscreen(&out);
    moves.widened = widened;
    let (out, trimmed) = trim_outlines_off_copper(&out, board, &scene.vias);
    moves.trimmed = trimmed;
    (out, moves)
}

/// Cut the silkscreen outline lines of every footprint back from the vias and
/// pads they cross or come too near. Returns the new text and the number of lines changed.
fn trim_outlines_off_copper(
    text: &str,
    board: &PcbBoard,
    vias: &[((f64, f64), f64)],
) -> (String, usize) {
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut base = 0;
    while let Some(i) = text[base..].find("(footprint") {
        let start = base + i;
        let len = block_len(&text[start..]).unwrap_or(text.len() - start);
        base = start + len.max(1);
        let fp = &text[start..start + len];
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
        let mut scan = 0;
        while let Some(g) = fp[scan..].find("(fp_line") {
            let line_start = scan + g;
            let end = block_len(&fp[line_start..]).unwrap_or(fp.len() - line_start);
            scan = line_start + end.max(1);
            let line = &fp[line_start..line_start + end];
            if !(line.contains("\"F.SilkS\"") || line.contains("\"B.SilkS\"")) {
                continue;
            }
            let (Some(a), Some(b)) = (block_point(line, "(start"), block_point(line, "(end"))
            else {
                continue;
            };
            let width = block_number(line, "(width").unwrap_or(0.12);
            let (ba, bb) = (to_board(a), to_board(b));
            let cuts = merge_intervals(
                cut_intervals(ba, bb, vias, width / 2.0)
                    .into_iter()
                    .chain(pad_cut_intervals(ba, bb, board, width / 2.0))
                    .collect(),
            );
            if cuts.is_empty() {
                continue;
            }
            let pieces = kept_pieces(&cuts, (b.0 - a.0).hypot(b.1 - a.1));
            let at_t = |t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            let replacement: String = pieces
                .iter()
                .enumerate()
                .map(|(n, &(t0, t1))| piece_text(line, at_t(t0), at_t(t1), n))
                .collect::<Vec<_>>()
                .join("\n\t");
            edits.push((start + line_start..start + line_start + end, replacement));
        }
    }
    let count = edits.len();
    let mut out = text.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    (out, count)
}

/// The parameters `[t0, t1]` along `a`..`b` that lie within a via's clearance
/// (its radius, the silkscreen clearance and half the line width), merged.
fn cut_intervals(
    a: (f64, f64),
    b: (f64, f64),
    vias: &[((f64, f64), f64)],
    half_width: f64,
) -> Vec<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return Vec::new();
    }
    let cuts: Vec<(f64, f64)> = vias
        .iter()
        .filter_map(|&((vx, vy), r)| {
            let reach = r + CLEARANCE_MM + half_width;
            let (fx, fy) = (a.0 - vx, a.1 - vy);
            let qb = 2.0 * (fx * dx + fy * dy);
            let qc = fx * fx + fy * fy - reach * reach;
            let disc = qb * qb - 4.0 * len2 * qc;
            if disc <= 0.0 {
                return None;
            }
            let root = disc.sqrt();
            let (t0, t1) = ((-qb - root) / (2.0 * len2), (-qb + root) / (2.0 * len2));
            (t1 > 0.0 && t0 < 1.0).then_some((t0.max(0.0), t1.min(1.0)))
        })
        .collect();
    merge_intervals(cuts)
}

/// Sort and merge overlapping parameter intervals.
fn merge_intervals(mut cuts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for cut in cuts {
        match merged.last_mut() {
            Some(last) if cut.0 <= last.1 => last.1 = last.1.max(cut.1),
            _ => merged.push(cut),
        }
    }
    merged
}

/// The parameters along `a`..`b` where the line's edge would come within the
/// silkscreen clearance of a pad, found by sampling the signed distance to
/// every pad near the line.
fn pad_cut_intervals(
    a: (f64, f64),
    b: (f64, f64),
    board: &PcbBoard,
    half_width: f64,
) -> Vec<(f64, f64)> {
    let reach = CLEARANCE_MM + half_width;
    let near: Vec<_> = board
        .pads()
        .filter(|p| {
            let r = p.size_nm.0.max(p.size_nm.1) as f64 / NM_PER_MM / 2.0 + reach;
            let (px, py) = (p.at.x_mm(), p.at.y_mm());
            px >= a.0.min(b.0) - r
                && px <= a.0.max(b.0) + r
                && py >= a.1.min(b.1) - r
                && py <= a.1.max(b.1) + r
        })
        .collect();
    if near.is_empty() {
        return Vec::new();
    }
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    let steps = ((length / PAD_SAMPLE_MM).ceil() as u32).max(1);
    let step = 1.0 / f64::from(steps);
    let mut cuts = Vec::new();
    for k in 0..=steps {
        let t = f64::from(k) * step;
        let at = Point::new(
            mm_to_nm(a.0 + (b.0 - a.0) * t),
            mm_to_nm(a.1 + (b.1 - a.1) * t),
        );
        if near
            .iter()
            .any(|p| p.signed_distance_nm(at) < reach * NM_PER_MM)
        {
            cuts.push(((t - step).max(0.0), (t + step).min(1.0)));
        }
    }
    cuts
}

/// Draw every silkscreen stroke and text thinner than the minimum at the
/// minimum. Returns the new text and how many were widened.
fn widen_silkscreen(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut widened = 0;
    while let Some(i) = [
        "(fp_line",
        "(fp_arc",
        "(fp_circle",
        "(fp_rect",
        "(property",
        "(fp_text",
    ]
    .iter()
    .filter_map(|k| rest.find(k))
    .min()
    {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        let len = block_len(tail).unwrap_or(tail.len());
        let block = &tail[..len];
        let on_silk = block.contains("\"F.SilkS\"") || block.contains("\"B.SilkS\"");
        if on_silk {
            let (changed, n) = widen_block(block);
            out.push_str(&changed);
            widened += n;
        } else {
            out.push_str(block);
        }
        rest = &tail[len..];
    }
    out.push_str(rest);
    (out, widened)
}

/// Raise the `(width ..)` and `(thickness ..)` values in one block to the minimum.
fn widen_block(block: &str) -> (String, usize) {
    let mut out = block.to_string();
    let mut count = 0;
    for key in ["(width ", "(thickness "] {
        let mut from = 0;
        while let Some(k) = out[from..].find(key) {
            let start = from + k + key.len();
            let Some(close) = out[start..].find(')') else {
                break;
            };
            let value: Option<f64> = out[start..start + close].trim().parse().ok();
            from = start + close;
            if let Some(v) = value.filter(|v| *v > 0.0 && *v < MIN_SILK_WIDTH_MM - 1e-9) {
                let _ = v;
                out.replace_range(start..start + close, &number(MIN_SILK_WIDTH_MM));
                count += 1;
                from = start + number(MIN_SILK_WIDTH_MM).len();
            }
        }
    }
    (out, count)
}

/// The parts of a line of `length_mm` left after the cuts that are long
/// enough to keep, as parameter intervals.
fn kept_pieces(cuts: &[(f64, f64)], length_mm: f64) -> Vec<(f64, f64)> {
    let mut pieces = Vec::new();
    let mut from = 0.0;
    for &(t0, t1) in cuts {
        pieces.push((from, t0));
        from = t1;
    }
    pieces.push((from, 1.0));
    pieces
        .into_iter()
        .filter(|(t0, t1)| (t1 - t0) * length_mm >= MIN_PIECE_MM)
        .collect()
}

/// A copy of the line block with new end points. Every piece after the first
/// gets its own uuid, derived from the line's so the file stays deterministic.
fn piece_text(line: &str, start: (f64, f64), end: (f64, f64), n: usize) -> String {
    let mut text = line.to_string();
    for (key, to) in [("(start", start), ("(end", end)] {
        if let Some(k) = text.find(key) {
            if let Some(close) = text[k..].find(')') {
                text.replace_range(
                    k..k + close,
                    &format!("{key} {} {}", number(to.0), number(to.1)),
                );
            }
        }
    }
    if n > 0 {
        if let Some(u) = text.rfind("(uuid \"") {
            let body = u + "(uuid \"".len();
            if let Some(close) = text[body..].find('"') {
                let mut id: Vec<char> = text[body..body + close].chars().collect();
                if let Some(last) = id.last_mut() {
                    let digit = last.to_digit(16).unwrap_or(0);
                    *last = char::from_digit((digit + n as u32) % 16, 16).unwrap_or('0');
                }
                text.replace_range(body..body + close, &id.into_iter().collect::<String>());
            }
        }
    }
    text
}

/// The two numbers after `key` in a block.
fn block_point(block: &str, key: &str) -> Option<(f64, f64)> {
    let k = block.find(key)?;
    let body = &block[k + key.len()..];
    let v: Vec<f64> = body[..body.find(')')?]
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    Some((*v.first()?, *v.get(1)?))
}

/// The number after `key` in a block.
fn block_number(block: &str, key: &str) -> Option<f64> {
    let k = block.find(key)?;
    let body = &block[k + key.len()..];
    body[..body.find(')')?].trim().parse().ok()
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
                hidden: 0,
                trimmed: 0,
                widened: 0
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

    /// A resistor whose outline line runs along y = 22 from x = 18 to 22,
    /// with the given extra board text after it.
    fn outlined(extra: &str) -> String {
        let text = board_text(extra);
        text.replacen(
            "    (pad \"1\"",
            "    (fp_line (start -2 2) (end 2 2) (stroke (width 0.12) (type solid)) (layer \"F.SilkS\") (uuid \"00000000-0000-0000-0000-000000000010\"))\n    (pad \"1\"",
            1,
        )
    }

    fn outline_lines(text: &str) -> Vec<(f64, f64, f64, f64)> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find("(fp_line") {
            let block = &rest[i..];
            let len = block_len(block).expect("closed");
            let (a, b) = (
                block_point(&block[..len], "(start").expect("start"),
                block_point(&block[..len], "(end").expect("end"),
            );
            out.push((a.0, a.1, b.0, b.1));
            rest = &block[len..];
        }
        out
    }

    #[test]
    fn an_outline_line_across_a_via_is_cut_back_from_it() {
        let text =
            outlined(r#"  (via (at 20 22) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#);
        let (out, moves) = run(&text);
        assert_eq!(moves.trimmed, 1);
        let lines = outline_lines(&out);
        assert_eq!(lines.len(), 2, "one piece each side of the via: {lines:?}");
        let reach = 0.3 + CLEARANCE_MM + MIN_SILK_WIDTH_MM / 2.0;
        assert!((lines[0].0 + 2.0).abs() < 1e-6 && (lines[0].2 + reach).abs() < 1e-6);
        assert!((lines[1].0 - reach).abs() < 1e-6 && (lines[1].2 - 2.0).abs() < 1e-6);
        assert_eq!(
            out.matches("(uuid \"00000000-0000-0000-0000-000000000010\")")
                .count(),
            1,
            "the second piece has its own uuid"
        );
    }

    #[test]
    fn an_outline_line_clear_of_every_via_is_left_alone() {
        let text =
            outlined(r#"  (via (at 30 30) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#);
        let (out, moves) = run(&text);
        assert_eq!(moves.trimmed, 0);
        assert_eq!(outline_lines(&out), outline_lines(&text));
    }

    /// An outline line `y` below the resistor's centre, width 0.12 mm.
    fn outlined_at(y: f64, extra: &str) -> String {
        outlined(extra).replace(
            "(start -2 2) (end 2 2)",
            &format!("(start -2 {y}) (end 2 {y})"),
        )
    }

    #[test]
    fn a_thin_silkscreen_stroke_is_drawn_at_the_minimum_width() {
        let text = outlined_at(2.0, "");
        let (out, moves) = run(&text);
        assert_eq!(moves.widened, 1);
        assert!(out.contains("(width 0.18)"), "{out}");
        assert!(!out.contains("(width 0.12)"));
        // Already wide enough: untouched, and idempotent.
        let (again, moves) = run(&out);
        assert_eq!(moves.widened, 0);
        assert_eq!(again, out);
    }

    #[test]
    fn thin_reference_text_on_the_silkscreen_is_thickened_too() {
        let text = board_text("").replace(
            "(property \"Reference\" \"R1\" (at 0 -2) (layer \"F.SilkS\"))",
            "(property \"Reference\" \"R1\" (at 0 -2) (layer \"F.SilkS\") (effects (font (size 1 1) (thickness 0.12))))",
        );
        let (out, moves) = run(&text);
        assert_eq!(moves.widened, 1);
        assert!(out.contains("(thickness 0.18)"), "{out}");
    }

    #[test]
    fn an_outline_line_beside_a_pad_is_cut_back_from_it() {
        // 0.6 mm below the centre the line runs 0.125 mm from both pads: closer
        // than the silkscreen clearance once its own width is counted.
        let text = outlined_at(0.6, "");
        let (out, moves) = run(&text);
        assert_eq!(moves.trimmed, 1);
        let lines = outline_lines(&out);
        // A stub beyond each pad and the piece between them, which keeps the
        // clearance (the gap between the pads is wider than the clearance).
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].2 < -1.0, "{lines:?}");
        assert!(lines[1].0 > -0.5 && lines[1].2 < 0.5, "{lines:?}");
        assert!(lines[2].0 > 1.0, "{lines:?}");
    }

    #[test]
    fn an_outline_line_clear_of_the_pads_is_not_cut() {
        let text = outlined_at(2.0, "");
        let (out, moves) = run(&text);
        assert_eq!(moves.trimmed, 0);
        assert_eq!(outline_lines(&out).len(), 1);
    }
}
