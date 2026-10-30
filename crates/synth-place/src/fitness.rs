// SPDX-License-Identifier: Apache-2.0

//! Placement fitness: measure what makes a placement electrically placed.
//!
//! Four dimensions, each normalized to `[0, 1]` and weighted equally:
//! support distance (a bound capacitor's pad to the anchor pad it serves),
//! rail-band discipline (passives parked in the top connector lane), mating
//! inset (a connector body to the edge it must reach) and area efficiency
//! (placed courtyard envelope against the outline).
//!
//! The report is an agent-facing measurement, not an export gate or a ranking
//! key. `gate_failures` is reported separately and is not folded into
//! `score`, so one stranded capacitor cannot be averaged away by three healthy
//! metrics.
//!
//! Provenance: the idea of a multi-metric placement report came from
//! reviewing the Copperhead tool. This implementation, its definitions,
//! thresholds, vocabulary and code are original; no Copperhead source,
//! identifier, comment or constant is reproduced.

#![allow(clippy::cast_precision_loss)] // Board counts are far below 2^53.

use serde::{Deserialize, Serialize};

use synth_geometry::{mm_to_nm, nm_to_mm, Point, Rect};
use synth_ir::{recognize_clusters, Board, ComponentId, MemberRole, PinId};
use synth_registry::{MatingFace, Part};

use crate::{fallback_courtyard, Placement};

/// Default pad-to-pad limit for a decoupling or rail capacitor.
const SUPPORT_LIMIT_MM: f64 = 3.0;
/// Default pad-to-pad limit for a crystal load or orphan rail capacitor.
const LOOSE_SUPPORT_LIMIT_MM: f64 = 5.0;
/// Connector body inset from the edge it mates with.
const MATING_LIMIT_MM: f64 = 3.0;
/// Top-edge strip: depth as a share of board height, bounded in millimetres.
const RAIL_BAND_FRACTION: f64 = 0.10;
const RAIL_BAND_MIN_MM: f64 = 4.0;
const RAIL_BAND_MAX_MM: f64 = 10.0;
/// A passive this close to a connector is serving it, not parked in the lane.
const CONNECTOR_SERVICE_MM: f64 = 2.0;
/// Occupancy the area score reaches 1.0 at; matches the existing sparse-board
/// warning of an outline 1.8x the component envelope.
const AREA_TARGET_OCCUPANCY: f64 = 0.55;

/// A placement's fitness report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementFitness {
    /// Weighted composite in `[0, 100]` over the metrics that could be
    /// measured.
    pub score: f64,
    /// Number of measured values over their limit. Not folded into `score`.
    pub gate_failures: usize,
    /// Fixed order: support distance, rail band, mating inset, area.
    pub metrics: Vec<FitnessMetric>,
    /// What could not be measured, and which fallbacks were used.
    pub assumptions: Vec<String>,
}

/// One row of the fitness report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitnessMetric {
    pub key: String,
    /// The worst reported measurement, in millimetres or a ratio.
    pub measured: f64,
    /// The mean measurement when the metric averages over pairs; equal to
    /// `measured` for a single-valued metric.
    pub mean: f64,
    /// The limit the measurement was judged against, when one applies.
    pub limit: Option<f64>,
    /// Values over their limit. Zero for a metric that is not a gate.
    #[serde(default)]
    pub failures: usize,
    /// Normalized goodness in `[0, 1]`.
    pub score: f64,
    /// Share of the composite after renormalizing over measured metrics.
    pub weight: f64,
    /// `weight * score`, in composite points.
    pub contribution: f64,
}

/// Measure `placement` on `board`.
#[must_use]
pub fn fitness(board: &Board, placement: &Placement) -> PlacementFitness {
    let mut assumptions = Vec::new();
    let mut rows = Vec::new();
    let mut gate_failures = 0;

    let fallback_courtyards = board
        .components
        .iter()
        .filter(|component| {
            component.part.as_ref().is_none_or(|part| {
                part.kicad_footprint
                    .as_deref()
                    .and_then(synth_layout::kicad_footprint_loader::courtyard_rect)
                    .is_none()
                    && part.footprint_dimensions.is_none()
            })
        })
        .count();
    if fallback_courtyards > 0 {
        assumptions.push(format!(
            "{fallback_courtyards} part(s) had no footprint geometry; fallback courtyards were used"
        ));
    }

    let by_id: std::collections::HashMap<ComponentId, &crate::ComponentPlacement> =
        placement.components.iter().map(|p| (p.id, p)).collect();
    let pad_offsets = crate::build_pad_offset_lookup(board);

    let support = support_distance(board, &by_id, &pad_offsets, &mut assumptions);
    if let Some(row) = support {
        rows.push(row);
    }

    let connectors: Vec<(&synth_ir::Component, Rect)> = board
        .components
        .iter()
        .filter(|c| c.kind == "connector")
        .filter_map(|c| {
            let placed = by_id.get(&c.id)?;
            Some((c, courtyard_of(c, placed)))
        })
        .collect();

    if let Some(row) = rail_band(board, placement, &by_id, &connectors) {
        rows.push(row);
    } else {
        assumptions.push(
            "no two-pin passives outside connector service; rail_band was not measured".to_string(),
        );
    }

    if let Some(row) = mating_inset(placement, &by_id, &connectors, &mut assumptions) {
        rows.push(row);
    } else {
        assumptions.push("no connectors were placed; mating_inset was not measured".to_string());
    }

    if let Some(row) = area_efficiency(placement, &by_id, board) {
        rows.push(row);
    } else {
        assumptions.push("no placed courtyards; area_efficiency was not measured".to_string());
    }

    gate_failures += rows.iter().map(|row| row.failures).sum::<usize>();
    let measured = rows.len();
    if measured == 0 {
        assumptions.push("no metric could be measured; fitness_score is 0".to_string());
    }
    let weight = if measured == 0 {
        0.0
    } else {
        1.0 / measured as f64
    };
    for row in &mut rows {
        row.weight = weight;
        row.contribution = weight * row.score;
    }
    let score = 100.0 * rows.iter().map(|r| r.contribution).sum::<f64>();
    // Keep an empty report at +0.0 rather than the negative zero a clamped
    // contribution can carry.
    let score = if score <= 0.0 { 0.0 } else { score };

    PlacementFitness {
        score,
        gate_failures,
        metrics: rows,
        assumptions,
    }
}

/// Roles the support-distance metric binds to their anchor.
#[must_use]
fn is_support_role(role: MemberRole) -> bool {
    matches!(
        role,
        MemberRole::DecouplingCap
            | MemberRole::RailCap
            | MemberRole::OrphanRailCap
            | MemberRole::LoadCap
    )
}

/// Pad-to-pad support distance for every bound capacitor.
///
/// Pairs come from the cluster recognizer's `MemberBinding`, so a capacitor is
/// judged against the pin that claimed it, not every IC on a merged rail.
fn support_distance(
    board: &Board,
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    pad_offsets: &crate::PadOffsetLookup,
    assumptions: &mut Vec<String>,
) -> Option<FitnessMetric> {
    let mut distances = Vec::new();
    let mut raw_distances = Vec::new();
    let mut failures = 0_usize;
    let mut missing_pads = 0_usize;
    let mut worst: Option<(f64, f64, f64)> = None;

    for cluster in recognize_clusters(board) {
        let Some(anchor) = by_id.get(&cluster.anchor) else {
            continue;
        };
        for member in &cluster.members {
            if !is_support_role(member.role) {
                continue;
            }
            let Some(placed_member) = by_id.get(&member.component) else {
                continue;
            };
            let anchor_offset =
                pad_offsets.lookup(cluster.anchor, member.binding.anchor_pin.0 as usize);
            let member_offset =
                pad_offsets.lookup(member.component, member.binding.member_pin.0 as usize);
            if anchor_offset.is_none() || member_offset.is_none() {
                missing_pads += 1;
            }
            let anchor_pad = pad_position(anchor, anchor_offset);
            let member_pad = pad_position(placed_member, member_offset);
            let distance_mm = nm_to_mm(pad_distance_nm(anchor_pad, member_pad));
            let limit = support_limit(
                board
                    .component(cluster.anchor)
                    .and_then(|c| c.part.as_ref()),
                member.binding.anchor_pin,
                member.role,
            );
            distances.push(clamp_unit(1.0 - distance_mm / limit));
            raw_distances.push(distance_mm);
            if distance_mm > limit {
                failures += 1;
            }
            // Report the binding that is worst relative to its own limit, so
            // a 4.9 mm rail cap cannot hide a 3.1 mm decoupling failure.
            let ratio = distance_mm / limit;
            if worst.is_none_or(|(worst_ratio, _, _)| ratio > worst_ratio) {
                worst = Some((ratio, distance_mm, limit));
            }
        }
    }

    if distances.is_empty() {
        assumptions.push(
            "no bound support capacitor was placed; support_distance was not measured".to_string(),
        );
        return None;
    }

    if missing_pads > 0 {
        assumptions.push(format!(
            "{missing_pads} support binding(s) had no footprint pad; component centres were used"
        ));
    }
    let (_, measured, limit) = worst.expect("a non-empty distance list has a worst pair");
    let score = distances.iter().sum::<f64>() / distances.len() as f64;
    let mean = raw_distances.iter().sum::<f64>() / raw_distances.len() as f64;
    Some(FitnessMetric {
        key: "support_distance".to_string(),
        measured,
        mean,
        limit: Some(limit),
        failures,
        score,
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Share of two-pin passives parked in the top-edge connector lane.
fn rail_band(
    board: &Board,
    placement: &Placement,
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    connectors: &[(&synth_ir::Component, Rect)],
) -> Option<FitnessMetric> {
    let height_mm = nm_to_mm(placement.board_outline.height_nm());
    let depth_mm = (RAIL_BAND_FRACTION * height_mm).clamp(RAIL_BAND_MIN_MM, RAIL_BAND_MAX_MM);
    let band_max_y = placement.board_outline.min.y_nm + mm_to_nm(depth_mm);
    let service_nm = mm_to_nm(CONNECTOR_SERVICE_MM);

    let mut passives = 0_usize;
    let mut in_band = 0_usize;
    for component in &board.components {
        let Some(placed) = by_id.get(&component.id) else {
            continue;
        };
        let two_pin_passive = matches!(
            component.kind.as_str(),
            "resistor" | "capacitor" | "inductor" | "diode"
        ) && component
            .part
            .as_ref()
            .is_some_and(|part| part.pins.len() == 2);
        if !two_pin_passive {
            continue;
        }
        let courtyard = courtyard_of(component, placed);
        let serves_connector = connectors.iter().any(|(_, connector)| {
            let expanded = Rect::new(
                Point::new(
                    connector.min.x_nm - service_nm,
                    connector.min.y_nm - service_nm,
                ),
                Point::new(
                    connector.max.x_nm + service_nm,
                    connector.max.y_nm + service_nm,
                ),
            );
            expanded.intersects(&courtyard)
        });
        // A passive serving its own connector is not part of the lane, in the
        // numerator or the denominator.
        if serves_connector {
            continue;
        }
        passives += 1;
        // `placed.center` is the courtyard-bbox centre (see ComponentPlacement).
        if placed.center.y_nm <= band_max_y {
            in_band += 1;
        }
    }

    if passives == 0 {
        return None;
    }
    let ratio = in_band as f64 / passives as f64;
    Some(FitnessMetric {
        key: "rail_band".to_string(),
        measured: ratio,
        mean: ratio,
        limit: Some(0.0),
        failures: usize::from(ratio > 0.0),
        score: clamp_unit(1.0 - ratio),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Worst connector body inset, averaged per connector.
fn mating_inset(
    placement: &Placement,
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    connectors: &[(&synth_ir::Component, Rect)],
    assumptions: &mut Vec<String>,
) -> Option<FitnessMetric> {
    if connectors.is_empty() {
        return None;
    }
    let mut scores = Vec::new();
    let mut raw_insets = Vec::new();
    let mut min_inset = f64::INFINITY;
    let mut max_inset = f64::NEG_INFINITY;
    for (component, _) in connectors {
        let (inset, declared) = mating_distance(component, placement, by_id);
        if !declared {
            assumptions.push(format!(
                "{} has no declared mating face; the nearest outline edge was used",
                component.refdes
            ));
        }
        let placed = by_id
            .get(&component.id)
            .expect("connector was filtered on placement");
        let outside = outside_inset(component, placed, placement.board_outline);
        let effective = outside.map_or(inset, |(_, distance)| distance);
        min_inset = min_inset.min(effective);
        max_inset = max_inset.max(effective);
        raw_insets.push(effective);
        // A body outside the outline is a defect, not a perfect edge inset.
        scores.push(if outside.is_some() || effective < 0.0 {
            0.0
        } else {
            clamp_unit(1.0 - effective / MATING_LIMIT_MM)
        });
    }

    if scores.is_empty() {
        return None;
    }
    Some(FitnessMetric {
        key: "mating_inset".to_string(),
        // A negative inset is the strongest signal; otherwise the largest
        // inset is the worst.
        measured: if min_inset < 0.0 {
            min_inset
        } else {
            max_inset
        },
        mean: raw_insets.iter().sum::<f64>() / raw_insets.len() as f64,
        limit: Some(MATING_LIMIT_MM),
        failures: raw_insets
            .iter()
            .filter(|inset| **inset < 0.0 || **inset > MATING_LIMIT_MM)
            .count(),
        score: scores.iter().sum::<f64>() / scores.len() as f64,
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Placed courtyard envelope against the board outline.
fn area_efficiency(
    placement: &Placement,
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    board: &Board,
) -> Option<FitnessMetric> {
    let mut min = Point::new(i64::MAX, i64::MAX);
    let mut max = Point::new(i64::MIN, i64::MIN);
    let mut found = false;
    for component in &board.components {
        let Some(placed) = by_id.get(&component.id) else {
            continue;
        };
        let rect = courtyard_of(component, placed);
        min = Point::new(min.x_nm.min(rect.min.x_nm), min.y_nm.min(rect.min.y_nm));
        max = Point::new(max.x_nm.max(rect.max.x_nm), max.y_nm.max(rect.max.y_nm));
        found = true;
    }
    if !found {
        return None;
    }
    let envelope_nm2 = envelope_area_nm2(min, max);
    let outline_nm2 = placement
        .board_outline
        .width_nm()
        .saturating_mul(placement.board_outline.height_nm());
    if outline_nm2 <= 0 || envelope_nm2 <= 0 {
        return None;
    }
    let occupancy = envelope_nm2 as f64 / outline_nm2 as f64;
    Some(FitnessMetric {
        key: "area_efficiency".to_string(),
        measured: occupancy,
        mean: occupancy,
        limit: Some(AREA_TARGET_OCCUPANCY),
        failures: 0,
        score: clamp_unit(occupancy / AREA_TARGET_OCCUPANCY),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Pad-to-pad limit for one binding.
fn support_limit(anchor: Option<&Part>, anchor_pin: PinId, role: MemberRole) -> f64 {
    if let Some(part) = anchor {
        if let Some(pin) = part.pins.get(anchor_pin.0 as usize) {
            if let Some(rule) = part
                .required_decoupling
                .iter()
                .find(|rule| rule.net == pin.name)
            {
                if let Some(limit) = rule.max_distance_mm {
                    if limit > 0.0 {
                        return limit;
                    }
                }
            }
        }
    }
    match role {
        MemberRole::RailCap | MemberRole::LoadCap | MemberRole::OrphanRailCap => {
            LOOSE_SUPPORT_LIMIT_MM
        }
        _ => SUPPORT_LIMIT_MM,
    }
}

/// Connector body inset and whether the mating face was declared.
fn mating_distance(
    component: &synth_ir::Component,
    placement: &Placement,
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
) -> (f64, bool) {
    let Some(placed) = by_id.get(&component.id) else {
        return (0.0, false);
    };
    let declared = board_mating_face(component, placed).is_some();
    let (_, inset) = mating_edge_and_inset(component, placed, placement.board_outline);
    (inset, declared)
}

/// The mating face in board axes, when the part declares one.
fn board_mating_face(
    component: &synth_ir::Component,
    placed: &crate::ComponentPlacement,
) -> Option<MatingFace> {
    component
        .part
        .as_ref()
        .and_then(|part| part.footprint_dimensions.as_ref())
        .and_then(|dimensions| dimensions.mating_face)
        .map(|face| rotate_face(face, placed.rotation))
}

/// The most negative edge inset, naming an edge the body has left. `None`
/// when the courtyard is wholly inside the outline.
fn outside_inset(
    component: &synth_ir::Component,
    placed: &crate::ComponentPlacement,
    outline: Rect,
) -> Option<(MatingFace, f64)> {
    let body = courtyard_of(component, placed);
    let edges = [
        (MatingFace::Left, body.min.x_nm - outline.min.x_nm),
        (MatingFace::Right, outline.max.x_nm - body.max.x_nm),
        (MatingFace::Top, body.min.y_nm - outline.min.y_nm),
        (MatingFace::Bottom, outline.max.y_nm - body.max.y_nm),
    ];
    edges
        .into_iter()
        .filter(|(_, distance)| *distance < 0)
        .min_by_key(|(_, distance)| *distance)
        .map(|(face, distance)| (face, nm_to_mm(distance)))
}

/// The edge a connector faces and the body inset on that edge. An undeclared
/// mating face falls back to the nearest outline edge.
fn mating_edge_and_inset(
    component: &synth_ir::Component,
    placed: &crate::ComponentPlacement,
    outline: Rect,
) -> (MatingFace, f64) {
    let body = courtyard_of(component, placed);
    let left = body.min.x_nm - outline.min.x_nm;
    let right = outline.max.x_nm - body.max.x_nm;
    let top = body.min.y_nm - outline.min.y_nm;
    let bottom = outline.max.y_nm - body.max.y_nm;
    if let Some(face) = board_mating_face(component, placed) {
        let inset = match face {
            MatingFace::Top => top,
            MatingFace::Bottom => bottom,
            MatingFace::Left => left,
            MatingFace::Right => right,
        };
        return (face, nm_to_mm(inset));
    }
    let edges = [
        (MatingFace::Left, left),
        (MatingFace::Right, right),
        (MatingFace::Top, top),
        (MatingFace::Bottom, bottom),
    ];
    let (face, inset) = edges
        .into_iter()
        .min_by_key(|(_, inset)| *inset)
        .expect("four edges");
    (face, nm_to_mm(inset))
}

/// A footprint-local mating face in board axes after the placement rotation.
fn rotate_face(face: MatingFace, rotation: synth_geometry::Rotation) -> MatingFace {
    let (x, y) = match face {
        MatingFace::Top => (0_i64, -1_i64),
        MatingFace::Right => (1, 0),
        MatingFace::Bottom => (0, 1),
        MatingFace::Left => (-1, 0),
    };
    let (rx, ry) = rotation.rotate_offset(x, y);
    match (rx, ry) {
        (0, -1) => MatingFace::Top,
        (1, 0) => MatingFace::Right,
        (0, 1) => MatingFace::Bottom,
        _ => MatingFace::Left,
    }
}

/// Placed courtyard of one component.
fn courtyard_of(component: &synth_ir::Component, placed: &crate::ComponentPlacement) -> Rect {
    let (width, height, offset) = component.part.as_ref().map_or_else(
        || {
            let (width, height) = fallback_courtyard(&component.kind);
            (width, height, None)
        },
        |part| {
            let ((offset_x, offset_y), (width, height)) =
                synth_layout::pcb_courtyard_geometry_for_part(part);
            (width, height, Some((offset_x, offset_y)))
        },
    );
    crate::courtyard_rect_for_placement(placed, width, height, offset, 0)
}

/// Placed pad position: courtyard centre plus the rotated localized pad offset.
fn pad_position(placed: &crate::ComponentPlacement, offset: Option<(i64, i64)>) -> Point {
    crate::apply_rotation(*placed, offset.unwrap_or((0, 0)))
}

fn pad_distance_nm(a: Point, b: Point) -> i64 {
    let dx = i128::from(a.x_nm) - i128::from(b.x_nm);
    let dy = i128::from(a.y_nm) - i128::from(b.y_nm);
    let squared = dx * dx + dy * dy;
    // Integer square root keeps the measurement deterministic (no float in
    // geometry); millimetres are produced at the report boundary.
    isqrt_i128(squared).try_into().unwrap_or(i64::MAX)
}

/// Floor integer square root, exact for the board-scale values used here.
fn isqrt_i128(value: i128) -> i128 {
    if value <= 0 {
        return 0;
    }
    let mut root = value;
    let mut next = (root + 1) / 2;
    while next < root {
        root = next;
        next = (root + value / root) / 2;
    }
    root
}

fn envelope_area_nm2(min: Point, max: Point) -> i64 {
    let width = (max.x_nm - min.x_nm).max(0);
    let height = (max.y_nm - min.y_nm).max(0);
    width.saturating_mul(height)
}

fn clamp_unit(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_geometry::Rotation;

    #[test]
    fn mating_face_rotates_with_the_footprint() {
        assert_eq!(
            rotate_face(MatingFace::Bottom, Rotation::Zero),
            MatingFace::Bottom
        );
        assert_eq!(
            rotate_face(MatingFace::Bottom, Rotation::Ninety),
            MatingFace::Right
        );
        assert_eq!(
            rotate_face(MatingFace::Bottom, Rotation::OneEighty),
            MatingFace::Top
        );
        assert_eq!(
            rotate_face(MatingFace::Bottom, Rotation::TwoSeventy),
            MatingFace::Left
        );
    }
}
