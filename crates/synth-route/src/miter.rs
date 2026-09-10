// SPDX-License-Identifier: Apache-2.0

//! Post-route 45° octilinear path mitering and segment consolidation pass.
//!
//! Eliminates 90° Manhattan staircases/jogs by chamfering orthogonal corners
//! into 45° diagonal octilinear segments and merging collinear adjacent segments.
//!
//! ## Do not call this on a `Routing` used for anything but display/export
//!
//! Per `seniorreview.md`'s P1 finding: `Segment`, `Routing::length_for_net`
//! (Manhattan `dx + dy`), the independent route checker, and Synth DRC all
//! document and assume axis-aligned geometry. Calling
//! [`apply_octilinear_mitering`] on the canonical `Routing` a design's DRC
//! check, length report, or fabrication export is computed from would
//! silently invalidate those — there is no diagonal-aware clearance/length
//! checker yet to back it up. Only call this on a disposable `.clone()`
//! used purely to render or export a cosmetic view (e.g. the CLI's
//! `display_segments` JSON field); never assign its result back onto, or
//! otherwise let it replace, the `Routing` any correctness-sensitive check
//! still runs against.

use crate::grid::Grid;
use crate::{Routing, Segment};
use synth_geometry::{Layer, Point};
use synth_ir::NetId;

/// Apply post-route 45° miter chamfering and segment consolidation across all nets.
///
/// See the module-level safety note: only call this on a disposable clone
/// used for display/export, never on a `Routing` anything else still checks.
pub fn apply_octilinear_mitering(routing: &mut Routing, grid: Option<&Grid>) {
    let via_points: std::collections::HashSet<Point> = routing.vias.iter().map(|v| v.at).collect();
    let mut consolidated = Vec::new();
    let mut by_net_layer: std::collections::BTreeMap<(NetId, Layer), Vec<Segment>> =
        std::collections::BTreeMap::new();

    for seg in &routing.segments {
        by_net_layer
            .entry((seg.net, seg.layer))
            .or_default()
            .push(*seg);
    }

    for ((net, layer), segs) in by_net_layer {
        let merged = merge_collinear_segments(segs);
        let mitered = miter_corners(merged, net, layer, &via_points, grid);
        consolidated.extend(mitered);
    }

    routing.segments = consolidated;
}

/// Merge adjacent collinear segments on the same net and layer into unified straight segments.
fn merge_collinear_segments(segs: Vec<Segment>) -> Vec<Segment> {
    if segs.len() <= 1 {
        return segs;
    }

    let mut merged = Vec::new();
    let mut i = 0;
    while i < segs.len() {
        let mut curr = segs[i];
        let mut j = i + 1;
        while j < segs.len() {
            let next = segs[j];
            // Check if curr end connects to next start and they are collinear
            if curr.end == next.start && curr.width_nm == next.width_nm {
                let is_horiz = curr.start.y_nm == curr.end.y_nm && next.start.y_nm == next.end.y_nm;
                let is_vert = curr.start.x_nm == curr.end.x_nm && next.start.x_nm == next.end.x_nm;
                if is_horiz || is_vert {
                    curr.end = next.end;
                    j += 1;
                    continue;
                }
            }
            break;
        }
        merged.push(curr);
        i = j;
    }
    merged
}

/// Chamfer 90° orthogonal corners into 45° diagonal miter segments where safe.
fn miter_corners(
    mut segs: Vec<Segment>,
    net: NetId,
    layer: Layer,
    via_points: &std::collections::HashSet<Point>,
    grid: Option<&Grid>,
) -> Vec<Segment> {
    if segs.len() <= 1 {
        return segs;
    }

    let miter_dist_nm: i64 = 250_000; // 0.25 mm miter chamfer offset
    let mut out = Vec::new();
    let mut i = 0;

    while i < segs.len() {
        if i + 1 < segs.len() {
            let s1 = segs[i];
            let s2 = segs[i + 1];

            if s1.end == s2.start && s1.width_nm == s2.width_nm {
                // Do not miter corners that sit on a via location
                if via_points.contains(&s1.end)
                    || via_points.contains(&s1.start)
                    || via_points.contains(&s2.end)
                {
                    out.push(segs[i]);
                    i += 1;
                    continue;
                }
                let s1_len =
                    (s1.end.x_nm - s1.start.x_nm).abs() + (s1.end.y_nm - s1.start.y_nm).abs();
                let s2_len =
                    (s2.end.x_nm - s2.start.x_nm).abs() + (s2.end.y_nm - s2.start.y_nm).abs();

                let is_s1_h = s1.start.y_nm == s1.end.y_nm;
                let is_s2_v = s2.start.x_nm == s2.end.x_nm;
                let is_s1_v = s1.start.x_nm == s1.end.x_nm;
                let is_s2_h = s2.start.y_nm == s2.end.y_nm;

                if ((is_s1_h && is_s2_v) || (is_s1_v && is_s2_h))
                    && s1_len > miter_dist_nm * 2
                    && s2_len > miter_dist_nm * 2
                {
                    let p1_trimmed = trim_point(s1.start, s1.end, miter_dist_nm);
                    let p2_trimmed = trim_point(s2.end, s2.start, miter_dist_nm);

                    let safe = check_miter_clearance(p1_trimmed, p2_trimmed, layer, net, grid);
                    if safe {
                        out.push(Segment {
                            net,
                            layer,
                            start: s1.start,
                            end: p1_trimmed,
                            width_nm: s1.width_nm,
                        });
                        out.push(Segment {
                            net,
                            layer,
                            start: p1_trimmed,
                            end: p2_trimmed,
                            width_nm: s1.width_nm,
                        });
                        segs[i + 1].start = p2_trimmed;
                        i += 1;
                        continue;
                    }
                }
            }
        }
        out.push(segs[i]);
        i += 1;
    }

    out
}

fn trim_point(from: Point, towards: Point, dist_nm: i64) -> Point {
    let dx = towards.x_nm - from.x_nm;
    let dy = towards.y_nm - from.y_nm;
    if dx > 0 {
        Point::new(towards.x_nm - dist_nm, towards.y_nm)
    } else if dx < 0 {
        Point::new(towards.x_nm + dist_nm, towards.y_nm)
    } else if dy > 0 {
        Point::new(towards.x_nm, towards.y_nm - dist_nm)
    } else {
        Point::new(towards.x_nm, towards.y_nm + dist_nm)
    }
}

fn check_miter_clearance(
    start: Point,
    end: Point,
    layer: Layer,
    net: NetId,
    grid: Option<&Grid>,
) -> bool {
    let Some(g) = grid else { return true };
    let layer_idx = layer.index(g.layers);
    let steps = 8;
    // Board coordinates in nanometres stay far below f64's 52-bit
    // mantissa, so the lerp casts are exact in practice.
    #[allow(clippy::cast_precision_loss)]
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let px = start.x_nm + ((end.x_nm - start.x_nm) as f64 * t) as i64;
        let py = start.y_nm + ((end.y_nm - start.y_nm) as f64 * t) as i64;
        if px < g.origin_nm.x_nm || py < g.origin_nm.y_nm {
            return false;
        }
        let gx = ((px - g.origin_nm.x_nm) / g.pitch_nm) as i32;
        let gy = ((py - g.origin_nm.y_nm) / g.pitch_nm) as i32;
        for dx in -1_i32..=1 {
            for dy in -1_i32..=1 {
                let cx = gx + dx;
                let cy = gy + dy;
                if cx >= 0 && cy >= 0 && cx < g.width as i32 && cy < g.height as i32 {
                    match g.get(layer_idx, cx as usize, cy as usize) {
                        Some(crate::grid::Cell::Free) => {}
                        Some(crate::grid::Cell::Pad(n) | crate::grid::Cell::Track(n))
                            if n == net => {}
                        _ => return false,
                    }
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::Point;

    #[test]
    fn test_merge_collinear_segments() {
        let net = NetId(1);
        let layer = Layer::Top;
        let segs = vec![
            Segment {
                net,
                layer,
                start: Point::new(0, 0),
                end: Point::new(5_000_000, 0),
                width_nm: 200_000,
            },
            Segment {
                net,
                layer,
                start: Point::new(5_000_000, 0),
                end: Point::new(10_000_000, 0),
                width_nm: 200_000,
            },
        ];

        let merged = merge_collinear_segments(segs);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start, Point::new(0, 0));
        assert_eq!(merged[0].end, Point::new(10_000_000, 0));
    }
}
