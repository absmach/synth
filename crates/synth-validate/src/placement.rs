// SPDX-License-Identifier: Apache-2.0

//! Independent placement verifier.
//!
//! Validates `synth_place::Placement` against `synth_ir::Board` without
//! sharing algorithm logic with `synth-place`. Emits stable `E-SYNTH-PLACE-*`
//! and `W-SYNTH-PLACE-*` diagnostics.

use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity};
use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rect};
use synth_ir::{Board, ComponentId, PinId};
use synth_place::{ComponentPlacement, Placement};

/// Independently validate a placement against the board constraints.
#[allow(clippy::too_many_lines, clippy::similar_names)]
pub fn validate_placement(board: &Board, placement: &Placement, file: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let placement_lookup: std::collections::HashMap<ComponentId, ComponentPlacement> =
        placement.components.iter().map(|p| (p.id, *p)).collect();

    // Reconstruct courtyard Rects for all placed components
    let component_rects: Vec<(ComponentId, String, Rect)> = placement
        .components
        .iter()
        .map(|p| {
            let component = board.components.iter().find(|c| c.id == p.id);
            let refdes = component.map_or_else(|| format!("#{}", p.id.0), |c| c.refdes.clone());
            let part = component.and_then(|c| c.part.as_ref());
            let ((cx_mm, cy_mm), (w_mm, h_mm)) = part.map_or_else(
                || {
                    component.map_or(((0.0, 0.0), (4.0, 4.0)), |c| {
                        ((0.0, 0.0), synth_place::fallback_courtyard(&c.kind))
                    })
                },
                synth_layout::pcb_courtyard_geometry_for_part,
            );
            let (rot_w_nm, rot_h_nm) = match p.rotation {
                synth_geometry::Rotation::Zero | synth_geometry::Rotation::OneEighty => {
                    (mm_to_nm(w_mm), mm_to_nm(h_mm))
                }
                synth_geometry::Rotation::Ninety | synth_geometry::Rotation::TwoSeventy => {
                    (mm_to_nm(h_mm), mm_to_nm(w_mm))
                }
            };
            let (rot_cx_nm, rot_cy_nm) = p.rotation.rotate_offset(mm_to_nm(cx_mm), mm_to_nm(cy_mm));
            let court_center = Point::new(p.center.x_nm + rot_cx_nm, p.center.y_nm + rot_cy_nm);
            let rect = Rect::from_center_half_extents(court_center, rot_w_nm / 2, rot_h_nm / 2);
            (p.id, refdes, rect)
        })
        .collect();

    // 1. Outline containment check
    for (_, refdes, rect) in &component_rects {
        let inside = rect.min.x_nm >= placement.board_outline.min.x_nm
            && rect.min.y_nm >= placement.board_outline.min.y_nm
            && rect.max.x_nm <= placement.board_outline.max.x_nm
            && rect.max.y_nm <= placement.board_outline.max.y_nm;

        if !inside {
            let span = board
                .components
                .iter()
                .find(|c| c.refdes == *refdes)
                .map_or(board.source_span, |c| c.source_span);

            diagnostics.push(
                DiagnosticBuilder::new(
                    "E-SYNTH-PLACE-002",
                    Severity::Error,
                    format!("Component `{refdes}` courtyard extends outside board outline"),
                )
                .location(Location::from_span(file.to_string(), span))
                .expected(format!(
                    "component courtyard to be contained within board outline (0.0 × 0.0 to {:.1} × {:.1} mm)",
                    nm_to_mm(placement.board_outline.max.x_nm),
                    nm_to_mm(placement.board_outline.max.y_nm),
                ))
                .found(format!(
                    "courtyard min=({:.1}, {:.1}) max=({:.1}, {:.1}) mm",
                    nm_to_mm(rect.min.x_nm),
                    nm_to_mm(rect.min.y_nm),
                    nm_to_mm(rect.max.x_nm),
                    nm_to_mm(rect.max.y_nm),
                ))
                .explanation_url("synth.docs/diagnostics/E-SYNTH-PLACE-002")
                .build(),
            );
        }
    }

    // 2. Pairwise courtyard collision check
    for i in 0..component_rects.len() {
        for j in (i + 1)..component_rects.len() {
            let (_, refdes1, r1) = &component_rects[i];
            let (_, refdes2, r2) = &component_rects[j];

            if r1.intersects(r2) {
                let span = board
                    .components
                    .iter()
                    .find(|c| c.refdes == *refdes1)
                    .map_or(board.source_span, |c| c.source_span);

                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-PLACE-002",
                        Severity::Error,
                        format!("Courtyard collision between `{refdes1}` and `{refdes2}`"),
                    )
                    .location(Location::from_span(file.to_string(), span))
                    .expected(format!("courtyard of `{refdes1}` to be disjoint from `{refdes2}`"))
                    .found(format!(
                        "`{refdes1}` rect ({:.1},{:.1} to {:.1},{:.1}) overlaps `{refdes2}` rect ({:.1},{:.1} to {:.1},{:.1}) mm",
                        nm_to_mm(r1.min.x_nm),
                        nm_to_mm(r1.min.y_nm),
                        nm_to_mm(r1.max.x_nm),
                        nm_to_mm(r1.max.y_nm),
                        nm_to_mm(r2.min.x_nm),
                        nm_to_mm(r2.min.y_nm),
                        nm_to_mm(r2.max.x_nm),
                        nm_to_mm(r2.max.y_nm),
                    ))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-PLACE-002")
                    .build(),
                );
            }
        }
    }

    // 3. Keepout region check
    for keepout in &board.keepouts {
        let radius_mm = keepout.radius.map_or(0.0, synth_ir::Length::to_mm);
        let keepout_radius_nm = mm_to_nm(radius_mm);
        if keepout_radius_nm <= 0 {
            continue;
        }

        // Find component anchoring this keepout (e.g., keepout "antenna" -> component ANT1 of kind "antenna")
        let anchor_component = board.components.iter().find(|c| {
            c.refdes.to_lowercase() == keepout.name.to_lowercase()
                || c.kind.to_lowercase() == keepout.name.to_lowercase()
                || c.refdes
                    .to_lowercase()
                    .starts_with(&keepout.name.to_lowercase())
        });

        let keepout_center = if let Some(anchor) = anchor_component {
            placement_lookup.get(&anchor.id).map(|p| p.center)
        } else {
            None
        };

        let (kx_nm, ky_nm) = keepout_center.map_or_else(
            || {
                (
                    placement.board_outline.max.x_nm / 2,
                    placement.board_outline.max.y_nm / 2,
                )
            },
            |p| (p.x_nm, p.y_nm),
        );

        for (comp_id, refdes, r) in &component_rects {
            if let Some(anchor) = anchor_component {
                if *comp_id == anchor.id {
                    continue; // Anchor component itself is allowed in its keepout
                }
            }

            let center_x = (r.min.x_nm + r.max.x_nm) / 2;
            let center_y = (r.min.y_nm + r.max.y_nm) / 2;

            let dx: i64 = (center_x - kx_nm).abs();
            let dy: i64 = (center_y - ky_nm).abs();
            let dist_squared = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            let radius_squared = keepout_radius_nm.saturating_mul(keepout_radius_nm);

            if dist_squared < radius_squared {
                let span = board
                    .components
                    .iter()
                    .find(|c| c.refdes == *refdes)
                    .map_or(board.source_span, |c| c.source_span);

                diagnostics.push(
                    DiagnosticBuilder::new(
                        "E-SYNTH-PLACE-004",
                        Severity::Error,
                        format!("Component `{refdes}` placement overlaps keepout region `{}`", keepout.name),
                    )
                    .location(Location::from_span(file.to_string(), span))
                    .expected(format!("component `{refdes}` to clear keepout `{}` radius {:.1} mm", keepout.name, radius_mm))
                    .found(format!("`{refdes}` center at ({:.1}, {:.1}) mm is inside keepout radius of ({:.1}, {:.1}) mm", nm_to_mm(center_x), nm_to_mm(center_y), nm_to_mm(kx_nm), nm_to_mm(ky_nm)))
                    .explanation_url("synth.docs/diagnostics/E-SYNTH-PLACE-004")
                    .build(),
                );
            }
        }
    }

    // 4. Decoupling distance check (W-SYNTH-PLACE-003)
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(comp_pos) = placement_lookup.get(&component.id) else {
            continue;
        };

        for rule in &part.required_decoupling {
            let max_allowed_mm = rule.max_distance_mm.unwrap_or(15.0);
            let max_allowed_nm = mm_to_nm(max_allowed_mm);

            let Some(pin_idx) = part.pins.iter().position(|p| p.name == rule.net) else {
                continue;
            };
            let pid = PinId(pin_idx as u32);
            let Some((_, net)) = board.nets_containing(component.id, pid).next() else {
                continue;
            };

            for ep in &net.endpoints {
                if ep.component == component.id {
                    continue;
                }
                let Some(other_comp) = board.components.iter().find(|c| c.id == ep.component)
                else {
                    continue;
                };
                let is_cap = other_comp
                    .part
                    .as_ref()
                    .is_some_and(|p| p.kind == "capacitor");
                if !is_cap {
                    continue;
                }

                if let Some(cap_pos) = placement_lookup.get(&ep.component) {
                    let dx = (comp_pos.center.x_nm - cap_pos.center.x_nm).abs();
                    let dy = (comp_pos.center.y_nm - cap_pos.center.y_nm).abs();
                    let l1_dist_nm = dx + dy;

                    if l1_dist_nm > max_allowed_nm {
                        let dist_mm = nm_to_mm(l1_dist_nm);
                        diagnostics.push(
                            DiagnosticBuilder::new(
                                "W-SYNTH-PLACE-003",
                                Severity::Warning,
                                format!(
                                    "Decoupling capacitor `{}` is too far from `{}`",
                                    other_comp.refdes, component.refdes
                                ),
                            )
                            .location(Location::from_span(file.to_string(), component.source_span))
                            .expected(format!(
                                "decoupling capacitor to be within {max_allowed_mm:.1} mm of `{}`",
                                component.refdes
                            ))
                            .found(format!(
                                "`{}` is {dist_mm:.1} mm away from `{}`",
                                other_comp.refdes, component.refdes
                            ))
                            .explanation_url("synth.docs/diagnostics/W-SYNTH-PLACE-003")
                            .build(),
                        );
                    }
                }
            }
        }
    }

    diagnostics
}
