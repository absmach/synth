// SPDX-License-Identifier: Apache-2.0

//! Tie a poured plane's islands back to the rest of the plane.
//!
//! Tracks cut a pour into separate regions. Each region is the same net, and
//! the routing report counts the net as connected, but a region with no via to
//! the plane on the other layer is connected to nothing else: KiCad reports it
//! as a zone against itself, and the board is refused for a reason no
//! diagnostic names.
//!
//! The router's own repair step reads KiCad's missing links and bridges them,
//! but KiCad reports a zone-to-zone link at the zone's origin, not at the
//! island, so there is no place on the pour to put the bridge. This pass reads
//! the poured polygons instead. It joins polygons through the vias, plated
//! pads and tracks that touch them, and for every region that holds copper of
//! its own yet is not joined to the main pour, adds a via where the island and
//! a polygon of the main pour on another layer both have room for it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::pcb_read::{mm_to_nm, PcbBoard, Point, NM_PER_MM};

/// The via a net takes when it has none to copy: the Default net class.
const DEFAULT_VIA_SIZE_NM: i64 = 600_000;
const DEFAULT_VIA_DRILL_NM: i64 = 300_000;

/// Copper kept between the via's pad and the edge of the pour, on top of the
/// pad radius. The pour edge already stands off foreign copper by the
/// clearance rule, so this is slack against rounding, not a second clearance.
///
/// 0.1 mm left a decoupling capacitor's ground island on a 77-part board
/// untied: its widest circle has a radius of 0.387 mm, against the 0.4 mm
/// that a 0.6 mm via and 0.1 mm of slack asked for.
const EDGE_SLACK_MM: f64 = 0.05;

/// How many fill-and-tie rounds to run. A via added in one round can move the
/// next fill, so the pass repeats until nothing is left to tie.
const MAX_ROUNDS: usize = 3;

/// Rounding tolerance when comparing a distance with the room a via needs.
const EPS: f64 = 1e-6;

/// Copper kept between a new via's pad and any pad, via or foreign track near
/// it. Inside the pour the fill's own clearance already stands off foreign
/// copper; this covers what the fill does not: pads of the net itself, other
/// vias, and copper on layers where there is no pour.
const VIA_GAP_MM: f64 = 0.2;

/// Spacing of the points tried for a return via.
const RETURN_STEP_MM: f64 = 0.1;

/// How far a return via may be from a signal via when the board declares no
/// stackup: W-SYNTH-DRC-016's own window on a 1.6 mm board.
const DEFAULT_REACH_MM: f64 = 1.53;

/// Kept off the edge of W-SYNTH-DRC-016's window, so a via on the edge is not
/// lost to rounding.
const REACH_MARGIN_MM: f64 = 0.05;

/// A via the pass wants added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TieVia {
    pub net: String,
    pub at: Point,
    pub size_nm: i64,
    pub drill_nm: i64,
}

/// What the pass did to a candidate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TieOutcome {
    /// How many vias were added.
    pub added: usize,
    /// Islands that hold copper of their own and are still tied to nothing: no
    /// spot holds a via, or the rounds ran out. Each is a reason the board must
    /// not be delivered, because KiCad refuses a zone against itself.
    pub unresolved: Vec<String>,
    /// Signal vias on a slow net that no ground return via could be placed for:
    /// listed for the review, not a reason to hold the board back.
    pub advisory: Vec<String>,
}

/// Fill the candidate's zones, tie every island and give every layer change a
/// ground return via, and write the vias into the candidate.
///
/// Both come from the same fill, so a tie via and a return via cannot land on
/// each other, and a clean board costs one fill, not one per pass. The candidate
/// is left unfilled: the delivery step fills it again once it has been
/// validated. On any failure it is put back exactly as it was, however many
/// rounds had already written.
///
/// `ground_nets` are the nets the design's DRC calls ground, by topology and pin
/// as well as by name; a net whose name does not say so, such as `POWER_RETURN`,
/// is ground only if it is listed. The name rule still applies on top.
///
/// # Errors
/// Returns why the board could not be filled, read or written. A caller must
/// treat that as the repair not having run, never as nothing to repair.
pub fn tie_islands(candidate: &Path, ground_nets: &[String]) -> Result<TieOutcome, String> {
    tie_islands_for(candidate, &Ground::new(ground_nets))
}

/// [`tie_islands`] for a [`Ground`] that may also say which nets must keep a
/// return via, so a slow net with no room for one is an advisory.
///
/// # Errors
/// As [`tie_islands`].
pub fn tie_islands_for(candidate: &Path, ground: &Ground) -> Result<TieOutcome, String> {
    tie_islands_with(candidate, ground, &mut |path| {
        crate::zone_fill::filled_text(path)
    })
}

/// [`tie_islands`] with the fill supplied, so a failure in a later round can be
/// tested without KiCad.
fn tie_islands_with(
    candidate: &Path,
    ground: &Ground,
    fill: &mut dyn FnMut(&Path) -> Result<Option<String>, String>,
) -> Result<TieOutcome, String> {
    let original = std::fs::read_to_string(candidate)
        .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
    run_rounds(candidate, ground, fill).inspect_err(|_| {
        // Each round writes the candidate; a later failure must not leave an
        // earlier round's vias in a file that was never checked.
        let _ = std::fs::write(candidate, &original);
    })
}

fn run_rounds(
    candidate: &Path,
    ground: &Ground,
    fill: &mut dyn FnMut(&Path) -> Result<Option<String>, String>,
) -> Result<TieOutcome, String> {
    let mut added = 0;
    for _ in 0..MAX_ROUNDS {
        let Some(filled) = fill(candidate)? else {
            return Ok(TieOutcome {
                added,
                ..TieOutcome::default()
            });
        };
        let board = PcbBoard::parse(&filled).map_err(|e| e.to_string())?;
        let text = std::fs::read_to_string(candidate)
            .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
        let plan = plan_all_with(
            &board,
            dielectric_reach_mm(&text, board.copper_layers().len()),
            ground,
        );
        if plan.vias.is_empty() {
            return Ok(TieOutcome {
                added,
                unresolved: plan.unresolved,
                advisory: plan.advisory,
            });
        }
        let tied = with_vias(&text, &board, &plan.vias)?;
        PcbBoard::parse(&tied).map_err(|e| format!("the tied board does not parse: {e}"))?;
        std::fs::write(candidate, tied)
            .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
        added += plan.vias.len();
    }
    // The rounds are spent: whatever the last fill still wants tied stays untied.
    let Some(filled) = fill(candidate)? else {
        return Ok(TieOutcome {
            added,
            ..TieOutcome::default()
        });
    };
    let board = PcbBoard::parse(&filled).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(candidate)
        .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
    // The whole plan, not the island ties alone: a signal via that still has no
    // legal return via must be reported with the islands, or a candidate that
    // spent its rounds on island vias would leave with the return path missing.
    let islands = plan_ties(&board).vias.len();
    let plan = plan_all_with(
        &board,
        dielectric_reach_mm(&text, board.copper_layers().len()),
        ground,
    );
    let mut unresolved = plan.unresolved;
    unresolved.extend(plan.vias.iter().enumerate().map(|(i, via)| {
        if i < islands {
            format!(
                "a ground island on net {} near ({:.2}, {:.2}) mm is still untied after {MAX_ROUNDS} rounds",
                via.net,
                via.at.x_mm(),
                via.at.y_mm()
            )
        } else {
            format!(
                "a signal via still wants a ground return via on net {} near ({:.2}, {:.2}) mm after {MAX_ROUNDS} rounds",
                via.net,
                via.at.x_mm(),
                via.at.y_mm()
            )
        }
    }));
    Ok(TieOutcome {
        added,
        unresolved,
        advisory: plan.advisory,
    })
}

/// A poured polygon on one layer.
struct Region {
    layer: String,
    ring: Vec<(f64, f64)>,
    min: (f64, f64),
    max: (f64, f64),
    area: f64,
}

impl Region {
    fn new(layer: &str, points: &[Point]) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let ring: Vec<(f64, f64)> = points.iter().map(|p| (p.x_mm(), p.y_mm())).collect();
        let mut min = (f64::MAX, f64::MAX);
        let mut max = (f64::MIN, f64::MIN);
        for &(x, y) in &ring {
            min = (min.0.min(x), min.1.min(y));
            max = (max.0.max(x), max.1.max(y));
        }
        let area = ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
            .sum::<f64>()
            .abs()
            / 2.0;
        Some(Self {
            layer: layer.to_string(),
            ring,
            min,
            max,
            area,
        })
    }

    fn contains(&self, p: (f64, f64)) -> bool {
        if p.0 < self.min.0 || p.0 > self.max.0 || p.1 < self.min.1 || p.1 > self.max.1 {
            return false;
        }
        let mut inside = false;
        let mut j = self.ring.len() - 1;
        for i in 0..self.ring.len() {
            let (xi, yi) = self.ring[i];
            let (xj, yj) = self.ring[j];
            if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Distance from `p` to the nearest edge of the polygon, holes included.
    fn edge_distance(&self, p: (f64, f64)) -> f64 {
        let mut best = f64::MAX;
        let mut j = self.ring.len() - 1;
        for i in 0..self.ring.len() {
            best = best.min(point_segment(p, self.ring[j], self.ring[i]));
            j = i;
        }
        best
    }
}

fn point_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

struct Groups(Vec<usize>);

impl Groups {
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// What the pass would do to a filled board.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TiePlan {
    /// The vias that tie an island to its net's main pour.
    pub vias: Vec<TieVia>,
    /// Copper-bearing islands no via fits in: a finding, not a success.
    pub unresolved: Vec<String>,
    /// Slow-net signal vias with no room for a return via: advisories.
    pub advisory: Vec<String>,
}

/// The vias that would tie each unjoined, copper-bearing island of every
/// poured net to that net's main pour. `board` must carry fills.
#[must_use]
pub fn islands_to_tie(board: &PcbBoard) -> Vec<TieVia> {
    plan_ties(board).vias
}

/// Every poured polygon of the board, by net.
fn pours(board: &PcbBoard) -> BTreeMap<String, Vec<Region>> {
    let mut regions: BTreeMap<String, Vec<Region>> = BTreeMap::new();
    for zone in &board.zones {
        let Some(net) = net_of(board, zone.net, zone.net_name.as_ref()) else {
            continue;
        };
        for (layer, polygons) in &zone.fills {
            for polygon in polygons {
                if let Some(region) = Region::new(layer, polygon) {
                    regions.entry(net.clone()).or_default().push(region);
                }
            }
        }
    }
    regions
}

/// The vias that tie the islands of `board`, and the islands that cannot be tied.
#[must_use]
pub fn plan_ties(board: &PcbBoard) -> TiePlan {
    let mut regions: BTreeMap<String, Vec<Region>> = pours(board);
    let copper = board.copper_layers();
    let mut plan = TiePlan::default();
    for (net, regions) in &mut regions {
        let (mut groups, attached, via_sizes) = join_copper(board, net, regions, &copper);

        // The main pour is the joined set with the most copper.
        let mut area: BTreeMap<usize, f64> = BTreeMap::new();
        for (i, region) in regions.iter().enumerate() {
            *area.entry(groups.find(i)).or_default() += region.area;
        }
        let Some((&main, _)) = area
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        else {
            continue;
        };
        // The same for every island of the net, so it is built once.
        let mains: Vec<usize> = (0..regions.len())
            .filter(|&i| groups.find(i) == main)
            .collect();

        let (size_nm, drill_nm) = usual_via(&via_sizes);
        let need = nm_to_mm(size_nm) / 2.0 + EDGE_SLACK_MM;
        let keepout = Keepout::from_board(board, net);

        let via = TieVia {
            net: net.clone(),
            at: Point::default(),
            size_nm,
            drill_nm,
        };
        for &root in area.keys().filter(|&&root| root != main) {
            let members: Vec<usize> = (0..regions.len())
                .filter(|&i| groups.find(i) == root)
                .collect();
            if !members.iter().any(|&i| attached[i]) {
                continue;
            }
            if let Some(at) = best_spot(regions, &members, &mains, need, &keepout) {
                plan.vias.push(TieVia { at, ..via.clone() });
            } else {
                let first = &regions[members[0]];
                plan.unresolved.push(format!(
                    "an island of net {net} on {} near ({:.2}, {:.2}) mm holds copper but is tied to nothing, and no spot in it and the main pour holds a {} mm via clear of the pads, vias and tracks beside it",
                    first.layer,
                    f64::midpoint(first.min.0, first.max.0),
                    f64::midpoint(first.min.1, first.max.1),
                    nm_to_mm(size_nm),
                ));
            }
        }
    }
    plan
}

/// The island ties and the return vias for one fill. The return vias are
/// planned knowing where the tie vias will go, so neither lands on the other.
#[must_use]
pub fn plan_all(board: &PcbBoard, reach_mm: f64) -> TiePlan {
    plan_all_with(board, reach_mm, &Ground::default())
}

/// [`plan_all`] with the nets the design's DRC calls ground.
#[must_use]
pub fn plan_all_with(board: &PcbBoard, reach_mm: f64, ground: &Ground) -> TiePlan {
    let mut plan = plan_ties(board);
    let stitched = stitch(board, reach_mm, &plan.vias, ground);
    plan.vias.extend(stitched.vias);
    plan.unresolved.extend(stitched.unresolved);
    plan.advisory.extend(stitched.advisory);
    plan
}

/// Which nets are ground. The DRC's return-path check calls a net ground by
/// topology (a ground-reference pin, a declared domain) and by a pin's name, not
/// only by the net's own; the board file carries none of that, so the caller
/// lists them. The name rule is always applied as well.
#[derive(Debug, Clone, Default)]
pub struct Ground {
    named: BTreeSet<String>,
    /// The nets that must keep a return via. `None` is every signal net, which
    /// is the strict reading; `Some` lets any other net go without one as an
    /// advisory.
    critical: Option<BTreeSet<String>>,
}

impl Ground {
    #[must_use]
    pub fn new(names: &[String]) -> Self {
        Self {
            named: names.iter().cloned().collect(),
            critical: None,
        }
    }

    /// Only `critical` nets are held to a return via: a layer change on any
    /// other net with no room for one is listed, not blocking.
    #[must_use]
    pub fn with_critical(mut self, critical: &[String]) -> Self {
        self.critical = Some(critical.iter().cloned().collect());
        self
    }

    fn needs_return(&self, net: &str) -> bool {
        self.critical.as_ref().is_none_or(|set| set.contains(net))
    }

    fn is_ground(&self, net: &str) -> bool {
        synth_drc::is_ground_name(net) || self.named.contains(net)
    }
}

/// The return vias to add, and the signal vias that could not be given one.
struct Stitched {
    vias: Vec<TieVia>,
    unresolved: Vec<String>,
    advisory: Vec<String>,
}

/// A pad's copper as a rectangle turned to the pad's own angle.
struct PadBox {
    at: (f64, f64),
    half: (f64, f64),
    cos: f64,
    sin: f64,
}

impl PadBox {
    fn new(pad: &crate::pcb_read::Pad) -> Self {
        let angle = pad.angle_deg.to_radians();
        Self {
            at: (pad.at.x_mm(), pad.at.y_mm()),
            half: (nm_to_mm(pad.size_nm.0) / 2.0, nm_to_mm(pad.size_nm.1) / 2.0),
            cos: angle.cos(),
            sin: angle.sin(),
        }
    }

    /// Distance from `p` to the pad's rectangle, zero inside it.
    fn distance(&self, p: (f64, f64)) -> f64 {
        let (dx, dy) = (p.0 - self.at.0, p.1 - self.at.1);
        // Into the pad's own axes.
        let (x, y) = (
            dx * self.cos + dy * self.sin,
            -dx * self.sin + dy * self.cos,
        );
        (x.abs() - self.half.0)
            .max(0.0)
            .hypot((y.abs() - self.half.1).max(0.0))
    }
}

/// `(start, end, half width)` of one track.
type Segment = ((f64, f64), (f64, f64), f64);

/// What a new via has to keep clear of, on every layer it crosses: a through
/// via drills the whole stack, so a pad, a via or a foreign track on any layer
/// counts. Another net's pour is not an obstacle: the fill cuts an antipad
/// round the via when it is refilled.
struct Keepout {
    /// `(min, max)` of each pad's copper.
    pads: Vec<PadBox>,
    /// `(start, end, half width)` of each track of another net.
    tracks: Vec<Segment>,
    /// `(centre, radius)` of each via.
    vias: Vec<((f64, f64), f64)>,
}

impl Keepout {
    /// The obstacles on `board` for a via of net `own`. Tracks of `own` are not
    /// obstacles, since a via may sit on its own net's copper.
    fn from_board(board: &PcbBoard, own: &str) -> Self {
        let mut pads = Vec::new();
        for footprint in &board.footprints {
            for pad in &footprint.pads {
                pads.push(PadBox::new(pad));
            }
        }
        let tracks = board
            .segments
            .iter()
            .filter(|segment| {
                net_of(board, segment.net, segment.net_name.as_ref()).as_deref() != Some(own)
            })
            .map(|segment| {
                (
                    (segment.start.x_mm(), segment.start.y_mm()),
                    (segment.end.x_mm(), segment.end.y_mm()),
                    nm_to_mm(segment.width_nm) / 2.0,
                )
            })
            .collect();
        let vias = board
            .vias
            .iter()
            .map(|via| ((via.at.x_mm(), via.at.y_mm()), nm_to_mm(via.size_nm) / 2.0))
            .collect();
        Self { pads, tracks, vias }
    }

    /// Whether a via of `radius` mm at `p` keeps [`VIA_GAP_MM`] of copper from
    /// every obstacle.
    fn clear(&self, p: (f64, f64), radius: f64) -> bool {
        let need = radius + VIA_GAP_MM;
        self.pads.iter().all(|pad| pad.distance(p) >= need - EPS)
            && self
                .tracks
                .iter()
                .all(|(a, b, half)| point_segment(p, *a, *b) >= need + half - EPS)
            && self
                .vias
                .iter()
                .all(|(at, other)| (at.0 - p.0).hypot(at.1 - p.1) >= need + other - EPS)
    }
}

/// The table names a coded net; a name alone is what a board written without a
/// net table carries.
fn net_of(board: &PcbBoard, code: Option<u32>, name: Option<&String>) -> Option<String> {
    code.and_then(|c| board.nets.get(&c).cloned())
        .or_else(|| name.cloned())
        .filter(|n| !n.is_empty())
}

/// Join the regions of `net` through the vias, plated pads and tracks of that
/// net that touch them. Also returns which regions hold any of that copper, and
/// the sizes of the net's existing vias.
fn join_copper(
    board: &PcbBoard,
    net: &str,
    regions: &[Region],
    copper: &[String],
) -> (Groups, Vec<bool>, Vec<(i64, i64)>) {
    let mut groups = Groups((0..regions.len()).collect());
    let mut attached = vec![false; regions.len()];
    let containing = |layer: &str, p: Point| -> Vec<usize> {
        let at = (p.x_mm(), p.y_mm());
        (0..regions.len())
            .filter(|&i| regions[i].layer == layer && regions[i].contains(at))
            .collect()
    };

    let mut via_sizes: Vec<(i64, i64)> = Vec::new();
    for via in &board.vias {
        if net_of(board, via.net, via.net_name.as_ref()).as_deref() != Some(net) {
            continue;
        }
        via_sizes.push((via.size_nm, via.drill_nm));
        let hit: Vec<usize> = via
            .layers
            .iter()
            .flat_map(|layer| containing(layer, via.at))
            .collect();
        join(&mut groups, &mut attached, &hit);
    }
    for pad in board.pads() {
        if net_of(board, pad.net, pad.net_name.as_ref()).as_deref() != Some(net) {
            continue;
        }
        let layers: Vec<&String> = if pad.kind == "thru_hole" {
            copper.iter().collect()
        } else {
            pad.layers.iter().collect()
        };
        let hit: Vec<usize> = layers
            .iter()
            .flat_map(|layer| containing(layer, pad.at))
            .collect();
        join(&mut groups, &mut attached, &hit);
    }
    for segment in &board.segments {
        if net_of(board, segment.net, segment.net_name.as_ref()).as_deref() != Some(net) {
            continue;
        }
        let hit: Vec<usize> = [segment.start, segment.end]
            .iter()
            .flat_map(|end| containing(&segment.layer, *end))
            .collect();
        join(&mut groups, &mut attached, &hit);
    }
    (groups, attached, via_sizes)
}

/// The via size the net uses most, the larger on a tie. The smallest would
/// copy a router's one escalated via, which is under the board's own minimum.
fn usual_via(sizes: &[(i64, i64)]) -> (i64, i64) {
    let mut counts: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for &size in sizes {
        *counts.entry(size).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(size, count)| (count, size))
        .map_or((DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM), |(size, _)| {
            size
        })
}

fn join(groups: &mut Groups, attached: &mut [bool], hit: &[usize]) {
    for &i in hit {
        attached[i] = true;
        groups.union(hit[0], i);
    }
}

/// The point inside an island and a region of the main pour on another layer
/// that stands furthest from both of their edges, if any stands at least
/// `need` from them.
/// The point in an island, and in the main pour on another layer, that holds a
/// via with the most copper to spare and clears the pads, vias and tracks near
/// it. Searched coarse to fine: a grid over the island's box, finer each time it
/// finds nothing, then refined round the best point, so a large pour is not
/// scanned at the finest step.
fn best_spot(
    regions: &[Region],
    island: &[usize],
    main: &[usize],
    need: f64,
    keepout: &Keepout,
) -> Option<Point> {
    let radius = need - EDGE_SLACK_MM;
    // The room a via has at `p`, if `p` is a spot for one.
    let margin_at = |a: &Region, p: (f64, f64)| -> Option<f64> {
        if !a.contains(p) {
            return None;
        }
        let from_a = a.edge_distance(p);
        if from_a < need - EPS || !keepout.clear(p, radius) {
            return None;
        }
        let from_main = main
            .iter()
            .map(|&b| &regions[b])
            .filter(|b| b.layer != a.layer && b.contains(p))
            .map(|b| b.edge_distance(p))
            .fold(f64::MIN, f64::max);
        let margin = from_a.min(from_main);
        (margin >= need - EPS).then_some(margin)
    };
    let mut best: Option<(f64, (f64, f64))> = None;
    for &index in island {
        let a = &regions[index];
        // About 2,500 points at the first pass; a small island has few points
        // to try, so its step has a floor, and the pass repeats finer.
        let coarse = ((a.max.0 - a.min.0) * (a.max.1 - a.min.1) / 2_500.0)
            .sqrt()
            .max(0.02);
        let mut step = coarse;
        let mut found: Option<(f64, (f64, f64))> = None;
        while found.is_none() && step >= 0.02 - EPS {
            let mut y = a.min.1 + need;
            while y <= a.max.1 - need {
                let mut x = a.min.0 + need;
                while x <= a.max.0 - need {
                    if let Some(margin) = margin_at(a, (x, y)) {
                        if found.is_none_or(|(m, _)| margin > m) {
                            found = Some((margin, (x, y)));
                        }
                    }
                    x += step;
                }
                y += step;
            }
            step /= 2.0;
        }
        // Refine round the best point found, twice, at a quarter of the step.
        if let Some((mut margin, mut point)) = found {
            let mut window = step * 2.0;
            for _ in 0..2 {
                let fine = window / 4.0;
                for iy in -4..=4 {
                    for ix in -4..=4 {
                        let p = (
                            point.0 + f64::from(ix) * fine,
                            point.1 + f64::from(iy) * fine,
                        );
                        if let Some(m) = margin_at(a, p) {
                            if m > margin {
                                margin = m;
                                point = p;
                            }
                        }
                    }
                }
                window = fine * 2.0;
            }
            if best.is_none_or(|(m, _)| margin > m) {
                best = Some((margin, point));
            }
        }
    }
    best.map(|(_, (x, y))| Point::new(mm_to_nm(x), mm_to_nm(y)))
}

/// `text` with `vias` written in, in the net spelling the board already uses.
fn with_vias(text: &str, board: &PcbBoard, vias: &[TieVia]) -> Result<String, String> {
    let copper = board.copper_layers();
    let (Some(first), Some(last)) = (copper.first(), copper.last()) else {
        return Err("the board declares no copper layers".to_string());
    };
    // A board with a net table spells a net by its code; one without carries
    // names. The parsed table says which, whatever the file's whitespace.
    let numbered = !board.nets.is_empty();
    let close = text
        .rfind(')')
        .ok_or_else(|| "the board has no closing parenthesis".to_string())?;
    let body_end = text[..close].trim_end().len();

    let mut out = String::with_capacity(text.len() + vias.len() * 220);
    out.push_str(&text[..body_end]);
    for via in vias {
        let net = if numbered {
            let code = board
                .nets
                .iter()
                .find(|(_, name)| **name == via.net)
                .map(|(code, _)| *code)
                .ok_or_else(|| format!("net {} has no code", via.net))?;
            code.to_string()
        } else {
            format!("\"{}\"", escape_net_name(&via.net))
        };
        let _ = write!(
            out,
            "\n\t(via\n\t\t(at {} {})\n\t\t(size {})\n\t\t(drill {})\n\t\t(layers \"{first}\" \"{last}\")\n\t\t(net {net})\n\t\t(uuid \"{}\")\n\t)",
            mm(via.at.x_nm),
            mm(via.at.y_nm),
            mm(via.size_nm),
            mm(via.drill_nm),
            uuid_for(via),
        );
    }
    out.push_str(&text[body_end..]);
    Ok(out)
}

/// A net name as the board file writes it inside quotes: a backslash or a quote
/// in it would otherwise end the string early and break the file.
fn escape_net_name(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}

/// How far from a signal via W-SYNTH-DRC-016 looks for a ground via, along either
/// axis: the thickest dielectric in the board's stackup, as the rule takes it.
/// A board with no stackup gets the rule's own window on a 1.6 mm board.
fn dielectric_reach_mm(text: &str, copper_layers: usize) -> f64 {
    fn block_end(text: &str) -> usize {
        let mut depth = 0_i32;
        for (index, byte) in text.bytes().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        text.len()
    }
    let Some(start) = text.find("(stackup") else {
        return default_reach_mm(copper_layers);
    };
    let stackup = &text[start..start + block_end(&text[start..])];
    let mut dielectrics = Vec::new();
    let mut rest = stackup;
    while let Some(index) = rest.find("(layer ") {
        let layer = &rest[index..index + block_end(&rest[index..])];
        let dielectric = layer.contains("dielectric")
            || layer.contains("(type \"core\")")
            || layer.contains("(type \"prepreg\")");
        if dielectric {
            if let Some(at) = layer.find("(thickness ") {
                let number: String = layer[at + "(thickness ".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(value) = number.parse::<f64>() {
                    dielectrics.push(value);
                }
            }
        }
        rest = &rest[index + layer.len().max(1)..];
    }
    // The rule reads each copper layer's height: the one dielectric beside an
    // outer layer, the mean of the two around an inner layer.
    let mut thickest = 0.0_f64;
    for (index, below) in dielectrics.iter().enumerate() {
        let above = if index == 0 {
            None
        } else {
            dielectrics.get(index - 1)
        };
        thickest = thickest.max(above.map_or(*below, |above| (above + below) / 2.0));
    }
    if let Some(last) = dielectrics.last() {
        thickest = thickest.max(*last);
    }
    if thickest > 0.0 {
        thickest
    } else {
        default_reach_mm(copper_layers)
    }
}

/// The window of the default stackup for a board of `copper_layers`, as the
/// rule computes it for a board that declares none.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn default_reach_mm(copper_layers: usize) -> f64 {
    let thickest = synth_geometry::StackupProfile::default_for_layers(copper_layers as u32)
        .layers
        .iter()
        .map(|geometry| geometry.dielectric_height.to_mm())
        .fold(0.0_f64, f64::max);
    if thickest > 0.0 {
        thickest
    } else {
        DEFAULT_REACH_MM
    }
}

/// The ground vias that give every layer change a return path, for a board whose
/// fill is `board`'s, with `planned` vias about to be added.
///
/// Return current follows the signal's reference plane, and a signal via
/// interrupts it, so the return has to jump layers too. W-SYNTH-DRC-016 flags a
/// via of any net that is not ground, and that has a net, with no ground via
/// within `reach_mm` along either axis; this adds one for each, at the nearest
/// point that lies inside the ground pour on the first and last layers and clears
/// every pad, via and foreign track on every layer. A net named for ground is
/// ground by the DRC's own rule, so `VSSA` is. A via with no net is not seen by
/// the rule and is not given a return. A new via counts for the signal vias after
/// it, so neighbours share one.
#[must_use]
pub fn stitch_vias(board: &PcbBoard, reach_mm: f64, planned: &[TieVia]) -> Vec<TieVia> {
    stitch(board, reach_mm, planned, &Ground::default()).vias
}

fn stitch(board: &PcbBoard, reach_mm: f64, planned: &[TieVia], ground_nets: &Ground) -> Stitched {
    let none = || Stitched {
        vias: Vec::new(),
        unresolved: Vec::new(),
        advisory: Vec::new(),
    };
    let pours = pours(board);
    let Some((net, regions)) = pours
        .iter()
        .filter(|(net, _)| ground_nets.is_ground(net))
        .max_by(|a, b| {
            let area = |r: &Vec<Region>| r.iter().map(|x| x.area).sum::<f64>();
            area(a.1)
                .partial_cmp(&area(b.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    else {
        return none();
    };
    let net_of_via = |via: &crate::pcb_read::Via| net_of(board, via.net, via.net_name.as_ref());
    let is_ground =
        |via: &crate::pcb_read::Via| net_of_via(via).is_some_and(|n| ground_nets.is_ground(&n));

    let sizes: Vec<(i64, i64)> = board
        .vias
        .iter()
        .filter(|via| is_ground(via))
        .map(|via| (via.size_nm, via.drill_nm))
        .collect();
    let (size_nm, drill_nm) = usual_via(&sizes);
    let need = nm_to_mm(size_nm) / 2.0 + EDGE_SLACK_MM;
    let radius = nm_to_mm(size_nm) / 2.0;
    let window = (reach_mm - REACH_MARGIN_MM).max(0.0);

    let mut ground: Vec<(f64, f64)> = board
        .vias
        .iter()
        .filter(|via| is_ground(via))
        .map(|via| (via.at.x_mm(), via.at.y_mm()))
        .collect();
    ground.extend(planned.iter().map(|via| (via.at.x_mm(), via.at.y_mm())));

    let copper = board.copper_layers();
    let (Some(first), Some(last)) = (copper.first(), copper.last()) else {
        return none();
    };
    let room = |layer: &str, p: (f64, f64)| -> bool {
        regions
            .iter()
            .any(|r| r.layer == layer && r.contains(p) && r.edge_distance(p) >= need - EPS)
    };
    let room_both = |p: (f64, f64)| room(first, p) && room(last, p);
    // Built once: the signal vias and the keep-out do not change over the loop.
    let keepout = Keepout::from_board(board, net);
    let signals: Vec<(f64, f64, String)> = board
        .vias
        .iter()
        .filter(|via| !is_ground(via))
        .filter_map(|via| Some((via.at.x_mm(), via.at.y_mm(), net_of_via(via)?)))
        .collect();

    let apart = nm_to_mm(size_nm) + VIA_GAP_MM;
    let mut out = Vec::new();
    let mut unresolved = Vec::new();
    let mut advisory = Vec::new();
    for (vx, vy, signal_net) in &signals {
        let (vx, vy) = (*vx, *vy);
        if ground
            .iter()
            .any(|g| (g.0 - vx).abs() <= reach_mm && (g.1 - vy).abs() <= reach_mm)
        {
            continue;
        }
        let best = nearest_return_spot(
            (vx, vy),
            window,
            apart,
            &ground,
            &room_both,
            &keepout,
            radius,
        );
        if let Some(p) = best {
            ground.push(p);
            out.push(TieVia {
                net: net.clone(),
                at: Point::new(mm_to_nm(p.0), mm_to_nm(p.1)),
                size_nm,
                drill_nm,
            });
        } else if ground_nets.needs_return(signal_net) {
            unresolved.push(format!(
                "the signal via at ({vx:.2}, {vy:.2}) mm has no ground return via within {reach_mm:.2} mm, and no spot in the {net} pour holds one clear of the pads, vias and tracks beside it"
            ));
        } else {
            advisory.push(format!(
                "net {signal_net} changes layers at ({vx:.2}, {vy:.2}) mm with no ground return via within {reach_mm:.2} mm and no spot in the {net} pour for one; the net carries no fast edge, so the loop is left as an advisory"
            ));
        }
    }
    Stitched {
        vias: out,
        unresolved,
        advisory,
    }
}

/// The point nearest the signal via `from`, on the search grid, that is clear of
/// the ground vias, inside the pour on both outer layers and clear of every
/// obstacle.
fn nearest_return_spot(
    from: (f64, f64),
    window: f64,
    apart: f64,
    ground: &[(f64, f64)],
    room: &dyn Fn((f64, f64)) -> bool,
    keepout: &Keepout,
    radius: f64,
) -> Option<(f64, f64)> {
    // The grid, plus the window's own edges: a via and its return need `apart`
    // between centres, which inside a narrow window is only reached out on the
    // diagonal, past the last whole grid step.
    let steps = (window / RETURN_STEP_MM).floor() as i32;
    let offsets: Vec<f64> = (-steps..=steps)
        .map(|i| f64::from(i) * RETURN_STEP_MM)
        .chain([-window, window])
        .collect();
    let mut best: Option<(f64, (f64, f64))> = None;
    for &dx in &offsets {
        for &dy in &offsets {
            let p = (from.0 + dx, from.1 + dy);
            let distance = (p.0 - from.0).hypot(p.1 - from.1);
            if best.is_some_and(|(d, _)| distance >= d) {
                continue;
            }
            if ground.iter().all(|g| (g.0 - p.0).hypot(g.1 - p.1) >= apart)
                && room(p)
                && keepout.clear(p, radius)
            {
                best = Some((distance, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

#[allow(clippy::cast_precision_loss)]
fn nm_to_mm(nm: i64) -> f64 {
    nm as f64 / NM_PER_MM
}

fn mm(nm: i64) -> String {
    let text = format!("{:.4}", nm_to_mm(nm));
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A stable id, so a rerun writes the same board.
fn uuid_for(via: &TieVia) -> String {
    let hash = |salt: u64| {
        let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ salt;
        for byte in format!("{} {} {}", via.net, via.at.x_nm, via.at.y_nm).bytes() {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    let (a, b) = (hash(1), hash(2));
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xfff,
        (b >> 48) & 0xfff,
        b & 0xffff_ffff_ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two layers of GND pour. On F.Cu a track splits it into the main region
    /// (0..20) and an island (25..35) that holds a GND pad; B.Cu is whole.
    fn board(extra: &str) -> PcbBoard {
        PcbBoard::parse(&board_text(extra)).expect("board must parse")
    }

    fn board_text(extra: &str) -> String {
        format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
              (net 0 "")
              (net 1 "GND")
              (net 2 "SIG")
              (segment (start 25 7) (end 30 7) (width 0.25) (layer "F.Cu") (net 1))
              (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
              {extra}
              (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
                (filled_polygon (layer "F.Cu") (pts (xy 25 0) (xy 35 0) (xy 35 20) (xy 25 20))))
              (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))
            "#
        )
    }

    #[test]
    fn a_pad_is_measured_in_its_own_rotated_axes() {
        let pad = PadBox {
            at: (0.0, 0.0),
            half: (1.0, 0.5),
            cos: 0.0,
            sin: 1.0,
        };
        // Turned a quarter, the 2 mm side runs along y.
        assert!((pad.distance((0.0, 1.2)) - 0.2).abs() < 1e-9);
        assert!(pad.distance((0.0, 0.9)).abs() < 1e-9, "inside is zero");
        assert!((pad.distance((1.0, 0.0)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_pad_turned_forty_five_degrees_is_not_its_axis_aligned_box() {
        let pad = PadBox {
            at: (0.0, 0.0),
            half: (2.0, 0.25),
            cos: std::f64::consts::FRAC_1_SQRT_2,
            sin: std::f64::consts::FRAC_1_SQRT_2,
        };
        // The corner of the unturned box is empty; a point on the diagonal is on copper.
        assert!(pad.distance((1.0, 1.0)).abs() < 1e-9);
        assert!(pad.distance((1.0, -1.0)) > 1.0);
    }

    #[test]
    fn an_island_holding_a_track_gets_a_via_inside_both_pours() {
        let vias = islands_to_tie(&board(""));
        assert_eq!(vias.len(), 1, "{vias:?}");
        let via = &vias[0];
        assert_eq!(via.net, "GND");
        assert!(
            via.at.x_mm() > 25.0 && via.at.x_mm() < 35.0,
            "the via must land in the island: {via:?}"
        );
        assert_eq!((via.size_nm, via.drill_nm), (600_000, 300_000));
    }

    #[test]
    fn the_usual_via_wins_over_one_escalated_via() {
        let sizes = [(560_000, 300_000), (600_000, 300_000), (600_000, 300_000)];
        assert_eq!(usual_via(&sizes), (600_000, 300_000));
        assert_eq!(usual_via(&[]), (DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM));
    }

    #[test]
    fn an_island_already_holding_a_via_is_left_alone() {
        let board =
            board(r#"(via (at 30 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#);
        assert!(islands_to_tie(&board).is_empty());
    }

    #[test]
    fn an_island_with_no_copper_of_its_own_is_left_alone() {
        // No track or pad touches the island, so KiCad drops it on fill.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND")
          (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 25 0) (xy 35 0) (xy 35 20) (xy 25 20))))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("board must parse");
        let plan = plan_ties(&board);
        assert!(
            plan.vias.is_empty() && plan.unresolved.is_empty(),
            "{plan:?}"
        );
    }

    #[test]
    fn an_island_too_small_for_the_via_is_reported_not_accepted() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND")
          (segment (start 25 7) (end 25.2 7) (width 0.1) (layer "F.Cu") (net 1))
          (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 25 6.5) (xy 25.5 6.5) (xy 25.5 7.5) (xy 25 7.5))))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("board must parse");
        let plan = plan_ties(&board);
        assert!(plan.vias.is_empty());
        assert_eq!(
            plan.unresolved.len(),
            1,
            "a live island with no room for a via must be a finding: {plan:?}"
        );
        assert!(plan.unresolved[0].contains("GND"), "{plan:?}");
    }

    #[test]
    fn an_island_a_hair_wider_than_the_via_still_gets_one() {
        // 1.5 x 0.75 mm: its widest circle has a radius of 0.375 mm, which a
        // 0.6 mm via (0.3 mm) fits with the 0.05 mm of slack but not with 0.1.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND")
          (segment (start 25.2 6.6) (end 25.4 6.6) (width 0.1) (layer "F.Cu") (net 1))
          (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 25 6.25) (xy 26.5 6.25) (xy 26.5 7) (xy 25 7))))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("board must parse");
        assert_eq!(islands_to_tie(&board).len(), 1);
    }

    /// The net spelling comes from the parsed net table, not from matching two
    /// indentations: a valid board with its table on one line, or indented some
    /// other way, still takes a numeric net code.
    #[test]
    fn a_net_table_in_any_whitespace_gets_a_numeric_via() {
        let board = board("");
        let vias = islands_to_tie(&board);
        for text in [
            "(kicad_pcb (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal)) (net 0 \"\") (net 1 \"GND\"))",
            "(kicad_pcb\n    (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal))\n    (net 0 \"\")\n    (net 1 \"GND\")\n)",
            "(kicad_pcb\r\n\t\t(layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal))\r\n\t\t(net  0  \"\")\r\n\t\t(net  1  \"GND\")\r\n)",
        ] {
            let tied = with_vias(text, &board, &vias).expect("tied");
            let back = PcbBoard::parse(&tied).expect("parses");
            assert_eq!(back.vias.len(), 1, "{text}");
            assert_eq!(back.vias[0].net, Some(1), "numeric code for {text}");
        }
    }

    #[test]
    fn a_board_without_a_net_table_takes_the_net_by_name() {
        let board = PcbBoard::parse(r#"(kicad_pcb (layers (0 "F.Cu" signal) (2 "B.Cu" signal)))"#)
            .expect("parses");
        assert!(board.nets.is_empty());
        let via = TieVia {
            net: "GND".to_string(),
            at: Point::new(mm_to_nm(1.0), mm_to_nm(2.0)),
            size_nm: 600_000,
            drill_nm: 300_000,
        };
        let text = r#"(kicad_pcb (layers (0 "F.Cu" signal) (2 "B.Cu" signal)))"#;
        let tied = with_vias(text, &board, &[via]).expect("tied");
        assert!(tied.contains("(net \"GND\")"), "{tied}");
    }

    const REACH: f64 = DEFAULT_REACH_MM;

    /// GND pours on both layers, `layers` declared, one signal via at (10, 10)
    /// and `extra`.
    fn stitched_text(layers: &str, extra: &str) -> String {
        format!(
            r#"(kicad_pcb
              (layers {layers})
              (net 0 "") (net 1 "GND") (net 2 "SIG") (net 3 "VSSA")
              (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
              {extra}
              (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20))))
              (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))
            "#
        )
    }

    fn stitched_board(extra: &str) -> PcbBoard {
        PcbBoard::parse(&stitched_text(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#,
            extra,
        ))
        .expect("board must parse")
    }

    fn at_mm(via: &TieVia) -> (f64, f64) {
        (via.at.x_mm(), via.at.y_mm())
    }

    #[test]
    fn a_layer_change_without_a_ground_via_gets_one_beside_it() {
        let vias = stitch_vias(&stitched_board(""), REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let at = at_mm(&vias[0]);
        assert_eq!(vias[0].net, "GND");
        assert!((at.0 - 10.0).abs() <= REACH && (at.1 - 10.0).abs() <= REACH);
        assert!(
            (at.0 - 10.0).hypot(at.1 - 10.0) >= 0.8,
            "clear of the signal via: {at:?}"
        );
    }

    /// A 0.6 mm via needs 0.8 mm between centres, so inside a window of 0.64 mm
    /// along either axis the only room is out on the diagonal, past the last
    /// grid step. The thin 4-layer stackup the buck board declares has exactly
    /// that window.
    #[test]
    fn a_tight_window_still_gets_a_return_via_on_the_diagonal() {
        const TIGHT: f64 = 0.64;
        let vias = stitch_vias(&stitched_board(""), TIGHT, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let at = at_mm(&vias[0]);
        assert!(
            (at.0 - 10.0).abs() <= TIGHT && (at.1 - 10.0).abs() <= TIGHT,
            "{at:?}"
        );
        assert!((at.0 - 10.0).hypot(at.1 - 10.0) >= 0.8, "{at:?}");
    }

    /// The return-path rule is advisory in the DRC because a missing return via
    /// only matters to a fast edge. A slow net with no room for one is listed for
    /// the review; a net that rides a fast edge still holds the board back.
    #[test]
    fn a_slow_net_with_no_room_for_a_return_via_is_an_advisory_and_a_critical_net_is_not() {
        let blocker = r#"(footprint "R" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 6 6) (layers "F.Cu") (net 3)))"#;
        let board = stitched_board(blocker);

        let slow = Ground::default().with_critical(&["CLK".to_string()]);
        let plan = plan_all_with(&board, REACH, &slow);
        assert!(plan.unresolved.is_empty(), "{plan:?}");
        assert_eq!(plan.advisory.len(), 1, "{plan:?}");
        assert!(plan.advisory[0].contains("net SIG"), "{plan:?}");

        let fast = Ground::default().with_critical(&["SIG".to_string()]);
        let plan = plan_all_with(&board, REACH, &fast);
        assert!(plan.advisory.is_empty(), "{plan:?}");
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");

        let strict = plan_all_with(&board, REACH, &Ground::default());
        assert_eq!(
            strict.unresolved.len(),
            1,
            "no list means every net: {strict:?}"
        );
    }

    /// A signal via that cannot be given a return must be reported, not skipped:
    /// W-SYNTH-DRC-016 is a warning, so a silent skip would let the candidate be
    /// promoted with the return path still missing.
    #[test]
    fn a_signal_via_with_no_legal_spot_for_a_return_is_reported() {
        // A 6 mm pad round the via covers every spot within the reach.
        let blocker = r#"(footprint "R" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 6 6) (layers "F.Cu") (net 3)))"#;
        let board = stitched_board(blocker);
        let plan = plan_all(&board, REACH);
        assert!(plan.vias.is_empty(), "{plan:?}");
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");
        assert!(
            plan.unresolved[0].contains("no ground return via"),
            "{plan:?}"
        );
        // The free board resolves it, so the finding is the blockage's.
        assert!(plan_all(&stitched_board(""), REACH).unresolved.is_empty());
    }

    /// A ground net the DRC knows by topology, not by name, is ground here too.
    #[test]
    fn a_ground_net_named_for_neither_gnd_nor_vss_is_ground_when_listed() {
        let text = stitched_text(r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#, "")
            .replace("\"GND\"", "\"POWER_RETURN\"");
        let board = PcbBoard::parse(&text).expect("board must parse");
        assert!(
            stitch_vias(&board, REACH, &[]).is_empty(),
            "by name alone nothing here is ground"
        );
        let ground = Ground::new(&["POWER_RETURN".to_string()]);
        let plan = plan_all_with(&board, REACH, &ground);
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        assert_eq!(plan.vias[0].net, "POWER_RETURN");
    }

    #[test]
    fn a_layer_change_with_a_ground_via_near_it_gets_nothing() {
        let near = r#"(via (at 11 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#;
        assert!(stitch_vias(&stitched_board(near), REACH, &[]).is_empty());
    }

    #[test]
    fn two_signal_vias_share_one_return_via() {
        let second = r#"(via (at 10.5 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))"#;
        assert_eq!(stitch_vias(&stitched_board(second), REACH, &[]).len(), 1);
    }

    #[test]
    fn no_return_via_is_added_where_one_layer_has_no_pour() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("parses");
        assert!(stitch_vias(&board, REACH, &[]).is_empty());
    }

    /// A pad sits where the nearest return via would go. Pad copper in the pour
    /// is not an obstacle the fill knows about, so the pass keeps clear of it.
    #[test]
    fn a_return_via_keeps_clear_of_pad_copper() {
        // Pad copper (10.3..11.1, 9.6..10.4), so no via closer than 0.5 mm to it.
        let pad = r#"(footprint "R" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10.7 10) (size 0.8 0.8) (layers "F.Cu") (net 1)))"#;
        let vias = stitch_vias(&stitched_board(pad), REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let (x, y) = at_mm(&vias[0]);
        let (dx, dy) = (
            (10.3 - x).max(x - 11.1).max(0.0),
            (9.6 - y).max(y - 10.4).max(0.0),
        );
        assert!(dx.hypot(dy) >= 0.5 - 1e-6, "on or beside the pad: {x} {y}");
    }

    /// The via drills every layer, so a track of another net on an inner layer
    /// counts, though no pour is cut round it there.
    #[test]
    fn a_return_via_keeps_clear_of_a_track_on_an_inner_layer() {
        let track =
            r#"(segment (start 10.8 9) (end 10.8 11) (width 0.2) (layer "In1.Cu") (net 2))"#;
        let text = stitched_text(
            r#"(0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal)"#,
            track,
        );
        let board = PcbBoard::parse(&text).expect("parses");
        let vias = stitch_vias(&board, REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let (x, y) = at_mm(&vias[0]);
        let along = ((y - 9.0) / 2.0).clamp(0.0, 1.0);
        let nearest = (10.8, 9.0 + 2.0 * along);
        // Via radius 0.3, gap 0.2, half the track's width 0.1.
        assert!(
            (x - nearest.0).hypot(y - nearest.1) >= 0.6 - 1e-6,
            "too close to the inner track: {x} {y}"
        );
    }

    /// The DRC calls any net named for ground a ground: `VSSA` is one, though the
    /// pass once matched `vss` exactly.
    #[test]
    fn a_ground_by_the_drcs_name_rule_counts_as_a_return() {
        let vssa = r#"(via (at 10.5 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 3))"#;
        assert!(stitch_vias(&stitched_board(vssa), REACH, &[]).is_empty());
    }

    /// The rule does not see a via with no net, so it is not given a return.
    #[test]
    fn a_via_with_no_net_is_not_given_a_return() {
        let text =
            stitched_text(r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#, "").replace("(net 2))", ")");
        let board = PcbBoard::parse(&text).expect("parses");
        assert!(board.vias.iter().all(|via| via.net.is_none()));
        assert!(stitch_vias(&board, REACH, &[]).is_empty());
    }

    /// The window is the stackup's thickest dielectric, as the rule takes it: a
    /// ground via 1.2 mm away is a return on a 1.51 mm board and not on a 1.0 mm
    /// one.
    #[test]
    fn the_reach_follows_the_stackups_thickest_dielectric() {
        let stackup = |thickness: &str| {
            format!(
                r#"(stackup (layer "F.Cu" (type "copper") (thickness 0.035))
                    (layer "dielectric 1" (type "core") (thickness {thickness}))
                    (layer "B.Cu" (type "copper") (thickness 0.035)))"#
            )
        };
        assert!((dielectric_reach_mm(&stackup("1.51"), 2) - 1.51).abs() < 1e-9);
        assert!((dielectric_reach_mm(&stackup("1.0"), 2) - 1.0).abs() < 1e-9);
        let jlc = r#"(stackup (layer "F.Cu" (type "copper") (thickness 0.035))
            (layer "dielectric 1" (type "prepreg") (thickness 0.2104))
            (layer "In1.Cu" (type "copper") (thickness 0.0152))
            (layer "dielectric 2" (type "core") (thickness 1.065))
            (layer "In2.Cu" (type "copper") (thickness 0.0152))
            (layer "dielectric 3" (type "prepreg") (thickness 0.2104))
            (layer "B.Cu" (type "copper") (thickness 0.035)))"#;
        assert!((dielectric_reach_mm(jlc, 4) - 0.6377).abs() < 1e-9);
        // No stackup: the rule falls back to the default profile of the layer count,
        // whose widest height is half of the prepreg and core on a four-layer board.
        assert!((dielectric_reach_mm("(kicad_pcb)", 4) - 0.6377).abs() < 1e-9);
        let ground = r#"(via (at 11.2 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#;
        let board = stitched_board(ground);
        assert!(
            stitch_vias(&board, 1.51, &[]).is_empty(),
            "1.2 mm is inside 1.51"
        );
        assert_eq!(
            stitch_vias(&board, 1.0, &[]).len(),
            1,
            "1.2 mm is outside 1.0"
        );
    }

    /// Island ties and return vias are planned from one fill, each knowing the
    /// other: a tie via is a ground via for the signal vias beside it.
    #[test]
    fn a_planned_tie_via_serves_as_the_return_for_a_signal_via_beside_it() {
        let signal = r#"(via (at 30 9.6) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))"#;
        let board = board(signal);
        let tie = plan_ties(&board);
        assert_eq!(tie.vias.len(), 1, "{tie:?}");
        let tie_at = at_mm(&tie.vias[0]);
        let all = plan_all(&board, REACH);
        let near_signal = (tie_at.0 - 30.0).abs() <= REACH && (tie_at.1 - 9.6).abs() <= REACH;
        if near_signal {
            assert_eq!(all.vias.len(), 1, "the tie via is the return: {all:?}");
        } else {
            for pair in all.vias.windows(2) {
                let d = (at_mm(&pair[0]).0 - at_mm(&pair[1]).0)
                    .hypot(at_mm(&pair[0]).1 - at_mm(&pair[1]).1);
                assert!(d >= 0.8, "planned vias must not overlap: {d}");
            }
        }
    }

    fn scratch_file(name: &str, text: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "synth_island_{name}_{}.kicad_pcb",
            std::process::id()
        ));
        std::fs::write(&path, text).expect("scratch board");
        path
    }

    /// Each round writes the candidate, so a failure after the first leaves its
    /// vias in a file nobody checked. The candidate is put back as it was.
    #[test]
    fn a_failure_in_a_later_round_puts_the_candidate_back_as_it_was() {
        let text = board_text("");
        let path = scratch_file("restore", &text);
        let mut rounds = 0;
        let result = tie_islands_with(&path, &Ground::default(), &mut |_| {
            rounds += 1;
            if rounds == 1 {
                Ok(Some(text.clone()))
            } else {
                Err("the fill failed".to_string())
            }
        });
        assert_eq!(result, Err("the fill failed".to_string()));
        assert_eq!(
            rounds, 2,
            "the first round ran and wrote, the second failed"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            text,
            "the first round's vias must not survive a later failure"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The rounds are spent on island vias; a signal via that has no legal
    /// return via must still be in the outcome, not dropped with the planning.
    #[test]
    fn an_unresolved_return_is_reported_when_the_repair_rounds_are_spent() {
        // A 6 mm pad round the signal via leaves no spot for its return.
        let extra = r#"(net 3 "BLK")
            (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
            (footprint "R" (at 0 0) (layer "F.Cu")
              (pad "1" smd rect (at 10 10) (size 6 6) (layers "F.Cu") (net 3)))"#;
        let text = board_text(extra);
        let path = scratch_file("spent", &text);
        // The fill never shows the vias written, so the island stays and every
        // round is used up.
        let outcome = tie_islands_with(&path, &Ground::default(), &mut |_| Ok(Some(text.clone())))
            .expect("the repair runs");
        assert!(
            outcome
                .unresolved
                .iter()
                .any(|u| u.contains("still untied after")),
            "{outcome:?}"
        );
        assert!(
            outcome
                .unresolved
                .iter()
                .any(|u| u.contains("no ground return via")),
            "the unresolved return path was dropped: {outcome:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A clean board costs one fill; one that needs vias costs the fill that
    /// finds them and the fill that confirms nothing is left.
    #[test]
    fn a_clean_board_is_filled_once_and_a_board_with_an_island_twice() {
        let clean = stitched_text(r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#, "").replace(
            r#"(via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))"#,
            "",
        );
        let path = scratch_file("clean", &clean);
        let mut fills = 0;
        let outcome = tie_islands_with(&path, &Ground::default(), &mut |_| {
            fills += 1;
            Ok(Some(clean.clone()))
        })
        .expect("a clean board passes");
        assert_eq!((fills, outcome.added), (1, 0));
        let _ = std::fs::remove_file(&path);

        let text = board_text("");
        let path = scratch_file("island", &text);
        let mut fills = 0;
        let outcome = tie_islands_with(&path, &Ground::default(), &mut |path| {
            fills += 1;
            // The first fill is the island's; the second is of the tied board.
            let now = std::fs::read_to_string(path).expect("read");
            Ok(Some(if fills == 1 { text.clone() } else { now }))
        })
        .expect("ties");
        assert!(outcome.added >= 1, "{outcome:?}");
        assert!(fills <= 3, "not a fill per pass: {fills}");
        let _ = std::fs::remove_file(&path);
    }

    /// A quote or backslash in a net name must not end the string early.
    #[test]
    fn a_net_name_with_a_quote_or_backslash_is_escaped() {
        let text = r#"(kicad_pcb (layers (0 "F.Cu" signal) (2 "B.Cu" signal)))"#;
        let board = PcbBoard::parse(text).expect("parses");
        let via = TieVia {
            net: "GN\"D\\X".to_string(),
            at: Point::new(mm_to_nm(1.0), mm_to_nm(2.0)),
            size_nm: 600_000,
            drill_nm: 300_000,
        };
        let tied = with_vias(text, &board, &[via]).expect("tied");
        assert!(tied.contains(r#"(net "GN\"D\\X")"#), "{tied}");
    }

    /// A large pour with a many-vertex outline is searched coarse to fine, not
    /// at the finest step over the whole island: a guard against the scan that
    /// took billions of segment checks.
    #[test]
    fn a_large_island_is_searched_in_reasonable_time() {
        let ring = |cx: f64, cy: f64, r: f64, n: usize| {
            (0..n)
                .map(|i| {
                    let t = std::f64::consts::TAU * f64::from(u32::try_from(i).unwrap())
                        / f64::from(u32::try_from(n).unwrap());
                    format!("(xy {:.3} {:.3})", cx + r * t.cos(), cy + r * t.sin())
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let text = format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
              (net 0 "") (net 1 "GND")
              (segment (start 100 50) (end 101 50) (width 0.25) (layer "F.Cu") (net 1))
              (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
              (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 300 0) (xy 300 100) (xy 0 100)))
                (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 100) (xy 0 100)))
                (filled_polygon (layer "F.Cu") (pts {})))
              (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 300 0) (xy 300 100) (xy 0 100)))
                (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 300 0) (xy 300 100) (xy 0 100)))))"#,
            ring(100.0, 50.0, 45.0, 4000)
        );
        let board = PcbBoard::parse(&text).expect("parses");
        let started = std::time::Instant::now();
        let plan = plan_ties(&board);
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "the search took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_written_via_parses_back_on_the_net_and_both_layers() {
        let board = board("");
        let vias = islands_to_tie(&board);
        let text = "(kicad_pcb\n\t(layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal))\n\t(net 0 \"\")\n\t(net 1 \"GND\")\n)";
        let tied = with_vias(text, &board, &vias).expect("tied");
        let back = PcbBoard::parse(&tied).expect("parses");
        assert_eq!(back.vias.len(), 1);
        assert_eq!(back.vias[0].net, Some(1));
        assert_eq!(back.vias[0].layers, ["F.Cu", "B.Cu"]);
        assert_eq!(
            with_vias(text, &board, &vias).expect("again"),
            tied,
            "the same island must give the same board"
        );
    }
}
