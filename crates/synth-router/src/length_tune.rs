// SPDX-License-Identifier: Apache-2.0

//! Length tuning: compact accordion meanders for match groups and
//! differential pairs.
//!
//! The skew rules (`E-SYNTH-DRC-014`, `E-SYNTH-DRC-019`) can only report a
//! byte lane, bus or pair that is out of budget. This pass closes the gap on
//! the routed board, after the engine and before validation, so the board it
//! writes is the one `kicad-cli` DRC and the skew rules then judge.
//!
//! For each net short of its target it replaces one straight track with the
//! same track carrying a run of single-sided bumps:
//!
//! ```text
//!        ┌─┐   ┌─┐   ┌─┐
//!   ─────┘ └───┘ └───┘ └─────
//! ```
//!
//! Each bump adds twice its height. The legs sit `self_spacing_multiple`
//! widths apart edge to edge (3W by default), which is the coupling rule a
//! meander must keep with itself. A bump is only placed where its whole
//! footprint, grown by the board clearance, is clear of every other net's
//! tracks, pads and vias, of the board edge, and (by the self-spacing) of the
//! net's own other copper. Meanders placed earlier in the pass count as
//! copper for the ones after.
//!
//! Compactness is the objective, not only feasibility:
//!
//! - the tallest bump that fits is tried first, so a run spans as little of
//!   the track as possible;
//! - a net uses as few tuning regions as possible (one, then two);
//! - a side that keeps the run inside the group's corridor (the box round
//!   the group's copper) is preferred;
//! - every result reports the length added, the area the runs occupy, the
//!   number of regions and the runs outside the corridor.
//!
//! A differential pair is tuned as a unit. Its intra-pair skew is closed
//! first on the short half, on the straight run nearest that half's ends,
//! where a breakout mismatch comes from, with bumps on the side away from the
//! partner so the pair's spacing never shrinks. When the pair is in a match
//! group, both halves then get the same added length.
//!
//! With a declared stackup the target is matched in delay (ps), using each
//! layer's propagation speed and a via delay; otherwise in length.

// Board geometry is far below f64's mantissa; see `pcb_read`.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pcb_read::{PcbBoard, Point, Segment};

/// A set of nets that must match in length: a netclass with `max_skew`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchGroupRequest {
    pub name: String,
    pub nets: Vec<String>,
    pub max_skew_nm: i64,
    /// Tune every member to this length (`target_length`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_length_nm: Option<i64>,
    /// Tune every member to this net's length (`reference`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// Propagation delay per copper layer, from a declared stackup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DelayModel {
    /// `(KiCad layer name, ps per mm)`.
    pub ps_per_mm: Vec<(String, f64)>,
    /// Delay through one via, in ps.
    pub via_ps: f64,
}

impl DelayModel {
    fn layer(&self, layer: &str) -> f64 {
        self.ps_per_mm
            .iter()
            .find(|(name, _)| name == layer)
            .map_or(DEFAULT_PS_PER_MM, |(_, ps)| *ps)
    }
}

/// FR-4 stripline at εr 4.3: sqrt(4.3) × 3.3356 ps/mm.
const DEFAULT_PS_PER_MM: f64 = 6.917;

/// Limits the meanders keep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TuneRules {
    /// Tallest bump, in nm.
    pub amplitude_max_nm: i64,
    /// Smallest bump worth drawing, in nm.
    pub amplitude_min_nm: i64,
    /// Edge-to-edge gap between a meander's legs, in track widths.
    pub self_spacing_multiple: f64,
    /// Clearance to other nets' copper, in nm.
    pub clearance_nm: i64,
    /// Clearance to the board edge, in nm.
    pub edge_clearance_nm: i64,
    /// Most tuning regions one net may get.
    pub max_regions_per_net: usize,
    /// Ceiling on the area one net's meanders occupy, in mm², for the
    /// compactness verdict.
    pub max_area_per_net_mm2: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<DelayModel>,
}

impl Default for TuneRules {
    fn default() -> Self {
        Self {
            amplitude_max_nm: 1_000_000,
            amplitude_min_nm: 50_000,
            self_spacing_multiple: 3.0,
            clearance_nm: 127_000,
            edge_clearance_nm: 300_000,
            max_regions_per_net: 2,
            max_area_per_net_mm2: 20.0,
            delay: None,
        }
    }
}

/// What tuning did to one net.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetTune {
    pub net: String,
    pub before_nm: i64,
    pub after_nm: i64,
    pub added_nm: i64,
    pub regions: usize,
    /// Area the net's meander runs occupy, in mm².
    pub area_mm2: f64,
    /// Runs whose footprint leaves the group's corridor.
    pub outside_corridor: usize,
}

/// One match group or pair, before and after.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupTune {
    pub name: String,
    pub max_skew_nm: i64,
    pub before_spread_nm: i64,
    pub after_spread_nm: i64,
    pub met: bool,
    /// Every net's added area and region count are inside the ceilings.
    pub within_ceilings: bool,
    pub nets: Vec<NetTune>,
    /// Why a net could not be brought into budget.
    pub unresolved: Vec<String>,
}

/// The pass's result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TuneReport {
    pub groups: Vec<GroupTune>,
    pub pairs: Vec<GroupTune>,
    /// The unit lengths are measured in: `"nm"` or `"ps"`.
    pub unit: String,
}

impl TuneReport {
    /// Total length added across every net.
    #[must_use]
    pub fn added_nm(&self) -> i64 {
        self.groups
            .iter()
            .chain(&self.pairs)
            .flat_map(|g| &g.nets)
            .map(|n| n.added_nm)
            .sum()
    }

    /// Every group and pair meets its budget.
    #[must_use]
    pub fn all_met(&self) -> bool {
        self.groups.iter().chain(&self.pairs).all(|g| g.met)
    }
}

/// A point in nanometres.
type Xy = (f64, f64);
/// An axis-aligned box `(min, max)`.
type Corridor = (Xy, Xy);
/// A candidate run: the chain, its area in mm², and whether it leaves the
/// corridor.
type Run = (Vec<Xy>, f64, bool);

/// A track in local working form.
#[derive(Debug, Clone, PartialEq)]
struct Track {
    net: String,
    layer: String,
    a: (f64, f64),
    b: (f64, f64),
    width: f64,
    /// Index into the parsed board's segments, `None` for meander copper.
    source: Option<usize>,
}

/// A round obstacle: a pad or a via.
#[derive(Debug, Clone)]
struct Disc {
    net: Option<String>,
    /// `None` for every layer.
    layer: Option<Vec<String>>,
    at: (f64, f64),
    radius: f64,
}

/// One replaced track: the original and the chain that replaces it.
#[derive(Debug, Clone)]
struct Replacement {
    source: usize,
    chain: Vec<(f64, f64)>,
}

struct Work<'a> {
    rules: &'a TuneRules,
    tracks: Vec<Track>,
    discs: Vec<Disc>,
    edges: Vec<((f64, f64), (f64, f64))>,
    vias_per_net: BTreeMap<String, usize>,
    replacements: Vec<Replacement>,
}

fn p(point: Point) -> (f64, f64) {
    (point.x_nm as f64, point.y_nm as f64)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// A net's name: from the board's net table by code, else the name written
/// on the item. The reader fills `net_name` with the code's digits when the
/// item writes only `(net 3)`, so the table comes first.
fn net_name(board: &PcbBoard, code: Option<u32>, named: Option<&String>) -> Option<String> {
    code.and_then(|c| board.nets.get(&c).cloned())
        .or_else(|| named.cloned())
        .filter(|n| !n.is_empty())
}

impl<'a> Work<'a> {
    fn new(board: &'a PcbBoard, rules: &'a TuneRules) -> Self {
        let mut tracks = Vec::new();
        for (i, s) in board.segments.iter().enumerate() {
            let Some(net) = net_name(board, s.net, s.net_name.as_ref()) else {
                continue;
            };
            tracks.push(Track {
                net,
                layer: s.layer.clone(),
                a: p(s.start),
                b: p(s.end),
                width: s.width_nm as f64,
                source: Some(i),
            });
        }
        for s in &board.arc_chords {
            if let Some(net) = net_name(board, s.net, s.net_name.as_ref()) {
                tracks.push(Track {
                    net,
                    layer: s.layer.clone(),
                    a: p(s.start),
                    b: p(s.end),
                    width: s.width_nm as f64,
                    source: None,
                });
            }
        }
        let mut discs = Vec::new();
        let mut vias_per_net: BTreeMap<String, usize> = BTreeMap::new();
        for v in &board.vias {
            let net = net_name(board, v.net, v.net_name.as_ref());
            if let Some(n) = &net {
                *vias_per_net.entry(n.clone()).or_default() += 1;
            }
            discs.push(Disc {
                net,
                layer: Some(v.layers.clone()),
                at: p(v.at),
                radius: v.size_nm as f64 / 2.0,
            });
        }
        for fp in &board.footprints {
            for pad in &fp.pads {
                let layers: Vec<String> = ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu"]
                    .iter()
                    .filter(|l| pad.copper_on(l) || pad.hole().is_some())
                    .map(ToString::to_string)
                    .collect();
                discs.push(Disc {
                    net: net_name(board, pad.net, pad.net_name.as_ref()),
                    layer: Some(layers),
                    at: p(pad.at),
                    radius: pad.reach_nm(),
                });
            }
        }
        let edges = board
            .edge_cuts
            .iter()
            .map(|(a, b)| (p(*a), p(*b)))
            .collect();
        Self {
            rules,
            tracks,
            discs,
            edges,
            vias_per_net,
            replacements: Vec::new(),
        }
    }

    /// A net's measure: routed length in nm, or delay in ps under a model.
    fn measure(&self, net: &str) -> f64 {
        match &self.rules.delay {
            None => longest_path(self.tracks.iter().filter(|t| t.net == net)),
            Some(model) => {
                let mut by_layer: BTreeMap<&str, Vec<&Track>> = BTreeMap::new();
                for t in self.tracks.iter().filter(|t| t.net == net) {
                    by_layer.entry(t.layer.as_str()).or_default().push(t);
                }
                let along: f64 = by_layer
                    .iter()
                    .map(|(layer, ts)| {
                        ts.iter().map(|t| dist(t.a, t.b)).sum::<f64>() / 1e6 * model.layer(layer)
                    })
                    .sum();
                along + self.vias_per_net.get(net).copied().unwrap_or(0) as f64 * model.via_ps
            }
        }
    }

    /// Convert a measure deficit to track length on `layer`, in nm.
    fn to_length(&self, deficit: f64, layer: &str) -> f64 {
        match &self.rules.delay {
            None => deficit,
            Some(model) => deficit / model.layer(layer) * 1e6,
        }
    }

    fn to_measure(&self, length_nm: f64, layer: &str) -> f64 {
        match &self.rules.delay {
            None => length_nm,
            Some(model) => length_nm / 1e6 * model.layer(layer),
        }
    }

    /// Whether the rectangle `(t0, t1) × (0, h)` beside track `host` on side
    /// `side`, grown by the clearances, is clear.
    fn clear(&self, host: usize, t0: f64, t1: f64, h: f64, side: f64) -> bool {
        let track = &self.tracks[host];
        let w = track.width;
        let self_gap = self.rules.self_spacing_multiple * w;
        let frame = Frame::new(track.a, track.b, side);
        for (i, other) in self.tracks.iter().enumerate() {
            if i == host || other.layer != track.layer {
                continue;
            }
            let grow = if other.net == track.net {
                self_gap
            } else {
                self.rules.clearance_nm as f64
            };
            let rect = (
                t0 - w / 2.0 - grow,
                t1 + w / 2.0 + grow,
                0.0,
                h + w / 2.0 + grow,
            );
            if frame.segment_hits(other.a, other.b, other.width / 2.0, rect) {
                return false;
            }
        }
        for disc in &self.discs {
            if disc
                .layer
                .as_ref()
                .is_some_and(|ls| !ls.iter().any(|l| l == &track.layer))
            {
                continue;
            }
            let grow = if disc.net.as_deref() == Some(track.net.as_str()) {
                self_gap
            } else {
                self.rules.clearance_nm as f64
            };
            let rect = (
                t0 - w / 2.0 - grow,
                t1 + w / 2.0 + grow,
                0.0,
                h + w / 2.0 + grow,
            );
            if frame.segment_hits(disc.at, disc.at, disc.radius, rect) {
                return false;
            }
        }
        let grow = self.rules.edge_clearance_nm as f64;
        let rect = (
            t0 - w / 2.0 - grow,
            t1 + w / 2.0 + grow,
            0.0,
            h + w / 2.0 + grow,
        );
        !self
            .edges
            .iter()
            .any(|(a, b)| frame.segment_hits(*a, *b, 0.0, rect))
    }

    /// Place a run adding `extra` nm to `net`, on its longest free straight
    /// tracks. `away_from` keeps the bumps off one net's side (a pair's
    /// partner); `near` prefers tracks close to the given points (a pair's
    /// breakout). Returns the regions placed and their area, or `None`.
    fn place(
        &mut self,
        net: &str,
        extra: f64,
        away_from: Option<&str>,
        near: &[(f64, f64)],
        corridor: Option<Corridor>,
    ) -> Option<(usize, f64, usize)> {
        if extra <= 0.0 {
            return Some((0, 0.0, 0));
        }
        let mut hosts: Vec<usize> = (0..self.tracks.len())
            .filter(|&i| self.tracks[i].net == net && self.tracks[i].source.is_some())
            .collect();
        if near.is_empty() {
            hosts.sort_by(|&a, &b| {
                let la = dist(self.tracks[a].a, self.tracks[a].b);
                let lb = dist(self.tracks[b].a, self.tracks[b].b);
                lb.total_cmp(&la).then(a.cmp(&b))
            });
        } else {
            let near_d = |i: usize| {
                let t = &self.tracks[i];
                let mid = (f64::midpoint(t.a.0, t.b.0), f64::midpoint(t.a.1, t.b.1));
                near.iter()
                    .map(|q| dist(mid, *q))
                    .fold(f64::INFINITY, f64::min)
            };
            hosts.sort_by(|&a, &b| near_d(a).total_cmp(&near_d(b)).then(a.cmp(&b)));
        }
        for regions in 1..=self.rules.max_regions_per_net {
            // A split that places some regions and then runs out of room
            // must not leave those regions behind for the next attempt.
            let saved = (self.tracks.clone(), self.replacements.len());
            if let Some(found) = self.try_regions(&hosts, extra, regions, away_from, corridor) {
                return Some(found);
            }
            self.tracks = saved.0;
            self.replacements.truncate(saved.1);
        }
        None
    }

    fn try_regions(
        &mut self,
        hosts: &[usize],
        extra: f64,
        regions: usize,
        away_from: Option<&str>,
        corridor: Option<Corridor>,
    ) -> Option<(usize, f64, usize)> {
        let share = extra / regions as f64;
        let mut placed: Vec<(usize, Run)> = Vec::new();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for _ in 0..regions {
            let mut found = None;
            for &host in hosts {
                if used.contains(&host) {
                    continue;
                }
                let track = self.tracks[host].clone();
                let length = self.to_length(share, &track.layer);
                if let Some((chain, area, outside)) = self
                    .candidates(host, length, away_from, corridor)
                    .into_iter()
                    .next()
                {
                    found = Some((host, chain, area, outside));
                    break;
                }
            }
            let (host, chain, area, outside) = found?;
            used.insert(host);
            // Commit now so later regions see this one as copper.
            self.commit(host, &chain);
            placed.push((host, (chain, area, outside)));
        }
        let area: f64 = placed.iter().map(|(_, (_, a, _))| *a).sum();
        let outside = placed.iter().filter(|(_, (_, _, o))| *o).count();
        Some((placed.len(), area, outside))
    }

    /// Every legal run on `host` adding `length`, best first: tallest bump,
    /// then inside the corridor.
    fn candidates(
        &self,
        host: usize,
        length: f64,
        away_from: Option<&str>,
        corridor: Option<Corridor>,
    ) -> Vec<Run> {
        let track = &self.tracks[host];
        let w = track.width;
        let pitch = (self.rules.self_spacing_multiple + 1.0) * w;
        let end_margin = pitch + w;
        let span_avail = dist(track.a, track.b) - 2.0 * end_margin;
        if span_avail <= 0.0 {
            return Vec::new();
        }
        let sides: Vec<f64> = match away_from.and_then(|partner| self.partner_side(host, partner)) {
            Some(partner_side) => vec![-partner_side],
            None => vec![1.0, -1.0],
        };
        let mut out = Vec::new();
        let max_h = self.rules.amplitude_max_nm as f64;
        for fraction in [1.0, 0.75, 0.5, 0.35, 0.25, 0.15] {
            let h_cap = (max_h * fraction).max(self.rules.amplitude_min_nm as f64);
            let bumps = (length / (2.0 * h_cap)).ceil().max(1.0);
            let h = length / (2.0 * bumps);
            if h < self.rules.amplitude_min_nm as f64 && length > 0.0 && bumps > 1.0 {
                continue;
            }
            let span = (2.0 * bumps - 1.0) * pitch;
            if span > span_avail {
                continue;
            }
            let t0 = (dist(track.a, track.b) - span) / 2.0;
            let mut fits: Vec<Run> = Vec::new();
            for &side in &sides {
                if !self.clear(host, t0, t0 + span, h, side) {
                    continue;
                }
                let frame = Frame::new(track.a, track.b, side);
                let chain = meander_chain(&frame, t0, bumps as usize, pitch, h);
                let area = span * (h + w) / 1e12;
                let outside = corridor.is_some_and(|c| {
                    let far = [frame.at(t0, h + w / 2.0), frame.at(t0 + span, h + w / 2.0)];
                    far.iter()
                        .any(|q| q.0 < (c.0).0 || q.0 > (c.1).0 || q.1 < (c.0).1 || q.1 > (c.1).1)
                });
                fits.push((chain, area, outside));
            }
            fits.sort_by_key(|(_, _, outside)| *outside);
            out.extend(fits);
            if !out.is_empty() {
                break;
            }
        }
        out
    }

    /// Which side of `host` the partner net's nearest track lies on: `1.0`
    /// for the frame's positive normal, `-1.0` for the negative.
    fn partner_side(&self, host: usize, partner: &str) -> Option<f64> {
        let track = &self.tracks[host];
        let frame = Frame::new(track.a, track.b, 1.0);
        let mid = (
            f64::midpoint(track.a.0, track.b.0),
            f64::midpoint(track.a.1, track.b.1),
        );
        let nearest = self
            .tracks
            .iter()
            .filter(|t| t.net == partner && t.layer == track.layer)
            .min_by(|x, y| {
                let mx = (f64::midpoint(x.a.0, x.b.0), f64::midpoint(x.a.1, x.b.1));
                let my = (f64::midpoint(y.a.0, y.b.0), f64::midpoint(y.a.1, y.b.1));
                dist(mid, mx).total_cmp(&dist(mid, my))
            })?;
        let m = (
            f64::midpoint(nearest.a.0, nearest.b.0),
            f64::midpoint(nearest.a.1, nearest.b.1),
        );
        Some(if frame.local(m).1 >= 0.0 { 1.0 } else { -1.0 })
    }

    fn commit(&mut self, host: usize, chain: &[(f64, f64)]) {
        let track = self.tracks[host].clone();
        let source = track.source.expect("hosts are file tracks");
        // The host's slot takes the chain's first piece so every other index
        // stays valid; the pieces carry no source, so none is a host again.
        let mut pieces = chain.windows(2).map(|pair| (pair[0], pair[1]));
        let (a, b) = pieces.next().unwrap_or((track.a, track.b));
        self.tracks[host] = Track {
            a,
            b,
            source: None,
            ..track.clone()
        };
        for (a, b) in pieces.chain(std::iter::once((
            chain.last().copied().unwrap_or(track.b),
            track.b,
        ))) {
            self.tracks.push(Track {
                net: track.net.clone(),
                layer: track.layer.clone(),
                a,
                b,
                width: track.width,
                source: None,
            });
        }
        self.replacements.push(Replacement {
            source,
            chain: chain.to_vec(),
        });
    }

    fn ends_of(&self, net: &str) -> Vec<(f64, f64)> {
        let mut degree: BTreeMap<(i64, i64), usize> = BTreeMap::new();
        for t in self.tracks.iter().filter(|t| t.net == net) {
            for q in [t.a, t.b] {
                *degree.entry((q.0 as i64, q.1 as i64)).or_default() += 1;
            }
        }
        degree
            .into_iter()
            .filter(|(_, d)| *d == 1)
            .map(|((x, y), _)| (x as f64, y as f64))
            .collect()
    }

    fn corridor(&self, nets: &[String]) -> Option<Corridor> {
        let mut lo = (f64::INFINITY, f64::INFINITY);
        let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for t in self.tracks.iter().filter(|t| nets.contains(&t.net)) {
            for q in [t.a, t.b] {
                lo = (lo.0.min(q.0), lo.1.min(q.1));
                hi = (hi.0.max(q.0), hi.1.max(q.1));
            }
        }
        lo.0.is_finite().then_some((lo, hi))
    }
}

/// Longest path through a net's tracks, the way the skew rules measure it.
fn longest_path<'t>(tracks: impl Iterator<Item = &'t Track>) -> f64 {
    let segments: Vec<synth_pcb::Segment> = tracks
        .map(|t| synth_pcb::Segment {
            net: synth_ir::NetId(0),
            layer: synth_geometry::Layer::Top,
            start: synth_geometry::Point::new(t.a.0.round() as i64, t.a.1.round() as i64),
            end: synth_geometry::Point::new(t.b.0.round() as i64, t.b.1.round() as i64),
            width_nm: t.width as i64,
        })
        .collect();
    let routing = synth_pcb::Routing {
        segments,
        ..synth_pcb::Routing::default()
    };
    routing.length_for_net(synth_ir::NetId(0)) as f64
}

/// A track's local frame: `t` along it from `a`, `u` along the chosen normal.
struct Frame {
    origin: (f64, f64),
    along: (f64, f64),
    normal: (f64, f64),
}

impl Frame {
    fn new(a: (f64, f64), b: (f64, f64), side: f64) -> Self {
        let len = dist(a, b).max(1.0);
        let along = ((b.0 - a.0) / len, (b.1 - a.1) / len);
        Self {
            origin: a,
            along,
            normal: (-along.1 * side, along.0 * side),
        }
    }

    fn at(&self, t: f64, u: f64) -> (f64, f64) {
        (
            self.origin.0 + self.along.0 * t + self.normal.0 * u,
            self.origin.1 + self.along.1 * t + self.normal.1 * u,
        )
    }

    fn local(&self, q: (f64, f64)) -> (f64, f64) {
        let d = (q.0 - self.origin.0, q.1 - self.origin.1);
        (
            d.0 * self.along.0 + d.1 * self.along.1,
            d.0 * self.normal.0 + d.1 * self.normal.1,
        )
    }

    /// Whether the segment `a`-`b` swept by `radius` touches the local box
    /// `(t0, t1, u0, u1)`.
    fn segment_hits(
        &self,
        a: (f64, f64),
        b: (f64, f64),
        radius: f64,
        rect: (f64, f64, f64, f64),
    ) -> bool {
        let la = self.local(a);
        let lb = self.local(b);
        segment_rect_distance(la, lb, rect) <= radius
    }
}

fn segment_rect_distance(a: (f64, f64), b: (f64, f64), r: (f64, f64, f64, f64)) -> f64 {
    let (t0, t1, u0, u1) = r;
    let inside = |q: (f64, f64)| q.0 >= t0 && q.0 <= t1 && q.1 >= u0 && q.1 <= u1;
    if inside(a) || inside(b) {
        return 0.0;
    }
    let corners = [(t0, u0), (t1, u0), (t1, u1), (t0, u1)];
    let mut best = f64::INFINITY;
    for i in 0..4 {
        let e = (corners[i], corners[(i + 1) % 4]);
        best = best.min(seg_seg(a, b, e.0, e.1));
    }
    best
}

#[allow(clippy::many_single_char_names)]
fn point_seg(q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let d = (b.0 - a.0, b.1 - a.1);
    let len2 = d.0 * d.0 + d.1 * d.1;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((q.0 - a.0) * d.0 + (q.1 - a.1) * d.1) / len2).clamp(0.0, 1.0)
    };
    dist(q, (a.0 + t * d.0, a.1 + t * d.1))
}

#[allow(clippy::many_single_char_names)]
fn seg_seg(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    let cross = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let d1 = cross(a, b, c);
    let d2 = cross(a, b, d);
    let d3 = cross(c, d, a);
    let d4 = cross(c, d, b);
    if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
        return 0.0;
    }
    point_seg(a, c, d)
        .min(point_seg(b, c, d))
        .min(point_seg(c, a, b))
        .min(point_seg(d, a, b))
}

/// The points of a track carrying `bumps` bumps of height `h`, `pitch` apart,
/// starting `t0` along it.
fn meander_chain(frame: &Frame, t0: f64, bumps: usize, pitch: f64, h: f64) -> Vec<(f64, f64)> {
    let mut chain = vec![frame.origin];
    let mut t = t0;
    for i in 0..bumps {
        chain.push(frame.at(t, 0.0));
        chain.push(frame.at(t, h));
        chain.push(frame.at(t + pitch, h));
        chain.push(frame.at(t + pitch, 0.0));
        t += pitch;
        if i + 1 < bumps {
            t += pitch;
        }
    }
    chain
}

/// Spread of a set of measures.
fn spread(values: &[f64]) -> f64 {
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    if values.is_empty() { 0.0 } else { max - min }
}

/// Plan the tuning for `board` without writing anything: the report and the
/// replacements to make.
#[allow(clippy::too_many_lines)]
fn plan(
    board: &PcbBoard,
    groups: &[MatchGroupRequest],
    pairs: &[crate::contract::DiffPairRequest],
    rules: &TuneRules,
) -> (TuneReport, Vec<(usize, Vec<Xy>)>) {
    let mut work = Work::new(board, rules);
    let unit = if rules.delay.is_some() { "ps" } else { "nm" };
    let mut report = TuneReport {
        unit: unit.to_string(),
        ..TuneReport::default()
    };
    // Each replacement records the end point of the original track it took;
    // the host track's end `b` closes the chain.
    let mut pair_of: BTreeMap<String, String> = BTreeMap::new();
    for pair in pairs {
        pair_of.insert(pair.positive.clone(), pair.negative.clone());
        pair_of.insert(pair.negative.clone(), pair.positive.clone());
    }
    let allowance = |budget: f64| budget / 4.0;

    // 1. Intra-pair skew, on the short half near its ends.
    for pair in pairs.iter().filter(|p| p.declares_skew()) {
        let budget = to_unit(&work, pair.max_skew_nm.unwrap_or(0) as f64);
        let (lp, ln) = (work.measure(&pair.positive), work.measure(&pair.negative));
        let mut tune = GroupTune {
            name: format!("{}/{}", pair.positive, pair.negative),
            max_skew_nm: pair.max_skew_nm.unwrap_or(0),
            before_spread_nm: from_unit(&work, (lp - ln).abs()),
            ..GroupTune::default()
        };
        if lp == 0.0 || ln == 0.0 {
            // A half with no copper cannot be measured, so the pair is not
            // known to match: record it unmet rather than leave it out.
            for (net, length) in [(&pair.positive, lp), (&pair.negative, ln)] {
                if length == 0.0 {
                    tune.unresolved
                        .push(format!("`{net}` has no routed copper to measure"));
                }
            }
            report.pairs.push(tune);
            continue;
        }
        let (short, long, deficit) = if lp < ln {
            (&pair.positive, &pair.negative, ln - lp)
        } else {
            (&pair.negative, &pair.positive, lp - ln)
        };
        let mut net = NetTune {
            net: short.clone(),
            before_nm: from_unit(&work, work.measure(short)),
            ..NetTune::default()
        };
        if deficit > allowance(budget) {
            let ends = work.ends_of(short);
            match work.place(short, deficit, Some(long), &ends, None) {
                Some((regions, area, outside)) => {
                    net.regions = regions;
                    net.area_mm2 = area;
                    net.outside_corridor = outside;
                }
                None => tune.unresolved.push(format!(
                    "no straight run of `{short}` has room for {:.3} mm of meander",
                    work.to_length(deficit, "F.Cu") / 1e6
                )),
            }
        }
        net.after_nm = from_unit(&work, work.measure(short));
        net.added_nm = net.after_nm - net.before_nm;
        let after = (work.measure(&pair.positive) - work.measure(&pair.negative)).abs();
        tune.after_spread_nm = from_unit(&work, after);
        tune.met = after <= budget;
        tune.within_ceilings =
            net.regions <= rules.max_regions_per_net && net.area_mm2 <= rules.max_area_per_net_mm2;
        tune.nets.push(net);
        report.pairs.push(tune);
    }

    // 2. Match groups. A pair inside a group is one unit: both halves get
    //    the deficit of the longer half.
    for group in groups
        .iter()
        .filter(|g| g.max_skew_nm > 0 && g.nets.len() >= 2)
    {
        let budget = to_unit(&work, group.max_skew_nm as f64);
        let routed: Vec<String> = group
            .nets
            .iter()
            .filter(|n| work.measure(n) > 0.0)
            .cloned()
            .collect();
        if routed.len() < 2 {
            // Fewer than two measurable members means the group's skew is
            // unknown, not inside budget: record it unmet.
            report.groups.push(GroupTune {
                name: group.name.clone(),
                max_skew_nm: group.max_skew_nm,
                unresolved: group
                    .nets
                    .iter()
                    .filter(|n| !routed.contains(n))
                    .map(|n| format!("`{n}` has no routed copper to measure"))
                    .collect(),
                ..GroupTune::default()
            });
            continue;
        }
        let before: Vec<f64> = routed.iter().map(|n| work.measure(n)).collect();
        let corridor = work.corridor(&routed);
        let target = match (&group.reference, group.target_length_nm) {
            (Some(reference), _) if routed.contains(reference) => work.measure(reference),
            (_, Some(target)) => to_unit(&work, target as f64),
            _ => before.iter().copied().fold(0.0, f64::max),
        };
        let mut tune = GroupTune {
            name: group.name.clone(),
            max_skew_nm: group.max_skew_nm,
            before_spread_nm: from_unit(&work, spread(&before)),
            ..GroupTune::default()
        };
        let mut done: BTreeSet<String> = BTreeSet::new();
        for net in &routed {
            if done.contains(net) || group.reference.as_deref() == Some(net.as_str()) {
                continue;
            }
            let partner = pair_of.get(net).filter(|p| routed.contains(p)).cloned();
            let members: Vec<String> = std::iter::once(net.clone())
                .chain(partner.clone())
                .collect();
            let length = members.iter().map(|n| work.measure(n)).fold(0.0, f64::max);
            let deficit = target - length;
            for member in &members {
                done.insert(member.clone());
                let mut tuned = NetTune {
                    net: member.clone(),
                    before_nm: from_unit(&work, work.measure(member)),
                    ..NetTune::default()
                };
                if group.reference.as_deref() == Some(member.as_str()) {
                    // The reference sets the target; it is never tuned.
                } else if deficit < -budget / 2.0 {
                    tune.unresolved.push(format!(
                        "`{member}` is {:.3} {unit} longer than the target; tuning only adds length",
                        -deficit / if unit == "ps" { 1.0 } else { 1e6 },
                    ));
                } else {
                    // A pair half adds the unit's deficit plus its own gap to
                    // the longer half, so the halves stay matched.
                    let own = deficit + (length - work.measure(member));
                    if own > allowance(budget) {
                        let away = members.iter().find(|n| *n != member).map(String::as_str);
                        match work.place(member, own, away, &[], corridor) {
                            Some((regions, area, outside)) => {
                                tuned.regions = regions;
                                tuned.area_mm2 = area;
                                tuned.outside_corridor = outside;
                            }
                            None => tune.unresolved.push(format!(
                                "no straight run of `{member}` has room for its meander"
                            )),
                        }
                    }
                }
                tuned.after_nm = from_unit(&work, work.measure(member));
                tuned.added_nm = tuned.after_nm - tuned.before_nm;
                tune.nets.push(tuned);
            }
        }
        let after: Vec<f64> = routed.iter().map(|n| work.measure(n)).collect();
        tune.after_spread_nm = from_unit(&work, spread(&after));
        tune.met = spread(&after) <= budget;
        tune.within_ceilings = tune.nets.iter().all(|n| {
            n.regions <= rules.max_regions_per_net && n.area_mm2 <= rules.max_area_per_net_mm2
        });
        report.groups.push(tune);
    }
    let replacements = work
        .replacements
        .iter()
        .map(|r| {
            let end = p(board.segments[r.source].end);
            let mut chain = r.chain.clone();
            chain.push(end);
            (r.source, chain)
        })
        .collect();
    (report, replacements)
}

/// A length in nm as the working measure.
fn to_unit(work: &Work<'_>, nm: f64) -> f64 {
    work.to_measure(nm, "F.Cu")
}

/// A working measure back as an integer report value (nm or ps).
fn from_unit(_work: &Work<'_>, value: f64) -> i64 {
    value.round() as i64
}

/// Tune the board text in memory: the new text and the report.
///
/// # Errors
///
/// Returns the parse error when the text is not a readable board.
pub fn tune_text(
    text: &str,
    groups: &[MatchGroupRequest],
    pairs: &[crate::contract::DiffPairRequest],
    rules: &TuneRules,
) -> Result<(String, TuneReport), String> {
    let board = PcbBoard::parse(text).map_err(|e| e.to_string())?;
    let (report, replacements) = plan(&board, groups, pairs, rules);
    let mut out = text.to_string();
    for (source, chain) in &replacements {
        let s: &Segment = &board.segments[*source];
        out = crate::island_vias::remove_segment_in_text(
            &out,
            (
                (s.start.x_mm(), s.start.y_mm()),
                (s.end.x_mm(), s.end.y_mm()),
            ),
            &s.layer,
        );
        let token = s.net.map_or_else(
            || {
                format!(
                    "\"{}\"",
                    s.net_name.clone().unwrap_or_default().replace('"', "\\\"")
                )
            },
            |code| code.to_string(),
        );
        let Some(close) = out.rfind(')') else {
            return Err("board text has no closing parenthesis".to_string());
        };
        let body_end = out[..close].trim_end().len();
        let mut add = String::new();
        for (k, piece) in chain.windows(2).enumerate() {
            let (a, b) = (piece[0], piece[1]);
            if dist(a, b) < 1.0 {
                continue;
            }
            let _ = write!(
                add,
                "\n\t(segment\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(width {})\n\t\t(layer \"{}\")\n\t\t(net {token})\n\t\t(uuid \"{}\")\n\t)",
                mm(a.0),
                mm(a.1),
                mm(b.0),
                mm(b.1),
                mm(s.width_nm as f64),
                s.layer,
                uuid(&format!("{source}:{k}:{}:{}", a.0, a.1)),
            );
        }
        out = format!("{}{add}\n{}", &out[..body_end], &out[close..]);
    }
    Ok((out, report))
}

/// Tune the routed board file in place.
///
/// # Errors
///
/// Returns a description when the file cannot be read, parsed or written.
pub fn tune_file(
    path: &Path,
    groups: &[MatchGroupRequest],
    pairs: &[crate::contract::DiffPairRequest],
    rules: &TuneRules,
) -> Result<TuneReport, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let (out, report) = tune_text(&text, groups, pairs, rules)?;
    if out != text {
        std::fs::write(path, out)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    }
    Ok(report)
}

/// Nanometres as a millimetre coordinate, rounded to 1 µm.
fn mm(nm: f64) -> String {
    let text = format!("{:.4}", (nm / 1000.0).round() / 1000.0);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A stable id, so a rerun writes the same board.
fn uuid(seed: &str) -> String {
    let hash = |salt: u64| {
        let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ salt;
        for byte in seed.bytes() {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    let (a, b) = (hash(3), hash(4));
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xfff,
        (b >> 48) & 0xfff,
        b & 0xffff_ffff_ffff
    )
}

/// Delay per layer from a declared stackup: a stripline at `sqrt(εr)` and a
/// microstrip at the usual FR-4 effective permittivity `0.475 εr + 0.67`, both
/// times 3.3356 ps/mm. A via costs the board thickness at stripline speed.
#[must_use]
pub fn delay_model(stackup: &synth_geometry::StackupProfile) -> DelayModel {
    const PS_PER_MM_VACUUM: f64 = 3.335_64;
    let mut ps_per_mm = Vec::new();
    let mut slowest: f64 = 0.0;
    for geometry in &stackup.layers {
        let er = geometry.er.0 as f64 / 1e6;
        let eff = match geometry.trace_model {
            synth_geometry::TraceModel::Stripline => er,
            synth_geometry::TraceModel::Microstrip => 0.475 * er + 0.67,
        };
        let ps = eff.sqrt() * PS_PER_MM_VACUUM;
        slowest = slowest.max(er.sqrt() * PS_PER_MM_VACUUM);
        ps_per_mm.push((kicad_layer(geometry.layer).to_string(), ps));
    }
    DelayModel {
        ps_per_mm,
        via_ps: stackup.total_thickness.to_mm() * slowest,
    }
}

fn kicad_layer(layer: synth_geometry::Layer) -> &'static str {
    match layer {
        synth_geometry::Layer::Top => "F.Cu",
        synth_geometry::Layer::Inner1 => "In1.Cu",
        synth_geometry::Layer::Inner2 => "In2.Cu",
        synth_geometry::Layer::Inner3 => "In3.Cu",
        synth_geometry::Layer::Inner4 => "In4.Cu",
        synth_geometry::Layer::Bottom => "B.Cu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 0.15;

    /// A board with straight tracks on F.Cu: `(net, y, x_start, x_end)`,
    /// each ending in a small pad at both ends.
    fn board(tracks: &[(&str, f64, f64, f64)]) -> String {
        let mut nets: Vec<&str> = tracks.iter().map(|t| t.0).collect();
        nets.dedup();
        let mut text = String::from(
            "(kicad_pcb\n  (version 20260206)\n  (generator \"test\")\n  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n  (net 0 \"\")\n",
        );
        for (i, n) in nets.iter().enumerate() {
            let _ = writeln!(text, "  (net {} \"{n}\")", i + 1);
        }
        for (k, (net, y, x0, x1)) in tracks.iter().enumerate() {
            let code = nets.iter().position(|n| n == net).unwrap() + 1;
            let _ = writeln!(
                text,
                "  (footprint \"T:P\" (layer \"F.Cu\") (at {x0} {y}) (property \"Reference\" \"TP{k}A\")\n    (pad \"1\" smd rect (at 0 0) (size 0.3 0.3) (layers \"F.Cu\") (net {code} \"{net}\")))\n  (footprint \"T:P\" (layer \"F.Cu\") (at {x1} {y}) (property \"Reference\" \"TP{k}B\")\n    (pad \"1\" smd rect (at 0 0) (size 0.3 0.3) (layers \"F.Cu\") (net {code} \"{net}\")))\n  (segment (start {x0} {y}) (end {x1} {y}) (width {W}) (layer \"F.Cu\") (net {code}))"
            );
        }
        text.push_str(
            "  (gr_line (start 0 0) (end 60 0) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 60 0) (end 60 60) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 60 60) (end 0 60) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 0 60) (end 0 0) (layer \"Edge.Cuts\") (width 0.1))\n)\n",
        );
        text
    }

    fn lengths(text: &str, nets: &[&str]) -> Vec<f64> {
        let board = PcbBoard::parse(text).expect("tuned board parses");
        let rules = TuneRules::default();
        let work = Work::new(&board, &rules);
        nets.iter().map(|n| work.measure(n)).collect()
    }

    /// No two different-net tracks closer than `clearance`, and no two
    /// non-adjacent pieces of one net closer than the self spacing.
    fn assert_spacing(text: &str) {
        let board = PcbBoard::parse(text).expect("parses");
        let segs: Vec<_> = board.segments.iter().collect();
        for (i, a) in segs.iter().enumerate() {
            for b in segs.iter().skip(i + 1) {
                if a.layer != b.layer {
                    continue;
                }
                let d = seg_seg(p(a.start), p(a.end), p(b.start), p(b.end))
                    - (a.width_nm + b.width_nm) as f64 / 2.0;
                if a.net == b.net {
                    // Self spacing is a rule between parallel runs that face
                    // each other: a meander's legs.
                    let da = (a.end.x_nm - a.start.x_nm, a.end.y_nm - a.start.y_nm);
                    let db = (b.end.x_nm - b.start.x_nm, b.end.y_nm - b.start.y_nm);
                    let parallel =
                        i128::from(da.0) * i128::from(db.1) == i128::from(da.1) * i128::from(db.0);
                    let overlap = if da.0 == 0 {
                        let (a0, a1) = (a.start.y_nm.min(a.end.y_nm), a.start.y_nm.max(a.end.y_nm));
                        let (b0, b1) = (b.start.y_nm.min(b.end.y_nm), b.start.y_nm.max(b.end.y_nm));
                        a0.max(b0) < a1.min(b1)
                    } else {
                        let (a0, a1) = (a.start.x_nm.min(a.end.x_nm), a.start.x_nm.max(a.end.x_nm));
                        let (b0, b1) = (b.start.x_nm.min(b.end.x_nm), b.start.x_nm.max(b.end.x_nm));
                        a0.max(b0) < a1.min(b1)
                    };
                    if parallel && overlap {
                        // Legs of the same bump are 3W apart edge to edge.
                        assert!(
                            d >= 3.0 * W * 1e6 - 2_000.0,
                            "{a:?} and {b:?} are {d} nm apart"
                        );
                    }
                } else {
                    assert!(d >= 127_000.0 - 1_000.0, "{a:?} and {b:?} are {d} nm apart");
                }
            }
        }
    }

    /// The acceptance fixture: a byte lane of 8 DQ, DM and a DQS pair at a
    /// 2 mm pitch, lengths from 20 to 26 mm, matched to DQS within 0.2 mm.
    #[test]
    fn a_byte_lane_is_matched_to_its_strobe_compactly() {
        let mut tracks: Vec<(String, f64, f64, f64)> = Vec::new();
        for i in 0..8 {
            tracks.push((
                format!("DQ{i}"),
                5.0 + 2.0 * f64::from(i),
                5.0,
                25.0 + 0.6 * f64::from(i),
            ));
        }
        tracks.push(("DM".to_string(), 21.0, 5.0, 28.0));
        tracks.push(("DQS_P".to_string(), 25.0, 5.0, 31.0));
        tracks.push(("DQS_N".to_string(), 25.6, 5.0, 31.0));
        let refs: Vec<(&str, f64, f64, f64)> = tracks
            .iter()
            .map(|t| (t.0.as_str(), t.1, t.2, t.3))
            .collect();
        let text = board(&refs);
        let nets: Vec<String> = tracks.iter().map(|t| t.0.clone()).collect();
        let group = MatchGroupRequest {
            name: "BYTE0".to_string(),
            nets: nets.clone(),
            max_skew_nm: 200_000,
            target_length_nm: None,
            reference: Some("DQS_P".to_string()),
        };
        let pair = crate::contract::DiffPairRequest {
            positive: "DQS_P".to_string(),
            negative: "DQS_N".to_string(),
            max_skew_nm: Some(100_000),
        };
        let (out, report) =
            tune_text(&text, &[group], &[pair], &TuneRules::default()).expect("tunes");
        let names: Vec<&str> = nets.iter().map(String::as_str).collect();
        let after = lengths(&out, &names);
        let spread_mm = spread(&after) / 1e6;
        assert!(spread_mm <= 0.2, "spread {spread_mm} mm: {report:#?}");
        let g = &report.groups[0];
        assert!(g.met && g.unresolved.is_empty(), "{g:#?}");
        assert!(g.within_ceilings, "{g:#?}");
        // One region per net: compact, not a long zig-zag.
        assert!(g.nets.iter().all(|n| n.regions <= 1), "{g:#?}");
        assert_spacing(&out);
    }

    /// A pair whose positive half breaks out 1.2 mm shorter is closed on that
    /// half, with the bumps on the side away from its partner.
    #[test]
    fn a_pair_with_uneven_breakout_is_tuned_away_from_its_partner() {
        let text = board(&[("USB_P", 10.0, 6.2, 30.0), ("USB_N", 10.4, 5.0, 30.0)]);
        let pair = crate::contract::DiffPairRequest {
            positive: "USB_P".to_string(),
            negative: "USB_N".to_string(),
            max_skew_nm: Some(100_000),
        };
        let (out, report) = tune_text(&text, &[], &[pair], &TuneRules::default()).expect("tunes");
        let after = lengths(&out, &["USB_P", "USB_N"]);
        assert!(
            (after[0] - after[1]).abs() <= 100_000.0,
            "{after:?} {report:#?}"
        );
        assert!(report.pairs[0].met);
        // USB_N sits at y = 10.4, below USB_P; every new USB_P point stays
        // at or above y = 10.
        let board = PcbBoard::parse(&out).expect("parses");
        for s in board.segments.iter().filter(|s| s.net == Some(1)) {
            assert!(
                s.start.y_mm() <= 10.0 + 1e-6 && s.end.y_mm() <= 10.0 + 1e-6,
                "{s:?}"
            );
        }
        assert_spacing(&out);
    }

    #[test]
    fn a_matched_group_is_left_alone() {
        let text = board(&[("A", 5.0, 5.0, 25.0), ("B", 8.0, 5.0, 25.05)]);
        let group = MatchGroupRequest {
            name: "G".to_string(),
            nets: vec!["A".to_string(), "B".to_string()],
            max_skew_nm: 500_000,
            target_length_nm: None,
            reference: None,
        };
        let (out, report) = tune_text(&text, &[group], &[], &TuneRules::default()).expect("tunes");
        assert_eq!(out, text);
        assert_eq!(report.added_nm(), 0);
    }

    #[test]
    fn a_meander_never_enters_a_neighbours_clearance() {
        // B is 0.5 mm above A on one side and C 0.5 mm below: no room for a
        // 1 mm bump on either side, so a shorter, wider run is used.
        let text = board(&[
            ("A", 10.0, 5.0, 25.0),
            ("B", 9.5, 5.0, 40.0),
            ("C", 10.5, 5.0, 40.0),
            ("D", 30.0, 5.0, 28.0),
        ]);
        let group = MatchGroupRequest {
            name: "G".to_string(),
            nets: vec!["A".to_string(), "D".to_string()],
            max_skew_nm: 200_000,
            target_length_nm: None,
            reference: None,
        };
        let (out, report) = tune_text(&text, &[group], &[], &TuneRules::default()).expect("tunes");
        assert_spacing(&out);
        // A has a 0.5 mm gap either side: 0.5 - 0.075 - 0.127 - 0.075 leaves
        // ~0.22 mm of bump, so 3 mm needs many bumps; it may not fit.
        let g = &report.groups[0];
        assert!(g.met || !g.unresolved.is_empty(), "{g:#?}");
    }

    /// A runs 1 mm inside the bottom edge with a neighbour 0.5 mm above it:
    /// the only tall bump goes through the edge, which must count as a hit.
    #[test]
    fn a_meander_never_crosses_the_board_edge() {
        let text = board(&[
            ("A", 59.0, 5.0, 25.0),
            ("C", 58.5, 5.0, 40.0),
            ("D", 30.0, 5.0, 28.0),
        ]);
        let group = MatchGroupRequest {
            name: "G".to_string(),
            nets: vec!["A".to_string(), "D".to_string()],
            max_skew_nm: 200_000,
            target_length_nm: None,
            reference: None,
        };
        let (out, report) = tune_text(&text, &[group], &[], &TuneRules::default()).expect("tunes");
        let limit = 60.0 - TuneRules::default().edge_clearance_nm as f64 / 1e6 - W / 2.0;
        let board = PcbBoard::parse(&out).expect("parses");
        for s in &board.segments {
            assert!(
                s.start.y_mm() <= limit + 1e-6 && s.end.y_mm() <= limit + 1e-6,
                "{s:?} crosses the edge clearance: {report:#?}"
            );
        }
        assert_spacing(&out);
    }

    /// A group with one routed member has no measurable skew: it is recorded
    /// unmet, so `all_met` cannot read true.
    #[test]
    fn a_group_with_one_routed_net_is_unmet() {
        let text = board(&[("A", 5.0, 5.0, 25.0)]);
        let group = MatchGroupRequest {
            name: "G".to_string(),
            nets: vec!["A".to_string(), "B".to_string()],
            max_skew_nm: 200_000,
            target_length_nm: None,
            reference: None,
        };
        let (out, report) = tune_text(&text, &[group], &[], &TuneRules::default()).expect("tunes");
        assert_eq!(out, text);
        let g = &report.groups[0];
        assert!(!g.met, "{g:#?}");
        assert!(g.unresolved.iter().any(|r| r.contains("`B`")), "{g:#?}");
        assert!(!report.all_met());
    }

    /// A pair with an unrouted half is recorded unmet, not left out.
    #[test]
    fn a_pair_with_an_unrouted_half_is_unmet() {
        let text = board(&[("USB_P", 10.0, 5.0, 30.0)]);
        let pair = crate::contract::DiffPairRequest {
            positive: "USB_P".to_string(),
            negative: "USB_N".to_string(),
            max_skew_nm: Some(100_000),
        };
        let (_, report) = tune_text(&text, &[], &[pair], &TuneRules::default()).expect("tunes");
        assert!(!report.pairs[0].met, "{report:#?}");
        assert!(!report.all_met());
    }

    #[test]
    fn a_declared_stackup_tunes_in_delay() {
        let rules = TuneRules {
            delay: Some(DelayModel {
                ps_per_mm: vec![("F.Cu".to_string(), 6.0)],
                via_ps: 10.0,
            }),
            ..TuneRules::default()
        };
        let text = board(&[("A", 5.0, 5.0, 25.0), ("B", 10.0, 5.0, 27.0)]);
        let group = MatchGroupRequest {
            name: "G".to_string(),
            nets: vec!["A".to_string(), "B".to_string()],
            // 0.2 mm of budget is 1.2 ps at 6 ps/mm.
            max_skew_nm: 200_000,
            target_length_nm: None,
            reference: None,
        };
        let (_, report) = tune_text(&text, &[group], &[], &rules).expect("tunes");
        assert_eq!(report.unit, "ps");
        assert!(report.groups[0].met, "{report:#?}");
        // 2 mm at 6 ps/mm: 12 ps added to A.
        let added = report.groups[0]
            .nets
            .iter()
            .find(|n| n.net == "A")
            .unwrap()
            .added_nm;
        assert!((added - 12).abs() <= 1, "{report:#?}");
    }
}
