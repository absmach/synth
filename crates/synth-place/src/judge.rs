// SPDX-License-Identifier: Apache-2.0

//! Placement quality judge: the layout numbers a round is accepted on.
//!
//! `fitness` answers whether the parts sit where the circuit needs them.
//! This module answers the complementary question a placement round asks:
//! is the finished arrangement tidy and usable? It measures alignment, the
//! decoupling ring, dead space, dead pockets, outline occupancy, connector
//! docking and centring, review-group gaps, header rows, overlaps, parts
//! outside the outline and mounting-hole corners.
//!
//! The judge is a pure measurement. It never moves a part, never selects a
//! candidate and is not called by [`crate::place`]. A report carries a 0–100
//! composite over the measured quality rows plus a separate `gate_failures`
//! count and `passed` flag, so a structural defect cannot be averaged away by
//! good soft numbers. Later rounds compare two placements on the same board by
//! `passed` first, then `score`, and by the individual rows for the reason.

#![allow(clippy::cast_precision_loss)] // Board counts and nanometres are far below 2^53.

use serde::{Deserialize, Serialize};
use synth_geometry::{Point, Rect, mm_to_nm, nm_to_mm};
use synth_ir::{Board, Component, ComponentId};

use crate::{
    Placement, apply_rotation, build_pad_offset_lookup, courtyard_rect_for_placement,
    fallback_courtyard,
};

/// A passive counts as aligned when its centre shares a row or column with a
/// peer in the same group to this tolerance.
const ALIGN_MM: f64 = 0.5;
/// A group capacitor counts as ringing its chip inside this courtyard gap.
const RING_MM: f64 = 3.0;
/// Every courtyard grows by this much before the largest empty rectangle is
/// measured, so a slot one routing channel wide is not reported as empty.
const EMPTY_INFLATE_MM: f64 = 1.0;
/// A connector is flush when its courtyard is this close to an edge.
const FLUSH_MM: f64 = 0.15;
/// Interior headers within this gap form one row.
const HEADER_CLUSTER_MM: f64 = 6.0;
/// A USB connector's body centre must sit this close to the edge midpoint.
const USB_CENTRE_LIMIT_MM: f64 = 0.5;
/// Review-group boxes must keep this gap.
const GROUP_GAP_LIMIT_MM: f64 = 2.0;
/// Headers on one edge or in one row must share their pin-1 line this closely.
const HEADER_ALIGN_LIMIT_MM: f64 = 0.5;
/// A connector this close to an edge counts as a wall connector.
const WALL_LIMIT_MM: f64 = 1.0;
/// A mounting hole further than this from every board corner is not a corner
/// hole. The nominal 3.5 mm inset puts the centre at `sqrt(2) * 3.5 = 4.95`.
const HOLE_CORNER_LIMIT_MM: f64 = 5.5;
/// Occupancy the area score reaches 1.0 at; matches `fitness`.
const AREA_TARGET_OCCUPANCY: f64 = 0.55;
/// Side of one dead-space raster cell.
const DEAD_CELL_MM: f64 = 0.5;
/// A cell is dead when its centre is farther than this from every courtyard
/// and from the outline edge.
const DEAD_CLEAR_MM: f64 = 2.5;
/// Pockets larger than this are reported as findings.
const DEAD_POCKET_REPORT_MM2: f64 = 4.0;

/// Component kinds the round metrics treated as chips.
const CHIP_KINDS: [&str; 5] = ["ic", "mcu", "sensor", "regulator", "memory"];
/// Component kinds the round metrics treated as passives.
const PASSIVE_KINDS: [&str; 4] = ["resistor", "capacitor", "inductor", "crystal"];

/// A finished placement's layout quality report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementJudgement {
    /// Equal-weight composite in `[0, 100]` over the rows that carry a score.
    pub score: f64,
    /// True when every hard gate passed. Acceptance uses this, not `score`.
    pub passed: bool,
    /// Sum of `JudgeMetric::failures`. Not folded into `score`.
    pub gate_failures: usize,
    /// Rows in a fixed order; see [`judge`].
    pub metrics: Vec<JudgeMetric>,
    /// What could not be measured, and which fallbacks were used.
    pub assumptions: Vec<String>,
    /// Connected empty regions more than `DEAD_CLEAR_MM` from every part
    /// and the outline edge, in reading order.
    #[serde(default)]
    pub dead_pockets: Vec<DeadPocket>,
    /// Total area of every dead raster cell, in square millimetres.
    #[serde(default)]
    pub dead_area_total: f64,
    /// `dead_area_total / outline area`, 0 for an empty outline.
    #[serde(default)]
    pub dead_area_ratio: f64,
    /// Actionable messages for the pockets larger than
    /// `DEAD_POCKET_REPORT_MM2`.
    #[serde(default)]
    pub findings: Vec<String>,
}

/// One empty region found by the dead-space raster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadPocket {
    pub area_mm2: f64,
    pub min_x_mm: f64,
    pub min_y_mm: f64,
    pub max_x_mm: f64,
    pub max_y_mm: f64,
    /// Up to two review groups nearest the pocket, nearest first.
    pub nearest_groups: Vec<String>,
}

/// One row of the judgement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeMetric {
    pub key: String,
    /// The reported measurement, in millimetres, square millimetres, a ratio,
    /// or a count depending on the row.
    pub measured: f64,
    /// The hard limit for a gated row; `None` for a reading or a soft score.
    pub limit: Option<f64>,
    /// Values outside `limit`. Zero for a row that does not gate.
    #[serde(default)]
    pub failures: usize,
    /// Normalized goodness in `[0, 1]`, or `None` for a reading that is not
    /// scored.
    pub score: Option<f64>,
    /// Share of the composite after renormalizing over scored rows.
    pub weight: f64,
    /// `weight * score`, in composite points.
    pub contribution: f64,
}

/// One placed component and its courtyard.
struct Part<'a> {
    component: &'a Component,
    rect: Rect,
}

/// Judge `placement` on `board` without touching it.
///
/// Row order: `outline_area`, `largest_empty`, `alignment`, `ring`,
/// `area_efficiency`, `docking`, `usb_offset`, `group_gap`,
/// `header_alignment`, `header_parallel`, `overlaps`, `outside_outline`,
/// `hole_corner`.
#[must_use]
pub fn judge(board: &Board, placement: &Placement) -> PlacementJudgement {
    let mut assumptions = Vec::new();
    let mut rows = Vec::new();

    let by_id: std::collections::HashMap<ComponentId, &crate::ComponentPlacement> =
        placement.components.iter().map(|p| (p.id, p)).collect();
    let parts: Vec<Part<'_>> = board
        .components
        .iter()
        .filter_map(|component| {
            let placed = by_id.get(&component.id)?;
            Some(Part {
                component,
                rect: courtyard_of(component, placed),
            })
        })
        .collect();

    let outline = placement.board_outline;
    let outline_area_mm2 = outline_area_nm2(outline) as f64 / 1e12;
    rows.push(JudgeMetric {
        key: "outline_area".into(),
        measured: outline_area_mm2,
        limit: None,
        failures: 0,
        score: None,
        weight: 0.0,
        contribution: 0.0,
    });
    if outline_area_mm2 <= 0.0 {
        assumptions.push("the board outline has no area; readings are not normalized".into());
    }

    let courts: Vec<(Rect, Option<&str>)> = parts
        .iter()
        .map(|part| (part.rect, part.component.group.as_deref()))
        .collect();
    let dead = dead_pockets_of(outline, &courts);
    let dead_area_ratio = if outline_area_mm2 > 0.0 {
        dead.total_mm2 / outline_area_mm2
    } else {
        0.0
    };
    let dead_findings: Vec<String> = dead
        .pockets
        .iter()
        .filter(|pocket| pocket.area_mm2 > DEAD_POCKET_REPORT_MM2)
        .map(|pocket| {
            let x = pocket.min_x_mm.midpoint(pocket.max_x_mm);
            let y = pocket.min_y_mm.midpoint(pocket.max_y_mm);
            format!(
                "pocket {:.1} mm2 at ({x:.1},{y:.1}) beside {}",
                pocket.area_mm2,
                pocket
                    .nearest_groups
                    .first()
                    .map_or("no group", String::as_str)
            )
        })
        .collect();

    let empty_nm2 = largest_empty_rect_nm2(outline, &parts, EMPTY_INFLATE_MM);
    let empty_mm2 = (empty_nm2 as f64) / 1e12;
    let compactness = if outline_area_mm2 > 0.0 {
        Some(clamp_unit(1.0 - empty_mm2 / outline_area_mm2))
    } else {
        None
    };
    rows.push(JudgeMetric {
        key: "largest_empty".into(),
        measured: empty_mm2,
        limit: None,
        failures: 0,
        score: compactness,
        weight: 0.0,
        contribution: 0.0,
    });

    rows.extend(group_quality_rows(board, &parts, &mut assumptions));

    if let Some(row) = area_efficiency(&parts, outline) {
        rows.push(row);
    } else {
        assumptions.push("no placed courtyards; area_efficiency was not measured".into());
    }

    if let Some(row) = docking(&parts, outline) {
        rows.push(row);
    } else {
        assumptions.push("no wired connector was placed; docking was not measured".into());
    }

    if let Some(row) = usb_centring(&parts, outline) {
        rows.push(row);
    } else {
        assumptions.push("no USB connector was placed; usb_offset was not measured".into());
    }

    if let Some(row) = group_gap(&parts, outline) {
        rows.push(row);
    } else {
        assumptions.push("fewer than two review-group boxes; group_gap was not measured".into());
    }

    if let Some(row) = header_alignment(board, outline, &parts, &by_id, &mut assumptions) {
        rows.push(row);
    } else {
        assumptions.push(
            "no header row with two measured pin-1 pads; header_alignment was not measured".into(),
        );
    }

    if let Some(row) = header_parallel(board, outline, &parts, &by_id, &mut assumptions) {
        rows.push(row);
    } else {
        assumptions.push("no connector sits on an edge; header_parallel was not measured".into());
    }

    let overlaps = overlaps(&parts);
    rows.push(JudgeMetric {
        key: "overlaps".into(),
        measured: overlaps.measured,
        limit: Some(0.0),
        failures: overlaps.count,
        score: None,
        weight: 0.0,
        contribution: 0.0,
    });

    let outside = outside_outline(&parts, outline);
    rows.push(JudgeMetric {
        key: "outside_outline".into(),
        measured: outside.measured,
        limit: Some(0.0),
        failures: outside.count,
        score: None,
        weight: 0.0,
        contribution: 0.0,
    });

    if let Some(row) = hole_corner(&parts, outline) {
        rows.push(row);
    } else {
        assumptions.push("no mounting hole was placed; hole_corner was not measured".into());
    }

    let gate_failures = rows.iter().map(|row| row.failures).sum::<usize>();
    let scored = rows.iter().filter(|row| row.score.is_some()).count();
    if scored == 0 {
        assumptions.push("no quality row could be scored; score is 0".into());
    }
    let weight = if scored == 0 {
        0.0
    } else {
        1.0 / scored as f64
    };
    for row in &mut rows {
        if let Some(score) = row.score {
            row.weight = weight;
            row.contribution = weight * score;
        }
    }
    let score = 100.0 * rows.iter().map(|row| row.contribution).sum::<f64>();
    let score = if score <= 0.0 { 0.0 } else { score };

    PlacementJudgement {
        score,
        passed: gate_failures == 0,
        gate_failures,
        metrics: rows,
        assumptions,
        dead_pockets: dead.pockets,
        dead_area_total: dead.total_mm2,
        dead_area_ratio,
        findings: dead_findings,
    }
}

/// Rows computed per chip-bearing review group: alignment and ring.
fn group_quality_rows(
    board: &Board,
    parts: &[Part<'_>],
    assumptions: &mut Vec<String>,
) -> Vec<JudgeMetric> {
    let mut aligned = 0_usize;
    let mut passives = 0_usize;
    let mut ringing = 0_usize;
    let mut capacitors = 0_usize;

    for group in &board.groups {
        let members: Vec<&Part<'_>> = parts
            .iter()
            .filter(|part| part.component.group.as_deref() == Some(group.name.as_str()))
            .collect();
        let chips: Vec<&Part<'_>> = members
            .iter()
            .copied()
            .filter(|part| CHIP_KINDS.contains(&part.component.kind.as_str()))
            .collect();
        if chips.is_empty() {
            continue;
        }
        let group_passives: Vec<&Part<'_>> = members
            .iter()
            .copied()
            .filter(|part| PASSIVE_KINDS.contains(&part.component.kind.as_str()))
            .collect();
        for (index, passive) in group_passives.iter().enumerate() {
            passives += 1;
            let centre = rect_centre(passive.rect);
            let aligned_with_peer = group_passives.iter().enumerate().any(|(other, peer)| {
                other != index && {
                    let peer_centre = rect_centre(peer.rect);
                    nm_to_mm((centre.x_nm - peer_centre.x_nm).abs()) <= ALIGN_MM
                        || nm_to_mm((centre.y_nm - peer_centre.y_nm).abs()) <= ALIGN_MM
                }
            });
            if aligned_with_peer {
                aligned += 1;
            }
        }
        for member in &members {
            if member.component.kind != "capacitor" {
                continue;
            }
            capacitors += 1;
            let near = chips
                .iter()
                .map(|chip| gap_nm(member.rect, chip.rect))
                .min()
                .unwrap_or(i64::MAX);
            if nm_to_mm(near) <= RING_MM {
                ringing += 1;
            }
        }
    }

    let mut rows = Vec::new();
    if passives > 0 {
        let ratio = aligned as f64 / passives as f64;
        rows.push(JudgeMetric {
            key: "alignment".into(),
            measured: ratio,
            limit: None,
            failures: 0,
            score: Some(clamp_unit(ratio)),
            weight: 0.0,
            contribution: 0.0,
        });
    } else {
        assumptions.push("no passive in a chip-bearing group; alignment was not measured".into());
    }
    if capacitors > 0 {
        let ratio = ringing as f64 / capacitors as f64;
        rows.push(JudgeMetric {
            key: "ring".into(),
            measured: ratio,
            limit: None,
            failures: 0,
            score: Some(clamp_unit(ratio)),
            weight: 0.0,
            contribution: 0.0,
        });
    } else {
        assumptions.push("no capacitor in a chip-bearing group; ring was not measured".into());
    }
    rows
}

/// Placed courtyard envelope against the outline.
fn area_efficiency(parts: &[Part<'_>], outline: Rect) -> Option<JudgeMetric> {
    let envelope = envelope(parts)?;
    let outline_area = outline_area_nm2(outline);
    if outline_area <= 0 {
        return None;
    }
    let occupancy = envelope_area_nm2(envelope) as f64 / outline_area as f64;
    Some(JudgeMetric {
        key: "area_efficiency".into(),
        measured: occupancy,
        limit: None,
        failures: 0,
        score: Some(clamp_unit(occupancy / AREA_TARGET_OCCUPANCY)),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Share of wired connectors flush on an edge.
fn docking(parts: &[Part<'_>], outline: Rect) -> Option<JudgeMetric> {
    let connectors: Vec<&Part<'_>> = parts
        .iter()
        .filter(|part| part.component.kind == "connector")
        .filter(|part| part.component.placement_hint.is_none())
        .collect();
    if connectors.is_empty() {
        return None;
    }
    let docked = connectors
        .iter()
        .filter(|part| {
            let (gap, _) = edge_of_rect(part.rect, outline);
            nm_to_mm(gap).abs() <= FLUSH_MM
        })
        .count();
    let ratio = docked as f64 / connectors.len() as f64;
    Some(JudgeMetric {
        key: "docking".into(),
        measured: ratio,
        limit: Some(1.0),
        failures: connectors.len() - docked,
        score: Some(clamp_unit(ratio)),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Worst USB body-centre offset from the midpoint of its edge.
fn usb_centring(parts: &[Part<'_>], outline: Rect) -> Option<JudgeMetric> {
    let usb: Vec<&Part<'_>> = parts.iter().filter(|part| is_usb(part.component)).collect();
    if usb.is_empty() {
        return None;
    }
    let mut worst: Option<f64> = None;
    let mut failures = 0_usize;
    for part in usb {
        let (_, edge) = edge_of_rect(part.rect, outline);
        let centre = rect_centre(part.rect);
        let mid = rect_centre(outline);
        let offset_mm = if matches!(edge, Edge::Top | Edge::Bottom) {
            nm_to_mm((centre.x_nm - mid.x_nm).abs())
        } else {
            nm_to_mm((centre.y_nm - mid.y_nm).abs())
        };
        if offset_mm > USB_CENTRE_LIMIT_MM {
            failures += 1;
        }
        worst = Some(worst.map_or(offset_mm, |value| value.max(offset_mm)));
    }
    let measured = worst.unwrap_or(0.0);
    Some(JudgeMetric {
        key: "usb_offset".into(),
        measured,
        limit: Some(USB_CENTRE_LIMIT_MM),
        failures,
        score: Some(clamp_unit(1.0 - measured / USB_CENTRE_LIMIT_MM)),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Least gap between review-group boxes.
fn group_gap(parts: &[Part<'_>], outline: Rect) -> Option<JudgeMetric> {
    let boxes = group_boxes(parts, outline);
    if boxes.len() < 2 {
        return None;
    }
    let mut least = i64::MAX;
    let mut failures = 0_usize;
    for (index, a) in boxes.iter().enumerate() {
        for b in &boxes[index + 1..] {
            let gap = gap_nm(*a, *b);
            least = least.min(gap);
            if nm_to_mm(gap) < GROUP_GAP_LIMIT_MM {
                failures += 1;
            }
        }
    }
    let measured = nm_to_mm(least);
    Some(JudgeMetric {
        key: "group_gap".into(),
        measured,
        limit: Some(GROUP_GAP_LIMIT_MM),
        failures,
        score: Some(clamp_unit(measured / GROUP_GAP_LIMIT_MM)),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Review-group boxes: a connector leaves the box when the group also holds a
/// major part, because a docked connector sits on its own edge. A mounting
/// hole seated on a board corner leaves too: its corner is the board's, not
/// the group's, and its distance from the group would otherwise span the
/// board.
fn group_boxes(parts: &[Part<'_>], outline: Rect) -> Vec<Rect> {
    let mut groups: std::collections::BTreeMap<&str, Vec<&Part<'_>>> =
        std::collections::BTreeMap::new();
    for part in parts {
        if let Some(group) = part.component.group.as_deref() {
            groups.entry(group).or_default().push(part);
        }
    }
    let mut boxes = Vec::new();
    for members in groups.values() {
        let has_major = members.iter().any(|part| {
            part.component.kind != "connector"
                && part
                    .component
                    .part
                    .as_ref()
                    .is_some_and(|part| part.pins.len() >= 16)
        });
        if let Some(rect) = crate::fold::group_box(
            members
                .iter()
                .map(|part| (part.component.kind.as_str(), part.rect)),
            has_major,
            outline,
        ) {
            boxes.push(rect);
        }
    }
    boxes
}

/// Worst pin-1 spread among headers on one edge or in one interior row.
fn header_alignment(
    board: &Board,
    outline: Rect,
    parts: &[Part<'_>],
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    assumptions: &mut Vec<String>,
) -> Option<JudgeMetric> {
    let headers: Vec<&Part<'_>> = parts
        .iter()
        .filter(|part| is_header(part.component))
        .collect();
    if headers.len() < 2 {
        return None;
    }
    let pad_offsets = build_pad_offset_lookup(board);
    let mut edge_groups: std::collections::BTreeMap<Edge, Vec<Point>> =
        std::collections::BTreeMap::new();
    let mut interior: Vec<(Rect, Point)> = Vec::new();
    let mut missing_pads = 0_usize;
    for part in &headers {
        let pin_one = part
            .component
            .part
            .as_ref()
            .and_then(|part| part.pins.iter().position(|pin| pin.number.0 == "1"))
            .and_then(|index| pad_offsets.lookup(part.component.id, index));
        let Some(offset) = pin_one else {
            missing_pads += 1;
            continue;
        };
        let placed = by_id
            .get(&part.component.id)
            .expect("header was filtered on placement");
        let point = apply_rotation(**placed, offset);
        let (gap, edge) = edge_of_rect(part.rect, outline);
        if nm_to_mm(gap) <= FLUSH_MM {
            edge_groups.entry(edge).or_default().push(point);
        } else {
            interior.push((part.rect, point));
        }
    }
    if missing_pads > 0 {
        assumptions.push(format!(
            "{missing_pads} header(s) had no pin-1 pad; header_alignment skipped them"
        ));
    }

    let mut spreads = Vec::new();
    for (edge, points) in &edge_groups {
        if points.len() < 2 {
            continue;
        }
        let axis = if matches!(edge, Edge::Top | Edge::Bottom) {
            Axis::Y
        } else {
            Axis::X
        };
        spreads.push(spread(points, axis));
    }
    // Interior headers form rows by proximity; a row shares one pin-1 line.
    let mut parent: Vec<usize> = (0..interior.len()).collect();
    for i in 0..interior.len() {
        for j in i + 1..interior.len() {
            if nm_to_mm(gap_nm(interior[i].0, interior[j].0)) <= HEADER_CLUSTER_MM {
                union(&mut parent, i, j);
            }
        }
    }
    let mut clusters: std::collections::BTreeMap<usize, Vec<Point>> =
        std::collections::BTreeMap::new();
    for (index, (_, point)) in interior.iter().enumerate() {
        let root = find(&mut parent, index);
        clusters.entry(root).or_default().push(*point);
    }
    for points in clusters.values() {
        if points.len() >= 2 {
            spreads.push(spread(points, Axis::Y));
        }
    }

    if spreads.is_empty() {
        return None;
    }
    let measured = spreads.iter().copied().fold(0.0_f64, f64::max);
    let failures = spreads
        .iter()
        .filter(|spread| **spread > HEADER_ALIGN_LIMIT_MM)
        .count();
    Some(JudgeMetric {
        key: "header_alignment".into(),
        measured,
        limit: Some(HEADER_ALIGN_LIMIT_MM),
        failures,
        score: Some(clamp_unit(1.0 - measured / HEADER_ALIGN_LIMIT_MM)),
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Wall connectors whose pin row runs across their wall.
///
/// The long axis is the pad row, not the courtyard box: a right-angle JST is
/// deep along its mating direction, so its courtyard is longest across the
/// wall even when the part is seated correctly.
fn header_parallel(
    board: &Board,
    outline: Rect,
    parts: &[Part<'_>],
    by_id: &std::collections::HashMap<ComponentId, &crate::ComponentPlacement>,
    assumptions: &mut Vec<String>,
) -> Option<JudgeMetric> {
    let walled: Vec<&Part<'_>> = parts
        .iter()
        .filter(|part| part.component.kind == "connector")
        .filter(|part| nm_to_mm(edge_of_rect(part.rect, outline).0) <= WALL_LIMIT_MM)
        .collect();
    if walled.is_empty() {
        return None;
    }
    let pad_offsets = build_pad_offset_lookup(board);
    let mut failures = 0_usize;
    let mut fallbacks = 0_usize;
    for part in walled {
        let (_, edge) = edge_of_rect(part.rect, outline);
        let Some(placed) = by_id.get(&part.component.id) else {
            continue;
        };
        let pins = part
            .component
            .part
            .as_ref()
            .map_or(&[][..], |part| part.pins.as_slice());
        let mut min = (i64::MAX, i64::MAX);
        let mut max = (i64::MIN, i64::MIN);
        let mut seen = 0_usize;
        for index in 0..pins.len() {
            let Some(offset) = pad_offsets.lookup(part.component.id, index) else {
                continue;
            };
            let point = apply_rotation(**placed, offset);
            min = (min.0.min(point.x_nm), min.1.min(point.y_nm));
            max = (max.0.max(point.x_nm), max.1.max(point.y_nm));
            seen += 1;
        }
        let horizontal = if seen >= 2 {
            (max.0 - min.0) >= (max.1 - min.1)
        } else {
            fallbacks += 1;
            part.rect.width_nm() >= part.rect.height_nm()
        };
        let along = matches!(edge, Edge::Top | Edge::Bottom);
        if horizontal != along {
            failures += 1;
        }
    }
    if fallbacks > 0 {
        assumptions.push(format!(
            "{fallbacks} wall connector(s) had no pad row; their courtyard was read instead"
        ));
    }
    Some(JudgeMetric {
        key: "header_parallel".into(),
        measured: failures as f64,
        limit: None,
        failures: 0,
        score: None,
        weight: 0.0,
        contribution: 0.0,
    })
}

/// Worst mounting-hole centre distance to a board corner.
fn hole_corner(parts: &[Part<'_>], outline: Rect) -> Option<JudgeMetric> {
    let holes: Vec<&Part<'_>> = parts
        .iter()
        .filter(|part| part.component.kind == "mounting_hole")
        .collect();
    if holes.is_empty() {
        return None;
    }
    let corners = [
        Point::new(outline.min.x_nm, outline.min.y_nm),
        Point::new(outline.max.x_nm, outline.min.y_nm),
        Point::new(outline.min.x_nm, outline.max.y_nm),
        Point::new(outline.max.x_nm, outline.max.y_nm),
    ];
    let mut worst = 0.0_f64;
    let mut failures = 0_usize;
    for hole in holes {
        let centre = rect_centre(hole.rect);
        let nearest = corners
            .iter()
            .map(|corner| {
                let dx = (centre.x_nm - corner.x_nm) as f64;
                let dy = (centre.y_nm - corner.y_nm) as f64;
                (dx * dx + dy * dy).sqrt() / 1e6
            })
            .fold(f64::INFINITY, f64::min);
        if nearest > HOLE_CORNER_LIMIT_MM {
            failures += 1;
        }
        worst = worst.max(nearest);
    }
    Some(JudgeMetric {
        key: "hole_corner".into(),
        measured: worst,
        limit: Some(HOLE_CORNER_LIMIT_MM),
        failures,
        score: None,
        weight: 0.0,
        contribution: 0.0,
    })
}

/// A measured count: the number as reported and as a gate failure count.
struct Count {
    measured: f64,
    count: usize,
}

fn overlaps(parts: &[Part<'_>]) -> Count {
    let mut count = 0_usize;
    for (index, a) in parts.iter().enumerate() {
        for b in &parts[index + 1..] {
            if courtyards_overlap(a.rect, b.rect) {
                count += 1;
            }
        }
    }
    Count {
        measured: count as f64,
        count,
    }
}

/// Touching courtyards are collisions here: the placer treats a shared
/// boundary as occupied, and the PCB exporter rounds coordinates to 0.001 mm,
/// so a zero gap can ship as a real overlap.
fn courtyards_overlap(a: Rect, b: Rect) -> bool {
    gap_nm(a, b) <= 0
}

fn outside_outline(parts: &[Part<'_>], outline: Rect) -> Count {
    let count = parts
        .iter()
        .filter(|part| !(outline.contains(part.rect.min) && outline.contains(part.rect.max)))
        .count();
    Count {
        measured: count as f64,
        count,
    }
}

/// Largest axis-aligned rectangle no inflated courtyard touches.
fn largest_empty_rect_nm2(outline: Rect, parts: &[Part<'_>], inflate_mm: f64) -> i64 {
    let inflate = synth_geometry::mm_to_nm(inflate_mm);
    let bx0 = outline.min.x_nm;
    let by0 = outline.min.y_nm;
    let bx1 = outline.max.x_nm;
    let by1 = outline.max.y_nm;
    if bx1 <= bx0 || by1 <= by0 {
        return 0;
    }
    let mut boxes: Vec<Rect> = Vec::with_capacity(parts.len());
    for part in parts {
        let min_x = (part.rect.min.x_nm - inflate).max(bx0);
        let min_y = (part.rect.min.y_nm - inflate).max(by0);
        let max_x = (part.rect.max.x_nm + inflate).min(bx1);
        let max_y = (part.rect.max.y_nm + inflate).min(by1);
        if min_x < max_x && min_y < max_y {
            boxes.push(Rect::new(
                Point::new(min_x, min_y),
                Point::new(max_x, max_y),
            ));
        }
    }
    let mut xs: Vec<i64> = Vec::with_capacity(boxes.len() * 2 + 2);
    xs.push(bx0);
    xs.push(bx1);
    for rect in &boxes {
        xs.push(rect.min.x_nm);
        xs.push(rect.max.x_nm);
    }
    xs.sort_unstable();
    xs.dedup();

    let mut best = 0_i64;
    for stripe in xs.windows(2) {
        let left = stripe[0];
        let right = stripe[1];
        if right <= left {
            continue;
        }
        let mut spans: Vec<(i64, i64)> = boxes
            .iter()
            .filter(|rect| rect.min.x_nm < right && rect.max.x_nm > left)
            .map(|rect| (rect.min.y_nm, rect.max.y_nm))
            .collect();
        spans.sort_unstable();
        let mut y = by0;
        for (start, end) in spans {
            if start > y {
                best = best.max((right - left) * (start - y));
            }
            y = y.max(end);
        }
        if by1 > y {
            best = best.max((right - left) * (by1 - y));
        }
    }
    best
}

/// Placed courtyard of one component, using the same fallback and geometry the
/// other placement reports use.
fn courtyard_of(component: &Component, placed: &crate::ComponentPlacement) -> Rect {
    let (width, height) = component.part.as_ref().map_or_else(
        || fallback_courtyard(&component.kind),
        |part| synth_layout::pcb_courtyard_geometry_for_part(part).1,
    );
    // `placed.center` is already the courtyard-bbox centre (see
    // `ComponentPlacement`), and `PLACEMENT_COURTYARD_OFFSET_MM` keeps the
    // exported footprint origin there too. Adding the footprint's courtyard
    // offset here would move the rect one origin away from the exported board.
    courtyard_rect_for_placement(placed, width, height, None, 0)
}

fn is_usb(component: &Component) -> bool {
    let lower = component.refdes.to_ascii_lowercase();
    if lower.contains("usb") {
        return true;
    }
    component.part.as_ref().is_some_and(|part| {
        part.kicad_footprint
            .as_deref()
            .is_some_and(|footprint| footprint.to_ascii_lowercase().contains("usb"))
    })
}

fn is_header(component: &Component) -> bool {
    component.kind == "connector"
        && component.part.as_ref().is_some_and(|part| {
            part.kicad_footprint
                .as_deref()
                .is_some_and(|footprint| footprint.to_ascii_lowercase().contains("header"))
                || part.id.0.to_ascii_lowercase().contains("header")
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn edge_of_rect(rect: Rect, outline: Rect) -> (i64, Edge) {
    let mut best = (rect.min.y_nm - outline.min.y_nm, Edge::Top);
    let candidates = [
        (outline.max.x_nm - rect.max.x_nm, Edge::Right),
        (outline.max.y_nm - rect.max.y_nm, Edge::Bottom),
        (rect.min.x_nm - outline.min.x_nm, Edge::Left),
    ];
    for (gap, edge) in candidates {
        if gap < best.0 {
            best = (gap, edge);
        }
    }
    best
}

fn spread(points: &[Point], axis: Axis) -> f64 {
    let values: Vec<i64> = points
        .iter()
        .map(|point| match axis {
            Axis::X => point.x_nm,
            Axis::Y => point.y_nm,
        })
        .collect();
    let min = values.iter().copied().min().unwrap_or(0);
    let max = values.iter().copied().max().unwrap_or(0);
    nm_to_mm(max - min)
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let root_a = find(parent, a);
    let root_b = find(parent, b);
    if root_a != root_b {
        parent[root_b] = root_a;
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    Rect::new(
        Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
        Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
    )
}

fn rect_centre(rect: Rect) -> Point {
    Point::new(
        rect.min.x_nm / 2 + rect.max.x_nm / 2,
        rect.min.y_nm / 2 + rect.max.y_nm / 2,
    )
}

fn gap_nm(a: Rect, b: Rect) -> i64 {
    (b.min.x_nm - a.max.x_nm)
        .max(a.min.x_nm - b.max.x_nm)
        .max(b.min.y_nm - a.max.y_nm)
        .max(a.min.y_nm - b.max.y_nm)
}

fn envelope(parts: &[Part<'_>]) -> Option<Rect> {
    let mut union: Option<Rect> = None;
    for part in parts {
        union = Some(union.map_or(part.rect, |rect| union_rect(rect, part.rect)));
    }
    union
}

fn envelope_area_nm2(rect: Rect) -> i64 {
    rect.width_nm().saturating_mul(rect.height_nm())
}

fn outline_area_nm2(outline: Rect) -> i64 {
    outline.width_nm().saturating_mul(outline.height_nm())
}

fn clamp_unit(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Dead-space raster result: the connected pockets plus the whole dead area.
#[derive(Debug)]
struct DeadSummary {
    pockets: Vec<DeadPocket>,
    total_mm2: f64,
}

/// Raster `outline` at [`DEAD_CELL_MM`]. A cell whose centre is more than
/// `DEAD_CLEAR_MM` from every courtyard and from the outline edge is dead;
/// 4-connected dead cells form one pocket.
fn dead_pockets_of(outline: Rect, courts: &[(Rect, Option<&str>)]) -> DeadSummary {
    let cell = mm_to_nm(DEAD_CELL_MM);
    let clear = i128::from(mm_to_nm(DEAD_CLEAR_MM));
    let cols = outline.width_nm() / cell;
    let rows = outline.height_nm() / cell;
    if cols <= 0 || rows <= 0 {
        return DeadSummary {
            pockets: Vec::new(),
            total_mm2: 0.0,
        };
    }
    let cols = usize::try_from(cols).unwrap_or(0);
    let rows = usize::try_from(rows).unwrap_or(0);
    let clear_sq = clear * clear;
    let mut dead = vec![false; cols * rows];
    for row in 0..rows {
        let y = outline.min.y_nm + row as i64 * cell + cell / 2;
        for col in 0..cols {
            let x = outline.min.x_nm + col as i64 * cell + cell / 2;
            let edge = (x - outline.min.x_nm)
                .min(outline.max.x_nm - x)
                .min(y - outline.min.y_nm)
                .min(outline.max.y_nm - y);
            if i128::from(edge) <= clear {
                continue;
            }
            let far = courts
                .iter()
                .all(|(rect, _)| rect_distance_sq(x, y, *rect) > clear_sq);
            dead[row * cols + col] = far;
        }
    }

    let area_cell = DEAD_CELL_MM * DEAD_CELL_MM;
    let mut total_cells = 0_usize;
    let mut pockets = Vec::new();
    for start in 0..dead.len() {
        if !dead[start] {
            continue;
        }
        let mut stack = vec![start];
        dead[start] = false;
        let (mut min_col, mut max_col, mut min_row, mut max_row) =
            (start % cols, start % cols, start / cols, start / cols);
        let mut cells = 0_usize;
        while let Some(index) = stack.pop() {
            cells += 1;
            let (row, col) = (index / cols, index % cols);
            min_col = min_col.min(col);
            max_col = max_col.max(col);
            min_row = min_row.min(row);
            max_row = max_row.max(row);
            for (dr, dc) in [(-1_isize, 0_isize), (1, 0), (0, -1), (0, 1)] {
                let Some(next_row) = row.checked_add_signed(dr) else {
                    continue;
                };
                let Some(next_col) = col.checked_add_signed(dc) else {
                    continue;
                };
                if next_row >= rows || next_col >= cols {
                    continue;
                }
                let next = next_row * cols + next_col;
                if dead[next] {
                    dead[next] = false;
                    stack.push(next);
                }
            }
        }
        total_cells += cells;
        let min_x = outline.min.x_nm + min_col as i64 * cell;
        let min_y = outline.min.y_nm + min_row as i64 * cell;
        let max_x = outline.min.x_nm + (max_col as i64 + 1) * cell;
        let max_y = outline.min.y_nm + (max_row as i64 + 1) * cell;
        pockets.push(DeadPocket {
            area_mm2: cells as f64 * area_cell,
            min_x_mm: nm_to_mm(min_x),
            min_y_mm: nm_to_mm(min_y),
            max_x_mm: nm_to_mm(max_x),
            max_y_mm: nm_to_mm(max_y),
            nearest_groups: nearest_groups(min_x, min_y, max_x, max_y, courts),
        });
    }
    let total_mm2 = total_cells as f64 * area_cell;
    DeadSummary { pockets, total_mm2 }
}

/// Squared distance from a point to a rectangle, in square nanometres.
fn rect_distance_sq(x: i64, y: i64, rect: Rect) -> i128 {
    let dx = i128::from((rect.min.x_nm - x).max(0).max(x - rect.max.x_nm));
    let dy = i128::from((rect.min.y_nm - y).max(0).max(y - rect.max.y_nm));
    dx * dx + dy * dy
}

/// The two groups whose courtyards are nearest to `pocket`, nearest first.
fn nearest_groups(
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
    courts: &[(Rect, Option<&str>)],
) -> Vec<String> {
    let pocket = Rect::new(Point::new(min_x, min_y), Point::new(max_x, max_y));
    let mut groups: std::collections::BTreeMap<&str, i128> = std::collections::BTreeMap::new();
    for (rect, group) in courts {
        let Some(group) = group else {
            continue;
        };
        let dx = i128::from(
            (rect.min.x_nm - pocket.max.x_nm)
                .max(pocket.min.x_nm - rect.max.x_nm)
                .max(0),
        );
        let dy = i128::from(
            (rect.min.y_nm - pocket.max.y_nm)
                .max(pocket.min.y_nm - rect.max.y_nm)
                .max(0),
        );
        let gap = dx * dx + dy * dy;
        groups
            .entry(group)
            .and_modify(|near| *near = (*near).min(gap))
            .or_insert(gap);
    }
    let mut ranked: Vec<(&str, i128)> = groups.into_iter().collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(2)
        .map(|(name, _)| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm_rect(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Rect {
        Rect::new(
            Point::new(mm_to_nm(min_x), mm_to_nm(min_y)),
            Point::new(mm_to_nm(max_x), mm_to_nm(max_y)),
        )
    }

    #[test]
    fn touching_courtyards_are_a_collision() {
        let a = Rect::new(Point::new(0, 0), Point::new(10, 10));
        let touching = Rect::new(Point::new(10, 0), Point::new(20, 10));
        let clear = Rect::new(Point::new(11, 0), Point::new(20, 10));

        assert!(courtyards_overlap(a, touching));
        assert!(!courtyards_overlap(a, clear));
    }

    fn hole_board(shift_mm: f64) -> (Rect, Vec<(Rect, Option<&'static str>)>) {
        let d = shift_mm;
        let outline = mm_rect(0.0, 0.0, 40.0, 40.0);
        // Four blocks enclose a 10 x 10 hole; the 2.5 mm clear band leaves a
        // 5 x 5 dead pocket in its middle.
        let courts = vec![
            (mm_rect(0.0 + d, 0.0 + d, 15.0 + d, 40.0 + d), Some("A")),
            (mm_rect(25.0 + d, 0.0 + d, 40.0 + d, 40.0 + d), Some("B")),
            (mm_rect(15.0 + d, 0.0 + d, 25.0 + d, 15.0 + d), Some("C")),
            (mm_rect(15.0 + d, 25.0 + d, 25.0 + d, 40.0 + d), Some("D")),
        ];
        (outline, courts)
    }

    #[test]
    fn a_five_by_five_hole_is_one_pocket_of_25_mm2() {
        let (outline, courts) = hole_board(0.0);
        let summary = dead_pockets_of(outline, &courts);

        assert_eq!(summary.pockets.len(), 1, "{summary:?}");
        let pocket = &summary.pockets[0];
        assert!(
            (pocket.area_mm2 - 25.0).abs() <= 2.0,
            "pocket area {}",
            pocket.area_mm2
        );
        assert_eq!(pocket.nearest_groups.len(), 2, "{pocket:?}");
    }

    #[test]
    fn parts_every_four_mm_leave_no_pocket() {
        let outline = mm_rect(0.0, 0.0, 20.0, 20.0);
        let mut courts = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                let cx = 2.0 + 4.0 * f64::from(x);
                let cy = 2.0 + 4.0 * f64::from(y);
                courts.push((mm_rect(cx - 1.0, cy - 0.6, cx + 1.0, cy + 0.6), Some("G")));
            }
        }
        let summary = dead_pockets_of(outline, &courts);

        assert!(summary.pockets.is_empty(), "{summary:?}");
    }

    #[test]
    fn a_thousandth_of_a_mm_does_not_change_the_pocket_count() {
        let (outline, courts) = hole_board(0.0);
        let (shifted_outline, shifted) = hole_board(0.001);
        assert_eq!(
            dead_pockets_of(outline, &courts).pockets.len(),
            dead_pockets_of(shifted_outline, &shifted).pockets.len()
        );
    }
}
