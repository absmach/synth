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

use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// A pad this close to a neighbour in its own footprint (centre to centre) is
/// part of a fine-pitch row: a QFN or connector breakout, where the pads leave
/// no room for a ground via between a signal via and the part.
const FINE_PITCH_MM: f64 = 0.5;

/// How far from a fine-pitch pad a signal via still counts as in its breakout.
const FINE_PITCH_BREAKOUT_MM: f64 = 2.5;

/// The return via tried when the usual one has no room: a smaller pad and drill
/// held off other copper by the board's own clearance, not the 0.2 mm margin.
const COMPACT_VIA_SIZE_NM: i64 = 460_000;
const COMPACT_VIA_DRILL_NM: i64 = 200_000;
const COMPACT_GAP_MM: f64 = 0.13;

/// Spacing of the points tried for a return via.
const RETURN_STEP_MM: f64 = 0.1;

/// How far a return via may be from a signal via when the board declares no
/// stackup: W-SYNTH-DRC-016's own window on a 1.6 mm board.
const DEFAULT_REACH_MM: f64 = 1.53;

/// Kept off the edge of W-SYNTH-DRC-016's window, so a via on the edge is not
/// lost to rounding.
const REACH_MARGIN_MM: f64 = 0.05;

/// The least copper to board edge the manufacturers ask, in mm, where the board
/// states no rule of its own.
const DEFAULT_EDGE_CLEARANCE_MM: f64 = 0.3;

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
        let declared = declared_vias(candidate, &board);
        let plan = plan_all_with(
            &board,
            return_via_window_mm(&text, &board),
            ground,
            &declared,
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
    let declared = declared_vias(candidate, &board);
    let islands = plan_ties_and_pads(&board, ground, &declared).vias.len();
    let plan = plan_all_with(
        &board,
        return_via_window_mm(&text, &board),
        ground,
        &declared,
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

    /// Whether a track from `a` to `b`, `half` wide each side, has copper in
    /// the polygon: an end inside it, or the track's body reaching an edge. A
    /// track can cross a thin pour with both ends outside and still be joined
    /// to it.
    fn touches_segment(&self, a: (f64, f64), b: (f64, f64), half: f64) -> bool {
        if a.0.min(b.0) - half > self.max.0
            || a.0.max(b.0) + half < self.min.0
            || a.1.min(b.1) - half > self.max.1
            || a.1.max(b.1) + half < self.min.1
        {
            return false;
        }
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let mut j = self.ring.len() - 1;
        for i in 0..self.ring.len() {
            if segment_distance(a, b, self.ring[j], self.ring[i]) <= half {
                return true;
            }
            j = i;
        }
        false
    }

    /// Whether copper of `radius` round `centre` has any of the polygon: the
    /// centre inside it, or the disc reaching an edge. A via whose centre is
    /// outside a pour still joins it when its pad overlaps the pour's edge.
    fn touches_disc(&self, centre: (f64, f64), radius: f64) -> bool {
        if centre.0 + radius < self.min.0
            || centre.0 - radius > self.max.0
            || centre.1 + radius < self.min.1
            || centre.1 - radius > self.max.1
        {
            return false;
        }
        self.contains(centre) || self.edge_distance(centre) <= radius
    }

    /// Whether the pad's copper has any of the polygon: the polygon holding the
    /// pad's centre or one of its own corners, or one of the polygon's edges
    /// reaching the pad.
    fn touches_pad(&self, pad: &PadBox) -> bool {
        let (low, high) = pad.bounds();
        if high.0 < self.min.0 || low.0 > self.max.0 || high.1 < self.min.1 || low.1 > self.max.1 {
            return false;
        }
        if self.contains(pad.at) {
            return true;
        }
        let mut j = self.ring.len() - 1;
        for i in 0..self.ring.len() {
            if pad.segment_distance(self.ring[j], self.ring[i]) <= 0.0 {
                return true;
            }
            j = i;
        }
        false
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

/// Whether segments `p1p2` and `q1q2` cross.
fn segments_cross(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
    let side = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let (d1, d2) = (side(q1, q2, p1), side(q1, q2, p2));
    let (d3, d4) = (side(p1, p2, q1), side(p1, p2, q2));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// The shortest distance between two segments; zero when they cross.
fn segment_distance(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> f64 {
    if segments_cross(p1, p2, q1, q2) {
        return 0.0;
    }
    point_segment(p1, q1, q2)
        .min(point_segment(p2, q1, q2))
        .min(point_segment(q1, p1, p2))
        .min(point_segment(q2, p1, p2))
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
    plan_ties(board, &ViaDeclarations::new()).vias
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
pub fn plan_ties(board: &PcbBoard, declared: &ViaDeclarations) -> TiePlan {
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

        let (size_nm, drill_nm) = usual_via(
            &via_sizes,
            declared
                .via(net.as_str())
                .unwrap_or((DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM)),
        );
        let need = nm_to_mm(size_nm) / 2.0 + EDGE_SLACK_MM;
        let keepout = Keepout::from_board(board, net, declared.gap_mm());
        let compact_keepout =
            Keepout::from_board(board, net, declared.clearance_mm.unwrap_or(COMPACT_GAP_MM));

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
            let usual = best_spot(regions, &members, &mains, need, &keepout)
                .map(|at| TieVia { at, ..via.clone() });
            let tie = usual.or_else(|| {
                let (compact_size, compact_drill) = declared.compact_via();
                if size_nm <= compact_size {
                    return None;
                }
                let compact_need = nm_to_mm(compact_size) / 2.0 + EDGE_SLACK_MM;
                best_spot(regions, &members, &mains, compact_need, &compact_keepout).map(|at| {
                    TieVia {
                        at,
                        size_nm: compact_size,
                        drill_nm: compact_drill,
                        ..via.clone()
                    }
                })
            });
            if let Some(tie) = tie {
                plan.vias.push(tie);
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

/// The island ties plus the vias for pads the router left joined to nothing.
fn plan_ties_and_pads(board: &PcbBoard, ground: &Ground, declared: &ViaDeclarations) -> TiePlan {
    let mut plan = plan_ties(board, declared);
    if let Some(in_pad) = ground.in_pad_via {
        let pads = plan_pad_ties(board, in_pad, &ground.underpad, declared);
        plan.vias.extend(pads.vias);
        plan.unresolved.extend(pads.unresolved);
    }
    plan
}

/// A via in the pad of a plane net's SMD pad that nothing reaches.
///
/// A net poured on an inner layer reaches its pads through vias. The router
/// can leave one pad of such a net with no via and no track: it sits on the
/// outer layer, where the net has no pour, and the plane never sees it. The
/// connectivity check then reports the pad as an island of one against the
/// rest of the net. The plane via is what the pad needs, so it is put in the
/// pad, at the land the process gives a filled and capped via, when the pad
/// holds it and no other net's copper is within the gap of it.
///
/// A pad that a track or via of its net already touches is not judged here:
/// that copper leads somewhere, and whether it reaches the plane is the
/// connectivity check's question.
#[allow(clippy::cast_precision_loss)]
fn plan_pad_ties(
    board: &PcbBoard,
    in_pad: (i64, i64),
    underpad: &BTreeSet<String>,
    declared: &ViaDeclarations,
) -> TiePlan {
    let (size_nm, drill_nm) = in_pad;
    let radius = nm_to_mm(size_nm) / 2.0;
    // The in-pad via's own gap, as the return via keeps: its ring is the
    // filled and capped land's, not two annular rings.
    let gap = declared.return_gap_mm(size_nm, drill_nm);
    let mut poured: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for zone in &board.zones {
        if let Some(net) = net_of(board, zone.net, zone.net_name.as_ref()) {
            poured
                .entry(net)
                .or_default()
                .extend(zone.layers.iter().cloned());
        }
    }
    let mut plan = TiePlan::default();
    let mut keepouts: HashMap<String, Keepout> = HashMap::new();
    // Only the pads of a component that declares the under-pad escape: a via of
    // this land elsewhere would be judged against the plain annular floor.
    for footprint in board
        .footprints
        .iter()
        .filter(|f| underpad.contains(&f.reference))
    {
        for pad in &footprint.pads {
            if pad.kind != "smd" {
                continue;
            }
            let mut copper = pad.layers.iter().filter(|l| l.ends_with(".Cu"));
            let (Some(layer), None) = (copper.next(), copper.next()) else {
                continue;
            };
            let Some(net) = net_of(board, pad.net, pad.net_name.as_ref()) else {
                continue;
            };
            // A net with no pour has no plane to reach, and one poured on the
            // pad's own layer takes the pad into the pour.
            let Some(layers) = poured.get(&net) else {
                continue;
            };
            if layers.contains(layer) {
                continue;
            }
            let own = |code: Option<u32>, name: Option<&String>| {
                net_of(board, code, name).as_deref() == Some(net.as_str())
            };
            let touched = board.vias.iter().any(|via| {
                own(via.net, via.net_name.as_ref())
                    && pad.signed_distance_nm(via.at) <= (via.size_nm / 2) as f64
            }) || board.segments.iter().any(|segment| {
                own(segment.net, segment.net_name.as_ref())
                    && &segment.layer == layer
                    && [segment.start, segment.end]
                        .iter()
                        .any(|end| pad.signed_distance_nm(*end) <= (segment.width_nm / 2) as f64)
            });
            if touched {
                continue;
            }
            let at = (pad.at.x_mm(), pad.at.y_mm());
            if pad.size_nm.0.min(pad.size_nm.1) < size_nm {
                plan.unresolved.push(format!(
                    "pad {}.{} of net {net} at ({:.2}, {:.2}) mm is joined to nothing, and a {} mm via does not fit in it",
                    footprint.reference,
                    pad.number,
                    at.0,
                    at.1,
                    nm_to_mm(size_nm),
                ));
                continue;
            }
            // The same obstacles a return via keeps off: foreign pads, tracks and
            // vias, and a rule area that refuses vias. The net's own pads are not
            // obstacles, since the via sits in a pad of its own net.
            let others_clear = keepouts
                .entry(net.clone())
                .or_insert_with(|| Keepout::from_board_in_pad(board, &net, gap))
                .clear(at, radius);
            if others_clear {
                plan.vias.push(TieVia {
                    net: net.clone(),
                    at: pad.at,
                    size_nm,
                    drill_nm,
                });
            } else {
                plan.unresolved.push(format!(
                    "pad {}.{} of net {net} at ({:.2}, {:.2}) mm is joined to nothing, and other copper is within {gap} mm of a {} mm via in it",
                    footprint.reference,
                    pad.number,
                    at.0,
                    at.1,
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
    plan_all_with(board, reach_mm, &Ground::default(), &ViaDeclarations::new())
}

/// [`plan_all`] with the nets the design's DRC calls ground.
#[must_use]
pub fn plan_all_with(
    board: &PcbBoard,
    reach_mm: f64,
    ground: &Ground,
    declared: &ViaDeclarations,
) -> TiePlan {
    let mut plan = plan_ties_and_pads(board, ground, declared);
    let stitched = stitch(board, reach_mm, &plan.vias, ground, declared);
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
    /// The `(size, drill)` of a filled and capped via in a pad, in nanometres,
    /// where the process offers one. `None` leaves a pad with no way to the
    /// plane as it is.
    in_pad_via: Option<(i64, i64)>,
    /// The components that declare `escape { method underpad }`: the only places
    /// a filled and capped via in a pad is declared, which is where the DRC and
    /// the `.kicad_dru` exempt its ring. A via in a pad of any other component
    /// is not one the process was asked for.
    underpad: BTreeSet<String>,
    /// The nets that declare a medium edge: they keep the return-via requirement
    /// but look for the via in the wider window of `synth_drc::MEDIUM_EDGE_WINDOW_NM`.
    medium: BTreeSet<String>,
    /// Opt-in: a missing fast-edge return via in a fine-pitch pad breakout is
    /// an advisory, not blocking. Needs SI-review sign-off; off by default.
    fine_pitch_advisory: bool,
}

impl Ground {
    #[must_use]
    pub fn new(names: &[String]) -> Self {
        Self {
            named: names.iter().cloned().collect(),
            critical: None,
            in_pad_via: None,
            underpad: BTreeSet::new(),
            medium: BTreeSet::new(),
            fine_pitch_advisory: false,
        }
    }

    /// Nets that declare `edge_rate medium`: their return via may sit in the wider
    /// window of a medium edge.
    #[must_use]
    pub fn with_medium(mut self, medium: &[String]) -> Self {
        self.medium = medium.iter().cloned().collect();
        self
    }

    /// The reach in which `net` looks for a return via, and the search window inside
    /// it: the stackup's `reach_mm`, or the medium-edge window where that is wider
    /// and the net declares one.
    fn reach_and_window(&self, net: &str, reach_mm: f64) -> (f64, f64) {
        let reach = if self.medium.contains(net) {
            reach_mm.max(nm_to_mm(synth_drc::MEDIUM_EDGE_WINDOW_NM))
        } else {
            reach_mm
        };
        (reach, (reach - REACH_MARGIN_MM).max(0.0))
    }

    /// A pad of a poured net that nothing reaches gets a via of this
    /// `(size, drill)` in the pad, the way the process fills and caps one.
    #[must_use]
    pub fn with_in_pad_via(mut self, size_nm: i64, drill_nm: i64) -> Self {
        self.in_pad_via = Some((size_nm, drill_nm));
        self
    }

    /// The components (by reference) that declare an under-pad escape: a via in
    /// a pad is put only in the pad of one of these.
    #[must_use]
    pub fn with_underpad(mut self, references: &[String]) -> Self {
        self.underpad = references.iter().cloned().collect();
        self
    }

    #[must_use]
    pub fn with_fine_pitch_advisory(mut self, on: bool) -> Self {
        self.fine_pitch_advisory = on;
        self
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

    /// The pad's axis-aligned bounds `(min, max)`.
    fn bounds(&self) -> ((f64, f64), (f64, f64)) {
        let reach = (
            self.half.0 * self.cos.abs() + self.half.1 * self.sin.abs(),
            self.half.0 * self.sin.abs() + self.half.1 * self.cos.abs(),
        );
        (
            (self.at.0 - reach.0, self.at.1 - reach.1),
            (self.at.0 + reach.0, self.at.1 + reach.1),
        )
    }

    /// The point `p` in the pad's own axes, with the pad centred on the origin.
    fn local(&self, p: (f64, f64)) -> (f64, f64) {
        let (dx, dy) = (p.0 - self.at.0, p.1 - self.at.1);
        (
            dx * self.cos + dy * self.sin,
            -dx * self.sin + dy * self.cos,
        )
    }

    /// The shortest distance between the pad's rectangle and the segment from
    /// `a` to `b`, zero when they touch or cross. Exact: the segment is taken
    /// into the pad's axes, where the pad is a box, and is either clipped by it
    /// or nearest to it at one of its ends or one of the pad's corners.
    fn segment_distance(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        let (a, b) = (self.local(a), self.local(b));
        let (hx, hy) = self.half;
        // Liang-Barsky clip of the segment to the box.
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let mut clipped = true;
        for (p, q) in [
            (-dx, a.0 + hx),
            (dx, hx - a.0),
            (-dy, a.1 + hy),
            (dy, hy - a.1),
        ] {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    clipped = false;
                    break;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
                if t0 > t1 {
                    clipped = false;
                    break;
                }
            }
        }
        if clipped {
            return 0.0;
        }
        let to_box = |p: (f64, f64)| (p.0.abs() - hx).max(0.0).hypot((p.1.abs() - hy).max(0.0));
        let corners = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)];
        corners
            .iter()
            .map(|&c| point_segment(c, a, b))
            .chain([to_box(a), to_box(b)])
            .fold(f64::MAX, f64::min)
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
    /// The outline of each rule area that keeps vias out.
    no_via_areas: Vec<Region>,
    /// The copper a via keeps between its pad and every obstacle.
    gap_mm: f64,
}

impl Keepout {
    /// The obstacles on `board` for a via of net `own`. Tracks of `own` are not
    /// obstacles, since a via may sit on its own net's copper.
    fn from_board(board: &PcbBoard, own: &str, gap_mm: f64) -> Self {
        Self::build(board, own, gap_mm, false)
    }

    /// [`Self::from_board`] without the pads of `own`: a via of `own` filled and
    /// capped in a pad of its own net touches no foreign copper there.
    fn from_board_in_pad(board: &PcbBoard, own: &str, gap_mm: f64) -> Self {
        Self::build(board, own, gap_mm, true)
    }

    fn build(board: &PcbBoard, own: &str, gap_mm: f64, skip_own_pads: bool) -> Self {
        let mut pads = Vec::new();
        for footprint in &board.footprints {
            for pad in &footprint.pads {
                let own_pad = net_of(board, pad.net, pad.net_name.as_ref()).as_deref() == Some(own);
                if !(skip_own_pads && own_pad) {
                    pads.push(PadBox::new(pad));
                }
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
        let mut no_via_areas: Vec<Region> = Vec::new();
        let mut seen: BTreeSet<Vec<(i64, i64)>> = BTreeSet::new();
        for zone in board.zones.iter().filter(|zone| zone.keeps_vias_out) {
            for (_, outline) in &zone.outlines {
                // A zone on several layers repeats its outline once per layer.
                let key: Vec<(i64, i64)> = outline.iter().map(|p| (p.x_nm, p.y_nm)).collect();
                if seen.insert(key) {
                    no_via_areas.extend(Region::new("", outline));
                }
            }
        }
        Self {
            pads,
            tracks,
            vias,
            no_via_areas,
            gap_mm,
        }
    }

    /// Whether a via of `radius` mm at `p` keeps the gap of copper from every
    /// obstacle.
    fn clear(&self, p: (f64, f64), radius: f64) -> bool {
        let need = radius + self.gap_mm;
        self.pads.iter().all(|pad| pad.distance(p) >= need - EPS)
            && self
                .tracks
                .iter()
                .all(|(a, b, half)| point_segment(p, *a, *b) >= need + half - EPS)
            && self
                .vias
                .iter()
                .all(|(at, other)| (at.0 - p.0).hypot(at.1 - p.1) >= need + other - EPS)
            && self
                .no_via_areas
                .iter()
                .all(|area| !area.contains(p) && area.edge_distance(p) >= radius - EPS)
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
    let mut via_sizes: Vec<(i64, i64)> = Vec::new();
    for via in &board.vias {
        if net_of(board, via.net, via.net_name.as_ref()).as_deref() != Some(net) {
            continue;
        }
        via_sizes.push((via.size_nm, via.drill_nm));
        let (at, radius) = ((via.at.x_mm(), via.at.y_mm()), nm_to_mm(via.size_nm) / 2.0);
        let hit: Vec<usize> = via
            .layers
            .iter()
            .flat_map(|layer| {
                (0..regions.len())
                    .filter(|&i| regions[i].layer == *layer && regions[i].touches_disc(at, radius))
            })
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
        let shape = PadBox::new(pad);
        let hit: Vec<usize> = layers
            .iter()
            .flat_map(|layer| {
                (0..regions.len())
                    .filter(|&i| regions[i].layer == **layer && regions[i].touches_pad(&shape))
            })
            .collect();
        join(&mut groups, &mut attached, &hit);
    }
    for segment in &board.segments {
        if net_of(board, segment.net, segment.net_name.as_ref()).as_deref() != Some(net) {
            continue;
        }
        let (a, b) = (
            (segment.start.x_mm(), segment.start.y_mm()),
            (segment.end.x_mm(), segment.end.y_mm()),
        );
        let half = nm_to_mm(segment.width_nm) / 2.0;
        let hit: Vec<usize> = (0..regions.len())
            .filter(|&i| {
                regions[i].layer == segment.layer && regions[i].touches_segment(a, b, half)
            })
            .collect();
        join(&mut groups, &mut attached, &hit);
    }
    join_through_copper(&mut groups, &pieces_of(board, net, regions, copper));
    (groups, attached, via_sizes)
}

/// One piece of a net's copper that is not a pour, with the pour regions it
/// sits in.
struct Piece {
    layers: Vec<String>,
    shape: PieceShape,
    regions: Vec<usize>,
}

enum PieceShape {
    Pad(PadBox),
    Via((f64, f64), f64),
    /// Both ends and the half width of the copper.
    Track((f64, f64), (f64, f64), f64),
}

fn pieces_of(board: &PcbBoard, net: &str, regions: &[Region], copper: &[String]) -> Vec<Piece> {
    let in_net =
        |n: Option<u32>, name: Option<&String>| net_of(board, n, name).as_deref() == Some(net);
    let mut pieces = Vec::new();
    for pad in board.pads() {
        if !in_net(pad.net, pad.net_name.as_ref()) {
            continue;
        }
        let layers: Vec<String> = if pad.kind == "thru_hole" {
            copper.to_vec()
        } else {
            pad.layers.clone()
        };
        let shape = PadBox::new(pad);
        let regions = layers
            .iter()
            .flat_map(|l| {
                (0..regions.len())
                    .filter(|&i| regions[i].layer == *l && regions[i].touches_pad(&shape))
            })
            .collect();
        pieces.push(Piece {
            layers,
            shape: PieceShape::Pad(shape),
            regions,
        });
    }
    for via in &board.vias {
        if !in_net(via.net, via.net_name.as_ref()) {
            continue;
        }
        let at = (via.at.x_mm(), via.at.y_mm());
        let radius = nm_to_mm(via.size_nm) / 2.0;
        let regions = via
            .layers
            .iter()
            .flat_map(|l| {
                (0..regions.len())
                    .filter(|&i| regions[i].layer == *l && regions[i].touches_disc(at, radius))
            })
            .collect();
        pieces.push(Piece {
            layers: via.layers.clone(),
            shape: PieceShape::Via(at, radius),
            regions,
        });
    }
    for segment in &board.segments {
        if !in_net(segment.net, segment.net_name.as_ref()) {
            continue;
        }
        let (a, b) = (
            (segment.start.x_mm(), segment.start.y_mm()),
            (segment.end.x_mm(), segment.end.y_mm()),
        );
        let half = nm_to_mm(segment.width_nm) / 2.0;
        let regions = (0..regions.len())
            .filter(|&i| {
                regions[i].layer == segment.layer && regions[i].touches_segment(a, b, half)
            })
            .collect();
        pieces.push(Piece {
            layers: vec![segment.layer.clone()],
            shape: PieceShape::Track(a, b, half),
            regions,
        });
    }
    pieces
}

/// Joins the pour regions that the net's pads, vias and tracks connect through
/// copper outside any pour: a track from a pad that sits in a sliver of pour to
/// a pad or via that reaches the main one ties the sliver to it as surely as a
/// via would. KiCad counts that as connected, so the repair must too.
fn join_through_copper(groups: &mut Groups, pieces: &[Piece]) {
    const TOUCH_MM: f64 = 0.02;
    let base = groups.0.len();
    groups.0.extend(base..base + pieces.len());
    for (i, piece) in pieces.iter().enumerate() {
        for &region in &piece.regions {
            groups.union(region, base + i);
        }
    }
    let shares_layer = |a: &Piece, b: &Piece| a.layers.iter().any(|l| b.layers.contains(l));
    // Only pieces whose boxes share a cell of a grid can touch, so a large net
    // is compared locally rather than every piece against every other.
    for (i, j) in neighbouring_pairs(pieces, TOUCH_MM) {
        let (a, b) = (&pieces[i], &pieces[j]);
        if shares_layer(a, b) && copper_gap(&a.shape, &b.shape) <= TOUCH_MM {
            groups.union(base + i, base + j);
        }
    }
}

/// The axis-aligned bounds `(min, max)` of a piece's copper.
fn piece_bounds(shape: &PieceShape) -> ((f64, f64), (f64, f64)) {
    match shape {
        PieceShape::Pad(pad) => pad.bounds(),
        PieceShape::Via(at, radius) => (
            (at.0 - radius, at.1 - radius),
            (at.0 + radius, at.1 + radius),
        ),
        PieceShape::Track(a, b, half) => (
            (a.0.min(b.0) - half, a.1.min(b.1) - half),
            (a.0.max(b.0) + half, a.1.max(b.1) + half),
        ),
    }
}

/// Every pair of pieces (by index, smaller first) whose bounds, grown by
/// `reach`, share a cell of a grid: a superset of the pairs that touch, found
/// in time that follows the copper rather than the square of the piece count.
#[allow(clippy::cast_possible_truncation)]
fn neighbouring_pairs(pieces: &[Piece], reach: f64) -> Vec<(usize, usize)> {
    const CELL_MM: f64 = 2.0;
    let cell = |v: f64| (v / CELL_MM).floor() as i64;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, piece) in pieces.iter().enumerate() {
        let (low, high) = piece_bounds(&piece.shape);
        for x in cell(low.0 - reach)..=cell(high.0 + reach) {
            for y in cell(low.1 - reach)..=cell(high.1 + reach) {
                grid.entry((x, y)).or_default().push(i);
            }
        }
    }
    let mut pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for members in grid.values() {
        for (k, &i) in members.iter().enumerate() {
            for &j in &members[k + 1..] {
                pairs.insert((i.min(j), i.max(j)));
            }
        }
    }
    pairs.into_iter().collect()
}

/// The clear distance between two pieces' copper, by their real widths; zero or
/// less when they overlap. Pads join only through a track or a via.
fn copper_gap(first: &PieceShape, second: &PieceShape) -> f64 {
    use PieceShape::{Pad, Track, Via};
    // A track against a pad box: exact, not sampled along the track.
    let track_pad = |start: (f64, f64), end: (f64, f64), half: f64, pad: &PadBox| -> f64 {
        pad.segment_distance(start, end) - half
    };
    match (first, second) {
        (Track(a_start, a_end, a_half), Track(b_start, b_end, b_half)) => {
            segment_distance(*a_start, *a_end, *b_start, *b_end) - a_half - b_half
        }
        (Track(start, end, half), Via(centre, radius))
        | (Via(centre, radius), Track(start, end, half)) => {
            point_segment(*centre, *start, *end) - half - radius
        }
        (Via(a_centre, a_radius), Via(b_centre, b_radius)) => {
            (a_centre.0 - b_centre.0).hypot(a_centre.1 - b_centre.1) - a_radius - b_radius
        }
        (Track(start, end, half), Pad(pad)) | (Pad(pad), Track(start, end, half)) => {
            track_pad(*start, *end, *half, pad)
        }
        (Via(centre, radius), Pad(pad)) | (Pad(pad), Via(centre, radius)) => {
            pad.distance(*centre) - radius
        }
        (Pad(_), Pad(_)) => f64::MAX,
    }
}

/// What the board's net classes declare, read from the `.kicad_pro` beside the
/// candidate: the via each poured net's own class takes, by net name, and the
/// widest copper clearance any class asks for. A net with no entry takes the
/// board's `Default` class, or the 0.6 / 0.3 mm fallback.
#[derive(Debug, Clone, Default)]
pub struct ViaDeclarations {
    vias: BTreeMap<String, (i64, i64)>,
    clearance_mm: Option<f64>,
    /// The least distance from a via's hole to other copper the board's DRC rules
    /// ask (`hole_clearance` in the `.kicad_dru` beside it), in mm.
    hole_clearance_mm: Option<f64>,
    /// The least distance from copper to the board edge the DRC rules ask
    /// (`edge_clearance` in the `.kicad_dru` beside it), in mm.
    edge_clearance_mm: Option<f64>,
    /// The board's own via floors (`design_settings.rules`), 0 when unread.
    min_via_nm: i64,
    min_annular_nm: i64,
    min_hole_nm: i64,
}

impl ViaDeclarations {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The copper kept between a via's land and the board outline: what the
    /// board's DRC rules ask, or the 0.3 mm the manufacturers' floor gives.
    fn edge_gap_mm(&self) -> f64 {
        self.edge_clearance_mm.unwrap_or(DEFAULT_EDGE_CLEARANCE_MM)
    }

    /// The compact return via: the fixed 0.46 / 0.2 mm, grown to the board's
    /// own minimum via diameter, annular ring and hole so KiCad DRC accepts it.
    fn compact_via(&self) -> (i64, i64) {
        let drill = COMPACT_VIA_DRILL_NM.max(self.min_hole_nm);
        let size = COMPACT_VIA_SIZE_NM
            .max(self.min_via_nm)
            .max(drill + 2 * self.min_annular_nm);
        (size, drill)
    }

    fn via(&self, net: &str) -> Option<(i64, i64)> {
        self.vias.get(net).copied()
    }

    /// The copper kept between a new via's pad and other copper: the fixed
    /// 0.2 mm, or the widest clearance a class declares when that is larger,
    /// because KiCad holds two nets apart by the larger of their classes.
    fn gap_mm(&self) -> f64 {
        self.clearance_mm.map_or(VIA_GAP_MM, |c| c.max(VIA_GAP_MM))
    }

    /// The copper kept around a return via of `size_nm` and `drill_nm`: what the
    /// board's own rules ask, not the fixed 0.2 mm. The declared class clearance
    /// governs copper to copper, and the hole clearance, less the ring between
    /// the hole and the land's edge, governs hole to copper; the larger holds.
    /// Without a declared clearance the fixed [`VIA_GAP_MM`] stands. The router's
    /// own KiCad DRC checks the result.
    fn return_gap_mm(&self, size_nm: i64, drill_nm: i64) -> f64 {
        let Some(clearance) = self.clearance_mm else {
            return VIA_GAP_MM;
        };
        let ring = nm_to_mm((size_nm - drill_nm).max(0)) / 2.0;
        let hole = self.hole_clearance_mm.map_or(0.0, |h| h - ring);
        clearance.max(hole)
    }
}

/// The via size each poured net's own class declares, read from the
/// `.kicad_pro` beside the candidate, so an island repair gives every net the
/// geometry of its own class and not that of the first ground net listed.
fn declared_vias(candidate: &Path, board: &PcbBoard) -> ViaDeclarations {
    ViaDeclarations {
        vias: pours(board)
            .keys()
            .filter_map(|net| declared_via_size(candidate, net).map(|via| (net.clone(), via)))
            .collect(),
        clearance_mm: declared_clearance_mm(candidate),
        hole_clearance_mm: declared_hole_clearance_mm(candidate),
        edge_clearance_mm: declared_edge_clearance_mm(candidate),
        ..declared_via_floors(candidate)
    }
}

/// The via diameter, annular ring and hole floors of the `.kicad_pro` beside a
/// board, in nm; 0 for any it does not state.
fn declared_via_floors(candidate: &Path) -> ViaDeclarations {
    let mut floors = ViaDeclarations::default();
    let rules = std::fs::read_to_string(candidate.with_extension("kicad_pro"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("design_settings")?.get("rules").cloned());
    let nm = |key: &str| {
        rules
            .as_ref()
            .and_then(|r| r.get(key)?.as_f64())
            .map_or(0, mm_to_nm)
    };
    floors.min_via_nm = nm("min_via_diameter");
    floors.min_annular_nm = nm("min_via_annular_width");
    floors.min_hole_nm = nm("min_through_hole_diameter");
    floors
}

/// The copper to board edge clearance the `.kicad_dru` beside a board states, in
/// mm. None when the file or the rule is missing.
fn declared_edge_clearance_mm(candidate: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(candidate.with_extension("kicad_dru")).ok()?;
    let rest = text.split("copper to board edge").nth(1)?;
    let rest = rest.split("edge_clearance (min ").nth(1)?;
    rest.split("mm").next()?.trim().parse().ok()
}

/// The hole clearance the `.kicad_dru` beside a board states, in mm. None when
/// the file or the rule is missing.
fn declared_hole_clearance_mm(candidate: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(candidate.with_extension("kicad_dru")).ok()?;
    let rest = text.split("hole_clearance (min ").nth(1)?;
    rest.split("mm").next()?.trim().parse().ok()
}

/// The widest copper clearance any net class of the `.kicad_pro` beside a
/// board declares, in mm. None when the file is missing or unreadable.
fn declared_clearance_mm(candidate: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(candidate.with_extension("kicad_pro")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("net_settings")?
        .get("classes")?
        .as_array()?
        .iter()
        .filter_map(|class| class.get("clearance").and_then(serde_json::Value::as_f64))
        .reduce(f64::max)
}

/// Read a net's class via size from the `.kicad_pro` beside a board, so an
/// island repair on a board that declared a smaller drill uses it instead of
/// the 0.6 / 0.3 mm fallback. The `Default` class answers for a net no class
/// pattern names. Returns None when the file is missing or unreadable.
fn declared_via_size(candidate: &Path, net: &str) -> Option<(i64, i64)> {
    let pro = candidate.with_extension("kicad_pro");
    let text = std::fs::read_to_string(&pro).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ns = value.get("net_settings")?;
    let classes = ns.get("classes")?.as_array()?;
    let mut class_name = "Default".to_string();
    if let Some(patterns) = ns.get("netclass_patterns").and_then(|v| v.as_array()) {
        for pat in patterns {
            if let (Some(name), Some(pattern)) = (
                pat.get("netclass").and_then(|v| v.as_str()),
                pat.get("pattern").and_then(|v| v.as_str()),
            ) {
                if net == pattern || net.contains(pattern) {
                    class_name = name.to_string();
                    break;
                }
            }
        }
    }
    for class in classes {
        if class.get("name").and_then(|v| v.as_str()) == Some(&class_name) {
            let dia = class.get("via_diameter")?.as_f64()?;
            let drill = class.get("via_drill")?.as_f64()?;
            return Some(((dia * 1_000_000.0) as i64, (drill * 1_000_000.0) as i64));
        }
    }
    for class in classes {
        if class.get("name").and_then(|v| v.as_str()) == Some("Default") {
            let dia = class.get("via_diameter")?.as_f64()?;
            let drill = class.get("via_drill")?.as_f64()?;
            return Some(((dia * 1_000_000.0) as i64, (drill * 1_000_000.0) as i64));
        }
    }
    None
}

/// The via size the net uses most, the larger on a tie. The smallest would
/// copy a router's one escalated via, which is under the board's own minimum.
fn usual_via(sizes: &[(i64, i64)], fallback: (i64, i64)) -> (i64, i64) {
    let mut counts: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    for &size in sizes {
        *counts.entry(size).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(size, count)| (count, size))
        .map_or(fallback, |(size, _)| size)
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

/// The window W-SYNTH-DRC-016 looks in for a ground return via: the stackup's
/// reach, floored at the least distance a ground via can sit from a signal via
/// of the board's widest via pad. The rule applies the same floor.
fn return_via_window_mm(text: &str, board: &PcbBoard) -> f64 {
    let pad_nm = board
        .vias
        .iter()
        .map(|via| via.size_nm)
        .max()
        .unwrap_or(DEFAULT_VIA_SIZE_NM);
    let reach_nm = mm_to_nm(dielectric_reach_mm(text, board.copper_layers().len()));
    nm_to_mm(synth_drc::return_via_window_nm(reach_nm, pad_nm))
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
/// point that lies inside the ground pour on some layer and clears
/// every pad, via and foreign track on every layer. A net named for ground is
/// ground by the DRC's own rule, so `VSSA` is. A via with no net is not seen by
/// the rule and is not given a return. A new via counts for the signal vias after
/// it, so neighbours share one.
#[must_use]
pub fn stitch_vias(board: &PcbBoard, reach_mm: f64, planned: &[TieVia]) -> Vec<TieVia> {
    stitch(
        board,
        reach_mm,
        planned,
        &Ground::default(),
        &ViaDeclarations::new(),
    )
    .vias
}

/// The corners of the box that holds every region of the fill on a layer.
type Bounds = ((f64, f64), (f64, f64));

fn fill_bounds(regions: &[Region], layer: &str) -> Bounds {
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for r in regions.iter().filter(|r| r.layer == layer) {
        lo = (lo.0.min(r.min.0), lo.1.min(r.min.1));
        hi = (hi.0.max(r.max.0), hi.1.max(r.max.1));
    }
    (lo, hi)
}

/// Whether a via of `radius` at `p` has ground fill under it on `layer`: its centre
/// is inside the fill, and its land stays inside the extent of the copper, which
/// is the board edge inset by its own clearance. The land may reach into the
/// clearance the fill keeps from foreign copper: the fill stands off by the zone's
/// clearance, the board's class clearance is smaller, and the keep-out holds the
/// via to that gap; the zone is refilled around the via. The bounds of the fill on
/// `layer` are given, so a search over thousands of points does not rescan the
/// regions for each.
fn pour_room_within(
    regions: &[Region],
    layer: &str,
    (lo, hi): Bounds,
    p: (f64, f64),
    radius: f64,
) -> bool {
    let here = || regions.iter().filter(|r| r.layer == layer);
    let reach = radius + EDGE_SLACK_MM - EPS;
    p.0 - lo.0 >= reach
        && p.1 - lo.1 >= reach
        && hi.0 - p.0 >= reach
        && hi.1 - p.1 >= reach
        && here().any(|r| r.contains(p) && r.edge_distance(p) >= EDGE_SLACK_MM - EPS)
}

/// Whether a via land of `radius` at `p` stays `edge_gap` off every `Edge.Cuts`
/// segment of the board, a notch or a cutout in the outline included. The fill's
/// own extent holds a via off the outer edge only; a wall of the outline inside
/// it counts as a gap in the fill unless the outline itself is checked, and the
/// board's copper to edge rule, not the slack, is the distance it keeps.
fn clear_of_outline(board: &PcbBoard, p: (f64, f64), radius: f64, edge_gap: f64) -> bool {
    board.edge_cuts.iter().all(|(a, b)| {
        point_segment(p, (a.x_mm(), a.y_mm()), (b.x_mm(), b.y_mm())) >= radius + edge_gap - EPS
    })
}

#[allow(clippy::too_many_lines)]
fn stitch(
    board: &PcbBoard,
    reach_mm: f64,
    planned: &[TieVia],
    ground_nets: &Ground,
    declared: &ViaDeclarations,
) -> Stitched {
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
    let (size_nm, drill_nm) = usual_via(&sizes, (DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM));
    let need = nm_to_mm(size_nm) / 2.0 + EDGE_SLACK_MM;
    let radius = nm_to_mm(size_nm) / 2.0;
    let mut ground: Vec<(f64, f64)> = board
        .vias
        .iter()
        .filter(|via| is_ground(via))
        .map(|via| (via.at.x_mm(), via.at.y_mm()))
        .collect();
    ground.extend(planned.iter().map(|via| (via.at.x_mm(), via.at.y_mm())));

    let copper = board.copper_layers();
    if copper.len() < 2 {
        return none();
    }
    let first = &copper[0];
    // Each layer's fill bounds are read once, not for every point searched.
    let bounds: Vec<(&String, Bounds)> = copper
        .iter()
        .map(|layer| (layer, fill_bounds(regions, layer)))
        .collect();
    let room = |layer: &str, p: (f64, f64)| {
        bounds
            .iter()
            .find(|(l, _)| l.as_str() == layer)
            .is_some_and(|(_, b)| pour_room_within(regions, layer, *b, p, need - EDGE_SLACK_MM))
    };
    // A return via carries the return current from one ground plane to another, so
    // it needs ground copper on two layers, whichever they are. Under a ball grid
    // the outer layer is all pads and holds no pour, while the inner planes do.
    let room_both = |p: (f64, f64)| {
        clear_of_outline(board, p, radius, declared.edge_gap_mm())
            && copper.iter().filter(|layer| room(layer, p)).count() >= 2
    };
    // Built once: the signal vias and the keep-out do not change over the loop.
    let keepout = Keepout::from_board(board, net, declared.return_gap_mm(size_nm, drill_nm));
    let signals: Vec<(f64, f64, String)> = board
        .vias
        .iter()
        .filter(|via| !is_ground(via))
        .filter_map(|via| Some((via.at.x_mm(), via.at.y_mm(), net_of_via(via)?)))
        .collect();

    let apart = nm_to_mm(size_nm) + keepout.gap_mm;
    // Where the process fills and caps a via in a pad, a pad of the ground net
    // that holds the via's land is a place for a return via too.
    let in_pad = ground_nets.in_pad_via.unwrap_or((size_nm, drill_nm));
    // The in-pad via has its own ring, so its own gap and spacing: the usual
    // via's would leave its hole too near foreign copper, or turn away a ground
    // ball at the grid's pitch that would fit.
    let in_pad_gap = declared.return_gap_mm(in_pad.0, in_pad.1);
    let in_pad_apart = nm_to_mm(in_pad.0) + in_pad_gap;
    let in_pad_keepout = Keepout::from_board_in_pad(board, net, in_pad_gap);
    let last = &copper[copper.len() - 1];
    let ground_pads = ground_nets.in_pad_via.map_or_else(Vec::new, |(land, _)| {
        ground_pad_spots(
            board,
            net,
            [first, last],
            &ground_nets.underpad,
            nm_to_mm(land),
        )
    });
    // A via in a ground pad joins a plane only where a layer other than the pad's
    // own holds ground fill under it.
    let filled_under = |pad: &GroundPad| {
        copper
            .iter()
            .any(|layer| !pad.layers.contains(layer) && room(layer, pad.at))
    };
    let fine_pads = fine_pitch_pads(board);
    let compact_gap = declared.clearance_mm.unwrap_or(COMPACT_GAP_MM);
    let compact_keepout = Keepout::from_board(board, net, compact_gap);
    let (compact_size, compact_drill) = declared.compact_via();
    let compact_apart = nm_to_mm(compact_size) + compact_gap;
    let mut out = Vec::new();
    let mut offsets_by_window: HashMap<u64, Vec<(f64, f64)>> = HashMap::new();
    let (mut unresolved, mut advisory) = (Vec::new(), Vec::new());
    for &(vx, vy, ref signal_net) in &signals {
        let (reach_mm, window) = ground_nets.reach_and_window(signal_net, reach_mm);
        if ground
            .iter()
            .any(|g| (g.0 - vx).abs() <= reach_mm && (g.1 - vy).abs() <= reach_mm)
        {
            continue;
        }
        let offsets = offsets_by_window
            .entry(window.to_bits())
            .or_insert_with(|| return_offsets(window));
        let usual = nearest_return_spot(
            (vx, vy),
            offsets,
            apart,
            &ground,
            &room_both,
            &keepout,
            radius,
        )
        .map(|p| (p, size_nm, drill_nm));
        let compact = || {
            if size_nm <= compact_size {
                return None;
            }
            nearest_return_spot(
                (vx, vy),
                offsets,
                compact_apart,
                &ground,
                &room_both,
                &compact_keepout,
                nm_to_mm(compact_size) / 2.0,
            )
            .map(|p| (p, compact_size, compact_drill))
        };
        let spot = usual.or_else(compact).or_else(|| {
            let spots = (&ground[..], &ground_pads[..]);
            in_pad_spot(
                (vx, vy),
                (window, in_pad_apart),
                spots,
                &in_pad_keepout,
                &filled_under,
                in_pad,
            )
            .map(|p| (p, in_pad.0, in_pad.1))
        });
        if let Some((p, size, drill)) = spot {
            ground.push(p);
            out.push(tie_via_at(net, p, size, drill));
        } else if ground_nets.needs_return(signal_net)
            && !(ground_nets.fine_pitch_advisory
                && fine_pads
                    .iter()
                    .any(|p| (p.0 - vx).hypot(p.1 - vy) <= FINE_PITCH_BREAKOUT_MM))
        {
            unresolved.push(missing_return((vx, vy), reach_mm, net, None));
        } else if ground_nets.needs_return(signal_net) {
            advisory.push(format!(
                "net {signal_net} changes layers at ({vx:.2}, {vy:.2}) mm in a fine-pitch pad breakout with no ground return via within {reach_mm:.2} mm and no spot in the {net} pour for one; the pads leave no room, so the loop is left as an advisory for the signal-integrity review"
            ));
        } else {
            advisory.push(missing_return((vx, vy), reach_mm, net, Some(signal_net)));
        }
    }
    Stitched {
        vias: out,
        unresolved,
        advisory,
    }
}

/// What the report says of a signal via at `at` that no ground via serves: a
/// blocking finding, or, for a net that carries no fast edge (`advisory` names
/// it), an advisory.
fn missing_return(at: (f64, f64), reach_mm: f64, net: &str, advisory: Option<&str>) -> String {
    let (x, y) = at;
    match advisory {
        None => format!(
            "the signal via at ({x:.2}, {y:.2}) mm has no ground return via within {reach_mm:.2} mm, and no spot in the {net} pour holds one clear of the pads, vias and tracks beside it"
        ),
        Some(signal_net) => format!(
            "net {signal_net} changes layers at ({x:.2}, {y:.2}) mm with no ground return via within {reach_mm:.2} mm and no spot in the {net} pour for one; the net carries no fast edge, so the loop is left as an advisory"
        ),
    }
}

/// A ground pad that can take a return via: where it is and the copper layers
/// it is on.
struct GroundPad {
    at: (f64, f64),
    layers: Vec<String>,
}

/// The pads of `net`, in components that declare an under-pad escape, on either
/// outer layer, `outer`, that hold a via land of `land_mm`. A through via reaches either side, so a ball grid on
/// the back is as good a place for a return via as one on the front.
fn ground_pad_spots(
    board: &PcbBoard,
    net: &str,
    outer: [&String; 2],
    underpad: &BTreeSet<String>,
    land_mm: f64,
) -> Vec<GroundPad> {
    board
        .footprints
        .iter()
        .filter(|footprint| underpad.contains(&footprint.reference))
        .flat_map(|footprint| &footprint.pads)
        .filter(|pad| {
            net_of(board, pad.net, pad.net_name.as_ref()).as_deref() == Some(net)
                && pad.layers.iter().any(|l| outer.contains(&l))
                && nm_to_mm(pad.size_nm.0.min(pad.size_nm.1)) >= land_mm
        })
        .map(|pad| GroundPad {
            at: (pad.at.x_mm(), pad.at.y_mm()),
            layers: pad.layers.clone(),
        })
        .collect()
}

/// A via of `net` at `p`, in millimetres.
fn tie_via_at(net: &str, p: (f64, f64), size_nm: i64, drill_nm: i64) -> TieVia {
    TieVia {
        net: net.to_string(),
        at: Point::new(mm_to_nm(p.0), mm_to_nm(p.1)),
        size_nm,
        drill_nm,
    }
}

/// Points on the board, in millimetres.
type Spots<'a> = &'a [(f64, f64)];

/// The ground pad nearest the signal via `from`, within `window`, that takes a
/// via of `in_pad` size: clear of the ground vias, and of every foreign pad,
/// track and via, and with ground fill under it on a layer other than its own, so
/// the via joins a plane there.
fn in_pad_spot(
    from: (f64, f64),
    (window, apart): (f64, f64),
    (ground, pads): (Spots<'_>, &[GroundPad]),
    keepout: &Keepout,
    filled: &dyn Fn(&GroundPad) -> bool,
    in_pad: (i64, i64),
) -> Option<(f64, f64)> {
    let radius = nm_to_mm(in_pad.0) / 2.0;
    pads.iter()
        .filter(|pad| filled(pad))
        .map(|pad| pad.at)
        .filter(|p| (p.0 - from.0).abs() <= window && (p.1 - from.1).abs() <= window)
        .filter(|p| (p.0 - from.0).hypot(p.1 - from.1) >= apart)
        .filter(|p| ground.iter().all(|g| (g.0 - p.0).hypot(g.1 - p.1) >= apart))
        .filter(|p| keepout.clear(*p, radius))
        .min_by(|a, b| {
            let da = (a.0 - from.0).hypot(a.1 - from.1);
            let db = (b.0 - from.0).hypot(b.1 - from.1);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// The centres of the pads on `board` that sit in a fine-pitch row.
fn fine_pitch_pads(board: &PcbBoard) -> Vec<(f64, f64)> {
    let mut fine = Vec::new();
    for footprint in &board.footprints {
        let centres: Vec<(f64, f64)> = footprint
            .pads
            .iter()
            .map(|pad| (pad.at.x_mm(), pad.at.y_mm()))
            .collect();
        for (i, a) in centres.iter().enumerate() {
            let crowded = centres
                .iter()
                .enumerate()
                .any(|(j, b)| j != i && (a.0 - b.0).hypot(a.1 - b.1) <= FINE_PITCH_MM + EPS);
            if crowded {
                fine.push(*a);
            }
        }
    }
    fine
}

/// The point nearest the signal via `from`, on the search grid, that is clear of
/// the ground vias, inside ground copper on any layer and clear of every
/// obstacle.
fn nearest_return_spot(
    from: (f64, f64),
    offsets: &[(f64, f64)],
    apart: f64,
    ground: &[(f64, f64)],
    room: &dyn Fn((f64, f64)) -> bool,
    keepout: &Keepout,
    radius: f64,
) -> Option<(f64, f64)> {
    offsets
        .iter()
        .map(|&(dx, dy)| (from.0 + dx, from.1 + dy))
        .find(|p| {
            ground.iter().all(|g| (g.0 - p.0).hypot(g.1 - p.1) >= apart)
                && room(*p)
                && keepout.clear(*p, radius)
        })
}

/// The search grid around a signal via, nearest first, for a window of `window`
/// millimetres: the grid, plus the window's own edges, since a via and its
/// return need `apart` between centres, which inside a narrow window is only
/// reached out on the diagonal, past the last whole grid step. The first spot
/// that fits is the answer, and a wide window holds thousands of points, so the
/// list is sorted once per window and shared by every signal via.
fn return_offsets(window: f64) -> Vec<(f64, f64)> {
    let steps = (window / RETURN_STEP_MM).floor() as i32;
    let axis: Vec<f64> = (-steps..=steps)
        .map(|i| f64::from(i) * RETURN_STEP_MM)
        .chain([-window, window])
        .collect();
    let mut offsets: Vec<(f64, f64)> = axis
        .iter()
        .flat_map(|&dx| axis.iter().map(move |&dy| (dx, dy)))
        .collect();
    offsets.sort_by(|a, b| {
        a.0.hypot(a.1)
            .partial_cmp(&b.0.hypot(b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    offsets
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

/// Moves each via that sits in a pad of its own net off that pad, taking the
/// tracks that end at it along, and returns how many it moved.
///
/// The engine puts a via at the edge of a pad now and then, and the release
/// gate refuses a via in a pad. A via that is a few tenths of a millimetre from
/// where it should be is not a reason to give up a board that otherwise routes:
/// the nearest spot that keeps the via's hole off every pad and clear of other
/// copper is found, the via and the track ends that met it move there, and the
/// caller validates the result as it validates any candidate. A via with no
/// such spot is left where it is.
///
/// # Errors
/// Returns why the board could not be read or written.
pub fn move_vias_off_pads(candidate: &Path) -> Result<usize, String> {
    move_vias_off_pads_with(candidate, &BTreeSet::new())
}

/// [`move_vias_off_pads`] for a board that may use via-in-pad: a via in a pad of
/// a component that declares the under-pad escape, `underpad`, is the pad's
/// escape and stays, and a via in any other pad is moved off it, since only that
/// land is sized for the filled and capped via and exempt from the plain ring.
///
/// # Errors
/// As [`move_vias_off_pads`].
pub fn move_vias_off_component_pads(
    candidate: &Path,
    underpad: &BTreeSet<String>,
) -> Result<usize, String> {
    move_vias_off_pads_with(candidate, underpad)
}

#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn move_vias_off_pads_with(candidate: &Path, spare: &BTreeSet<String>) -> Result<usize, String> {
    let text = std::fs::read_to_string(candidate)
        .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
    let board = PcbBoard::parse(&text).map_err(|e| e.to_string())?;
    let mut out = text.clone();
    let mut moved = 0;
    let mut taken: Vec<((f64, f64), f64)> = Vec::new();
    // The tracks this pass has added, which the board parsed at the start does
    // not hold: a stub to a moved via, and the far end of a track taken along.
    let mut added: Vec<AddedTrack> = Vec::new();
    for via in &board.vias {
        let Some(net) = net_of(&board, via.net, via.net_name.as_ref()) else {
            continue;
        };
        let host = board.pads().find(|pad| {
            pad.net == via.net
                && !spare.contains(&pad.footprint)
                && (pad.kind == "thru_hole" || pad.layers.iter().any(|l| via.layers.contains(l)))
                && pad.signed_distance_nm(via.at) <= (via.drill_nm / 2) as f64
        });
        let Some(host) = host else {
            continue;
        };
        // A surface pad whose only link was the via keeps one: a short track from
        // the pad's centre to where the via goes.
        let stub_layer = stub_layer_of(host, via);
        let pad_centre = (host.at.x_mm(), host.at.y_mm());
        let stub_keepout = Keepout::from_board_in_pad(&board, &net, VIA_GAP_MM);
        let from = (via.at.x_mm(), via.at.y_mm());
        let radius = nm_to_mm(via.size_nm) / 2.0;
        // A track of the net on any layer that meets the via's copper but not its
        // centre would be left behind by a move: the via stays.
        if touched_off_centre(&board, &net, from, radius) {
            continue;
        }
        let keepout = Keepout::from_board(&board, &net, VIA_GAP_MM);
        let attached: Vec<&crate::pcb_read::Segment> = board
            .segments
            .iter()
            .filter(|segment| {
                net_of(&board, segment.net, segment.net_name.as_ref()).as_deref() == Some(&net)
                    && via.layers.contains(&segment.layer)
                    && (near(segment.start, from) || near(segment.end, from))
            })
            .collect();
        let others: Vec<((f64, f64), f64)> = taken.clone();
        // A via moved earlier in the pass is not in the parsed board where it now
        // sits, so a track must keep off it as it keeps off any via.
        let moved_clear = |q: (f64, f64), need: f64| {
            others
                .iter()
                .all(|(at, r)| (at.0 - q.0).hypot(at.1 - q.1) >= need + r - EPS)
        };
        let earlier =
            |q: (f64, f64), need: f64| added.iter().all(|track| track.clear_of(&net, q, need));
        let Some(to) = nearest_free_spot(from, radius, &keepout, &others, |p| {
            let stub_clear = stub_layer.is_none()
                || (0..=8_u32).all(|i| {
                    let t = f64::from(i) / 8.0;
                    let at = (
                        pad_centre.0 + (p.0 - pad_centre.0) * t,
                        pad_centre.1 + (p.1 - pad_centre.1) * t,
                    );
                    stub_keepout.track_clear(at, STUB_WIDTH_MM / 2.0, from)
                        && moved_clear(at, STUB_WIDTH_MM / 2.0 + TRACK_GAP_MM)
                        && earlier(at, STUB_WIDTH_MM / 2.0 + TRACK_GAP_MM)
                });
            earlier(p, radius + VIA_GAP_MM)
                && stub_clear
                && attached.iter().all(|segment| {
                    let far = if near(segment.start, from) {
                        segment.end
                    } else {
                        segment.start
                    };
                    let far = (far.x_mm(), far.y_mm());
                    // The track from where it was to the new spot must also be clear.
                    let half = nm_to_mm(segment.width_nm) / 2.0;
                    let steps = 8_u32;
                    (0..=steps).all(|i| {
                        let t = f64::from(i) / f64::from(steps);
                        let at = (far.0 + (p.0 - far.0) * t, far.1 + (p.1 - far.1) * t);
                        keepout.track_clear(at, half, from)
                            && moved_clear(at, half + TRACK_GAP_MM)
                            && earlier(at, half + TRACK_GAP_MM)
                    })
                })
        }) else {
            // Nowhere to put it. A via that carries no track and has a neighbour of
            // its net close by, one the pad does not swallow, only joins the pours
            // that the neighbour joins as well, so it can simply go.
            // A surface pad needs its via to stay joined unless something else
            // reaches the pad.
            let joined_otherwise = stub_layer
                .as_ref()
                .is_none_or(|layer| pad_otherwise_joined(&board, host, via, &net, layer));
            let spare = attached.is_empty()
                && joined_otherwise
                && board.vias.iter().any(|other| {
                    other != via
                        && net_of(&board, other.net, other.net_name.as_ref()).as_deref()
                            == Some(&net)
                        && (other.at.x_mm() - from.0).hypot(other.at.y_mm() - from.1) < 3.0
                        && !board.pads().any(|pad| {
                            pad.net == other.net
                                && pad.signed_distance_nm(other.at) <= (other.drill_nm / 2) as f64
                        })
                });
            if spare {
                out = remove_via_in_text(&out, from);
                moved += 1;
            }
            continue;
        };
        out = move_via_in_text(&out, from, to);
        if let Some(layer) = &stub_layer {
            out = add_stub(&out, &board, &net, layer, pad_centre, to);
            added.push(AddedTrack::new(&net, pad_centre, to, STUB_WIDTH_MM / 2.0));
        }
        for segment in &attached {
            let far = if near(segment.start, from) {
                segment.end
            } else {
                segment.start
            };
            added.push(AddedTrack::new(
                &net,
                (far.x_mm(), far.y_mm()),
                to,
                nm_to_mm(segment.width_nm) / 2.0,
            ));
        }
        taken.push((to, radius));
        moved += 1;
    }
    if moved > 0 {
        std::fs::write(candidate, out)
            .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
    }
    Ok(moved)
}

/// Whether the surface pad `host`, which `via` sits in, is joined to its net by
/// something other than that via: a track of the net on `layer` that ends in the
/// pad away from the via, or another via of the net in the pad.
#[allow(clippy::cast_precision_loss)]
fn pad_otherwise_joined(
    board: &PcbBoard,
    host: &crate::pcb_read::Pad,
    via: &crate::pcb_read::Via,
    net: &str,
    layer: &str,
) -> bool {
    let radius_nm = via.size_nm as f64 / 2.0;
    let own = |code: Option<u32>, name: Option<&String>| {
        net_of(board, code, name).as_deref() == Some(net)
    };
    board.segments.iter().any(|segment| {
        own(segment.net, segment.net_name.as_ref())
            && segment.layer == layer
            && [segment.start, segment.end].iter().any(|end| {
                host.signed_distance_nm(*end) <= (segment.width_nm / 2) as f64
                    && ((end.x_nm - via.at.x_nm) as f64).hypot((end.y_nm - via.at.y_nm) as f64)
                        > radius_nm
            })
    }) || board.vias.iter().any(|other| {
        other != via
            && own(other.net, other.net_name.as_ref())
            && host.signed_distance_nm(other.at) <= (other.drill_nm / 2) as f64
    })
}

/// A track this pass has laid, to be kept clear of by the vias and tracks that
/// move after it.
struct AddedTrack {
    net: String,
    from: (f64, f64),
    to: (f64, f64),
    half: f64,
}

impl AddedTrack {
    fn new(net: &str, from: (f64, f64), to: (f64, f64), half: f64) -> Self {
        Self {
            net: net.to_string(),
            from,
            to,
            half,
        }
    }

    /// Whether a point of `net` that needs `need` mm beside a track's copper is
    /// clear of this one. A track of the same net is no obstacle.
    fn clear_of(&self, net: &str, p: (f64, f64), need: f64) -> bool {
        self.net == net || point_segment(p, self.from, self.to) >= need + self.half - EPS
    }
}

/// The layer a surface pad's stub track runs on: the pad's own layer that the via
/// reaches. A through-hole pad needs none.
fn stub_layer_of(pad: &crate::pcb_read::Pad, via: &crate::pcb_read::Via) -> Option<String> {
    (pad.kind == "smd")
        .then(|| pad.layers.iter().find(|l| via.layers.contains(l)).cloned())
        .flatten()
}

/// Whether a track of `net`, on any layer, ends inside the copper of a via of
/// `radius` at `from` without ending at its centre.
fn touched_off_centre(board: &PcbBoard, net: &str, from: (f64, f64), radius: f64) -> bool {
    board.segments.iter().any(|segment| {
        net_of(board, segment.net, segment.net_name.as_ref()).as_deref() == Some(net)
            && [segment.start, segment.end].iter().any(|end| {
                let gap = (end.x_mm() - from.0).hypot(end.y_mm() - from.1);
                gap >= 0.01 && gap <= radius
            })
    })
}

/// The width of the track that keeps a pad joined to its moved via.
const STUB_WIDTH_MM: f64 = 0.2;

/// `text` with a track of net `net` on `layer` from `from` to `to` added.
fn add_stub(
    text: &str,
    board: &PcbBoard,
    net: &str,
    layer: &str,
    from: (f64, f64),
    to: (f64, f64),
) -> String {
    let Some(close) = text.rfind(')') else {
        return text.to_string();
    };
    let body_end = text[..close].trim_end().len();
    let token = board
        .nets
        .iter()
        .find(|(_, name)| name.as_str() == net)
        .map_or_else(
            || format!("\"{}\"", escape_net_name(net)),
            |(code, _)| code.to_string(),
        );
    let id = uuid_for(&TieVia {
        net: net.to_string(),
        at: Point::new(mm_to_nm(to.0), mm_to_nm(to.1)),
        size_nm: mm_to_nm(STUB_WIDTH_MM),
        drill_nm: 0,
    });
    let mut out = String::with_capacity(text.len() + 200);
    out.push_str(&text[..body_end]);
    let _ = write!(
        out,
        "\n\t(segment\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(width {})\n\t\t(layer \"{layer}\")\n\t\t(net {token})\n\t\t(uuid \"{id}\")\n\t)",
        mm(mm_to_nm(from.0)),
        mm(mm_to_nm(from.1)),
        mm(mm_to_nm(to.0)),
        mm(mm_to_nm(to.1)),
        mm(mm_to_nm(STUB_WIDTH_MM)),
    );
    out.push_str(&text[body_end..]);
    out
}

fn near(p: Point, at: (f64, f64)) -> bool {
    (p.x_mm() - at.0).hypot(p.y_mm() - at.1) < 0.01
}

impl Keepout {
    /// Whether a track of half width `half` passing through `p` keeps clear of
    /// every pad, via and foreign track, other than the via at `ignore`, which is
    /// the one being moved and sits where the track starts.
    fn track_clear(&self, p: (f64, f64), half: f64, ignore: (f64, f64)) -> bool {
        let need = half + TRACK_GAP_MM;
        self.pads.iter().all(|pad| pad.distance(p) >= need - EPS)
            && self
                .tracks
                .iter()
                .all(|(a, b, other)| point_segment(p, *a, *b) >= need + other - EPS)
            && self.vias.iter().all(|(at, other)| {
                (at.0 - ignore.0).hypot(at.1 - ignore.1) < 0.01
                    || (at.0 - p.0).hypot(at.1 - p.1) >= need + other - EPS
            })
    }
}

/// The track-to-copper gap a moved track keeps, a little over the board's
/// usual 0.127 mm minimum.
const TRACK_GAP_MM: f64 = 0.15;

/// The nearest point within a millimetre of `from` where a via of `radius` with a
/// hole of `hole` is clear of everything, and `accept` agrees.
fn nearest_free_spot(
    from: (f64, f64),
    radius: f64,
    keepout: &Keepout,
    taken: &[((f64, f64), f64)],
    accept: impl Fn((f64, f64)) -> bool,
) -> Option<(f64, f64)> {
    for step in 1..=12_u32 {
        let r = f64::from(step) * 0.1;
        for k in 0..24_u32 {
            let angle = f64::from(k) * std::f64::consts::TAU / 24.0;
            let p = (from.0 + r * angle.cos(), from.1 + r * angle.sin());
            if keepout.clear_of_pads_only(p, radius, from)
                && taken.iter().all(|(at, other)| {
                    (at.0 - p.0).hypot(at.1 - p.1) >= radius + other + keepout.gap_mm
                })
                && accept(p)
            {
                return Some(p);
            }
        }
    }
    None
}

impl Keepout {
    /// A via of `radius` at `p` keeps the gap from every pad, foreign track and
    /// via other than the one being moved (which is at the same spot it started
    /// from, so it would never be clear of itself).
    fn clear_of_pads_only(&self, p: (f64, f64), radius: f64, ignore: (f64, f64)) -> bool {
        let need = radius + self.gap_mm;
        self.pads.iter().all(|pad| pad.distance(p) >= need - EPS)
            && self
                .tracks
                .iter()
                .all(|(a, b, half)| point_segment(p, *a, *b) >= need + half - EPS)
            && self.vias.iter().all(|(at, other)| {
                (at.0 - ignore.0).hypot(at.1 - ignore.1) < 0.01
                    || (at.0 - p.0).hypot(at.1 - p.1) >= need + other - EPS
            })
            && self
                .no_via_areas
                .iter()
                .all(|area| !area.contains(p) && area.edge_distance(p) >= radius - EPS)
    }
}

/// `text` with the via at `from`, and the ends of the tracks that meet it, put
/// at `to`.
fn move_via_in_text(text: &str, from: (f64, f64), to: (f64, f64)) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = next_block(rest) {
        out.push_str(&rest[..start]);
        let block = &rest[start..];
        let Some(end) = block_end(block) else {
            out.push_str(block);
            rest = "";
            break;
        };
        let (head, body) = block.split_at(end);
        out.push_str(&rewrite_points(head, from, to));
        rest = body;
    }
    out.push_str(rest);
    out
}

/// `text` without the via at `at`.
fn remove_via_in_text(text: &str, at: (f64, f64)) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("(via") {
        let block = &rest[i..];
        let Some(end) = block_end(block) else {
            break;
        };
        let here = rewrite_points(&block[..end], at, (f64::NAN, f64::NAN)).contains("NaN");
        if here {
            // Take the line's leading whitespace with it.
            let head = rest[..i].trim_end_matches([' ', '\t']);
            out.push_str(head);
            rest = block[end..].trim_start_matches('\n');
        } else {
            out.push_str(&rest[..i + end]);
            rest = &block[end..];
        }
    }
    out.push_str(rest);
    out
}

/// The index of the next `(via` or `(segment` block in `text`.
fn next_block(text: &str) -> Option<usize> {
    let via = text.find("(via");
    let segment = text.find("(segment");
    match (via, segment) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The length of the balanced `(...)` block `text` starts with.
fn block_end(text: &str) -> Option<usize> {
    let mut depth = 0_i32;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// `block` with every `(at X Y)`, `(start X Y)` and `(end X Y)` at `from` put at `to`.
fn rewrite_points(block: &str, from: (f64, f64), to: (f64, f64)) -> String {
    let mut out = String::with_capacity(block.len());
    let mut rest = block;
    'scan: while !rest.is_empty() {
        for tag in ["(at ", "(start ", "(end "] {
            if let Some(after) = rest.strip_prefix(tag) {
                let close = after.find(')').unwrap_or(after.len());
                let mut numbers = after[..close].split_whitespace().map(str::parse::<f64>);
                if let (Some(Ok(x)), Some(Ok(y))) = (numbers.next(), numbers.next()) {
                    if (x - from.0).abs() < 0.002 && (y - from.1).abs() < 0.002 {
                        let _ = write!(out, "{tag}{:.6} {:.6}", to.0, to.1);
                        rest = &after[close..];
                        continue 'scan;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Drops each track that is shorter than its own width and has one end in nothing,
/// and returns how many it dropped.
///
/// The engine now and then leaves a sliver of track at the end of a branch: a
/// piece too short to carry anything, whose far end touches no track, via, pad or
/// pour of its net. KiCad reports it as an unconnected track end. Taking it away
/// cannot disconnect anything, because an end that touches nothing joins nothing;
/// a track with both ends in copper, and one longer than it is wide, are left
/// alone. The caller validates the result as it validates any candidate.
///
/// # Errors
/// Returns why the board could not be read or written.
#[allow(clippy::cast_precision_loss)]
pub fn drop_dangling_stubs(candidate: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(candidate)
        .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
    let board = PcbBoard::parse(&text).map_err(|e| e.to_string())?;
    let mut alive = vec![true; board.segments.len()];
    loop {
        let mut dropped = false;
        for (i, segment) in board.segments.iter().enumerate() {
            if !alive[i] {
                continue;
            }
            let (a, b) = (
                (segment.start.x_mm(), segment.start.y_mm()),
                (segment.end.x_mm(), segment.end.y_mm()),
            );
            let width = nm_to_mm(segment.width_nm);
            if (b.0 - a.0).hypot(b.1 - a.1) >= width {
                continue;
            }
            let free = |p: (f64, f64)| !end_touches_copper(&board, &alive, i, p);
            if free(a) != free(b) {
                alive[i] = false;
                dropped = true;
            }
        }
        if !dropped {
            break;
        }
    }
    let mut out = text.clone();
    let mut count = 0;
    for (i, segment) in board.segments.iter().enumerate() {
        if alive[i] {
            continue;
        }
        let ends = (
            (segment.start.x_mm(), segment.start.y_mm()),
            (segment.end.x_mm(), segment.end.y_mm()),
        );
        let next = remove_segment_in_text(&out, ends, &segment.layer);
        if next != out {
            out = next;
            count += 1;
        }
    }
    if count > 0 {
        std::fs::write(candidate, out)
            .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
    }
    Ok(count)
}

/// Whether the end of track `index` at `p` touches copper of its own net: the end
/// of another live track on the layer, a via, a pad, or a pour that covers the point.
#[allow(clippy::cast_precision_loss)]
fn end_touches_copper(board: &PcbBoard, alive: &[bool], index: usize, p: (f64, f64)) -> bool {
    let segment = &board.segments[index];
    let Some(net) = net_of(board, segment.net, segment.net_name.as_ref()) else {
        return true;
    };
    let half = nm_to_mm(segment.width_nm) / 2.0;
    let at = Point::new(mm_to_nm(p.0), mm_to_nm(p.1));
    // Tracks join end to end, as KiCad counts them: a track that merely overlaps
    // another's copper leaves its own end unconnected.
    let track = board.segments.iter().enumerate().any(|(j, other)| {
        j != index
            && alive[j]
            && other.layer == segment.layer
            && net_of(board, other.net, other.net_name.as_ref()).as_deref() == Some(net.as_str())
            && [other.start, other.end]
                .iter()
                .any(|end| (end.x_mm() - p.0).hypot(end.y_mm() - p.1) < 0.01)
    });
    let via = board.vias.iter().any(|via| {
        net_of(board, via.net, via.net_name.as_ref()).as_deref() == Some(net.as_str())
            && via.layers.contains(&segment.layer)
            && (via.at.x_mm() - p.0).hypot(via.at.y_mm() - p.1)
                <= nm_to_mm(via.size_nm) / 2.0 + half
    });
    let pad = board.pads().any(|pad| {
        net_of(board, pad.net, pad.net_name.as_ref()).as_deref() == Some(net.as_str())
            && (pad.kind == "thru_hole" || pad.layers.contains(&segment.layer))
            && pad.signed_distance_nm(at) <= half * NM_PER_MM
    });
    let pour = board.zones.iter().any(|zone| {
        net_of(board, zone.net, zone.net_name.as_ref()).as_deref() == Some(net.as_str())
            && zone
                .outlines
                .iter()
                .any(|(layer, outline)| layer == &segment.layer && polygon_contains(outline, p))
    });
    track || via || pad || pour
}

/// Whether `p` is inside the closed polygon `outline` (even-odd).
fn polygon_contains(outline: &[Point], p: (f64, f64)) -> bool {
    let mut inside = false;
    let mut j = outline.len().saturating_sub(1);
    for i in 0..outline.len() {
        let (xi, yi) = (outline[i].x_mm(), outline[i].y_mm());
        let (xj, yj) = (outline[j].x_mm(), outline[j].y_mm());
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// `text` without the track on `layer` between `ends`.
fn remove_segment_in_text(text: &str, ends: ((f64, f64), (f64, f64)), layer: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("(segment") {
        let block = &rest[i..];
        let Some(end) = block_end(block) else {
            break;
        };
        let body = &block[..end];
        let numbers = |tag: &str| -> Option<(f64, f64)> {
            let after = &body[body.find(tag)? + tag.len()..];
            let mut values = after[..after.find(')')?]
                .split_whitespace()
                .map(str::parse::<f64>);
            Some((values.next()?.ok()?, values.next()?.ok()?))
        };
        let same = numbers("(start ")
            .zip(numbers("(end "))
            .is_some_and(|(a, b)| {
                (a.0 - ends.0 .0).abs() < 0.002
                    && (a.1 - ends.0 .1).abs() < 0.002
                    && (b.0 - ends.1 .0).abs() < 0.002
                    && (b.1 - ends.1 .1).abs() < 0.002
            })
            && body.contains(&format!("\"{layer}\""));
        if same {
            // Take the line's leading whitespace with it.
            let head = rest[..i].trim_end_matches([' ', '\t']);
            out.push_str(head);
            rest = block[end..].trim_start_matches('\n');
        } else {
            out.push_str(&rest[..i + end]);
            rest = &block[end..];
        }
    }
    out.push_str(rest);
    out
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

    /// A supply ball on the outer layer that the router left with no via and
    /// no track is an island of one against a net poured on an inner layer.
    fn lone_pad_board(extra: &str) -> PcbBoard {
        let text = format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (2 "B.Cu" signal))
              (net 0 "") (net 1 "+3V3") (net 2 "SIG")
              (footprint "U" (at 0 0) (layer "F.Cu") (property "Reference" "U1")
                (pad "C10" smd circle (at 10 10) (size 0.35 0.35) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "+3V3"))
                (pad "C11" smd circle (at 10.8 10) (size 0.35 0.35) (layers "F.Cu" "F.Paste" "F.Mask") (net 2 "SIG")))
              {extra}
              (zone (net 1) (net_name "+3V3") (layer "In1.Cu") (fill yes)
                (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                (filled_polygon (layer "In1.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#
        );
        PcbBoard::parse(&text).expect("board must parse")
    }

    /// JLCPCB's via-in-pad land for the 0.2 mm drill is 0.35 mm.
    const IN_PAD: (i64, i64) = (350_000, 200_000);

    /// The components that declare the under-pad escape on the test boards.
    fn declared() -> BTreeSet<String> {
        BTreeSet::from(["U1".to_string()])
    }

    #[test]
    fn a_pad_the_router_left_joined_to_nothing_gets_the_plane_via_in_the_pad() {
        let board = lone_pad_board("");
        let plan = plan_pad_ties(&board, IN_PAD, &declared(), &ViaDeclarations::new());
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        assert_eq!(plan.vias[0].net, "+3V3");
        assert_eq!(plan.vias[0].at, Point::new(10_000_000, 10_000_000));
        assert_eq!(
            (plan.vias[0].size_nm, plan.vias[0].drill_nm),
            (350_000, 200_000)
        );
        assert!(plan.unresolved.is_empty(), "{plan:?}");
    }

    #[test]
    fn the_pad_via_is_only_planned_where_the_process_offers_one() {
        let board = lone_pad_board("");
        let without = plan_all_with(&board, REACH, &Ground::default(), &ViaDeclarations::new());
        assert!(without.vias.is_empty(), "{without:?}");
        let with = plan_all_with(
            &board,
            REACH,
            &Ground::default()
                .with_in_pad_via(IN_PAD.0, IN_PAD.1)
                .with_underpad(&["U1".to_string()]),
            &ViaDeclarations::new(),
        );
        assert!(
            with.vias.iter().any(|v| v.net == "+3V3"),
            "the ground carries the via the process offers: {with:?}"
        );
    }

    #[test]
    fn a_pad_a_via_of_its_net_already_touches_is_left_alone() {
        let board = lone_pad_board(
            r#"(via (at 10 10) (size 0.35) (drill 0.2) (layers "F.Cu" "B.Cu") (net 1))"#,
        );
        let plan = plan_pad_ties(&board, IN_PAD, &declared(), &ViaDeclarations::new());
        assert!(
            plan.vias.is_empty() && plan.unresolved.is_empty(),
            "{plan:?}"
        );
    }

    #[test]
    fn a_pad_narrower_than_the_via_is_reported_not_given_one() {
        let board = lone_pad_board("");
        let plan = plan_pad_ties(
            &board,
            (450_000, 300_000),
            &declared(),
            &ViaDeclarations::new(),
        );
        assert!(plan.vias.is_empty());
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");
        assert!(plan.unresolved[0].contains("C10"), "{plan:?}");
    }

    #[test]
    fn a_via_is_not_put_where_another_nets_copper_is_within_the_gap() {
        // A track of SIG passes 0.1 mm from the pad's edge.
        let board = lone_pad_board(
            r#"(segment (start 9 10.3) (end 11 10.3) (width 0.1) (layer "F.Cu") (net 2))"#,
        );
        let plan = plan_pad_ties(&board, IN_PAD, &declared(), &ViaDeclarations::new());
        assert!(plan.vias.is_empty(), "{plan:?}");
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");
    }

    #[test]
    fn the_usual_via_wins_over_one_escalated_via() {
        let sizes = [(560_000, 300_000), (600_000, 300_000), (600_000, 300_000)];
        assert_eq!(
            usual_via(&sizes, (DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM)),
            (600_000, 300_000)
        );
        assert_eq!(
            usual_via(&[], (DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM)),
            (DEFAULT_VIA_SIZE_NM, DEFAULT_VIA_DRILL_NM)
        );
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
        let plan = plan_ties(&board, &ViaDeclarations::new());
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
        let plan = plan_ties(&board, &ViaDeclarations::new());
        assert!(plan.vias.is_empty());
        assert_eq!(
            plan.unresolved.len(),
            1,
            "a live island with no room for a via must be a finding: {plan:?}"
        );
        assert!(plan.unresolved[0].contains("GND"), "{plan:?}");
    }

    #[test]
    fn an_island_too_small_for_the_usual_via_gets_a_compact_one() {
        // 1.2 x 0.62 mm: its widest circle has a radius of 0.31 mm, short of a
        // 0.6 mm via with the slack (0.35 mm) but enough for a 0.46 mm one.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND")
          (segment (start 25.2 7) (end 25.4 7) (width 0.1) (layer "F.Cu") (net 1))
          (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 25 6.69) (xy 26.2 6.69) (xy 26.2 7.31) (xy 25 7.31))))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("board must parse");
        let plan = plan_ties(&board, &ViaDeclarations::new());
        assert!(plan.unresolved.is_empty(), "{plan:?}");
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        assert_eq!(plan.vias[0].size_nm, COMPACT_VIA_SIZE_NM);
        assert_eq!(plan.vias[0].drill_nm, COMPACT_VIA_DRILL_NM);
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

    /// The land of a return via may reach into the clearance the fill keeps from
    /// foreign copper, since the keep-out holds the board's own gap there, but it
    /// may not leave the extent of the copper at the board edge.
    #[test]
    fn a_return_via_may_sit_close_to_a_cutout_in_the_fill_but_not_at_the_board_edge() {
        let l_shape = [
            (0.0, 0.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, 20.0),
            (0.0, 20.0),
        ];
        let points: Vec<Point> = l_shape
            .iter()
            .map(|&(x, y)| Point::new(mm_to_nm(x), mm_to_nm(y)))
            .collect();
        let region = Region::new("B.Cu", &points).expect("a region");
        let regions = [region];
        let radius = 0.23;
        // 0.1 mm from the notch's wall: inside the fill, far from the board edge.
        assert!(pour_room_within(
            &regions,
            "B.Cu",
            fill_bounds(&regions, "B.Cu"),
            (9.9, 15.0),
            radius
        ));
        // 0.1 mm from the board edge: the land would leave the copper.
        assert!(!pour_room_within(
            &regions,
            "B.Cu",
            fill_bounds(&regions, "B.Cu"),
            (0.1, 5.0),
            radius
        ));
        // Outside the fill, or on another layer, there is no room.
        assert!(!pour_room_within(
            &regions,
            "B.Cu",
            fill_bounds(&regions, "B.Cu"),
            (15.0, 15.0),
            radius
        ));
        assert!(!pour_room_within(
            &regions,
            "F.Cu",
            fill_bounds(&regions, "F.Cu"),
            (5.0, 5.0),
            radius
        ));
    }

    /// A rule area that keeps vias out, such as a mounting hole's screw-head keepout,
    /// is an obstacle to a return via like a pad is: the ground via never sits in it.
    #[test]
    fn a_return_via_stays_out_of_a_keepout_that_refuses_vias() {
        let board = stitched_board("");
        let free = stitch(
            &board,
            REACH,
            &[],
            &Ground::default(),
            &ViaDeclarations::new(),
        )
        .vias;
        assert_eq!(free.len(), 1, "an unobstructed board takes one ground via");
        let keepout = stitched_board(
            r#"(zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed))
                 (polygon (pts (xy 6 6) (xy 14 6) (xy 14 14) (xy 6 14))))"#,
        );
        let placed = stitch(
            &keepout,
            REACH,
            &[],
            &Ground::default(),
            &ViaDeclarations::new(),
        )
        .vias;
        for via in &placed {
            let (x, y) = at_mm(via);
            assert!(
                !(6.0..=14.0).contains(&x) || !(6.0..=14.0).contains(&y),
                "a ground via at ({x}, {y}) sits in the keepout"
            );
        }
    }

    /// A return via keeps the copper the board's own rules ask around it: the
    /// declared class clearance, and the hole clearance less the ring. With a 0.127
    /// mm class clearance that is closer than the fixed 0.2 mm, so the ground via
    /// sits nearer the signal via it serves.
    #[test]
    fn a_return_via_follows_the_boards_declared_clearance_not_the_fixed_gap() {
        let board = stitched_board("");
        let nearest = |declared: &ViaDeclarations| {
            let vias = stitch(&board, REACH, &[], &Ground::default(), declared).vias;
            assert_eq!(vias.len(), 1, "{vias:?}");
            let (x, y) = at_mm(&vias[0]);
            (x - 10.0).hypot(y - 10.0)
        };
        let fixed = nearest(&ViaDeclarations::new());
        let declared = ViaDeclarations {
            clearance_mm: Some(0.127),
            hole_clearance_mm: Some(0.2),
            ..ViaDeclarations::new()
        };
        assert!(
            nearest(&declared) < fixed - 0.01,
            "a 0.127 mm clearance lets the ground via sit nearer than the fixed gap's {fixed}"
        );
        // The hole clearance still governs when it asks more than the class does.
        let strict = ViaDeclarations {
            clearance_mm: Some(0.127),
            hole_clearance_mm: Some(0.6),
            ..ViaDeclarations::new()
        };
        assert!((strict.return_gap_mm(600_000, 300_000) - 0.45).abs() < 1e-9);
        assert!((declared.return_gap_mm(460_000, 200_000) - 0.127).abs() < 1e-9);
        assert!((ViaDeclarations::new().return_gap_mm(600_000, 300_000) - VIA_GAP_MM).abs() < 1e-9);
    }

    /// A net that declares a medium edge looks for its return via in a wider window:
    /// a ground via 3 mm away serves it, where a fast net gets a new one beside it.
    #[test]
    fn a_medium_edge_net_is_served_by_a_ground_via_in_the_wider_window() {
        let board = stitched_board(
            r#"(via (at 13 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
        );
        let fast = stitch(
            &board,
            REACH,
            &[],
            &Ground::default(),
            &ViaDeclarations::new(),
        );
        assert_eq!(
            fast.vias.len(),
            1,
            "a fast net wants a ground via within a dielectric height: {:?}",
            fast.vias
        );
        let medium = Ground::default().with_medium(&["SIG".to_string()]);
        let served = stitch(&board, REACH, &[], &medium, &ViaDeclarations::new());
        assert!(
            served.vias.is_empty() && served.unresolved.is_empty(),
            "a medium net accepts the ground via 3 mm away: {:?}",
            served.vias
        );
        let far = stitched_board(
            r#"(via (at 17 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
        );
        let beyond = stitch(&far, REACH, &[], &medium, &ViaDeclarations::new());
        assert_eq!(
            beyond.vias.len(),
            1,
            "a ground via 7 mm away is outside the medium window: {:?}",
            beyond.vias
        );
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
        let plan = plan_all_with(&board, REACH, &slow, &ViaDeclarations::new());
        assert!(plan.unresolved.is_empty(), "{plan:?}");
        assert_eq!(plan.advisory.len(), 1, "{plan:?}");
        assert!(plan.advisory[0].contains("net SIG"), "{plan:?}");

        let fast = Ground::default().with_critical(&["SIG".to_string()]);
        let plan = plan_all_with(&board, REACH, &fast, &ViaDeclarations::new());
        assert!(plan.advisory.is_empty(), "{plan:?}");
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");

        let strict = plan_all_with(&board, REACH, &Ground::default(), &ViaDeclarations::new());
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

    /// Beside a fine-pitch pad row (a QFN breakout) the pads leave no room for a
    /// return via, whatever the search tries, so a signal via there is listed
    /// for the review instead of holding the board back.
    #[test]
    fn a_signal_via_in_a_fine_pitch_breakout_with_no_room_is_an_advisory() {
        let breakout = r#"(footprint "U" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 6 6) (layers "F.Cu") (net 3))
            (pad "2" smd rect (at 11.5 10) (size 0.2 0.8) (layers "F.Cu") (net 3))
            (pad "3" smd rect (at 11.9 10) (size 0.2 0.8) (layers "F.Cu") (net 3)))"#;
        let ground = Ground::default().with_fine_pitch_advisory(true);
        let plan = plan_all_with(
            &stitched_board(breakout),
            REACH,
            &ground,
            &ViaDeclarations::new(),
        );
        assert!(plan.vias.is_empty(), "{plan:?}");
        assert!(plan.unresolved.is_empty(), "{plan:?}");
        assert_eq!(plan.advisory.len(), 1, "{plan:?}");
        assert!(plan.advisory[0].contains("fine-pitch"), "{plan:?}");
    }

    /// Without the opt-in the same breakout blocks, as on main.
    #[test]
    fn a_fine_pitch_breakout_with_no_room_blocks_without_the_opt_in() {
        let breakout = r#"(footprint "U" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 6 6) (layers "F.Cu") (net 3))
            (pad "2" smd rect (at 11.5 10) (size 0.2 0.8) (layers "F.Cu") (net 3))
            (pad "3" smd rect (at 11.9 10) (size 0.2 0.8) (layers "F.Cu") (net 3)))"#;
        let plan = plan_all(&stitched_board(breakout), REACH);
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");
        assert!(plan.advisory.is_empty(), "{plan:?}");
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
        let plan = plan_all_with(&board, REACH, &ground, &ViaDeclarations::new());
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

    /// Foreign pads leave a corridor round the signal via too narrow for the
    /// usual 0.6 mm return via but wide enough for a compact one, which the
    /// board's own 0.127 mm clearance still allows.
    #[test]
    fn a_compact_return_via_fits_where_the_usual_one_does_not() {
        let frame = r#"(footprint "U" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 11.475) (size 4 1.05) (layers "F.Cu") (net 3))
            (pad "2" smd rect (at 10 8.525) (size 4 1.05) (layers "F.Cu") (net 3))
            (pad "3" smd rect (at 8.525 10) (size 1.05 1.9) (layers "F.Cu") (net 3))
            (pad "4" smd rect (at 11.475 10) (size 1.05 1.9) (layers "F.Cu") (net 3)))"#;
        let vias = stitch_vias(&stitched_board(frame), REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        assert_eq!(vias[0].net, "GND");
        assert_eq!(vias[0].size_nm, 460_000);
        assert_eq!(vias[0].drill_nm, 200_000);
    }

    /// Under a ball grid the outer layer holds no pour, but two inner planes do: the
    /// return via lands in those, not in an outer pour.
    #[test]
    fn a_return_via_lands_in_two_inner_planes_when_the_outer_layers_hold_no_pour() {
        let plane = |layer: &str| {
            format!(
                r#"(zone (net 1) (net_name "GND") (layer "{layer}") (fill yes)
                  (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                  (filled_polygon (layer "{layer}") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20))))"#
            )
        };
        let text = format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "In2.Cu" signal) (31 "B.Cu" signal))
              (net 0 "") (net 1 "GND") (net 2 "SIG")
              (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
              {} {})"#,
            plane("In1.Cu"),
            plane("In2.Cu")
        );
        let board = PcbBoard::parse(&text).expect("parses");
        assert_eq!(stitch_vias(&board, REACH, &[]).len(), 1);
    }

    /// With no free spot in reach, a ground pad within reach takes the return via
    /// in the pad where the process fills and caps one, and is left alone where
    /// it does not.
    #[test]
    fn a_return_via_goes_in_a_ground_pad_when_no_free_spot_is_in_reach() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (footprint "C" (at 0 0) (layer "F.Cu") (property "Reference" "U1")
            (pad "1" smd rect (at 10.7 10.5) (size 0.5 0.5) (layers "F.Cu") (net 1)))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("parses");
        let none = stitch(
            &board,
            REACH,
            &[],
            &Ground::default(),
            &ViaDeclarations::new(),
        );
        assert!(
            none.vias.is_empty(),
            "no free spot, no in-pad process: {:?}",
            none.vias
        );
        let ground = Ground::default()
            .with_in_pad_via(350_000, 200_000)
            .with_underpad(&["U1".to_string()]);
        let placed = stitch(&board, REACH, &[], &ground, &ViaDeclarations::new());
        assert_eq!(placed.vias.len(), 1, "{:?}", placed.vias);
        let via = &placed.vias[0];
        assert_eq!(at_mm(via), (10.7, 10.5));
        assert_eq!((via.size_nm, via.drill_nm), (350_000, 200_000));
    }

    /// The same board with the pad on the back: a through via reaches either side, so
    /// a ground ball on the back of a BGA is as good a place for the return via as
    /// one on the front.
    #[test]
    fn a_return_via_goes_in_a_ground_pad_on_the_back_layer_too() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (footprint "C" (at 0 0) (layer "B.Cu") (property "Reference" "U1")
            (pad "1" smd rect (at 10.7 10.5) (size 0.5 0.5) (layers "B.Cu") (net 1)))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
          (zone (net 1) (net_name "GND") (layer "In1.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "In1.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("parses");
        let ground = Ground::default()
            .with_in_pad_via(350_000, 200_000)
            .with_underpad(&["U1".to_string()]);
        let placed = stitch(&board, REACH, &[], &ground, &ViaDeclarations::new());
        assert_eq!(placed.vias.len(), 1, "{:?}", placed.vias);
        assert_eq!(at_mm(&placed.vias[0]), (10.7, 10.5));
    }

    /// Only a component that declares the under-pad escape has a pad the filled and
    /// capped via may sit in: the DRC exempts no ring anywhere else, so a ground pad
    /// of any other component is not a place for a return via.
    #[test]
    fn a_return_via_does_not_go_in_a_pad_of_a_component_that_declares_no_escape() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (footprint "C" (at 0 0) (layer "F.Cu") (property "Reference" "C7")
            (pad "1" smd rect (at 10.7 10.5) (size 0.5 0.5) (layers "F.Cu") (net 1)))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("parses");
        let ground = Ground::default()
            .with_in_pad_via(350_000, 200_000)
            .with_underpad(&["U1".to_string()]);
        let placed = stitch(&board, REACH, &[], &ground, &ViaDeclarations::new());
        assert!(placed.vias.is_empty(), "{:?}", placed.vias);
    }

    /// A pad tie goes only in the pad of a component that declares the escape.
    #[test]
    fn a_pad_tie_is_planned_only_for_a_component_that_declares_the_escape() {
        let board = lone_pad_board("");
        let none = plan_pad_ties(
            &board,
            IN_PAD,
            &BTreeSet::from(["C7".to_string()]),
            &ViaDeclarations::new(),
        );
        assert!(
            none.vias.is_empty() && none.unresolved.is_empty(),
            "{none:?}"
        );
    }

    /// A pad tie keeps out of a rule area that refuses vias, and a same-net pad beside
    /// it is not "other copper".
    #[test]
    fn a_pad_tie_keeps_out_of_a_via_keepout_and_ignores_a_pad_of_its_own_net() {
        let keepout = lone_pad_board(
            r#"(zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed))
                 (polygon (pts (xy 9 9) (xy 11 9) (xy 11 11) (xy 9 11))))"#,
        );
        let plan = plan_pad_ties(&keepout, IN_PAD, &declared(), &ViaDeclarations::new());
        assert!(
            plan.vias.is_empty(),
            "the via would break the rule area: {plan:?}"
        );
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "+3V3")
          (footprint "U" (at 0 0) (layer "F.Cu") (property "Reference" "U1")
            (pad "1" smd circle (at 10 10) (size 0.35 0.35) (layers "F.Cu") (net 1 "+3V3"))
            (pad "2" smd circle (at 10.45 10) (size 0.35 0.35) (layers "F.Cu") (net 1 "+3V3")))
          (zone (net 1) (net_name "+3V3") (layer "In1.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "In1.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("parses");
        let plan = plan_pad_ties(&board, IN_PAD, &declared(), &ViaDeclarations::new());
        assert!(
            plan.unresolved.is_empty(),
            "a pad of the same net beside it is not other copper: {plan:?}"
        );
    }

    /// A wall of the board outline inside the fill is not a gap in the fill: a return
    /// via keeps its land off it, though the fill's own extent reaches the wall.
    #[test]
    fn a_return_via_keeps_off_a_wall_of_the_board_outline_inside_the_fill() {
        // The signal via at (10, 10) stands inside a 1.6 mm square of outline walls, so
        // the nearest whole-grid spot beside it is within the land of a wall.
        let walls = r#"(gr_line (start 9.2 9.2) (end 10.8 9.2) (layer "Edge.Cuts") (width 0.1))
                       (gr_line (start 10.8 9.2) (end 10.8 10.8) (layer "Edge.Cuts") (width 0.1))
                       (gr_line (start 10.8 10.8) (end 9.2 10.8) (layer "Edge.Cuts") (width 0.1))
                       (gr_line (start 9.2 10.8) (end 9.2 9.2) (layer "Edge.Cuts") (width 0.1))"#;
        let board = stitched_board(walls);
        let vias = stitch_vias(&board, REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let (x, y) = at_mm(&vias[0]);
        // Land radius 0.3 plus the 0.3 mm copper to edge floor.
        for (a, b) in [
            ((9.2, 9.2), (10.8, 9.2)),
            ((10.8, 9.2), (10.8, 10.8)),
            ((10.8, 10.8), (9.2, 10.8)),
            ((9.2, 10.8), (9.2, 9.2)),
        ] {
            assert!(
                point_segment((x, y), a, b) >= 0.6 - 1e-6,
                "the land of a via at ({x}, {y}) reaches the outline wall {a:?}-{b:?}"
            );
        }
    }

    /// The search grid is sorted nearest first, once per window, and holds the
    /// window's own edges.
    #[test]
    fn the_return_search_grid_is_nearest_first_and_reaches_the_window_edge() {
        let offsets = return_offsets(1.0);
        assert!(
            offsets
                .windows(2)
                .all(|w| w[0].0.hypot(w[0].1) <= w[1].0.hypot(w[1].1)),
            "not nearest first"
        );
        assert_eq!(offsets[0], (0.0, 0.0));
        assert!(offsets.contains(&(1.0, 1.0)) && offsets.contains(&(-1.0, 0.0)));
    }

    /// A stub or track laid earlier in the pass is copper a later move must keep off,
    /// unless it is of the same net.
    #[test]
    fn a_track_laid_earlier_is_clear_only_beyond_its_copper_and_only_for_other_nets() {
        let track = AddedTrack::new("A", (0.0, 0.0), (2.0, 0.0), 0.1);
        assert!(
            !track.clear_of("B", (1.0, 0.3), 0.3),
            "0.3 from the axis is 0.2 from the copper"
        );
        assert!(track.clear_of("B", (1.0, 0.5), 0.3));
        assert!(
            track.clear_of("A", (1.0, 0.0), 0.3),
            "its own net is no obstacle"
        );
    }

    /// A pad sits where the nearest return via would go. Pad copper in the pour
    /// is not an obstacle the fill knows about, so the pass keeps clear of it.
    #[test]
    fn a_return_via_keeps_clear_of_pad_copper() {
        // Pad copper (10.3..11.1, 9.6..10.4), so no via closer than 0.5 mm to it.
        let pad = r#"(footprint "R" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10.7 10) (size 0.8 0.8) (layers "F.Cu") (net 3)))"#;
        let vias = stitch_vias(&stitched_board(pad), REACH, &[]);
        assert_eq!(vias.len(), 1, "{vias:?}");
        let (x, y) = at_mm(&vias[0]);
        let (dx, dy) = (
            (10.3 - x).max(x - 11.1).max(0.0),
            (9.6 - y).max(y - 10.4).max(0.0),
        );
        assert!(dx.hypot(dy) >= 0.5 - 1e-6, "on or beside the pad: {x} {y}");
    }

    #[test]
    fn the_compact_via_grows_to_the_boards_own_via_minimum() {
        assert_eq!(
            ViaDeclarations::new().compact_via(),
            (COMPACT_VIA_SIZE_NM, COMPACT_VIA_DRILL_NM)
        );
        let declared = ViaDeclarations {
            min_via_nm: 560_000,
            ..ViaDeclarations::new()
        };
        assert_eq!(declared.compact_via().0, 560_000);
    }

    /// `min_through_hole_diameter` is the drill floor: KiCad has no separate
    /// via-drill rule, and the exporter writes the profile's drill minimum there.
    #[test]
    fn via_floors_are_read_from_the_kicad_pro_beside_the_board() {
        let dir = std::env::temp_dir().join(format!("synth_via_floors_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcb = dir.join("b.kicad_pcb");
        std::fs::write(
            dir.join("b.kicad_pro"),
            r#"{"design_settings":{"rules":{"min_via_diameter":0.56,
               "min_via_annular_width":0.13,"min_through_hole_diameter":0.3}}}"#,
        )
        .unwrap();
        let floors = declared_via_floors(&pcb);
        assert_eq!(floors.compact_via(), (560_000, 300_000));
        assert_eq!(floors.min_annular_nm, 130_000);
        // No project file: no floors.
        let none = declared_via_floors(&dir.join("absent.kicad_pcb"));
        assert_eq!(none.compact_via(), ViaDeclarations::new().compact_via());
        assert_eq!(
            (none.min_via_nm, none.min_annular_nm, none.min_hole_nm),
            (0, 0, 0)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_island_that_only_a_sub_minimum_via_fits_is_reported_not_given_one() {
        // The compact-island board of the test above: room for 0.46 mm, not 0.56.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND")
          (segment (start 25.2 7) (end 25.4 7) (width 0.1) (layer "F.Cu") (net 1))
          (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
            (filled_polygon (layer "F.Cu") (pts (xy 25 6.69) (xy 26.2 6.69) (xy 26.2 7.31) (xy 25 7.31))))
          (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
            (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
            (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))))"#;
        let board = PcbBoard::parse(text).expect("board must parse");
        let declared = ViaDeclarations {
            min_via_nm: 560_000,
            ..ViaDeclarations::new()
        };
        let plan = plan_ties(&board, &declared);
        assert!(plan.vias.is_empty(), "{plan:?}");
        assert_eq!(plan.unresolved.len(), 1, "{plan:?}");
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
        let tie = plan_ties(&board, &ViaDeclarations::new());
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

    /// A class that asks for 0.25 mm of copper clearance holds a new via 0.25 mm
    /// from a foreign track, not the fixed 0.2 mm: KiCad keeps two nets apart by
    /// the larger of their classes, so a via placed at 0.2 mm fails the board's
    /// own DRC.
    #[test]
    fn a_via_keeps_the_widest_clearance_a_net_class_declares() {
        let board =
            board(r#"(segment (start 5 10) (end 15 10) (width 0.25) (layer "F.Cu") (net 2))"#);
        // A 0.6 mm via 0.65 mm from the track's axis: 0.65 - 0.125 - 0.3 = 0.225 mm of copper.
        let spot = (10.0, 10.65);
        let default = ViaDeclarations::new();
        let wide = ViaDeclarations {
            clearance_mm: Some(0.25),
            ..ViaDeclarations::new()
        };
        assert!((default.gap_mm() - VIA_GAP_MM).abs() < 1e-9);
        assert!((wide.gap_mm() - 0.25).abs() < 1e-9);
        assert!(Keepout::from_board(&board, "GND", default.gap_mm()).clear(spot, 0.3));
        assert!(!Keepout::from_board(&board, "GND", wide.gap_mm()).clear(spot, 0.3));
        // A class asking for less than the fixed gap does not loosen it.
        let narrow = ViaDeclarations {
            clearance_mm: Some(0.127),
            ..ViaDeclarations::new()
        };
        assert!((narrow.gap_mm() - VIA_GAP_MM).abs() < 1e-9);
    }

    /// Two ground nets in two classes with different vias, and an island on
    /// each with no via of its own: the repair gives every island the via its
    /// own class declares, not the first ground net's.
    #[test]
    fn each_island_via_takes_the_via_its_own_netclass_declares() {
        let pour = |net: u32, name: &str, y0: u32| {
            let y1 = y0 + 20;
            format!(
                r#"(segment (start 25 {y}) (end 30 {y}) (width 0.25) (layer "F.Cu") (net {net}))
                (zone (net {net}) (net_name "{name}") (layer "F.Cu") (fill yes)
                  (polygon (pts (xy 0 {y0}) (xy 40 {y0}) (xy 40 {y1}) (xy 0 {y1})))
                  (filled_polygon (layer "F.Cu") (pts (xy 0 {y0}) (xy 20 {y0}) (xy 20 {y1}) (xy 0 {y1})))
                  (filled_polygon (layer "F.Cu") (pts (xy 25 {y0}) (xy 35 {y0}) (xy 35 {y1}) (xy 25 {y1}))))
                (zone (net {net}) (net_name "{name}") (layer "B.Cu") (fill yes)
                  (polygon (pts (xy 0 {y0}) (xy 40 {y0}) (xy 40 {y1}) (xy 0 {y1})))
                  (filled_polygon (layer "B.Cu") (pts (xy 0 {y0}) (xy 40 {y0}) (xy 40 {y1}) (xy 0 {y1}))))"#,
                y = y0 + 7
            )
        };
        let text = format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
              (net 0 "")
              (net 1 "GND_A")
              (net 2 "GND_B")
              {}
              {})"#,
            pour(1, "GND_A", 0),
            pour(2, "GND_B", 30),
        );
        let path = scratch_file("two-classes", &text);
        let project = r#"{"net_settings": {
              "classes": [
                {"name": "Default", "via_diameter": 0.6, "via_drill": 0.2},
                {"name": "CLASS_A", "via_diameter": 0.6, "via_drill": 0.3},
                {"name": "CLASS_B", "via_diameter": 0.8, "via_drill": 0.4}],
              "netclass_patterns": [
                {"netclass": "CLASS_A", "pattern": "GND_A"},
                {"netclass": "CLASS_B", "pattern": "GND_B"}]}}"#;
        std::fs::write(path.with_extension("kicad_pro"), project).expect("write the project");
        let ground = Ground::new(&["GND_A".to_string(), "GND_B".to_string()]);
        let mut fills = 0;
        let outcome = tie_islands_with(&path, &ground, &mut |_| {
            fills += 1;
            Ok((fills == 1).then(|| text.clone()))
        })
        .expect("the repair runs");
        assert_eq!(outcome.added, 2, "one via for each island: {outcome:?}");
        let tied = PcbBoard::parse(&std::fs::read_to_string(&path).expect("read the tied board"))
            .expect("the tied board parses");
        let sized = |net: u32| -> Vec<(i64, i64)> {
            tied.vias
                .iter()
                .filter(|via| via.net == Some(net))
                .map(|via| (via.size_nm, via.drill_nm))
                .collect()
        };
        assert_eq!(sized(1), vec![(600_000, 300_000)], "GND_A takes CLASS_A");
        assert_eq!(sized(2), vec![(800_000, 400_000)], "GND_B takes CLASS_B");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("kicad_pro"));
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
        let plan = plan_ties(&board, &ViaDeclarations::new());
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "the search took {:?}",
            started.elapsed()
        );
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn a_via_in_a_pad_of_its_net_is_moved_off_it_with_the_track_that_meets_it() {
        // A ground via 0.025 mm outside a ground pad's edge, with a track to a far pad.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (footprint "J" (at 10 10) (layer "F.Cu")
            (pad "1" thru_hole rect (at 0 0) (size 1.8 1.8) (drill 0.8) (layers "*.Cu") (net 1 "GND")))
          (via (at 9.0 10.0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (segment (start 9.0 10.0) (end 4.0 10.0) (width 0.25) (layer "F.Cu") (net 1))
        )"#;
        let dir = std::env::temp_dir().join(format!("via-repair-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(move_vias_off_pads(&path), Ok(1));
        let moved = PcbBoard::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let via = &moved.vias[0];
        let pad = moved.pads().next().unwrap();
        assert!(
            pad.signed_distance_nm(via.at) > via.drill_nm as f64 / 2.0,
            "the via is off the pad"
        );
        let seg = &moved.segments[0];
        assert!(
            (seg.start.x_mm() - via.at.x_mm()).hypot(seg.start.y_mm() - via.at.y_mm()) < 0.01,
            "the track still meets the via"
        );
        assert_eq!(moved.footprints[0].pads.len(), 1, "nothing else changed");
        // A second pass finds nothing to move.
        assert_eq!(move_vias_off_pads(&path), Ok(0));
    }

    #[test]
    fn a_via_with_no_room_to_move_is_dropped_when_a_neighbour_of_its_net_covers_for_it() {
        // The via sits on a ground pad that a ground track also reaches, boxed in
        // by another net's pads, with a second ground via 1.5 mm away that is not
        // in any pad.
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "GND") (net 2 "SIG")
          (footprint "J" (at 10 10) (layer "F.Cu")
            (pad "1" smd rect (at 0 0) (size 1.0 1.0) (layers "F.Cu") (net 1 "GND"))
            (pad "2" smd rect (at 0 1.6) (size 3.0 1.2) (layers "F.Cu") (net 2 "SIG"))
            (pad "3" smd rect (at 0 -1.6) (size 3.0 1.2) (layers "F.Cu") (net 2 "SIG"))
            (pad "4" smd rect (at 1.6 0) (size 1.2 3.0) (layers "F.Cu") (net 2 "SIG"))
            (pad "5" smd rect (at -1.6 0) (size 1.2 3.0) (layers "F.Cu") (net 2 "SIG")))
          (via (at 10.0 10.0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (via (at 11.5 12.5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (segment (start 10.45 10.4) (end 10.45 13.0) (width 0.25) (layer "F.Cu") (net 1))
        )"#;
        let dir = std::env::temp_dir().join(format!("via-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(move_vias_off_pads(&path), Ok(1));
        let after = PcbBoard::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after.vias.len(), 1, "the covered-for via is gone");
        assert!(
            (after.vias[0].at.x_mm() - 11.5).abs() < 0.01,
            "the other stays"
        );
    }

    /// A surface pad whose only link was the via in it stays joined: the via moves
    /// off the pad and a short track keeps the pad connected to it.
    #[test]
    fn a_pad_whose_via_moves_off_it_stays_joined_to_the_via() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "SIG")
          (footprint "C" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 0.6 0.6) (layers "F.Cu") (net 1 "SIG")))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (segment (start 10 10) (end 4 10) (width 0.25) (layer "B.Cu") (net 1))
        )"#;
        let dir = std::env::temp_dir().join(format!("stub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(move_vias_off_pads(&path), Ok(1));
        let after = PcbBoard::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let via = &after.vias[0];
        assert!(
            after.segments.iter().any(|segment| {
                segment.layer == "F.Cu"
                    && near(segment.start, (10.0, 10.0))
                    && near(segment.end, (via.at.x_mm(), via.at.y_mm()))
            }),
            "a track on the pad's layer joins the pad centre to the moved via"
        );
    }

    /// A stub track must keep off another net's via as it keeps off its pads and
    /// tracks: one drawn across a via on the pad's layer is a short. The via being
    /// moved is where the stub starts, so it is the one via the track ignores.
    #[test]
    fn a_stub_keeps_off_another_nets_via_but_not_off_the_one_it_starts_from() {
        let board = PcbBoard::parse(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
              (net 0 "") (net 1 "SIG") (net 2 "OTHER")
              (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
              (via (at 8 8) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1)))"#,
        )
        .expect("parses");
        let keepout = Keepout::from_board(&board, "SIG", VIA_GAP_MM);
        assert!(
            !keepout.track_clear((5.1, 5.0), STUB_WIDTH_MM / 2.0, (8.0, 8.0)),
            "a track through another net's via is a short"
        );
        assert!(
            keepout.track_clear((8.0, 8.0), STUB_WIDTH_MM / 2.0, (8.0, 8.0)),
            "the via being moved is where the stub starts"
        );
    }

    /// A via that is a surface pad's only link is not "spare": with no spot to move
    /// to it stays, since deleting it opens the pad and nothing else joins it.
    #[test]
    fn a_surface_pads_only_via_is_kept_when_it_has_nowhere_to_go() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "SIG")
          (footprint "C" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 0.6 0.6) (layers "F.Cu") (net 1 "SIG")))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (via (at 12.5 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (keepout (tracks allowed) (vias not_allowed) (pads allowed) (copperpour allowed))
            (polygon (pts (xy 6 6) (xy 14 6) (xy 14 14) (xy 6 14)))))"#;
        let dir = std::env::temp_dir().join(format!("only-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(move_vias_off_pads(&path), Ok(0));
        let after = PcbBoard::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after.vias.len(), 2, "the pad's only link stays");
    }

    /// The copper to board edge a `.kicad_dru` states is read for the return via's
    /// distance from the outline, the rule for the board and not the NPTH one.
    #[test]
    fn the_declared_edge_clearance_is_read_from_the_boards_design_rules() {
        let dir = std::env::temp_dir().join(format!("edge-rule-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(
            dir.join("board.kicad_dru"),
            "(version 1)\n(rule \"manufacturer copper to board edge\"\n  (constraint edge_clearance (min 0.45mm)))\n(rule \"manufacturer copper to NPTH hole\"\n  (constraint edge_clearance (min 0.33mm)))\n",
        )
        .unwrap();
        assert_eq!(declared_edge_clearance_mm(&path), Some(0.45));
        assert_eq!(
            declared_edge_clearance_mm(&dir.join("none.kicad_pcb")),
            None
        );
        assert!((ViaDeclarations::new().edge_gap_mm() - DEFAULT_EDGE_CLEARANCE_MM).abs() < 1e-9);
    }

    /// A pad tie keeps the in-pad via's own gap, as a return via in a pad does: the
    /// board's 0.127 mm clearance lets a track 0.39 mm from the ball's centre stay,
    /// where the fixed 0.2 mm gap would refuse the pad.
    #[test]
    fn a_pad_tie_keeps_the_in_pad_vias_own_gap_not_the_fixed_one() {
        let board = lone_pad_board(
            r#"(segment (start 9 10.39) (end 11 10.39) (width 0.1) (layer "F.Cu") (net 2))"#,
        );
        let class = ViaDeclarations {
            clearance_mm: Some(0.127),
            hole_clearance_mm: Some(0.2),
            ..ViaDeclarations::new()
        };
        let plan = plan_pad_ties(&board, IN_PAD, &declared(), &class);
        assert_eq!(plan.vias.len(), 1, "{plan:?}");
        let fixed = plan_pad_ties(&board, IN_PAD, &declared(), &ViaDeclarations::new());
        assert!(
            fixed.vias.is_empty(),
            "no declared gap, the fixed one holds: {fixed:?}"
        );
    }

    /// A ground pad over a void in the plane takes no return via: it would join no
    /// ground copper there, though it would count for the signal vias after it.
    #[test]
    fn a_return_via_does_not_go_in_a_ground_pad_over_a_void_in_the_plane() {
        let text = |pour: &str| {
            format!(
                r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal))
              (net 0 "") (net 1 "GND") (net 2 "SIG")
              (footprint "C" (at 0 0) (layer "F.Cu") (property "Reference" "U1")
                (pad "1" smd rect (at 10.7 10.5) (size 0.5 0.5) (layers "F.Cu") (net 1)))
              (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
              (zone (net 1) (net_name "GND") (layer "In1.Cu") (fill yes)
                (polygon (pts {pour}))
                (filled_polygon (layer "In1.Cu") (pts {pour}))))"#
            )
        };
        let ground = Ground::default()
            .with_in_pad_via(350_000, 200_000)
            .with_underpad(&["U1".to_string()]);
        let place = |pour: &str| {
            let board = PcbBoard::parse(&text(pour)).expect("parses");
            stitch(&board, REACH, &[], &ground, &ViaDeclarations::new()).vias
        };
        let under = place("(xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)");
        assert_eq!(under.len(), 1, "{under:?}");
        assert_eq!(at_mm(&under[0]), (10.7, 10.5), "the plane is under the pad");
        let void = place("(xy 0 0) (xy 5 0) (xy 5 20) (xy 0 20)");
        assert!(
            void.is_empty(),
            "the plane is nowhere near the pad: {void:?}"
        );
    }

    /// A track that meets the via's copper off its centre would be left behind by a
    /// move, so that via is not moved.
    #[test]
    fn a_via_touched_by_a_track_off_its_centre_is_not_moved() {
        let text = r#"(kicad_pcb
          (layers (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "B.Cu" signal))
          (net 0 "") (net 1 "SIG")
          (footprint "C" (at 0 0) (layer "F.Cu")
            (pad "1" smd rect (at 10 10) (size 0.6 0.6) (layers "F.Cu") (net 1 "SIG")))
          (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
          (segment (start 4 10) (end 9.8 9.8) (width 0.25) (layer "In1.Cu") (net 1))
        )"#;
        let dir = std::env::temp_dir().join(format!("offcentre-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        assert_eq!(move_vias_off_pads(&path), Ok(0));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    /// An island that holds a pad or a track end is connected when a chain of its
    /// own net's tracks leads from it to the main pour through copper that lies
    /// in no pour: KiCad counts that as connected, so no tie via is wanted.
    #[test]
    fn an_island_reached_by_a_track_through_the_gap_is_not_tied_again() {
        // Two tracks meet at (22.5, 7), in the gap between the pours: one ends in
        // the island, the other in the main pour.
        let linked = board(
            r#"(segment (start 25 7) (end 22.5 7) (width 0.25) (layer "F.Cu") (net 1))
               (segment (start 22.5 7) (end 15 7) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(
            islands_to_tie(&linked).is_empty(),
            "the island is already tied through the tracks"
        );
        assert!(
            !islands_to_tie(&board("")).is_empty(),
            "the same island without them is not"
        );
    }

    /// A track that crosses a pour with both ends outside it is joined to it in
    /// KiCad, and so is whatever the track leads to.
    #[test]
    fn a_track_crossing_the_island_with_both_ends_outside_it_joins_it() {
        // (22, 3) lies in the gap and (38, 3) beyond the island's far edge; the
        // body crosses the island at x 25 to 35. A second track leads from the
        // gap to the main pour.
        let crossing = board(
            r#"(segment (start 22 3) (end 38 3) (width 0.25) (layer "F.Cu") (net 1))
               (segment (start 22 3) (end 15 3) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(islands_to_tie(&crossing).is_empty());
        assert!(!islands_to_tie(&board("")).is_empty());
    }

    /// Track copper has width: a track whose end stops short of the island's own
    /// track by less than their half widths overlaps it, though the centre
    /// lines never meet.
    #[test]
    fn tracks_joined_by_their_width_not_their_centre_lines_are_connected() {
        // The island's track starts at x 25; this one ends at 24.9, 0.1 away
        // centre to centre and 0.025 inside each other's copper.
        let linked =
            board(r#"(segment (start 24.9 7) (end 15 7) (width 0.25) (layer "F.Cu") (net 1))"#);
        assert!(islands_to_tie(&linked).is_empty());
    }

    /// A track that stops at the edge of a pad is on it, and the pad's own copper
    /// leads on to the island.
    #[test]
    fn a_track_ending_at_a_pad_edge_is_joined_through_the_pad() {
        // The pad spans x 22.5 to 24.9, ending 0.1 short of the island's track;
        // the track from the main pour reaches the pad's left edge.
        let linked = board(
            r#"(segment (start 22.5 7) (end 15 7) (width 0.25) (layer "F.Cu") (net 1))
               (footprint "R" (at 0 0) (layer "F.Cu")
                 (pad "1" smd rect (at 23.7 7) (size 2.4 1) (layers "F.Cu") (net 1)))"#,
        );
        assert!(islands_to_tie(&linked).is_empty());
    }

    /// Two vias that overlap join the tracks on their two sides, on a board whose
    /// back pour does not reach the gap.
    #[test]
    fn a_via_chain_joins_the_island_to_the_main_pour() {
        let text = |extra: &str| {
            format!(
                r#"(kicad_pcb
                  (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
                  (net 0 "")
                  (net 1 "GND")
                  (segment (start 25 7) (end 30 7) (width 0.25) (layer "F.Cu") (net 1))
                  (via (at 5 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
                  {extra}
                  (zone (net 1) (net_name "GND") (layer "F.Cu") (fill yes)
                    (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                    (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))
                    (filled_polygon (layer "F.Cu") (pts (xy 25 0) (xy 35 0) (xy 35 20) (xy 25 20))))
                  (zone (net 1) (net_name "GND") (layer "B.Cu") (fill yes)
                    (polygon (pts (xy 0 0) (xy 40 0) (xy 40 20) (xy 0 20)))
                    (filled_polygon (layer "B.Cu") (pts (xy 0 0) (xy 20 0) (xy 20 20) (xy 0 20)))))
                "#
            )
        };
        let chain = PcbBoard::parse(&text(
            r#"(segment (start 22 12) (end 15 12) (width 0.25) (layer "F.Cu") (net 1))
               (via (at 22 12) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
               (via (at 22.4 12) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
               (segment (start 22.4 12) (end 26 12) (width 0.25) (layer "F.Cu") (net 1))"#,
        ))
        .expect("parses");
        assert!(islands_to_tie(&chain).is_empty());
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

    /// A pad whose copper overlaps the edge of a pour joins it though its centre
    /// is outside: the centre-point test left it unattached.
    #[test]
    fn a_pad_overlapping_a_pour_edge_with_its_centre_outside_joins_the_pour() {
        // The pad spans x 19.8 to 22.2 on F.Cu, 0.2 mm into the main pour (x to
        // 20) with its centre at x 21 in the gap. A track leads from the pad into
        // the island (x 25 to 35).
        let linked = board(
            r#"(footprint "R" (at 0 0) (layer "F.Cu")
                 (pad "1" smd rect (at 21 12) (size 2.4 1) (layers "F.Cu") (net 1)))
               (segment (start 22 12) (end 27 12) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(
            islands_to_tie(&linked).is_empty(),
            "the island reaches the main pour through the pad"
        );
        let apart = board(
            r#"(footprint "R" (at 0 0) (layer "F.Cu")
                 (pad "1" smd rect (at 23 12) (size 1.2 1) (layers "F.Cu") (net 1)))
               (segment (start 23.5 12) (end 27 12) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(
            !islands_to_tie(&apart).is_empty(),
            "a pad clear of the main pour joins only the island"
        );
    }

    /// The same for a via: its pad overlaps the pour's corner, its centre does not.
    #[test]
    fn a_via_overlapping_a_pour_edge_with_its_centre_outside_joins_the_pour() {
        // The via at (20.5, 21) is 2.4 mm across, 1.118 mm from the main pour's
        // corner at (20, 20). A track leads from it into the island.
        let linked = board(
            r#"(via (at 20.5 21) (size 2.4) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
               (segment (start 20.5 21) (end 26 19.5) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(islands_to_tie(&linked).is_empty());
        let apart = board(
            r#"(via (at 20.5 24) (size 2.4) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
               (segment (start 20.5 24) (end 26 19.5) (width 0.25) (layer "F.Cu") (net 1))"#,
        );
        assert!(
            !islands_to_tie(&apart).is_empty(),
            "a via clear of every pour's edge joins nothing"
        );
    }

    /// The segment-to-pad distance is exact, not read off points along the track.
    #[test]
    fn a_segment_crossing_a_pad_corner_is_zero_away_however_short_the_overlap() {
        let pad = PadBox {
            at: (0.0, 0.0),
            half: (0.5, 0.5),
            cos: 1.0,
            sin: 0.0,
        };
        // The line y = x - 0.99 clips the corner (0.5, -0.5) over 0.014 mm, far
        // less than the 0.05 mm a sampled walk steps by.
        let a = (0.2, -0.79);
        let b = (1.7, 0.71);
        assert_eq!(pad.segment_distance(a, b), 0.0);
        // The same line moved 0.01 mm clear of the corner (y = x - 1.01) misses
        // the pad by 0.01 / sqrt(2) mm.
        let clear = pad.segment_distance((0.2, -0.81), (1.7, 0.69));
        assert!((clear - 0.01 / 2.0_f64.sqrt()).abs() < 1e-9, "{clear}");
        // And it agrees with a fine brute-force walk on turned pads.
        let turned = PadBox {
            at: (1.0, 2.0),
            half: (0.4, 0.15),
            cos: 0.6,
            sin: 0.8,
        };
        for (p, q) in [
            ((0.0, 0.0), (3.0, 3.0)),
            ((0.2, 2.5), (1.9, 1.1)),
            ((-1.0, 2.0), (-0.2, 2.0)),
            ((1.0, 2.0), (1.0, 2.0)),
        ] {
            let steps = 20_000;
            let walked = (0..=steps)
                .map(|k| {
                    let t = f64::from(k) / f64::from(steps);
                    turned.distance((p.0 + t * (q.0 - p.0), p.1 + t * (q.1 - p.1)))
                })
                .fold(f64::MAX, f64::min);
            let exact = turned.segment_distance(p, q);
            assert!(exact <= walked + 1e-9, "exact {exact} walked {walked}");
            assert!(walked - exact < 1e-3, "exact {exact} walked {walked}");
        }
    }

    /// A large net is joined in time that follows its size: twelve thousand vias
    /// on a grid, each touching its neighbours, would take minutes compared pair
    /// by pair.
    #[test]
    #[allow(clippy::cast_precision_loss)] // grid indices are small
    fn a_large_net_is_joined_in_time_that_follows_its_size() {
        let (columns, rows) = (200_usize, 60_usize);
        let pieces: Vec<Piece> = (0..columns * rows)
            .map(|k| Piece {
                layers: vec!["F.Cu".to_string()],
                shape: PieceShape::Via(
                    ((k % columns) as f64 * 0.5, (k / columns) as f64 * 0.5),
                    0.3,
                ),
                regions: Vec::new(),
            })
            .collect();
        let mut groups = Groups(Vec::new());
        let started = std::time::Instant::now();
        join_through_copper(&mut groups, &pieces);
        let elapsed = started.elapsed();
        let root = groups.find(0);
        assert!(
            (0..pieces.len()).all(|k| groups.find(k) == root),
            "every via touches its neighbour, so the net is one piece"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "joining {} pieces took {elapsed:?}",
            pieces.len()
        );
    }
    /// A board with a 3 mm track of net 1 that ends in a via at (0, 0), a pad of
    /// net 1 at (10, 0) and a pour of net 1 over (20..30, -5..5), and `extra`.
    fn stub_text(extra: &str) -> String {
        format!(
            r#"(kicad_pcb
              (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
              (net 0 "")
              (net 1 "RAIL")
              (footprint "R" (layer "F.Cu") (at 10 0)
                (pad "1" smd rect (at 0 0) (size 1.0 1.0) (layers "F.Cu") (net 1 "RAIL")))
              (via (at 0 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
              (segment (start 0 0) (end 3 0) (width 0.4) (layer "F.Cu") (net 1))
              (zone (net 1) (net_name "RAIL") (layer "F.Cu") (fill yes)
                (polygon (pts (xy 20 -5) (xy 30 -5) (xy 30 5) (xy 20 5))))
              {extra}
            )"#
        )
    }

    /// The segments left in `text` after the prune, and how many it dropped.
    fn pruned(text: &str, name: &str) -> (usize, usize) {
        let dir = std::env::temp_dir().join(format!("stub-prune-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.kicad_pcb");
        std::fs::write(&path, text).unwrap();
        let dropped = drop_dangling_stubs(&path).unwrap();
        let after = PcbBoard::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        (dropped, after.segments.len())
    }

    /// A sliver shorter than its own width that leaves a junction and ends in nothing
    /// carries nothing and is dropped; the track it hangs from stays.
    #[test]
    fn a_sliver_of_track_that_ends_in_nothing_is_dropped() {
        let text =
            stub_text(r#"(segment (start 3 0) (end 3 0.07) (width 0.4) (layer "F.Cu") (net 1))"#);
        assert_eq!(pruned(&text, "sliver"), (1, 1));
    }

    /// An end in a via, a pad or a pour of the net is joined to it, so the track
    /// is not a stub, however short.
    #[test]
    fn a_short_track_ending_in_a_via_a_pad_or_a_pour_is_kept() {
        for (name, extra) in [
            (
                "via",
                r#"(segment (start 3 0) (end 3 0.3) (width 0.4) (layer "F.Cu") (net 1))
                   (via (at 3 0.3) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))"#,
            ),
            (
                "pad",
                r#"(segment (start 3 0) (end 9.8 0) (width 0.4) (layer "F.Cu") (net 1))
                   (segment (start 9.8 0) (end 9.85 0) (width 0.4) (layer "F.Cu") (net 1))"#,
            ),
            (
                "pour",
                r#"(segment (start 3 0) (end 19.95 0) (width 0.4) (layer "F.Cu") (net 1))
                   (segment (start 19.95 0) (end 20.2 0) (width 0.4) (layer "F.Cu") (net 1))"#,
            ),
        ] {
            let text = stub_text(extra);
            let before = PcbBoard::parse(&text).unwrap().segments.len();
            assert_eq!(pruned(&text, name), (0, before), "{name}");
        }
    }

    /// A track longer than it is wide is a branch of the router's, not a sliver,
    /// and so is a track with copper at both ends.
    #[test]
    fn a_long_dead_end_and_a_track_between_two_junctions_are_kept() {
        let long =
            stub_text(r#"(segment (start 3 0) (end 3 2) (width 0.4) (layer "F.Cu") (net 1))"#);
        assert_eq!(pruned(&long, "long"), (0, 2));
        let bridge = stub_text(
            r#"(segment (start 3 0) (end 3 0.2) (width 0.4) (layer "F.Cu") (net 1))
               (segment (start 3 0.2) (end 0 0.2) (width 0.4) (layer "F.Cu") (net 1))"#,
        );
        assert_eq!(pruned(&bridge, "bridge"), (0, 3));
    }
}
