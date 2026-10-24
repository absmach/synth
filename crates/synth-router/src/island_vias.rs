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

use std::collections::BTreeMap;
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

/// A via the pass wants added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TieVia {
    pub net: String,
    pub at: Point,
    pub size_nm: i64,
    pub drill_nm: i64,
}

/// Fill the candidate's zones, tie every island, and write the vias into the
/// candidate. Returns how many vias were added.
///
/// The candidate is left unfilled: the delivery step fills it again once it has
/// been validated. On any failure it is left as it was.
///
/// # Errors
/// Returns why the board could not be filled, read or written.
pub fn tie_islands(candidate: &Path) -> Result<usize, String> {
    let mut added = 0;
    for _ in 0..MAX_ROUNDS {
        let Some(filled) = crate::zone_fill::filled_text(candidate)? else {
            return Ok(added);
        };
        let board = PcbBoard::parse(&filled).map_err(|e| e.to_string())?;
        let vias = islands_to_tie(&board);
        if vias.is_empty() {
            return Ok(added);
        }
        let text = std::fs::read_to_string(candidate)
            .map_err(|e| format!("could not read {}: {e}", candidate.display()))?;
        let tied = with_vias(&text, &board, &vias)?;
        PcbBoard::parse(&tied).map_err(|e| format!("the tied board does not parse: {e}"))?;
        std::fs::write(candidate, tied)
            .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
        added += vias.len();
    }
    Ok(added)
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

/// The vias that would tie each unjoined, copper-bearing island of every
/// poured net to that net's main pour. `board` must carry fills.
#[must_use]
pub fn islands_to_tie(board: &PcbBoard) -> Vec<TieVia> {
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

    let copper = board.copper_layers();
    let mut out = Vec::new();
    for (net, regions) in &regions {
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

        let (size_nm, drill_nm) = usual_via(&via_sizes);
        let need = nm_to_mm(size_nm) / 2.0 + EDGE_SLACK_MM;

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
            let mains: Vec<usize> = (0..regions.len())
                .filter(|&i| groups.find(i) == main)
                .collect();
            if let Some(at) = best_spot(regions, &members, &mains, need) {
                out.push(TieVia { at, ..via.clone() });
            }
        }
    }
    out
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
fn best_spot(regions: &[Region], island: &[usize], main: &[usize], need: f64) -> Option<Point> {
    let mut best: Option<(f64, (f64, f64))> = None;
    for &a in island {
        let a = &regions[a];
        let step = ((a.max.0 - a.min.0) * (a.max.1 - a.min.1) / 40_000.0)
            .sqrt()
            // A small island has few points to try, and a coarse grid can step
            // over the one narrow window that holds the via.
            .max(0.02);
        let mut y = a.min.1 + need;
        while y <= a.max.1 - need {
            let mut x = a.min.0 + need;
            while x <= a.max.0 - need {
                let p = (x, y);
                if a.contains(p) {
                    let from_a = a.edge_distance(p);
                    if from_a >= need - EPS && best.is_none_or(|(margin, _)| from_a > margin) {
                        let from_main = main
                            .iter()
                            .map(|&b| &regions[b])
                            .filter(|b| b.layer != a.layer && b.contains(p))
                            .map(|b| b.edge_distance(p))
                            .fold(f64::MIN, f64::max);
                        let margin = from_a.min(from_main);
                        if margin >= need - EPS && best.is_none_or(|(m, _)| margin > m) {
                            best = Some((margin, p));
                        }
                    }
                }
                x += step;
            }
            y += step;
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
    let numbered = text.contains("\n\t(net 0 ") || text.contains("\n  (net 0 ");
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
            format!("\"{}\"", via.net)
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
        let text = format!(
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
        );
        PcbBoard::parse(&text).expect("board must parse")
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
        assert!(islands_to_tie(&board).is_empty());
    }

    #[test]
    fn an_island_too_small_for_the_via_is_left_alone() {
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
        assert!(islands_to_tie(&board).is_empty());
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
