// SPDX-License-Identifier: Apache-2.0

//! Directional Net Affinity Engine.
//!
//! Phase 2 (Enhancement Specification §7): Components connected to a large IC
//! should preferentially be placed on the side corresponding to their connected IC pins.
//!
//! Evaluates pin-level vector directions from IC pad locations, filters global/power rails,
//! aggregates functional group vectors, and calculates directional alignment penalty costs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use synth_geometry::Rotation;
use synth_ir::{Board, ComponentId, Net};

use crate::pad_geometry::{extract_all_pad_geometries, PadSide};
use crate::ComponentPlacement;

/// Default penalty scale weight for directional affinity in integer nanometers.
/// Equivalent to a 10.0 mm HPWL penalty for a completely inverted (180 deg) placement.
pub const DEFAULT_AFFINITY_WEIGHT_NM: i64 = 10_000_000;

/// Normalized directional affinity vector in board space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffinityVector {
    /// Normalized X component [-1.0, 1.0] (positive = East).
    pub dx: f64,
    /// Normalized Y component [-1.0, 1.0] (positive = South, KiCad screen coords).
    pub dy: f64,
    /// Vector magnitude before normalization [0.0, 1.0].
    pub magnitude: f64,
    /// Dominant cardinal side or Ambiguous.
    pub dominant_side: PadSide,
    /// Directional confidence [0.0, 1.0].
    pub confidence: f64,
}

impl Default for AffinityVector {
    fn default() -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            magnitude: 0.0,
            dominant_side: PadSide::Ambiguous,
            confidence: 0.0,
        }
    }
}

impl AffinityVector {
    /// Create an affinity vector from unnormalized accumulated components (x, y).
    #[must_use]
    pub fn from_components(acc_x: f64, acc_y: f64, max_possible: f64) -> Self {
        let mag = (acc_x * acc_x + acc_y * acc_y).sqrt();
        if mag < 1e-4 {
            return Self::default();
        }

        let dx = acc_x / mag;
        let dy = acc_y / mag;
        let norm_mag = if max_possible > 1e-4 {
            (mag / max_possible).min(1.0)
        } else {
            1.0
        };

        // If magnitude is very low relative to connections (e.g. pins pulling equally North and South),
        // mark direction as ambiguous.
        let dominant_side = if norm_mag < 0.25 {
            PadSide::Ambiguous
        } else if dy.abs() > dx.abs() {
            if dy < -0.15 {
                PadSide::North
            } else if dy > 0.15 {
                PadSide::South
            } else {
                PadSide::Ambiguous
            }
        } else if dx.abs() > dy.abs() {
            if dx > 0.15 {
                PadSide::East
            } else if dx < -0.15 {
                PadSide::West
            } else {
                PadSide::Ambiguous
            }
        } else {
            PadSide::Ambiguous
        };

        Self {
            dx,
            dy,
            magnitude: norm_mag,
            dominant_side,
            confidence: norm_mag,
        }
    }

    /// Compute alignment with an actual direction vector (from IC to placed component).
    ///
    /// Returns value in [-1.0, 1.0] where 1.0 is perfectly aligned, 0.0 is orthogonal,
    /// and -1.0 is completely opposing. If affinity is ambiguous, returns 0.0.
    #[must_use]
    pub fn alignment_with(&self, actual_dx: f64, actual_dy: f64) -> f64 {
        if self.dominant_side == PadSide::Ambiguous || self.magnitude < 1e-4 {
            return 0.0;
        }

        let actual_mag = (actual_dx * actual_dx + actual_dy * actual_dy).sqrt();
        if actual_mag < 1e-4 {
            return 0.0;
        }

        let ax = actual_dx / actual_mag;
        let ay = actual_dy / actual_mag;

        (self.dx * ax + self.dy * ay).clamp(-1.0, 1.0)
    }
}

/// Affinity relationship between a major IC and a connected destination component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentAffinity {
    pub ic_id: ComponentId,
    pub dest_id: ComponentId,
    pub dest_refdes: String,
    pub preferred_vector: AffinityVector,
    pub pin_count: usize,
    pub weight: f64,
}

/// Directional Affinity Engine. Precomputes pin-to-pad vectors and scores placements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectionalAffinityEngine {
    /// Affinities indexed by destination component ID.
    pub affinities: BTreeMap<ComponentId, Vec<ComponentAffinity>>,
}

/// Helper to identify power, ground, and global distribution nets that must not bias placement.
#[must_use]
pub fn is_power_or_ground_net(net: &Net) -> bool {
    let lower = net.name.to_ascii_lowercase();
    lower.contains("gnd")
        || lower.contains("vss")
        || lower == "0v"
        || lower.contains("vcc")
        || lower.contains("vdd")
        || lower.contains("vbus")
        || lower.contains("3v3")
        || lower.contains("5v")
        || lower.contains("1v8")
        || lower.contains("12v")
        || lower.contains("vbat")
        || lower.contains("pwr")
        || lower.contains("power")
        || net.voltage.is_some()
        || net.netclass.as_deref().is_some_and(|c| {
            c.eq_ignore_ascii_case("power") || c.eq_ignore_ascii_case("pwr")
        })
}

impl DirectionalAffinityEngine {
    /// Build engine by analyzing the board's nets and physical pad geometries.
    #[must_use]
    pub fn build(board: &Board) -> Self {
        let pad_geometries = extract_all_pad_geometries(board);

        let mut comp_map = BTreeMap::new();
        for comp in &board.components {
            comp_map.insert(comp.id, comp);
        }

        // Identify major IC anchors (components with >= 8 pins or refdes starting with 'U')
        let mut major_ics = Vec::new();
        for comp in &board.components {
            let pin_count = comp.part.as_ref().map_or(0, |p| p.pins.len());
            let is_major = (comp.refdes.starts_with('U') && pin_count >= 4) || pin_count >= 8;
            if is_major {
                major_ics.push(comp);
            }
        }

        let mut affinities: BTreeMap<ComponentId, Vec<ComponentAffinity>> = BTreeMap::new();

        for ic in &major_ics {
            let Some(ic_geom) = pad_geometries.get(&ic.id) else {
                continue;
            };

            // For each destination component, accumulate pin direction vectors from this IC
            let mut dest_vectors: BTreeMap<ComponentId, (f64, f64, usize, f64)> = BTreeMap::new();

            for net in &board.nets {
                // Ignore power/ground rails
                if is_power_or_ground_net(net) {
                    continue;
                }

                // Check if IC connects to this net
                let mut ic_pin_offsets = Vec::new();
                for ep in &net.endpoints {
                    if ep.component == ic.id {
                        if let Some(pad) = ic_geom.find_by_pin_id(ic, ep.pin) {
                            // Pad offset from courtyard center (rotation 0)
                            ic_pin_offsets.push(pad.rel_courtyard_nm);
                        }
                    }
                }

                if ic_pin_offsets.is_empty() {
                    continue;
                }

                // For each destination endpoint on this signal net
                for ep in &net.endpoints {
                    if ep.component == ic.id {
                        continue;
                    }

                    let Some(dest_comp) = comp_map.get(&ep.component) else {
                        continue;
                    };

                    // Skip decoupling caps: 2-pin passives connected to ground
                    let is_passive_cap = dest_comp.refdes.starts_with('C')
                        && dest_comp.part.as_ref().is_some_and(|p| p.pins.len() == 2);
                    if is_passive_cap {
                        // Check if dest_comp is also connected to ground
                        let connects_ground = board.nets.iter().any(|other_net| {
                            is_power_or_ground_net(other_net)
                                && other_net.endpoints.iter().any(|o_ep| o_ep.component == dest_comp.id)
                        });
                        if connects_ground {
                            continue;
                        }
                    }

                    // Weight factor: 1.0 per signal net connection
                    let weight = 1.0;

                    let entry = dest_vectors.entry(ep.component).or_insert((0.0, 0.0, 0, 0.0));
                    for pin_offset in &ic_pin_offsets {
                        let px = pin_offset.x_nm as f64;
                        let py = pin_offset.y_nm as f64;
                        let pin_dist = (px * px + py * py).sqrt().max(1.0);

                        // Normalized pin unit vector from IC center
                        let u_x = px / pin_dist;
                        let u_y = py / pin_dist;

                        entry.0 += u_x * weight;
                        entry.1 += u_y * weight;
                        entry.2 += 1;
                        entry.3 += weight;
                    }
                }
            }

            for (dest_id, (acc_x, acc_y, pin_count, total_weight)) in dest_vectors {
                let preferred_vector = AffinityVector::from_components(acc_x, acc_y, total_weight);
                let dest_refdes = comp_map.get(&dest_id).map_or("?".to_string(), |c| c.refdes.clone());

                affinities.entry(dest_id).or_default().push(ComponentAffinity {
                    ic_id: ic.id,
                    dest_id,
                    dest_refdes,
                    preferred_vector,
                    pin_count,
                    weight: total_weight,
                });
            }
        }

        Self { affinities }
    }

    /// Calculate directional affinity penalty cost for a candidate placement.
    ///
    /// Cost is proportional to (1.0 - alignment(preferred, actual)) scaled by weight.
    #[must_use]
    pub fn placement_penalty(&self, placements: &[ComponentPlacement]) -> i64 {
        let mut place_map = BTreeMap::new();
        for cp in placements {
            place_map.insert(cp.id, cp);
        }

        let mut total_penalty: i64 = 0;

        for (dest_id, aff_list) in &self.affinities {
            let Some(dest_place) = place_map.get(dest_id) else {
                continue;
            };

            for aff in aff_list {
                let Some(ic_place) = place_map.get(&aff.ic_id) else {
                    continue;
                };

                if aff.preferred_vector.dominant_side == PadSide::Ambiguous {
                    continue;
                }

                // Vector from IC center to placed destination component
                let (ic_cx, ic_cy) = (ic_place.center.x_nm as f64, ic_place.center.y_nm as f64);
                let (dest_cx, dest_cy) = (dest_place.center.x_nm as f64, dest_place.center.y_nm as f64);

                let actual_dx = dest_cx - ic_cx;
                let actual_dy = dest_cy - ic_cy;

                // Adjust preferred vector for IC rotation on the board
                let (pref_dx, pref_dy) = match ic_place.rotation {
                    Rotation::Zero => (aff.preferred_vector.dx, aff.preferred_vector.dy),
                    Rotation::Ninety => (aff.preferred_vector.dy, -aff.preferred_vector.dx),
                    Rotation::OneEighty => (-aff.preferred_vector.dx, -aff.preferred_vector.dy),
                    Rotation::TwoSeventy => (-aff.preferred_vector.dy, aff.preferred_vector.dx),
                };

                let rotated_pref = AffinityVector {
                    dx: pref_dx,
                    dy: pref_dy,
                    magnitude: aff.preferred_vector.magnitude,
                    dominant_side: aff.preferred_vector.dominant_side.rotate(ic_place.rotation),
                    confidence: aff.preferred_vector.confidence,
                };

                let alignment = rotated_pref.alignment_with(actual_dx, actual_dy);
                // Alignment ranges from -1.0 (opposite side) to +1.0 (exact same side)
                // (1.0 - alignment) in [0.0, 2.0]
                let factor = (1.0 - alignment).max(0.0);
                let penalty = (factor * (DEFAULT_AFFINITY_WEIGHT_NM as f64) * aff.weight) as i64;

                total_penalty = total_penalty.saturating_add(penalty);
            }
        }

        total_penalty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use synth_diagnostics::Span;
    use synth_geometry::{mm_to_nm, Point, Rotation};
    use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
    use synth_registry::{ElectricalType, Part, PartId, Pin, PinNumber};

    fn make_qfp_part() -> Part {
        let mut pins = Vec::new();
        // 4 pins: Pin 1 North, Pin 2 East, Pin 3 South, Pin 4 West
        let names = ["P_NORTH", "P_EAST", "P_SOUTH", "P_WEST"];
        for (i, name) in names.iter().enumerate() {
            pins.push(Pin {
                name: (*name).to_string(),
                number: PinNumber(format!("{}", i + 1)),
                electrical_type: ElectricalType::Bidirectional,
                capabilities: Vec::new(),
                required: false,
                footprint_only: false,
                physical_pads: Vec::new(),
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
                drive: None,
            });
        }
        Part {
            id: PartId("TEST_QFP".to_string()),
            kind: "mcu".to_string(),
            description: None,
            version: 1,
            lifecycle: synth_registry::Lifecycle::Active,
            signed_by: Vec::new(),
            substitutes: Vec::new(),
            mpn: Some("TEST_QFP".to_string()),
            lcsc_pn: None,
            pins,
            required_decoupling: Vec::new(),
            kicad_symbol: None,
            kicad_footprint: None,
            footprint_dimensions: None,
            operating_conditions: None,
            spice: None,
            provenance: None,
        }
    }

    fn make_test_board_with_connections(
        extra_components: Vec<Component>,
        nets: Vec<Net>,
    ) -> Board {
        let dummy_span = Span::new(0, 0);
        let mcu = Component {
            id: ComponentId(1),
            refdes: "U1".to_string(),
            kind: "mcu".to_string(),
            part: Some(make_qfp_part()),
            value: Some("TEST_MCU".to_string()),
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };

        let mut components = vec![mcu];
        components.extend(extra_components);

        Board {
            outline: None,
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "synthetic_affinity_test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            stackup: None,
            netclasses: Vec::new(),
            buses: Vec::new(),
            modules: Vec::new(),
            variants: Vec::new(),
            required_processes: BTreeSet::new(),
            source_span: dummy_span,
        }
    }

    fn make_net(id: u32, name: &str, p1: (u32, u32), p2: (u32, u32)) -> Net {
        let dummy_span = Span::new(0, 0);
        Net {
            id: NetId(id),
            name: name.to_string(),
            auto_named: false,
            endpoints: vec![
                NetEndpoint {
                    component: ComponentId(p1.0),
                    pin: PinId(p1.1),
                    source_span: dummy_span,
                },
                NetEndpoint {
                    component: ComponentId(p2.0),
                    pin: PinId(p2.1),
                    source_span: dummy_span,
                },
            ],
            netclass: None,
            voltage: None,
        }
    }

    #[test]
    fn test_cardinal_destinations_affinity() {
        let dummy_span = Span::new(0, 0);
        // Dest 10 -> North (Pin 0 of MCU)
        let c_north = Component {
            id: ComponentId(10),
            refdes: "R_N".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        // Dest 11 -> South (Pin 2 of MCU)
        let c_south = Component {
            id: ComponentId(11),
            refdes: "R_S".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        // Dest 12 -> East (Pin 1 of MCU)
        let c_east = Component {
            id: ComponentId(12),
            refdes: "R_E".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        // Dest 13 -> West (Pin 3 of MCU)
        let c_west = Component {
            id: ComponentId(13),
            refdes: "R_W".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };

        let nets = vec![
            make_net(1, "NET_N", (1, 0), (10, 0)),
            make_net(2, "NET_S", (1, 2), (11, 0)),
            make_net(3, "NET_E", (1, 1), (12, 0)),
            make_net(4, "NET_W", (1, 3), (13, 0)),
        ];

        let board = make_test_board_with_connections(
            vec![c_north, c_south, c_east, c_west],
            nets,
        );

        let engine = DirectionalAffinityEngine::build(&board);

        // Verify North affinity
        let aff_n = &engine.affinities[&ComponentId(10)][0];
        assert_eq!(aff_n.preferred_vector.dominant_side, PadSide::North);

        // Verify South affinity
        let aff_s = &engine.affinities[&ComponentId(11)][0];
        assert_eq!(aff_s.preferred_vector.dominant_side, PadSide::South);

        // Verify East affinity
        let aff_e = &engine.affinities[&ComponentId(12)][0];
        assert_eq!(aff_e.preferred_vector.dominant_side, PadSide::East);

        // Verify West affinity
        let aff_w = &engine.affinities[&ComponentId(13)][0];
        assert_eq!(aff_w.preferred_vector.dominant_side, PadSide::West);
    }

    #[test]
    fn test_penalty_calculation_aligned_vs_opposing() {
        let dummy_span = Span::new(0, 0);
        let dest = Component {
            id: ComponentId(10),
            refdes: "R_N".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        let nets = vec![make_net(1, "SIG_N", (1, 0), (10, 0))];
        let board = make_test_board_with_connections(vec![dest], nets);
        let engine = DirectionalAffinityEngine::build(&board);

        let mcu_center = Point::new(mm_to_nm(20.0), mm_to_nm(20.0));

        // Placed on North side (y < mcu_center.y in KiCad coords)
        let aligned_placements = vec![
            ComponentPlacement {
                id: ComponentId(1),
                center: mcu_center,
                rotation: Rotation::Zero,
                layer: synth_geometry::Layer::Top,
            },
            ComponentPlacement {
                id: ComponentId(10),
                center: Point::new(mm_to_nm(20.0), mm_to_nm(10.0)), // North (dy = -10mm)
                rotation: Rotation::Zero,
                layer: synth_geometry::Layer::Top,
            },
        ];

        let aligned_penalty = engine.placement_penalty(&aligned_placements);
        assert_eq!(aligned_penalty, 0, "Aligned placement should have 0 penalty");

        // Placed on South side (opposing side, dy = +10mm)
        let opposing_placements = vec![
            ComponentPlacement {
                id: ComponentId(1),
                center: mcu_center,
                rotation: Rotation::Zero,
                layer: synth_geometry::Layer::Top,
            },
            ComponentPlacement {
                id: ComponentId(10),
                center: Point::new(mm_to_nm(20.0), mm_to_nm(30.0)), // South
                rotation: Rotation::Zero,
                layer: synth_geometry::Layer::Top,
            },
        ];

        let opposing_penalty = engine.placement_penalty(&opposing_placements);
        assert!(opposing_penalty > 0, "Opposing placement must produce positive penalty");
        assert_eq!(opposing_penalty, DEFAULT_AFFINITY_WEIGHT_NM * 2);
    }

    #[test]
    fn test_rotated_mcu_affinity_alignment() {
        let dummy_span = Span::new(0, 0);
        let dest = Component {
            id: ComponentId(10),
            refdes: "R_N".to_string(),
            kind: "resistor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        let nets = vec![make_net(1, "SIG_N", (1, 0), (10, 0))];
        let board = make_test_board_with_connections(vec![dest], nets);
        let engine = DirectionalAffinityEngine::build(&board);

        let mcu_center = Point::new(mm_to_nm(20.0), mm_to_nm(20.0));

        // When MCU is rotated 90 deg, North side rotates to West (-X)
        let rotated_placements = vec![
            ComponentPlacement {
                id: ComponentId(1),
                center: mcu_center,
                rotation: Rotation::Ninety,
                layer: synth_geometry::Layer::Top,
            },
            ComponentPlacement {
                id: ComponentId(10),
                center: Point::new(mm_to_nm(10.0), mm_to_nm(20.0)), // West (dx = -10mm)
                rotation: Rotation::Zero,
                layer: synth_geometry::Layer::Top,
            },
        ];

        let penalty = engine.placement_penalty(&rotated_placements);
        assert_eq!(penalty, 0, "Placing on the rotated side should produce 0 penalty");
    }

    #[test]
    fn test_power_ground_and_ambiguous_affinity() {
        let dummy_span = Span::new(0, 0);
        // Component connected to GND and VDD only
        let cap = Component {
            id: ComponentId(20),
            refdes: "C1".to_string(),
            kind: "capacitor".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        let nets = vec![
            make_net(1, "GND", (1, 0), (20, 0)),
            make_net(2, "3V3", (1, 2), (20, 1)),
        ];
        let board = make_test_board_with_connections(vec![cap], nets);
        let engine = DirectionalAffinityEngine::build(&board);

        // Power rails are filtered out; C1 should have no directional net affinity
        assert!(!engine.affinities.contains_key(&ComponentId(20)));
    }

    #[test]
    fn test_multi_net_affinity_weight() {
        let dummy_span = Span::new(0, 0);
        let dest = Component {
            id: ComponentId(30),
            refdes: "U_FLASH".to_string(),
            kind: "ic".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        // Connect dest to 2 nets both originating from Pin 1 (East)
        let nets = vec![
            make_net(1, "SPI_SCK", (1, 1), (30, 0)),
            make_net(2, "SPI_MISO", (1, 1), (30, 1)),
        ];
        let board = make_test_board_with_connections(vec![dest], nets);
        let engine = DirectionalAffinityEngine::build(&board);

        let aff = &engine.affinities[&ComponentId(30)][0];
        assert_eq!(aff.preferred_vector.dominant_side, PadSide::East);
        assert_eq!(aff.pin_count, 2);
        assert_eq!(aff.weight, 2.0);
    }

    #[test]
    fn test_distributed_opposing_affinity_ambiguous() {
        let dummy_span = Span::new(0, 0);
        let dest = Component {
            id: ComponentId(40),
            refdes: "U_SPLIT".to_string(),
            kind: "ic".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        // Connect dest to Pin 0 (North) AND Pin 2 (South) equally
        let nets = vec![
            make_net(1, "NET_N", (1, 0), (40, 0)),
            make_net(2, "NET_S", (1, 2), (40, 1)),
        ];
        let board = make_test_board_with_connections(vec![dest], nets);
        let engine = DirectionalAffinityEngine::build(&board);

        let aff = &engine.affinities[&ComponentId(40)][0];
        // Symmetrical opposing pulls result in ambiguous classification
        assert_eq!(aff.preferred_vector.dominant_side, PadSide::Ambiguous);
    }

    #[test]
    fn test_affinity_engine_deterministic() {
        let dummy_span = Span::new(0, 0);
        let dest = Component {
            id: ComponentId(50),
            refdes: "U_DET".to_string(),
            kind: "ic".to_string(),
            part: None,
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: None,
            sheet: None,
            source_span: dummy_span,
        };
        let nets = vec![make_net(1, "SIG_1", (1, 3), (50, 0))];
        let board = make_test_board_with_connections(vec![dest], nets);

        let engine1 = DirectionalAffinityEngine::build(&board);
        let engine2 = DirectionalAffinityEngine::build(&board);
        assert_eq!(engine1, engine2);
    }
}
