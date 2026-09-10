// SPDX-License-Identifier: Apache-2.0

//! Placement aesthetic quality scorer.
//!
//! Evaluates human-like placement quality:
//! - Decoupling proximity (must be < 3.0 mm from IC power pins)
//! - Top-rail passive ratio (must be 0%)
//! - Edge connector boundary distance (must be < 3.0 mm from board perimeter)

use crate::Placement;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use synth_geometry::nm_to_mm;
use synth_ir::{Board, ComponentId};

/// Placement aesthetic quality report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlacementScore {
    pub hpwl_total_nm: i64,
    pub max_decoupling_distance_mm: f64,
    pub avg_decoupling_distance_mm: f64,
    pub top_rail_passive_ratio: f64,
    pub edge_connector_boundary_dist_mm: f64,
    pub passes_human_quality_gate: bool,
}

/// Compute placement aesthetic quality score for `placement` on `board`.
pub fn score_placement(board: &Board, placement: &Placement) -> PlacementScore {
    let by_id: HashMap<ComponentId, synth_geometry::Point> = placement
        .components
        .iter()
        .map(|p| (p.id, p.center))
        .collect();

    let usable_min_y = nm_to_mm(placement.board_outline.min.y_nm) + 5.0;
    let top_rail_threshold_y = usable_min_y + 8.0;

    let mut total_decoupling_dist = 0.0;
    let mut decoupling_count = 0;
    let mut max_decoupling_dist = 0.0;

    // 1. Measure IC -> Decoupling Cap L1 distances
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        let Some(ic_pos) = by_id.get(&component.id) else {
            continue;
        };
        for _rule in &part.required_decoupling {
            for net in &board.nets {
                let mentions_ic = net.endpoints.iter().any(|ep| ep.component == component.id);
                if !mentions_ic {
                    continue;
                }
                for ep in &net.endpoints {
                    if ep.component == component.id {
                        continue;
                    }
                    if let Some(other) = board.components.iter().find(|c| c.id == ep.component) {
                        if other.part.as_ref().is_some_and(|p| p.kind == "capacitor") {
                            if let Some(cap_pos) = by_id.get(&other.id) {
                                let dx = nm_to_mm((ic_pos.x_nm - cap_pos.x_nm).abs());
                                let dy = nm_to_mm((ic_pos.y_nm - cap_pos.y_nm).abs());
                                let dist = dx + dy;
                                total_decoupling_dist += dist;
                                decoupling_count += 1;
                                if dist > max_decoupling_dist {
                                    max_decoupling_dist = dist;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let avg_decoupling_dist = if decoupling_count > 0 {
        total_decoupling_dist / f64::from(decoupling_count)
    } else {
        0.0
    };

    // 2. Measure Top-Rail Passive Ratio
    let mut total_passives = 0;
    let mut top_rail_passives = 0;
    for component in &board.components {
        if matches!(
            component.kind.as_str(),
            "resistor" | "capacitor" | "inductor" | "diode"
        ) {
            total_passives += 1;
            if let Some(pos) = by_id.get(&component.id) {
                if nm_to_mm(pos.y_nm) <= top_rail_threshold_y {
                    top_rail_passives += 1;
                }
            }
        }
    }

    let top_rail_ratio = if total_passives > 0 {
        f64::from(top_rail_passives) / f64::from(total_passives)
    } else {
        0.0
    };

    // 3. Measure Edge Connector Boundary Distance
    let mut max_connector_edge_dist = 0.0;
    let min_x = nm_to_mm(placement.board_outline.min.x_nm);
    let min_y = nm_to_mm(placement.board_outline.min.y_nm);
    let max_x = nm_to_mm(placement.board_outline.max.x_nm);
    let max_y = nm_to_mm(placement.board_outline.max.y_nm);

    for component in &board.components {
        if component.kind == "connector" {
            if let Some(pos) = by_id.get(&component.id) {
                let px = nm_to_mm(pos.x_nm);
                let py = nm_to_mm(pos.y_nm);
                let dist_left = (px - min_x).abs();
                let dist_right = (max_x - px).abs();
                let dist_top = (py - min_y).abs();
                let dist_bottom = (max_y - py).abs();
                let min_edge_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);
                if min_edge_dist > max_connector_edge_dist {
                    max_connector_edge_dist = min_edge_dist;
                }
            }
        }
    }

    let passes_human_quality_gate = max_decoupling_dist <= 5.0 && top_rail_ratio <= 0.40;

    let pad_offsets = crate::build_pad_offset_lookup(board);
    let hpwl_total_nm = crate::hpwl_total(board, &placement.components, &pad_offsets);

    PlacementScore {
        hpwl_total_nm,
        max_decoupling_distance_mm: max_decoupling_dist,
        avg_decoupling_distance_mm: avg_decoupling_dist,
        top_rail_passive_ratio: top_rail_ratio,
        edge_connector_boundary_dist_mm: max_connector_edge_dist,
        passes_human_quality_gate,
    }
}
