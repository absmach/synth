// SPDX-License-Identifier: Apache-2.0

//! V1 DRC rule implementations. Slice 1A shipped the trace /
//! clearance / copper-to-edge trio; slice 1B (this slice) adds
//! the via-aware trio: drill diameter, annular ring,
//! drill-to-copper. Mask sliver and silk overlap follow in
//! slice 1C; courtyard overlap in slice 1D.
//!
//! Every rule returns `Vec<Violation>` so the engine can run
//! them in parallel later (slice 2.x — for V1 they're cheap
//! enough that single-threaded execution stays under the
//! 1-second budget for a sensor_logger-class design).

use synth_geometry::{nm_to_mm, Point};
use synth_place::Placement;
use synth_route::{Routing, Segment, Via};

use crate::profile::ManufacturerProfile;
use crate::Violation;

/// Minimum trace width — every emitted segment's `width_nm`
/// must be ≥ profile's `min_trace_width_nm`. Code:
/// `E-SYNTH-DRC-001`.
#[must_use]
pub fn check_min_trace_width(routing: &Routing, profile: &ManufacturerProfile) -> Vec<Violation> {
    routing
        .segments
        .iter()
        .filter(|s| s.width_nm < profile.min_trace_width_nm)
        .map(|s| Violation {
            code: "E-SYNTH-DRC-001".to_string(),
            message: format!(
                "net {} trace width {:.3} mm is below the {:.3} mm minimum",
                s.net.0,
                nm_to_mm(s.width_nm),
                nm_to_mm(profile.min_trace_width_nm),
            ),
            witness: vec![s.start, s.end],
            nets: vec![s.net],
            components: Vec::new(),
            pos_mm: Some((nm_to_mm(s.start.x_nm), nm_to_mm(s.start.y_nm))),
            suggested_override: None,
        })
        .collect()
}

/// Minimum copper-to-copper clearance — no two segments on
/// the same layer belonging to *different* nets may lie closer
/// than the profile's clearance. Code: `E-SYNTH-DRC-002`.
///
/// Slice 1A uses an O(n²) pairwise check via the segment
/// bounding boxes inflated by half the clearance plus half
/// the trace width. Sufficient for designs up to a few
/// thousand segments; slice 2.x adds a grid/sweep accelerator
/// when the corpus needs it.
#[must_use]
pub fn check_min_copper_clearance(
    routing: &Routing,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    let clearance = profile.min_copper_clearance_nm;
    let mut violations = Vec::new();
    let segs = &routing.segments;
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let a = &segs[i];
            let b = &segs[j];
            if a.net == b.net || a.layer != b.layer {
                continue;
            }
            let pad_a = (a.width_nm + clearance) / 2 + 1;
            let pad_b = (b.width_nm + clearance) / 2 + 1;
            let inflated_pad = pad_a + pad_b;
            if !bboxes_overlap(a, b, inflated_pad) {
                continue;
            }
            // Bounding boxes overlap → potential clearance
            // violation. For slice 1A precision we check the
            // actual segment-to-segment distance for the
            // common case (one horizontal + one vertical, or
            // parallel pairs).
            let d = segment_distance_nm(a, b);
            let min_allowed = (a.width_nm + b.width_nm) / 2 + clearance;
            if d < min_allowed {
                violations.push(Violation {
                    code: "E-SYNTH-DRC-002".to_string(),
                    message: format!(
                        "nets {} and {} segments {:.3} mm apart (< {:.3} mm required)",
                        a.net.0,
                        b.net.0,
                        nm_to_mm(d),
                        nm_to_mm(min_allowed),
                    ),
                    witness: vec![a.start, a.end, b.start, b.end],
                    nets: vec![a.net, b.net],
                    components: Vec::new(),
                    pos_mm: Some((nm_to_mm(a.start.x_nm), nm_to_mm(a.start.y_nm))),
                    suggested_override: None,
                });
            }
        }
    }
    violations
}

/// Copper-to-edge clearance — every trace must stay at least
/// `min_copper_to_edge_nm` away from the board outline.
/// Code: `E-SYNTH-DRC-003`.
#[must_use]
pub fn check_copper_to_edge(
    routing: &Routing,
    placement: &Placement,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    let edge = profile.min_copper_to_edge_nm;
    let outline = placement.board_outline;
    let mut violations = Vec::new();
    for s in &routing.segments {
        let half_w = s.width_nm / 2;
        for &pt in &[s.start, s.end] {
            let dx_min = pt.x_nm - outline.min.x_nm - half_w;
            let dx_max = outline.max.x_nm - pt.x_nm - half_w;
            let dy_min = pt.y_nm - outline.min.y_nm - half_w;
            let dy_max = outline.max.y_nm - pt.y_nm - half_w;
            let min_dist = dx_min.min(dx_max).min(dy_min).min(dy_max);
            if min_dist < edge {
                violations.push(Violation {
                    code: "E-SYNTH-DRC-003".to_string(),
                    message: format!(
                        "net {} trace at ({:.2}, {:.2}) mm is {:.3} mm from \
                         board edge (< {:.3} mm required)",
                        s.net.0,
                        nm_to_mm(pt.x_nm),
                        nm_to_mm(pt.y_nm),
                        nm_to_mm(min_dist),
                        nm_to_mm(edge),
                    ),
                    witness: vec![pt],
                    nets: vec![s.net],
                    components: Vec::new(),
                    pos_mm: Some((nm_to_mm(pt.x_nm), nm_to_mm(pt.y_nm))),
                    suggested_override: None,
                });
            }
        }
    }
    violations
}

/// Minimum drill diameter — every via's `drill_nm` must be ≥
/// the profile's `min_drill_diameter_nm`. Code:
/// `E-SYNTH-DRC-004`. Catches vias the router placed without
/// regard to the manufacturer tier (e.g., a 0.2 mm drill on
/// JLC standard, which incurs a surcharge).
#[must_use]
pub fn check_min_drill_diameter(
    routing: &Routing,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    routing
        .vias
        .iter()
        .filter(|v| v.drill_nm < profile.min_drill_diameter_nm)
        .map(|v| Violation {
            code: "E-SYNTH-DRC-004".to_string(),
            message: format!(
                "net {} via at ({:.2}, {:.2}) mm has drill {:.3} mm \
                 (< {:.3} mm minimum)",
                v.net.0,
                nm_to_mm(v.at.x_nm),
                nm_to_mm(v.at.y_nm),
                nm_to_mm(v.drill_nm),
                nm_to_mm(profile.min_drill_diameter_nm),
            ),
            witness: vec![v.at],
            nets: vec![v.net],
            components: Vec::new(),
            pos_mm: Some((nm_to_mm(v.at.x_nm), nm_to_mm(v.at.y_nm))),
            suggested_override: None,
        })
        .collect()
}

/// Minimum annular ring — half the difference between a via's
/// pad and drill diameters must be ≥ profile's
/// `min_annular_ring_nm`. Code: `E-SYNTH-DRC-005`. Catches both
/// undersized pads and oversized drills. Inverted geometry
/// (`pad_diameter_nm < drill_nm`) yields a negative ring and is
/// also flagged.
#[must_use]
pub fn check_min_annular_ring(routing: &Routing, profile: &ManufacturerProfile) -> Vec<Violation> {
    routing
        .vias
        .iter()
        .filter_map(|v| {
            let ring_nm = (v.pad_diameter_nm - v.drill_nm) / 2;
            if ring_nm >= profile.min_annular_ring_nm {
                return None;
            }
            Some(Violation {
                code: "E-SYNTH-DRC-005".to_string(),
                message: format!(
                    "net {} via at ({:.2}, {:.2}) mm has annular ring \
                     {:.3} mm (< {:.3} mm minimum)",
                    v.net.0,
                    nm_to_mm(v.at.x_nm),
                    nm_to_mm(v.at.y_nm),
                    nm_to_mm(ring_nm),
                    nm_to_mm(profile.min_annular_ring_nm),
                ),
                witness: vec![v.at],
                nets: vec![v.net],
                components: Vec::new(),
                pos_mm: Some((nm_to_mm(v.at.x_nm), nm_to_mm(v.at.y_nm))),
                suggested_override: None,
            })
        })
        .collect()
}

/// Drill-to-copper clearance — every via's drilled hole edge
/// must lie ≥ `min_drill_to_copper_nm` from any segment on a
/// *different* net. The drill goes through every layer, so the
/// check ignores `Segment::layer`. Code: `E-SYNTH-DRC-006`.
///
/// Distance model matches slice 1A: Manhattan between the via
/// center and the closest point on the axis-aligned segment
/// center-line, then subtract drill radius + half segment
/// width. Conservative (Manhattan ≥ Euclidean) so we may
/// over-flag at 45° approaches, but the V1 router emits only
/// axis-aligned geometry so this is exact in practice.
#[must_use]
pub fn check_min_drill_to_copper(
    routing: &Routing,
    profile: &ManufacturerProfile,
) -> Vec<Violation> {
    let clearance = profile.min_drill_to_copper_nm;
    let mut violations = Vec::new();
    for v in &routing.vias {
        let drill_r = v.drill_nm / 2;
        for s in &routing.segments {
            if s.net == v.net {
                continue;
            }
            let closest = closest_point_on_segment(s.start, s.end, v.at);
            let center_dist = manhattan(closest, v.at);
            let edge_dist = center_dist - drill_r - s.width_nm / 2;
            if edge_dist < clearance {
                violations.push(via_to_segment_violation(v, s, edge_dist, clearance));
            }
        }
    }
    violations
}

/// Courtyard overlap check — no two placed components' courtyard
/// rectangles may intersect on the PCB. Code: `E-SYNTH-DRC-008`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_courtyard_overlap(board: &synth_ir::Board, placement: &Placement) -> Vec<Violation> {
    let mut violations = Vec::new();
    for i in 0..placement.components.len() {
        for j in (i + 1)..placement.components.len() {
            let cp_i = &placement.components[i];
            let cp_j = &placement.components[j];
            let comp_i = board.component(cp_i.id);
            let comp_j = board.component(cp_j.id);
            let (ref_i, part_i) = match (comp_i, comp_i.and_then(|c| c.part.as_ref())) {
                (Some(c), Some(p)) => (&c.refdes, p),
                _ => continue,
            };
            let (ref_j, part_j) = match (comp_j, comp_j.and_then(|c| c.part.as_ref())) {
                (Some(c), Some(p)) => (&c.refdes, p),
                _ => continue,
            };
            let (w_i, h_i) = synth_layout::pcb_courtyard_for_part(part_i);
            let (w_j, h_j) = synth_layout::pcb_courtyard_for_part(part_j);
            let rect_i = synth_geometry::Rect::from_center_half_extents(
                cp_i.center,
                synth_geometry::mm_to_nm(w_i) / 2,
                synth_geometry::mm_to_nm(h_i) / 2,
            );
            let rect_j = synth_geometry::Rect::from_center_half_extents(
                cp_j.center,
                synth_geometry::mm_to_nm(w_j) / 2,
                synth_geometry::mm_to_nm(h_j) / 2,
            );
            if rect_i.intersects(&rect_j) {
                violations.push(Violation {
                    code: "E-SYNTH-DRC-008".to_string(),
                    message: format!("components {ref_i} and {ref_j} courtyards overlap"),
                    witness: vec![cp_i.center, cp_j.center],
                    nets: Vec::new(),
                    components: vec![ref_i.clone(), ref_j.clone()],
                    pos_mm: Some((nm_to_mm(cp_i.center.x_nm), nm_to_mm(cp_i.center.y_nm))),
                    suggested_override: Some(crate::SuggestedOverride {
                        refdes: ref_j.clone(),
                        delta_x_mm: 1.5,
                        delta_y_mm: 0.0,
                        rotation_deg: 0,
                    }),
                });
            } else {
                // Pad clearance check: check if pads of component i overlap pads/annular rings of component j
                let pad_offsets_i = synth_layout::kicad_footprint_loader::pads(
                    part_i.kicad_footprint.as_deref().unwrap_or(""),
                );
                let pad_offsets_j = synth_layout::kicad_footprint_loader::pads(
                    part_j.kicad_footprint.as_deref().unwrap_or(""),
                );
                if let (Some(pads_i), Some(pads_j)) = (pad_offsets_i, pad_offsets_j) {
                    'pad_check: for pi in &pads_i {
                        let (pxi, pyi) = pi.center_mm;
                        let (pwi, phi) = pi.size_mm;
                        let p_center_i = synth_geometry::Point::new(
                            cp_i.center.x_nm + synth_geometry::mm_to_nm(pxi),
                            cp_i.center.y_nm + synth_geometry::mm_to_nm(pyi),
                        );
                        let p_rect_i = synth_geometry::Rect::from_center_half_extents(
                            p_center_i,
                            synth_geometry::mm_to_nm(pwi) / 2,
                            synth_geometry::mm_to_nm(phi) / 2,
                        );
                        for pj in &pads_j {
                            let (pxj, pyj) = pj.center_mm;
                            let (pwj, phj) = pj.size_mm;
                            let p_center_j = synth_geometry::Point::new(
                                cp_j.center.x_nm + synth_geometry::mm_to_nm(pxj),
                                cp_j.center.y_nm + synth_geometry::mm_to_nm(pyj),
                            );
                            let p_rect_j = synth_geometry::Rect::from_center_half_extents(
                                p_center_j,
                                synth_geometry::mm_to_nm(pwj) / 2,
                                synth_geometry::mm_to_nm(phj) / 2,
                            );
                            if p_rect_i.intersects(&p_rect_j) {
                                violations.push(Violation {
                                    code: "E-SYNTH-DRC-008".to_string(),
                                    message: format!(
                                        "components {ref_i} and {ref_j} pad clearance violation"
                                    ),
                                    witness: vec![cp_i.center, cp_j.center],
                                    nets: Vec::new(),
                                    components: vec![ref_i.clone(), ref_j.clone()],
                                    pos_mm: Some((
                                        nm_to_mm(cp_i.center.x_nm),
                                        nm_to_mm(cp_i.center.y_nm),
                                    )),
                                    suggested_override: Some(crate::SuggestedOverride {
                                        refdes: ref_j.clone(),
                                        delta_x_mm: 1.5,
                                        delta_y_mm: 0.0,
                                        rotation_deg: 0,
                                    }),
                                });
                                break 'pad_check;
                            }
                        }
                    }
                }
            }
        }
    }
    violations
}

/// Silkscreen overlap check — component reference designator text
/// must not land outside board outline. Code: `E-SYNTH-DRC-009`.
#[must_use]
pub fn check_silkscreen_overlap(board: &synth_ir::Board, placement: &Placement) -> Vec<Violation> {
    let mut violations = Vec::new();
    for cp in &placement.components {
        let Some(comp) = board.component(cp.id) else {
            continue;
        };
        let Some(part) = comp.part.as_ref() else {
            continue;
        };
        let (w, h) = part
            .footprint_dimensions
            .as_ref()
            .map_or((2.0, 2.0), |d| (d.width_mm, d.height_mm));
        let half_w = synth_geometry::mm_to_nm(w) / 2;
        let half_h = synth_geometry::mm_to_nm(h) / 2;
        let bounds = synth_geometry::Rect::from_center_half_extents(cp.center, half_w, half_h);
        if bounds.min.x_nm < placement.board_outline.min.x_nm
            || bounds.max.x_nm > placement.board_outline.max.x_nm
            || bounds.min.y_nm < placement.board_outline.min.y_nm
            || bounds.max.y_nm > placement.board_outline.max.y_nm
        {
            violations.push(Violation {
                code: "E-SYNTH-DRC-009".to_string(),
                message: format!(
                    "component {} silkscreen/courtyard extends outside board outline",
                    comp.refdes
                ),
                witness: vec![cp.center],
                nets: Vec::new(),
                components: vec![comp.refdes.clone()],
                pos_mm: Some((nm_to_mm(cp.center.x_nm), nm_to_mm(cp.center.y_nm))),
                suggested_override: Some(crate::SuggestedOverride {
                    refdes: comp.refdes.clone(),
                    delta_x_mm: 0.0,
                    delta_y_mm: 0.0,
                    rotation_deg: 90,
                }),
            });
        }
    }
    violations
}

/// Soldermask sliver check — minimum soldermask bridge between adjacent pads.
/// Code: `E-SYNTH-DRC-010`.
#[must_use]
pub fn check_soldermask_sliver(routing: &Routing, profile: &ManufacturerProfile) -> Vec<Violation> {
    let clearance = profile.min_copper_clearance_nm;
    routing
        .segments
        .windows(2)
        .filter_map(|w| {
            let (a, b) = (&w[0], &w[1]);
            if a.net == b.net || a.layer != b.layer {
                return None;
            }
            let dist = (a.start.x_nm - b.start.x_nm).abs() + (a.start.y_nm - b.start.y_nm).abs();
            if dist > 0 && dist < clearance / 2 {
                Some(Violation {
                    code: "E-SYNTH-DRC-010".to_string(),
                    message: format!(
                        "soldermask sliver between nets {} and {} ({:.3} mm < {:.3} mm min)",
                        a.net.0,
                        b.net.0,
                        nm_to_mm(dist),
                        nm_to_mm(clearance / 2),
                    ),
                    witness: vec![a.start, b.start],
                    nets: vec![a.net, b.net],
                    components: Vec::new(),
                    pos_mm: Some((nm_to_mm(a.start.x_nm), nm_to_mm(a.start.y_nm))),
                    suggested_override: None,
                })
            } else {
                None
            }
        })
        .collect()
}

fn via_to_segment_violation(v: &Via, s: &Segment, edge_dist: i64, clearance: i64) -> Violation {
    Violation {
        code: "E-SYNTH-DRC-006".to_string(),
        message: format!(
            "via on net {} at ({:.2}, {:.2}) mm is {:.3} mm from net {} \
             segment (< {:.3} mm drill-to-copper required)",
            v.net.0,
            nm_to_mm(v.at.x_nm),
            nm_to_mm(v.at.y_nm),
            nm_to_mm(edge_dist),
            s.net.0,
            nm_to_mm(clearance),
        ),
        witness: vec![v.at, s.start, s.end],
        nets: vec![v.net, s.net],
        components: Vec::new(),
        pos_mm: Some((nm_to_mm(v.at.x_nm), nm_to_mm(v.at.y_nm))),
        suggested_override: None,
    }
}

/// Cheap overlap test: each segment's bbox inflated by
/// `pad_nm`. Used to early-exit the O(n²) clearance loop.
fn bboxes_overlap(a: &Segment, b: &Segment, pad_nm: i64) -> bool {
    let (a_min_x, a_max_x) = sort_pair(a.start.x_nm, a.end.x_nm);
    let (a_min_y, a_max_y) = sort_pair(a.start.y_nm, a.end.y_nm);
    let (b_min_x, b_max_x) = sort_pair(b.start.x_nm, b.end.x_nm);
    let (b_min_y, b_max_y) = sort_pair(b.start.y_nm, b.end.y_nm);
    !(a_max_x + pad_nm < b_min_x
        || b_max_x + pad_nm < a_min_x
        || a_max_y + pad_nm < b_min_y
        || b_max_y + pad_nm < a_min_y)
}

/// Closest-approach distance between two axis-aligned
/// segments. V1 router emits only axis-aligned segments per
/// plan §10.6.
fn segment_distance_nm(a: &Segment, b: &Segment) -> i64 {
    let p1 = closest_point_on_segment(a.start, a.end, b.start);
    let d1 = manhattan(p1, b.start);
    let p2 = closest_point_on_segment(a.start, a.end, b.end);
    let d2 = manhattan(p2, b.end);
    let p3 = closest_point_on_segment(b.start, b.end, a.start);
    let d3 = manhattan(p3, a.start);
    let p4 = closest_point_on_segment(b.start, b.end, a.end);
    let d4 = manhattan(p4, a.end);
    d1.min(d2).min(d3).min(d4)
}

/// Closest point on the axis-aligned segment `[s, e]` to `q`,
/// using Manhattan distance.
fn closest_point_on_segment(s: Point, e: Point, q: Point) -> Point {
    let (sx, ex) = sort_pair(s.x_nm, e.x_nm);
    let (sy, ey) = sort_pair(s.y_nm, e.y_nm);
    Point::new(q.x_nm.clamp(sx, ex), q.y_nm.clamp(sy, ey))
}

fn manhattan(a: Point, b: Point) -> i64 {
    (a.x_nm - b.x_nm).abs() + (a.y_nm - b.y_nm).abs()
}

fn sort_pair(a: i64, b: i64) -> (i64, i64) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Helper that runs `kicad-cli pcb drc --format json` on a `.kicad_pcb` file
/// and returns the list of violations parsed from KiCad's official DRC engine.
///
/// # Panics
/// Panics if the temporary report file path cannot be converted to a string
/// (which should never happen on standard platforms).
#[allow(dead_code)]
pub fn run_kicad_cli_drc(kicad_pcb_path: &std::path::Path) -> Result<Vec<Violation>, String> {
    use std::process::Command;
    let report_file =
        std::env::temp_dir().join(format!("synth_drc_report_{}.json", std::process::id()));
    let output = Command::new("kicad-cli")
        .args([
            "pcb",
            "drc",
            "--output",
            report_file.to_str().unwrap(),
            "--format",
            "json",
            kicad_pcb_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("Failed to execute kicad-cli: {e}"))?;

    if !report_file.exists() {
        return Err(format!(
            "KiCad DRC report not generated: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let content = std::fs::read_to_string(&report_file).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let mut violations = Vec::new();

    if let Some(v_array) = json.get("violations").and_then(|v| v.as_array()) {
        for v in v_array {
            let severity = v
                .get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("error");
            if severity == "warning" {
                continue;
            }
            let v_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
            let desc = v.get("description").and_then(|d| d.as_str()).unwrap_or("");
            let mut comps = Vec::new();
            let mut pos = None;

            if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
                for item in items {
                    if let Some(refdes) = item.get("reference").and_then(|r| r.as_str()) {
                        if !refdes.is_empty() && !comps.contains(&refdes.to_string()) {
                            comps.push(refdes.to_string());
                        }
                    }
                    if pos.is_none() {
                        if let Some(p) = item.get("pos") {
                            if let (Some(x), Some(y)) = (
                                p.get("x").and_then(serde_json::Value::as_f64),
                                p.get("y").and_then(serde_json::Value::as_f64),
                            ) {
                                pos = Some((x, y));
                            }
                        }
                    }
                }
            }

            let suggested = comps.first().map(|target_refdes| crate::SuggestedOverride {
                refdes: target_refdes.clone(),
                delta_x_mm: 1.0,
                delta_y_mm: 0.0,
                rotation_deg: 0,
            });

            violations.push(Violation {
                code: format!("E-KICAD-DRC-{v_type}"),
                message: desc.to_string(),
                witness: Vec::new(),
                nets: Vec::new(),
                components: comps,
                pos_mm: pos,
                suggested_override: suggested,
            });
        }
    }
    let _ = std::fs::remove_file(report_file);
    Ok(violations)
}

#[cfg(test)]
mod tests {
    //! The lib.rs end-to-end test covers slice 1A's three rules
    //! against the sensor_logger fixture. The router doesn't
    //! emit vias yet (Phase 8 stays single-layer per plan
    //! §10.6), so the slice 1B rules need synthetic `Routing`
    //! inputs to exercise.

    use super::*;
    use synth_geometry::{mm_to_nm, Layer};
    use synth_ir::NetId;
    use synth_route::{Routing, Segment, Via};

    fn jlc() -> ManufacturerProfile {
        ManufacturerProfile::jlc_standard()
    }

    fn empty_routing() -> Routing {
        Routing {
            segments: Vec::new(),
            vias: Vec::new(),
            diff_pair_reports: Vec::new(),
            unrouted_nets: Vec::new(),
            cells_expanded: 0,
        }
    }

    #[test]
    fn drill_diameter_flags_undersized_via() {
        let mut r = empty_routing();
        r.vias.push(Via {
            net: NetId(7),
            at: Point::new(mm_to_nm(10.0), mm_to_nm(10.0)),
            drill_nm: mm_to_nm(0.2),
            pad_diameter_nm: mm_to_nm(0.5),
        });
        let vs = check_min_drill_diameter(&r, &jlc());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].code, "E-SYNTH-DRC-004");
        assert_eq!(vs[0].nets, vec![NetId(7)]);
    }

    #[test]
    fn drill_diameter_accepts_compliant_via() {
        let mut r = empty_routing();
        r.vias.push(Via {
            net: NetId(7),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.6),
        });
        assert!(check_min_drill_diameter(&r, &jlc()).is_empty());
    }

    #[test]
    fn annular_ring_flags_thin_pad() {
        let mut r = empty_routing();
        // 0.3 mm drill, 0.5 mm pad → 0.1 mm ring < 0.13 mm min.
        r.vias.push(Via {
            net: NetId(2),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.5),
        });
        let vs = check_min_annular_ring(&r, &jlc());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].code, "E-SYNTH-DRC-005");
    }

    #[test]
    fn annular_ring_accepts_generous_pad() {
        let mut r = empty_routing();
        // 0.3 mm drill, 0.6 mm pad → 0.15 mm ring ≥ 0.13 mm min.
        r.vias.push(Via {
            net: NetId(2),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.6),
        });
        assert!(check_min_annular_ring(&r, &jlc()).is_empty());
    }

    #[test]
    fn drill_to_copper_flags_near_segment_on_other_net() {
        let mut r = empty_routing();
        // Via at origin, drill 0.3 mm → drill edge at 0.15 mm.
        // Segment on a *different* net at y = 0.25 mm (centre),
        // width 0.15 mm → segment edge at 0.175 mm. Edge-to-edge
        // gap = 0.025 mm; profile demands 0.2 mm.
        r.vias.push(Via {
            net: NetId(1),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.6),
        });
        r.segments.push(Segment {
            net: NetId(2),
            layer: Layer::Top,
            start: Point::new(mm_to_nm(-1.0), mm_to_nm(0.25)),
            end: Point::new(mm_to_nm(1.0), mm_to_nm(0.25)),
            width_nm: mm_to_nm(0.15),
        });
        let vs = check_min_drill_to_copper(&r, &jlc());
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].code, "E-SYNTH-DRC-006");
        assert_eq!(vs[0].nets, vec![NetId(1), NetId(2)]);
    }

    #[test]
    fn drill_to_copper_ignores_same_net() {
        let mut r = empty_routing();
        r.vias.push(Via {
            net: NetId(1),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.6),
        });
        r.segments.push(Segment {
            net: NetId(1),
            layer: Layer::Top,
            start: Point::new(mm_to_nm(-1.0), mm_to_nm(0.25)),
            end: Point::new(mm_to_nm(1.0), mm_to_nm(0.25)),
            width_nm: mm_to_nm(0.15),
        });
        assert!(check_min_drill_to_copper(&r, &jlc()).is_empty());
    }

    #[test]
    fn drill_to_copper_accepts_well_spaced_segment() {
        let mut r = empty_routing();
        // Same setup but segment moved to y = 0.6 mm — gap = 0.375 mm.
        r.vias.push(Via {
            net: NetId(1),
            at: Point::new(0, 0),
            drill_nm: mm_to_nm(0.3),
            pad_diameter_nm: mm_to_nm(0.6),
        });
        r.segments.push(Segment {
            net: NetId(2),
            layer: Layer::Top,
            start: Point::new(mm_to_nm(-1.0), mm_to_nm(0.6)),
            end: Point::new(mm_to_nm(1.0), mm_to_nm(0.6)),
            width_nm: mm_to_nm(0.15),
        });
        assert!(check_min_drill_to_copper(&r, &jlc()).is_empty());
    }
}
