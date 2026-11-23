// SPDX-License-Identifier: Apache-2.0

//! Deterministic PCB placement analysis and instrumentation.
//!
//! Phase 0 (Enhancement Specification §5): Establish measurable placement quality
//! before and during placement algorithm enhancements.
//!
//! Evaluates geometric validity (overlaps, board outline containment, clearances),
//! connectivity metrics (HPWL, net distances, IC fan-out), and directional pad distribution.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use synth_geometry::{nm_to_mm, Point, Rect, Rotation};
use synth_ir::{Board, ComponentId};

use crate::pad_geometry::{extract_all_pad_geometries, PadSide};
use crate::Placement;

/// Complete placement analysis report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementAnalysisReport {
    pub board_name: String,
    pub component_count: usize,
    pub net_count: usize,
    pub geometric: GeometricMetrics,
    pub connectivity: ConnectivityMetrics,
    pub directional_affinity: BTreeMap<String, IcDirectionalReport>,
    pub functional_groups: BTreeMap<String, usize>,
    pub routing: Option<RoutingMetrics>,
}

/// Geometric layout metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometricMetrics {
    pub total_components: usize,
    pub total_footprint_area_mm2: f64,
    pub courtyard_overlaps: usize,
    pub out_of_board_components: usize,
    pub min_component_spacing_mm: Option<f64>,
    pub min_board_edge_clearance_mm: Option<f64>,
}

/// Connectivity and wirelength metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectivityMetrics {
    pub total_nets: usize,
    pub connected_components: usize,
    pub estimated_wirelength_mm: f64,
    pub average_connection_distance_mm: f64,
    pub ic_net_counts: BTreeMap<String, usize>,
}

/// IC-specific directional affinity and pad metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcDirectionalReport {
    pub refdes: String,
    pub total_pads: usize,
    pub connected_nets: usize,
    pub pad_distribution: BTreeMap<PadSide, usize>,
    pub destinations_by_side: BTreeMap<PadSide, Vec<String>>,
}

/// Post-routing verification metrics (optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingMetrics {
    pub completion_percent: f64,
    pub total_copper_length_mm: f64,
    pub via_count: usize,
    pub unrouted_connections: usize,
    pub drc_violations: usize,
    pub runtime_ms: Option<u64>,
}

/// Analyze a board and placement to produce a [`PlacementAnalysisReport`].
#[must_use]
pub fn analyze_placement(board: &Board, placement: &Placement) -> PlacementAnalysisReport {
    let pad_geometries = extract_all_pad_geometries(board);

    // Map component id to refdes and part
    let mut comp_map = BTreeMap::new();
    for comp in &board.components {
        comp_map.insert(comp.id, comp);
    }

    // Map component id to placement
    let mut place_map = BTreeMap::new();
    for cp in &placement.components {
        place_map.insert(cp.id, cp);
    }

    // 1. Geometric metrics
    let mut total_footprint_area_nm2: i128 = 0;
    let mut courtyard_rects: Vec<(ComponentId, String, Rect)> = Vec::new();
    let mut out_of_board_count = 0;
    let mut min_edge_clearance_nm: Option<i64> = None;

    let outline = &placement.board_outline;

    for cp in &placement.components {
        let (w_nm, h_nm) = pad_geometries.get(&cp.id).map_or((5_000_000, 5_000_000), |geom| {
            if matches!(cp.rotation, Rotation::Ninety | Rotation::TwoSeventy) {
                (geom.courtyard_height_nm, geom.courtyard_width_nm)
            } else {
                (geom.courtyard_width_nm, geom.courtyard_height_nm)
            }
        });

        let area_nm2 = (w_nm as i128) * (h_nm as i128);
        total_footprint_area_nm2 += area_nm2;

        let half_w = w_nm / 2;
        let half_h = h_nm / 2;
        let rect = Rect::from_center_half_extents(cp.center, half_w, half_h);

        let refdes = comp_map.get(&cp.id).map_or("?".to_string(), |c| c.refdes.clone());

        // Check if inside board outline
        let inside_x = rect.min.x_nm >= outline.min.x_nm && rect.max.x_nm <= outline.max.x_nm;
        let inside_y = rect.min.y_nm >= outline.min.y_nm && rect.max.y_nm <= outline.max.y_nm;
        if !inside_x || !inside_y {
            out_of_board_count += 1;
        }

        // Edge clearances
        let cl_left = rect.min.x_nm - outline.min.x_nm;
        let cl_right = outline.max.x_nm - rect.max.x_nm;
        let cl_top = rect.min.y_nm - outline.min.y_nm;
        let cl_bottom = outline.max.y_nm - rect.max.y_nm;
        let min_cl = cl_left.min(cl_right).min(cl_top).min(cl_bottom);

        min_edge_clearance_nm = Some(match min_edge_clearance_nm {
            Some(curr) => curr.min(min_cl),
            None => min_cl,
        });

        courtyard_rects.push((cp.id, refdes, rect));
    }

    // Pairwise overlap detection and minimum spacing
    let mut overlap_count = 0;
    let mut min_spacing_nm: Option<i64> = None;

    for i in 0..courtyard_rects.len() {
        for j in (i + 1)..courtyard_rects.len() {
            let (_, _, r1) = &courtyard_rects[i];
            let (_, _, r2) = &courtyard_rects[j];

            if r1.intersects(r2) {
                overlap_count += 1;
            } else {
                // Calculate disjoint gap
                let dx = if r1.max.x_nm < r2.min.x_nm {
                    r2.min.x_nm - r1.max.x_nm
                } else if r2.max.x_nm < r1.min.x_nm {
                    r1.min.x_nm - r2.max.x_nm
                } else {
                    0
                };

                let dy = if r1.max.y_nm < r2.min.y_nm {
                    r2.min.y_nm - r1.max.y_nm
                } else if r2.max.y_nm < r1.min.y_nm {
                    r1.min.y_nm - r2.max.y_nm
                } else {
                    0
                };

                let dist = dx.max(dy);
                min_spacing_nm = Some(match min_spacing_nm {
                    Some(curr) => curr.min(dist),
                    None => dist,
                });
            }
        }
    }

    // 2. Connectivity metrics
    let mut total_hpwl_nm: i128 = 0;
    let mut multi_pin_nets = 0;
    let mut connected_components_set = BTreeSet::new();

    // Map each net to the set of connected components and pin positions
    for net in &board.nets {
        let mut pin_pts: Vec<Point> = Vec::new();

        for endpoint in &net.endpoints {
            connected_components_set.insert(endpoint.component);

            if let Some(cp) = place_map.get(&endpoint.component) {
                if let Some(geom) = pad_geometries.get(&endpoint.component) {
                    let comp_ref = comp_map.get(&endpoint.component);
                    if let Some(pad) = comp_ref.and_then(|c| geom.find_by_pin_id(c, endpoint.pin)) {
                        pin_pts.push(pad.board_point(cp.center, cp.rotation));
                    } else {
                        pin_pts.push(cp.center);
                    }
                } else {
                    pin_pts.push(cp.center);
                }
            }
        }

        if pin_pts.len() >= 2 {
            multi_pin_nets += 1;
            let mut min_x = i64::MAX;
            let mut max_x = i64::MIN;
            let mut min_y = i64::MAX;
            let mut max_y = i64::MIN;

            for pt in pin_pts {
                min_x = min_x.min(pt.x_nm);
                max_x = max_x.max(pt.x_nm);
                min_y = min_y.min(pt.y_nm);
                max_y = max_y.max(pt.y_nm);
            }

            let hpwl = (max_x - min_x) + (max_y - min_y);
            total_hpwl_nm += hpwl as i128;
        }
    }

    let estimated_wirelength_mm = nm_to_mm(total_hpwl_nm as i64);
    let average_connection_distance_mm = if multi_pin_nets > 0 {
        estimated_wirelength_mm / (multi_pin_nets as f64)
    } else {
        0.0
    };

    // Major IC net connections and directional affinity
    let mut ic_net_counts = BTreeMap::new();
    let mut directional_affinity = BTreeMap::new();

    // Identify major ICs: components with refdes starting with 'U' or with >= 8 pins
    for comp in &board.components {
        let pin_count = comp.part.as_ref().map_or(0, |p| p.pins.len());
        let is_major = comp.refdes.starts_with('U') || pin_count >= 8;

        if is_major {
            let mut connected_nets_set = BTreeSet::new();
            let mut destinations_by_side: BTreeMap<PadSide, BTreeSet<String>> = BTreeMap::new();

            if let Some(ic_geom) = pad_geometries.get(&comp.id) {
                let ic_place = place_map.get(&comp.id);

                for net in &board.nets {
                    let mut ic_has_net = false;
                    let mut ic_sides_for_net = Vec::new();

                    for endpoint in &net.endpoints {
                        if endpoint.component == comp.id {
                            ic_has_net = true;
                            if let Some(pad) = ic_geom.find_by_pin_id(comp, endpoint.pin) {
                                let side = if let Some(cp) = ic_place {
                                    pad.board_side(cp.rotation)
                                } else {
                                    pad.local_side
                                };
                                ic_sides_for_net.push(side);
                            }
                        }
                    }

                    if ic_has_net {
                        connected_nets_set.insert(&net.name);

                        // Find all other components on this net
                        for endpoint in &net.endpoints {
                            if endpoint.component != comp.id {
                                if let Some(dest_comp) = comp_map.get(&endpoint.component) {
                                    for &side in &ic_sides_for_net {
                                        destinations_by_side
                                            .entry(side)
                                            .or_default()
                                            .insert(dest_comp.refdes.clone());
                                    }
                                }
                            }
                        }
                    }
                }

                ic_net_counts.insert(comp.refdes.clone(), connected_nets_set.len());

                let mut pad_dist = BTreeMap::new();
                for pad in &ic_geom.pads {
                    let side = if let Some(cp) = ic_place {
                        pad.board_side(cp.rotation)
                    } else {
                        pad.local_side
                    };
                    *pad_dist.entry(side).or_insert(0) += 1;
                }

                let dest_by_side_vec = destinations_by_side
                    .into_iter()
                    .map(|(k, v)| (k, v.into_iter().collect()))
                    .collect();

                directional_affinity.insert(
                    comp.refdes.clone(),
                    IcDirectionalReport {
                        refdes: comp.refdes.clone(),
                        total_pads: ic_geom.pads.len(),
                        connected_nets: connected_nets_set.len(),
                        pad_distribution: pad_dist,
                        destinations_by_side: dest_by_side_vec,
                    },
                );
            }
        }
    }

    // 3. Functional groups summary
    let mut functional_groups = BTreeMap::new();
    for comp in &board.components {
        let group = if comp.refdes.starts_with('U') {
            "MCU/IC"
        } else if comp.refdes.starts_with('C') {
            "Capacitors"
        } else if comp.refdes.starts_with('R') {
            "Resistors"
        } else if comp.refdes.starts_with('J') || comp.refdes.starts_with('P') {
            "Connectors"
        } else if comp.refdes.starts_with('D') || comp.refdes.starts_with('L') {
            "Discrete/Inductors"
        } else if comp.refdes.starts_with('Y') || comp.refdes.starts_with('X') {
            "Crystals/Oscillators"
        } else {
            "Auxiliary"
        };
        *functional_groups.entry(group.to_string()).or_insert(0) += 1;
    }

    let geometric = GeometricMetrics {
        total_components: placement.components.len(),
        total_footprint_area_mm2: (total_footprint_area_nm2 as f64) / 1_000_000_000_000.0,
        courtyard_overlaps: overlap_count,
        out_of_board_components: out_of_board_count,
        min_component_spacing_mm: min_spacing_nm.map(nm_to_mm),
        min_board_edge_clearance_mm: min_edge_clearance_nm.map(nm_to_mm),
    };

    let connectivity = ConnectivityMetrics {
        total_nets: board.nets.len(),
        connected_components: connected_components_set.len(),
        estimated_wirelength_mm,
        average_connection_distance_mm,
        ic_net_counts,
    };

    PlacementAnalysisReport {
        board_name: board.name.clone(),
        component_count: placement.components.len(),
        net_count: board.nets.len(),
        geometric,
        connectivity,
        directional_affinity,
        functional_groups,
        routing: None,
    }
}

impl fmt::Display for PlacementAnalysisReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "PCB PLACEMENT ANALYSIS")?;
        writeln!(f)?;
        writeln!(f, "Board: {}", self.board_name)?;
        writeln!(f, "Components: {}", self.component_count)?;
        writeln!(f, "Nets: {}", self.net_count)?;
        writeln!(f)?;
        writeln!(f, "Geometric:")?;
        writeln!(f, "  Courtyard overlaps: {}", self.geometric.courtyard_overlaps)?;
        writeln!(
            f,
            "  Out-of-board components: {}",
            self.geometric.out_of_board_components
        )?;
        if let Some(spacing) = self.geometric.min_component_spacing_mm {
            writeln!(f, "  Minimum component spacing: {spacing:.2} mm")?;
        }
        if let Some(clearance) = self.geometric.min_board_edge_clearance_mm {
            writeln!(f, "  Minimum board-edge clearance: {clearance:.2} mm")?;
        }
        writeln!(
            f,
            "  Total footprint area: {:.2} mm\u{00b2}",
            self.geometric.total_footprint_area_mm2
        )?;
        writeln!(f)?;
        writeln!(f, "Connectivity:")?;
        writeln!(
            f,
            "  Estimated wirelength: {:.2} mm",
            self.connectivity.estimated_wirelength_mm
        )?;
        writeln!(
            f,
            "  Average connection distance: {:.2} mm",
            self.connectivity.average_connection_distance_mm
        )?;
        writeln!(f)?;
        writeln!(f, "Directional affinity:")?;
        for (refdes, ic_rep) in &self.directional_affinity {
            writeln!(f, "  {}:", refdes)?;
            for &side in &[PadSide::North, PadSide::South, PadSide::East, PadSide::West] {
                let dests = ic_rep.destinations_by_side.get(&side);
                let count = dests.map_or(0, |v| v.len());
                let dest_str = if let Some(v) = dests {
                    if v.is_empty() {
                        "none".to_string()
                    } else {
                        v.join(", ")
                    }
                } else {
                    "none".to_string()
                };
                writeln!(
                    f,
                    "    {} affinity: {} connected components ({})",
                    side.as_str(),
                    count,
                    dest_str
                )?;
            }
        }
        writeln!(f)?;
        writeln!(f, "Functional groups:")?;
        for (grp, count) in &self.functional_groups {
            writeln!(f, "  {}: {} components", grp, count)?;
        }
        if let Some(routing) = &self.routing {
            writeln!(f)?;
            writeln!(f, "Routing:")?;
            writeln!(f, "  Completion: {:.1}%", routing.completion_percent)?;
            writeln!(
                f,
                "  Total copper length: {:.2} mm",
                routing.total_copper_length_mm
            )?;
            writeln!(f, "  Via count: {}", routing.via_count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_geometry::{Point, Rect, Rotation};
    use synth_ir::{Board, Component, ComponentId};
    use crate::ComponentPlacement;

    fn make_test_component(id: u32, refdes: &str) -> Component {
        Component {
            id: ComponentId(id),
            refdes: refdes.to_string(),
            kind: "test".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: Span::new(0, 0),
        }
    }

    fn make_test_board(name: &str, components: Vec<Component>) -> Board {
        Board {
            outline: None,
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: name.to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            stackup: None,
            netclasses: Vec::new(),
            buses: Vec::new(),
            modules: Vec::new(),
            variants: Vec::new(),
            required_processes: BTreeSet::new(),
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn test_empty_board_analysis() {
        let board = make_test_board("test_empty", Vec::new());
        let placement = Placement {
            board_outline: Rect::new(Point::new(0, 0), Point::new(50_000_000, 50_000_000)),
            components: Vec::new(),
        };

        let report = analyze_placement(&board, &placement);
        assert_eq!(report.component_count, 0);
        assert_eq!(report.net_count, 0);
        assert_eq!(report.geometric.courtyard_overlaps, 0);
        assert_eq!(report.geometric.out_of_board_components, 0);
        assert_eq!(report.connectivity.estimated_wirelength_mm, 0.0);

        let s = format!("{}", report);
        assert!(s.contains("PCB PLACEMENT ANALYSIS"));
        assert!(s.contains("Board: test_empty"));
    }

    #[test]
    fn test_overlap_and_out_of_board_detection() {
        let comp1 = make_test_component(1, "R1");
        let comp2 = make_test_component(2, "R2");
        let board = make_test_board("overlap_test", vec![comp1, comp2]);

        // Place them overlapping: p1 partially out of board, p2 inside board
        let p1 = ComponentPlacement {
            id: ComponentId(1),
            center: Point::new(1_000_000, 5_000_000), // courtyard 5x5mm extends to -1.5mm (out of board)
            rotation: Rotation::Zero,
            layer: synth_geometry::Layer::Top,
        };
        let p2 = ComponentPlacement {
            id: ComponentId(2),
            center: Point::new(4_000_000, 5_000_000), // courtyard [1.5mm, 6.5mm] (inside board, overlaps with p1 [−1.5, 3.5])
            rotation: Rotation::Zero,
            layer: synth_geometry::Layer::Top,
        };

        let placement = Placement {
            board_outline: Rect::new(Point::new(0, 0), Point::new(20_000_000, 20_000_000)),
            components: vec![p1, p2],
        };

        let report = analyze_placement(&board, &placement);
        assert_eq!(report.geometric.courtyard_overlaps, 1);
        assert_eq!(report.geometric.out_of_board_components, 1);
    }
}
