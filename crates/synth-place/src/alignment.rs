// SPDX-License-Identifier: Apache-2.0

//! Alignment of like parts for the placement review.
//!
//! Resistors of one value laid in a row, a column of decoupling capacitors, a
//! row of indicator LEDs: a reader expects like parts that sit side by side to
//! share a line. This module finds them by structure alone, with no part
//! named. Parts of one part id that lie close together form a cluster; inside
//! it, three or more whose centres sit within [`BAND_MM`] of one line, and
//! that run along it, form a row or a column. A member that is off the
//! line by more than [`ALIGN_TOLERANCE_MM`] is reported with the coordinate
//! that would put it on the line.

use std::collections::HashMap;

use synth_geometry::{nm_to_mm, Point};
use synth_ir::{Board, ComponentId};

use crate::Placement;

/// Parts of one id closer than this, centre to centre, belong to one cluster.
const CLUSTER_MM: f64 = 10.0;

/// Centres within this of each other, across a line, can be one row or column.
const BAND_MM: f64 = 1.0;

/// How far off the shared line a part may sit before it is reported.
pub(crate) const ALIGN_TOLERANCE_MM: f64 = 0.05;

/// A part with more pins than this is an IC, not a like part to line up.
const SMALL_PART_PINS: usize = 5;

/// A part off the line its row or column shares.
#[derive(Debug, Clone)]
pub(crate) struct Misaligned {
    pub id: ComponentId,
    /// True for a row (the parts share a y), false for a column (a shared x).
    pub row: bool,
    /// How far off the line the part sits.
    pub off_mm: f64,
    /// The coordinate the shared line has: y for a row, x for a column.
    pub line_mm: f64,
    /// The members of the row or column, the part included.
    pub members: Vec<ComponentId>,
}

fn key_of(board: &Board, id: ComponentId) -> Option<String> {
    let component = board.component(id)?;
    if matches!(
        component.kind.as_str(),
        "connector" | "jack" | "mounting_hole" | "test_point"
    ) {
        return None;
    }
    let part = component.part.as_ref()?;
    (part.pins.len() <= SMALL_PART_PINS).then(|| format!("{:?}", part.id))
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
        root = parent[root];
    }
    let mut at = i;
    while parent[at] != root {
        let next = parent[at];
        parent[at] = root;
        at = next;
    }
    root
}

fn mm(p: Point) -> (f64, f64) {
    (nm_to_mm(p.x_nm), nm_to_mm(p.y_nm))
}

/// Like parts that sit side by side but not on one line, each with the
/// coordinate that would align it.
pub(crate) fn misaligned_like_parts(board: &Board, placement: &Placement) -> Vec<Misaligned> {
    let mut by_key: HashMap<String, Vec<(ComponentId, (f64, f64))>> = HashMap::new();
    for p in &placement.components {
        if let Some(key) = key_of(board, p.id) {
            by_key.entry(key).or_default().push((p.id, mm(p.center)));
        }
    }
    let mut found = Vec::new();
    for members in by_key.values() {
        let mut parent: Vec<usize> = (0..members.len()).collect();
        for (i, a) in members.iter().enumerate() {
            for (j, b) in members.iter().enumerate().skip(i + 1) {
                if (a.1 .0 - b.1 .0).hypot(a.1 .1 - b.1 .1) <= CLUSTER_MM {
                    let (x, y) = (find(&mut parent, i), find(&mut parent, j));
                    parent[x] = y;
                }
            }
        }
        let mut clusters: HashMap<usize, Vec<(ComponentId, (f64, f64))>> = HashMap::new();
        for (i, member) in members.iter().enumerate() {
            let root = find(&mut parent, i);
            clusters.entry(root).or_default().push(*member);
        }
        for cluster in clusters.values().filter(|c| c.len() >= 3) {
            for row in [true, false] {
                found.extend(bands(cluster, row));
            }
        }
    }
    found.sort_by(|a, b| b.off_mm.total_cmp(&a.off_mm));
    found
}

/// The rows (or columns) of one cluster, with the parts that miss their line.
fn bands(cluster: &[(ComponentId, (f64, f64))], row: bool) -> Vec<Misaligned> {
    // `across` is the coordinate the line fixes, `along` the one it runs over.
    let across = |m: &(ComponentId, (f64, f64))| if row { m.1 .1 } else { m.1 .0 };
    let along = |m: &(ComponentId, (f64, f64))| if row { m.1 .0 } else { m.1 .1 };
    let mut sorted: Vec<_> = cluster.to_vec();
    sorted.sort_by(|a, b| across(a).total_cmp(&across(b)));
    let mut out = Vec::new();
    let mut start = 0;
    while start < sorted.len() {
        let mut end = start + 1;
        while end < sorted.len() && across(&sorted[end]) - across(&sorted[end - 1]) <= BAND_MM {
            end += 1;
        }
        let band = &sorted[start..end];
        start = end;
        if band.len() < 3 {
            continue;
        }
        let (lo, hi) = band.iter().fold((f64::MAX, f64::MIN), |(lo, hi), m| {
            (lo.min(along(m)), hi.max(along(m)))
        });
        let spread = band.iter().map(across).fold(f64::MIN, f64::max)
            - band.iter().map(across).fold(f64::MAX, f64::min);
        // A line only if the parts run along it, not stack across it.
        if hi - lo < 3.0 * spread.max(ALIGN_TOLERANCE_MM) {
            continue;
        }
        let mut values: Vec<f64> = band.iter().map(across).collect();
        values.sort_by(f64::total_cmp);
        let line = values[values.len() / 2];
        for m in band {
            let off = (across(m) - line).abs();
            if off > ALIGN_TOLERANCE_MM {
                out.push(Misaligned {
                    id: m.0,
                    row,
                    off_mm: off,
                    line_mm: line,
                    members: band.iter().map(|b| b.0).collect(),
                });
            }
        }
    }
    out
}

/// Centre-to-centre distance past which a small part counts as stranded from
/// every part it shares a signal with.
pub(crate) const ISOLATION_MM: f64 = 8.0;

/// A small part far from every part it is wired to.
#[derive(Debug, Clone)]
pub(crate) struct Isolated {
    pub id: ComponentId,
    /// The nearest part it shares a signal net with.
    pub neighbour: ComponentId,
    pub mm: f64,
}

/// Small parts whose every signal-net neighbour is farther than
/// [`ISOLATION_MM`]: a pull-up, a gate resistor or an indicator that drifted
/// away from the circuit it belongs to. Rails are ignored, they reach
/// everywhere.
pub(crate) fn isolated_parts(board: &Board, placement: &Placement) -> Vec<Isolated> {
    let domains = synth_ir::infer_power_domains(board);
    let centres: HashMap<ComponentId, (f64, f64)> = placement
        .components
        .iter()
        .map(|p| (p.id, mm(p.center)))
        .collect();
    let mut neighbours: HashMap<ComponentId, Vec<ComponentId>> = HashMap::new();
    for net in board
        .nets
        .iter()
        .filter(|n| !crate::repeats::is_rail(board, &domains, n))
    {
        for a in &net.endpoints {
            for b in &net.endpoints {
                if a.component != b.component {
                    neighbours.entry(a.component).or_default().push(b.component);
                }
            }
        }
    }
    let mut found = Vec::new();
    for (id, near) in &neighbours {
        if key_of(board, *id).is_none() {
            continue;
        }
        let Some(here) = centres.get(id) else {
            continue;
        };
        let nearest = near
            .iter()
            .filter_map(|n| {
                let c = centres.get(n)?;
                Some((n, (here.0 - c.0).hypot(here.1 - c.1)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((neighbour, mm)) = nearest {
            if mm > ISOLATION_MM {
                found.push(Isolated {
                    id: *id,
                    neighbour: *neighbour,
                    mm,
                });
            }
        }
    }
    found.sort_by(|a, b| b.mm.total_cmp(&a.mm));
    found
}

/// Centre-to-centre distance past which a capacitor on a supply rail counts as
/// serving nothing near it.
pub(crate) const RAIL_CAP_MM: f64 = 12.0;

/// A capacitor between a supply rail and ground that sits far from every part
/// that draws on the rail.
///
/// The parts that count as served are the integrated circuits (more pins than
/// a small part) and inductors on the rail: a regulator's output capacitor
/// stands by its inductor, a decoupling capacitor by its IC. Rails reach
/// everywhere, so [`isolated_parts`] cannot see this: a capacitor wired only
/// to rails has no signal neighbour. The nearest part it should stand beside
/// is returned.
pub(crate) fn rail_capacitors_far(board: &Board, placement: &Placement) -> Vec<Isolated> {
    let domains = synth_ir::infer_power_domains(board);
    let centres: HashMap<ComponentId, (f64, f64)> = placement
        .components
        .iter()
        .map(|p| (p.id, mm(p.center)))
        .collect();
    let serves = |id: ComponentId| {
        board.component(id).is_some_and(|c| {
            c.kind.as_str() == "inductor"
                || c.part
                    .as_ref()
                    .is_some_and(|p| p.pins.len() > SMALL_PART_PINS)
        })
    };
    let is_ground = |net: &synth_ir::Net| net.name.to_lowercase().contains("gnd");
    let mut found = Vec::new();
    for component in &board.components {
        let id = component.id;
        if component.kind.as_str() != "capacitor" {
            continue;
        }
        let Some(here) = centres.get(&id) else {
            continue;
        };
        let nets: Vec<&synth_ir::Net> = board
            .nets
            .iter()
            .filter(|n| n.endpoints.iter().any(|e| e.component == id))
            .collect();
        let rail = nets
            .iter()
            .find(|n| crate::repeats::is_rail(board, &domains, n) && !is_ground(n));
        if nets.len() != 2 || !nets.iter().any(|n| is_ground(n)) {
            continue;
        }
        let Some(rail) = rail else { continue };
        let nearest = rail
            .endpoints
            .iter()
            .map(|e| e.component)
            .filter(|c| *c != id && serves(*c))
            .filter_map(|c| {
                let at = centres.get(&c)?;
                Some((c, (here.0 - at.0).hypot(here.1 - at.1)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((neighbour, mm)) = nearest {
            if mm > RAIL_CAP_MM {
                found.push(Isolated { id, neighbour, mm });
            }
        }
    }
    found.sort_by(|a, b| b.mm.total_cmp(&a.mm));
    found
}
