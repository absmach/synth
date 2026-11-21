// SPDX-License-Identifier: Apache-2.0

//! Default placement rules, applied to a finished placement: unhinted mounting
//! holes go one per board corner, an unhinted wired connector sits centred on an
//! edge facing outward, and each declared `group` becomes a compact block.
//!
//! A part with a `placement_hint` is never moved by a rule, except an edge
//! connector, which slides inward with the outline while staying on its hinted
//! side. Once the board is packed, free passives spread into the open sides
//! while staying near their group's chip. Each rule is all-or-nothing: if it
//! leaves more courtyard overlaps than it found, it is dropped. A board with no
//! holes, USB connector or group is returned untouched.

#![allow(clippy::cast_precision_loss)] // Board counts and nanometres are far below 2^53.

use std::collections::{BTreeMap, BTreeSet};

use synth_geometry::{Point, Rect, Rotation, mm_to_nm, nm_to_mm};
use synth_ir::{Board, Component};

use crate::floorplan::{self, BoardEdge};
use crate::judge::{JudgeMetric, PlacementJudgement};
use crate::{
    ComponentPlacement, Placement, describe_placement, fit_outline_docked, fitted_edge_margin_nm,
    flush_edge_index, hole_rim_nm, placement_spacing_mm, sidecar_courtyard_rect, spacing_gap_mm,
};

/// Courtyard clearance a rule leaves when it pushes a part off another.
const RULE_GAP_MM: f64 = 0.5;

/// How far (mm) from a connector a group passive can sit and still ride with
/// it. A part further out is not attached to the connector, only to the same
/// declared group; dragging it would move it across another group's block.
const ATTACHED_MM: f64 = 5.5;

/// Least distance between two groups' bounding boxes.
const GROUP_GAP_MM: f64 = 2.0;

/// How far a rule may leave an edge-flush connector from its edge, to absorb rounding.
const FLUSH_SLACK_MM: f64 = 0.1;

/// Most the packed groups may grow the outline over the arrangement they started from.
const MAX_AREA_GROWTH: f64 = 1.2;

/// Box area a folded block aims for, as a multiple of its parts' area; rows
/// of unequal parts waste about this much.
const FOLD_FILL: f64 = 1.5;

/// Width/height a folded block aims for.
const FOLD_TARGET_ASPECT: f64 = 1.3;

/// Most groups a net may touch and still count as linking them.
const LINK_MAX_GROUPS: usize = 3;

/// Most the nearest edges of two groups sharing a net should be apart, so
/// connected groups are neighbours rather than ends of the board.
const MAX_LINK_GAP_MM: f64 = 8.0;

/// Up to this many blocks, every packing order is tried; more fall back to two.
const MAX_PERMUTED_BLOCKS: usize = 6;

/// Area allowance when a spacing is set: the gaps inside and between blocks
/// grow with it.
const SPACED_AREA_GROWTH: f64 = 3.0;

/// The same, when a spacing is set: bigger gaps block more corridors, so more
/// packings are worth a look.
const SPACED_TRIED_PACKINGS: usize = 80;

/// How many of the best-scoring packings are tried before the group rule gives up.
const TRIED_PACKINGS: usize = 12;

/// Packings within this much outline area (2% of a 2750 mm2 board) count as the
/// same size, so the link score decides between them; a smaller one always wins.
const AREA_BUCKET_UM2: i64 = 55_000_000;

/// Cohesion rounds: each pulls a group member a quarter of the way to the
/// group centre, so a few rounds settle the block without a convergence test.
const COHESION_ROUNDS: usize = 10;

/// Rounds the group spacing gets; each is followed by a part push-apart.
const SPACE_GROUP_ROUNDS: usize = 10;

/// Grid a part's centre snaps to, measured from the outline's top-left corner.
const GRID_MM: f64 = 0.5;

/// How far past a USB connector its group may sit after a both-sides seat, so
/// the frame stays compact instead of one long row along the edge.
const USB_SEAT_REACH_MM: f64 = 11.0;

/// The same for pin headers, whose pads sit on a 1.27 mm pitch.
const HEADER_GRID_MM: f64 = 1.27;

/// Most sweeps over the free parts when snapping to the grid.
const SNAP_SWEEPS: usize = 4;

/// Compression moves one wall by this much per accepted step.
const COMPRESS_STEP_MM: f64 = 0.5;

/// Hard cap on compression steps, one per side attempt.
const COMPRESS_MAX_STEPS: usize = 40;

/// Judge tries a wall jump may spend, halving the distance each time.
const COMPRESS_JUMP_TRIES: usize = 5;

/// Default wall-clock budget for the feedback loop, in seconds.
const REFINE_BUDGET_S: u64 = 90;

/// Default wall-clock budget for wall compression, in seconds.
const COMPRESS_BUDGET_S: u64 = 120;

/// A positive integer of seconds from `name`, else `default_s`; 0 expires now.
fn budget_seconds(name: &str, default_s: u64) -> std::time::Duration {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(
            std::time::Duration::from_secs(default_s),
            std::time::Duration::from_secs,
        )
}

/// A compressed step may not lose more alignment or ring than this.
const COMPRESS_QUALITY_SLACK: f64 = 0.02;

/// The composite is compared to half of the report's last displayed digit:
/// a change that rounds away in the reported score is not a regression.
const COMPRESS_SCORE_SLACK: f64 = 0.005;

/// A connector this close to a wall rides with it.
const COMPRESS_FLUSH_MM: f64 = 0.15;

/// Parts of a group whose centres are this close on an axis share one row or column.
const ALIGN_TOL_MM: f64 = 0.75;

/// Slack (mm) a block's own layout leaves between parts on top of the part gap,
/// so the grid snap has room to move a part without closing a gap.
const SNAP_ROOM_MM: f64 = 0.25;

/// Cap on the push-apart passes that clear overlaps after a rule moved parts.
const RELAX_PASSES: usize = 200;

/// Vertical step (mm) at which the row of chip blocks looks for a clear seat.
const CHIP_ROW_STEP_MM: f64 = 0.5;

/// Packings whose board is more than this much bigger than the smallest are
/// not traded for a more central chip.
const CHIP_AREA_SLACK: f64 = 1.1;

/// Passes over the caps: a cap that moved frees a spot the next one can take.
const HUG_ROUNDS: usize = 3;

/// How far (mm) from the pin it serves a cap is looked for a spot.
const HUG_WINDOW_MM: f64 = 10.0;

/// Part boxes grow by this much when looking for the largest empty rectangle.
const EMPTY_INFLATE_MM: f64 = 1.0;

/// Grid the empty-rectangle search uses.
const EMPTY_CELL_MM: f64 = 0.5;

/// Rounds the spread pass takes, one grid step per part per round.
const SPREAD_ROUNDS: usize = 20;

/// A part walks one grid cell per round toward the largest empty rectangle.
const SPREAD_STEP_MM: f64 = 0.5;

/// How much further from its anchor a part may end up, over the whole pass.
const SPREAD_SLACK_MM: f64 = 6.0;

/// Chip distances from the board centre that differ by less than this (mm) tie.
const CHIP_BUCKET_MM: i64 = 3;

/// How far a passive blocking a connector's escape corridor is stepped aside.
const CORRIDOR_STEP_MM: f64 = 5.0;

/// The rim the outline keeps round the parts. A grouped board takes the
/// export's tight one, plus how far a part's pads reach past its courtyard
/// (the outline fit keeps the copper inside it too): the holes sit in the
/// packed outline's corners, so a wider rim would leave an empty band round
/// the parts. A board without groups or holes keeps the placer's own.
fn rim_nm(board: &Board, placed: &[ComponentPlacement], grouped: bool) -> i64 {
    let has = |keep: fn(&Component) -> bool| board.components.iter().any(keep);
    if !grouped || !has(|c| c.group.is_some()) || !has(|c| c.kind == "mounting_hole") {
        return fitted_edge_margin_nm(board);
    }
    let overhang = placed
        .iter()
        .filter_map(|p| {
            let component = board.component(p.id)?;
            if synth_layout::is_board_feature_kind(&component.kind) {
                return None;
            }
            let (r, pads) = (
                sidecar_courtyard_rect(board, component, p),
                crate::exported_pad_bounds(component, p)?,
            );
            Some(
                (r.min.x_nm - pads.min.x_nm)
                    .max(r.min.y_nm - pads.min.y_nm)
                    .max(pads.max.x_nm - r.max.x_nm)
                    .max(pads.max.y_nm - r.max.y_nm),
            )
        })
        .max()
        .unwrap_or(0);
    // Whole grid cells, so a part at the rim sits on the grid as well.
    let grid = mm_to_nm(GRID_MM);
    let rim = mm_to_nm(crate::tight_edge_margin_mm(board)) + overhang.max(0);
    (rim + grid - 1) / grid * grid
}

/// Distance between two rects: negative when they overlap.
fn rect_gap(a: Rect, b: Rect) -> i64 {
    (b.min.x_nm - a.max.x_nm)
        .max(a.min.x_nm - b.max.x_nm)
        .max(b.min.y_nm - a.max.y_nm)
        .max(a.min.y_nm - b.max.y_nm)
}

fn mean_point(points: &[Point]) -> Point {
    let n = points.len() as i64;
    Point::new(
        points.iter().map(|p| p.x_nm).sum::<i64>() / n,
        points.iter().map(|p| p.y_nm).sum::<i64>() / n,
    )
}

fn point_distance(a: Point, b: Point) -> i64 {
    ((a.x_nm - b.x_nm) as f64).hypot((a.y_nm - b.y_nm) as f64) as i64
}

/// The largest rectangle inside `hull` that no part box, grown by `inflate`,
/// touches: `(area_nm2, rect)`. Snapped to a [`EMPTY_CELL_MM`] grid.
fn largest_empty_rect(hull: Rect, boxes: &[Rect], inflate: i64) -> (i64, Rect) {
    let cell = mm_to_nm(EMPTY_CELL_MM);
    let (width, height) = (hull.width_nm(), hull.height_nm());
    if width <= 0 || height <= 0 || cell <= 0 {
        return (0, hull);
    }
    let cols = usize::try_from((width + cell - 1) / cell).expect("a positive cell count");
    let rows = usize::try_from((height + cell - 1) / cell).expect("a positive cell count");
    let mut blocked = vec![false; cols * rows];
    for b in boxes {
        let col = |nm: i64| usize::try_from(nm).expect("a non-negative grid coordinate");
        let x0 = col(((b.min.x_nm - inflate - hull.min.x_nm).max(0) / cell).min(cols as i64));
        let x1 = col(((b.max.x_nm + inflate - hull.min.x_nm).clamp(0, width) + cell - 1) / cell)
            .min(cols);
        let y0 = col(((b.min.y_nm - inflate - hull.min.y_nm).max(0) / cell).min(rows as i64));
        let y1 = col(((b.max.y_nm + inflate - hull.min.y_nm).clamp(0, height) + cell - 1) / cell)
            .min(rows);
        for y in y0..y1 {
            for x in x0..x1 {
                blocked[y * cols + x] = true;
            }
        }
    }
    let mut heights = vec![0usize; cols];
    let mut best = (0i64, Rect::new(hull.min, hull.min));
    for y in 0..rows {
        for x in 0..cols {
            heights[x] = if blocked[y * cols + x] {
                0
            } else {
                heights[x] + 1
            };
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for x in 0..=cols {
            let h = heights.get(x).copied().unwrap_or(0);
            let mut start = x;
            while let Some(&(sx, sh)) = stack.last() {
                if sh <= h {
                    break;
                }
                stack.pop();
                let (w, hh) = ((x - sx) as i64 * cell, sh as i64 * cell);
                if w * hh > best.0 {
                    let x0 = hull.min.x_nm + sx as i64 * cell;
                    let y0 = hull.min.y_nm + (y as i64 + 1 - sh as i64) * cell;
                    best = (
                        w * hh,
                        Rect::new(
                            Point::new(x0, y0),
                            Point::new((x0 + w).min(hull.max.x_nm), (y0 + hh).min(hull.max.y_nm)),
                        ),
                    );
                }
                start = sx;
            }
            if stack.last().is_none_or(|&(_, sh)| sh < h) {
                stack.push((start, h));
            }
        }
    }
    best
}

struct State<'a> {
    board: &'a Board,
    placed: Vec<ComponentPlacement>,
    /// Parts a hint pins or names; no rule moves them.
    pinned: Vec<bool>,
    /// `pinned` plus parts a rule has fixed; the push-apart never moves them.
    anchored: Vec<bool>,
    outline: Rect,
    /// Whether the arrangement may add review findings (other than a connector
    /// short of its edge) that the one before any rule ran did not have.
    keep_clean: bool,
    /// Keys of the findings the arrangement before any rule ran already had.
    known: Vec<String>,
    /// Gap kept between parts a rule moves: the spacing, else `RULE_GAP_MM`.
    gap_nm: i64,
    /// Half the spacing, zero when none is set: every courtyard is grown by it
    /// when looking for pairs that sit closer than the spacing.
    infl: i64,
    /// Least gap between two groups' boxes.
    group_gap_nm: i64,
    /// Dock only USB connectors; the fallback when docking every wired
    /// connector cannot keep the spacing.
    usb_only: bool,
    /// Rim the outline is fitted at: see [`rim_nm`].
    rim: i64,
    /// Courtyard envelopes of connectors already docked (the connector plus the
    /// passives that ride with it). Later docking keeps the part gap off them.
    reserved: Vec<Rect>,
    /// Parts a header row carries: the spread pass leaves them with it.
    spread_keep: Vec<usize>,
}

/// Group boxes as the placement review draws them: a connector in a group with
/// a major IC docks on its own edge and is left out of the block, and a
/// mounting hole on a board corner leaves too. The judge, the rules and the
/// spacing tests all read this one rule.
pub(crate) fn review_group_boxes(
    board: &Board,
    placed: &[ComponentPlacement],
    outline: Rect,
) -> BTreeMap<String, Rect> {
    let mut boxes: BTreeMap<String, Rect> = BTreeMap::new();
    let mut groups: BTreeMap<&str, bool> = BTreeMap::new();
    for item in placed {
        if let Some(group) = board.component(item.id).and_then(|c| c.group.as_deref()) {
            let major = groups.entry(group).or_insert(false);
            *major |= board
                .component(item.id)
                .is_some_and(crate::fold::is_major_component);
        }
    }
    for (group, has_chip) in groups {
        let members = placed.iter().filter_map(|placed| {
            let component = board.component(placed.id)?;
            (component.group.as_deref() == Some(group)).then(|| {
                (
                    component.kind.as_str(),
                    sidecar_courtyard_rect(board, component, placed),
                )
            })
        });
        if let Some(rect) = crate::fold::group_box(members, has_chip, outline) {
            boxes.insert(group.to_string(), rect);
        }
    }
    boxes
}

/// `(gap_nm, infl, group_gap_nm)` for a board.
fn spacing_gaps(board: &Board) -> (i64, i64, i64) {
    match (spacing_gap_mm(board), placement_spacing_mm(board)) {
        (Some(gap), Some(raw)) => (
            mm_to_nm(gap),
            mm_to_nm(gap) / 2,
            // Groups sit at least as far apart as parts do, or the push-apart
            // that keeps the part gap would shove the groups into each other.
            mm_to_nm((raw + 1.0).max(GROUP_GAP_MM).max(gap)),
        ),
        _ => (mm_to_nm(RULE_GAP_MM), 0, mm_to_nm(GROUP_GAP_MM)),
    }
}

/// Apply the three rules to `placement`, refitting the outline if any moved a part.
pub(crate) fn apply(board: &Board, placement: &mut Placement, with_groups: bool) {
    // A hinted part is pinned, and so is one another hint says to sit `near`.
    let near_targets: Vec<&str> = board
        .components
        .iter()
        .filter_map(|c| c.placement_hint.as_ref()?.near.as_deref())
        .filter_map(|near| near.split('.').next())
        .collect();
    let pinned: Vec<bool> = placement
        .components
        .iter()
        .map(|placed| {
            board.component(placed.id).is_none_or(|component| {
                component.placement_hint.is_some()
                    || near_targets.contains(&component.refdes.as_str())
            })
        })
        .collect();
    let (gap_nm, infl, group_gap_nm) = spacing_gaps(board);
    let rim = rim_nm(board, &placement.components, with_groups);
    // A rule may not add a review finding the starting arrangement lacked, a
    // blocked escape corridor included; the spacing does not excuse one.
    let known_state = State {
        board,
        anchored: Vec::new(),
        pinned: Vec::new(),
        placed: placement.components.clone(),
        outline: placement.board_outline,
        keep_clean: false,
        known: Vec::new(),
        usb_only: false,
        gap_nm,
        infl,
        group_gap_nm,
        rim,
        reserved: Vec::new(),
        spread_keep: Vec::new(),
    };
    let known = known_state.finding_keys();
    // Whether the last run kept the groups (a run without them counts as kept).
    let grouped = std::cell::Cell::new(true);
    let run = |with_usb: bool, with_groups: bool, usb_only: bool| {
        let mut state = State {
            board,
            anchored: pinned.clone(),
            pinned: pinned.clone(),
            placed: placement.components.clone(),
            outline: placement.board_outline,
            keep_clean: true,
            known: known.clone(),
            usb_only,
            gap_nm,
            infl,
            group_gap_nm,
            rim,
            reserved: Vec::new(),
            spread_keep: Vec::new(),
        };
        // The connector goes first so its group gathers round it; the groups
        // then give it a slot on the packed outline and reserve the corners
        // for the holes, which are snapped to the corners of the finished board.
        if with_usb {
            let rule_start = std::time::Instant::now();
            state.try_rule(State::usb_to_edge);
            crate::stage_trace::record_time("rule_usb", rule_start.elapsed());
        }
        grouped.set(true);
        if with_groups {
            let rule_start = std::time::Instant::now();
            let before = state.placed.clone();
            state.try_rule(State::groups);
            grouped.set(state.placed != before);
            crate::stage_trace::record_time("rule_groups", rule_start.elapsed());
        }
        let rule_start = std::time::Instant::now();
        state.try_rule(State::holes_to_corners);
        crate::stage_trace::record_time("rule_holes", rule_start.elapsed());
        // With a spacing set, no pair may end up closer than it.
        if state.infl > 0 {
            state.relax();
            state.space_groups();
        }
        state
    };
    // Moving a connector or a group can block an escape corridor. If the
    // arrangement reviewed clean before, it must after, so the costliest moves
    // are dropped one at a time until it does.
    let mut state = run(true, with_groups, false);
    // Flush side blocks can leave no room for the spacing; then only USB docks.
    if infl > 0 && !state.keeps_spacing() {
        state = run(true, with_groups, true);
        if !state.keeps_spacing() {
            state = run(false, with_groups, false);
        }
    }
    // Docking all wired connectors can leave no packing of the groups that
    // keeps every corridor open: dock only the USB one, then none, before the
    // groups are given up.
    for (with_usb, with_groups, usb_only) in [
        (true, with_groups, true),
        (false, with_groups, false),
        (true, false, false),
        (false, false, false),
    ] {
        if !state.needs_revision() && grouped.get() {
            break;
        }
        state = run(with_usb, with_groups, usb_only);
    }
    if state.needs_revision() {
        // The rules are dropped, but the spacing is a promise, not a layout rule.
        state = State {
            board,
            anchored: pinned.clone(),
            pinned: pinned.clone(),
            placed: placement.components.clone(),
            outline: placement.board_outline,
            keep_clean: false,
            known: Vec::new(),
            usb_only: false,
            gap_nm,
            infl,
            group_gap_nm,
            rim,
            reserved: Vec::new(),
            spread_keep: Vec::new(),
        };
        if infl > 0 {
            state.relax();
        }
    }
    // Snapping must not grow the outline the placement would otherwise have.
    let unaligned = state.placed.clone();
    let area_of = |outline: Rect| outline.width_nm() as f64 * outline.height_nm() as f64;
    let area_without = if unaligned == placement.components {
        area_of(placement.board_outline)
    } else {
        state.hull_at(|_| true, rim).map_or(0.0, area_of)
    };
    state.align();
    if state.hull_at(|_| true, rim).map_or(0.0, area_of) > area_without {
        state.placed = unaligned;
    }
    // Past the groups and the grid: parts only move toward the pins they serve,
    // and only when the groups kept their gap (else the groups were given up).
    if !state.group_members().is_empty() && state.min_group_gap() >= state.group_gap_nm {
        for _ in 0..HUG_ROUNDS {
            state.guarded(State::hug);
        }
    }
    if state.placed != placement.components {
        placement.components = state.placed;
        fit_outline_docked(board, placement, rim);
    }
}

/// Pull the edge connectors in with the outline once the arrangement is final.
/// Runs after the placement is chosen, so it cannot change which candidate wins.
/// Fold the groups, then settle the edges. The fold is a candidate: it is
/// kept only when the judged result is smaller and no measured row got worse.
/// Otherwise the settle runs on the input, which is what this stage did
/// before the fold existed.
pub(crate) fn close_edges(board: &Board, placement: &mut Placement) {
    let input = placement.clone();
    let fold_start = std::time::Instant::now();
    let folded = crate::fold::fold_groups_pack(board, placement);
    crate::stage_trace::record("fold_groups_pack", board, placement, fold_start.elapsed());
    if !folded {
        settle_edges(board, placement, true);
        return;
    }
    let mut candidate = placement.clone();
    // The fold has already packed the boxes: the group push-apart would undo
    // that packing by inflating the board around the old sprawl. The later
    // dock and the judge's group_gap row decide whether the pack held.
    let candidate_start = std::time::Instant::now();
    settle_edges(board, &mut candidate, false);
    crate::stage_trace::record(
        "fold_candidate",
        board,
        &candidate,
        candidate_start.elapsed(),
    );
    let rim = rim_nm(board, &candidate.components, true);
    if crate::fold::fold_is_better(board, &input, &mut candidate, rim) {
        *placement = candidate;
    } else {
        *placement = input;
        settle_edges(board, placement, true);
    }
}

fn settle_edges(board: &Board, placement: &mut Placement, push_groups: bool) {
    let pinned: Vec<bool> = placement
        .components
        .iter()
        .map(|placed| {
            board
                .component(placed.id)
                .is_none_or(|component| component.placement_hint.is_some())
        })
        .collect();
    let (gap_nm, infl, group_gap_nm) = spacing_gaps(board);
    let rim = rim_nm(board, &placement.components, true);
    let mut state = State {
        board,
        anchored: pinned.clone(),
        pinned,
        placed: placement.components.clone(),
        outline: placement.board_outline,
        keep_clean: false,
        known: Vec::new(),
        usb_only: false,
        gap_nm,
        infl,
        group_gap_nm,
        rim,
        reserved: Vec::new(),
        spread_keep: Vec::new(),
    };
    let before = state.placed.clone();
    let settle_start = std::time::Instant::now();
    let mut groups = state.review_group_gap();
    // A tighter packing can leave two groups closer than the rule keeps them.
    // Push the boxes apart before the edges are drawn in and the holes cornered.
    if push_groups && groups < group_gap_nm {
        let findings = state.finding_keys();
        state.space_groups();
        if state.review_group_gap() < group_gap_nm
            || state.overlaps() > 0
            || state
                .finding_keys()
                .iter()
                .any(|key| !findings.contains(key))
        {
            state.placed = before;
        }
    }
    groups = state.review_group_gap();
    let before_shrink = state.placed.clone();
    let area_before = state
        .hull_at(|_| true, rim)
        .map_or(i64::MAX, |h| h.width_nm().saturating_mul(h.height_nm()));
    state.guarded(State::shrink_edges);
    state.align();
    let area_after = state
        .hull_at(|_| true, rim)
        .map_or(i64::MAX, |h| h.width_nm().saturating_mul(h.height_nm()));
    if state.review_group_gap() < groups.min(group_gap_nm) || area_after >= area_before {
        state.placed = before_shrink;
    }
    // Size the outline to the packed body: the holes leave the packing's
    // reserved corners for the corners of the body plus the rim. Runs after
    // the placement is chosen, so it cannot change which candidate wins.
    state.fit_corners();
    state.trace("close_edges_settle", settle_start);
    // Connectors still in the interior dock on the finished outline, so their
    // seats do not push the edge out with them.
    let stage_start = std::time::Instant::now();
    state.dock_loose_connectors();
    state.trace("dock_loose_connectors", stage_start);
    // A connector seated on the packing's width is not centred on the body's:
    // walk each USB block along its edge to the final outline's middle.
    let stage_start = std::time::Instant::now();
    state.centre_usb_blocks();
    state.trace("usb_centring", stage_start);
    let stage_start = std::time::Instant::now();
    state.pack_edge_headers();
    crate::stage_trace::record_time("pack_headers", stage_start.elapsed());
    let before_spread = state.placed.clone();
    let spread_groups = state.review_group_gap();
    let compact_before = state.groups_compact();
    let stage_start = std::time::Instant::now();
    state.spread();
    crate::stage_trace::record_time("spread", stage_start.elapsed());
    if state.review_group_gap() < spread_groups.min(group_gap_nm)
        || state.overlaps() > 0
        || (compact_before && !state.groups_compact())
    {
        state.placed = before_spread;
    }
    if state.placed != placement.components {
        placement.components = state.placed;
        fit_outline_docked(board, placement, rim);
    }
}

/// Whether the implicit compression pass runs. `SYNTH_PLACE_COMPRESS=0` turns
/// it off; every other value, or no value, leaves it on.
pub(crate) fn compression_enabled() -> bool {
    std::env::var_os("SYNTH_PLACE_COMPRESS").is_none_or(|value| value.to_str() != Some("0"))
}

/// Shrink the finished outline while the placement judge stays clean.
///
/// Each wall first jumps to its rectangle limit: the furthest it can move
/// while every stationary part stays inside and every moving part keeps the
/// 2 mm group gap. A rejected jump is halved to the `COMPRESS_STEP_MM` grid
/// up to `COMPRESS_JUMP_TRIES` times. The wall carries the connectors docked
/// to it (and their passive frames) and re-corners the free mounting holes.
/// The parts inside stay put, so a group keeps its internal geometry.
///
/// A step is kept when no gated row's failure count rises, no measured quality
/// row regresses beyond `COMPRESS_QUALITY_SLACK`, and, on a board that
/// already passed, the composite does not fall. A board that already fails a
/// gate can therefore still shrink without adding or worsening one. The
/// 0.5 mm stepping runs only while a wall's rectangle limit is at least one
/// step; a board whose jumps already reached every limit stops after the jumps.
///
/// This is called by [`crate::finish_outline`] after the outline fit, unless
/// `SYNTH_PLACE_COMPRESS=0` is set. Calling it directly always compresses,
/// up to its wall-clock budget (120 s, or `SYNTH_PLACE_COMPRESS_BUDGET_S`).
pub fn compress_outline(board: &Board, placement: &mut Placement) {
    let deadline = std::time::Instant::now()
        + budget_seconds("SYNTH_PLACE_COMPRESS_BUDGET_S", COMPRESS_BUDGET_S);
    compress_outline_until(board, placement, deadline);
}

fn compress_outline_until(board: &Board, placement: &mut Placement, deadline: std::time::Instant) {
    let stage_start = std::time::Instant::now();
    if std::time::Instant::now() >= deadline {
        return;
    }
    let mut state = compression_state(board, placement);
    // The round-22 pull is the feedback loop's first fix; it runs only when
    // the loop is enabled, so `SYNTH_PLACE_REFINE=0` leaves round 21 alone.
    // The pull shares the compression deadline: a pass that cannot finish is
    // cut off there.
    if refine_enabled() {
        let pull_start = std::time::Instant::now();
        pull_if_space_leads(board, &mut state, deadline);
        state.trace("compress_pull_groups", pull_start);
    }
    compress_walls(board, &mut state, deadline);
    if state.placed != placement.components || state.outline != placement.board_outline {
        placement.components = state.placed;
        placement.board_outline = state.outline;
    }
    crate::stage_trace::record("compress_outline", board, placement, stage_start.elapsed());
}

/// The round-21 wall compression without the round-22 pull.
fn compress_outline_plain_until(
    board: &Board,
    placement: &mut Placement,
    deadline: std::time::Instant,
) {
    let stage_start = std::time::Instant::now();
    if std::time::Instant::now() >= deadline {
        return;
    }
    let mut state = compression_state(board, placement);
    compress_walls(board, &mut state, deadline);
    if state.placed != placement.components || state.outline != placement.board_outline {
        placement.components = state.placed;
        placement.board_outline = state.outline;
    }
    crate::stage_trace::record(
        "compress_outline_plain",
        board,
        placement,
        stage_start.elapsed(),
    );
}

/// One compression stage after the feedback loop: the round-22 pull and wall
/// pass, the re-fit the old in-loop compress fix used to try, then the plain
/// wall pass. All three share one wall-clock budget and keep the best accepted
/// outline.
pub(crate) fn finish_compression(board: &Board, placement: &mut Placement) {
    let deadline = std::time::Instant::now()
        + budget_seconds("SYNTH_PLACE_COMPRESS_BUDGET_S", COMPRESS_BUDGET_S);
    compress_outline_until(board, placement, deadline);
    if std::time::Instant::now() >= deadline {
        return;
    }
    let base = placement.clone();
    let base_error = refine_error(board, &base);
    let mut extra = base.clone();
    crate::finish_outline_base(board, &mut extra);
    compress_outline_plain_until(board, &mut extra, deadline);
    if refine_error(board, &extra) < base_error {
        *placement = extra;
    }
    compress_outline_plain_until(board, placement, deadline);
}

/// Run the pull pass first only when the judge's dead-space or group-gap row is
/// not at its best, so a tightly packed board is not moved for nothing.
fn pull_if_space_leads(board: &Board, state: &mut State<'_>, deadline: std::time::Instant) {
    let report = judgement(board, state.outline, &state.placed);
    let leads = ["largest_empty", "group_gap"].iter().any(|key| {
        judge_metric(&report, key).is_some_and(|metric| {
            metric.failures > 0 || metric.score.is_some_and(|score| score < 1.0)
        })
    });
    if leads {
        pull_groups(board, state, deadline);
    }
}

/// Pull each non-MCU group toward the largest chip, then settle and compress.
///
/// The groups are taken in order of distance from the MCU. Each moves rigidly
/// along the axis of greatest separation until it is `GROUP_GAP_MM` from the
/// first courtyard, hole keep-out, docked connector or USB frame envelope in
/// its path, rounded down to the `GRID_MM` grid. A group move that would take
/// a docked connector off its wall is not made. After the moves the existing
/// group spacing runs, then the round-21 wall compression, then the judge. A
/// candidate is kept only when every gate passes and the placement error (the
/// summed group gap to the MCU, then the outline area, then the judge deficit)
/// strictly drops; otherwise it is reverted and the group is marked tried.
pub fn pull_groups_inward(board: &Board, placement: &mut Placement) {
    let deadline = std::time::Instant::now()
        + budget_seconds("SYNTH_PLACE_COMPRESS_BUDGET_S", COMPRESS_BUDGET_S);
    let mut state = compression_state(board, placement);
    pull_groups(board, &mut state, deadline);
    if state.placed != placement.components || state.outline != placement.board_outline {
        placement.components = state.placed;
        placement.board_outline = state.outline;
    }
}

/// Walk the round-21 wall compression on an already prepared state. A wall
/// step is only started while `deadline` is in the future; the state keeps the
/// last accepted outline when the budget expires.
///
/// ponytail: the deadline is checked between wall steps, so one step that
/// takes minutes (a 126-part board's group settle took 311–345 s) overruns
/// the nominal budget. Interrupt inside `space_groups`/`relax` if that
/// becomes a hard requirement.
fn compress_walls(board: &Board, state: &mut State<'_>, deadline: std::time::Instant) {
    let trace = CompressTrace::from_env();
    let sides = [
        BoardEdge::Left,
        BoardEdge::Right,
        BoardEdge::Top,
        BoardEdge::Bottom,
    ];
    let mut jumped = false;
    for side in &sides {
        if std::time::Instant::now() >= deadline {
            trace.append(&[
                &format!("{side:?}"),
                "jump",
                "false",
                "-",
                "-",
                "budget",
                "0.000",
            ]);
            return;
        }
        jumped |= jump_wall(board, state, *side, deadline, &trace);
    }
    let step = mm_to_nm(COMPRESS_STEP_MM);
    // A jump leaves the parts where they are and only moves the wall and the
    // parts docked to it. When every wall's rectangle limit is below one step,
    // the settle and the 0.5 mm stepping that follow would only spend the
    // budget: a board that already fails a gate can have a grouped hole moved
    // out of its corner for no area gain.
    if jumped
        && sides
            .iter()
            .all(|side| wall_jump_limit(state, *side) < step)
    {
        return;
    }

    // The round-21 wall stepping stays as the fallback for a board the jump
    // arithmetic cannot read. A wall whose rectangle limit is below one step
    // is done before it pays for the group settle.
    let mut done = [false; 4];
    let mut steps = 0_usize;
    while steps < COMPRESS_MAX_STEPS && done.iter().any(|side| !side) {
        for (index, side) in sides.iter().enumerate() {
            if done[index] || steps >= COMPRESS_MAX_STEPS {
                continue;
            }
            if std::time::Instant::now() >= deadline {
                trace.append(&[
                    &format!("{side:?}"),
                    &steps.to_string(),
                    "false",
                    "-",
                    "-",
                    "budget",
                    "0.000",
                ]);
                return;
            }
            // A wall whose rectangle limit is below one step has no valid
            // 0.5 mm move left; checking it here keeps a rejected step from
            // paying for the group settle.
            if wall_jump_limit(state, *side) < step {
                done[index] = true;
                trace.append(&[
                    &format!("{side:?}"),
                    &steps.to_string(),
                    "false",
                    "-",
                    "-",
                    "shrink_false",
                    "0.000",
                ]);
                continue;
            }
            steps += 1;
            let started = std::time::Instant::now();
            let before_placed = state.placed.clone();
            let before_outline = state.outline;
            let before = judgement(board, before_outline, &before_placed);
            let docked = docked_sides(state);
            let shrunk = shrink_side(state, *side);
            if !shrunk {
                done[index] = true;
                trace.append(&[
                    &format!("{side:?}"),
                    &steps.to_string(),
                    "false",
                    &failing_gates(&before),
                    "-",
                    "shrink_false",
                    &format!("{:.3}", started.elapsed().as_secs_f64()),
                ]);
                continue;
            }
            for _ in 0..3 {
                state.space_groups();
                dock_all_connectors(state, &docked);
                centre_usb(state);
            }
            state.space_groups();
            // Keep the free holes in the corners the new outline made. The
            // settle can move a group that contains them; re-cornering last
            // keeps `hole_corner` from failing on a hole the step never
            // targeted.
            place_holes(state);
            let after = judgement(board, state.outline, &state.placed);
            let kept = compression_is_no_worse(&before, &after);
            if !kept {
                state.placed = before_placed;
                state.outline = before_outline;
                done[index] = true;
            }
            trace.append(&[
                &format!("{side:?}"),
                &steps.to_string(),
                "true",
                &failing_gates(&before),
                &failing_gates(&after),
                if kept { "kept" } else { "judge_worse" },
                &format!("{:.3}", started.elapsed().as_secs_f64()),
            ]);
            if std::time::Instant::now() >= deadline {
                trace.append(&[
                    &format!("{side:?}"),
                    &steps.to_string(),
                    "false",
                    "-",
                    "-",
                    "budget",
                    "0.000",
                ]);
                return;
            }
        }
    }
}

/// The largest chip's group and courtyard: the MCU when the board declares one,
/// else the largest major IC. `None` when that chip is not in a group.
fn mcu_chip(state: &State<'_>) -> Option<(String, Rect)> {
    let mut best: Option<(bool, i64, usize)> = None;
    for index in 0..state.placed.len() {
        let component = state.component(index);
        let is_mcu = component.kind == "mcu";
        if !is_mcu && !state.is_major_ic(index) {
            continue;
        }
        let rect = state.rect(index);
        let area = rect.width_nm().saturating_mul(rect.height_nm());
        let rank = (is_mcu, area, index);
        if best.is_none_or(|(mcu, size, _)| (mcu, size) < (rank.0, rank.1)) {
            best = Some(rank);
        }
    }
    let (_, _, index) = best?;
    let group = state.component(index).group.clone()?;
    Some((group, state.rect(index)))
}

/// One pull pass: nearest group to the MCU first, each kept only when the
/// placement error drops and every gate holds.
fn pull_groups(board: &Board, state: &mut State<'_>, deadline: std::time::Instant) {
    let Some((mcu_group, mcu)) = mcu_chip(state) else {
        return;
    };
    let mcu_parts: Vec<(usize, Point)> =
        state
            .group_members()
            .get(&mcu_group)
            .map_or_else(Vec::new, |members| {
                members
                    .iter()
                    .map(|&i| (i, state.placed[i].center))
                    .collect()
            });
    let groups = state.group_members();
    let mut order: Vec<(i64, String)> = groups
        .iter()
        .filter(|(name, _)| **name != mcu_group)
        .map(|(name, members)| (rect_gap(state.bounds(members), mcu), name.clone()))
        .collect();
    order.sort_by_key(|(gap, name)| (*gap, name.clone()));
    let mut tried: BTreeSet<String> = BTreeSet::new();
    for (_, name) in order {
        if tried.contains(&name) {
            continue;
        }
        let Some(members) = state.group_members().get(&name).cloned() else {
            continue;
        };
        let Some((dx, dy)) = pull_shift(state, &members, mcu) else {
            continue;
        };
        let before_placed = state.placed.clone();
        let before_outline = state.outline;
        let before = judgement(board, before_outline, &before_placed);
        let before_error = pull_error(state, &mcu_group, mcu, &before);
        let docked: Vec<(usize, BoardEdge)> = members
            .iter()
            .filter(|&&i| state.component(i).kind == "connector")
            .filter_map(|&i| wall_of(state.rect(i), state.outline).map(|edge| (i, edge)))
            .collect();
        for &i in &members {
            state.shift(i, dx, dy);
        }
        let stays_docked = docked
            .iter()
            .all(|&(i, edge)| wall_of(state.rect(i), state.outline) == Some(edge));
        if !stays_docked {
            state.placed = before_placed;
            state.outline = before_outline;
            tried.insert(name);
            continue;
        }
        state.space_groups();
        compress_walls(board, state, deadline);
        let mcu_stays = mcu_parts
            .iter()
            .all(|&(i, center)| state.placed[i].center == center);
        let after = judgement(board, state.outline, &state.placed);
        let after_error = pull_error(state, &mcu_group, mcu, &after);
        if !after.passed || !mcu_stays || after_error >= before_error {
            state.placed = before_placed;
            state.outline = before_outline;
            tried.insert(name);
        }
    }
}

/// Error the pull minimises: the summed gap from every other group to the MCU
/// (the move's purpose), then the outline area (the round's target), then the
/// judge deficit.
fn pull_error(
    state: &State<'_>,
    mcu_group: &str,
    mcu: Rect,
    report: &PlacementJudgement,
) -> (i64, i64, f64) {
    let gaps = state
        .group_members()
        .iter()
        .filter(|(name, _)| name.as_str() != mcu_group)
        .map(|(_, members)| rect_gap(state.bounds(members), mcu).max(0))
        .sum();
    let area = state
        .outline
        .width_nm()
        .saturating_mul(state.outline.height_nm());
    (gaps, area, 100.0 - report.score)
}

/// The rigid shift that takes the group to `GROUP_GAP_MM` from the first
/// obstacle toward the MCU, rounded down to the grid. `None` when it is already
/// there, when a hinted part would have to move, or when no step fits.
fn pull_shift(state: &State<'_>, members: &[usize], mcu: Rect) -> Option<(i64, i64)> {
    if members.iter().any(|&i| state.pinned[i]) {
        return None;
    }
    let group = state.bounds(members);
    let group_mid = Point::new(
        group.min.x_nm / 2 + group.max.x_nm / 2,
        group.min.y_nm / 2 + group.max.y_nm / 2,
    );
    let mcu_mid = Point::new(
        mcu.min.x_nm / 2 + mcu.max.x_nm / 2,
        mcu.min.y_nm / 2 + mcu.max.y_nm / 2,
    );
    let gap_x = (group.min.x_nm - mcu.max.x_nm).max(mcu.min.x_nm - group.max.x_nm);
    let gap_y = (group.min.y_nm - mcu.max.y_nm).max(mcu.min.y_nm - group.max.y_nm);
    let along_x = gap_x >= gap_y;
    let toward_mcu = if along_x {
        group_mid.x_nm < mcu_mid.x_nm
    } else {
        group_mid.y_nm < mcu_mid.y_nm
    };
    let free = free_gap(state, members, group, along_x, toward_mcu);
    if free == i64::MAX {
        return None;
    }
    let travel = free - mm_to_nm(GROUP_GAP_MM);
    let grid = mm_to_nm(GRID_MM);
    if travel <= 0 || grid <= 0 {
        return None;
    }
    let distance = (travel / grid) * grid;
    if distance == 0 {
        return None;
    }
    let distance = if toward_mcu { distance } else { -distance };
    Some(if along_x {
        (distance, 0)
    } else {
        (0, distance)
    })
}

/// Distance from the group to the first obstacle in the chosen direction. Only
/// obstacles whose perpendicular span crosses the group are in the way.
fn free_gap(
    state: &State<'_>,
    members: &[usize],
    group: Rect,
    along_x: bool,
    forward: bool,
) -> i64 {
    let mut obstacles: Vec<Rect> = (0..state.placed.len())
        .filter(|i| !members.contains(i))
        .map(|i| state.rect(i))
        .collect();
    for (other, rect) in state.review_group_boxes() {
        if state.component(members[0]).group.as_deref() != Some(other.as_str()) {
            obstacles.push(rect);
        }
    }
    let holes: Vec<Rect> = (0..state.placed.len())
        .filter(|&i| state.component(i).kind == "mounting_hole")
        .map(|i| state.rect(i))
        .collect();
    for hole in holes {
        let rim = hole_rim_nm(state.board, hole);
        obstacles.push(Rect::new(
            Point::new(hole.min.x_nm - rim, hole.min.y_nm - rim),
            Point::new(hole.max.x_nm + rim, hole.max.y_nm + rim),
        ));
    }
    for index in 0..state.placed.len() {
        if !is_usb_connector(state.component(index)) || members.contains(&index) {
            continue;
        }
        let mut frame = state.rect(index);
        for member in state.attached_passives(index) {
            let rect = state.rect(member);
            frame = Rect::new(
                Point::new(
                    frame.min.x_nm.min(rect.min.x_nm),
                    frame.min.y_nm.min(rect.min.y_nm),
                ),
                Point::new(
                    frame.max.x_nm.max(rect.max.x_nm),
                    frame.max.y_nm.max(rect.max.y_nm),
                ),
            );
        }
        obstacles.push(frame);
    }
    let mut free = i64::MAX;
    for obstacle in obstacles {
        let crosses = if along_x {
            obstacle.min.y_nm <= group.max.y_nm && obstacle.max.y_nm >= group.min.y_nm
        } else {
            obstacle.min.x_nm <= group.max.x_nm && obstacle.max.x_nm >= group.min.x_nm
        };
        if !crosses {
            continue;
        }
        let gap = if along_x && forward {
            if obstacle.max.x_nm <= group.max.x_nm {
                continue;
            }
            obstacle.min.x_nm - group.max.x_nm
        } else if along_x {
            if obstacle.min.x_nm >= group.min.x_nm {
                continue;
            }
            group.min.x_nm - obstacle.max.x_nm
        } else if forward {
            if obstacle.max.y_nm <= group.max.y_nm {
                continue;
            }
            obstacle.min.y_nm - group.max.y_nm
        } else {
            if obstacle.min.y_nm >= group.min.y_nm {
                continue;
            }
            group.min.y_nm - obstacle.max.y_nm
        };
        free = free.min(gap.max(0));
    }
    free
}

/// Maximum feedback-loop iterations (`round 22` stop guard 1).
const REFINE_MAX_ITERATIONS: usize = 12;

/// Stop after two kept iterations whose error gain is below this fraction
/// (`round 22` stop guard 4).
const REFINE_GAIN_STOP: f64 = 0.01;

/// A refine fix may grow the outline area by at most this fraction over the
/// state before it. Without the guard a group pull trades a 100-per-pair error
/// drop for a doubled board, because the area weight is 0.01 per mm2.
const AREA_GUARD_STEP: f64 = 0.01;

/// The whole refine pass may grow the outline area by at most this fraction
/// over its entry.
const AREA_GUARD_TOTAL: f64 = 0.03;

/// A grouped part farther than this from every peer is a lone part.
const LONE_MM: f64 = 6.0;

/// Whether the feedback loop runs. `SYNTH_PLACE_REFINE=0` turns it off.
pub(crate) fn refine_enabled() -> bool {
    std::env::var_os("SYNTH_PLACE_REFINE").is_none_or(|value| value.to_str() != Some("0"))
}

/// One iteration of the round-22 feedback loop.
#[derive(Debug, Clone, PartialEq)]
pub struct RefineStep {
    pub finding: String,
    pub fix: String,
    pub error_before: f64,
    pub error_after: f64,
    pub kept: bool,
    /// Why a fix was rejected, when the error alone would have kept it.
    pub reason: Option<String>,
}

/// What the feedback loop did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RefineReport {
    pub iterations: usize,
    pub error_before: f64,
    pub error_after: f64,
    pub steps: Vec<RefineStep>,
}

#[derive(Clone, Copy)]
enum RefineFix {
    NudgeOverhang,
    CentreUsb,
    DockLoose,
    PullGroups,
    PackHeaders,
    MoveLone,
}

struct AppliedFix {
    moved: bool,
}

#[derive(Clone)]
struct RefineFinding {
    key: String,
    fix: RefineFix,
    contribution: f64,
    step_mm: f64,
    part: Option<usize>,
}

/// Round-22 self-correcting loop: score the finished placement, fix the worst
/// finding, re-score, and keep the change only when the total error falls.
///
/// The error is the weighted sum from the placement judge: overlaps, overhang,
/// USB offset, undocked connectors, close groups, header row, lone parts,
/// largest empty rectangle, area, alignment and ring. The loop stops at 12
/// iterations, when no untried finding remains, after two kept iterations
/// gain less than 1 %, or when its wall-clock budget (90 s, or
/// `SYNTH_PLACE_REFINE_BUDGET_S`) expires; expiry keeps the last accepted
/// placement and writes `stop budget` to the trace. A fix that raises the
/// outline area by more than `AREA_GUARD_STEP` over the state before it is
/// rejected, and one that would take the pass past `AREA_GUARD_TOTAL` over
/// its entry stops it; both record `area_guard` in the trace. Every iteration
/// is appended to the file named by `SYNTH_REFINE_TRACE`; with it unset no
/// trace is written.
/// `SYNTH_PLACE_REFINE=0` disables the loop.
pub fn refine_by_feedback(board: &Board, placement: &mut Placement) -> RefineReport {
    let trace_start = std::time::Instant::now();
    let deadline =
        std::time::Instant::now() + budget_seconds("SYNTH_PLACE_REFINE_BUDGET_S", REFINE_BUDGET_S);
    if !refine_enabled() {
        let before = refine_error(board, placement);
        return RefineReport {
            error_before: before,
            error_after: before,
            ..RefineReport::default()
        };
    }
    let trace = refine_trace_path();
    if let Some(trace) = &trace {
        let _ = std::fs::write(
            trace,
            "iteration\tfinding\tfix\terror_before\terror_after\tkept\treason\n",
        );
    }
    if std::time::Instant::now() >= deadline {
        append_refine_stop(trace.as_deref(), "budget");
        return RefineReport::default();
    }
    let before = refine_error(board, placement);
    let mut report = RefineReport {
        error_before: before,
        error_after: before,
        ..RefineReport::default()
    };
    let mut accepted = before;
    let entry_area = outline_area(placement);
    let mut tried: BTreeSet<String> = BTreeSet::new();
    let mut flat = 0_usize;
    for iteration in 0..REFINE_MAX_ITERATIONS {
        if std::time::Instant::now() >= deadline {
            append_refine_stop(trace.as_deref(), "budget");
            report.error_after = accepted;
            return report;
        }
        let stage_start = std::time::Instant::now();
        let Some(finding) = refine_findings(board, placement)
            .into_iter()
            .filter(|finding| !tried.contains(&finding.key))
            .max_by(|a, b| {
                a.contribution
                    .total_cmp(&b.contribution)
                    .then_with(|| b.key.cmp(&a.key))
            })
        else {
            break;
        };
        let error_before = refine_error(board, placement);
        let mut candidate = placement.clone();
        let applied = apply_refine_fix(board, &mut candidate, &finding);
        if std::time::Instant::now() >= deadline {
            // The fix was not re-scored, so keep the accepted placement.
            append_refine_stop(trace.as_deref(), "budget");
            report.error_after = accepted;
            return report;
        }
        let (error_after, kept, reason, over_total) = if applied.moved {
            crate::finish_outline_base(board, &mut candidate);
            let error = refine_error(board, &candidate);
            let guard = area_guard(
                entry_area,
                outline_area(placement),
                outline_area(&candidate),
            );
            if guard.rejects() {
                (
                    error,
                    false,
                    Some("area_guard".to_string()),
                    guard.over_total,
                )
            } else if error < error_before {
                *placement = candidate;
                accepted = error;
                (error, true, None, false)
            } else {
                (error, false, None, false)
            }
        } else {
            (error_before, false, None, false)
        };
        if kept {
            crate::stage_trace::record(
                refine_fix_name(finding.fix),
                board,
                placement,
                stage_start.elapsed(),
            );
        } else {
            tried.insert(finding.key.clone());
        }
        let step = RefineStep {
            finding: finding.key.clone(),
            fix: refine_fix_name(finding.fix).to_string(),
            error_before,
            error_after,
            kept,
            reason,
        };
        report.steps.push(step.clone());
        report.iterations += 1;
        append_refine_trace(trace.as_deref(), iteration + 1, &step);
        if over_total {
            append_refine_stop(trace.as_deref(), "area_guard");
            break;
        }
        if kept {
            let gain = if error_before > 0.0 {
                (error_before - error_after) / error_before
            } else {
                0.0
            };
            if gain < REFINE_GAIN_STOP {
                flat += 1;
            } else {
                flat = 0;
            }
            if flat >= 2 {
                break;
            }
        }
    }
    report.error_after = refine_error(board, placement);
    crate::stage_trace::record(
        "refine_by_feedback",
        board,
        placement,
        trace_start.elapsed(),
    );
    report
}

/// Outline area in nm2, as a float for the guard's percentage comparisons.
#[allow(clippy::cast_precision_loss)] // Board dimensions are far below 2^53 nm.
fn outline_area(placement: &Placement) -> f64 {
    (placement.board_outline.width_nm() as f64) * (placement.board_outline.height_nm() as f64)
}

/// The area guard's verdict on a fix that grows the outline to `grown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AreaGuard {
    /// Past `AREA_GUARD_TOTAL` over the pass's entry outline.
    over_total: bool,
    /// Past `AREA_GUARD_STEP` over the outline before this fix.
    over_step: bool,
}

impl AreaGuard {
    fn rejects(self) -> bool {
        self.over_total || self.over_step
    }
}

fn area_guard(entry_area: f64, current_area: f64, grown: f64) -> AreaGuard {
    AreaGuard {
        over_total: grown > entry_area * (1.0 + AREA_GUARD_TOTAL),
        over_step: grown > current_area * (1.0 + AREA_GUARD_STEP),
    }
}

fn refine_fix_name(fix: RefineFix) -> &'static str {
    match fix {
        RefineFix::NudgeOverhang => "nudge_overhang_inside",
        RefineFix::CentreUsb => "centre_usb_blocks",
        RefineFix::DockLoose => "dock_loose_connectors",
        RefineFix::PullGroups => "pull_groups_inward",
        RefineFix::PackHeaders => "pack_edge_headers",
        RefineFix::MoveLone => "move_lone_part",
    }
}

/// The weighted error from the judge plus the lone-part count.
fn refine_error(board: &Board, placement: &Placement) -> f64 {
    let report = crate::judge::judge(board, placement);
    let failures =
        |key: &str| judge_metric(&report, key).map_or(0, |metric| metric.failures) as f64;
    let measured = |key: &str| judge_metric(&report, key).map_or(0.0, |metric| metric.measured);
    let score = |key: &str| {
        judge_metric(&report, key)
            .and_then(|metric| metric.score)
            .unwrap_or(1.0)
    };
    failures("overlaps") * 1000.0
        + failures("outside_outline") * 100.0
        + measured("usb_offset") * 10.0
        + failures("docking") * 50.0
        + failures("group_gap") * 100.0
        + failures("header_alignment") * 50.0
        + lone_parts(board, placement).len() as f64 * 5.0
        + measured("largest_empty") * 0.05
        + measured("outline_area") * 0.01
        + (1.0 - score("alignment")) * 20.0
        + (1.0 - score("ring")) * 20.0
}

/// Grouped parts with no peer within [`LONE_MM`].
fn lone_parts(board: &Board, placement: &Placement) -> Vec<usize> {
    let reach = mm_to_nm(LONE_MM);
    let mut lone = Vec::new();
    for (index, placed) in placement.components.iter().enumerate() {
        let Some(component) = board.component(placed.id) else {
            continue;
        };
        let Some(group) = component.group.as_ref() else {
            continue;
        };
        if component.kind == "mounting_hole" || synth_layout::is_board_feature_kind(&component.kind)
        {
            continue;
        }
        let rect = sidecar_courtyard_rect(board, component, placed);
        let has_peer = placement
            .components
            .iter()
            .enumerate()
            .any(|(other, peer)| {
                other != index
                    && board.component(peer.id).is_some_and(|peer_component| {
                        peer_component.group.as_ref() == Some(group)
                            && rect_gap(rect, sidecar_courtyard_rect(board, peer_component, peer))
                                <= reach
                    })
            });
        if !has_peer {
            lone.push(index);
        }
    }
    lone
}

/// The findings the loop can act on, worst weighted contribution first.
fn refine_findings(board: &Board, placement: &Placement) -> Vec<RefineFinding> {
    let report = crate::judge::judge(board, placement);
    let failures =
        |key: &str| judge_metric(&report, key).map_or(0, |metric| metric.failures) as f64;
    let measured = |key: &str| judge_metric(&report, key).map_or(0.0, |metric| metric.measured);
    let mut findings = Vec::new();

    for (index, placed) in placement.components.iter().enumerate() {
        let Some(component) = board.component(placed.id) else {
            continue;
        };
        let rect = sidecar_courtyard_rect(board, component, placed);
        if !inside(rect, placement.board_outline) {
            findings.push(RefineFinding {
                key: format!("overhang:{}", component.refdes),
                fix: RefineFix::NudgeOverhang,
                contribution: 100.0,
                step_mm: 2.0,
                part: Some(index),
            });
        }
    }
    let usb = measured("usb_offset");
    if failures("usb_offset") > 0.0 {
        findings.push(RefineFinding {
            key: "usb_offset".to_string(),
            fix: RefineFix::CentreUsb,
            contribution: usb * 10.0,
            step_mm: (usb * 1.0).min(2.0),
            part: None,
        });
    }
    if failures("docking") > 0.0 {
        findings.push(RefineFinding {
            key: "docking".to_string(),
            fix: RefineFix::DockLoose,
            contribution: failures("docking") * 50.0,
            step_mm: 2.0,
            part: None,
        });
    }
    let close = failures("group_gap");
    if close > 0.0 {
        let contribution = close * 100.0;
        findings.push(RefineFinding {
            key: "group_gap".to_string(),
            fix: RefineFix::PullGroups,
            contribution,
            step_mm: (contribution * 0.1).min(2.0),
            part: None,
        });
    }
    if failures("header_alignment") > 0.0 {
        findings.push(RefineFinding {
            key: "header_alignment".to_string(),
            fix: RefineFix::PackHeaders,
            contribution: failures("header_alignment") * 50.0,
            step_mm: 2.0,
            part: None,
        });
    }
    for index in lone_parts(board, placement) {
        let refdes = placement
            .components
            .get(index)
            .and_then(|placed| board.component(placed.id))
            .map_or_else(|| "part".to_string(), |component| component.refdes.clone());
        findings.push(RefineFinding {
            key: format!("lone:{refdes}"),
            fix: RefineFix::MoveLone,
            contribution: 5.0,
            step_mm: 0.5,
            part: Some(index),
        });
    }
    findings
}

/// Apply one finding's fix to `placement`; false when it cannot move anything.
fn apply_refine_fix(
    board: &Board,
    placement: &mut Placement,
    finding: &RefineFinding,
) -> AppliedFix {
    let mut state = compression_state(board, placement);
    let before = state.placed.clone();
    match finding.fix {
        RefineFix::NudgeOverhang => {
            let Some(index) = finding.part else {
                return AppliedFix { moved: false };
            };
            if state.pinned[index] {
                return AppliedFix { moved: false };
            }
            let step = mm_to_nm(finding.step_mm);
            // Move it the rim inside the outline, not merely inside: the
            // pass refits at that rim, and stopping at the edge would grow
            // the outline the area guard is there to protect.
            let rim = rim_nm(board, &state.placed, true);
            let (dx, dy) = inside_shift(state.rect(index), state.outline, step, rim);
            if dx == 0 && dy == 0 {
                return AppliedFix { moved: false };
            }
            state.shift(index, dx, dy);
        }
        RefineFix::CentreUsb => state.centre_usb_blocks(),
        RefineFix::DockLoose => state.dock_loose_connectors(),
        RefineFix::PullGroups => {
            let deadline = std::time::Instant::now()
                + budget_seconds("SYNTH_PLACE_COMPRESS_BUDGET_S", COMPRESS_BUDGET_S);
            pull_groups(board, &mut state, deadline);
            state.space_groups();
        }
        RefineFix::PackHeaders => state.pack_edge_headers(),
        RefineFix::MoveLone => {
            let Some(index) = finding.part else {
                return AppliedFix { moved: false };
            };
            if state.pinned[index] {
                return AppliedFix { moved: false };
            }
            let Some(group) = state.component(index).group.clone() else {
                return AppliedFix { moved: false };
            };
            let rect = state.rect(index);
            let peer = (0..state.placed.len())
                .filter(|&other| other != index)
                .filter(|&other| state.component(other).group.as_ref() == Some(&group))
                .min_by_key(|&other| rect_gap(rect, state.rect(other)));
            let Some(peer) = peer else {
                return AppliedFix { moved: false };
            };
            let from = state.placed[index].center;
            let to = state.placed[peer].center;
            let step = mm_to_nm(finding.step_mm);
            let toward = |a: i64, b: i64| {
                if (b - a).abs() <= step {
                    b - a
                } else {
                    (b - a).signum() * step
                }
            };
            let (dx, dy) = (toward(from.x_nm, to.x_nm), toward(from.y_nm, to.y_nm));
            if dx == 0 && dy == 0 {
                return AppliedFix { moved: false };
            }
            state.shift(index, dx, dy);
        }
    }
    if state.placed == before {
        return AppliedFix { moved: false };
    }
    placement.components = state.placed.clone();
    placement.board_outline = state.outline;
    AppliedFix { moved: true }
}

/// Shift `rect` back inside `outline` by at most `step` on each axis, keeping
/// `rim` between it and the edge the outline will be refitted at.
fn inside_shift(rect: Rect, outline: Rect, step: i64, rim: i64) -> (i64, i64) {
    let mut dx = 0;
    let mut dy = 0;
    // Only an axis that actually overhangs moves; shifting a part that is
    // merely closer to the edge than the rim would break its row.
    if rect.min.x_nm < outline.min.x_nm {
        dx = (outline.min.x_nm + rim - rect.min.x_nm).min(step);
    } else if rect.max.x_nm > outline.max.x_nm {
        dx = -(rect.max.x_nm + rim - outline.max.x_nm).min(step);
    }
    if rect.min.y_nm < outline.min.y_nm {
        dy = (outline.min.y_nm + rim - rect.min.y_nm).min(step);
    } else if rect.max.y_nm > outline.max.y_nm {
        dy = -(rect.max.y_nm + rim - outline.max.y_nm).min(step);
    }
    (dx, dy)
}

fn refine_trace_path() -> Option<std::path::PathBuf> {
    std::env::var_os("SYNTH_REFINE_TRACE").map(std::path::PathBuf::from)
}

fn append_refine_stop(path: Option<&std::path::Path>, reason: &str) {
    use std::io::Write as _;
    let Some(path) = path else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(file, "stop\t{reason}");
}

fn append_refine_trace(path: Option<&std::path::Path>, iteration: usize, step: &RefineStep) {
    use std::io::Write as _;
    let Some(path) = path else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(
        file,
        "{iteration}\t{}\t{}\t{:.6}\t{:.6}\t{}\t{}",
        step.finding,
        step.fix,
        step.error_before,
        step.error_after,
        step.kept,
        step.reason.as_deref().unwrap_or("-")
    );
}

/// Wall-compression trace. `SYNTH_PLACE_COMPRESS_TRACE=1` writes
/// `compress_trace.tsv` beside `SYNTH_REFINE_TRACE` (the export directory);
/// any other non-zero value is the trace file itself. Off writes nothing.
struct CompressTrace {
    path: Option<std::path::PathBuf>,
}

impl CompressTrace {
    fn from_env() -> Self {
        let Some(value) = std::env::var_os("SYNTH_PLACE_COMPRESS_TRACE") else {
            return Self { path: None };
        };
        let Some(value) = value.to_str() else {
            return Self { path: None };
        };
        let path = match value {
            "" | "0" => None,
            "1" => Some(
                refine_trace_path()
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join("compress_trace.tsv"),
            ),
            other => Some(std::path::PathBuf::from(other)),
        };
        Self { path }
    }

    fn append(&self, fields: &[&str]) {
        use std::io::Write as _;
        let Some(path) = &self.path else {
            return;
        };
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        if file.metadata().is_ok_and(|meta| meta.len() == 0) {
            let _ = writeln!(
                file,
                "side\tstep\tmove_ok\tbefore_failures\tafter_failures\treason\tseconds"
            );
        }
        let _ = writeln!(file, "{}", fields.join("\t"));
    }
}

fn failing_gates(report: &PlacementJudgement) -> String {
    let gates: Vec<&str> = report
        .metrics
        .iter()
        .filter(|metric| metric.failures > 0)
        .map(|metric| metric.key.as_str())
        .collect();
    if gates.is_empty() {
        "-".to_string()
    } else {
        gates.join(",")
    }
}

fn compression_state<'a>(board: &'a Board, placement: &Placement) -> State<'a> {
    let pinned: Vec<bool> = placement
        .components
        .iter()
        .map(|placed| {
            board
                .component(placed.id)
                .is_none_or(|component| component.placement_hint.is_some())
        })
        .collect();
    let (gap_nm, infl, group_gap_nm) = spacing_gaps(board);
    State {
        board,
        anchored: pinned.clone(),
        pinned,
        placed: placement.components.clone(),
        outline: placement.board_outline,
        keep_clean: true,
        known: Vec::new(),
        usb_only: false,
        gap_nm,
        infl,
        group_gap_nm,
        rim: 0,
        reserved: Vec::new(),
        spread_keep: Vec::new(),
    }
}

fn judgement(
    board: &Board,
    outline: Rect,
    components: &[ComponentPlacement],
) -> PlacementJudgement {
    crate::judge::judge(
        board,
        &Placement {
            board_outline: outline,
            components: components.to_vec(),
        },
    )
}

fn judge_metric<'a>(report: &'a PlacementJudgement, key: &str) -> Option<&'a JudgeMetric> {
    report.metrics.iter().find(|metric| metric.key == key)
}

/// A step is accepted only when it adds no gate failure and no quality row got
/// worse beyond the allowed slack. On a board that already fails a gate a step
/// may still be kept when every gate's failure count stays the same or falls;
/// on a clean board that is the same as requiring the judge to pass, and the
/// composite may not fall either. Alignment and ring may drop by
/// `COMPRESS_QUALITY_SLACK`; the other rows may not drop at all.
fn compression_is_no_worse(before: &PlacementJudgement, after: &PlacementJudgement) -> bool {
    if !gates_not_worse(before, after) {
        return false;
    }
    if before.passed && after.score + COMPRESS_SCORE_SLACK < before.score {
        return false;
    }
    match (
        judge_metric(before, "docking"),
        judge_metric(after, "docking"),
    ) {
        (Some(before), Some(after)) => {
            if after.measured + f64::EPSILON < before.measured {
                return false;
            }
        }
        (Some(_), None) => return false,
        _ => {}
    }
    // Header quality is `1 - spread / limit`, so "not lower" is a score
    // comparison even though the row is a millimetre spread.
    match (
        judge_metric(before, "header_alignment"),
        judge_metric(after, "header_alignment"),
    ) {
        (Some(before), Some(after)) => {
            if after.score.unwrap_or(0.0) + f64::EPSILON < before.score.unwrap_or(0.0) {
                return false;
            }
        }
        (Some(_), None) => return false,
        _ => {}
    }
    for key in ["alignment", "ring"] {
        match (judge_metric(before, key), judge_metric(after, key)) {
            (Some(before), Some(after)) => {
                if after.measured + COMPRESS_QUALITY_SLACK + f64::EPSILON < before.measured {
                    return false;
                }
            }
            (Some(_), None) => return false,
            _ => {}
        }
    }
    true
}

/// Every gated row fails no more often after the step than before it. A row
/// that was not measured before and has a failure after is a new failing gate.
fn gates_not_worse(before: &PlacementJudgement, after: &PlacementJudgement) -> bool {
    after
        .metrics
        .iter()
        .filter(|metric| metric.limit.is_some())
        .all(|after| {
            let before_failures =
                judge_metric(before, &after.key).map_or(0, |metric| metric.failures);
            after.failures <= before_failures
        })
}

/// Connectors on a wall before a step, and which wall each sits on. They are
/// re-seated after the group spacing pass may have moved them.
fn docked_sides(state: &State<'_>) -> Vec<(usize, BoardEdge)> {
    let mut docked = Vec::new();
    for index in 0..state.placed.len() {
        let component = state.component(index);
        if component.kind != "connector" {
            continue;
        }
        if let Some(edge) = component
            .placement_hint
            .as_ref()
            .and_then(|hint| hint.edge.as_ref())
        {
            docked.push((index, board_edge_of(edge)));
            continue;
        }
        if let Some(edge) = wall_of(state.rect(index), state.outline) {
            docked.push((index, edge));
        }
    }
    docked
}

fn board_edge_of(edge: &synth_ir::PlacementEdge) -> BoardEdge {
    match edge {
        synth_ir::PlacementEdge::Top => BoardEdge::Top,
        synth_ir::PlacementEdge::Right => BoardEdge::Right,
        synth_ir::PlacementEdge::Bottom => BoardEdge::Bottom,
        synth_ir::PlacementEdge::Left => BoardEdge::Left,
    }
}

/// The wall a courtyard sits on, when it is within [`COMPRESS_FLUSH_MM`].
fn wall_of(rect: Rect, outline: Rect) -> Option<BoardEdge> {
    let gaps = [
        (rect.min.y_nm - outline.min.y_nm, BoardEdge::Top),
        (outline.max.x_nm - rect.max.x_nm, BoardEdge::Right),
        (outline.max.y_nm - rect.max.y_nm, BoardEdge::Bottom),
        (rect.min.x_nm - outline.min.x_nm, BoardEdge::Left),
    ];
    let (gap, edge) = gaps
        .into_iter()
        .min_by_key(|(gap, _)| *gap)
        .expect("four edges");
    (nm_to_mm(gap.abs()) <= COMPRESS_FLUSH_MM).then_some(edge)
}

/// Connectors flush on `side` and the passives that ride with them.
fn moving_with_wall(state: &State<'_>, side: BoardEdge) -> Vec<bool> {
    let mut moving = vec![false; state.placed.len()];
    for index in 0..state.placed.len() {
        if state.component(index).kind != "connector"
            || wall_of(state.rect(index), state.outline) != Some(side)
        {
            continue;
        }
        moving[index] = true;
        if let Some(frame) = state.attached_frame(index) {
            for member in frame {
                moving[member] = true;
            }
        }
    }
    moving
}

/// How far `side` can move before a stationary part leaves the outline.
fn wall_room(state: &State<'_>, side: BoardEdge) -> i64 {
    let outline = state.outline;
    let moving = moving_with_wall(state, side);
    let holes = state.free_holes();
    let mut room = i64::MAX;
    for (index, &moved) in moving.iter().enumerate() {
        if moved || holes.contains(&index) {
            continue;
        }
        let rect = state.rect(index);
        let gap = match side {
            BoardEdge::Left => rect.min.x_nm - outline.min.x_nm,
            BoardEdge::Right => outline.max.x_nm - rect.max.x_nm,
            BoardEdge::Top => rect.min.y_nm - outline.min.y_nm,
            BoardEdge::Bottom => outline.max.y_nm - rect.max.y_nm,
        };
        room = room.min(gap);
    }
    room
}

/// The largest distance `side` can move inward such that every stationary part
/// stays inside the new outline and every moving part keeps the part and group
/// gaps from every stationary one. Rectangle arithmetic, no judge.
fn wall_jump_limit(state: &State<'_>, side: BoardEdge) -> i64 {
    let moving = moving_with_wall(state, side);
    let holes = state.free_holes();
    let need = state.group_gap_nm.max(state.gap_nm);
    let mut limit = wall_room(state, side);
    if limit == i64::MAX {
        return 0;
    }
    for mover in 0..state.placed.len() {
        if !moving[mover] {
            continue;
        }
        let rect = state.rect(mover);
        for (index, &moved) in moving.iter().enumerate() {
            if moved || holes.contains(&index) {
                continue;
            }
            let other = state.rect(index);
            let cross = match side {
                BoardEdge::Left | BoardEdge::Right => {
                    (other.min.y_nm - rect.max.y_nm).max(rect.min.y_nm - other.max.y_nm)
                }
                BoardEdge::Top | BoardEdge::Bottom => {
                    (other.min.x_nm - rect.max.x_nm).max(rect.min.x_nm - other.max.x_nm)
                }
            };
            if cross >= need {
                continue;
            }
            let along = match side {
                BoardEdge::Left => other.min.x_nm - rect.max.x_nm,
                BoardEdge::Right => rect.min.x_nm - other.max.x_nm,
                BoardEdge::Top => other.min.y_nm - rect.max.y_nm,
                BoardEdge::Bottom => rect.min.y_nm - other.max.y_nm,
            };
            if along < 0 {
                continue;
            }
            limit = limit.min(along - need);
        }
    }
    limit.max(0)
}

/// Move one wall inward by `distance` and carry the parts that must follow it.
/// Returns `false` when a part that stays put would leave the new outline.
fn move_wall(state: &mut State<'_>, side: BoardEdge, distance: i64) -> bool {
    let old = state.outline;
    let mut new = old;
    match side {
        BoardEdge::Left => new.min.x_nm += distance,
        BoardEdge::Right => new.max.x_nm -= distance,
        BoardEdge::Top => new.min.y_nm += distance,
        BoardEdge::Bottom => new.max.y_nm -= distance,
    }
    if new.width_nm() <= 0 || new.height_nm() <= 0 {
        return false;
    }

    let moving = moving_with_wall(state, side);
    let holes = state.free_holes();
    for (index, &moved) in moving.iter().enumerate() {
        if moved || holes.contains(&index) {
            continue;
        }
        let rect = state.rect(index);
        if !inside(rect, new) {
            return false;
        }
    }

    let (dx, dy) = match side {
        BoardEdge::Left => (distance, 0),
        BoardEdge::Right => (-distance, 0),
        BoardEdge::Top => (0, distance),
        BoardEdge::Bottom => (0, -distance),
    };
    for (index, move_part) in moving.iter().enumerate() {
        if *move_part {
            state.shift(index, dx, dy);
        }
    }
    state.outline = new;
    place_holes(state);
    true
}

/// Move one wall by `COMPRESS_STEP_MM`, keeping the round-21 behaviour.
fn shrink_side(state: &mut State<'_>, side: BoardEdge) -> bool {
    move_wall(state, side, mm_to_nm(COMPRESS_STEP_MM))
}

/// Jump `side` inward to [`wall_jump_limit`], halving a judge-rejected distance
/// up to `COMPRESS_JUMP_TRIES` times. The wall, its docked connectors and the
/// free holes move; no group is re-packed and no settle runs, so a group that
/// contains a hole can not drag it out of its corner. Leaves the state before
/// the jump when every try is rejected.
fn jump_wall(
    board: &Board,
    state: &mut State<'_>,
    side: BoardEdge,
    deadline: std::time::Instant,
    trace: &CompressTrace,
) -> bool {
    let grid = mm_to_nm(COMPRESS_STEP_MM);
    let mut distance = wall_jump_limit(state, side) / grid * grid;
    if distance < grid {
        return false;
    }
    let before_placed = state.placed.clone();
    let before_outline = state.outline;
    let before = judgement(board, before_outline, &before_placed);
    for _ in 0..COMPRESS_JUMP_TRIES {
        if std::time::Instant::now() >= deadline {
            trace.append(&[
                &format!("{side:?}"),
                &format!("jump:{:.1}", nm_to_mm(distance)),
                "false",
                &failing_gates(&before),
                "-",
                "budget",
                "0.000",
            ]);
            return false;
        }
        let started = std::time::Instant::now();
        let step = format!("jump:{:.1}", nm_to_mm(distance));
        if move_wall(state, side, distance) {
            // The wall length changed, so a USB on it moves to the new middle.
            centre_usb(state);
            let after = judgement(board, state.outline, &state.placed);
            let kept = compression_is_no_worse(&before, &after);
            trace.append(&[
                &format!("{side:?}"),
                &step,
                "true",
                &failing_gates(&before),
                &failing_gates(&after),
                if kept { "kept" } else { "judge_worse" },
                &format!("{:.3}", started.elapsed().as_secs_f64()),
            ]);
            if kept {
                return true;
            }
            state.placed.clone_from(&before_placed);
            state.outline = before_outline;
        } else {
            trace.append(&[
                &format!("{side:?}"),
                &step,
                "false",
                &failing_gates(&before),
                "-",
                "shrink_false",
                &format!("{:.3}", started.elapsed().as_secs_f64()),
            ]);
        }
        distance = distance / 2 / grid * grid;
        if distance < grid {
            break;
        }
    }
    false
}

fn inside(rect: Rect, outline: Rect) -> bool {
    outline.contains(rect.min) && outline.contains(rect.max)
}

/// Put every free hole back in a corner of the current outline, 3.5 mm in.
fn place_holes(state: &mut State<'_>) {
    let holes = state.free_holes();
    if holes.is_empty() {
        return;
    }
    let sizes = state.hole_sizes(&holes);
    for (&index, spot) in holes
        .iter()
        .zip(hole_spots(state.board, state.outline, &sizes))
    {
        state.put_corner(index, spot.x_nm, spot.y_nm);
    }
}

/// Dock every connector on its recorded wall, or the nearest wall, after the
/// group spacing pass, keeping its frame's offsets.
fn dock_all_connectors(state: &mut State<'_>, docked: &[(usize, BoardEdge)]) {
    for index in 0..state.placed.len() {
        if state.component(index).kind != "connector" {
            continue;
        }
        let Some(side) = docked
            .iter()
            .find_map(|(docked_index, side)| (*docked_index == index).then_some(*side))
            .or_else(|| nearest_wall(state.rect(index), state.outline))
        else {
            continue;
        };
        let Some(spot) = state.edge_slot(index, side, state.outline) else {
            continue;
        };
        let rect = state.rect(index);
        let (dx, dy) = (spot.x_nm - rect.min.x_nm, spot.y_nm - rect.min.y_nm);
        if dx == 0 && dy == 0 {
            continue;
        }
        let frame = state.attached_frame(index).unwrap_or_default();
        state.shift_with_attached(index, dx, dy, &frame);
    }
}

fn nearest_wall(rect: Rect, outline: Rect) -> Option<BoardEdge> {
    let gaps = [
        (rect.min.y_nm - outline.min.y_nm, BoardEdge::Top),
        (outline.max.x_nm - rect.max.x_nm, BoardEdge::Right),
        (outline.max.y_nm - rect.max.y_nm, BoardEdge::Bottom),
        (rect.min.x_nm - outline.min.x_nm, BoardEdge::Left),
    ];
    gaps.into_iter()
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, edge)| edge)
}

/// Slide each flush USB connector and its frame to the middle of its wall.
fn centre_usb(state: &mut State<'_>) {
    for index in 0..state.placed.len() {
        if !is_usb_connector(state.component(index)) {
            continue;
        }
        let Some(side) = wall_of(state.rect(index), state.outline) else {
            continue;
        };
        let rect = state.rect(index);
        let outline = state.outline;
        let (mut dx, mut dy) = match side {
            BoardEdge::Left | BoardEdge::Right => {
                let mid = outline.min.y_nm / 2 + outline.max.y_nm / 2;
                let centre = rect.min.y_nm / 2 + rect.max.y_nm / 2;
                (0, mid - centre)
            }
            BoardEdge::Top | BoardEdge::Bottom => {
                let mid = outline.min.x_nm / 2 + outline.max.x_nm / 2;
                let centre = rect.min.x_nm / 2 + rect.max.x_nm / 2;
                (mid - centre, 0)
            }
        };
        if dx == 0 && dy == 0 {
            continue;
        }
        // Keep the connector inside the wall it just joined.
        let (width, height) = (rect.width_nm(), rect.height_nm());
        let x = (rect.min.x_nm + dx).clamp(
            outline.min.x_nm,
            (outline.max.x_nm - width).max(outline.min.x_nm),
        );
        let y = (rect.min.y_nm + dy).clamp(
            outline.min.y_nm,
            (outline.max.y_nm - height).max(outline.min.y_nm),
        );
        dx = x - rect.min.x_nm;
        dy = y - rect.min.y_nm;
        let frame = state.attached_frame(index).unwrap_or_default();
        state.shift_with_attached(index, dx, dy, &frame);
    }
}

fn is_usb_connector(component: &Component) -> bool {
    component.part.as_ref().is_some_and(|part| {
        let id = part.id.0.to_ascii_lowercase();
        id.contains("usb") || id.contains("type-c")
    })
}

/// Top-left corner for each hole of `sizes` inside `hull`: all four corners
/// for four holes, three for three, a diagonal for two, one for one.
fn hole_spots(board: &Board, hull: Rect, sizes: &[(i64, i64)]) -> Vec<Point> {
    // (at the left, at the top)
    let sides: &[(bool, bool)] = match sizes.len() {
        4 => &[(true, true), (false, true), (true, false), (false, false)],
        3 => &[(true, true), (false, true), (true, false)],
        2 => &[(true, true), (false, false)],
        1 => &[(true, true)],
        _ => &[],
    };
    sizes
        .iter()
        .zip(sides)
        .map(|(&(w, h), &(left, top))| {
            let rim = hole_rim_nm(board, Rect::new(Point::new(0, 0), Point::new(w, h)));
            Point::new(
                if left {
                    hull.min.x_nm + rim
                } else {
                    hull.max.x_nm - rim - w
                },
                if top {
                    hull.min.y_nm + rim
                } else {
                    hull.max.y_nm - rim - h
                },
            )
        })
        .collect()
}

/// Top-left corner of a connector of `size` seated on `edge` of `outline`,
/// centred along it, `margin` in from the edge.
fn usb_spot(outline: Rect, (w, h): (i64, i64), edge: BoardEdge, margin: i64) -> Point {
    let cx = outline.min.x_nm / 2 + outline.max.x_nm / 2 - w / 2;
    let cy = outline.min.y_nm / 2 + outline.max.y_nm / 2 - h / 2;
    match edge {
        BoardEdge::Top => Point::new(cx, outline.min.y_nm + margin),
        BoardEdge::Bottom => Point::new(cx, outline.max.y_nm - margin - h),
        BoardEdge::Left => Point::new(outline.min.x_nm + margin, cy),
        BoardEdge::Right => Point::new(outline.max.x_nm - margin - w, cy),
    }
}

/// Every ordering of `items` (Heap's algorithm), in a fixed order.
fn permutations<T: Copy>(items: &[T]) -> Vec<Vec<T>> {
    fn go<T: Copy>(k: usize, items: &mut Vec<T>, out: &mut Vec<Vec<T>>) {
        if k <= 1 {
            out.push(items.clone());
            return;
        }
        for i in 0..k {
            go(k - 1, items, out);
            items.swap(if k.is_multiple_of(2) { i } else { 0 }, k - 1);
        }
    }
    let mut out = Vec::new();
    go(items.len(), &mut items.to_vec(), &mut out);
    out
}

/// A USB connector and the group it belongs to, moved as one block.
struct UsbBlock {
    usb: usize,
    members: Vec<usize>,
    /// The connector's courtyard size.
    size: (i64, i64),
    /// The connector's corner relative to the block's.
    offset: (i64, i64),
    /// The block's size.
    extent: (i64, i64),
    edge: BoardEdge,
}

/// A rigid quarter turn about a part, then a translation: an offset maps
/// through one matrix op. `rot90` counts the turns the matrix makes.
#[derive(Clone, Copy, Default)]
struct Frame {
    rot90: u8,
    dx: i64,
    dy: i64,
}

impl Frame {
    fn map(self, x: i64, y: i64) -> (i64, i64) {
        let (x, y) = match self.rot90 & 3 {
            0 => (x, y),
            1 => (-y, x),
            2 => (-x, -y),
            _ => (y, -x),
        };
        (x + self.dx, y + self.dy)
    }

    /// The rotation a part has after the frame's turn: the frame's matrix
    /// is the enum's, so a positive turn subtracts a quarter from degrees.
    fn turn(rotation: Rotation, rot90: u8) -> Rotation {
        match (rotation.degrees() - 90 * i32::from(rot90 & 3)).rem_euclid(360) {
            0 => Rotation::Zero,
            90 => Rotation::Ninety,
            180 => Rotation::OneEighty,
            _ => Rotation::TwoSeventy,
        }
    }
}

impl State<'_> {
    /// Snapshot the state into the stage trace without moving anything.
    fn trace(&self, stage: &str, since: std::time::Instant) {
        if !crate::stage_trace::enabled() {
            return;
        }
        crate::stage_trace::record(
            stage,
            self.board,
            &crate::Placement {
                board_outline: self.outline,
                components: self.placed.clone(),
            },
            since.elapsed(),
        );
    }

    fn component(&self, index: usize) -> &Component {
        self.board
            .component(self.placed[index].id)
            .expect("placement ids come from the board")
    }

    fn rect(&self, index: usize) -> Rect {
        sidecar_courtyard_rect(self.board, self.component(index), &self.placed[index])
    }

    fn overlaps(&self) -> usize {
        let rects: Vec<Rect> = (0..self.placed.len()).map(|i| self.rect(i)).collect();
        (0..rects.len())
            .map(|i| {
                rects[i + 1..]
                    .iter()
                    .filter(|r| rects[i].intersects(r))
                    .count()
            })
            .sum()
    }

    /// How far each edge-flush connector's courtyard sits from its hinted edge
    /// of the fitted outline.
    fn flush_gaps(&self) -> Vec<i64> {
        let Some(hull) = self.hull(|_| true) else {
            return Vec::new();
        };
        (0..self.placed.len())
            .filter_map(|i| {
                let side = flush_edge_index(self.board, self.placed[i].id)?;
                let rect = self.rect(i);
                Some(match side {
                    0 => rect.min.x_nm - hull.min.x_nm,
                    1 => rect.min.y_nm - hull.min.y_nm,
                    2 => hull.max.x_nm - rect.max.x_nm,
                    _ => hull.max.y_nm - rect.max.y_nm,
                })
            })
            .collect()
    }

    /// Whether the visual review reports something the starting arrangement
    /// did not: a blocked escape corridor is a regression like any other.
    fn needs_revision(&self) -> bool {
        self.finding_keys()
            .iter()
            .any(|key| !self.known.contains(key))
    }

    /// The review findings of this arrangement, each cut to the part before the
    /// blocker list (which changes as parts move). A connector the placer
    /// cannot seat flush is always reported as "not on an edge", so that
    /// finding is left out.
    fn finding_keys(&self) -> Vec<String> {
        self.findings()
            .into_iter()
            .filter(|finding| !finding.contains("from its nearest board edge"))
            .map(|finding| {
                finding
                    .split(" has ")
                    .next()
                    .unwrap_or(&finding)
                    .to_string()
            })
            .collect()
    }

    /// Every finding of the visual review of this arrangement.
    fn findings(&self) -> Vec<String> {
        let Some(outline) = self.hull(|_| true) else {
            return Vec::new();
        };
        let placement = Placement {
            board_outline: outline,
            components: self.placed.clone(),
        };
        describe_placement(self.board, &placement)
            .visual_review
            .findings
    }

    /// Run `pass` and undo it if the visual review shows a finding it lacked.
    fn guarded(&mut self, pass: fn(&mut Self)) {
        let before = (self.placed.clone(), self.findings());
        pass(self);
        if self.findings().iter().any(|f| !before.1.contains(f)) {
            self.placed = before.0;
        }
    }

    /// Run `rule` and keep it only if it adds no overlap and leaves no
    /// edge-flush connector short of its edge (which parts pushed past it would).
    fn try_rule(&mut self, rule: fn(&mut Self)) {
        let before = (self.placed.clone(), self.anchored.clone());
        let (overlaps, flush) = (self.overlaps(), self.flush_gaps());
        rule(self);
        let slack = mm_to_nm(FLUSH_SLACK_MM);
        let flush_lost = self
            .flush_gaps()
            .iter()
            .zip(&flush)
            .any(|(after, before)| after > &(before + slack));
        if self.overlaps() > overlaps || flush_lost {
            (self.placed, self.anchored) = before;
        }
    }

    /// The outline fitted round every part for which `keep` holds.
    fn hull(&self, keep: impl Fn(usize) -> bool) -> Option<Rect> {
        self.hull_at(keep, fitted_edge_margin_nm(self.board))
    }

    fn hull_at(&self, keep: impl Fn(usize) -> bool, rim: i64) -> Option<Rect> {
        let mut subset = Placement {
            board_outline: self.outline,
            components: (0..self.placed.len())
                .filter(|&i| keep(i))
                .map(|i| self.placed[i])
                .collect(),
        };
        if subset.components.is_empty() {
            return None;
        }
        fit_outline_docked(self.board, &mut subset, rim);
        Some(subset.board_outline)
    }

    /// Least gap between any two groups' bounding boxes.
    fn min_group_gap(&self) -> i64 {
        let boxes: Vec<Rect> = self
            .group_members()
            .values()
            .map(|m| self.bounds(m))
            .collect();
        let mut least = i64::MAX;
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                least = least.min(rect_gap(*a, *b));
            }
        }
        least
    }

    /// Group boxes as the placement review draws them: a connector in a group
    /// with a major IC docks on its own edge and is left out of the block, and
    /// a mounting hole on a board corner leaves too. [`review_group_boxes`] is
    /// the one rule behind this, the judge and the spacing tests.
    fn review_group_boxes(&self) -> BTreeMap<String, Rect> {
        review_group_boxes(self.board, &self.placed, self.outline)
    }

    /// Least gap between the group boxes the review draws.
    fn review_group_gap(&self) -> i64 {
        let boxes: Vec<Rect> = self.review_group_boxes().into_values().collect();
        let mut least = i64::MAX;
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                least = least.min(rect_gap(*a, *b));
            }
        }
        least
    }

    /// The review group box's area over its parts' courtyard area, as the
    /// placement tests measure compactness.
    fn group_ratio(&self, group: &str) -> f64 {
        let boxes = self.review_group_boxes();
        let Some(b) = boxes.get(group) else {
            return 0.0;
        };
        let area: f64 = (0..self.placed.len())
            .filter(|&i| self.component(i).group.as_deref() == Some(group))
            .map(|i| {
                let r = self.rect(i);
                (r.width_nm() * r.height_nm()) as f64
            })
            .sum();
        if area <= 0.0 {
            0.0
        } else {
            (b.width_nm() * b.height_nm()) as f64 / area
        }
    }

    /// Whether every group box is as tight as the placement review asks.
    fn groups_compact(&self) -> bool {
        self.review_group_boxes().into_iter().all(|(name, b)| {
            let parts: f64 = (0..self.placed.len())
                .filter(|&i| self.component(i).group.as_ref() == Some(&name))
                .map(|i| {
                    let r = self.rect(i);
                    (r.width_nm() * r.height_nm()) as f64
                })
                .sum();
            let box_area = (b.width_nm() * b.height_nm()) as f64;
            let limit = if name == "MCU" { 2.7 } else { 2.5 };
            parts == 0.0 || box_area / parts <= limit
        })
    }

    /// Centre grid of a part, in nm.
    fn grid_nm(&self, index: usize) -> i64 {
        let header = self
            .component(index)
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("header"));
        mm_to_nm(if header { HEADER_GRID_MM } else { GRID_MM })
    }

    /// Move `index` to `center` if that adds no overlap, keeps every pair and
    /// group at its gap (or at least no closer than it was), and leaves the
    /// fitted outline as it was, which keeps the holes, docked connectors and
    /// USB centring where they are. Returns whether the part moved.
    fn try_center(&mut self, index: usize, center: Point, hull: Rect) -> bool {
        let (old_center, old_rect) = (self.placed[index].center, self.rect(index));
        let groups_before = self.min_group_gap();
        self.placed[index].center = center;
        let rect = self.rect(index);
        let spacing_ok = (0..self.placed.len()).filter(|&j| j != index).all(|j| {
            let other = self.rect(j);
            !rect.intersects(&other)
                && rect_gap(rect, other) >= rect_gap(old_rect, other).min(self.gap_nm)
        });
        if spacing_ok
            && self.min_group_gap() >= groups_before.min(self.group_gap_nm)
            && self.hull_at(|_| true, self.rim) == Some(hull)
        {
            return true;
        }
        self.placed[index].center = old_center;
        false
    }

    /// Last rule: snap free parts to the grid and line up near-rows and
    /// near-columns inside each group. A move that would break a rule is skipped,
    /// and the whole pass is undone if it newly trips the visual review.
    fn align(&mut self) {
        // The grid is measured from the outline the rule's caller will fit.
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let before = (self.placed.clone(), self.findings());
        // ponytail: an ungrouped board the review already objects to is left
        // alone, because the refine loop that repairs it is sensitive to the
        // snapped input (secure_tracker); upgrade by aligning after the refine loop.
        if !before.1.is_empty() && self.group_members().is_empty() {
            return;
        }
        // An arrangement the review already objects to is left for the router
        // and the refine loop; nudging it would only move the finding around.
        let origin = hull.min;
        // The rules anchored their parts; only a hinted part, hole or dock stays put now.
        let docks = self.docks();
        // A board no other rule touches is returned as the solver placed it.
        let has_holes = self.placed.iter().any(|p| {
            self.board
                .component(p.id)
                .is_some_and(|c| c.kind == "mounting_hole")
        });
        if docks.is_empty() && !has_holes && self.group_members().is_empty() {
            return;
        }
        let free = self.free_parts();
        // A snap frees the room a neighbour needs, so sweep until none moves.
        for _ in 0..SNAP_SWEEPS {
            for &i in &free {
                let grid = self.grid_nm(i);
                let c = self.placed[i].center;
                let (fx, fy) = (
                    (c.x_nm - origin.x_nm).div_euclid(grid),
                    (c.y_nm - origin.y_nm).div_euclid(grid),
                );
                // Packed parts sit at their gap already, so a snap may need to go a
                // cell further than the nearest grid point to move away from a neighbour.
                let mut spots: Vec<Point> = (-1..=2)
                    .flat_map(|dx| (-1..=2).map(move |dy| (fx + dx, fy + dy)))
                    .map(|(gx, gy)| Point::new(origin.x_nm + gx * grid, origin.y_nm + gy * grid))
                    .collect();
                spots.sort_by_key(|p| (p.x_nm - c.x_nm).abs() + (p.y_nm - c.y_nm).abs());
                for spot in spots {
                    if self.try_center(i, spot, hull) {
                        break;
                    }
                }
            }
        }
        let tol = mm_to_nm(ALIGN_TOL_MM);
        for members in self.group_members().values() {
            let free_members: Vec<usize> = members
                .iter()
                .copied()
                .filter(|i| free.contains(i))
                .collect();
            for horizontal in [true, false] {
                let axis = |s: &Self, i: usize| {
                    let c = s.placed[i].center;
                    if horizontal { c.y_nm } else { c.x_nm }
                };
                let mut order = free_members.clone();
                order.sort_by_key(|&i| axis(self, i));
                let mut start = 0;
                while start < order.len() {
                    let mut end = start + 1;
                    while end < order.len()
                        && axis(self, order[end]) - axis(self, order[end - 1]) <= tol
                    {
                        end += 1;
                    }
                    let row = &order[start..end];
                    let target = axis(self, row[row.len() / 2]);
                    for &i in row {
                        let c = self.placed[i].center;
                        let on_grid = (target - if horizontal { origin.y_nm } else { origin.x_nm })
                            % self.grid_nm(i)
                            == 0;
                        if on_grid && axis(self, i) != target {
                            let to = if horizontal {
                                Point::new(c.x_nm, target)
                            } else {
                                Point::new(target, c.y_nm)
                            };
                            self.try_center(i, to, hull);
                        }
                    }
                    start = end;
                }
            }
        }
        // Any new finding, blocked escape corridors included, undoes the pass.
        if self.findings().iter().any(|f| !before.1.contains(f)) {
            self.placed = before.0;
        }
    }

    /// Parts no rule or hint holds: not hinted, docked or a mounting hole.
    fn free_parts(&self) -> Vec<usize> {
        let docks = self.docks();
        (0..self.placed.len())
            .filter(|&i| {
                !self.pinned[i] && !docks.contains(&i) && self.component(i).kind != "mounting_hole"
            })
            .collect()
    }

    /// Where pin `pin` of part `index` sits on the board.
    fn pad_at(&self, pads: &crate::PadOffsetLookup, index: usize, pin: usize) -> Point {
        let placed = self.placed[index];
        crate::apply_rotation(placed, pads.lookup(placed.id, pin).unwrap_or((0, 0)))
    }

    /// `(part, its pin, the part it serves, that part's pin)` for every
    /// decoupling or crystal-load capacitor, and for each crystal and the IC
    /// its oscillator pins go to (crystal first, so its load caps follow it).
    fn supports(&self) -> Vec<(usize, usize, usize, usize)> {
        let index = |id| self.placed.iter().position(|p| p.id == id);
        let clusters = synth_ir::recognize_clusters(self.board);
        let mut crystals = Vec::new();
        let mut caps = Vec::new();
        for cluster in &clusters {
            let Some(anchor) = index(cluster.anchor) else {
                continue;
            };
            if cluster.kind == synth_ir::ClusterKind::Crystal {
                // The small nets are the oscillator ones; a ground net also reaches the IC.
                let hit = self
                    .board
                    .nets
                    .iter()
                    .filter(|n| n.endpoints.len() <= 3)
                    .find_map(|net| {
                        let own = net
                            .endpoints
                            .iter()
                            .find(|e| e.component == cluster.anchor)?;
                        let ic = net.endpoints.iter().find_map(|e| {
                            index(e.component)
                                .filter(|&i| self.is_major_ic(i))
                                .map(|i| (i, e))
                        })?;
                        Some((anchor, own.pin.0 as usize, ic.0, ic.1.pin.0 as usize))
                    });
                crystals.extend(hit);
            }
            for member in &cluster.members {
                use synth_ir::MemberRole::{DecouplingCap, LoadCap, OrphanRailCap, RailCap};
                if let (true, Some(m)) = (
                    matches!(
                        member.role,
                        DecouplingCap | RailCap | OrphanRailCap | LoadCap
                    ),
                    index(member.component),
                ) {
                    caps.push((
                        m,
                        member.binding.member_pin.0 as usize,
                        anchor,
                        member.binding.anchor_pin.0 as usize,
                    ));
                }
            }
        }
        crystals.extend(caps);
        crystals
    }

    /// Move each free decoupling cap and crystal to the free spot nearest the
    /// pin it serves, a part gap from its neighbours, if that is nearer than it is.
    /// The outline stays as it was, so holes, docks and USB centring hold.
    fn hug(&mut self) {
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let pads = crate::build_pad_offset_lookup(self.board);
        let free = self.free_parts();
        let (grid, window) = (mm_to_nm(GRID_MM), mm_to_nm(HUG_WINDOW_MM));
        for (m, pin, anchor, anchor_pin) in self.supports() {
            if !free.contains(&m) {
                continue;
            }
            let target = self.pad_at(&pads, anchor, anchor_pin);
            let offset = pads.lookup(self.placed[m].id, pin).unwrap_or((0, 0));
            let dist = |s: &Self, centre: Point| {
                let at = ComponentPlacement {
                    center: centre,
                    ..s.placed[m]
                };
                let p = crate::apply_rotation(at, offset);
                ((p.x_nm - target.x_nm) as f64).hypot((p.y_nm - target.y_nm) as f64) as i64
            };
            let (now, rect) = (dist(self, self.placed[m].center), self.rect(m));
            let others: Vec<Rect> = (0..self.placed.len())
                .filter(|&j| j != m)
                .map(|j| self.rect(j))
                .collect();
            let from = self.placed[m].center;
            let mut spots: Vec<(i64, Point)> = Vec::new();
            let lattice = |origin: i64, v: i64| origin + (v - origin).div_euclid(grid) * grid;
            let (x0, y0) = (
                lattice(hull.min.x_nm, target.x_nm - window),
                lattice(hull.min.y_nm, target.y_nm - window),
            );
            for gx in 0..=2 * window / grid {
                for gy in 0..=2 * window / grid {
                    let spot = Point::new(x0 + gx * grid, y0 + gy * grid);
                    let d = dist(self, spot);
                    let (dx, dy) = (spot.x_nm - from.x_nm, spot.y_nm - from.y_nm);
                    let moved = Rect::new(
                        Point::new(rect.min.x_nm + dx, rect.min.y_nm + dy),
                        Point::new(rect.max.x_nm + dx, rect.max.y_nm + dy),
                    );
                    if d + grid / 2 < now
                        && others.iter().all(|o| rect_gap(moved, *o) >= self.gap_nm)
                    {
                        spots.push((d, spot));
                    }
                }
            }
            spots.sort_by_key(|&(d, p)| (d, p.x_nm, p.y_nm));
            for (_, spot) in spots {
                if self.try_center(m, spot, hull) {
                    break;
                }
            }
        }
    }

    fn movable(&self, members: &[usize]) -> Vec<usize> {
        members
            .iter()
            .copied()
            .filter(|&i| !self.anchored[i])
            .collect()
    }

    fn shift(&mut self, index: usize, dx: i64, dy: i64) {
        let center = self.placed[index].center;
        self.placed[index].center = Point::new(center.x_nm + dx, center.y_nm + dy);
    }

    /// Move `index` so its courtyard's `(left, top)` corner is at `(x, y)`.
    fn put_corner(&mut self, index: usize, x: i64, y: i64) {
        let rect = self.rect(index);
        self.shift(index, x - rect.min.x_nm, y - rect.min.y_nm);
    }

    /// The connector's passive frame: every group passive within
    /// [`ATTACHED_MM`] of it. `None` when a passive sits further out, because
    /// then the group is a spread block, not a frame round the connector.
    fn attached_frame(&self, index: usize) -> Option<Vec<usize>> {
        if self.component(index).group.is_none() {
            return Some(Vec::new());
        }
        let frame = self.attached_passives(index);
        (frame.len() == self.frame_candidates(index).len()).then_some(frame)
    }

    /// Move `index` and its attached parts by the same translation, so their
    /// offsets from it are kept exactly.
    fn shift_with_attached(&mut self, index: usize, dx: i64, dy: i64, attached: &[usize]) {
        self.shift(index, dx, dy);
        for &i in attached {
            self.shift(i, dx, dy);
        }
    }

    /// Slide an attached part the move put in the way the least distance along
    /// `edge` that clears it, keeping at least `need` between group boxes. False
    /// when no step clears one; the caller undoes the whole move then. Parts
    /// already clear are left where they are.
    fn nudge_attached(
        &mut self,
        attached: &[usize],
        edge: BoardEdge,
        hull: Rect,
        need: i64,
    ) -> bool {
        let step = mm_to_nm(GRID_MM);
        let along_x = matches!(edge, BoardEdge::Top | BoardEdge::Bottom);
        let reach = if along_x {
            hull.width_nm()
        } else {
            hull.height_nm()
        };
        let clear = |state: &Self, i: usize| {
            let r = state.rect(i);
            hull.contains(r.min)
                && hull.contains(r.max)
                && (0..state.placed.len()).all(|j| j == i || !r.intersects(&state.rect(j)))
        };
        // A nudge can put a later part on an earlier one, so sweep again.
        for _ in 0..3 {
            let mut blocked = false;
            for &i in attached {
                if clear(self, i) {
                    continue;
                }
                blocked = true;
                let base = self.placed[i].center;
                let mut found = false;
                let mut k = 1;
                while i64::from(k) * step <= reach {
                    for sign in [1_i64, -1] {
                        let (dx, dy) = if along_x {
                            (sign * i64::from(k) * step, 0)
                        } else {
                            (0, sign * i64::from(k) * step)
                        };
                        self.placed[i].center = Point::new(base.x_nm + dx, base.y_nm + dy);
                        if clear(self, i) {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                    k += 1;
                }
                if !found {
                    self.placed[i].center = base;
                    return false;
                }
            }
            if !blocked {
                break;
            }
        }
        attached.iter().all(|&i| clear(self, i)) && self.review_group_gap() >= need
    }

    /// One pass pushing overlapping courtyards apart along the shallower axis.
    /// Returns whether anything moved.
    fn relax_pass(&mut self) -> bool {
        let gap = self.gap_nm;
        let infl = self.infl;
        let grow = |r: Rect| {
            Rect::new(
                Point::new(r.min.x_nm - infl, r.min.y_nm - infl),
                Point::new(r.max.x_nm + infl, r.max.y_nm + infl),
            )
        };
        let mut moved = false;
        for i in 0..self.placed.len() {
            for j in i + 1..self.placed.len() {
                let (a, b) = (grow(self.rect(i)), grow(self.rect(j)));
                if !a.intersects(&b) {
                    continue;
                }
                let (move_i, move_j) = (!self.anchored[i], !self.anchored[j]);
                if !move_i && !move_j {
                    continue;
                }
                let over_x = a.max.x_nm.min(b.max.x_nm) - a.min.x_nm.max(b.min.x_nm);
                let over_y = a.max.y_nm.min(b.max.y_nm) - a.min.y_nm.max(b.min.y_nm);
                let along_x = over_x <= over_y;
                let (centre_a, centre_b) = if along_x {
                    (a.min.x_nm + a.max.x_nm, b.min.x_nm + b.max.x_nm)
                } else {
                    (a.min.y_nm + a.max.y_nm, b.min.y_nm + b.max.y_nm)
                };
                // Ties go to the lower index, so the result is deterministic.
                let mut sign = if centre_a <= centre_b { 1 } else { -1 };
                // A part pushed off a fixed one goes inward: pushing it out
                // would carry it past the edge the fixed part was put on.
                let board_centre = if along_x {
                    self.outline.min.x_nm + self.outline.max.x_nm
                } else {
                    self.outline.min.y_nm + self.outline.max.y_nm
                };
                if move_i != move_j {
                    let fixed = if move_i { centre_b } else { centre_a };
                    let inward = if board_centre >= fixed { 1 } else { -1 };
                    // `sign` moves i by -sign and j by +sign.
                    sign = if move_j { inward } else { -inward };
                }
                let need =
                    if along_x { over_x } else { over_y } + if self.infl > 0 { 1 } else { gap };
                let (share_i, share_j) = match (move_i, move_j) {
                    (true, true) => (need / 2, need - need / 2),
                    (true, false) => (need, 0),
                    _ => (0, need),
                };
                for (index, delta) in [(i, -sign * share_i), (j, sign * share_j)] {
                    if along_x {
                        self.shift(index, delta, 0);
                    } else {
                        self.shift(index, 0, delta);
                    }
                }
                moved = true;
            }
        }
        moved
    }

    /// Push whole groups apart until every pair of boxes keeps `group_gap_nm`,
    /// letting the part push-apart settle after each round. The part rule can
    /// otherwise shove one group's parts across another's box.
    fn space_groups(&mut self) {
        let groups: Vec<Vec<usize>> = self.group_members().into_values().collect();
        // The rules fixed their parts; with them done, only hinted ones stay put.
        self.anchored = self.pinned.clone();
        for _ in 0..SPACE_GROUP_ROUNDS {
            let mut moved = false;
            for (n, a) in groups.iter().enumerate() {
                for b in &groups[n + 1..] {
                    let (ra, rb) = (self.bounds(a), self.bounds(b));
                    let dx = (rb.min.x_nm - ra.max.x_nm).max(ra.min.x_nm - rb.max.x_nm);
                    let dy = (rb.min.y_nm - ra.max.y_nm).max(ra.min.y_nm - rb.max.y_nm);
                    if dx.max(dy) >= self.group_gap_nm {
                        continue;
                    }
                    let along_x = dx >= dy;
                    let need = self.group_gap_nm - dx.max(dy);
                    // `dir` is the way b lies from a; b steps along it, else a steps back.
                    let dir = if (along_x && rb.min.x_nm + rb.max.x_nm >= ra.min.x_nm + ra.max.x_nm)
                        || (!along_x && rb.min.y_nm + rb.max.y_nm >= ra.min.y_nm + ra.max.y_nm)
                    {
                        1
                    } else {
                        -1
                    };
                    let (mover, sign) = if self.movable(b).is_empty() {
                        (a, -dir)
                    } else {
                        (b, dir)
                    };
                    for i in self.movable(mover) {
                        if along_x {
                            self.shift(i, sign * need, 0);
                        } else {
                            self.shift(i, 0, sign * need);
                        }
                    }
                    moved = true;
                }
            }
            if !moved {
                break;
            }
            self.relax();
        }
    }

    fn relax(&mut self) {
        for _ in 0..RELAX_PASSES {
            if !self.relax_pass() {
                break;
            }
        }
    }

    /// Whether every pair of parts and of groups keeps its gap (to 0.05 mm).
    fn keeps_spacing(&self) -> bool {
        let slack = mm_to_nm(0.05);
        let gap = |a: Rect, b: Rect| {
            (b.min.x_nm - a.max.x_nm)
                .max(a.min.x_nm - b.max.x_nm)
                .max(b.min.y_nm - a.max.y_nm)
                .max(a.min.y_nm - b.max.y_nm)
        };
        let rects: Vec<Rect> = (0..self.placed.len()).map(|i| self.rect(i)).collect();
        let parts = rects.iter().enumerate().all(|(i, a)| {
            rects[i + 1..]
                .iter()
                .all(|b| gap(*a, *b) >= self.gap_nm - slack)
        });
        let boxes: Vec<Rect> = self
            .group_members()
            .values()
            .map(|m| self.bounds(m))
            .collect();
        parts
            && boxes.iter().enumerate().all(|(i, a)| {
                boxes[i + 1..]
                    .iter()
                    .all(|b| gap(*a, *b) >= self.group_gap_nm - slack)
            })
    }

    fn is_major_ic(&self, index: usize) -> bool {
        crate::fold::is_major_component(self.component(index))
    }

    /// Whether the part is a chip the spread pass must leave alone.
    fn is_chip(&self, index: usize) -> bool {
        self.is_major_ic(index)
            || matches!(
                self.component(index).kind.as_str(),
                "mcu" | "sensor" | "regulator" | "ic" | "memory"
            )
    }

    /// Parts the spread pass may move: no holes, connectors, chips, capacitors
    /// (the round-8 decoupling caps) or crystals.
    fn spreadable(&self) -> Vec<usize> {
        (0..self.placed.len())
            .filter(|&i| {
                let component = self.component(i);
                !self.spread_keep.contains(&i)
                    && component.kind != "mounting_hole"
                    && !matches!(component.kind.as_str(), "connector" | "jack")
                    && component.kind != "capacitor"
                    && component.kind != "crystal"
                    && !self.is_chip(i)
            })
            .collect()
    }

    /// The centre a part should not drift from: its group's chip, else the
    /// parts it is wired to.
    fn anchor_point(&self, index: usize) -> Option<Point> {
        if let Some(group) = self.component(index).group.clone() {
            let chips: Vec<Point> = (0..self.placed.len())
                .filter(|&j| self.component(j).group.as_ref() == Some(&group) && self.is_chip(j))
                .map(|j| self.placed[j].center)
                .collect();
            if !chips.is_empty() {
                return Some(mean_point(&chips));
            }
        }
        let mut pulls = Vec::new();
        for net in &self.board.nets {
            if !net
                .endpoints
                .iter()
                .any(|e| e.component == self.placed[index].id)
            {
                continue;
            }
            for endpoint in &net.endpoints {
                if endpoint.component == self.placed[index].id {
                    continue;
                }
                if let Some(j) = self.placed.iter().position(|p| p.id == endpoint.component) {
                    pulls.push(self.placed[j].center);
                }
            }
        }
        (!pulls.is_empty()).then(|| mean_point(&pulls))
    }

    /// Fill the empty sides: each round, walk every movable part one grid step
    /// toward the largest empty rectangle, while it stays within the round-8
    /// distance of its anchor and keeps the part and group gaps. Stops when the
    /// largest empty rectangle stops shrinking.
    fn spread(&mut self) {
        if self.group_members().is_empty() {
            return;
        }
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let movable = self.spreadable();
        if movable.is_empty() {
            return;
        }
        let inflate = mm_to_nm(EMPTY_INFLATE_MM);
        let boxes = |s: &Self| -> Vec<Rect> { (0..s.placed.len()).map(|i| s.rect(i)).collect() };
        let (mut largest, _) = largest_empty_rect(hull, &boxes(self), inflate);
        if largest == 0 {
            return;
        }
        let step = mm_to_nm(SPREAD_STEP_MM);
        let slack = mm_to_nm(SPREAD_SLACK_MM);
        let anchors: Vec<Option<(Point, i64)>> = movable
            .iter()
            .map(|&i| {
                self.anchor_point(i)
                    .map(|a| (a, point_distance(self.placed[i].center, a)))
            })
            .collect();
        for _ in 0..SPREAD_ROUNDS {
            let (_, rect) = largest_empty_rect(hull, &boxes(self), inflate);
            let target = Point::new(
                rect.min.x_nm / 2 + rect.max.x_nm / 2,
                rect.min.y_nm / 2 + rect.max.y_nm / 2,
            );
            let round = self.placed.clone();
            let keys = self.finding_keys();
            let groups = self.review_group_gap();
            let mut moved = false;
            for (k, &i) in movable.iter().enumerate() {
                let Some((anchor, start)) = anchors[k] else {
                    continue;
                };
                moved |= self.step_toward(i, target, hull, anchor, start, slack, step, groups);
            }
            if self.finding_keys().iter().any(|key| !keys.contains(key)) {
                self.placed = round;
                break;
            }
            let (next, _) = largest_empty_rect(hull, &boxes(self), inflate);
            if next >= largest || !moved {
                break;
            }
            largest = next;
        }
    }

    /// One grid step for `index` toward `target`, if the part keeps its gap and
    /// stays within `slack` of its anchor. Returns whether it moved.
    #[allow(clippy::too_many_arguments)]
    fn step_toward(
        &mut self,
        index: usize,
        target: Point,
        hull: Rect,
        anchor: Point,
        start: i64,
        slack: i64,
        step: i64,
        groups: i64,
    ) -> bool {
        let c = self.placed[index].center;
        let toward = |from: i64, to: i64| {
            if (to - from).abs() >= step {
                (to - from).signum() * step
            } else {
                0
            }
        };
        let (dx, dy) = (toward(c.x_nm, target.x_nm), toward(c.y_nm, target.y_nm));
        if (dx, dy) == (0, 0) {
            return false;
        }
        for (mx, my) in [(dx, dy), (dx, 0), (0, dy)] {
            if (mx, my) == (0, 0) {
                continue;
            }
            let to = Point::new(c.x_nm + mx, c.y_nm + my);
            if point_distance(to, anchor) > start + slack {
                continue;
            }
            if !self.try_center(index, to, hull) {
                continue;
            }
            if self.review_group_gap() >= groups.min(self.group_gap_nm) {
                return true;
            }
            self.placed[index].center = c;
        }
        false
    }

    /// A docked connector in a group that has a major IC: it sits on the edge on
    /// its own while the chip and its support parts form the group's block. One
    /// that stayed undocked is part of the block.
    fn lone_connector(&self, index: usize) -> bool {
        let component = self.component(index);
        component.kind == "connector"
            && self.anchored.get(index).copied().unwrap_or(false)
            && self.docks().contains(&index)
            && component.group.as_ref().is_some_and(|name| {
                (0..self.placed.len())
                    .any(|i| self.component(i).group.as_ref() == Some(name) && self.is_major_ic(i))
            })
    }

    /// Wired connectors no hint pins, in docking order: USB first, then the
    /// ones with the most wired nets, the rest by refdes.
    fn docks(&self) -> Vec<usize> {
        let wired = crate::wired_free_connectors(self.board);
        let mut list: Vec<usize> = (0..self.placed.len())
            .filter(|&i| {
                !self.pinned[i]
                    && wired.contains(&self.placed[i].id)
                    && (!self.usb_only || is_usb_connector(self.component(i)))
            })
            .collect();
        list.sort_by_key(|&i| {
            (
                !is_usb_connector(self.component(i)),
                std::cmp::Reverse(self.wired_nets(i)),
                self.component(i).refdes.clone(),
            )
        });
        list
    }

    fn wired_nets(&self, index: usize) -> usize {
        let id = self.placed[index].id;
        self.board
            .nets
            .iter()
            .filter(|net| net.endpoints.iter().any(|e| e.component == id))
            .count()
    }

    /// The passives that ride with a connector: its group's, within
    /// [`ATTACHED_MM`] of it.
    fn attached_passives(&self, index: usize) -> Vec<usize> {
        let reach = mm_to_nm(ATTACHED_MM);
        let seat = self.rect(index);
        self.frame_candidates(index)
            .into_iter()
            .filter(|&i| rect_gap(seat, self.rect(i)) <= reach)
            .collect()
    }

    /// Every group member of a connector that may ride with it: no chips,
    /// connectors or holes, and nothing a hint fixed.
    fn frame_candidates(&self, index: usize) -> Vec<usize> {
        let Some(name) = self.component(index).group.clone() else {
            return Vec::new();
        };
        (0..self.placed.len())
            .filter(|&i| {
                i != index
                    && !self.pinned[i]
                    && self.component(i).group.as_ref() == Some(&name)
                    && !self.is_chip(i)
                    && !matches!(
                        self.component(i).kind.as_str(),
                        "connector" | "jack" | "mounting_hole"
                    )
            })
            .collect()
    }

    /// Reserve a docked connector's envelope: its courtyard plus the passives
    /// riding with it. Later connectors keep the part gap off it.
    fn reserve_envelope(&mut self, index: usize) {
        let mut envelope = self.rect(index);
        for i in self.attached_passives(index) {
            let r = self.rect(i);
            envelope = Rect::new(
                Point::new(
                    envelope.min.x_nm.min(r.min.x_nm),
                    envelope.min.y_nm.min(r.min.y_nm),
                ),
                Point::new(
                    envelope.max.x_nm.max(r.max.x_nm),
                    envelope.max.y_nm.max(r.max.y_nm),
                ),
            );
        }
        self.reserved.push(envelope);
    }

    fn in_reserved(&self, rect: Rect) -> bool {
        self.reserved
            .iter()
            .any(|envelope| rect_gap(rect, *envelope) < self.gap_nm)
    }

    /// Dock every wired connector flush on the board edge nearest it, centred
    /// along that edge. Connectors that want the same edge take the next
    /// nearest free one, so they spread out.
    fn usb_to_edge(&mut self) {
        let dock = self.docks();
        let Some(body) = self.hull(|i| !dock.contains(&i)) else {
            return;
        };
        let mut wants: Vec<(i64, usize, Vec<(i64, BoardEdge)>)> = dock
            .iter()
            .map(|&i| {
                let c = self.placed[i].center;
                let mut sides = vec![
                    (c.y_nm - body.min.y_nm, BoardEdge::Top),
                    (body.max.x_nm - c.x_nm, BoardEdge::Right),
                    (body.max.y_nm - c.y_nm, BoardEdge::Bottom),
                    (c.x_nm - body.min.x_nm, BoardEdge::Left),
                ];
                sides.sort_by_key(|s| s.0);
                (sides[0].0, i, sides)
            })
            .collect();
        wants.sort_by_key(|w| (w.0, w.1));
        let mut taken: Vec<BoardEdge> = Vec::new();
        for (_, index, sides) in wants {
            // Nearest free edge first, then the next free one; a connector no
            // edge takes without a new review finding (a blocked escape
            // corridor) stays undocked where it was.
            let mut candidates: Vec<BoardEdge> = sides
                .iter()
                .map(|s| s.1)
                .filter(|edge| !taken.contains(edge))
                .collect();
            if candidates.is_empty() {
                candidates.push(sides[0].1);
            }
            let before = (self.placed.clone(), self.anchored.clone());
            let seen = self.finding_keys();
            let docked = candidates.into_iter().find(|&edge| {
                self.dock(index, edge, &dock);
                // Overlaps are try_rule's to count; anything else new is a regression.
                let regression = self
                    .finding_keys()
                    .iter()
                    .any(|key| !seen.contains(key) && !key.starts_with("courtyard overlap"));
                if regression {
                    (self.placed, self.anchored) = before.clone();
                    return false;
                }
                taken.push(edge);
                true
            });
            if docked.is_none() {
                (self.placed, self.anchored) = before;
            }
        }
    }

    /// Seat connector `index` flush on `edge`, centred, then clear what it hit.
    fn dock(&mut self, index: usize, edge: BoardEdge, dock: &[usize]) {
        let Some(hull) = self.hull(|other| !dock.contains(&other)) else {
            return;
        };
        self.dock_on(index, edge, hull);
        self.relax();
    }

    /// [`Self::dock`] on a given outline, without settling afterwards.
    fn dock_on(&mut self, index: usize, edge: BoardEdge, hull: Rect) {
        self.set_mating_face(index, edge);
        let rect = self.rect(index);
        let mid_x = hull.min.x_nm / 2 + hull.max.x_nm / 2;
        let mid_y = hull.min.y_nm / 2 + hull.max.y_nm / 2;
        let (x, y) = match edge {
            BoardEdge::Top => (mid_x - rect.width_nm() / 2, hull.min.y_nm),
            BoardEdge::Bottom => (
                mid_x - rect.width_nm() / 2,
                hull.max.y_nm - rect.height_nm(),
            ),
            BoardEdge::Left => (hull.min.x_nm, mid_y - rect.height_nm() / 2),
            BoardEdge::Right => (
                hull.max.x_nm - rect.width_nm(),
                mid_y - rect.height_nm() / 2,
            ),
        };
        self.put_corner(index, x, y);
        self.anchored[index] = true;
    }

    /// Point the connector's mating face out of `edge`, if its part has one.
    /// A hinted part keeps the turn the hint (or the solver reading it) gave
    /// it: an explicit rotation request outranks the generic facing rule.
    fn set_mating_face(&mut self, index: usize, edge: BoardEdge) {
        if self.pinned[index] {
            return;
        }
        let face = self
            .component(index)
            .part
            .as_ref()
            .and_then(|part| part.footprint_dimensions.as_ref())
            .and_then(|dimensions| dimensions.mating_face);
        if let Some(face) = face {
            self.placed[index].rotation = floorplan::rotation_for_mating_edge(face, edge);
        }
    }

    /// Dock every wired connector the packing left in the interior flush on the
    /// nearest edge with a clear slot, trying the next edge when that one is
    /// full. A passive left in a new escape corridor is stepped aside once.
    fn dock_loose_connectors(&mut self) {
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let slack = mm_to_nm(FLUSH_SLACK_MM);
        let start = (self.placed.clone(), self.anchored.clone());
        let seen0 = self.finding_keys();
        let (overlaps0, groups0) = (self.overlaps(), self.review_group_gap());
        let mut history: Vec<(Vec<ComponentPlacement>, Vec<bool>)> = Vec::new();
        // Connectors the packing already seated on an edge reserve their frame
        // before any later one picks a slot.
        for index in self.docks() {
            if self
                .edge_gaps(index, hull)
                .first()
                .is_some_and(|g| g.0 <= slack)
            {
                self.reserve_envelope(index);
            }
        }
        for index in self.docks() {
            let gaps = self.edge_gaps(index, hull);
            if gaps.iter().map(|g| g.0).min().unwrap_or(0) <= slack {
                continue;
            }
            let before = (self.placed.clone(), self.anchored.clone());
            let seen = self.finding_keys();
            let overlaps = self.overlaps();
            // A connector whose chip-less group is compact must not be docked
            // away from its support parts: re-seat them round it, or give up.
            let group = self.component(index).group.clone();
            let members: Vec<usize> = group
                .as_ref()
                .and_then(|name| self.group_members().get(name).cloned())
                .unwrap_or_default();
            let passive_support = members
                .iter()
                .filter(|&&i| i != index)
                .all(|&i| self.component(i).kind != "connector");
            let reseat = group.is_some()
                && passive_support
                && members.len() <= 6
                && !members.iter().any(|&i| self.is_major_ic(i));
            let ratio = |state: &Self| group.as_ref().map_or(0.0, |name| state.group_ratio(name));
            let was_loose = ratio(self) > 2.5;
            // The connector alone must find a clear seat; its group stays where
            // the packing put it, and relax lets any part it lands on yield.
            let mut done = false;
            for (_, edge) in gaps {
                (self.placed, self.anchored) = before.clone();
                let Some(spot) = self.edge_slot(index, edge, hull) else {
                    continue;
                };
                self.put_corner(index, spot.x_nm, spot.y_nm);
                self.anchored[index] = true;
                self.relax();
                if self.overlaps() > overlaps {
                    continue;
                }
                let fresh = |state: &Self| {
                    state
                        .finding_keys()
                        .iter()
                        .any(|key| !seen.contains(key) && !key.starts_with("courtyard overlap"))
                };
                if fresh(self) {
                    self.clear_corridors(&seen);
                }
                if fresh(self) {
                    continue;
                }
                if !was_loose && ratio(self) > 2.5 {
                    if !reseat {
                        continue;
                    }
                    self.seat_group_inside(index, edge);
                    self.relax();
                    if self.overlaps() > overlaps || fresh(self) || ratio(self) > 2.5 {
                        continue;
                    }
                }
                done = true;
                break;
            }
            if done {
                self.reserve_envelope(index);
                history.push(before);
            } else {
                (self.placed, self.anchored) = before;
            }
        }
        // Docking one connector can leave two group boxes too close; give the
        // most recent seats back until the gaps hold again.
        while self.review_group_gap() < groups0.min(self.group_gap_nm)
            || self.overlaps() > overlaps0
            || self
                .finding_keys()
                .iter()
                .any(|key| !seen0.contains(key) && !key.starts_with("courtyard overlap"))
        {
            let Some(before) = history.pop() else {
                (self.placed, self.anchored) = start;
                return;
            };
            (self.placed, self.anchored) = before;
        }
    }

    /// Gap from the connector's courtyard to each outline edge, nearest first.
    fn edge_gaps(&self, index: usize, hull: Rect) -> Vec<(i64, BoardEdge)> {
        let rect = self.rect(index);
        let mut gaps = vec![
            (rect.min.y_nm - hull.min.y_nm, BoardEdge::Top),
            (hull.max.x_nm - rect.max.x_nm, BoardEdge::Right),
            (hull.max.y_nm - rect.max.y_nm, BoardEdge::Bottom),
            (rect.min.x_nm - hull.min.x_nm, BoardEdge::Left),
        ];
        gaps.sort_by_key(|g| g.0);
        gaps
    }

    /// Top-left corner where connector `index` sits flush on `edge` without
    /// touching any part outside its group, nearest its present place along the
    /// edge. Its own group may yield to it.
    fn edge_slot(&mut self, index: usize, edge: BoardEdge, hull: Rect) -> Option<Point> {
        self.set_mating_face(index, edge);
        let rect = self.rect(index);
        let (w, h) = (rect.width_nm(), rect.height_nm());
        let along_x = matches!(edge, BoardEdge::Top | BoardEdge::Bottom);
        let (lo, hi) = if along_x {
            (hull.min.x_nm, hull.max.x_nm - w)
        } else {
            (hull.min.y_nm, hull.max.y_nm - h)
        };
        if hi < lo {
            return None;
        }
        let centre = self.placed[index].center;
        let desired = if along_x {
            centre.x_nm - w / 2
        } else {
            centre.y_nm - h / 2
        };
        let step = mm_to_nm(GRID_MM);
        let reach = (desired - lo).max(hi - desired).max(0) / step;
        let mut order = vec![0_i64];
        for k in 1..=reach {
            order.push(k);
            order.push(-k);
        }
        let group = self.component(index).group.clone();
        for k in order {
            let along = (desired + k * step).clamp(lo, hi);
            let (x, y) = match edge {
                BoardEdge::Top => (along, hull.min.y_nm),
                BoardEdge::Bottom => (along, hull.max.y_nm - h),
                BoardEdge::Left => (hull.min.x_nm, along),
                BoardEdge::Right => (hull.max.x_nm - w, along),
            };
            let cand = Rect::new(Point::new(x, y), Point::new(x + w, y + h));
            let clear = (0..self.placed.len())
                .filter(|&j| j != index)
                .filter(|&j| group.is_none() || self.component(j).group != group)
                .all(|j| rect_gap(cand, self.rect(j)) >= self.gap_nm)
                && !self.in_reserved(cand);
            if clear {
                return Some(Point::new(x, y));
            }
        }
        None
    }

    /// Step each passive that blocks a new connector-to-IC corridor one
    /// [`CORRIDOR_STEP_MM`] to the side the connector review picks, if the part
    /// keeps its gaps and the outline. Corridors already in `seen` are left alone.
    fn clear_corridors(&mut self, seen: &[String]) {
        let Some(hull) = self.hull(|_| true) else {
            return;
        };
        let fresh: Vec<String> = self
            .findings()
            .into_iter()
            .filter(|f| f.contains("escape corridor"))
            .filter(|f| {
                let key = f.split(" has ").next().unwrap_or(f);
                !seen.iter().any(|s| s.as_str() == key)
            })
            .collect();
        for finding in fresh {
            let Some(rest) = finding.strip_prefix("connector ") else {
                continue;
            };
            let Some((conn, tail)) = rest.split_once(" to ") else {
                continue;
            };
            let Some(ic) = tail.split(' ').next() else {
                continue;
            };
            let Some(conn) = (0..self.placed.len()).find(|&i| self.component(i).refdes == conn)
            else {
                continue;
            };
            let Some(ic) = (0..self.placed.len()).find(|&i| self.component(i).refdes == ic) else {
                continue;
            };
            let a = self.placed[conn].center;
            let b = self.placed[ic].center;
            let (dx, dy) = (
                synth_geometry::nm_to_mm(b.x_nm - a.x_nm),
                synth_geometry::nm_to_mm(b.y_nm - a.y_nm),
            );
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            let (px, py) = (-dy / len, dx / len);
            let step = mm_to_nm(CORRIDOR_STEP_MM);
            let blockers: Vec<&str> = finding
                .split('(')
                .nth(1)
                .and_then(|s| s.split(')').next())
                .map_or_else(Vec::new, |list| list.split(',').map(str::trim).collect());
            let mut targets = Vec::new();
            for blocker in blockers {
                let Some(i) = (0..self.placed.len()).find(|&i| self.component(i).refdes == blocker)
                else {
                    continue;
                };
                if self.is_chip(i)
                    || !matches!(
                        self.component(i).kind.as_str(),
                        "resistor" | "capacitor" | "inductor" | "diode" | "crystal"
                    )
                    || self.anchored[i]
                {
                    continue;
                }
                let c = self.placed[i].center;
                let dot = (c.x_nm - a.x_nm) as f64 * px + (c.y_nm - a.y_nm) as f64 * py;
                let side = if dot >= 0.0 { 1.0 } else { -1.0 };
                let to = Point::new(
                    c.x_nm + (side * px * step as f64) as i64,
                    c.y_nm + (side * py * step as f64) as i64,
                );
                let other = Point::new(
                    c.x_nm - (side * px * step as f64) as i64,
                    c.y_nm - (side * py * step as f64) as i64,
                );
                targets.push((i, to, other));
            }
            for (i, to, other) in targets {
                if !self.try_center(i, to, hull) {
                    self.try_center(i, other, hull);
                }
            }
        }
    }

    /// Fold a docked connector's support parts and set the block just inside
    /// the edge: below a top connector, above a bottom one, beside the side
    /// ones. The group stays a compact block instead of a strip along the edge.
    fn seat_group_inside(&mut self, index: usize, edge: BoardEdge) {
        let Some(name) = self.component(index).group.clone() else {
            return;
        };
        let others: Vec<usize> = self
            .group_members()
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .filter(|&i| i != index)
            .collect();
        if others.is_empty() || others.iter().any(|&i| self.anchored[i]) {
            return;
        }
        let gap = self.gap_nm + mm_to_nm(SNAP_ROOM_MM);
        self.fold(&others, gap);
        let block = self.bounds(&others);
        let seat = self.rect(index);
        let (x, y) = match edge {
            BoardEdge::Top => (seat.min.x_nm, seat.max.y_nm + gap),
            BoardEdge::Bottom => (seat.min.x_nm, seat.min.y_nm - gap - block.height_nm()),
            BoardEdge::Left => (seat.max.x_nm + gap, seat.min.y_nm),
            BoardEdge::Right => (seat.min.x_nm - gap - block.width_nm(), seat.min.y_nm),
        };
        self.move_block(&others, Point::new(x, y));
    }

    fn spread_row(&mut self, members: &[usize], edge: BoardEdge, hull: Rect) {
        let runs_along_x = matches!(edge, BoardEdge::Top | BoardEdge::Bottom);
        let extent = |r: Rect| {
            if runs_along_x {
                r.width_nm()
            } else {
                r.height_nm()
            }
        };
        let mut order = members.to_vec();
        order.sort_by_key(|&i| {
            let c = self.placed[i].center;
            if runs_along_x { c.x_nm } else { c.y_nm }
        });
        let total: i64 = order.iter().map(|&i| extent(self.rect(i))).sum::<i64>()
            + self.gap_nm * (order.len() as i64 - 1);
        let mid = if runs_along_x {
            hull.min.x_nm / 2 + hull.max.x_nm / 2
        } else {
            hull.min.y_nm / 2 + hull.max.y_nm / 2
        };
        let mut cursor = mid - total / 2;
        for &i in &order {
            let size = extent(self.rect(i));
            let centre = cursor + size / 2;
            if runs_along_x {
                self.placed[i].center.x_nm = centre;
            } else {
                self.placed[i].center.y_nm = centre;
            }
            cursor += size + self.gap_nm;
        }
    }

    /// Edge connectors no hint pins, plus the hinted ones, with the side each
    /// stays on. A docked connector is flush on one side of the fitted outline.
    fn edge_rows(&self) -> Vec<(usize, BoardEdge)> {
        let side = |index: usize| match index {
            0 => BoardEdge::Left,
            1 => BoardEdge::Top,
            2 => BoardEdge::Right,
            _ => BoardEdge::Bottom,
        };
        let hull = self.hull_at(|_| true, self.rim);
        let docks = self.docks();
        (0..self.placed.len())
            .filter_map(|i| {
                let component = self.component(i);
                if !matches!(component.kind.as_str(), "connector" | "jack") {
                    return None;
                }
                if let Some(edge) = flush_edge_index(self.board, self.placed[i].id) {
                    return Some((i, side(edge)));
                }
                if !docks.contains(&i) {
                    return None;
                }
                let (hull, rect) = (hull?, self.rect(i));
                let gaps = [
                    rect.min.x_nm - hull.min.x_nm,
                    rect.min.y_nm - hull.min.y_nm,
                    hull.max.x_nm - rect.max.x_nm,
                    hull.max.y_nm - rect.max.y_nm,
                ];
                let (edge, gap) = gaps
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, gap)| **gap)
                    .expect("four sides");
                (*gap <= mm_to_nm(FLUSH_SLACK_MM)).then(|| (i, side(edge)))
            })
            .collect()
    }

    /// Pull the edge connectors in with the outline: fit the body (and any
    /// pinned holes), seat each edge connector flush on its own side, and put
    /// the free holes in the smaller outline's corners. Undone unless the board
    /// gets smaller and every gap holds.
    fn shrink_edges(&mut self) {
        let rows = self.edge_rows();
        if rows.is_empty() {
            return;
        }
        let docked: Vec<usize> = rows.iter().map(|&(i, _)| i).collect();
        // The holes the rules placed are re-cornered with the outline; a hinted
        // hole stays where its hint put it.
        let holes: Vec<usize> = (0..self.placed.len())
            .filter(|&i| !self.pinned[i] && self.component(i).kind == "mounting_hole")
            .take(4)
            .collect();
        let Some(hull) = self.hull_at(|i| !docked.contains(&i) && !holes.contains(&i), self.rim)
        else {
            return;
        };
        let before = (self.placed.clone(), self.anchored.clone());
        for &hole in &holes {
            self.anchored[hole] = false;
        }
        let area = |s: &Self| {
            s.hull_at(|_| true, s.rim)
                .map_or(i64::MAX, |h| h.width_nm().saturating_mul(h.height_nm()))
        };
        let (area_before, overlaps, groups) =
            (area(self), self.overlaps(), self.review_group_gap());
        let mut hull = hull;
        for _ in 0..8 {
            for &(i, edge) in &rows {
                self.dock_on(i, edge, hull);
            }
            for edge in [
                BoardEdge::Left,
                BoardEdge::Top,
                BoardEdge::Right,
                BoardEdge::Bottom,
            ] {
                let members: Vec<usize> =
                    rows.iter().filter(|r| r.1 == edge).map(|r| r.0).collect();
                if members.len() > 1 {
                    self.spread_row(&members, edge, hull);
                }
            }
            // A row that hits the packed body pushes its own edge out instead
            // of shoving a hinted part.
            let mut grew = false;
            for &(i, edge) in &rows {
                let rect = self.rect(i);
                for j in 0..self.placed.len() {
                    if docked.contains(&j) {
                        continue;
                    }
                    let other = self.rect(j);
                    if rect_gap(rect, other) >= self.gap_nm {
                        continue;
                    }
                    let need = match edge {
                        BoardEdge::Left => rect.max.x_nm + self.gap_nm - other.min.x_nm,
                        BoardEdge::Top => rect.max.y_nm + self.gap_nm - other.min.y_nm,
                        BoardEdge::Right => other.max.x_nm + self.gap_nm - rect.min.x_nm,
                        BoardEdge::Bottom => other.max.y_nm + self.gap_nm - rect.min.y_nm,
                    };
                    if need > 0 {
                        match edge {
                            BoardEdge::Left => hull.min.x_nm -= need,
                            BoardEdge::Top => hull.min.y_nm -= need,
                            BoardEdge::Right => hull.max.x_nm += need,
                            BoardEdge::Bottom => hull.max.y_nm += need,
                        }
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        if !holes.is_empty() {
            self.holes_to_corners();
        }
        let after_area = area(self);
        let accept = self.overlaps() <= overlaps
            && self.review_group_gap() >= groups.min(self.group_gap_nm)
            && after_area < area_before;
        if !accept {
            (self.placed, self.anchored) = before;
        }
    }

    /// Holes no hint pins, at most one per corner.
    fn free_holes(&self) -> Vec<usize> {
        (0..self.placed.len())
            .filter(|&i| !self.anchored[i] && self.component(i).kind == "mounting_hole")
            .take(4)
            .collect()
    }

    fn holes_to_corners(&mut self) {
        let holes = self.free_holes();
        let Some(hull) = self.hull(|i| !holes.contains(&i)) else {
            return;
        };
        for (&index, spot) in
            holes
                .iter()
                .zip(hole_spots(self.board, hull, &self.hole_sizes(&holes)))
        {
            self.put_corner(index, spot.x_nm, spot.y_nm);
            self.anchored[index] = true;
        }
        self.relax();
    }

    /// `hull` grown until every hole in its corners clears the body, with the
    /// corner spot for each hole. Growth is the least that separates the hole
    /// from the part it hits.
    fn cornered_hull(
        &self,
        mut hull: Rect,
        holes: &[usize],
        sizes: &[(i64, i64)],
    ) -> (Rect, Vec<Point>) {
        let body: Vec<Rect> = (0..self.placed.len())
            .filter(|i| !holes.contains(i))
            .map(|i| self.rect(i))
            .collect();
        let mut spots = hole_spots(self.board, hull, sizes);
        for _ in 0..32 {
            let mut grew = false;
            for (k, &spot) in spots.iter().enumerate() {
                let r = Rect::new(
                    spot,
                    Point::new(spot.x_nm + sizes[k].0, spot.y_nm + sizes[k].1),
                );
                let Some(other) = body.iter().find(|b| rect_gap(r, **b) < self.gap_nm) else {
                    continue;
                };
                let left = spot.x_nm < hull.min.x_nm / 2 + hull.max.x_nm / 2;
                let top = spot.y_nm < hull.min.y_nm / 2 + hull.max.y_nm / 2;
                let g = self.gap_nm;
                let (edge, amount) = if left {
                    (0, r.max.x_nm + g - other.min.x_nm)
                } else {
                    (2, other.max.x_nm + g - r.min.x_nm)
                };
                let vertical = if top {
                    (1, r.max.y_nm + g - other.min.y_nm)
                } else {
                    (3, other.max.y_nm + g - r.min.y_nm)
                };
                let (edge, amount) = if vertical.1 > 0 && vertical.1 < amount {
                    vertical
                } else {
                    (edge, amount)
                };
                match edge {
                    0 => hull.min.x_nm -= amount,
                    1 => hull.min.y_nm -= amount,
                    2 => hull.max.x_nm += amount,
                    _ => hull.max.y_nm += amount,
                }
                spots = hole_spots(self.board, hull, sizes);
                grew = true;
                break;
            }
            if !grew {
                return (hull, spots);
            }
        }
        (hull, spots)
    }

    /// Round 19: keep a top or bottom USB connector's frame on both sides when
    /// the rigid move cannot hold the group gap. The two smallest passives sit
    /// rotated beside its left edge, the rest in a compact block past its right
    /// edge, and the next group gives way. False (with the caller restoring)
    /// when the seat cannot keep the connector, the gaps and the frame.
    fn seat_usb_both_sides(&mut self, usb: usize, hull: Rect) -> bool {
        let Some(name) = self.component(usb).group.clone() else {
            return false;
        };
        let others: Vec<usize> = self
            .group_members()
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .filter(|&i| i != usb)
            .collect();
        if others.len() < 3 || others.iter().any(|&i| self.anchored[i] || self.pinned[i]) {
            return false;
        }
        let seat = self.rect(usb);
        let gap = self.gap_nm + mm_to_nm(SNAP_ROOM_MM);
        let rim = self.rim;
        // The two smallest passives ride on the left, turned to lie beside the
        // connector; a diode keeps its rotation, so it is not chosen.
        let mut left = others.clone();
        left.retain(|&i| !matches!(self.component(i).kind.as_str(), "diode" | "led"));
        left.sort_by_key(|&i| {
            let r = self.rect(i);
            (r.width_nm() * r.height_nm(), i)
        });
        left.truncate(2);
        if left.len() < 2 {
            return false;
        }
        for &i in &left {
            if !self.placed[i].rotation.swaps_extents() {
                self.placed[i].rotation = Rotation::Ninety;
            }
        }
        let w = left
            .iter()
            .map(|&i| self.rect(i).width_nm())
            .max()
            .unwrap_or(0);
        let x = seat.min.x_nm - gap - w;
        if x < hull.min.x_nm + rim {
            return false;
        }
        let mut y = seat.min.y_nm + rim;
        for &i in &left {
            let r = self.rect(i);
            self.put_corner(i, x, y);
            y += r.height_nm() + gap;
        }
        // The rest in rows past the connector's right edge, no further than
        // `USB_SEAT_REACH_MM` from it.
        let right: Vec<usize> = others
            .iter()
            .copied()
            .filter(|i| !left.contains(i))
            .collect();
        let sizes: Vec<(i64, i64)> = right
            .iter()
            .map(|&i| (self.rect(i).width_nm(), self.rect(i).height_nm()))
            .collect();
        let x0 = seat.max.x_nm + gap;
        let width =
            (seat.max.x_nm + mm_to_nm(USB_SEAT_REACH_MM) - x0).min(hull.max.x_nm - rim - x0);
        if width < sizes.iter().map(|s| s.0).max().unwrap_or(0) {
            return false;
        }
        let obstacles: Vec<Rect> = (0..self.placed.len())
            .filter(|&i| i != usb && !others.contains(&i))
            .map(|i| self.rect(i))
            .collect();
        let spots = Self::shelf(
            &sizes,
            false,
            width,
            gap,
            Point::new(x0, seat.min.y_nm + rim),
            &obstacles,
        );
        for (&i, spot) in right.iter().zip(&spots) {
            self.put_corner(i, spot.x_nm, spot.y_nm);
        }
        // Push the other groups away from the frame without moving it: pin the
        // frame and every part a rule already fixed for the pass.
        let frame: Vec<usize> = (0..self.placed.len())
            .filter(|&i| self.anchored[i] || self.component(i).group.as_ref() == Some(&name))
            .collect();
        let saved: Vec<(usize, bool, bool)> = frame
            .iter()
            .map(|&i| (i, self.pinned[i], self.anchored[i]))
            .collect();
        for &i in &frame {
            self.pinned[i] = true;
        }
        self.space_groups();
        for (i, pinned, anchored) in saved {
            self.pinned[i] = pinned;
            self.anchored[i] = anchored;
        }
        // The connector itself must not have ridden with the push-apart.
        self.rect(usb) == seat
    }

    fn is_row_header(&self, index: usize) -> bool {
        let component = self.component(index);
        component.kind == "connector"
            && component
                .part
                .as_ref()
                .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("header"))
    }

    /// The edge a docked connector is flush on, if any.
    fn flush_edge(&self, index: usize, hull: Rect) -> Option<BoardEdge> {
        let rect = self.rect(index);
        let gaps = [
            (rect.min.x_nm - hull.min.x_nm, BoardEdge::Left),
            (rect.min.y_nm - hull.min.y_nm, BoardEdge::Top),
            (hull.max.x_nm - rect.max.x_nm, BoardEdge::Right),
            (hull.max.y_nm - rect.max.y_nm, BoardEdge::Bottom),
        ];
        let (gap, edge) = *gaps.iter().min_by_key(|g| g.0)?;
        (gap <= mm_to_nm(FLUSH_SLACK_MM)).then_some(edge)
    }

    /// Where a group sits along an edge, for sorting a header row.
    fn group_centre_along(&self, index: usize, edge: BoardEdge) -> i64 {
        let centre = self
            .component(index)
            .group
            .as_ref()
            .and_then(|name| self.review_group_boxes().get(name).copied())
            .map_or(self.placed[index].center, |b| {
                Point::new(
                    b.min.x_nm / 2 + b.max.x_nm / 2,
                    b.min.y_nm / 2 + b.max.y_nm / 2,
                )
            });
        match edge {
            BoardEdge::Top | BoardEdge::Bottom => centre.x_nm,
            BoardEdge::Left | BoardEdge::Right => centre.y_nm,
        }
    }

    /// Whether a header's long axis already lies along `edge`.
    fn lies_along(&self, index: usize, edge: BoardEdge) -> bool {
        let rect = self.rect(index);
        match edge {
            BoardEdge::Top | BoardEdge::Bottom => rect.width_nm() >= rect.height_nm(),
            BoardEdge::Left | BoardEdge::Right => rect.height_nm() >= rect.width_nm(),
        }
    }

    /// The parts a row moves together: its headers and their attached passives.
    fn row_parts(&self, members: &[usize]) -> Vec<usize> {
        let mut parts = members.to_vec();
        for &m in members {
            parts.extend(self.attached_passives(m));
        }
        parts
    }

    /// Turn a row header and its attached passives about the header's centre,
    /// without judging where they land: the row placement checks that after it
    /// has moved them into place.
    fn rotate_row_header(&mut self, index: usize, rot90: u8) {
        if rot90 == 0 {
            return;
        }
        let attached = self.attached_passives(index);
        let old = self.placed[index].center;
        self.placed[index].rotation = Frame::turn(self.placed[index].rotation, rot90);
        let frame = Frame {
            rot90,
            dx: 0,
            dy: 0,
        };
        for &i in &attached {
            let c = self.placed[i].center;
            let (nx, ny) = frame.map(c.x_nm - old.x_nm, c.y_nm - old.y_nm);
            self.placed[i].center = Point::new(old.x_nm + nx, old.y_nm + ny);
            self.placed[i].rotation = Frame::turn(self.placed[i].rotation, rot90);
        }
    }

    /// Put a row header and its attached passives through `frame` about the
    /// header's centre. A passive the move leaves in the way steps the least
    /// distance clear; false (with the caller restoring) when it cannot.
    fn move_row_header(&mut self, index: usize, frame: Frame, hull: Rect) -> bool {
        let attached = self.attached_passives(index);
        let old = self.placed[index].center;
        // The row itself is flush; the passives riding with it keep the rim,
        // so they never push the outline past the header's edge.
        let inner = Rect::new(
            Point::new(hull.min.x_nm + self.rim, hull.min.y_nm + self.rim),
            Point::new(hull.max.x_nm - self.rim, hull.max.y_nm - self.rim),
        );
        self.placed[index].rotation = Frame::turn(self.placed[index].rotation, frame.rot90);
        self.placed[index].center = Point::new(old.x_nm + frame.dx, old.y_nm + frame.dy);
        let reach = mm_to_nm(ATTACHED_MM);
        for &i in &attached {
            let c = self.placed[i].center;
            let (nx, ny) = frame.map(c.x_nm - old.x_nm, c.y_nm - old.y_nm);
            self.placed[i].center = Point::new(old.x_nm + nx, old.y_nm + ny);
            self.placed[i].rotation = Frame::turn(self.placed[i].rotation, frame.rot90);
            if self.clear_around(i, inner) {
                continue;
            }
            // B4: the least move that clears the row, still hugging the header.
            let base = self.placed[i].center;
            let step = mm_to_nm(GRID_MM);
            let n = (reach / step) as i64;
            let mut best: Option<(i64, Point)> = None;
            for ix in -n..=n {
                for iy in -n..=n {
                    if ix == 0 && iy == 0 {
                        continue;
                    }
                    let d2 = ix * ix + iy * iy;
                    if best.is_some_and(|(bd, _)| d2 >= bd) {
                        continue;
                    }
                    let cand = Point::new(base.x_nm + ix * step, base.y_nm + iy * step);
                    self.placed[i].center = cand;
                    if self.clear_around(i, inner)
                        && rect_gap(self.rect(index), self.rect(i)) <= reach
                    {
                        best = Some((d2, cand));
                    }
                }
            }
            let found = if let Some((_, cand)) = best {
                self.placed[i].center = cand;
                true
            } else {
                self.placed[i].center = base;
                false
            };
            if !found {
                return false;
            }
        }
        true
    }

    /// Whether a part sits inside the outline with the part gap to everything
    /// but the row, and off every reserved envelope.
    fn clear_around(&self, index: usize, hull: Rect) -> bool {
        let rect = self.rect(index);
        hull.contains(rect.min)
            && hull.contains(rect.max)
            && !self.in_reserved(rect)
            && (0..self.placed.len())
                .filter(|&j| j != index)
                .all(|j| rect_gap(rect, self.rect(j)) >= self.gap_nm)
    }

    /// Seat a header row on `edge`: every member along the edge, flush, one
    /// uniform gap (at least the group gap). Tries a quarter turn each way,
    /// slides along the edge and keeps the attached passives with the row.
    fn try_header_row(&mut self, members: &[usize], edge: BoardEdge, hull: Rect) -> bool {
        let before = (self.placed.clone(), self.anchored.clone());
        let (overlaps0, keys0) = (self.overlaps(), self.finding_keys());
        let area0 = hull.width_nm() as f64 * hull.height_nm() as f64;
        let along_x = matches!(edge, BoardEdge::Top | BoardEdge::Bottom);
        // A little room over the group gap, so a passive that spills past a
        // header's courtyard does not close the gap to the next group.
        let gap = (self.group_gap_nm + mm_to_nm(SNAP_ROOM_MM)).max(self.gap_nm);
        let skip = self.row_parts(members);
        let options: Vec<u8> = if members.iter().all(|&i| self.lies_along(i, edge)) {
            // A half turn keeps the long axis along the edge and can bring the
            // attached passives inboard; the plain seat is the fallback.
            vec![0, 2]
        } else {
            vec![3, 1]
        };
        for rot90 in options {
            // Every member must end along the edge; turn only the square ones.
            (self.placed, self.anchored) = before.clone();
            for &i in members {
                let q = match rot90 {
                    0 => 0,
                    2 => 2,
                    _ if self.lies_along(i, edge) => 0,
                    _ => rot90,
                };
                if q != 0 {
                    self.rotate_row_header(i, q);
                }
            }
            if members.iter().any(|&i| !self.lies_along(i, edge)) {
                continue;
            }
            let sizes: Vec<i64> = members
                .iter()
                .map(|&i| {
                    let r = self.rect(i);
                    if along_x { r.width_nm() } else { r.height_nm() }
                })
                .collect();
            let total = sizes.iter().sum::<i64>() + gap * (members.len() as i64 - 1);
            let (lo, hi) = if along_x {
                (hull.min.x_nm, hull.max.x_nm - total)
            } else {
                (hull.min.y_nm, hull.max.y_nm - total)
            };
            if hi < lo {
                continue;
            }
            let mean = members
                .iter()
                .map(|&i| self.group_centre_along(i, edge))
                .sum::<i64>()
                / members.len() as i64;
            let desired = (mean - total / 2).clamp(lo, hi);
            let step = mm_to_nm(GRID_MM);
            let reach = (desired - lo).max(hi - desired).max(0) / step;
            let mut order = vec![0_i64];
            for k in 1..=reach {
                order.push(k);
                order.push(-k);
            }
            for k in order {
                (self.placed, self.anchored) = before.clone();
                // Re-apply the turn, then translate each member into the row.
                for &i in members {
                    let q = match rot90 {
                        0 => 0,
                        2 => 2,
                        _ if self.lies_along(i, edge) => 0,
                        _ => rot90,
                    };
                    if q != 0 {
                        self.rotate_row_header(i, q);
                    }
                }
                let mut ok = true;
                let mut cursor = (desired + k * step).clamp(lo, hi);
                for (&i, &size) in members.iter().zip(&sizes) {
                    let old = self.rect(i);
                    let (x, y) = match edge {
                        BoardEdge::Top => (cursor, hull.min.y_nm),
                        BoardEdge::Bottom => (cursor, hull.max.y_nm - old.height_nm()),
                        BoardEdge::Left => (hull.min.x_nm, cursor),
                        BoardEdge::Right => (hull.max.x_nm - old.width_nm(), cursor),
                    };
                    let frame = Frame {
                        rot90: 0,
                        dx: x - old.min.x_nm,
                        dy: y - old.min.y_nm,
                    };
                    if !self.move_row_header(i, frame, hull) {
                        ok = false;
                        break;
                    }
                    cursor += size + gap;
                }
                if !ok {
                    continue;
                }
                // The row itself: inside, off the holes and envelopes, and a
                // part gap from everything it does not carry.
                let mut row_ok = true;
                for &i in members {
                    let r = self.rect(i);
                    if !hull.contains(r.min) || !hull.contains(r.max) || self.in_reserved(r) {
                        row_ok = false;
                        break;
                    }
                    if (0..self.placed.len())
                        .filter(|&j| !skip.contains(&j))
                        .any(|j| rect_gap(r, self.rect(j)) < self.gap_nm)
                    {
                        row_ok = false;
                        break;
                    }
                }
                let new_keys = self.finding_keys();
                let grew = self
                    .hull_at(|_| true, self.rim)
                    .is_none_or(|h| (h.width_nm() as f64 * h.height_nm() as f64) > area0 * 1.02);
                if row_ok
                    && !grew
                    && self.overlaps() <= overlaps0
                    && self.review_group_gap() >= self.group_gap_nm
                    && !new_keys.iter().any(|key| !keys0.contains(key))
                {
                    return true;
                }
            }
        }
        (self.placed, self.anchored) = before;
        false
    }

    /// Round 19: every edge header lies along its edge in one packed row. The
    /// row keeps a uniform gap, clears corner holes and reserved envelopes, and
    /// carries its attached passives. Afterwards the groups are pushed apart,
    /// the holes cornered and the USB centred again.
    fn pack_edge_headers(&mut self) {
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let before = (self.placed.clone(), self.anchored.clone());
        let (overlaps0, groups0, keys0) = (
            self.overlaps(),
            self.review_group_gap(),
            self.finding_keys(),
        );
        // A rotation hint pins a header: only free wired ones are turned.
        let mut rows: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
        let mut rowing: Vec<usize> = Vec::new();
        for i in self.docks() {
            if !self.is_row_header(i) {
                continue;
            }
            let Some(edge) = self.flush_edge(i, hull) else {
                continue;
            };
            rows.entry(match edge {
                BoardEdge::Top => 1,
                BoardEdge::Right => 2,
                BoardEdge::Bottom => 3,
                BoardEdge::Left => 0,
            })
            .or_default()
            .push(i);
            rowing.push(i);
        }
        // A lone header per edge is already seated by the docking pass; the row
        // packing only pays off once several share an edge. Smaller sets keep
        // the docking pass's seats.
        if rows.values().map(Vec::len).sum::<usize>() < 3 {
            return;
        }
        // Reserve every connector that stays out of the rows, so a row slides
        // clear of it. The row's own envelopes move with it and are dropped.
        self.reserved.clear();
        for i in self.docks() {
            if !rowing.contains(&i) && self.flush_edge(i, hull).is_some() {
                self.reserve_envelope(i);
            }
        }
        let edges = [
            BoardEdge::Right,
            BoardEdge::Bottom,
            BoardEdge::Left,
            BoardEdge::Top,
        ];
        let mut packed: Vec<(Vec<usize>, BoardEdge)> = Vec::new();
        for (key, mut members) in rows {
            let current = match key {
                0 => BoardEdge::Left,
                1 => BoardEdge::Top,
                2 => BoardEdge::Right,
                _ => BoardEdge::Bottom,
            };
            members.sort_by_key(|&i| {
                (
                    self.group_centre_along(i, current),
                    self.component(i).refdes.clone(),
                )
            });
            // A lone header already along its edge needs no row.
            if members.len() == 1 && self.lies_along(members[0], current) {
                continue;
            }
            let mut order = vec![current];
            order.extend(edges.iter().copied().filter(|e| *e != current));
            for edge in order {
                if self.try_header_row(&members, edge, hull) {
                    packed.push((members.clone(), edge));
                    break;
                }
            }
        }
        if packed.is_empty() {
            return;
        }
        // B5: push the groups apart, corner the holes, re-centre the USB. The
        // row and every flush connector are pinned for the push.
        let carried: Vec<usize> = packed.iter().flat_map(|(m, _)| self.row_parts(m)).collect();
        let frame: Vec<usize> = (0..self.placed.len())
            .filter(|&i| {
                self.anchored[i] || carried.contains(&i) || self.flush_edge(i, hull).is_some()
            })
            .collect();
        let saved: Vec<(usize, bool, bool)> = frame
            .iter()
            .map(|&i| (i, self.pinned[i], self.anchored[i]))
            .collect();
        for &i in &frame {
            self.pinned[i] = true;
        }
        self.space_groups();
        for (i, pinned, anchored) in saved {
            self.pinned[i] = pinned;
            self.anchored[i] = anchored;
        }
        self.fit_corners();
        // The cornered holes decide where the edge is: put the rows back flush
        // on that outline, then re-centre the USB on it.
        if let Some(fitted) = self.hull_at(|_| true, self.rim) {
            for (members, edge) in &packed {
                for &i in members {
                    let r = self.rect(i);
                    let (x, y) = match edge {
                        BoardEdge::Top => (r.min.x_nm, fitted.min.y_nm),
                        BoardEdge::Bottom => (r.min.x_nm, fitted.max.y_nm - r.height_nm()),
                        BoardEdge::Left => (fitted.min.x_nm, r.min.y_nm),
                        BoardEdge::Right => (fitted.max.x_nm - r.width_nm(), r.min.y_nm),
                    };
                    let frame = Frame {
                        rot90: 0,
                        dx: x - r.min.x_nm,
                        dy: y - r.min.y_nm,
                    };
                    let _ = self.move_row_header(i, frame, fitted);
                }
            }
        }
        self.centre_usb_blocks();
        if self.overlaps() > overlaps0
            || self.review_group_gap() < groups0.min(self.group_gap_nm)
            || self.finding_keys().iter().any(|key| !keys0.contains(key))
        {
            (self.placed, self.anchored) = before;
        } else {
            // The row's passives ride it; the later spread must not pull them
            // off the header.
            self.spread_keep = carried;
        }
    }

    /// Centre each flush USB connector's block along its edge on the final
    /// outline, if that keeps the outline, the gaps and the review. A compact
    /// passive frame moves with the connector as one translation, nudging a
    /// part along the edge only where it would collide. A spread group is not a
    /// frame: it falls back to the connector-alone seat the earlier rule used.
    fn centre_usb_blocks(&mut self) {
        let Some(hull) = self.hull_at(|_| true, self.rim) else {
            return;
        };
        let slack = mm_to_nm(FLUSH_SLACK_MM);
        let usbs: Vec<usize> = self
            .docks()
            .into_iter()
            .filter(|&i| is_usb_connector(self.component(i)))
            .collect();
        for usb in usbs {
            let rect = self.rect(usb);
            let gaps = [
                rect.min.x_nm - hull.min.x_nm,
                rect.min.y_nm - hull.min.y_nm,
                hull.max.x_nm - rect.max.x_nm,
                hull.max.y_nm - rect.max.y_nm,
            ];
            let (side, gap) = gaps
                .iter()
                .enumerate()
                .min_by_key(|(_, gap)| **gap)
                .expect("four sides");
            if *gap > slack {
                continue;
            }
            let centre = |a: i64, b: i64| a / 2 + b / 2;
            let (dx, dy) = if side < 2 {
                (
                    centre(hull.min.x_nm, hull.max.x_nm) - centre(rect.min.x_nm, rect.max.x_nm),
                    0,
                )
            } else {
                (
                    0,
                    centre(hull.min.y_nm, hull.max.y_nm) - centre(rect.min.y_nm, rect.max.y_nm),
                )
            };
            if (dx, dy) == (0, 0) {
                continue;
            }
            let edge = match side {
                0 => BoardEdge::Left,
                1 => BoardEdge::Top,
                2 => BoardEdge::Right,
                _ => BoardEdge::Bottom,
            };
            let before = (self.placed.clone(), self.anchored.clone());
            let (overlaps, groups, keys) = (
                self.overlaps(),
                self.review_group_gap(),
                self.finding_keys(),
            );
            let need = groups.min(self.group_gap_nm);
            let frame = self.attached_frame(usb);
            let mut rigid = false;
            if let Some(frame) = &frame {
                self.shift_with_attached(usb, dx, dy, frame);
                rigid = self.nudge_attached(frame, edge, hull, need)
                    && self.hull_at(|_| true, self.rim) == Some(hull)
                    && self.overlaps() <= overlaps
                    && self.review_group_gap() >= need
                    && !self.finding_keys().iter().any(|key| !keys.contains(key));
            }
            if !rigid {
                // The frame cannot fit at the middle (its group is interleaved
                // with a neighbouring block): fall back to the connector alone
                // and the compact seat the round-16 rule used.
                (self.placed, self.anchored) = before.clone();
                self.shift(usb, dx, dy);
                let others: Vec<usize> = match self.component(usb).group.clone() {
                    Some(name) => (0..self.placed.len())
                        .filter(|&i| {
                            i != usb
                                && !self.pinned[i]
                                && !self.anchored[i]
                                && self.component(i).group.as_ref() == Some(&name)
                        })
                        .collect(),
                    None => Vec::new(),
                };
                if matches!(edge, BoardEdge::Top | BoardEdge::Bottom) && !others.is_empty() {
                    // First try a compact both-sides frame; the one-sided
                    // shelf is the fallback when the frame cannot be seated.
                    let after_shift = (self.placed.clone(), self.anchored.clone());
                    let both_sides = self.seat_usb_both_sides(usb, hull);
                    if !both_sides {
                        (self.placed, self.anchored) = after_shift;
                        let seat = self.rect(usb);
                        let gap = self.gap_nm + mm_to_nm(SNAP_ROOM_MM);
                        let sizes: Vec<(i64, i64)> = others
                            .iter()
                            .map(|&i| (self.rect(i).width_nm(), self.rect(i).height_nm()))
                            .collect();
                        let x0 = seat.max.x_nm + gap;
                        let width = (hull.max.x_nm - self.rim - x0)
                            .max(sizes.iter().map(|s| s.0).max().unwrap_or(0));
                        let obstacles: Vec<Rect> = (0..self.placed.len())
                            .filter(|&i| i != usb && !others.contains(&i))
                            .map(|i| self.rect(i))
                            .collect();
                        let spots = Self::shelf(
                            &sizes,
                            false,
                            width,
                            gap,
                            Point::new(x0, seat.min.y_nm + self.rim),
                            &obstacles,
                        );
                        for (&i, spot) in others.iter().zip(&spots) {
                            self.put_corner(i, spot.x_nm, spot.y_nm);
                        }
                    }
                    let grew = if both_sides {
                        self.hull_at(|_| true, self.rim).is_some_and(|h| {
                            h.width_nm() as f64 * h.height_nm() as f64
                                > (hull.width_nm() as f64 * hull.height_nm() as f64 * 1.02)
                        })
                    } else {
                        self.hull_at(|_| true, self.rim) != Some(hull)
                    };
                    if grew
                        || self.overlaps() > overlaps
                        || self.review_group_gap() < need
                        || self.finding_keys().iter().any(|key| !keys.contains(key))
                    {
                        (self.placed, self.anchored) = before;
                    }
                } else {
                    self.stack_behind_usb(usb, edge);
                    if self.hull_at(|_| true, self.rim) != Some(hull)
                        || self.overlaps() > overlaps
                        || self.review_group_gap() < need
                        || self.finding_keys().iter().any(|key| !keys.contains(key))
                    {
                        (self.placed, self.anchored) = before;
                    }
                }
            }
            self.reserve_envelope(usb);
        }
    }

    /// Move the holes from the packing's reserved corners to the corners of the
    /// packed body plus the rim, growing that outline only where a hole would
    /// hit a part. Kept only when the board gets no bigger and the gaps hold.
    fn fit_corners(&mut self) {
        let holes: Vec<usize> = (0..self.placed.len())
            .filter(|&i| !self.pinned[i] && self.component(i).kind == "mounting_hole")
            .take(4)
            .collect();
        if holes.is_empty() {
            return;
        }
        let Some(hull) = self.hull_at(|i| !holes.contains(&i), self.rim) else {
            return;
        };
        let sizes = self.hole_sizes(&holes);
        let area = |s: &Self| {
            s.hull_at(|_| true, s.rim)
                .map_or(i64::MAX, |h| h.width_nm().saturating_mul(h.height_nm()))
        };
        let (before, area_before) = ((self.placed.clone(), self.anchored.clone()), area(self));
        let (overlaps, groups, keys) = (
            self.overlaps(),
            self.review_group_gap(),
            self.finding_keys(),
        );
        let (_, spots) = self.cornered_hull(hull, &holes, &sizes);
        for (&index, spot) in holes.iter().zip(spots) {
            self.put_corner(index, spot.x_nm, spot.y_nm);
            self.anchored[index] = true;
        }
        if area(self) > area_before
            || self.overlaps() > overlaps
            || self.review_group_gap() < groups.min(self.group_gap_nm)
            || self.finding_keys().iter().any(|key| !keys.contains(key))
        {
            (self.placed, self.anchored) = before;
        }
    }

    fn hole_sizes(&self, holes: &[usize]) -> Vec<(i64, i64)> {
        holes
            .iter()
            .map(|&i| (self.rect(i).width_nm(), self.rect(i).height_nm()))
            .collect()
    }

    /// How many nets join each pair of groups, counting only nets that touch
    /// `LINK_MAX_GROUPS` groups or fewer: a rail on every group says nothing
    /// about which two belong together.
    fn group_links(&self) -> BTreeMap<(String, String), usize> {
        let mut links = BTreeMap::new();
        for net in &self.board.nets {
            let mut names: Vec<&str> = net
                .endpoints
                .iter()
                .filter_map(|e| self.board.component(e.component)?.group.as_deref())
                .collect();
            names.sort_unstable();
            names.dedup();
            if names.len() > LINK_MAX_GROUPS {
                continue;
            }
            for (i, a) in names.iter().enumerate() {
                for b in &names[i + 1..] {
                    *links.entry((a.to_string(), b.to_string())).or_insert(0) += 1;
                }
            }
        }
        links
    }

    /// Put `blocks` in packing order: start from the most connected, then
    /// always take the block most linked to those already taken, so groups that
    /// share nets land in the same or the next row.
    fn order_by_links(&self, blocks: &mut Vec<&Vec<usize>>) {
        let links = self.group_links();
        let name = |b: &Vec<usize>| self.component(b[0]).group.clone().unwrap_or_default();
        let weight = |a: &Vec<usize>, b: &Vec<usize>| {
            let (x, y) = (name(a), name(b));
            links
                .get(&(x.clone(), y.clone()))
                .or_else(|| links.get(&(y, x)))
                .copied()
                .unwrap_or(0)
        };
        let mut left: Vec<&Vec<usize>> = std::mem::take(blocks);
        while !left.is_empty() {
            let best = (0..left.len())
                .max_by_key(|&i| {
                    let pull: usize = blocks.iter().map(|t| weight(left[i], t)).sum();
                    let total: usize = left.iter().map(|o| weight(left[i], o)).sum();
                    (
                        if blocks.is_empty() { total } else { pull },
                        std::cmp::Reverse(i),
                    )
                })
                .expect("left is not empty");
            blocks.push(left.remove(best));
        }
    }

    /// Lay the rest of a USB connector's group in rows on the board side of the
    /// connector, as wide as the connector, so the group is one compact block.
    /// On a side edge the rest is folded and set beside the connector.
    fn stack_behind_usb(&mut self, usb: usize, edge: BoardEdge) {
        let Some(name) = self.component(usb).group.clone() else {
            return;
        };
        let others: Vec<usize> = self
            .group_members()
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .filter(|&i| i != usb)
            .collect();
        if others.is_empty() || others.iter().any(|&i| self.anchored[i]) {
            return;
        }
        let gap = self.gap_nm + mm_to_nm(SNAP_ROOM_MM);
        let seat = self.rect(usb);
        if matches!(edge, BoardEdge::Left | BoardEdge::Right) {
            // Folded squarish, then set beside the connector, centred on it.
            self.fold(&others, gap);
            let block = self.bounds(&others);
            let x = if edge == BoardEdge::Left {
                seat.max.x_nm + gap
            } else {
                seat.min.x_nm - gap - block.width_nm()
            };
            let y = seat.min.y_nm / 2 + seat.max.y_nm / 2 - block.height_nm() / 2;
            self.move_block(&others, Point::new(x, y));
            return;
        }
        // The connector in the middle, parts in columns beside it, the rest in
        // rows on the board side: the block stays about as deep as the connector.
        let sizes: Vec<(i64, i64)> = std::iter::once(usb)
            .chain(others.iter().copied())
            .map(|i| (self.rect(i).width_nm(), self.rect(i).height_nm()))
            .collect();
        let rows = if edge == BoardEdge::Top { 1 } else { -1 };
        let spots = Self::ring(&sizes, gap, Point::new(0, 0), rows);
        let (dx, dy) = (seat.min.x_nm - spots[0].x_nm, seat.min.y_nm - spots[0].y_nm);
        // Only the connector reaches the edge; the parts beside it keep the rim.
        let rim = self.rim;
        let inward = others
            .iter()
            .zip(&spots[1..])
            .map(|(&i, spot)| {
                let h = self.rect(i).height_nm();
                if edge == BoardEdge::Top {
                    seat.min.y_nm + rim - (spot.y_nm + dy)
                } else {
                    spot.y_nm + dy + h - (seat.max.y_nm - rim)
                }
            })
            .max()
            .unwrap_or(0)
            .max(0);
        let dy = if edge == BoardEdge::Top {
            dy + inward
        } else {
            dy - inward
        };
        for (&i, spot) in others.iter().zip(&spots[1..]) {
            let r = self.rect(i);
            self.shift(i, spot.x_nm + dx - r.min.x_nm, spot.y_nm + dy - r.min.y_nm);
        }
    }

    /// Members of each declared group, in board order.
    fn group_members(&self) -> BTreeMap<String, Vec<usize>> {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for index in 0..self.placed.len() {
            if self.lone_connector(index) {
                continue;
            }
            if let Some(group) = &self.component(index).group {
                groups.entry(group.clone()).or_default().push(index);
            }
        }
        groups
    }

    fn bounds(&self, members: &[usize]) -> Rect {
        members
            .iter()
            .map(|&i| self.rect(i))
            .reduce(|a, b| {
                Rect::new(
                    Point::new(a.min.x_nm.min(b.min.x_nm), a.min.y_nm.min(b.min.y_nm)),
                    Point::new(a.max.x_nm.max(b.max.x_nm), a.max.y_nm.max(b.max.y_nm)),
                )
            })
            .expect("a group has members")
    }

    /// Pull each movable member a quarter of the way to its group's centre.
    fn cohere(&mut self, groups: &BTreeMap<String, Vec<usize>>) {
        for members in groups.values().filter(|members| members.len() > 1) {
            let n = members.len() as i64;
            let cx = members
                .iter()
                .map(|&i| self.placed[i].center.x_nm)
                .sum::<i64>()
                / n;
            let cy = members
                .iter()
                .map(|&i| self.placed[i].center.y_nm)
                .sum::<i64>()
                / n;
            for index in self.movable(members) {
                let center = self.placed[index].center;
                self.shift(index, (cx - center.x_nm) / 4, (cy - center.y_nm) / 4);
            }
        }
    }

    /// Lay `sizes` out in rows no wider than `width` from `origin`, tallest
    /// first, keeping `gap` between them and clear of `obstacles`. Returns each
    /// item's top-left corner.
    fn shelf(
        sizes: &[(i64, i64)],
        keep_order: bool,
        width: i64,
        gap: i64,
        origin: Point,
        obstacles: &[Rect],
    ) -> Vec<Point> {
        let mut order: Vec<usize> = (0..sizes.len()).collect();
        if !keep_order {
            order.sort_by_key(|&i| (-sizes[i].1, i));
        }
        let mut out = vec![origin; sizes.len()];
        let (mut cursor_x, mut cursor_y, mut row_height) = (origin.x_nm, origin.y_nm, 0);
        for slot in order {
            let (item_w, item_h) = sizes[slot];
            loop {
                if cursor_x > origin.x_nm && cursor_x + item_w > origin.x_nm + width {
                    (cursor_x, cursor_y, row_height) =
                        (origin.x_nm, cursor_y + row_height + gap, 0);
                }
                let cand = Rect::new(
                    Point::new(cursor_x, cursor_y),
                    Point::new(cursor_x + item_w, cursor_y + item_h),
                );
                let blocker = obstacles.iter().find(|obstacle| {
                    Rect::new(
                        Point::new(obstacle.min.x_nm - gap, obstacle.min.y_nm - gap),
                        Point::new(obstacle.max.x_nm + gap, obstacle.max.y_nm + gap),
                    )
                    .intersects(&cand)
                });
                match blocker {
                    Some(obstacle) => cursor_x = obstacle.max.x_nm + gap + 1,
                    None => break,
                }
            }
            out[slot] = Point::new(cursor_x, cursor_y);
            cursor_x += item_w + gap;
            row_height = row_height.max(item_h);
        }
        out
    }

    /// Lay `sizes` round the chip `sizes[0]`, the rest nearest-first: columns
    /// beside it as tall as it is, the remainder in rows above and below, so its
    /// support parts sit on every side. `rows` is 0 for rows on both sides, else
    /// the sign of the one side they go on (positive below). The box's top-left
    /// corner is `corner`.
    fn ring(sizes: &[(i64, i64)], gap: i64, corner: Point, rows: i64) -> Vec<Point> {
        let chip = sizes[0];
        // Columns beside the chip, nearest first, each as tall as the chip. A
        // one-sided block (rows != 0) takes more columns instead of more rows.
        let mut cols: [Vec<Vec<usize>>; 2] = [Vec::new(), Vec::new()];
        let mut col_h: [Vec<i64>; 2] = [Vec::new(), Vec::new()];
        let mut rest: Vec<usize> = Vec::new();
        for (k, &(item_w, item_h)) in sizes.iter().enumerate().skip(1) {
            let add = |h: i64| item_h + if h == 0 { 0 } else { gap };
            let room = |side: usize| match col_h[side].last() {
                Some(&h) if h + add(h) <= chip.1 => true,
                Some(_) => rows != 0,
                None => item_h <= chip.1,
            };
            let side = [k % 2, 1 - k % 2]
                .into_iter()
                .find(|&side| item_w * 5 <= chip.0 * 3 && room(side));
            let Some(side) = side else {
                rest.push(k);
                continue;
            };
            match col_h[side].last_mut() {
                Some(h) if *h + add(*h) <= chip.1 => {
                    *h += add(*h);
                    cols[side].last_mut().expect("a column").push(k);
                }
                _ => {
                    col_h[side].push(item_h);
                    cols[side].push(vec![k]);
                }
            }
        }
        let col_w: [Vec<i64>; 2] = [0, 1].map(|side| {
            cols[side]
                .iter()
                .map(|c| c.iter().map(|&k| sizes[k].0).max().unwrap_or(0))
                .collect()
        });
        let reach = |side: usize| col_w[side].iter().map(|w| w + gap).sum::<i64>();
        let (left, right) = (reach(0), reach(1));
        let width =
            (left + chip.0 + right).max(rest.iter().map(|&k| sizes[k].0).max().unwrap_or(0));
        // Local frame: the chip's top-left corner is the origin.
        let mut at = vec![(0_i64, 0_i64); sizes.len()];
        for side in 0..2 {
            let mut edge = if side == 0 { 0 } else { chip.0 };
            for (column, &w) in cols[side].iter().zip(&col_w[side]) {
                let x = if side == 0 {
                    edge - gap - w
                } else {
                    edge + gap
                };
                let mut y = i64::midpoint(
                    chip.1 - column.iter().map(|&k| sizes[k].1 + gap).sum::<i64>(),
                    gap,
                );
                for &k in column {
                    // Hard against the chip's side of the column.
                    at[k].0 = if side == 0 { x + w - sizes[k].0 } else { x };
                    at[k].1 = y;
                    y += sizes[k].1 + gap;
                }
                edge = if side == 0 { x } else { x + w };
            }
        }
        let mid = i64::midpoint(chip.0 - left, right);
        // The rest go to the side with less area so far, the one-sided case to one.
        let mut halves: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        let mut area = [0_i64; 2];
        for &slot in &rest {
            let side = match rows {
                0 => usize::from(area[1] < area[0]),
                one_sided => usize::from(one_sided > 0),
            };
            area[side] += sizes[slot].0 / 1000 * (sizes[slot].1 / 1000);
            halves[side].push(slot);
        }
        for up in [true, false] {
            // The chip's silk reference sits just above it: keep that strip clear.
            // Whole grid cells, so the rows above it stay on the grid.
            let cell = mm_to_nm(GRID_MM);
            let label = if rows == 0 {
                (mm_to_nm(crate::LABEL_ROOM_MM) + cell - 1) / cell * cell
            } else {
                0
            };
            let mut edge = if up { -gap - label } else { chip.1 + gap };
            let list = std::mem::take(&mut halves[usize::from(!up)]);
            let mut start = 0;
            while start < list.len() {
                let (mut line_w, mut line_h, mut next) = (0, 0, start);
                while next < list.len()
                    && (next == start || line_w + gap + sizes[list[next]].0 <= width)
                {
                    line_w += sizes[list[next]].0 + if next == start { 0 } else { gap };
                    line_h = line_h.max(sizes[list[next]].1);
                    next += 1;
                }
                let mut x = mid - line_w / 2;
                let y = if up { edge - line_h } else { edge };
                for &slot in &list[start..next] {
                    at[slot] = (x, y);
                    x += sizes[slot].0 + gap;
                }
                edge = if up { y - gap } else { y + line_h + gap };
                start = next;
            }
        }
        let min_x = at.iter().map(|a| a.0).min().unwrap_or(0);
        let min_y = at.iter().map(|a| a.1).min().unwrap_or(0);
        at.iter()
            .map(|a| Point::new(corner.x_nm + a.0 - min_x, corner.y_nm + a.1 - min_y))
            .collect()
    }

    /// Where `sizes` go in a `(width, height)` region at `origin` round `walls`:
    /// the blocks flagged in `chip` in one row at the middle, nearest the
    /// centre that is clear, and the rest in a band above and a band below,
    /// each hard against its edge. Without a chip block, one shelf from the top.
    fn banded(
        sizes: &[(i64, i64)],
        chip: &[bool],
        (width, height): (i64, i64),
        gap: i64,
        origin: Point,
        walls: &[Rect],
    ) -> Option<Vec<Point>> {
        let end_y = origin.y_nm + height;
        let grown = |r: &Rect| {
            Rect::new(
                Point::new(r.min.x_nm - gap, r.min.y_nm - gap),
                Point::new(r.max.x_nm + gap, r.max.y_nm + gap),
            )
        };
        let at = |p: Point, s: (i64, i64)| Rect::new(p, Point::new(p.x_nm + s.0, p.y_nm + s.1));
        let mut spots = vec![origin; sizes.len()];
        let mut walls = walls.to_vec();
        let mut chips: Vec<usize> = (0..sizes.len()).filter(|&k| chip[k]).collect();
        let row_w = chips.iter().map(|&k| sizes[k].0 + gap).sum::<i64>() - gap;
        let row_h = chips.iter().map(|&k| sizes[k].1).max().unwrap_or(0);
        let x0 = origin.x_nm + (width - row_w) / 2;
        let ideal = origin.y_nm + (height - row_h) / 2;
        let row_at = |y: i64| at(Point::new(x0, y), (row_w, row_h));
        let step = mm_to_nm(CHIP_ROW_STEP_MM);
        // With no clear seat for the row the chip blocks are shelved like the rest.
        let seat = (!chips.is_empty() && row_w <= width && row_h <= height)
            .then(|| {
                (0..=height / step)
                    .flat_map(|n| [ideal + n * step, ideal - n * step])
                    .find(|&y| {
                        y >= origin.y_nm
                            && y + row_h <= end_y
                            && !walls.iter().any(|w| grown(w).intersects(&row_at(y)))
                    })
            })
            .flatten();
        if let Some(y) = seat {
            let mut x = x0;
            for &k in &chips {
                spots[k] = Point::new(x, y + (row_h - sizes[k].1) / 2);
                walls.push(at(spots[k], sizes[k]));
                x += sizes[k].0 + gap;
            }
        } else {
            chips.clear();
        }
        let mut rest: Vec<usize> = (0..sizes.len())
            .filter(|&k| !chip[k] || chips.is_empty())
            .collect();
        rest.sort_by_key(|&k| (std::cmp::Reverse(sizes[k].0 * sizes[k].1), k));
        let (mut above, mut below): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        let (mut area_above, mut area_below) = (0_i64, 0_i64);
        for k in rest {
            let area = sizes[k].0 / 1000 * (sizes[k].1 / 1000);
            if chips.is_empty() || area_above <= area_below {
                above.push(k);
                area_above += area;
            } else {
                below.push(k);
                area_below += area;
            }
        }
        let fits = |spots: &[Point], list: &[usize]| {
            list.iter()
                .all(|&k| spots[k].y_nm + sizes[k].1 <= end_y && spots[k].y_nm >= origin.y_nm)
        };
        let top: Vec<(i64, i64)> = above.iter().map(|&k| sizes[k]).collect();
        for (&k, p) in above
            .iter()
            .zip(Self::shelf(&top, true, width, gap, origin, &walls))
        {
            spots[k] = p;
        }
        if !fits(&spots, &above) {
            return None;
        }
        walls.extend(above.iter().map(|&k| at(spots[k], sizes[k])));
        // The lower band is laid out upside down, so its first row is the bottom one.
        let flip = origin.y_nm + end_y;
        let mirrored: Vec<Rect> = walls
            .iter()
            .map(|r| {
                Rect::new(
                    Point::new(r.min.x_nm, flip - r.max.y_nm),
                    Point::new(r.max.x_nm, flip - r.min.y_nm),
                )
            })
            .collect();
        let bottom: Vec<(i64, i64)> = below.iter().map(|&k| sizes[k]).collect();
        for (&k, p) in below
            .iter()
            .zip(Self::shelf(&bottom, true, width, gap, origin, &mirrored))
        {
            spots[k] = Point::new(p.x_nm, flip - p.y_nm - sizes[k].1);
        }
        fits(&spots, &below).then_some(spots)
    }

    /// Move `members` rigidly so their box's top-left corner is `corner`.
    fn move_block(&mut self, members: &[usize], corner: Point) {
        let b = self.bounds(members);
        for &i in members {
            self.shift(i, corner.x_nm - b.min.x_nm, corner.y_nm - b.min.y_nm);
        }
    }

    /// Re-lay a block's members in rows, which gives a compact, squarish block
    /// whose parts cannot overlap.
    fn fold(&mut self, members: &[usize], gap: i64) {
        let b = self.bounds(members);
        if members.len() < 2 {
            return;
        }
        let sizes: Vec<(i64, i64)> = members
            .iter()
            .map(|&i| (self.rect(i).width_nm(), self.rect(i).height_nm()))
            .collect();
        let area: f64 = sizes.iter().map(|s| s.0 as f64 * s.1 as f64).sum();
        let width = ((area * FOLD_FILL * FOLD_TARGET_ASPECT).sqrt() as i64)
            .max(sizes.iter().map(|s| s.0).max().unwrap_or(0));
        // Biggest part first, then the rest nearest-first, so the parts that sat
        // beside the IC end up in the rows beside it.
        let centres: Vec<(i64, i64)> = members
            .iter()
            .map(|&i| {
                let r = self.rect(i);
                (
                    r.min.x_nm / 2 + r.max.x_nm / 2,
                    r.min.y_nm / 2 + r.max.y_nm / 2,
                )
            })
            .collect();
        let anchor = (0..members.len())
            .max_by_key(|&k| (sizes[k].0 * sizes[k].1, std::cmp::Reverse(k)))
            .unwrap_or(0);
        let mut order: Vec<usize> = (0..members.len()).filter(|&k| k != anchor).collect();
        order.sort_by_key(|&k| {
            let (dx, dy) = (
                centres[k].0 - centres[anchor].0,
                centres[k].1 - centres[anchor].1,
            );
            (dx.abs() + dy.abs(), k)
        });
        order.insert(0, anchor);
        let ordered: Vec<(i64, i64)> = order.iter().map(|&k| sizes[k]).collect();
        let mut spots = vec![b.min; members.len()];
        if self.is_major_ic(members[anchor]) {
            let placed = Self::ring(&ordered, gap, b.min, 0);
            for (slot, &k) in order.iter().enumerate() {
                spots[k] = placed[slot];
            }
        } else {
            let placed = Self::shelf(&ordered, true, width, gap, b.min, &[]);
            for (slot, &k) in order.iter().enumerate() {
                spots[k] = placed[slot];
            }
        }
        for (&i, spot) in members.iter().zip(spots) {
            let r = self.rect(i);
            self.shift(i, spot.x_nm - r.min.x_nm, spot.y_nm - r.min.y_nm);
        }
    }

    /// Fitted outline's (area, distance of width/height from 1.0..=1.8) in mm.
    /// How far the packed groups may grow the outline. A spacing makes the
    /// blocks themselves bigger, so it widens the allowance.
    fn area_growth(&self) -> f64 {
        if self.infl > 0 {
            SPACED_AREA_GROWTH
        } else {
            MAX_AREA_GROWTH
        }
    }

    fn shape(&self) -> (f64, f64) {
        let Some(o) = self.hull(|_| true) else {
            return (0.0, 0.0);
        };
        let (w, h) = (o.width_nm() as f64, o.height_nm() as f64);
        let aspect = w / h.max(1.0);
        (w * h, (1.0 - aspect).max(aspect - 1.8).max(0.0))
    }

    /// Pack the declared groups as rigid blocks in rows over the board's
    /// footprint, so they form a grid instead of a column. A block holding a
    /// pinned part stays put. If the packing at `GROUP_GAP_MM` stretches the
    /// board, it is retried with the tighter `RULE_GAP_MM`; failing that, the
    /// rule is dropped.
    fn groups(&mut self) {
        let groups = self.group_members();
        let Some(base) = self.hull(|_| true) else {
            return;
        };
        if groups.is_empty() {
            return;
        }
        let (area0, aspect0) = self.shape();
        let start = (self.placed.clone(), self.anchored.clone());
        for _ in 0..COHESION_ROUNDS {
            self.cohere(&groups);
            self.relax_pass();
        }
        self.relax();
        let usbs: Vec<usize> = self
            .docks()
            .into_iter()
            .filter(|&i| self.anchored[i])
            .collect();
        let stacked: Vec<usize> = usbs
            .iter()
            .copied()
            .filter(|&u| !self.lone_connector(u))
            .collect();
        for u in stacked {
            let edge = floorplan::nearest_edge(self.placed[u].center, base);
            self.stack_behind_usb(u, edge);
        }
        let cohered = (self.placed.clone(), self.anchored.clone());
        let margin = self.rim;
        let holes = self.free_holes();
        let hole_sizes = self.hole_sizes(&holes);
        // Connectors the USB rule put on an edge get a slot there, centred on
        // the packed outline, instead of staying where the old outline had them.
        // Each such connector moves with the rest of its group as one block, so
        // the group stays whole.
        let usb_blocks: Vec<UsbBlock> = usbs
            .iter()
            .map(|&u| {
                let members: Vec<usize> = match &self.component(u).group {
                    Some(name) if !self.lone_connector(u) => groups[name].clone(),
                    _ => vec![u],
                };
                let (block, r) = (self.bounds(&members), self.rect(u));
                UsbBlock {
                    usb: u,
                    members,
                    size: (r.width_nm(), r.height_nm()),
                    offset: (r.min.x_nm - block.min.x_nm, r.min.y_nm - block.min.y_nm),
                    extent: (block.width_nm(), block.height_nm()),
                    edge: floorplan::nearest_edge(self.placed[u].center, base),
                }
            })
            .collect();
        let usb_members: Vec<usize> = usb_blocks
            .iter()
            .flat_map(|b| b.members.iter().copied())
            .collect();
        let gaps: Vec<i64> = if self.infl > 0 {
            vec![self.group_gap_nm]
        } else {
            vec![self.group_gap_nm, self.gap_nm]
        };
        for gap in gaps {
            (self.placed, self.anchored) = cohered.clone();
            let blocks: Vec<Vec<usize>> = groups
                .values()
                .filter(|m| !m.iter().any(|i| usb_members.contains(i)))
                .cloned()
                .collect();
            let (free, fixed): (Vec<&Vec<usize>>, Vec<&Vec<usize>>) = blocks
                .iter()
                .partition(|members| self.movable(members).len() == members.len());
            for members in &free {
                self.fold(members, self.gap_nm + mm_to_nm(SNAP_ROOM_MM));
            }
            let obstacles: Vec<Rect> = (0..self.placed.len())
                .filter(|&i| self.anchored[i] && !usb_members.contains(&i))
                .chain(fixed.iter().flat_map(|m| m.iter().copied()))
                .map(|i| self.rect(i))
                .collect();
            let origin = Point::new(base.min.x_nm + margin, base.min.y_nm + margin);
            let links = self.group_links();
            let name_of = |b: &Vec<usize>| self.component(b[0]).group.clone().unwrap_or_default();
            // Pack in the order that keeps linked groups together and in the
            // tallest-first order that wastes least; keep whichever scores better.
            let mut linked = free.clone();
            self.order_by_links(&mut linked);
            let mut tall = free.clone();
            tall.sort_by_key(|b| std::cmp::Reverse(self.bounds(b).height_nm()));
            let plan = |order: &Vec<&Vec<usize>>| {
                let sizes: Vec<(i64, i64)> = order
                    .iter()
                    .map(|m| {
                        let b = self.bounds(m);
                        (b.width_nm(), b.height_nm())
                    })
                    .collect();
                let names: Vec<String> = order.iter().map(|b| name_of(b)).collect();
                // Where each block's major IC sits from the block's corner.
                let chips: Vec<Vec<(i64, i64)>> = order
                    .iter()
                    .map(|m| {
                        let corner = self.bounds(m).min;
                        m.iter()
                            .filter(|&&i| self.is_major_ic(i))
                            .map(|&i| {
                                let c = self.placed[i].center;
                                (c.x_nm - corner.x_nm, c.y_nm - corner.y_nm)
                            })
                            .collect()
                    })
                    .collect();
                let is_chip: Vec<bool> = order
                    .iter()
                    .map(|m| m.iter().any(|&i| self.is_major_ic(i)))
                    .collect();
                // Try a spread of row widths. The holes' corners and the USB's
                // seat are reserved at the outline the packing will have, so the
                // blocks leave them free.
                let widest = sizes.iter().map(|s| s.0).max().unwrap_or(0);
                let reach = (base.width_nm() * 2).max(widest);
                let tallest = sizes.iter().map(|s| s.1).max().unwrap_or(0);
                let total: i64 = sizes.iter().map(|s| s.0 / 1000 * (s.1 / 1000)).sum();
                // The smallest outline of this width in which every block fits
                // round the reserved corners, or none.
                let pack = |width: i64| {
                    let floor = (total * 1_000_000 / width.max(1)).max(tallest);
                    (0..=60).find_map(|step| {
                        let height = floor + floor * step / 40;
                        // Holes sit in this outline's corners and a connector
                        // centred on its edge, so the board ends up this size.
                        let outline = Rect::new(
                            Point::new(origin.x_nm - margin, origin.y_nm - margin),
                            Point::new(origin.x_nm + width + margin, origin.y_nm + height + margin),
                        );
                        let at = |p: Point, (w, h): (i64, i64)| {
                            Rect::new(p, Point::new(p.x_nm + w, p.y_nm + h))
                        };
                        let slots: Vec<Rect> = hole_spots(self.board, outline, &hole_sizes)
                            .into_iter()
                            .zip(&hole_sizes)
                            .map(|(p, s)| at(p, *s))
                            .chain(usb_blocks.iter().map(|b| {
                                let seat = usb_spot(outline, b.size, b.edge, 0);
                                at(
                                    Point::new(seat.x_nm - b.offset.0, seat.y_nm - b.offset.1),
                                    b.extent,
                                )
                            }))
                            .collect();
                        // A tall connector block can reach into a hole's corner.
                        if slots
                            .iter()
                            .enumerate()
                            .any(|(i, a)| slots[i + 1..].iter().any(|b| a.intersects(b)))
                        {
                            return None;
                        }
                        let walls: Vec<Rect> = obstacles.iter().chain(&slots).copied().collect();
                        let spots =
                            Self::banded(&sizes, &is_chip, (width, height), gap, origin, &walls)?;
                        Some((spots, slots, outline))
                    })
                };
                (0..=24)
                    .filter_map(|step| pack(widest + (reach - widest) * step / 24))
                    .map(|(spots, slots, outline)| {
                        // Linked blocks more than the limit apart count against it.
                        let far = (0..names.len())
                            .flat_map(|i| (i + 1..names.len()).map(move |j| (i, j)))
                            .filter(|&(i, j)| {
                                let key = (names[i].clone(), names[j].clone());
                                let rev = (names[j].clone(), names[i].clone());
                                (links.contains_key(&key) || links.contains_key(&rev)) && {
                                    let (a, b) = (
                                        Rect::new(
                                            spots[i],
                                            Point::new(
                                                spots[i].x_nm + sizes[i].0,
                                                spots[i].y_nm + sizes[i].1,
                                            ),
                                        ),
                                        Rect::new(
                                            spots[j],
                                            Point::new(
                                                spots[j].x_nm + sizes[j].0,
                                                spots[j].y_nm + sizes[j].1,
                                            ),
                                        ),
                                    );
                                    let gx = (b.min.x_nm - a.max.x_nm).max(a.min.x_nm - b.max.x_nm);
                                    let gy = (b.min.y_nm - a.max.y_nm).max(a.min.y_nm - b.max.y_nm);
                                    gx.max(gy) > mm_to_nm(MAX_LINK_GAP_MM)
                                }
                            })
                            .count();
                        let (w, h) = (outline.width_nm(), outline.height_nm());
                        let landscape = w >= h && w * 5 <= h * 9;
                        let area = w / 1000 * (h / 1000);
                        let over = area as f64 > self.area_growth() * area0 / 1e6;
                        // How far the major ICs sit from the middle of this outline.
                        let (mx, my) = (
                            i64::midpoint(outline.min.x_nm, outline.max.x_nm),
                            i64::midpoint(outline.min.y_nm, outline.max.y_nm),
                        );
                        let off_centre: f64 = spots
                            .iter()
                            .zip(&chips)
                            .flat_map(|(p, cs)| {
                                cs.iter().map(move |c| {
                                    ((p.x_nm + c.0 - mx) as f64).hypot((p.y_nm + c.1 - my) as f64)
                                })
                            })
                            .sum();
                        (
                            (!landscape, over, area / AREA_BUCKET_UM2, far, area),
                            (off_centre / 1e6) as i64 / CHIP_BUCKET_MM,
                            spots,
                            slots,
                        )
                    })
                    .min_by_key(|(k, chip, _, _)| (k.0, k.1, k.2, *chip, k.3, k.4))
            };
            let mut orders = vec![linked, tall];
            if free.len() <= MAX_PERMUTED_BLOCKS {
                orders.extend(permutations(&free));
            }
            let mut cands: Vec<_> = orders
                .into_iter()
                .filter_map(|order| {
                    plan(&order).map(|(key, chip, spots, slots)| (key, chip, order, spots, slots))
                })
                .collect();
            // Among packings of about the smallest board, the most central
            // chips come first; with no chip this keeps the order as it was.
            let least = cands.iter().map(|c| c.0.4).min().unwrap_or(0);
            cands.sort_by_key(|c| {
                let (landscape, over, bucket, far, area) = c.0;
                let roomy = area as f64 > CHIP_AREA_SLACK * least as f64;
                (landscape, over, roomy, c.1, bucket, far, area)
            });
            let folded = (self.placed.clone(), self.anchored.clone());
            // The best few packings are tried in turn: the one that scores best
            // can still block a connector's escape corridor.
            for (_, _, free, spots, reserved) in cands.into_iter().take(if self.infl > 0 {
                SPACED_TRIED_PACKINGS
            } else {
                TRIED_PACKINGS
            }) {
                (self.placed, self.anchored) = folded.clone();
                for (members, spot) in free.iter().zip(spots) {
                    self.move_block(members, spot);
                }
                for (&hole, rect) in holes.iter().zip(&reserved) {
                    self.put_corner(hole, rect.min.x_nm, rect.min.y_nm);
                }
                for (block, rect) in usb_blocks.iter().zip(&reserved[holes.len()..]) {
                    let seat = self.rect(block.usb);
                    let (dx, dy) = (
                        rect.min.x_nm + block.offset.0 - seat.min.x_nm,
                        rect.min.y_nm + block.offset.1 - seat.min.y_nm,
                    );
                    for &member in &block.members {
                        self.shift(member, dx, dy);
                    }
                }
                // Only the loose parts yield to the blocks, which keeps their gaps.
                let loose = self.anchored.clone();
                for &i in free
                    .iter()
                    .flat_map(|m| m.iter())
                    .chain(&holes)
                    .chain(&usb_members)
                {
                    self.anchored[i] = true;
                }
                self.relax();
                self.anchored = loose;
                for &placed in holes.iter().chain(&usb_members) {
                    self.anchored[placed] = true;
                }
                let (area, aspect) = self.shape();
                if area <= self.area_growth() * area0
                    && aspect <= aspect0
                    && self.overlaps() == 0
                    && !(self.keep_clean && self.needs_revision())
                {
                    return;
                }
            }
        }
        (self.placed, self.anchored) = start;
    }
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

    fn load_group_corner_hole() -> Board {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = dir.join("tests/fixtures/group_corner_hole.synth");
        let source = std::fs::read_to_string(&path).expect("group_corner_hole fixture");
        let file = path.display().to_string();
        let parse = synth_parser::parse(&source, file.clone());
        let ast = parse.ast.as_ref().expect("parse");
        let root = dir.join("..").join("..").canonicalize().expect("workspace");
        let registry =
            synth_registry::load_dir(&root.join("registry").join("parts")).expect("registry");
        let loader = synth_ir::FsImportLoader { root };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        synth_ir::lower(&resolved.program, &registry, &file)
            .board
            .expect("board")
    }

    #[test]
    fn a_group_with_a_corner_hole_gets_the_same_box_in_the_rules_and_the_judge() {
        let board = load_group_corner_hole();
        let centre = |refdes: &str| match refdes {
            "U1" => (30.0, 30.0),
            "C1" => (30.0, 38.0),
            "J1" => (50.0, 30.0),
            "R1" => (20.0, 2.0),
            "H1" => (2.0, 2.0),
            _ => (40.0, 40.0),
        };
        let placement = Placement {
            board_outline: rect(0.0, 0.0, 80.0, 60.0),
            components: board
                .components
                .iter()
                .map(|component| {
                    let (x, y) = centre(&component.refdes);
                    ComponentPlacement {
                        id: component.id,
                        center: Point::new(mm_to_nm(x), mm_to_nm(y)),
                        rotation: Rotation::Zero,
                        layer: synth_geometry::Layer::Top,
                    }
                })
                .collect(),
        };

        let state = compression_state(&board, &placement);
        let boxes = state.review_group_boxes();
        let power = boxes.get("POWER").expect("POWER box");
        // The corner hole and the connector (the group has a major part) leave.
        assert!(
            power.min.x_nm > mm_to_nm(20.0),
            "the corner hole is still in the box: {:.3}",
            nm_to_mm(power.min.x_nm)
        );

        let report = crate::judge::judge(&board, &placement);
        let judge_gap = report
            .metrics
            .iter()
            .find(|metric| metric.key == "group_gap")
            .expect("group_gap row")
            .measured;
        let rules_gap = nm_to_mm(state.review_group_gap());
        assert!(
            (judge_gap - rules_gap).abs() < 1e-6,
            "judge {judge_gap:.6} mm and rules {rules_gap:.6} mm disagree"
        );
        assert!(
            judge_gap > 20.0,
            "the groups are not far apart: {judge_gap:.3}"
        );
    }

    #[test]
    fn largest_empty_rect_finds_the_open_side() {
        let hull = rect(0.0, 0.0, 100.0, 50.0);
        let block = rect(40.0, 10.0, 60.0, 40.0);
        let (area, found) = largest_empty_rect(hull, &[block], mm_to_nm(1.0));
        // The open left and right strips are 39 x 50 = 1950 mm2.
        assert!(
            area >= mm_to_nm(38.0) * mm_to_nm(49.0),
            "largest empty area {area}"
        );
        assert!(
            found.max.x_nm <= mm_to_nm(39.0) || found.min.x_nm >= mm_to_nm(61.0),
            "rectangle {found:?} is not beside the block"
        );
    }

    #[test]
    fn largest_empty_rect_with_no_parts_is_the_board() {
        let hull = rect(0.0, 0.0, 100.0, 50.0);
        let (area, _) = largest_empty_rect(hull, &[], mm_to_nm(1.0));
        assert_eq!(area, mm_to_nm(100.0) * mm_to_nm(50.0));
    }
}

#[cfg(test)]
mod frame_tests {
    use super::Frame;
    use synth_geometry::Rotation;

    #[test]
    fn frame_maps_a_quarter_turn() {
        // (3,-9) turned +90 about its part is (-(-9), 3) = (9,3), then moved.
        let frame = Frame {
            rot90: 1,
            dx: 5,
            dy: -7,
        };
        assert_eq!(frame.map(3, -9), (9 + 5, 3 - 7));
        assert_eq!(Frame::turn(Rotation::Zero, 1), Rotation::TwoSeventy);
        assert_eq!(Frame::turn(Rotation::Zero, 3), Rotation::Ninety);
        assert_eq!(Frame::turn(Rotation::TwoSeventy, 1), Rotation::OneEighty);
    }
}

#[cfg(test)]
mod area_guard_tests {
    use super::*;

    #[test]
    fn area_guard_rejects_a_fix_that_grows_the_outline() {
        // The fix doubles the outline: both limits reject it.
        let huge = area_guard(1000.0, 1000.0, 2000.0);
        assert!(
            huge.over_step && huge.over_total && huge.rejects(),
            "{huge:?}"
        );
        // Two percent over the state before the fix trips the step guard only.
        let step = area_guard(1000.0, 1000.0, 1020.0);
        assert!(
            step.over_step && !step.over_total && step.rejects(),
            "{step:?}"
        );
        // Four percent over the pass entry trips the total guard only.
        let total = area_guard(1000.0, 1030.0, 1040.0);
        assert!(
            total.over_total && !total.over_step && total.rejects(),
            "{total:?}"
        );
        // Half a percent is within both limits.
        let fine = area_guard(1000.0, 1000.0, 1005.0);
        assert!(!fine.rejects(), "{fine:?}");
    }
}
