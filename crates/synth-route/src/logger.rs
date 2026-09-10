// SPDX-License-Identifier: Apache-2.0

//! Routing outcome data logger for Dataset 6.
//!
//! Logs `(placement_snapshot, routing_outcome)` pairs generated during
//! router runs for downstream Phase 10 GNN spatial congestion predictor models.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use synth_ir::Board;
use synth_place::Placement;

use crate::Routing;

/// Serialized component placement record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlacedComponentRecord {
    pub refdes: String,
    pub part_id: String,
    pub x_nm: i64,
    pub y_nm: i64,
    pub rotation_deg: u32,
    pub layer: String,
}

/// A spatial grid congestion occupancy cell for GNN dataset tensor conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GridCellOccupancy {
    pub layer_index: u32,
    pub grid_x: usize,
    pub grid_y: usize,
    pub track_occupancy_count: usize,
}

/// Complete routing outcome log record (Dataset 6 item).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutingOutcomeRecord {
    pub board_name: String,
    pub layer_count: u32,
    pub board_width_nm: i64,
    pub board_height_nm: i64,
    pub component_count: usize,
    pub net_count: usize,
    pub placed_components: Vec<PlacedComponentRecord>,
    pub routed_segments_count: usize,
    pub routed_vias_count: usize,
    pub unrouted_nets_count: usize,
    pub total_wire_length_mm: f64,
    pub cells_expanded: u64,
    pub timestamp_sec: u64,
    pub occupancy_cells: Vec<GridCellOccupancy>,
}

/// Log a routing run to `log_dir` as a JSON file.
pub fn log_routing_outcome(
    board: &Board,
    placement: &Placement,
    routing: &Routing,
    log_dir: &Path,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(log_dir)?;

    let mut placed_components: Vec<PlacedComponentRecord> = placement
        .components
        .iter()
        .map(|cp| {
            let refdes = board
                .component(cp.id)
                .map_or_else(|| format!("#{}", cp.id.0), |c| c.refdes.clone());
            let part_id = board
                .component(cp.id)
                .and_then(|c| c.part.as_ref())
                .map_or_else(String::new, |p| p.id.as_str().to_string());
            PlacedComponentRecord {
                refdes,
                part_id,
                x_nm: cp.center.x_nm,
                y_nm: cp.center.y_nm,
                rotation_deg: cp.rotation.degrees() as u32,
                layer: format!("{:?}", cp.layer),
            }
        })
        .collect();
    placed_components.sort_by(|a, b| a.refdes.cmp(&b.refdes));

    let total_wire_length_nm: i64 = routing
        .segments
        .iter()
        .map(|s| (s.end.x_nm - s.start.x_nm).abs() + (s.end.y_nm - s.start.y_nm).abs())
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let total_wire_length_mm = (total_wire_length_nm as f64) / 1_000_000.0;

    let timestamp_sec = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    // Map segments to coarse spatial grid cells (1mm x 1mm bins)
    let bin_size_nm: i64 = 1_000_000;
    let width_nm = placement.board_outline.max.x_nm;
    let height_nm = placement.board_outline.max.y_nm;
    let grid_cols = ((width_nm / bin_size_nm).max(1)) as usize;
    let grid_rows = ((height_nm / bin_size_nm).max(1)) as usize;

    let mut occupancy_cells = Vec::new();
    let layer_vals = [
        synth_geometry::Layer::Top,
        synth_geometry::Layer::Inner1,
        synth_geometry::Layer::Inner2,
        synth_geometry::Layer::Bottom,
    ];
    for (layer_idx, layer_val) in layer_vals.iter().enumerate() {
        if (layer_idx as u32) >= board.layers {
            break;
        }
        for seg in &routing.segments {
            if seg.layer == *layer_val {
                let gx = ((seg.start.x_nm / bin_size_nm).max(0) as usize).min(grid_cols - 1);
                let gy = ((seg.start.y_nm / bin_size_nm).max(0) as usize).min(grid_rows - 1);
                occupancy_cells.push(GridCellOccupancy {
                    layer_index: layer_idx as u32,
                    grid_x: gx,
                    grid_y: gy,
                    track_occupancy_count: 1,
                });
            }
        }
    }

    let record = RoutingOutcomeRecord {
        board_name: board.name.clone(),
        layer_count: board.layers,
        board_width_nm: width_nm,
        board_height_nm: height_nm,
        component_count: placement.components.len(),
        net_count: board.nets.len(),
        placed_components,
        routed_segments_count: routing.segments.len(),
        routed_vias_count: routing.vias.len(),
        unrouted_nets_count: routing.unrouted_nets.len(),
        total_wire_length_mm,
        cells_expanded: routing.cells_expanded,
        timestamp_sec,
        occupancy_cells,
    };

    let filename = format!(
        "run_{}_{}.json",
        timestamp_sec,
        sanitize_filename(&board.name)
    );
    let out_path = log_dir.join(filename);

    let json_bytes = serde_json::to_vec_pretty(&record)?;
    fs::write(&out_path, json_bytes)?;

    Ok(out_path)
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
