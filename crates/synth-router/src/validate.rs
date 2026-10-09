// SPDX-License-Identifier: Apache-2.0

//! Independent validation of a routed board.
//!
//! A router's exit code is a claim. This module is the check on that
//! claim, and it is deliberately built so the router cannot satisfy it by
//! reporting well:
//!
//! - **Topology** is compared against the baseline the router was given,
//!   read from the file. A router that dropped a footprint, renamed a net,
//!   or lost the board outline fails even if it exited `0`.
//! - **Connectivity** is re-derived from copper geometry with union-find
//!   over pads, tracks, and vias. The router's own "connected nets" count
//!   is recorded for provenance but never used as the verdict.
//! - **DRC** is `kicad-cli pcb drc` against the resolved profile, with
//!   zones refilled. A DRC that could not run is reported as unavailable,
//!   which is *not* a pass.
//!
//! The verdict is fail-closed. Anything blocking, or any required check
//! that could not be performed, means the candidate is not
//! fabrication-ready, and no amount of router-reported success changes
//! that.

// The distance helpers below convert i64 nanometres to f64 before
// squaring. Board geometry spans at most ~1e9 nm, roughly six orders of
// magnitude under f64's 52-bit mantissa, so the conversion is exact.
#![allow(clippy::cast_precision_loss)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::contract::{FabricationPolicy, RouteRequest};
use crate::pcb_read::{Pad, PcbBoard, Point};
/// Distance tolerance, in nanometres, for copper that is *meant* to touch.
///
/// Routers round to four decimal places when they write coordinates, so
/// two shapes that meet exactly on paper routinely miss by a few hundred
/// nanometres. This tolerance absorbs that rounding and nothing more —
/// a real gap is measured in millimetres.
const TOUCH_TOLERANCE_NM: i64 = 2_000;

/// What the candidate board lost or gained relative to its baseline.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopologyReport {
    /// Footprints present in the baseline but absent from the candidate.
    pub missing_footprints: Vec<String>,
    /// Footprints present in the candidate but absent from the baseline.
    pub extra_footprints: Vec<String>,
    /// Nets declared in the baseline but absent from the candidate.
    pub missing_nets: Vec<String>,
    /// Copper layers declared in the baseline but absent from the
    /// candidate. Losing a layer silently changes the stackup.
    pub missing_layers: Vec<String>,
    /// Pads present in the baseline but absent from the candidate, as
    /// `REF.PAD` identifiers.
    pub missing_pads: Vec<String>,
    /// Pads whose net assignment changed.
    pub renet_pads: Vec<String>,
    /// `true` when the board outline survived.
    pub outline_present: bool,
}

impl TopologyReport {
    /// Every topological defect, as human-readable reasons.
    #[must_use]
    pub fn blocking_reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        for footprint in &self.missing_footprints {
            reasons.push(format!(
                "footprint {footprint} is missing from the routed board"
            ));
        }
        for footprint in &self.extra_footprints {
            reasons.push(format!(
                "footprint {footprint} appears on the routed board but not in the baseline"
            ));
        }
        for net in &self.missing_nets {
            reasons.push(format!("net {net} is missing from the routed board"));
        }
        for layer in &self.missing_layers {
            reasons.push(format!(
                "copper layer {layer} is missing from the routed board"
            ));
        }
        for pad in &self.missing_pads {
            reasons.push(format!("pad {pad} is missing from the routed board"));
        }
        for pad in &self.renet_pads {
            reasons.push(format!(
                "pad {pad} changed net between baseline and routed board"
            ));
        }
        if !self.outline_present {
            reasons.push("the routed board has no board outline".to_string());
        }
        reasons
    }

    /// Whether the candidate is topologically faithful to its baseline.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.blocking_reasons().is_empty()
    }
}

/// One pad that is not electrically joined to the rest of its net.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenConnection {
    /// Net name.
    pub net: String,
    /// The pad on the smaller side of the split.
    pub pad: String,
    /// How many pads share this pad's copper island.
    pub island_size: usize,
    /// Total pads the net requires.
    pub pad_count: usize,
}

/// Connectivity re-derived from copper.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConnectivityReport {
    /// Nets whose pads all share one copper island.
    pub connected_nets: usize,
    /// Nets split across more than one copper island.
    pub open_nets: usize,
    /// Per-net detail for every open net.
    pub open: Vec<OpenConnection>,
    /// Pads with no net at all.
    pub unconnected_pads: Vec<String>,
    /// Vias landing inside a pad of their own net, which the fabrication
    /// policy may forbid.
    pub via_in_pad: Vec<String>,
    /// Vias whose drill is thinner than the policy allows.
    pub undersized_vias: Vec<String>,
    /// Tracks thinner than the policy allows.
    pub undersized_tracks: Vec<String>,
}

impl ConnectivityReport {
    /// Whether every net is fully joined and nothing violates policy.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.open_nets == 0
            && self.unconnected_pads.is_empty()
            && self.via_in_pad.is_empty()
            && self.undersized_vias.is_empty()
            && self.undersized_tracks.is_empty()
    }

    /// Every connectivity finding, as reasons.
    #[must_use]
    pub fn blocking_reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        for open in &self.open {
            reasons.push(format!(
                "net {} is not fully connected: pad {} reaches only {} of {} pads",
                open.net, open.pad, open.island_size, open.pad_count
            ));
        }
        for pad in &self.unconnected_pads {
            reasons.push(format!("pad {pad} carries no net"));
        }
        for via in &self.via_in_pad {
            reasons.push(format!(
                "via {via} is inside a pad and via-in-pad is not approved"
            ));
        }
        for via in &self.undersized_vias {
            reasons.push(format!("via {via} is below the fabrication minimum drill"));
        }
        for track in &self.undersized_tracks {
            reasons.push(format!(
                "track {track} is below the fabrication minimum width"
            ));
        }
        reasons
    }
}

/// The verdict on a candidate board.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FabricationVerdict {
    /// True only when every required check ran and found nothing
    /// blocking. Fail-closed: an unavailable check yields `false`.
    pub fabrication_ready: bool,
    /// Reasons the candidate is not fabrication-ready.
    pub blocking_reasons: Vec<String>,
    /// KiCad DRC counts for the board the verdict was decided on.
    ///
    /// `None` means DRC could not be performed, which is not a clean result:
    /// that case is also recorded in `unavailable_checks`, so a `routed`
    /// verdict always has counts here.
    pub kicad_drc: Option<crate::result::DrcCounts>,

    /// Checks that could not be performed. Their absence is itself a
    /// reason not to call the board ready.
    #[serde(default)]
    pub unavailable_checks: Vec<String>,
    pub topology: TopologyReport,
    pub connectivity: ConnectivityReport,
}

impl FabricationVerdict {
    /// A verdict that is clean and ready. Used by tests and by callers
    /// that have already validated their input.
    #[must_use]
    pub fn ready() -> Self {
        Self {
            fabrication_ready: true,
            ..Self::default()
        }
    }

    /// A neutral verdict, for constructing test fixtures.
    #[must_use]
    pub fn synthetic() -> Self {
        Self::default()
    }

    /// How many blocking findings stand in the way of fabrication.
    #[must_use]
    pub fn blocking_count(&self) -> usize {
        self.blocking_reasons.len() + self.unavailable_checks.len()
    }

    /// Whether anything blocking was found.
    ///
    /// Distinct from [`FabricationVerdict::fabrication_ready`]: this asks
    /// "did a check find a problem", which is false when a check could
    /// not run at all. That difference is what makes a board with
    /// copper-but-no-verification land in *review* rather than *failed*.
    #[must_use]
    pub fn has_blocking_findings(&self) -> bool {
        !self.blocking_reasons.is_empty()
    }
}

/// Read a board, tolerating absence.
///
/// A missing file is a required check that could not be performed, which
/// is not a pass. Returning the reason rather than a default verdict is
/// what keeps that distinction visible.
fn read_board(path: &std::path::Path) -> Result<PcbBoard, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    PcbBoard::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Validate the candidate at `request.candidate_path()` against the
/// baseline it was routed from.
///
/// Both boards are read from disk. Nothing here consults the router's
/// summary, which is the point: the router is the thing being checked.
#[must_use]
pub fn validate_candidate(
    request: &RouteRequest,
    candidate: &crate::freerouting::Candidate,
) -> FabricationVerdict {
    let mut verdict = FabricationVerdict::default();

    let baseline = match read_board(&request.baseline_path()) {
        Ok(board) => board,
        Err(detail) => {
            verdict
                .unavailable_checks
                .push(format!("baseline topology: {detail}"));
            return verdict;
        }
    };
    let routed = match read_board(&candidate.candidate_path) {
        Ok(board) => board,
        Err(detail) => {
            verdict
                .unavailable_checks
                .push(format!("routed board topology: {detail}"));
            return verdict;
        }
    };

    verdict.topology = compare_topology(&baseline, &routed);
    verdict.connectivity = check_connectivity(&routed, &request.policy);
    // A pad with no net is only the router's fault if it *had* one in the
    // baseline. A design that leaves a pin unassigned has nothing for a
    // router to have dropped, and counting it would fail every export of such
    // a design for a condition KiCad DRC — the authority on unconnected pads,
    // and the check that does run here — accepts.
    verdict.connectivity.unconnected_pads.retain(|pad| {
        baseline
            .pads()
            .find(|p| p.id() == *pad)
            .and_then(|p| p.net)
            .is_some_and(|net| net != 0)
    });

    let mut blocking = Vec::new();
    blocking.extend(verdict.topology.blocking_reasons());
    blocking.extend(verdict.connectivity.blocking_reasons());
    verdict.blocking_reasons = blocking;

    // KiCad DRC is the last required check. A DRC that could not run
    // leaves the board in review, never in `Routed`.
    //
    // The counts are kept on the verdict so the run record reports the same
    // DRC the verdict was decided on. Re-deriving them later would be a
    // second run over the same board, which can disagree if the board
    // changed in between — and would let the summary say `kicad_drc: null`
    // beside a verdict of `routed`.
    let drc = crate::drc::run(&candidate.candidate_path, request);
    verdict.kicad_drc = drc.map(Into::into);
    match &drc {
        Some(counts) => {
            if counts.blocking_count() > 0 {
                verdict.blocking_reasons.push(format!(
                    "kicad-cli pcb drc reported {} blocking violation(s)",
                    counts.blocking_count()
                ));
            }
        }
        None => verdict.unavailable_checks.push(
            "kicad-cli pcb drc could not be run; an unperformed DRC is not a clean DRC".to_string(),
        ),
    }

    verdict.fabrication_ready = verdict.blocking_count() == 0;
    verdict
}

/// Compare a routed board against the baseline it was routed from.
///
/// The baseline is the contract: a router may add copper freely, but it
/// may not remove a footprint, a pad, a net, a layer, or the outline.
#[must_use]
pub fn compare_topology(baseline: &PcbBoard, routed: &PcbBoard) -> TopologyReport {
    let baseline_refs: BTreeSet<&String> =
        baseline.footprints.iter().map(|f| &f.reference).collect();
    let routed_refs: BTreeSet<&String> = routed.footprints.iter().map(|f| &f.reference).collect();

    let mut missing_footprints: Vec<String> = baseline_refs
        .difference(&routed_refs)
        .map(|r| (*r).clone())
        .collect();
    let mut extra_footprints: Vec<String> = routed_refs
        .difference(&baseline_refs)
        .map(|r| (*r).clone())
        .collect();
    missing_footprints.sort();
    extra_footprints.sort();

    let baseline_nets = baseline.net_names();
    let routed_nets = routed.net_names();
    let mut missing_nets: Vec<String> = baseline_nets.difference(&routed_nets).cloned().collect();
    missing_nets.sort();

    let baseline_layers: BTreeSet<String> = baseline.copper_layers().into_iter().collect();
    let routed_layers: BTreeSet<String> = routed.copper_layers().into_iter().collect();
    let mut missing_layers: Vec<String> = baseline_layers
        .difference(&routed_layers)
        .cloned()
        .collect();
    missing_layers.sort();

    // Pad-level comparison, keyed by `REF.PAD` so a pad that moved nets
    // under the same designator is caught rather than passing as
    // "still present".
    //
    // A pad's net is compared by *name*, not by numeric code. The code is
    // only meaningful within one file: an engine that writes a board without
    // a net table (KiCadRoutingTools does) numbers its nets by whatever order
    // the names fall in, so an unchanged net can carry a different code in
    // the candidate than in the baseline. Comparing codes would then report
    // every pad as renetted.
    let baseline_nets = baseline.nets.clone();
    let routed_nets = routed.nets.clone();
    let name_of = |table: &BTreeMap<u32, String>, code: Option<u32>| {
        code.and_then(|c| table.get(&c).cloned())
    };

    let baseline_pads: BTreeMap<String, Option<String>> = baseline
        .pads()
        .filter(|p| p.net.is_some_and(|n| n != 0))
        .map(|p| (p.id(), name_of(&baseline_nets, p.net)))
        .collect();
    let routed_pads: BTreeMap<String, Option<String>> = routed
        .pads()
        .filter(|p| p.net.is_some_and(|n| n != 0))
        .map(|p| (p.id(), name_of(&routed_nets, p.net)))
        .collect();

    let mut missing_pads: Vec<String> = baseline_pads
        .keys()
        .filter(|id| !routed_pads.contains_key(*id))
        .cloned()
        .collect();
    missing_pads.sort();

    let mut renet_pads: Vec<String> = baseline_pads
        .iter()
        .filter_map(|(id, net)| {
            let routed_net = routed_pads.get(id)?;
            (routed_net != net).then(|| id.clone())
        })
        .collect();
    renet_pads.sort();

    TopologyReport {
        missing_footprints,
        extra_footprints,
        missing_nets,
        missing_layers,
        missing_pads,
        renet_pads,
        outline_present: !routed.edge_cuts.is_empty(),
    }
}

/// A copper item participating in the connectivity graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Pad(String),
    Segment(usize),
    Via(usize),
}

/// Re-derive connectivity from copper geometry.
///
/// Union-find over pads, tracks, and vias: two items join when their
/// copper shapes touch on a shared layer. A net is connected when all its
/// pads land in one island. Nothing here reads a router-reported count.
///
/// Filled zone copper counts. A ground net is normally routed as stitching
/// vias between plane pours rather than as a trace to every pad, so ignoring
/// zones would report every plane-backed net as open however correctly it
/// was routed.
#[must_use]
pub fn check_connectivity(board: &PcbBoard, policy: &FabricationPolicy) -> ConnectivityReport {
    let mut report = ConnectivityReport::default();

    // Group items by net. Net 0 is KiCad's unconnected net, so its pads are
    // recorded separately rather than as a connected island. Whether they are
    // a *defect* is decided by the caller, which can compare against the
    // baseline: an unassigned pin in the design is not a routing failure.
    let mut by_net: BTreeMap<u32, Vec<Item>> = BTreeMap::new();
    for pad in board.pads() {
        match pad.net {
            Some(0) | None => report.unconnected_pads.push(pad.id()),
            Some(net) => by_net.entry(net).or_default().push(Item::Pad(pad.id())),
        }
    }
    for (idx, _) in board.segments.iter().enumerate() {
        if let Some(net) = board.segments[idx].net.filter(|n| *n != 0) {
            by_net.entry(net).or_default().push(Item::Segment(idx));
        }
    }
    for (idx, _) in board.vias.iter().enumerate() {
        if let Some(net) = board.vias[idx].net.filter(|n| *n != 0) {
            by_net.entry(net).or_default().push(Item::Via(idx));
        }
    }

    report.unconnected_pads.sort();

    let pad_by_id: BTreeMap<String, &Pad> = board.pads().map(|p| (p.id(), p)).collect();

    for (net, items) in by_net {
        let net_name = board
            .nets
            .get(&net)
            .cloned()
            .unwrap_or_else(|| format!("net_{net}"));
        let mut islands = UnionFind::new(items.len());
        let mut pad_indices: Vec<usize> = Vec::new();

        for (idx, item) in items.iter().enumerate() {
            if matches!(item, Item::Pad(_)) {
                pad_indices.push(idx);
            }
        }

        union_touching_items(board, &items, &mut islands, &pad_by_id);

        // Anything this net's own pour touches is one electrical island.
        let pour_items = zone_copper_items(board, net, &items);
        if let Some((&anchor, rest)) = pour_items.split_first() {
            for &other in rest {
                islands.union(anchor, other);
            }
        }

        let pad_count = pad_indices.len();
        // A single-pad net has nothing to connect. Counting it as
        // "connected" would inflate the figure with nets that were never
        // a routing problem, which is exactly the number a reader uses to
        // judge whether a route succeeded.
        if pad_count < 2 {
            continue;
        }

        let mut island_sizes: BTreeMap<usize, usize> = BTreeMap::new();
        for &idx in &pad_indices {
            let root = islands.find(idx);
            *island_sizes.entry(root).or_insert(0) += 1;
        }

        if island_sizes.len() == 1 {
            report.connected_nets += 1;
        } else {
            report.open_nets += 1;
            report.open.push(stranded_connection(
                &items,
                &mut islands,
                &island_sizes,
                &pad_indices,
                net_name,
                pad_count,
            ));
        }
    }

    report.open.sort_by(|a, b| a.net.cmp(&b.net));
    report.via_in_pad = find_via_in_pad(board);
    report.undersized_vias = board
        .vias
        .iter()
        .filter(|via| via.drill_nm > 0 && via.drill_nm < policy.min_drill_diameter_nm)
        .map(|via| describe_via(board, via))
        .collect();
    report.undersized_tracks = board
        .segments
        .iter()
        .filter(|s| s.width_nm > 0 && s.width_nm < policy.min_track_width_nm)
        .map(|s| {
            format!(
                "net {} {:.3} mm on {}",
                board
                    .nets
                    .get(&s.net.unwrap_or(0))
                    .cloned()
                    .unwrap_or_default(),
                s.width_nm as f64 / crate::pcb_read::NM_PER_MM,
                s.layer
            )
        })
        .collect();
    report
}

fn describe_via(board: &PcbBoard, via: &crate::pcb_read::Via) -> String {
    format!(
        "net {} at ({:.3}, {:.3}) mm",
        board
            .nets
            .get(&via.net.unwrap_or(0))
            .cloned()
            .unwrap_or_default(),
        via.at.x_mm(),
        via.at.y_mm()
    )
}

/// Whether two copper items touch on a shared layer.
/// The smallest island of a split net, as an open connection.
///
/// The smallest island is the net's real symptom, and naming one of its pads
/// is what makes the report actionable — "net GND is open" does not say where.
fn stranded_connection(
    items: &[Item],
    islands: &mut UnionFind,
    island_sizes: &BTreeMap<usize, usize>,
    pad_indices: &[usize],
    net_name: String,
    pad_count: usize,
) -> OpenConnection {
    let (root, size) = island_sizes
        .iter()
        .min_by_key(|(root, size)| (**size, **root))
        .map_or((0, 0), |(root, size)| (*root, *size));
    let stranded = pad_indices
        .iter()
        .find(|idx| islands.find(**idx) == root)
        .and_then(|idx| match &items[*idx] {
            Item::Pad(id) => Some(id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| format!("net {net_name}"));
    OpenConnection {
        net: net_name,
        pad: stranded,
        island_size: size,
        pad_count,
    }
}

/// Union every pair of items whose copper touches on a shared layer.
///
/// Quadratic per net, which is fine at the scale a net reaches in practice
/// and keeps the geometry check readable — an off-by-one in a spatial index
/// would misreport opens, which is the failure this pass exists to prevent.
fn union_touching_items(
    board: &PcbBoard,
    items: &[Item],
    islands: &mut UnionFind,
    pad_by_id: &BTreeMap<String, &Pad>,
) {
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            if items_touch(board, &items[i], &items[j], pad_by_id) {
                islands.union(i, j);
            }
        }
    }
}

/// Items of `items` that this net's own filled zone copper reaches.
///
/// Only zones carrying the same net count: a pour of a different net is a
/// different conductor, and treating it as one would short nets together.
/// Only zones set to fill are considered, and only on the layers each item
/// actually occupies, so a pour on `In1.Cu` does not join a pad on `B.Cu`.
fn zone_copper_items(board: &PcbBoard, net: u32, items: &[Item]) -> Vec<usize> {
    let zones: Vec<_> = board
        .zones
        .iter()
        .filter(|zone| zone.filled && zone.net == Some(net))
        .flat_map(|zone| zone.outlines.iter().map(move |outline| (zone, outline)))
        .collect();
    if zones.is_empty() {
        return Vec::new();
    }

    let pads: BTreeMap<String, &Pad> = board.pads().map(|p| (p.id(), p)).collect();
    let mut reached = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let touched = match item {
            Item::Pad(id) => pads.get(id).is_some_and(|pad| {
                pad.layers
                    .iter()
                    .any(|layer| entry_is_copper(layer) && covered_by_zone(&zones, layer, &pad.at))
            }),
            Item::Segment(idx) => {
                let segment = &board.segments[*idx];
                covered_by_zone(&zones, &segment.layer, &segment.start)
                    || covered_by_zone(&zones, &segment.layer, &segment.end)
            }
            Item::Via(idx) => {
                let via = &board.vias[*idx];
                via.layers
                    .iter()
                    .any(|layer| covered_by_zone(&zones, layer, &via.at))
            }
        };
        if touched {
            reached.push(index);
        }
    }
    reached
}

/// Whether a pour of this net covers `(at, layer)`.
fn covered_by_zone(
    zones: &[(&crate::pcb_read::Zone, &(String, Vec<Point>))],
    layer: &str,
    at: &Point,
) -> bool {
    zones.iter().any(|(_, (zone_layer, polygon))| {
        layer_entries_match(zone_layer, layer) && point_in_polygon(at, polygon)
    })
}

/// Ray-casting point-in-polygon test.
///
/// A point exactly on an edge counts as inside: the pour's own outline is
/// where its copper ends, and a via drilled on the outline is stitched to it.
fn point_in_polygon(point: &Point, polygon: &[Point]) -> bool {
    let mut inside = false;
    let count = polygon.len();
    for i in 0..count {
        let a = &polygon[i];
        let b = &polygon[(i + 1) % count];
        if point_on_segment(point, a, b) {
            return true;
        }
        let (a_y, b_y) = (a.y_nm, b.y_nm);
        if (a_y > point.y_nm) != (b_y > point.y_nm) {
            // x of the edge at the point's y
            let t = (point.y_nm - a_y) as f64 / (b_y - a_y) as f64;
            let x = a.x_nm as f64 + t * (b.x_nm - a.x_nm) as f64;
            if x > point.x_nm as f64 {
                inside = !inside;
            }
        }
    }
    inside
}

fn point_on_segment(point: &Point, a: &Point, b: &Point) -> bool {
    let cross =
        (b.x_nm - a.x_nm) * (point.y_nm - a.y_nm) - (b.y_nm - a.y_nm) * (point.x_nm - a.x_nm);
    if cross.abs() > TOUCH_TOLERANCE_NM {
        return false;
    }
    let within_x = point.x_nm >= a.x_nm.min(b.x_nm) - TOUCH_TOLERANCE_NM
        && point.x_nm <= a.x_nm.max(b.x_nm) + TOUCH_TOLERANCE_NM;
    let within_y = point.y_nm >= a.y_nm.min(b.y_nm) - TOUCH_TOLERANCE_NM
        && point.y_nm <= a.y_nm.max(b.y_nm) + TOUCH_TOLERANCE_NM;
    within_x && within_y
}

fn items_touch(board: &PcbBoard, a: &Item, b: &Item, pads: &BTreeMap<String, &Pad>) -> bool {
    match (a, b) {
        (Item::Pad(x), Item::Pad(y)) => {
            matches!((pads.get(x), pads.get(y)), (Some(x), Some(y)) if pads_touch(x, y))
        }
        (Item::Pad(x), Item::Segment(s)) | (Item::Segment(s), Item::Pad(x)) => {
            let Some(pad) = pads.get(x) else {
                return false;
            };
            let segment = &board.segments[*s];
            // Matched by name with the wildcard rule, not by set membership:
            // a through-hole pad's `"*.Cu"` is in no `BTreeSet` alongside a
            // literal `B.Cu`.
            pad_layers(&pad.layers)
                .iter()
                .any(|l| layer_entries_match(l, &segment.layer))
                && point_segment_distance(pad.at, segment.start, segment.end)
                    <= pad.radius_nm() + segment.width_nm / 2 + TOUCH_TOLERANCE_NM
        }
        (Item::Pad(x), Item::Via(v)) | (Item::Via(v), Item::Pad(x)) => {
            let Some(pad) = pads.get(x) else {
                return false;
            };
            let via = &board.vias[*v];
            shares_layer(pad, &via.layers)
                && point_distance(pad.at, via.at)
                    <= pad.radius_nm() + via.size_nm / 2 + TOUCH_TOLERANCE_NM
        }
        (Item::Segment(x), Item::Segment(y)) => {
            let first = &board.segments[*x];
            let second = &board.segments[*y];
            first.layer == second.layer
                && segment_distance(first.start, first.end, second.start, second.end)
                    <= (first.width_nm + second.width_nm) / 2 + TOUCH_TOLERANCE_NM
        }
        (Item::Segment(s), Item::Via(v)) | (Item::Via(v), Item::Segment(s)) => {
            // A via connects every layer it spans, so a track on any of
            // them touches it.
            let segment = &board.segments[*s];
            let via = &board.vias[*v];
            via.layers.contains(&segment.layer)
                && point_segment_distance(via.at, segment.start, segment.end)
                    <= via.size_nm / 2 + segment.width_nm / 2 + TOUCH_TOLERANCE_NM
        }
        (Item::Via(x), Item::Via(y)) => {
            board.vias[*x]
                .layers
                .iter()
                .any(|l| board.vias[*y].layers.contains(l))
                && point_distance(board.vias[*x].at, board.vias[*y].at)
                    <= board.vias[*x].size_nm / 2 + board.vias[*y].size_nm / 2 + TOUCH_TOLERANCE_NM
        }
    }
}

/// Whether a `(layers ...)` entry names copper.
///
/// KiCad spells through-hole pads `"*.Cu"`, which is a wildcard for every
/// copper layer rather than a single layer name. Treating it as
/// non-copper — because it does not literally end in `.Cu` — silently put
/// every through-hole pad on no layer at all, so no track could ever touch
/// one and each THT net read as unroutable no matter what the router drew.
fn entry_is_copper(entry: &str) -> bool {
    entry.ends_with(".Cu") || entry.ends_with(".CuIn") || entry == "*.Cu"
}

/// Whether one layer entry refers to the same layer as another.
///
/// `*.Cu` matches any copper layer, so a through-hole pad and a track on
/// `B.Cu` do share a layer even though neither name contains the other's.
fn layer_entries_match(a: &str, b: &str) -> bool {
    a == b || (a == "*.Cu" && entry_is_copper(b)) || (b == "*.Cu" && entry_is_copper(a))
}

/// Copper layers a pad participates in.
fn pad_layers(layers: &[String]) -> BTreeSet<&String> {
    layers.iter().filter(|l| entry_is_copper(l)).collect()
}

/// Whether two named layer sets share a layer.
fn layers_intersect(a: &[String], b: &[String]) -> bool {
    a.iter()
        .any(|l| b.iter().any(|other| layer_entries_match(l, other)))
}

fn shares_layer(pad: &Pad, via_layers: &[String]) -> bool {
    pad_layers(&pad.layers)
        .iter()
        .any(|l| via_layers.iter().any(|v| layer_entries_match(l, v)))
}

fn pads_touch(a: &Pad, b: &Pad) -> bool {
    if !layers_intersect(&a.layers, &b.layers) {
        return false;
    }
    point_distance(a.at, b.at) <= a.radius_nm() + b.radius_nm() + TOUCH_TOLERANCE_NM
}

fn point_distance(a: Point, b: Point) -> i64 {
    let dx = a.x_nm - b.x_nm;
    let dy = a.y_nm - b.y_nm;
    let squared = (dx as f64) * (dx as f64) + (dy as f64) * (dy as f64);
    squared.sqrt().round() as i64
}

/// Distance from `p` to the segment `a`-`b`, in nanometres.
fn point_segment_distance(p: Point, a: Point, b: Point) -> i64 {
    let abx = (b.x_nm - a.x_nm) as f64;
    let aby = (b.y_nm - a.y_nm) as f64;
    let length_squared = abx * abx + aby * aby;
    if length_squared == 0.0 {
        return point_distance(p, a);
    }
    let t = (((p.x_nm - a.x_nm) as f64) * abx + ((p.y_nm - a.y_nm) as f64) * aby) / length_squared;
    let t = t.clamp(0.0, 1.0);
    let cx = a.x_nm as f64 + t * abx;
    let cy = a.y_nm as f64 + t * aby;
    point_distance(p, Point::new(cx.round() as i64, cy.round() as i64))
}

/// Minimum distance between two segments, in nanometres.
fn segment_distance(a0: Point, a1: Point, b0: Point, b1: Point) -> i64 {
    if segments_intersect(a0, a1, b0, b1) {
        return 0;
    }
    [
        point_segment_distance(a0, b0, b1),
        point_segment_distance(a1, b0, b1),
        point_segment_distance(b0, a0, a1),
        point_segment_distance(b1, a0, a1),
    ]
    .into_iter()
    .min()
    .unwrap_or(0)
}

/// Whether two segments cross or overlap.
fn segments_intersect(a0: Point, a1: Point, b0: Point, b1: Point) -> bool {
    let d1 = cross(a0, a1, b0);
    let d2 = cross(a0, a1, b1);
    let d3 = cross(b0, b1, a0);
    let d4 = cross(b0, b1, a1);

    let opposite = |x: i64, y: i64| (x <= 0 && y >= 0) || (x >= 0 && y <= 0);
    if opposite(d1, d2) && opposite(d3, d4) {
        return true;
    }
    // Collinear overlap: a zero cross product with the point on the span.
    (d1 == 0 && on_span(b0, a0, a1))
        || (d2 == 0 && on_span(b1, a0, a1))
        || (d3 == 0 && on_span(a0, b0, b1))
        || (d4 == 0 && on_span(a1, b0, b1))
}

fn cross(a: Point, b: Point, c: Point) -> i64 {
    let abx = (b.x_nm - a.x_nm) as f64;
    let aby = (b.y_nm - a.y_nm) as f64;
    let acx = (c.x_nm - a.x_nm) as f64;
    let acy = (c.y_nm - a.y_nm) as f64;
    (abx * acy - aby * acx).round() as i64
}

fn on_span(p: Point, a: Point, b: Point) -> bool {
    p.x_nm >= a.x_nm.min(b.x_nm)
        && p.x_nm <= a.x_nm.max(b.x_nm)
        && p.y_nm >= a.y_nm.min(b.y_nm)
        && p.y_nm <= a.y_nm.max(b.y_nm)
}

/// Find vias whose own net has a pad underneath them.
fn find_via_in_pad(board: &PcbBoard) -> Vec<String> {
    let mut hits = Vec::new();
    for via in &board.vias {
        for pad in board.pads() {
            if pad.net != via.net || via.net.is_none() {
                continue;
            }
            if !shares_layer(pad, &via.layers) {
                continue;
            }
            // Exactly: the connectivity radius treats a pad as a disc of its
            // long half-dimension, which on an ordinary SOIC pad reaches
            // 0.675 mm past the copper across the narrow axis. Judging
            // via-in-pad from it failed every Class-A board on vias the
            // router had placed a quarter of a millimetre clear of the pad.
            if pad.contains_point(via.at) {
                hits.push(format!("{via} {}", pad.id()));
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

impl std::fmt::Display for crate::pcb_read::Via {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "via at ({:.3}, {:.3}) mm",
            self.at.x_mm(),
            self.at.y_mm()
        )
    }
}

/// Disjoint-set forest with union by rank.
#[derive(Debug, Clone)]
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, mut node: usize) -> usize {
        while self.parent[node] != node {
            self.parent[node] = self.parent[self.parent[node]];
            node = self.parent[node];
        }
        node
    }

    fn union(&mut self, a: usize, b: usize) {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.rank[ra] < self.rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        if self.rank[ra] == self.rank[rb] {
            self.rank[ra] += 1;
        }
    }
}

/// Report unrouted nets the way the router claimed them, so a mismatch
/// between the router's own count and the re-derived one is visible.
///
/// Recorded, never used as the verdict: the whole point of the
/// independent pass is that the router's bookkeeping is not evidence.
#[must_use]
pub fn router_claim_disagreements(
    router_connected_nets: usize,
    report: &ConnectivityReport,
) -> Vec<String> {
    if router_connected_nets == report.connected_nets {
        return Vec::new();
    }
    vec![format!(
        "the router reported {router_connected_nets} connected net(s); re-deriving \
         connectivity from copper found {}",
        report.connected_nets
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcb_read::{mm_to_nm, parse_sexpr, PcbParseError, SAMPLE_BOARD};

    fn board(text: &str) -> PcbBoard {
        PcbBoard::parse(text).expect("fixture parses")
    }

    #[test]
    fn a_correctly_routed_net_is_connected_end_to_end_across_layers() {
        // The sample routes R1.2 through a via to U1.1 on the bottom layer.
        // If the via or the layer check were wrong this net would read as
        // open, which is the failure this pass must not have.
        let report = check_connectivity(&board(SAMPLE_BOARD), &FabricationPolicy::default());
        assert_eq!(report.open_nets, 0, "{:?}", report.open);
        assert_eq!(report.connected_nets, 1, "net 2 must be connected");
        assert!(report.open.is_empty());
    }

    #[test]
    fn a_net_split_by_a_missing_track_is_reported_open() {
        let routed = SAMPLE_BOARD.replace(
            "(segment (start 15 20) (end 15 18)",
            "(segment (start 40 20) (end 15 18)",
        );
        let report = check_connectivity(&board(&routed), &FabricationPolicy::default());
        assert_eq!(report.open_nets, 1, "{:?}", report.open);
        assert_eq!(report.connected_nets, 0);
        let open = &report.open[0];
        assert_eq!(open.net, "SDA");
        assert_eq!(open.pad_count, 2);
    }

    #[test]
    fn the_open_report_names_the_stranded_pad() {
        let routed = SAMPLE_BOARD.replace(
            "(segment (start 15 20) (end 15 18)",
            "(segment (start 40 20) (end 15 18)",
        );
        let report = check_connectivity(&board(&routed), &FabricationPolicy::default());
        let stranded = &report.open[0].pad;
        assert!(
            stranded == "R1.2" || stranded == "U1.1",
            "unexpected stranded pad {stranded}"
        );
        assert_eq!(report.open[0].island_size, 1);
    }

    #[test]
    fn a_pad_on_a_different_layer_does_not_join_the_net() {
        // Two pads on the same net, both on F.Cu, but the track between
        // them is on B.Cu and no via joins the layers: still open.
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "SIG")
          (footprint "a:1" (layer "F.Cu") (at 0 0)
            (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (footprint "a:1" (layer "F.Cu") (at 5 0)
            (property "Reference" "R2" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (segment (start 0 0) (end 5 0) (width 0.25) (layer "B.Cu") (net 1))
          (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let report = check_connectivity(&board(text), &FabricationPolicy::default());
        assert_eq!(
            report.open_nets, 1,
            "a track on another layer joins nothing"
        );
    }

    #[test]
    fn an_unconnected_pad_is_listed_rather_than_counted_as_a_net() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (footprint "a:1" (layer "F.Cu") (at 0 0)
            (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 0 "")))
          (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let report = check_connectivity(&board(text), &FabricationPolicy::default());
        assert_eq!(report.unconnected_pads, vec!["R1.1".to_string()]);
        assert_eq!(report.connected_nets, 0);
    }

    #[test]
    fn via_in_pad_is_reported_only_when_policy_forbids_it() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "GND")
          (footprint "a:1" (layer "F.Cu") (at 0 0)
            (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 2 2) (layers "F.Cu") (net 1 "GND"))
            (pad "2" smd rect (at 6 0) (size 1 1) (layers "F.Cu") (net 1 "GND")))
          (via (at 0 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (segment (start 0 0) (end 6 0) (width 0.25) (layer "F.Cu") (net 1))
          (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let strict = check_connectivity(&board(text), &FabricationPolicy::default());
        assert!(
            !strict.via_in_pad.is_empty(),
            "a via inside a pad must be seen"
        );
        assert!(!strict.is_clean());

        let permissive = check_connectivity(
            &board(text),
            &FabricationPolicy {
                allow_via_in_pad: true,
                ..FabricationPolicy::default()
            },
        );
        // The finding is still recorded; policy decides whether it blocks,
        // and that decision is made by the caller reading the report.
        assert!(!permissive.via_in_pad.is_empty());
    }

    /// The geometry that failed `a3_protected_power_entry`: U1 pad 2 of a
    /// `SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm` at (23.975, 29.5183),
    /// 1.95 × 0.6 mm, and the via KiCadRoutingTools placed at
    /// (24.0, 28.95) — 0.268 mm below the pad's edge, against the 0.1 mm
    /// same-net clearance Synth had asked it for. Centre-to-centre that is
    /// 0.569 mm, inside the 0.975 mm connectivity disc and well outside the
    /// copper.
    #[test]
    fn a_via_clear_of_an_elongated_pad_is_not_via_in_pad() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "net_4")
          (footprint "Package_SO:SOIC-8-1EP" (layer "F.Cu") (at 26.45 30.1533)
            (property "Reference" "U1" (at 0 0) (layer "F.SilkS"))
            (pad "2" smd roundrect (at -2.475 -0.635) (size 1.95 0.6)
              (layers "F.Cu") (net 1 "net_4")))
          (via (at 24.0 28.95) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let board = board(text);
        let pad = board.pads().next().expect("the pad parses");
        assert!(
            point_distance(pad.at, board.vias[0].at) <= pad.radius_nm(),
            "the via must sit inside the connectivity disc, or this test has \
             stopped covering the case it was written for"
        );
        let report = check_connectivity(&board, &FabricationPolicy::default());
        assert!(
            report.via_in_pad.is_empty(),
            "a via 0.268 mm clear of the pad's copper is not in the pad: {:?}",
            report.via_in_pad
        );
    }

    /// The same pad, with the via moved onto its copper.
    #[test]
    fn a_via_on_an_elongated_pad_is_still_via_in_pad() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "net_4")
          (footprint "Package_SO:SOIC-8-1EP" (layer "F.Cu") (at 26.45 30.1533)
            (property "Reference" "U1" (at 0 0) (layer "F.SilkS"))
            (pad "2" smd roundrect (at -2.475 -0.635) (size 1.95 0.6)
              (layers "F.Cu") (net 1 "net_4")))
          (via (at 24.5 29.5183) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let report = check_connectivity(&board(text), &FabricationPolicy::default());
        assert_eq!(
            report.via_in_pad,
            vec!["via at (24.500, 29.518) mm U1.2".to_string()]
        );
    }

    /// The geometry that exposed the pad-rotation sign error: a 1x2 JST,
    /// `JST_XH_S2B-XH-A-1_1x02_P2.50mm_Horizontal`, at (81.205, 37.93)
    /// rotated 90°, whose pad 2 offset is `(at 2.5 0)`. KiCad's y axis
    /// points down, so the pad's copper is at (81.205, 35.43) — which is
    /// where KiCadRoutingTools routed it. Reading the rotation as +90 put
    /// it at (81.205, 40.43), 5 mm away, and the net was reported open
    /// against a board that was correctly routed.
    ///
    /// Pad 1 is at offset (0, 0), so it lands in the same place either way:
    /// that is why the two pads behaved differently and why symmetric
    /// two-pad passives never caught this.
    #[test]
    fn a_rotated_footprint_places_its_pads_in_the_y_down_sense() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 19 "net_18")
          (footprint "Connector_JST:JST_XH_S2B" (layer "F.Cu") (at 81.205 37.93 90)
            (property "Reference" "J4" (at 0 0) (layer "F.SilkS"))
            (pad "2" thru_hole oval (at 2.5 0) (size 1.7 2) (drill 1)
              (layers "*.Cu") (net 19 "net_18")))
          (gr_line (start 0 0) (end 94 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let board = board(text);
        let pad = board.pads().next().expect("the pad parses");
        assert_eq!(
            (pad.at.x_mm(), pad.at.y_mm()),
            (81.205, 35.43),
            "a +2.5 mm x offset on a footprint rotated 90 lands 2.5 mm *up* \
             the board, where the router put its copper"
        );
    }

    /// A rotated footprint turns the pad's long axis, and the check has to
    /// turn with it: the point that is clear at 0° is on copper at 90°.
    #[test]
    fn pad_rotation_turns_the_containment_test() {
        let make = |rotation: &str| {
            format!(
                r#"(kicad_pcb
              (version 20260206)
              (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
              (net 0 "")
              (net 1 "N")
              (footprint "a:1" (layer "F.Cu") (at 10 10 {rotation})
                (property "Reference" "U1" (at 0 0) (layer "F.SilkS"))
                (pad "1" smd rect (at 0 0) (size 1.95 0.6)
                  (layers "F.Cu") (net 1 "N")))
              (via (at 10 10.6) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
              (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
            )"#
            )
        };
        // Unrotated the pad spans ±0.3 mm in y, so a via 0.6 mm away in y
        // is clear of it.
        let flat = check_connectivity(&board(&make("0")), &FabricationPolicy::default());
        assert!(flat.via_in_pad.is_empty(), "{:?}", flat.via_in_pad);
        // Rotated 90° the long axis runs in y and reaches 0.975 mm, so the
        // same via is on copper.
        let turned = check_connectivity(&board(&make("90")), &FabricationPolicy::default());
        assert!(!turned.via_in_pad.is_empty(), "rotation must be honoured");
    }

    #[test]
    fn undersized_copper_is_reported_against_the_policy_minimums() {
        let routed = SAMPLE_BOARD
            .replace("(width 0.25)", "(width 0.1)")
            .replace("(drill 0.4)", "(drill 0.15)");
        let report = check_connectivity(
            &board(&routed),
            &FabricationPolicy {
                min_track_width_nm: 127_000,
                min_drill_diameter_nm: 300_000,
                ..FabricationPolicy::default()
            },
        );
        assert!(!report.undersized_tracks.is_empty());
        assert!(!report.undersized_vias.is_empty());
        assert!(!report.is_clean());
    }

    #[test]
    fn topology_comparison_passes_on_an_unchanged_board() {
        let baseline = board(SAMPLE_BOARD);
        let report = compare_topology(&baseline, &baseline.clone());
        assert!(report.is_clean(), "{:?}", report.blocking_reasons());
    }

    #[test]
    fn a_dropped_footprint_is_a_topological_failure() {
        let baseline = board(SAMPLE_BOARD);
        let routed = board(&SAMPLE_BOARD.replace("\"U1\"", "\"U9\""));
        let report = compare_topology(&baseline, &routed);
        assert!(!report.is_clean());
        assert!(report.missing_footprints.contains(&"U1".to_string()));
        assert!(report.extra_footprints.contains(&"U9".to_string()));
    }

    #[test]
    fn a_dropped_net_or_layer_is_a_topological_failure() {
        let baseline = board(SAMPLE_BOARD);
        let routed = board(&SAMPLE_BOARD.replace("(net 1 \"GND\")\n  ", ""));
        assert!(!compare_topology(&baseline, &routed).is_clean());

        let routed = board(&SAMPLE_BOARD.replace("(31 \"B.Cu\" signal)", "(31 \"In1.Cu\" signal)"));
        let report = compare_topology(&baseline, &routed);
        assert!(report.missing_layers.contains(&"B.Cu".to_string()));
    }

    #[test]
    fn a_renetted_pad_is_reported_separately_from_a_missing_one() {
        // These are different defects: a missing pad is lost copper, a
        // renetted pad is copper that went somewhere it must not. Only
        // U1's pad moves nets, so R1 stays connected and the report names
        // exactly one pad.
        let baseline = board(SAMPLE_BOARD);
        let relocated = SAMPLE_BOARD.replace(
            r#"(pad "1" smd roundrect (at -1 0) (size 0.6 0.3)
      (layers "B.Cu" "B.Paste" "B.Mask") (net 2 "SDA"))"#,
            r#"(pad "1" smd roundrect (at -1 0) (size 0.6 0.3)
      (layers "B.Cu" "B.Paste" "B.Mask") (net 1 "GND"))"#,
        );
        assert_ne!(relocated, SAMPLE_BOARD, "the fixture edit must apply");
        let routed = board(&relocated);
        let report = compare_topology(&baseline, &routed);
        assert!(!report.renet_pads.is_empty(), "{report:?}");
        assert!(report
            .blocking_reasons()
            .iter()
            .any(|r| r.contains("changed net")));
    }

    #[test]
    fn a_missing_outline_is_a_topological_failure() {
        let baseline = board(SAMPLE_BOARD);
        let routed = board(&SAMPLE_BOARD.replace("Edge.Cuts", "F.SilkS"));
        let report = compare_topology(&baseline, &routed);
        assert!(!report.outline_present);
        assert!(report
            .blocking_reasons()
            .iter()
            .any(|r| r.contains("outline")));
    }

    #[test]
    fn a_verdict_with_no_findings_but_an_unperformed_check_is_not_ready() {
        // The fail-closed rule: "we could not check" is not "it passed".
        let verdict = FabricationVerdict {
            fabrication_ready: false,
            blocking_reasons: Vec::new(),
            unavailable_checks: vec!["kicad-cli pcb drc could not be run".to_string()],
            ..FabricationVerdict::default()
        };
        assert!(!verdict.fabrication_ready);
        assert!(!verdict.has_blocking_findings());
        assert_eq!(verdict.blocking_count(), 1);
        // And that maps to review, not failure: copper exists, nothing
        // blocking was found.
        assert_eq!(
            crate::result::RouteReport::state_for_verdict(&verdict),
            crate::result::RouteState::ReviewRequired
        );
    }

    /// A board where `R1.2` carries no net, so the design left that pin
    /// unassigned and the router has nothing to have dropped.
    const UNASSIGNED_PIN_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers
    (0 "F.Cu" signal)
    (44 "Edge.Cuts" user)
  )
  (net 0 "")
  (net 1 "GND")
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (at 10 20)
    (property "Reference" "R1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))
    (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 0 ""))
  )
  (footprint "Resistor_SMD:R_0603_1608Metric"
    (layer "F.Cu")
    (at 20 20)
    (property "Reference" "R2" (at 0 -1) (layer "F.SilkS"))
    (pad "1" smd roundrect (at -0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))
    (pad "2" smd roundrect (at 0.825 0) (size 0.8 0.95)
      (layers "F.Cu" "F.Paste" "F.Mask") (net 0 ""))
  )
  (segment (start 9.175 20) (end 19.175 20) (width 0.25) (layer "F.Cu") (net 1))
  (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 40 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 0) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 0) (end 40 30) (layer "Edge.Cuts") (width 0.1))
)"#;

    // A through-hole pad joined to a pad on the far side, the way KiCad
    // writes a THT net: the pads carry a wildcard copper layer rather than a
    // named one.
    const THROUGH_HOLE_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "synth-eda")
  (layers
    (0 "F.Cu" signal)
    (31 "B.Cu" signal)
    (44 "Edge.Cuts" user)
  )
  (net 0 "")
  (net 1 "SIG")
  (footprint "Connector:Pin_1x02"
    (layer "F.Cu")
    (at 10 20)
    (property "Reference" "J1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8)
      (layers "*.Cu" "*.Mask") (net 1 "SIG"))
    (pad "2" thru_hole circle (at 2.54 0) (size 1.6 1.6) (drill 0.8)
      (layers "*.Cu" "*.Mask") (net 0 ""))
  )
  (footprint "Package_QFN:QFN-32"
    (layer "B.Cu")
    (at 30 20)
    (property "Reference" "U1" (at 0 -1) (layer "B.SilkS"))
    (pad "1" smd roundrect (at -1 0) (size 0.6 0.3)
      (layers "B.Cu" "B.Paste" "B.Mask") (net 1 "SIG"))
  )
  (segment (start 10 20) (end 29 20) (width 0.25) (layer "B.Cu") (net 1))
  (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 0) (end 0 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 0) (end 40 30) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 30) (end 40 30) (layer "Edge.Cuts") (width 0.1))
)"#;

    /// A ground net completed by a plane: two pads joined only by a filled
    /// zone, with the zone naming its net the way a table-less board does.
    const PLANE_BOARD: &str = r#"(kicad_pcb
  (version 20260206)
  (generator "pcbnew")
  (layers
    (0 "F.Cu" signal)
    (44 "Edge.Cuts" user)
  )
  (footprint "Connector:Pin_1x02"
    (layer "F.Cu")
    (at 10 20)
    (property "Reference" "J1" (at 0 -1) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8)
      (layers "*.Cu" "*.Mask") (net "GND"))
    (pad "2" thru_hole circle (at 20 20) (size 1.6 1.6) (drill 0.8)
      (layers "*.Cu" "*.Mask") (net "GND"))
  )
  (zone
    (net "GND")
    (layer "F.Cu")
    (fill yes)
    (polygon (pts (xy 0 0) (xy 40 0) (xy 40 40) (xy 0 40)))
  )
  (gr_line (start 0 0) (end 40 0) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 0) (end 0 40) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 40 0) (end 40 40) (layer "Edge.Cuts") (width 0.1))
  (gr_line (start 0 40) (end 40 40) (layer "Edge.Cuts") (width 0.1))
)"#;

    #[test]
    fn a_filled_plane_connects_its_nets_pads() {
        // A ground net is normally routed as a plane, not as a trace to every
        // pad. Counting only tracks reported every plane-backed net as open
        // however correctly it was routed.
        let report = check_connectivity(&board(PLANE_BOARD), &FabricationPolicy::default());
        assert_eq!(report.connected_nets, 1, "{report:?}");
        assert!(report.open.is_empty(), "{report:?}");
    }

    #[test]
    fn a_zone_whose_net_is_name_only_still_connects_its_pads() {
        // KiCadRoutingTools writes a board with no net table, so the zone
        // binds its net as `(net "GND")` with no code. Reading only the
        // numeric form left the pour unassigned and the net read as open.
        let report = check_connectivity(&board(PLANE_BOARD), &FabricationPolicy::default());
        assert!(report.open.is_empty(), "{report:?}");
    }

    #[test]
    fn a_zone_of_another_net_does_not_join_pads() {
        // The pour is a conductor of its own net; treating it as common
        // copper would short two nets together.
        // Only the zone's net moves; the pads stay on GND.
        let text = PLANE_BOARD.replace("(zone\n    (net \"GND\")", "(zone\n    (net \"OTHER\")");
        let report = check_connectivity(&board(&text), &FabricationPolicy::default());
        assert_eq!(
            report.open_nets, 1,
            "the pour must not join them: {report:?}"
        );
    }

    #[test]
    fn an_unfilled_zone_is_not_copper() {
        // `(fill no)` leaves the outline as a drawing, not a conductor.
        let text = PLANE_BOARD.replace("(fill yes)", "(fill no)");
        let report = check_connectivity(&board(&text), &FabricationPolicy::default());
        assert_eq!(report.open_nets, 1, "{report:?}");
    }

    #[test]
    fn a_through_hole_pad_is_routable_from_a_track_on_any_copper_layer() {
        // KiCad writes a through-hole pad's layers as `"*.Cu"`, a wildcard
        // for every copper layer. Reading that as "not copper" put the pad on
        // no layer at all, so no track could touch it and every THT net
        // reported as unroutable however well it was routed.
        let board = board(THROUGH_HOLE_BOARD);
        let report = check_connectivity(&board, &FabricationPolicy::default());
        assert!(
            report.open.is_empty(),
            "a THT pad must join the track on B.Cu: {report:?}"
        );
        assert_eq!(report.connected_nets, 1);
    }

    #[test]
    fn a_through_hole_pad_joins_a_via_from_any_copper_layer() {
        let text = THROUGH_HOLE_BOARD.replace(
            r#"(segment (start 10 20) (end 29 20) (width 0.25) (layer "B.Cu") (net 1))"#,
            r#"(via (at 10 20) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))
  (segment (start 10 20) (end 29 20) (width 0.25) (layer "B.Cu") (net 1))"#,
        );
        let report = check_connectivity(&board(&text), &FabricationPolicy::default());
        assert!(report.open.is_empty(), "{report:?}");
    }

    #[test]
    fn a_pad_the_design_left_unassigned_is_not_a_routing_failure() {
        // Net 0 in both boards: the design never assigned the pin, so there is
        // nothing for the router to have dropped. KiCad DRC remains the
        // authority on whether an unassigned pin is acceptable.
        let baseline = board(UNASSIGNED_PIN_BOARD);
        let routed = board(UNASSIGNED_PIN_BOARD);
        let mut report = check_connectivity(&routed, &FabricationPolicy::default());
        assert!(
            !report.unconnected_pads.is_empty(),
            "the fixture must exercise the case"
        );
        report.unconnected_pads.retain(|pad| {
            baseline
                .pads()
                .find(|p| p.id() == *pad)
                .and_then(|p| p.net)
                .is_some_and(|net| net != 0)
        });
        assert!(
            report.unconnected_pads.is_empty(),
            "an unassigned pin must not fail the export: {report:?}"
        );
        assert!(report.blocking_reasons().is_empty(), "{report:?}");
    }

    #[test]
    fn a_pad_that_carried_a_net_in_the_baseline_and_lost_it_is_a_failure() {
        // The counterpart: the baseline *did* bind the pad, so the candidate's
        // net 0 is the router's doing and has to block.
        let baseline = board(&UNASSIGNED_PIN_BOARD.replace(
            r#"(layers "F.Cu" "F.Paste" "F.Mask") (net 0 ""))"#,
            r#"(layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))"#,
        ));
        let routed = board(UNASSIGNED_PIN_BOARD);
        let mut report = check_connectivity(&routed, &FabricationPolicy::default());
        report.unconnected_pads.retain(|pad| {
            baseline
                .pads()
                .find(|p| p.id() == *pad)
                .and_then(|p| p.net)
                .is_some_and(|net| net != 0)
        });
        assert!(
            report
                .blocking_reasons()
                .iter()
                .any(|r| r.contains("carries no net")),
            "a net lost from the baseline is a routing failure: {report:?}"
        );
    }

    #[test]
    fn a_clean_verdict_is_fabrication_ready() {
        let verdict = FabricationVerdict::ready();
        assert!(verdict.fabrication_ready);
        assert_eq!(verdict.blocking_count(), 0);
        assert_eq!(
            crate::result::RouteReport::state_for_verdict(&verdict),
            crate::result::RouteState::Routed
        );
    }

    #[test]
    fn a_router_claim_that_disagrees_with_the_copper_is_recorded() {
        let report = ConnectivityReport {
            connected_nets: 3,
            ..ConnectivityReport::default()
        };
        assert!(router_claim_disagreements(3, &report).is_empty());
        let disagreements = router_claim_disagreements(9, &report);
        assert_eq!(disagreements.len(), 1);
        assert!(disagreements[0].contains('9'));
    }

    #[test]
    fn copper_that_is_meant_to_touch_survives_coordinate_rounding() {
        // Routers write four decimal places, so a track that ends exactly
        // on a pad centre lands a few hundred nanometres away. Missing
        // that would report every routed net as open.
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "SIG")
          (footprint "a:1" (layer "F.Cu") (at 10 10)
            (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (footprint "a:1" (layer "F.Cu") (at 20 10)
            (property "Reference" "R2" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (segment (start 10.5001 10.0001) (end 20 10) (width 0.25) (layer "F.Cu") (net 1))
          (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let report = check_connectivity(&board(text), &FabricationPolicy::default());
        assert_eq!(report.open_nets, 0, "{:?}", report.open);
        assert_eq!(report.connected_nets, 1);
    }

    #[test]
    fn a_gap_wider_than_the_tolerance_is_reported_open() {
        let text = r#"(kicad_pcb
          (version 20260206)
          (layers (0 "F.Cu" signal) (44 "Edge.Cuts" user))
          (net 0 "")
          (net 1 "SIG")
          (footprint "a:1" (layer "F.Cu") (at 10 10)
            (property "Reference" "R1" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (footprint "a:1" (layer "F.Cu") (at 20 10)
            (property "Reference" "R2" (at 0 0) (layer "F.SilkS"))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "SIG")))
          (segment (start 12 10) (end 18 10) (width 0.25) (layer "F.Cu") (net 1))
          (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts") (width 0.1))
        )"#;
        let report = check_connectivity(&board(text), &FabricationPolicy::default());
        assert_eq!(report.open_nets, 1, "a real gap must not be forgiven");
    }

    #[test]
    fn union_find_merges_transitively() {
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(2));
        assert_ne!(uf.find(0), uf.find(3));
        uf.union(0, 3);
        assert_eq!(uf.find(0), uf.find(3));
    }

    #[test]
    fn intersecting_segments_report_zero_distance() {
        let a0 = Point::new(mm_to_nm(0.0), mm_to_nm(0.0));
        let a1 = Point::new(mm_to_nm(10.0), mm_to_nm(0.0));
        let b0 = Point::new(mm_to_nm(5.0), mm_to_nm(-5.0));
        let b1 = Point::new(mm_to_nm(5.0), mm_to_nm(5.0));
        assert_eq!(segment_distance(a0, a1, b0, b1), 0);
    }

    #[test]
    fn a_malformed_candidate_is_reported_as_an_unperformed_check() {
        let err: PcbParseError = parse_sexpr("(kicad_pcb").expect_err("unclosed");
        assert!(err.to_string().contains("malformed .kicad_pcb"));
    }
}
