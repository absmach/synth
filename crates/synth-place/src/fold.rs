// SPDX-License-Identifier: Apache-2.0

//! Fold each declared group into a compact block, then scan a fresh outline.
//!
//! The group rule leaves a group as a ring of parts around its chip: compact
//! enough for the review, but its box still spans the board and the 2 mm
//! group gap then pushes the whole arrangement apart. This stage re-lays each
//! group's non-connector, non-hole parts in rows (shelf or skyline) at the
//! 1.3 mm part spacing, tries the row, column and chip-aware orders, and
//! keeps the block with the highest in-group ring, then the smallest box.
//!
//! The blocks are then packed into a *fresh* outline instead of the one the
//! previous stage happened to leave. For a candidate width `W` the group's
//! connectors lie in rows on the four walls, the folded blocks pack the
//! interior with a 2 mm gap, and `H` is scanned from the minimum until the
//! layout fits. The smallest `W x H` the scan finds is kept, with the A/B
//! attachment of each connector row and the wall choice of each unit refined
//! by a greedy descent. Wall units turn their parts toward their wall, so a
//! block on the left wall is rotated a quarter turn, one on the top wall half
//! a turn. Mounting holes are seated in the new outline's corners at the
//! offsets they held in the input, and a corner hole stays out of both the
//! folded block and the review-group box.
//!
//! The stage is all-or-nothing: the caller keeps the input unless the judged
//! result is smaller and every measured quality row is at least as good.

use std::collections::{BTreeMap, BTreeSet};

use synth_geometry::{Point, Rect, Rotation, mm_to_nm};
use synth_ir::{Board, PlacementEdge};
use synth_registry::MatingFace;

use crate::floorplan::{BoardEdge, rotation_for_mating_edge};
use crate::judge::{PlacementJudgement, judge};
use crate::{Placement, sidecar_courtyard_rect};

/// Spacing between the parts of a folded block.
const PART_GAP_MM: f64 = 1.3;
/// Least gap between two packed blocks.
const BLOCK_GAP_MM: f64 = 2.0;
/// Clearance between a wall row and a corner hole's courtyard.
const HOLE_CLEAR_MM: f64 = 0.5;
/// A hinted connector this close to its named edge is still flush enough for
/// the fold's candidate comparison; the export rim is tighter than this.
const HINT_FLUSH_MM: f64 = 2.0;
/// Alignment the fold may give up to lay connectors along their wall.
const ALIGN_SLACK: f64 = 0.05;
/// Width/height a folded block aims for; only breaks ties between blocks of
/// the same ring count and area.
const TARGET_ASPECT: f64 = 1.4;
/// A capacitor inside this courtyard gap rings its group's chip.
const RING_MM: f64 = 3.0;
/// Two passives whose centres share an axis this closely read as one row.
const ALIGN_MM: f64 = 0.5;
/// A mounting hole this close to a board corner is a corner hole.
pub(crate) const HOLE_CORNER_LIMIT_MM: f64 = 5.5;
/// A part with at least this many pins is the group's major IC.
const MAJOR_IC_PINS: usize = 16;
/// Smallest candidate width, before the walls ask for more.
const MIN_WIDTH_MM: f64 = 20.0;
/// How far past the minimum the width scan goes.
const SCAN_WIDTH_EXTRA_MM: f64 = 80.0;
/// Height scan start and step count; `H` grows 1 mm at a time.
const MIN_HEIGHT_MM: f64 = 20.0;
const SCAN_HEIGHT_STEPS: usize = 160;
/// The height scan steps 1 mm; shave the last fraction at this step.
const SHAVE_STEP_MM: f64 = 0.05;
const SHAVE_STEPS: usize = 20;
/// A USB courtyard must sit this close to the wall's midpoint.
const USB_CENTRE_MM: f64 = 0.05;

const CHIP_KINDS: [&str; 5] = ["ic", "mcu", "sensor", "regulator", "memory"];
/// Kinds the judge counts as passives when it measures alignment.
const PASSIVE_KINDS: [&str; 4] = ["resistor", "capacitor", "inductor", "crystal"];

/// Wall ids, in the order the spike votes and scans them.
const TOP: u8 = 0;
const RIGHT: u8 = 1;
const BOTTOM: u8 = 2;
const LEFT: u8 = 3;

/// One part the fold may move, with the courtyard it has now.
#[derive(Clone, Debug)]
struct Piece {
    /// Index into [`Placement::components`].
    slot: usize,
    refdes: String,
    kind: String,
    part: String,
    rect: Rect,
    /// Current rotation in quarter turns.
    quarter: u8,
    /// The footprint's declared mating side, in the unrotated frame.
    mating: Option<MatingFace>,
    /// A connector's edge hint, as a wall id.
    hint: Option<u8>,
    pins: usize,
    group: Option<String>,
}

impl Piece {
    fn width(&self) -> i64 {
        self.rect.width_nm()
    }

    fn height(&self) -> i64 {
        self.rect.height_nm()
    }

    fn centre(&self) -> (i64, i64) {
        (
            self.rect.min.x_nm + self.width() / 2,
            self.rect.min.y_nm + self.height() / 2,
        )
    }

    fn is_usb(&self) -> bool {
        self.refdes.to_ascii_lowercase().contains("usb") || self.part.contains("usb")
    }
}

/// A part's top-left corner inside a folded block.
#[derive(Clone, Debug)]
struct Slot {
    index: usize,
    x: i64,
    y: i64,
}

/// A block laid out from (0,0) with y increasing away from the top edge.
#[derive(Clone, Debug)]
struct Folded {
    width: i64,
    height: i64,
    slots: Vec<Slot>,
}

/// One planned unit: a group's connector row, optionally with its block.
#[derive(Clone, Debug)]
struct Unit {
    /// Group name; the descent keys its A/B and wall choices on this.
    name: String,
    /// Wall the unit sits on, or `None` for a block packed in the interior.
    wall: Option<u8>,
    /// Extent along the wall (or width when free).
    u: i64,
    /// Extent inward from the wall (or height when free).
    v: i64,
    /// Every part's courtyard centre in the unit's local frame, and the quarter
    /// turns that put it there.
    local: Vec<(usize, i64, i64, u8)>,
    /// The group's USB connectors; the row centres on the first one.
    usb_conns: Vec<usize>,
    /// Where the unit's connectors sat along the input wall; sorts the row.
    centre_along: i64,
    /// A group with a connector choice (arrangement A or B).
    has_choice: bool,
    ab: Option<char>,
    /// Some connector has an edge hint, so its wall is not the descent's.
    hinted: bool,
}

/// A mounting hole and the input corner it held.
#[derive(Clone, Debug)]
struct Hole {
    slot: usize,
    /// 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
    corner: u8,
    dx: i64,
    dy: i64,
    hw: i64,
    hh: i64,
}

/// Hole clearances on the four sides of the candidate outline.
#[derive(Clone, Copy, Debug, Default)]
struct Margins {
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
}

/// A complete candidate: `w x h`, wall rows and interior blocks.
#[derive(Clone, Debug)]
struct Layout {
    w: i64,
    h: i64,
    walled: Vec<(usize, Rect)>,
    /// `(unit, rect, quarter turn)`; the turn packs a tall block sideways.
    free: Vec<(usize, Rect, u8)>,
    /// Which walls a connector row sits on; those sides export flush.
    sides: [bool; 4],
}

/// The units and the fixed geometry a scan works against.
struct Scene<'a> {
    units: &'a [Unit],
    holes: &'a [Hole],
    margins: Margins,
    block_gap: i64,
    /// The exporter's perimeter margin on a side with no docked connector.
    export_margin: i64,
}

/// Fold the declared groups and scan a fresh outline.
///
/// Returns `false` without touching `placement` when there is nothing to fold
/// or the scan does not fit. The caller decides whether to keep the result.
pub(crate) fn fold_groups_pack(board: &Board, placement: &mut Placement) -> bool {
    // A declared outline is a contract; the scan sizes an auto-fitted board.
    if board.outline.is_some() {
        return false;
    }
    let outline = placement.board_outline;
    if outline.is_empty() {
        return false;
    }
    let Some(pieces) = build_pieces(board, placement) else {
        return false;
    };
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for piece in &pieces {
        if piece.kind == "mounting_hole" {
            continue;
        }
        let Some(group) = &piece.group else {
            // An ungrouped part has no block to join; leave the board alone.
            return false;
        };
        groups.entry(group.clone()).or_default().push(piece.slot);
    }
    if groups.is_empty() {
        return false;
    }
    let (holes, margins) = hole_plan(&pieces, outline);
    let export_margin = mm_to_nm(crate::tight_edge_margin_mm(board));
    let Some((layout, units)) =
        best_layout(&pieces, &groups, outline, &holes, margins, export_margin)
    else {
        return false;
    };
    apply_layout(placement, &pieces, &units, &layout, &holes);
    true
}

/// Read the placed parts into the fold's own pieces.
fn build_pieces(board: &Board, placement: &Placement) -> Option<Vec<Piece>> {
    let mut pieces = Vec::with_capacity(placement.components.len());
    for (slot, placed) in placement.components.iter().enumerate() {
        let component = board.component(placed.id)?;
        let rect = sidecar_courtyard_rect(board, component, placed);
        let hint = component
            .placement_hint
            .as_ref()
            .and_then(|hint| hint.edge.as_ref())
            .map(|edge| match edge {
                PlacementEdge::Top => TOP,
                PlacementEdge::Right => RIGHT,
                PlacementEdge::Bottom => BOTTOM,
                PlacementEdge::Left => LEFT,
            });
        pieces.push(Piece {
            slot,
            refdes: component.refdes.clone(),
            kind: component.kind.clone(),
            part: component
                .part
                .as_ref()
                .map_or_else(String::new, |part| part.id.0.to_ascii_lowercase()),
            rect,
            quarter: (placed.rotation.degrees() / 90).rem_euclid(4) as u8,
            mating: component
                .part
                .as_ref()
                .and_then(|part| part.footprint_dimensions.as_ref())
                .and_then(|dimensions| dimensions.mating_face),
            hint,
            pins: component.part.as_ref().map_or(0, |part| part.pins.len()),
            group: component.group.clone(),
        });
    }
    Some(pieces)
}

/// Seat every mounting hole in the input corner nearest it, and report the
/// clearances those corners need on the new outline.
fn hole_plan(pieces: &[Piece], outline: Rect) -> (Vec<Hole>, Margins) {
    let mut holes = Vec::new();
    let mut margins = Margins::default();
    let clear = mm_to_nm(HOLE_CLEAR_MM);
    for piece in pieces.iter().filter(|piece| piece.kind == "mounting_hole") {
        let (cx, cy) = piece.centre();
        let left = cx - outline.min.x_nm <= outline.max.x_nm - cx;
        let top = cy - outline.min.y_nm <= outline.max.y_nm - cy;
        let corner = match (top, left) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        };
        let dx = if left {
            (piece.rect.min.x_nm - outline.min.x_nm).max(0)
        } else {
            (outline.max.x_nm - piece.rect.max.x_nm).max(0)
        };
        let dy = if top {
            (piece.rect.min.y_nm - outline.min.y_nm).max(0)
        } else {
            (outline.max.y_nm - piece.rect.max.y_nm).max(0)
        };
        let (hw, hh) = (piece.width(), piece.height());
        if left {
            margins.left = margins.left.max(dx + hw + clear);
        } else {
            margins.right = margins.right.max(dx + hw + clear);
        }
        if top {
            margins.top = margins.top.max(dy + hh + clear);
        } else {
            margins.bottom = margins.bottom.max(dy + hh + clear);
        }
        holes.push(Hole {
            slot: piece.slot,
            corner,
            dx,
            dy,
            hw,
            hh,
        });
    }
    (holes, margins)
}

/// Build one unit per group, honouring the connector hints and the descent's
/// arrangement and wall overrides.
fn make_units(
    pieces: &[Piece],
    groups: &BTreeMap<String, Vec<usize>>,
    outline: Rect,
    options: &BTreeMap<String, char>,
    wall_over: &BTreeMap<String, u8>,
) -> Vec<Unit> {
    let part_gap = mm_to_nm(PART_GAP_MM);
    let mut units = Vec::new();
    for (name, slots) in groups {
        let members: Vec<&Piece> = slots
            .iter()
            .map(|&slot| &pieces[slot])
            .filter(|piece| piece.kind != "mounting_hole")
            .collect();
        if members.is_empty() {
            continue;
        }
        let mut conns: Vec<&Piece> = members
            .iter()
            .copied()
            .filter(|piece| is_wall_connector(&piece.kind))
            .collect();
        conns.sort_by_key(|piece| piece.refdes.clone());
        let rest: Vec<&Piece> = members
            .iter()
            .copied()
            .filter(|piece| !is_wall_connector(&piece.kind))
            .collect();
        let folded = (!rest.is_empty()).then(|| {
            let owned: Vec<Piece> = rest.iter().map(|piece| (*piece).clone()).collect();
            fold_block(&owned, part_gap)
        });
        let block = folded.as_ref().map(|folded| {
            folded
                .slots
                .iter()
                .map(|slot| {
                    let piece = &pieces[slot.index];
                    (
                        slot.index,
                        slot.x + piece.width() / 2,
                        slot.y + piece.height() / 2,
                    )
                })
                .collect::<Vec<_>>()
        });
        let (fw, fh) = folded
            .as_ref()
            .map_or((0, 0), |folded| (folded.width, folded.height));
        let has_major = rest.iter().any(|piece| piece.pins >= MAJOR_IC_PINS);
        let hinted = conns.iter().any(|piece| piece.hint.is_some());
        let voted = if conns.is_empty() {
            None
        } else {
            Some(connector_wall(&conns, outline))
        };
        let wall = conns
            .iter()
            .find_map(|piece| piece.hint)
            .or(voted)
            .map(|wall| wall_over.get(name).copied().unwrap_or(wall));
        let centre_along = row_centre_along(&conns, wall.unwrap_or(TOP));
        let usb_conns: Vec<usize> = conns
            .iter()
            .filter(|piece| piece.is_usb())
            .map(|piece| piece.slot)
            .collect();

        let base_turn = wall.map_or(0, wall_turn);
        let conn_turns: Vec<u8> = wall.map_or_else(
            || vec![0; conns.len()],
            |wall| {
                conns
                    .iter()
                    .map(|piece| connector_turn(piece, wall))
                    .collect()
            },
        );
        let conn_extents: Vec<(i64, i64)> = conns
            .iter()
            .zip(&conn_turns)
            .map(|(piece, &turn)| turned_extent(piece, wall.unwrap_or(BOTTOM), turn))
            .collect();
        let row_w = conn_extents.iter().map(|&(along, _)| along).sum::<i64>()
            + part_gap * (conns.len() as i64 - 1).max(0);
        let row_d = conn_extents
            .iter()
            .map(|&(_, inward)| inward)
            .max()
            .unwrap_or(0);

        if has_major {
            // The judge leaves these connectors out of the group box, so the
            // row docks on the wall and the block packs freely.
            if let Some(wall) = wall {
                let mut local = Vec::with_capacity(conns.len());
                let mut x = 0;
                for ((piece, &(along, inward)), &turn) in
                    conns.iter().zip(&conn_extents).zip(&conn_turns)
                {
                    local.push((piece.slot, x + along / 2, row_d - inward / 2, turn));
                    x += along + part_gap;
                }
                units.push(Unit {
                    name: name.clone(),
                    wall: Some(wall),
                    u: row_w,
                    v: row_d,
                    local,
                    usb_conns: usb_conns.clone(),
                    centre_along,
                    has_choice: false,
                    ab: None,
                    hinted,
                });
            }
            if let Some(block) = block {
                units.push(Unit {
                    name: name.clone(),
                    wall: None,
                    u: fw,
                    v: fh,
                    local: block
                        .into_iter()
                        .map(|(slot, x, y)| (slot, x, y, 0))
                        .collect(),
                    usb_conns: Vec::new(),
                    centre_along: 0,
                    has_choice: false,
                    ab: None,
                    hinted,
                });
            }
            continue;
        }

        let Some(wall) = wall else {
            if let Some(block) = block {
                units.push(Unit {
                    name: name.clone(),
                    wall: None,
                    u: fw,
                    v: fh,
                    local: block
                        .into_iter()
                        .map(|(slot, x, y)| (slot, x, y, 0))
                        .collect(),
                    usb_conns: Vec::new(),
                    centre_along: 0,
                    has_choice: false,
                    ab: None,
                    hinted,
                });
            }
            continue;
        };

        // Arrangement A stacks the row under the block; B puts it beside.
        let (wa, ha) = (row_w.max(fw), row_d + part_gap + fh);
        let (wb, hb) = (fw + part_gap + row_w, fh.max(row_d));
        let has_usb = conns.iter().any(|piece| piece.is_usb());
        let use_a = options.get(name).map_or_else(
            || (has_usb && wa <= wb) || (wa * ha <= wb * hb),
            |choice| *choice == 'A',
        );
        let mut local = Vec::new();
        if use_a {
            let off = (wa - fw) / 2;
            for &(slot, x, y) in block.as_deref().unwrap_or(&[]) {
                local.push((slot, off + x, y, base_turn));
            }
            let mut x = (wa - row_w) / 2;
            for ((piece, &(along, inward)), &turn) in
                conns.iter().zip(&conn_extents).zip(&conn_turns)
            {
                local.push((piece.slot, x + along / 2, ha - inward / 2, turn));
                x += along + part_gap;
            }
        } else {
            for &(slot, x, y) in block.as_deref().unwrap_or(&[]) {
                local.push((slot, x, hb - fh + y, base_turn));
            }
            let mut x = fw + part_gap;
            for ((piece, &(along, inward)), &turn) in
                conns.iter().zip(&conn_extents).zip(&conn_turns)
            {
                local.push((piece.slot, x + along / 2, hb - inward / 2, turn));
                x += along + part_gap;
            }
        }
        units.push(Unit {
            name: name.clone(),
            wall: Some(wall),
            u: if use_a { wa } else { wb },
            v: if use_a { ha } else { hb },
            local,
            usb_conns,
            centre_along,
            has_choice: true,
            ab: Some(if use_a { 'A' } else { 'B' }),
            hinted,
        });
    }
    units
}

/// The wall most of the group's connectors sit nearest.
fn connector_wall(conns: &[&Piece], outline: Rect) -> u8 {
    let mut votes = [0_usize; 4];
    for piece in conns {
        votes[nearest_wall(piece.rect, outline) as usize] += 1;
    }
    let best = votes.iter().copied().max().unwrap_or(0);
    let tied: Vec<u8> = (0..4)
        .filter(|&wall| votes[wall] == best)
        .map(|wall| wall as u8)
        .collect();
    if tied.len() == 1 {
        tied[0]
    } else {
        nearest_wall(conns[0].rect, outline)
    }
}

/// The row order along a wall: where the connectors sat, then the name.
fn row_centre_along(conns: &[&Piece], wall: u8) -> i64 {
    if conns.is_empty() {
        return 0;
    }
    let vertical = matches!(wall, LEFT | RIGHT);
    let sum: i64 = conns
        .iter()
        .map(|piece| {
            let (cx, cy) = piece.centre();
            if vertical { cy } else { cx }
        })
        .sum();
    sum / conns.len() as i64
}

/// Shelf/skyline pack `pieces` into a compact block.
fn fold_block(pieces: &[Piece], gap: i64) -> Folded {
    let orders = [order_row(pieces), order_col(pieces), order_chip(pieces)];
    let mut best: Option<((i64, i64, u8, i64, i64), Folded)> = None;
    for order in orders {
        if let Some(candidate) = evaluate(pieces, &order, gap)
            && best.as_ref().is_none_or(|current| candidate.0 < current.0)
        {
            best = Some(candidate);
        }
    }
    best.map_or_else(
        || Folded {
            width: 0,
            height: 0,
            slots: pieces
                .iter()
                .map(|piece| Slot {
                    index: piece.slot,
                    x: 0,
                    y: 0,
                })
                .collect(),
        },
        |(_, folded)| folded,
    )
}

/// Current row order: group rows by the part's y, then read left to right.
fn order_row(pieces: &[Piece]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by_key(|&index| {
        let (cx, cy) = pieces[index].centre();
        (row_bin(cy), cx, pieces[index].refdes.clone())
    });
    order
}

/// Current column order: group columns by x, then read top to bottom.
fn order_col(pieces: &[Piece]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by_key(|&index| {
        let (cx, cy) = pieces[index].centre();
        (row_bin(cx), cy, pieces[index].refdes.clone())
    });
    order
}

/// Each chip first, then the parts currently nearest it, capacitors first.
fn order_chip(pieces: &[Piece]) -> Vec<usize> {
    let mut chips: Vec<usize> = (0..pieces.len())
        .filter(|&index| CHIP_KINDS.contains(&pieces[index].kind.as_str()))
        .collect();
    chips.sort_by_key(|&index| {
        let (cx, cy) = pieces[index].centre();
        (cy, cx, pieces[index].refdes.clone())
    });
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    if chips.is_empty() {
        order.sort_by_key(|&index| {
            let (cx, cy) = pieces[index].centre();
            (cy, cx, pieces[index].refdes.clone())
        });
        return order;
    }
    order.sort_by_key(|&index| {
        let (cx, cy) = pieces[index].centre();
        let mut best = (0_usize, i64::MAX);
        for (position, &chip) in chips.iter().enumerate() {
            let (qx, qy) = pieces[chip].centre();
            let (dx, dy) = (qx - cx, qy - cy);
            let distance = dx * dx + dy * dy;
            if distance < best.1 {
                best = (position, distance);
            }
        }
        let own = usize::from(!chips.contains(&index));
        let cap_first = usize::from(pieces[index].kind != "capacitor");
        (best.0, own, cap_first, best.1, pieces[index].refdes.clone())
    });
    order
}

/// Bulk a 2 mm coordinate into a row id, matching the offline spike.
fn row_bin(nm: i64) -> i64 {
    (nm + mm_to_nm(1.0)).div_euclid(mm_to_nm(2.0))
}

/// Try every row width the part widths allow, in shelf and skyline packing.
fn evaluate(
    pieces: &[Piece],
    order: &[usize],
    gap: i64,
) -> Option<((i64, i64, u8, i64, i64), Folded)> {
    let widths: Vec<i64> = order.iter().map(|&index| pieces[index].width()).collect();
    let widest = widths.iter().copied().max().unwrap_or(0);
    let mut prefix = 0_i64;
    let mut targets: Vec<i64> = Vec::with_capacity(widths.len());
    for width in &widths {
        prefix += width + gap;
        targets.push(prefix - gap);
    }
    targets.sort_unstable();
    targets.dedup();

    let mut best: Option<((i64, i64, u8, i64, i64), Folded)> = None;
    for target in targets {
        if target < widest {
            continue;
        }
        if let Some(folded) = shelf(pieces, order, target, gap) {
            consider(&mut best, pieces, folded);
        }
        if let Some(folded) = skyline(pieces, order, target, gap) {
            consider(&mut best, pieces, folded);
        }
    }
    best
}

fn consider(
    best: &mut Option<((i64, i64, u8, i64, i64), Folded)>,
    pieces: &[Piece],
    folded: Folded,
) {
    // A packing heuristic in mm, not a coordinate: f64 is plenty.
    #[allow(clippy::cast_precision_loss)]
    let aspect = if folded.width > 0 && folded.height > 0 {
        folded.width.max(folded.height) as f64 / folded.width.min(folded.height) as f64
    } else {
        1e9
    };
    let ring = ring_count(pieces, &folded);
    let aligned = aligned_passives(pieces, &folded);
    let over = u8::from(aspect > 2.0);
    let area = folded.width * folded.height;
    let delta = ((aspect - TARGET_ASPECT) * 1000.0).round() as i64;
    let key = (-ring, -aligned, over, area, delta.abs());
    if best.as_ref().is_none_or(|current| key < current.0) {
        *best = Some((key, folded));
    }
}

/// Greedy rows no wider than `target`, each part centred vertically in its row.
fn shelf(pieces: &[Piece], order: &[usize], target: i64, gap: i64) -> Option<Folded> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut row_width = 0_i64;
    for &index in order {
        let width = pieces[index].width();
        let need = if row.is_empty() {
            width
        } else {
            row_width + gap + width
        };
        if !row.is_empty() && need > target {
            rows.push(std::mem::take(&mut row));
            row.push(index);
            row_width = width;
        } else {
            row.push(index);
            row_width = need;
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    if rows.is_empty() {
        return None;
    }
    let mut height = 0_i64;
    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 {
            height += gap;
        }
        height += row
            .iter()
            .map(|&index| pieces[index].height())
            .max()
            .unwrap_or(0);
    }
    let width = rows
        .iter()
        .map(|row| {
            row.iter().map(|&index| pieces[index].width()).sum::<i64>()
                + gap * (row.len() as i64 - 1)
        })
        .max()
        .unwrap_or(0);
    let mut slots = Vec::new();
    let mut y = 0_i64;
    for row in &rows {
        let row_height = row
            .iter()
            .map(|&index| pieces[index].height())
            .max()
            .unwrap_or(0);
        let mut x = 0_i64;
        for &index in row {
            slots.push(Slot {
                index: pieces[index].slot,
                x,
                y: y + (row_height - pieces[index].height()) / 2,
            });
            x += pieces[index].width() + gap;
        }
        y += row_height + gap;
    }
    Some(Folded {
        width,
        height,
        slots,
    })
}

/// Skyline pack with the gap kept on the right and bottom of every part.
fn skyline(pieces: &[Piece], order: &[usize], target: i64, gap: i64) -> Option<Folded> {
    let sizes: Vec<(i64, i64)> = order
        .iter()
        .map(|&index| (pieces[index].width(), pieces[index].height()))
        .collect();
    let positions = skyline_pack(&sizes, target, gap)?;
    let width = positions
        .iter()
        .zip(&sizes)
        .map(|(point, size)| point.0 + size.0)
        .max()
        .unwrap_or(0);
    let height = positions
        .iter()
        .zip(&sizes)
        .map(|(point, size)| point.1 + size.1)
        .max()
        .unwrap_or(0);
    Some(Folded {
        width,
        height,
        slots: order
            .iter()
            .zip(positions)
            .map(|(&index, (x, y))| Slot {
                index: pieces[index].slot,
                x,
                y,
            })
            .collect(),
    })
}

/// Skyline pack `sizes` into a strip of `width`; returns top-left corners.
fn skyline_pack(sizes: &[(i64, i64)], width: i64, gap: i64) -> Option<Vec<(i64, i64)>> {
    let mut skyline: Vec<(i64, i64, i64)> = vec![(0, width, 0)];
    let mut positions = Vec::with_capacity(sizes.len());
    for &(w, h) in sizes {
        let gw = w + gap;
        let gh = h + gap;
        let mut best: Option<(i64, i64)> = None;
        for (index, &(sx, _, _)) in skyline.iter().enumerate() {
            if sx + gw > width {
                continue;
            }
            let mut y = 0_i64;
            let mut j = index;
            while j < skyline.len() && skyline[j].0 < sx + gw {
                y = y.max(skyline[j].2);
                j += 1;
            }
            if best.is_none_or(|(by, bx)| (y, sx) < (by, bx)) {
                best = Some((y, sx));
            }
        }
        let (y, x) = best?;
        positions.push((x, y));
        let mut next: Vec<(i64, i64, i64)> = Vec::new();
        for &(sx, ex, sy) in &skyline {
            if ex <= x || sx >= x + gw {
                next.push((sx, ex, sy));
            } else {
                if sx < x {
                    next.push((sx, x, sy));
                }
                if ex > x + gw {
                    next.push((x + gw, ex, sy));
                }
            }
        }
        next.push((x, x + gw, y + gh));
        next.sort_unstable();
        let mut merged: Vec<(i64, i64, i64)> = Vec::new();
        for segment in next {
            if let Some(last) = merged.last_mut()
                && last.1 == segment.0
                && last.2 == segment.2
            {
                last.1 = segment.1;
                continue;
            }
            merged.push(segment);
        }
        skyline = merged
            .into_iter()
            .filter(|segment| segment.1 > segment.0)
            .collect();
    }
    Some(positions)
}

/// Build the smallest scanned layout, descending on the connector arrangement
/// and the wall choice.
fn best_layout(
    pieces: &[Piece],
    groups: &BTreeMap<String, Vec<usize>>,
    outline: Rect,
    holes: &[Hole],
    margins: Margins,
    export_margin: i64,
) -> Option<(Layout, Vec<Unit>)> {
    let mut options: BTreeMap<String, char> = BTreeMap::new();
    let mut walls: BTreeMap<String, u8> = BTreeMap::new();
    let (mut layout, mut units) = build_layout(
        pieces,
        groups,
        outline,
        holes,
        margins,
        export_margin,
        &options,
        &walls,
    )?;
    let mut best = exported_area(&layout, export_margin);

    for _cycle in 0..2 {
        let mut improved_any = false;
        for _ in 0..3 {
            let mut improved = false;
            let names: Vec<String> = units
                .iter()
                .filter(|unit| unit.has_choice)
                .map(|unit| unit.name.clone())
                .collect();
            for name in names {
                let Some(current) = units
                    .iter()
                    .find(|unit| unit.name == name && unit.has_choice)
                    .and_then(|unit| unit.ab)
                else {
                    continue;
                };
                let mut trial = options.clone();
                trial.insert(name.clone(), if current == 'A' { 'B' } else { 'A' });
                if let Some((candidate, candidate_units)) = build_layout(
                    pieces,
                    groups,
                    outline,
                    holes,
                    margins,
                    export_margin,
                    &trial,
                    &walls,
                ) {
                    let area = exported_area(&candidate, export_margin);
                    if area < best {
                        options = trial;
                        layout = candidate;
                        units = candidate_units;
                        best = area;
                        improved = true;
                        improved_any = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
        for _ in 0..2 {
            let mut improved = false;
            let mut walled: Vec<(String, u8)> = units
                .iter()
                .filter_map(|unit| {
                    let wall = unit.wall?;
                    (!unit.hinted).then(|| (unit.name.clone(), wall))
                })
                .collect();
            walled.dedup_by(|a, b| a.0 == b.0);
            for (name, current) in walled {
                for wall in [TOP, RIGHT, BOTTOM, LEFT] {
                    if wall == current {
                        continue;
                    }
                    let mut trial = walls.clone();
                    trial.insert(name.clone(), wall);
                    if let Some((candidate, candidate_units)) = build_layout(
                        pieces,
                        groups,
                        outline,
                        holes,
                        margins,
                        export_margin,
                        &options,
                        &trial,
                    ) {
                        let area = exported_area(&candidate, export_margin);
                        if area < best {
                            walls = trial;
                            layout = candidate;
                            units = candidate_units;
                            best = area;
                            improved = true;
                            improved_any = true;
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
        if !improved_any {
            break;
        }
    }
    Some((layout, units))
}

/// One candidate with the given arrangement and wall choices.
fn build_layout(
    pieces: &[Piece],
    groups: &BTreeMap<String, Vec<usize>>,
    outline: Rect,
    holes: &[Hole],
    margins: Margins,
    export_margin: i64,
    options: &BTreeMap<String, char>,
    wall_over: &BTreeMap<String, u8>,
) -> Option<(Layout, Vec<Unit>)> {
    let units = make_units(pieces, groups, outline, options, wall_over);
    if units.is_empty() {
        return None;
    }
    let scene = Scene {
        units: &units,
        holes,
        margins,
        block_gap: mm_to_nm(BLOCK_GAP_MM),
        export_margin,
    };
    let layout = scene.scan()?;
    Some((layout, units))
}

impl Scene<'_> {
    /// The units on one wall, ordered along it.
    fn wall_units(&self, wall: u8) -> Vec<usize> {
        let mut list: Vec<usize> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.wall == Some(wall))
            .map(|(index, _)| index)
            .collect();
        list.sort_by_key(|&index| {
            let unit = &self.units[index];
            (unit.centre_along, unit.name.clone())
        });
        list
    }

    fn row_len(&self, wall: u8) -> i64 {
        let units = self.wall_units(wall);
        if units.is_empty() {
            return 0;
        }
        units.iter().map(|&index| self.units[index].u).sum::<i64>()
            + self.block_gap * (units.len() as i64 - 1)
    }

    /// Where a wall row starts so its USB connector lands on the midpoint.
    fn row_start(&self, wall: u8, lo: i64, hi: i64, total: i64, mid: i64) -> i64 {
        let units = self.wall_units(wall);
        let mut prefix = 0;
        for &index in &units {
            let unit = &self.units[index];
            if let Some(&usb) = unit.usb_conns.first() {
                let lx = unit
                    .local
                    .iter()
                    .find(|&&(slot, _, _, _)| slot == usb)
                    .map_or(0, |&(_, x, _, _)| x);
                let off = if matches!(wall, TOP | RIGHT) {
                    unit.u - lx
                } else {
                    lx
                };
                return (mid - (prefix + off)).max(lo).min(hi - total);
            }
            prefix += unit.u + self.block_gap;
        }
        lo
    }

    /// Lay the wall rows for `w x h`; `None` when a row does not fit.
    fn wall_layout(&self, w: i64, h: i64) -> Option<Vec<(usize, Rect)>> {
        let top = self.wall_units(TOP);
        let bottom = self.wall_units(BOTTOM);
        let depth_top = top
            .iter()
            .map(|&index| self.units[index].v)
            .max()
            .unwrap_or(0);
        let depth_bottom = bottom
            .iter()
            .map(|&index| self.units[index].v)
            .max()
            .unwrap_or(0);
        let mut rows: Vec<(usize, Rect)> = Vec::new();

        let lo = (depth_top + self.block_gap).max(self.margins.top);
        if !self.add_row(&mut rows, LEFT, lo, h - self.margins.bottom, h / 2, w, h) {
            return None;
        }
        if !self.add_row(&mut rows, RIGHT, lo, h - self.margins.bottom, h / 2, w, h) {
            return None;
        }

        let (lo, hi) = self.clear_span(&rows, w, 0, depth_top);
        if !self.add_row(&mut rows, TOP, lo, hi, w / 2, w, h) {
            return None;
        }
        let (lo, hi) = self.clear_span(&rows, w, h - depth_bottom, h);
        if !self.add_row(&mut rows, BOTTOM, lo, hi, w / 2, w, h) {
            return None;
        }

        let holes = self.hole_rects(w, h);
        for (index, (_, rect)) in rows.iter().enumerate() {
            if rect.min.x_nm < 0 || rect.max.x_nm > w || rect.min.y_nm < 0 || rect.max.y_nm > h {
                // A wall row deeper than the board would stick out; the H
                // scan then has to grow.
                return None;
            }
            for (_, other) in rows.iter().skip(index + 1) {
                if rect_gap(*rect, *other) < self.block_gap {
                    return None;
                }
            }
            for hole in &holes {
                if rect_gap(*rect, *hole) < mm_to_nm(HOLE_CLEAR_MM) {
                    return None;
                }
            }
        }
        for &(index, rect) in &rows {
            let unit = &self.units[index];
            let wall = unit.wall?;
            for &conn in &unit.usb_conns {
                let lx = unit
                    .local
                    .iter()
                    .find(|&&(slot, _, _, _)| slot == conn)
                    .map_or(0, |&(_, x, _, _)| x);
                let ly = unit
                    .local
                    .iter()
                    .find(|&&(slot, _, _, _)| slot == conn)
                    .map_or(0, |&(_, _, y, _)| y);
                let (cx, cy) = unit_point(wall, rect, unit, lx, ly);
                let off = if matches!(wall, TOP | BOTTOM) {
                    (cx - w / 2).abs()
                } else {
                    (cy - h / 2).abs()
                };
                if off > mm_to_nm(USB_CENTRE_MM) {
                    return None;
                }
            }
        }
        Some(rows)
    }

    /// Add one wall's row; `false` when it cannot fit between `lo` and `hi`.
    fn add_row(
        &self,
        rows: &mut Vec<(usize, Rect)>,
        wall: u8,
        lo: i64,
        hi: i64,
        mid: i64,
        w: i64,
        h: i64,
    ) -> bool {
        let units = self.wall_units(wall);
        if units.is_empty() {
            return true;
        }
        let total = units.iter().map(|&index| self.units[index].u).sum::<i64>()
            + self.block_gap * (units.len() as i64 - 1);
        if total > hi - lo {
            return false;
        }
        let mut cursor = self.row_start(wall, lo, hi, total, mid);
        for index in units {
            let unit = &self.units[index];
            let rect = match wall {
                TOP => Rect::new(Point::new(cursor, 0), Point::new(cursor + unit.u, unit.v)),
                RIGHT => Rect::new(
                    Point::new(w - unit.v, cursor),
                    Point::new(w, cursor + unit.u),
                ),
                BOTTOM => Rect::new(
                    Point::new(cursor, h - unit.v),
                    Point::new(cursor + unit.u, h),
                ),
                _ => Rect::new(Point::new(0, cursor), Point::new(unit.v, cursor + unit.u)),
            };
            rows.push((index, rect));
            cursor += unit.u + self.block_gap;
        }
        true
    }

    /// The width left clear of the vertical rows between `y0` and `y1`.
    fn clear_span(&self, rows: &[(usize, Rect)], w: i64, y0: i64, y1: i64) -> (i64, i64) {
        let mut lo = self.margins.left;
        let mut hi = w - self.margins.right;
        for &(index, rect) in rows {
            match self.units[index].wall {
                Some(LEFT)
                    if y0 < rect.max.y_nm + self.block_gap
                        && rect.min.y_nm < y1 + self.block_gap =>
                {
                    lo = lo.max(rect.max.x_nm + self.block_gap);
                }
                Some(RIGHT)
                    if y0 < rect.max.y_nm + self.block_gap
                        && rect.min.y_nm < y1 + self.block_gap =>
                {
                    hi = hi.min(rect.min.x_nm - self.block_gap);
                }
                _ => {}
            }
        }
        (lo, hi)
    }

    fn hole_rects(&self, w: i64, h: i64) -> Vec<Rect> {
        self.holes
            .iter()
            .map(|hole| {
                let (x0, y0) = hole_origin(hole, w, h);
                Rect::new(Point::new(x0, y0), Point::new(x0 + hole.hw, y0 + hole.hh))
            })
            .collect()
    }

    /// The area the exporter would give this layout: a side with no docked
    /// connector row keeps the perimeter margin, a side with one is flush.
    fn exported_area(&self, layout: &Layout) -> i64 {
        exported_area(layout, self.export_margin)
    }

    /// The smallest layout for one width and a height scanned from the minimum.
    fn try_layout(&self, w: i64) -> Option<Layout> {
        let mut h = mm_to_nm(MIN_HEIGHT_MM);
        for _ in 0..SCAN_HEIGHT_STEPS {
            if let Some(layout) = self.fit(w, h) {
                return Some(layout);
            }
            h += mm_to_nm(1.0);
        }
        None
    }

    /// Wall rows plus the interior blocks at exactly `w x h`.
    fn fit(&self, w: i64, h: i64) -> Option<Layout> {
        let walled = self.wall_layout(w, h)?;
        let mut obstacles: Vec<Rect> = walled.iter().map(|&(_, rect)| rect).collect();
        obstacles.extend(self.hole_rects(w, h));
        let free_units: Vec<usize> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.wall.is_none())
            .map(|(index, _)| index)
            .collect();
        let free = self.pack_free(&free_units, &obstacles, w, h)?;
        let mut sides = [false; 4];
        for &(index, _) in &walled {
            if let Some(wall) = self.units[index].wall {
                sides[wall as usize] = true;
            }
        }
        Some(Layout {
            w,
            h,
            walled,
            free,
            sides,
        })
    }

    /// Greedy first-fit of the interior blocks beside the obstacles.
    fn pack_free(
        &self,
        free: &[usize],
        obstacles: &[Rect],
        w: i64,
        h: i64,
    ) -> Option<Vec<(usize, Rect, u8)>> {
        let mut order = free.to_vec();
        order.sort_by(|&a, &b| {
            let (ua, ub) = (&self.units[a], &self.units[b]);
            (ub.u * ub.v)
                .cmp(&(ua.u * ua.v))
                .then_with(|| ub.v.cmp(&ua.v))
                .then_with(|| ua.name.cmp(&ub.name))
        });
        let mut occupied: Vec<Rect> = obstacles.to_vec();
        let mut placed = Vec::with_capacity(order.len());
        for index in order {
            let unit = &self.units[index];
            let (bw, bh, turn) = if unit.u >= unit.v {
                (unit.u, unit.v, 0)
            } else {
                (unit.v, unit.u, 1)
            };
            let mut xs: BTreeSet<i64> = BTreeSet::new();
            let mut ys: BTreeSet<i64> = BTreeSet::new();
            xs.insert(0);
            ys.insert(0);
            for rect in &occupied {
                xs.insert(rect.max.x_nm + self.block_gap);
                xs.insert(rect.min.x_nm - self.block_gap - bw);
                ys.insert(rect.max.y_nm + self.block_gap);
                ys.insert(rect.min.y_nm - self.block_gap - bh);
            }
            let mut chosen = None;
            'place: for &y in &ys {
                if y < 0 || y + bh > h {
                    continue;
                }
                for &x in &xs {
                    if x < 0 || x + bw > w {
                        continue;
                    }
                    let rect = Rect::new(Point::new(x, y), Point::new(x + bw, y + bh));
                    if occupied
                        .iter()
                        .all(|other| rect_gap(rect, *other) >= self.block_gap)
                    {
                        chosen = Some((x, y));
                        break 'place;
                    }
                }
            }
            let (x, y) = chosen?;
            let rect = Rect::new(Point::new(x, y), Point::new(x + bw, y + bh));
            placed.push((index, rect, turn));
            occupied.push(rect);
        }
        Some(placed)
    }

    /// Scan the width from the minimum and keep the smallest area that fits.
    fn scan(&self) -> Option<Layout> {
        let top = self.row_len(TOP);
        let bottom = self.row_len(BOTTOM);
        let interior = self
            .units
            .iter()
            .filter(|unit| unit.wall.is_none())
            .map(|unit| unit.u.min(unit.v))
            .max()
            .unwrap_or(0);
        let min_w = (self.margins.left + self.margins.right + top)
            .max(self.margins.left + self.margins.right + bottom)
            .max(interior + 2 * self.block_gap)
            .max(mm_to_nm(MIN_WIDTH_MM));

        let mut best: Option<Layout> = None;
        let limit = min_w + mm_to_nm(SCAN_WIDTH_EXTRA_MM);
        let mut w = min_w;
        while w <= limit {
            if let Some(layout) = self.try_layout(w)
                && best
                    .as_ref()
                    .is_none_or(|current| self.exported_area(&layout) < self.exported_area(current))
            {
                best = Some(layout);
            }
            w += mm_to_nm(1.0);
        }
        let mut best = best?;
        for _ in 0..SHAVE_STEPS {
            let h = best.h - mm_to_nm(SHAVE_STEP_MM);
            if h <= 0 {
                break;
            }
            let Some(candidate) = self.fit(best.w, h) else {
                break;
            };
            best = candidate;
        }
        Some(best)
    }
}

/// The exporter's area for a layout: a side with no connector row keeps the
/// perimeter margin, a side with one exports flush.
fn exported_area(layout: &Layout, margin: i64) -> i64 {
    let w = layout.w
        + if layout.sides[LEFT as usize] {
            0
        } else {
            margin
        }
        + if layout.sides[RIGHT as usize] {
            0
        } else {
            margin
        };
    let h = layout.h
        + if layout.sides[TOP as usize] {
            0
        } else {
            margin
        }
        + if layout.sides[BOTTOM as usize] {
            0
        } else {
            margin
        };
    w * h
}

/// A wall unit's local point in board coordinates.
fn unit_point(wall: u8, rect: Rect, unit: &Unit, lx: i64, ly: i64) -> (i64, i64) {
    match wall {
        TOP => (rect.min.x_nm + unit.u - lx, rect.min.y_nm + unit.v - ly),
        RIGHT => (rect.min.x_nm + ly, rect.min.y_nm + unit.u - lx),
        BOTTOM => (rect.min.x_nm + lx, rect.min.y_nm + ly),
        _ => (rect.min.x_nm + unit.v - ly, rect.min.y_nm + lx),
    }
}

/// Quarter turns that put a unit's local frame on `wall`.
fn wall_turn(wall: u8) -> u8 {
    match wall {
        TOP => 2,
        RIGHT => 1,
        BOTTOM => 0,
        _ => 3,
    }
}

fn wall_edge(wall: u8) -> BoardEdge {
    match wall {
        TOP => BoardEdge::Top,
        RIGHT => BoardEdge::Right,
        BOTTOM => BoardEdge::Bottom,
        _ => BoardEdge::Left,
    }
}

/// The additional quarter turns that lay a connector along `wall`.
///
/// A declared mating side wins: the opening faces out of the wall. A plain
/// header turns only when its long axis is across the wall, so a header
/// already along the edge keeps the dock's orientation.
fn connector_turn(piece: &Piece, wall: u8) -> u8 {
    if let Some(face) = piece.mating {
        let desired =
            (rotation_for_mating_edge(face, wall_edge(wall)).degrees() / 90).rem_euclid(4) as u8;
        return (desired + 4 - piece.quarter) % 4;
    }
    let base = wall_turn(wall);
    let (along, inward) = turned_extent(piece, wall, base);
    if along >= inward {
        base
    } else {
        (base + 1) % 4
    }
}

/// A part's `(along the wall, inward)` extent after `turn` further quarters.
fn turned_extent(piece: &Piece, wall: u8, turn: u8) -> (i64, i64) {
    let (w, h) = (piece.width(), piece.height());
    let (bw, bh) = if turn.is_multiple_of(2) {
        (w, h)
    } else {
        (h, w)
    };
    if matches!(wall, TOP | BOTTOM) {
        (bw, bh)
    } else {
        (bh, bw)
    }
}

/// A hole's top-left corner in the new outline.
fn hole_origin(hole: &Hole, w: i64, h: i64) -> (i64, i64) {
    let left = matches!(hole.corner, 0 | 2);
    let top = matches!(hole.corner, 0 | 1);
    let x0 = if left { hole.dx } else { w - hole.dx - hole.hw };
    let y0 = if top { hole.dy } else { h - hole.dy - hole.hh };
    (x0, y0)
}

/// Write the chosen layout into the placement.
fn apply_layout(
    placement: &mut Placement,
    pieces: &[Piece],
    units: &[Unit],
    layout: &Layout,
    holes: &[Hole],
) {
    for &(index, rect) in &layout.walled {
        let unit = &units[index];
        let Some(wall) = unit.wall else {
            continue;
        };
        for &(slot, lx, ly, turn) in &unit.local {
            let (x, y) = unit_point(wall, rect, unit, lx, ly);
            set_piece(placement, pieces, slot, x, y, turn);
        }
    }
    for &(index, rect, turn) in &layout.free {
        let unit = &units[index];
        for &(slot, lx, ly, _) in &unit.local {
            let (x, y) = if turn == 1 {
                (rect.min.x_nm + ly, rect.min.y_nm + unit.u - lx)
            } else {
                (rect.min.x_nm + lx, rect.min.y_nm + ly)
            };
            set_piece(placement, pieces, slot, x, y, turn);
        }
    }
    for hole in holes {
        let (x0, y0) = hole_origin(hole, layout.w, layout.h);
        placement.components[hole.slot].center = Point::new(x0 + hole.hw / 2, y0 + hole.hh / 2);
    }
    placement.board_outline = Rect::new(Point::new(0, 0), Point::new(layout.w, layout.h));
}

/// Put a part's courtyard centre at `(x, y)`, turned `turn` quarters further.
fn set_piece(placement: &mut Placement, pieces: &[Piece], slot: usize, x: i64, y: i64, turn: u8) {
    let piece = &pieces[slot];
    let placed = &mut placement.components[slot];
    placed.center = Point::new(x, y);
    placed.rotation = quarter_rotation((piece.quarter + turn) % 4);
}

fn quarter_rotation(quarter: u8) -> Rotation {
    match quarter {
        1 => Rotation::Ninety,
        2 => Rotation::OneEighty,
        3 => Rotation::TwoSeventy,
        _ => Rotation::Zero,
    }
}

/// Nearest board edge of a courtyard: 0 top, 1 right, 2 bottom, 3 left.
fn nearest_wall(rect: Rect, outline: Rect) -> u8 {
    [
        (rect.min.y_nm - outline.min.y_nm, TOP),
        (outline.max.x_nm - rect.max.x_nm, RIGHT),
        (outline.max.y_nm - rect.max.y_nm, BOTTOM),
        (rect.min.x_nm - outline.min.x_nm, LEFT),
    ]
    .into_iter()
    .min_by_key(|(distance, _)| *distance)
    .map_or(TOP, |(_, wall)| wall)
}

/// How many passives share a row or column with a peer inside the block.
fn aligned_passives(pieces: &[Piece], folded: &Folded) -> i64 {
    let limit = mm_to_nm(ALIGN_MM);
    let passives: Vec<Rect> = folded
        .slots
        .iter()
        .filter_map(|slot| {
            let piece = pieces
                .iter()
                .find(|piece| piece.slot == slot.index)
                .expect("a slot names a block part");
            PASSIVE_KINDS.contains(&piece.kind.as_str()).then(|| {
                Rect::new(
                    Point::new(slot.x, slot.y),
                    Point::new(slot.x + piece.width(), slot.y + piece.height()),
                )
            })
        })
        .collect();
    passives
        .iter()
        .enumerate()
        .filter(|&(index, rect)| {
            let (ax, ay) = (
                rect.min.x_nm + rect.width_nm() / 2,
                rect.min.y_nm + rect.height_nm() / 2,
            );
            passives.iter().enumerate().any(|(other, peer)| {
                other != index && {
                    let (bx, by) = (
                        peer.min.x_nm + peer.width_nm() / 2,
                        peer.min.y_nm + peer.height_nm() / 2,
                    );
                    (ax - bx).abs() <= limit || (ay - by).abs() <= limit
                }
            })
        })
        .count() as i64
}

fn ring_count(pieces: &[Piece], folded: &Folded) -> i64 {
    let mut chips: Vec<Rect> = Vec::new();
    let mut caps: Vec<Rect> = Vec::new();
    for slot in &folded.slots {
        let piece = pieces
            .iter()
            .find(|piece| piece.slot == slot.index)
            .expect("a slot names a block part");
        let rect = Rect::new(
            Point::new(slot.x, slot.y),
            Point::new(slot.x + piece.width(), slot.y + piece.height()),
        );
        if CHIP_KINDS.contains(&piece.kind.as_str()) {
            chips.push(rect);
        } else if piece.kind == "capacitor" {
            caps.push(rect);
        }
    }
    if chips.is_empty() {
        return 0;
    }
    let limit = mm_to_nm(RING_MM);
    caps.iter()
        .filter(|rect| chips.iter().any(|chip| rect_gap(**rect, *chip) <= limit))
        .count() as i64
}

/// Chebyshev gap between two courtyards; negative when they overlap.
pub(crate) fn rect_gap(a: Rect, b: Rect) -> i64 {
    (b.min.x_nm - a.max.x_nm)
        .max(a.min.x_nm - b.max.x_nm)
        .max(b.min.y_nm - a.max.y_nm)
        .max(a.min.y_nm - b.max.y_nm)
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect::new(
        Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
        Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
    )
}

/// A part that docks on a wall: a connector or a jack.
fn is_wall_connector(kind: &str) -> bool {
    matches!(kind, "connector" | "jack")
}

/// Whether a part joins its group's review box.
///
/// A connector leaves the box when its group also holds a major part, because
/// a docked connector sits on its own edge. A mounting hole on a board corner
/// leaves too: its corner is the board's, not the group's, and its distance
/// from the group would otherwise span the board. The rules and the judge
/// both call this, so the settle and the score always draw the same box.
pub(crate) fn joins_group_box(kind: &str, has_major: bool, rect: Rect, outline: Rect) -> bool {
    if has_major && is_wall_connector(kind) {
        return false;
    }
    !(kind == "mounting_hole" && corner_seated(rect, outline))
}

/// The review-group box: the union of the members that join it.
pub(crate) fn group_box<'a>(
    members: impl IntoIterator<Item = (&'a str, Rect)>,
    has_major: bool,
    outline: Rect,
) -> Option<Rect> {
    members
        .into_iter()
        .filter(|(kind, rect)| joins_group_box(kind, has_major, *rect, outline))
        .map(|(_, rect)| rect)
        .reduce(union)
}

/// Whether a courtyard sits on one of the board's corners.
pub(crate) fn corner_seated(rect: Rect, outline: Rect) -> bool {
    let cx = i64::midpoint(rect.min.x_nm, rect.max.x_nm);
    let cy = i64::midpoint(rect.min.y_nm, rect.max.y_nm);
    let limit = mm_to_nm(HOLE_CORNER_LIMIT_MM);
    [
        (outline.min.x_nm, outline.min.y_nm),
        (outline.max.x_nm, outline.min.y_nm),
        (outline.min.x_nm, outline.max.y_nm),
        (outline.max.x_nm, outline.max.y_nm),
    ]
    .iter()
    .any(|&(x, y)| {
        let (dx, dy) = (cx - x, cy - y);
        dx * dx + dy * dy <= limit * limit
    })
}

/// Judge rows this stage compares.
struct Rows {
    area: f64,
    usb: Option<f64>,
    dock: Option<f64>,
    dock_failures: usize,
    alignment: Option<f64>,
    ring: Option<f64>,
    group_gap: Option<f64>,
    group_failures: usize,
    overlaps: usize,
}

impl Rows {
    fn of(report: &PlacementJudgement) -> Self {
        let metric = |key: &str| report.metrics.iter().find(|row| row.key == key);
        let measured = |key: &str| metric(key).map(|row| row.measured);
        let failures = |key: &str| metric(key).map_or(0, |row| row.failures);
        Self {
            area: measured("outline_area").unwrap_or(f64::MAX),
            usb: measured("usb_offset"),
            dock: measured("docking"),
            dock_failures: failures("docking"),
            alignment: measured("alignment"),
            ring: measured("ring"),
            group_gap: measured("group_gap"),
            group_failures: failures("group_gap"),
            overlaps: failures("overlaps"),
        }
    }
}

/// The acceptance rule: smaller, and no measured row worse than the input.
fn judged_better(before: &Rows, after: &Rows) -> bool {
    after.area < before.area - 1e-6 && metrics_not_worse(before, after)
}

/// Every measured row is at least as good, and there is no overlap.
fn metrics_not_worse(before: &Rows, after: &Rows) -> bool {
    !went_up(after.usb, before.usb)
        && !went_down(after.dock, before.dock)
        && after.dock_failures <= before.dock_failures
        && !went_down(after.alignment.map(|a| a + ALIGN_SLACK), before.alignment)
        && !went_down(after.ring, before.ring)
        && !went_down(after.group_gap, before.group_gap)
        && after.group_failures <= before.group_failures
        && after.overlaps == 0
        && after.overlaps <= before.overlaps
}

/// `after` fell below `before` beyond tolerance (a regression where higher is
/// better).
fn went_down(after: Option<f64>, before: Option<f64>) -> bool {
    match (after, before) {
        (Some(after), Some(before)) => after < before - 1e-6,
        (None, Some(_)) => true,
        _ => false,
    }
}

/// `after` rose above `before` beyond tolerance (a regression where lower is
/// better).
fn went_up(after: Option<f64>, before: Option<f64>) -> bool {
    match (after, before) {
        (Some(after), Some(before)) => after > before + 1e-6,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Keep the folded result only when the judge says it is not worse.
///
/// Both sides are fitted at the same rim, so the comparison is the outline
/// each would export, not the stale outline the stage started from.
pub(crate) fn fold_is_better(
    board: &Board,
    before: &Placement,
    after: &mut Placement,
    rim: i64,
) -> bool {
    let mut before = before.clone();
    crate::fit_outline_docked(board, &mut before, rim);
    crate::fit_outline_docked(board, after, rim);
    if hinted_off_edge(board, after) > hinted_off_edge(board, &before) {
        return false;
    }
    let judge_start = std::time::Instant::now();
    let better = judged_better(
        &Rows::of(&judge(board, &before)),
        &Rows::of(&judge(board, after)),
    );
    crate::stage_trace::record_time("fold_judge", judge_start.elapsed());
    better
}

/// Hard-hinted connectors whose courtyard left the edge the hint names.
fn hinted_off_edge(board: &Board, placement: &Placement) -> usize {
    let mut off = 0;
    for placed in &placement.components {
        let Some(component) = board.component(placed.id) else {
            continue;
        };
        let Some(hint) = component.placement_hint.as_ref() else {
            continue;
        };
        if hint.priority != synth_ir::PlacementPriority::Hard {
            continue;
        }
        let Some(edge) = hint.edge.as_ref() else {
            continue;
        };
        let rect = sidecar_courtyard_rect(board, component, placed);
        let outline = placement.board_outline;
        let gap = match edge {
            PlacementEdge::Top => rect.min.y_nm - outline.min.y_nm,
            PlacementEdge::Right => outline.max.x_nm - rect.max.x_nm,
            PlacementEdge::Bottom => outline.max.y_nm - rect.max.y_nm,
            PlacementEdge::Left => rect.min.x_nm - outline.min.x_nm,
        };
        if gap > mm_to_nm(HINT_FLUSH_MM) {
            off += 1;
        }
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(
            Point::new(mm_to_nm(x0), mm_to_nm(y0)),
            Point::new(mm_to_nm(x1), mm_to_nm(y1)),
        )
    }

    fn piece(slot: usize, kind: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Piece {
        Piece {
            slot,
            refdes: format!("P{slot}"),
            kind: kind.to_string(),
            part: String::new(),
            rect: rect(x0, y0, x1, y1),
            quarter: 0,
            mating: None,
            hint: None,
            pins: if kind == "mcu" { 24 } else { 2 },
            group: Some("G".to_string()),
        }
    }

    fn unit(name: &str, u: f64, v: f64) -> Unit {
        Unit {
            name: name.to_string(),
            wall: None,
            u: mm_to_nm(u),
            v: mm_to_nm(v),
            local: Vec::new(),
            usb_conns: Vec::new(),
            centre_along: 0,
            has_choice: false,
            ab: None,
            hinted: false,
        }
    }

    #[test]
    fn a_jack_leaves_the_group_box_like_a_connector() {
        let outline = rect(0.0, 0.0, 50.0, 50.0);
        let jack = rect(45.0, 0.0, 50.0, 5.0);
        assert!(!joins_group_box("connector", true, jack, outline));
        assert!(!joins_group_box("jack", true, jack, outline));
    }

    #[test]
    fn a_fold_shrinks_a_sprawling_group() {
        // Five parts strung across 80 mm, the shape the ring rule leaves.
        let pieces = vec![
            piece(0, "mcu", 0.0, 0.0, 5.0, 5.0),
            piece(1, "capacitor", 20.0, 0.0, 21.6, 0.8),
            piece(2, "capacitor", 40.0, 0.0, 41.6, 0.8),
            piece(3, "resistor", 60.0, 0.0, 61.6, 0.8),
            piece(4, "capacitor", 78.4, 0.0, 80.0, 0.8),
        ];
        let before = pieces
            .iter()
            .map(|piece| piece.rect)
            .reduce(union)
            .expect("block");
        let folded = fold_block(&pieces, mm_to_nm(PART_GAP_MM));
        assert!(
            folded.width * folded.height < before.width_nm() * before.height_nm(),
            "folded {}x{} is not smaller than {}x{}",
            folded.width,
            folded.height,
            before.width_nm(),
            before.height_nm()
        );
    }

    #[test]
    fn the_pack_leaves_two_mm_gaps_and_no_overlap() {
        let units = vec![
            unit("A", 10.0, 8.0),
            unit("B", 6.0, 4.0),
            unit("C", 4.0, 7.0),
        ];
        let scene = Scene {
            units: &units,
            holes: &[],
            margins: Margins::default(),
            block_gap: mm_to_nm(BLOCK_GAP_MM),
            export_margin: mm_to_nm(1.25),
        };
        let spots = scene
            .pack_free(&[0, 1, 2], &[], mm_to_nm(60.0), mm_to_nm(60.0))
            .expect("packs");
        for (index, &(_, a, _)) in spots.iter().enumerate() {
            for &(_, b, _) in spots.iter().skip(index + 1) {
                assert!(
                    rect_gap(a, b) >= mm_to_nm(BLOCK_GAP_MM),
                    "blocks {index} are {} mm apart",
                    synth_geometry::nm_to_mm(rect_gap(a, b))
                );
            }
        }
    }

    #[test]
    fn a_wall_unit_turns_toward_its_wall() {
        let unit = Unit {
            local: vec![(0, 0, 0, 0)],
            ..unit("A", 10.0, 4.0)
        };
        let rect = rect(0.0, 0.0, 10.0, 4.0);
        // Bottom keeps the local frame; top mirrors both axes; right and left
        // turn a quarter and map the local origin onto their near corner.
        assert_eq!(
            unit_point(BOTTOM, rect, &unit, 0, 0),
            (rect.min.x_nm, rect.min.y_nm)
        );
        assert_eq!(
            unit_point(TOP, rect, &unit, 0, 0),
            (rect.min.x_nm + unit.u, rect.min.y_nm + unit.v)
        );
        assert_eq!(
            unit_point(RIGHT, rect, &unit, 0, 0),
            (rect.min.x_nm, rect.min.y_nm + unit.u)
        );
        assert_eq!(
            unit_point(LEFT, rect, &unit, 0, 0),
            (rect.min.x_nm + unit.v, rect.min.y_nm)
        );
    }

    #[test]
    fn a_header_lays_along_every_wall_and_flush() {
        let groups = BTreeMap::from([("G".to_string(), vec![0_usize])]);
        let options = BTreeMap::new();
        let wall_over = BTreeMap::new();
        let board = mm_to_nm(60.0);
        for wall in [LEFT, RIGHT, TOP, BOTTOM] {
            // A 1x04 header: 3.54 mm across the pad row, 11.16 mm along it.
            let mut header = piece(0, "connector", 0.0, 0.0, 3.54, 11.16);
            header.part = "header_1x4".to_string();
            header.pins = 4;
            header.hint = Some(wall);
            let pieces = vec![header];
            let units = make_units(
                &pieces,
                &groups,
                rect(0.0, 0.0, 60.0, 60.0),
                &options,
                &wall_over,
            );
            assert_eq!(units.len(), 1, "wall {wall}");
            let unit = &units[0];
            assert!(
                unit.u >= unit.v,
                "wall {wall}: along {} is shorter than inward {}",
                unit.u,
                unit.v
            );
            let scene = Scene {
                units: &units,
                holes: &[],
                margins: Margins::default(),
                block_gap: mm_to_nm(BLOCK_GAP_MM),
                export_margin: mm_to_nm(1.25),
            };
            let rows = scene.wall_layout(board, board).expect("the row fits");
            let (_, row) = rows[0];
            let &(_, lx, ly, turn) = unit
                .local
                .iter()
                .find(|&&(slot, _, _, _)| slot == 0)
                .expect("the header is placed");
            let (cx, cy) = unit_point(wall, row, unit, lx, ly);
            let (w, h) = if turn % 2 == 0 {
                (mm_to_nm(3.54), mm_to_nm(11.16))
            } else {
                (mm_to_nm(11.16), mm_to_nm(3.54))
            };
            let placed = Rect::new(
                Point::new(cx - w / 2, cy - h / 2),
                Point::new(cx + w / 2, cy + h / 2),
            );
            let (along, across) = if matches!(wall, TOP | BOTTOM) {
                (placed.width_nm(), placed.height_nm())
            } else {
                (placed.height_nm(), placed.width_nm())
            };
            assert!(
                along >= across,
                "wall {wall}: header is {along} nm along and {across} nm across"
            );
            let gap = match wall {
                LEFT => placed.min.x_nm,
                RIGHT => board - placed.max.x_nm,
                TOP => placed.min.y_nm,
                _ => board - placed.max.y_nm,
            };
            assert_eq!(gap, 0, "wall {wall}: header is {gap} nm off the edge");
        }
    }

    #[test]
    fn a_worse_result_is_rejected() {
        let before = Rows {
            area: 4_000.0,
            usb: Some(0.0),
            dock: Some(1.0),
            dock_failures: 0,
            alignment: Some(0.9),
            ring: Some(0.9),
            group_gap: Some(2.5),
            group_failures: 0,
            overlaps: 0,
        };
        let mut after = Rows {
            area: 3_000.0,
            usb: Some(0.0),
            dock: Some(1.0),
            dock_failures: 0,
            alignment: Some(0.9),
            ring: Some(0.9),
            group_gap: Some(2.5),
            group_failures: 0,
            overlaps: 0,
        };
        assert!(judged_better(&before, &after));
        after.ring = Some(0.8);
        assert!(!judged_better(&before, &after), "a lower ring must reject");
        after.ring = Some(0.9);
        after.area = 4_000.0;
        assert!(!judged_better(&before, &after), "equal area must reject");
        after.area = 3_000.0;
        after.overlaps = 1;
        assert!(!judged_better(&before, &after), "an overlap must reject");
    }

    #[test]
    fn a_corner_hole_is_corner_seated() {
        let outline = rect(0.0, 0.0, 80.0, 60.0);
        assert!(corner_seated(rect(2.0, 2.0, 5.0, 5.0), outline));
        assert!(!corner_seated(rect(30.0, 25.0, 33.0, 28.0), outline));
    }

    #[test]
    fn a_group_with_a_corner_hole_gets_one_box() {
        let outline = rect(0.0, 0.0, 80.0, 60.0);
        let members = [
            ("connector", rect(74.0, 26.0, 78.0, 31.0)),
            ("mcu", rect(34.0, 26.0, 40.0, 32.0)),
            ("mounting_hole", rect(2.0, 2.0, 5.0, 5.0)),
            ("capacitor", rect(44.0, 26.0, 45.6, 26.8)),
            ("resistor", rect(24.0, 26.0, 25.6, 26.8)),
        ];
        // The major part leaves the connector out; the corner hole leaves too.
        let boxed = group_box(members.iter().copied(), true, outline).expect("box");
        assert_eq!(boxed, rect(24.0, 26.0, 45.6, 32.0));
        // The same members in any order draw the same box.
        assert_eq!(
            group_box(members.iter().rev().copied(), true, outline),
            Some(boxed)
        );
        let included = group_box(members.iter().copied(), false, outline).expect("box");
        assert_eq!(
            included.max.x_nm,
            mm_to_nm(78.0),
            "without the major part the connector stays in"
        );
        assert_eq!(
            included.min.x_nm,
            mm_to_nm(24.0),
            "the corner hole leaves even without a major part"
        );
    }
}
