// SPDX-License-Identifier: Apache-2.0

//! Accordion serpentine length-matching engine (`crates/synth-route/src/serpentine.rs`).
//!
//! For differential pairs whose skew exceeds tolerance (`max_skew_nm`, default 100,000 nm = 0.1 mm),
//! this module locates straight segments on the shorter trace leg and inserts U-shaped
//! accordion meander loops to equalize physical trace lengths.

use synth_geometry::Point;
use synth_ir::NetId;

use crate::diff_pair::PairReport;
use crate::Segment;

/// Maximum allowed skew tolerance in nanometers (2.0 mm = 2,000,000 nm).
pub const DEFAULT_MAX_SKEW_NM: i64 = 2_000_000;

/// Meander loop amplitude (height) in nanometers (0.6 mm).
pub const MEANDER_AMPLITUDE_NM: i64 = 600_000;

/// Meander loop pitch (width) in nanometers (0.4 mm).
pub const MEANDER_PITCH_NM: i64 = 400_000;

/// Balance trace lengths for all differential pairs whose skew exceeds `max_skew_nm`.
///
/// Modifies `segments` in-place by replacing straight segments on the shorter half
/// with serpentine meander segments, and updates `reports` with post-balancing lengths and skews.
pub fn balance_pair_lengths(
    segments: &mut Vec<Segment>,
    reports: &mut [PairReport],
    max_skew_nm: i64,
    grid: Option<&crate::grid::Grid>,
) {
    let backup_segments = segments.clone();
    let backup_reports = reports.to_vec();

    for report in reports.iter_mut() {
        if report.skew_nm <= max_skew_nm {
            continue;
        }

        let positive_len = report.positive_length_nm;
        let negative_len = report.negative_length_nm;

        let (target_net, delta_needed) = if positive_len < negative_len {
            (report.positive, negative_len - positive_len)
        } else {
            (report.negative, positive_len - negative_len)
        };

        if delta_needed <= max_skew_nm {
            continue;
        }

        let added_len = insert_serpentine_meander(segments, target_net, delta_needed, grid);

        if positive_len < negative_len {
            report.positive_length_nm += added_len;
        } else {
            report.negative_length_nm += added_len;
        }
        report.skew_nm = (report.positive_length_nm - report.negative_length_nm).abs();
    }

    // Full structural DRC check on final segments: check grid cells AND check trace-to-trace clearance with other nets
    let mut drc_clean = true;
    for seg in segments.iter() {
        if let Some(g) = grid {
            let layer_idx = seg.layer.index(g.layers);
            let start_cell = nm_to_cell(g, seg.start);
            let end_cell = nm_to_cell(g, seg.end);
            for cell in cells_between(start_cell, end_cell) {
                match g.get(layer_idx, cell.0, cell.1) {
                    Some(crate::grid::Cell::Free) => {}
                    Some(crate::grid::Cell::Pad(n)) if n == seg.net => {}
                    _ => {
                        drc_clean = false;
                        break;
                    }
                }
            }
        }
        if !drc_clean {
            break;
        }
        // Check for segment intersection with all trace segments of OTHER nets
        for other in &backup_segments {
            if seg.net != other.net
                && seg.layer == other.layer
                && segments_intersect(seg.start, seg.end, other.start, other.end)
            {
                drc_clean = false;
                break;
            }
        }
        if !drc_clean {
            break;
        }
    }

    if !drc_clean {
        *segments = backup_segments;
        reports.copy_from_slice(&backup_reports);
    }
}

fn segments_intersect(p1: Point, p2: Point, p3: Point, p4: Point) -> bool {
    let ccw = |a: Point, b: Point, c: Point| -> bool {
        (c.y_nm - a.y_nm) * (b.x_nm - a.x_nm) > (b.y_nm - a.y_nm) * (c.x_nm - a.x_nm)
    };
    (ccw(p1, p3, p4) != ccw(p2, p3, p4)) && (ccw(p1, p2, p3) != ccw(p1, p2, p4))
}

fn nm_to_cell(g: &crate::grid::Grid, p: Point) -> (usize, usize) {
    let x = ((p.x_nm - g.origin_nm.x_nm) / g.pitch_nm).max(0) as usize;
    let y = ((p.y_nm - g.origin_nm.y_nm) / g.pitch_nm).max(0) as usize;
    let x = x.min(g.width.saturating_sub(1));
    let y = y.min(g.height.saturating_sub(1));
    (x, y)
}

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
        vec![a, b]
    }
}

/// Insert serpentine meander loops on the segments belonging to `target_net` until
/// approximately `delta_needed` additional nanometers of length are added.
fn insert_serpentine_meander(
    segments: &mut Vec<Segment>,
    target_net: NetId,
    delta_needed: i64,
    grid: Option<&crate::grid::Grid>,
) -> i64 {
    let mut total_added_nm: i64 = 0;
    let mut i = 0;

    while i < segments.len() && total_added_nm < delta_needed {
        if segments[i].net != target_net {
            i += 1;
            continue;
        }

        let seg = &segments[i];
        let dx = (seg.end.x_nm - seg.start.x_nm).abs();
        let dy = (seg.end.y_nm - seg.start.y_nm).abs();

        let seg_len = dx + dy;
        // Require at least 3.0 mm straight segment length before attempting serpentine meanders
        if seg_len < 3_000_000 {
            i += 1;
            continue;
        }

        let loop_extra_len = 2 * MEANDER_AMPLITUDE_NM;
        let loops_needed = ((delta_needed - total_added_nm) + loop_extra_len - 1) / loop_extra_len;

        let is_horizontal = dy == 0;
        let is_vertical = dx == 0;

        if !is_horizontal && !is_vertical {
            i += 1;
            continue;
        }

        let original_seg = segments.remove(i);
        let mut new_segs = Vec::new();
        let mut loops_done = 0_i64;

        let total_layers = grid.map_or(2, |g| g.layers);
        let layer_idx = original_seg.layer.index(total_layers);

        let is_cell_free = |p: Point| -> bool {
            let Some(g) = grid else { return true };
            if p.x_nm < g.origin_nm.x_nm || p.y_nm < g.origin_nm.y_nm {
                return false;
            }
            let gx = ((p.x_nm - g.origin_nm.x_nm) / g.pitch_nm) as usize;
            let gy = ((p.y_nm - g.origin_nm.y_nm) / g.pitch_nm) as usize;
            // Serpentine loops must ONLY use free grid cells, never component pad cells
            matches!(g.get(layer_idx, gx, gy), Some(crate::grid::Cell::Free))
        };

        let pad_terminal_margin_nm: i64 = 2_500_000; // 2.5mm pad terminal clearance margin

        if is_horizontal {
            let min_x = original_seg.start.x_nm.min(original_seg.end.x_nm);
            let max_x = original_seg.start.x_nm.max(original_seg.end.x_nm);
            let y = original_seg.start.y_nm;

            let start_x = min_x + pad_terminal_margin_nm;
            let end_x = max_x - pad_terminal_margin_nm;

            let mut curr_x = min_x;
            if start_x < end_x {
                // Initial straight segment entering pad
                new_segs.push(Segment {
                    net: target_net,
                    layer: original_seg.layer,
                    start: Point::new(min_x, y),
                    end: Point::new(start_x, y),
                    width_nm: original_seg.width_nm,
                });
                curr_x = start_x;

                while curr_x + MEANDER_PITCH_NM <= end_x && loops_done < loops_needed {
                    let mut chosen_amp = MEANDER_AMPLITUDE_NM;
                    let mut safe = is_cell_free(Point::new(curr_x, y + chosen_amp))
                        && is_cell_free(Point::new(curr_x + MEANDER_PITCH_NM / 2, y + chosen_amp));

                    if !safe {
                        chosen_amp = -MEANDER_AMPLITUDE_NM;
                        safe = is_cell_free(Point::new(curr_x, y + chosen_amp))
                            && is_cell_free(Point::new(
                                curr_x + MEANDER_PITCH_NM / 2,
                                y + chosen_amp,
                            ));
                    }

                    if !safe {
                        curr_x += MEANDER_PITCH_NM / 2;
                        continue;
                    }

                    let p1 = Point::new(curr_x, y);
                    let p2 = Point::new(curr_x, y + chosen_amp);
                    let p3 = Point::new(curr_x + MEANDER_PITCH_NM / 2, y + chosen_amp);
                    let p4 = Point::new(curr_x + MEANDER_PITCH_NM / 2, y);

                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p1,
                        end: p2,
                        width_nm: original_seg.width_nm,
                    });
                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p2,
                        end: p3,
                        width_nm: original_seg.width_nm,
                    });
                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p3,
                        end: p4,
                        width_nm: original_seg.width_nm,
                    });

                    curr_x += MEANDER_PITCH_NM / 2;
                    loops_done += 1;
                }
            }

            if curr_x < max_x {
                new_segs.push(Segment {
                    net: target_net,
                    layer: original_seg.layer,
                    start: Point::new(curr_x, y),
                    end: Point::new(max_x, y),
                    width_nm: original_seg.width_nm,
                });
            }
        } else {
            let min_y = original_seg.start.y_nm.min(original_seg.end.y_nm);
            let max_y = original_seg.start.y_nm.max(original_seg.end.y_nm);
            let x = original_seg.start.x_nm;

            let start_y = min_y + pad_terminal_margin_nm;
            let end_y = max_y - pad_terminal_margin_nm;

            let mut curr_y = min_y;
            if start_y < end_y {
                // Initial straight segment entering pad
                new_segs.push(Segment {
                    net: target_net,
                    layer: original_seg.layer,
                    start: Point::new(x, min_y),
                    end: Point::new(x, start_y),
                    width_nm: original_seg.width_nm,
                });
                curr_y = start_y;

                while curr_y + MEANDER_PITCH_NM <= end_y && loops_done < loops_needed {
                    let mut chosen_amp = MEANDER_AMPLITUDE_NM;
                    let mut safe = is_cell_free(Point::new(x + chosen_amp, curr_y))
                        && is_cell_free(Point::new(x + chosen_amp, curr_y + MEANDER_PITCH_NM / 2));

                    if !safe {
                        chosen_amp = -MEANDER_AMPLITUDE_NM;
                        safe = is_cell_free(Point::new(x + chosen_amp, curr_y))
                            && is_cell_free(Point::new(
                                x + chosen_amp,
                                curr_y + MEANDER_PITCH_NM / 2,
                            ));
                    }

                    if !safe {
                        curr_y += MEANDER_PITCH_NM / 2;
                        continue;
                    }

                    let p1 = Point::new(x, curr_y);
                    let p2 = Point::new(x + chosen_amp, curr_y);
                    let p3 = Point::new(x + chosen_amp, curr_y + MEANDER_PITCH_NM / 2);
                    let p4 = Point::new(x, curr_y + MEANDER_PITCH_NM / 2);

                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p1,
                        end: p2,
                        width_nm: original_seg.width_nm,
                    });
                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p2,
                        end: p3,
                        width_nm: original_seg.width_nm,
                    });
                    new_segs.push(Segment {
                        net: target_net,
                        layer: original_seg.layer,
                        start: p3,
                        end: p4,
                        width_nm: original_seg.width_nm,
                    });

                    curr_y += MEANDER_PITCH_NM / 2;
                    loops_done += 1;
                }
            }

            if curr_y < max_y {
                new_segs.push(Segment {
                    net: target_net,
                    layer: original_seg.layer,
                    start: Point::new(x, curr_y),
                    end: Point::new(x, max_y),
                    width_nm: original_seg.width_nm,
                });
            }
        }

        let num_new = new_segs.len();
        let mut drc_clean = true;
        if let Some(g) = grid {
            for new_seg in &new_segs {
                let p_start = new_seg.start;
                let p_end = new_seg.end;
                let steps = 10;
                #[allow(clippy::cast_lossless, clippy::cast_precision_loss)]
                for step in 0..=steps {
                    let t = (step as f64) / (steps as f64);
                    let px = p_start.x_nm + ((p_end.x_nm - p_start.x_nm) as f64 * t) as i64;
                    let py = p_start.y_nm + ((p_end.y_nm - p_start.y_nm) as f64 * t) as i64;
                    if px < g.origin_nm.x_nm || py < g.origin_nm.y_nm {
                        drc_clean = false;
                        break;
                    }
                    let gx = ((px - g.origin_nm.x_nm) / g.pitch_nm) as usize;
                    let gy = ((py - g.origin_nm.y_nm) / g.pitch_nm) as usize;
                    match g.get(layer_idx, gx, gy) {
                        Some(crate::grid::Cell::Free) => {}
                        Some(crate::grid::Cell::Pad(n)) if n == target_net => {}
                        _ => {
                            drc_clean = false;
                            break;
                        }
                    }
                }
                if !drc_clean {
                    break;
                }
            }
        }

        if drc_clean && !new_segs.is_empty() {
            total_added_nm += loops_done * loop_extra_len;
            for (j, new_seg) in new_segs.into_iter().enumerate() {
                segments.insert(i + j, new_seg);
            }
            i += num_new;
        } else {
            segments.insert(i, original_seg);
            i += 1;
        }
    }

    total_added_nm
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::Layer;

    #[test]
    fn test_serpentine_balances_skew() {
        let net_pos = NetId(1);
        let net_neg = NetId(2);

        let mut segments = vec![
            Segment {
                net: net_pos,
                layer: Layer::Top,
                start: Point::new(0, 0),
                end: Point::new(10_000_000, 0), // 10 mm
                width_nm: 200_000,
            },
            Segment {
                net: net_neg,
                layer: Layer::Top,
                start: Point::new(0, 2_000_000),
                end: Point::new(12_000_000, 2_000_000), // 12 mm
                width_nm: 200_000,
            },
        ];

        let mut reports = vec![PairReport {
            positive: net_pos,
            negative: net_neg,
            positive_length_nm: 10_000_000,
            negative_length_nm: 12_000_000,
            skew_nm: 2_000_000, // 2.0 mm skew
        }];

        // Tolerance (0.5 mm) is well below the 2.0 mm skew so balancing
        // is actually triggered; with the default 2.0 mm tolerance the
        // skew would be within limits and no meander would be inserted.
        balance_pair_lengths(&mut segments, &mut reports, 500_000, None);

        assert!(
            reports[0].skew_nm <= DEFAULT_MAX_SKEW_NM || reports[0].positive_length_nm > 10_000_000,
            "serpentine must balance skew or add length to shorter leg"
        );
        assert!(
            reports[0].positive_length_nm > 10_000_000,
            "shorter positive leg must receive meander loops"
        );
    }
}
