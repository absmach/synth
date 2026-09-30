// SPDX-License-Identifier: Apache-2.0

//! MaxRects compaction of an already-placed layout.
//!
//! [`crate::place_clusters`] produces a *semantic* arrangement: columns in
//! power-flow order, declared `group`s kept contiguous, Brandes–Köpf
//! coordinates keeping a cluster's neighbours adjacent. All of that is worth
//! keeping, and none of it is compact — column packing leaves a tall dead
//! notch beside every short column, and a board with one wide MCU column
//! pays that width in every row.
//!
//! So the arrangement is not replaced, it is *compacted*: each cluster is
//! reduced to its bounding rectangle, those rectangles are repacked with
//! [`crate::maxrects`], and every component is then translated rigidly to
//! follow its cluster. Translating whole clusters is the important part —
//! the internal arrangement a cluster was given (an LDO's decoupling caps
//! hugging its regulator) is what makes the drawing readable, and a packer
//! that repositioned individual components would throw that away.
//!
//! [`compact`] only returns an arrangement that is genuinely tighter than
//! the one it was given, so a board that is already compact is untouched and
//! the caller can adopt the result without comparing scores.

use synth_ir::Board;

use crate::maxrects::{Heuristic, MaxRectsBin, Rect};
use crate::{ComponentId, ComponentPlacement, SheetSize};

/// Grid the packer snaps to, matching the rest of the placer.
const GRID: f64 = 2.54;
/// Padding added to each side of a cluster's box before packing, so two
/// clusters placed flush end up [`crate::INTRA_GROUP_CLUSTER_DX`] apart.
///
/// This is the single most important constant in the file. The semantic
/// placer keeps a real gap between columns so that a cluster's
/// Reference/Value text — which is wider than its symbol body — cannot
/// reach into the next column, and because four symbols side by side at
/// their bare body width are unreadable. Packing against a small pad
/// reproduces the exact thing the packer is supposed to fix: a
/// twenty-millimetre row of four components in the corner of an A4 sheet.
/// Half the minimum column pitch, on each side, makes contact impossible.
const CLUSTER_PAD: f64 = crate::INTRA_GROUP_CLUSTER_DX / 2.0;

/// A cluster reduced to the rectangle the packer moves, plus the placements
/// that travel with it.
#[derive(Debug, Clone)]
struct ClusterBox {
    /// Indices into the incoming `placements`, ascending.
    members: Vec<usize>,
    rect: Rect,
    /// The cluster holds a component carrying a `placement_hint`, so its
    /// position is the author's statement and the packer must not touch it.
    ///
    /// A hint is an explicit instruction — `near: U3 priority: hard` says
    /// "this capacitor belongs beside that part" — and a packer that
    /// overrides one is discarding the designer's intent while reporting a
    /// tidier sheet. Such clusters are reserved in the bin at their existing
    /// position and everything else is packed around them.
    pinned: bool,
}

/// Repack `placements` with MaxRects, or `None` to keep them as they are.
///
/// `None` means the semantic arrangement already wins — either nothing fit,
/// or no packing was tighter. Nothing worse is ever returned.
/// Whether the semantic power-flow layering survives a candidate
/// arrangement.
///
/// [`crate::layer_for`] assigns every component a reading-order column
/// — connectors, then regulators, then the MCU, then passives — and
/// `place_clusters` lays each layer out as a band with the bands
/// ordered left to right. Compaction may *reshape* those bands (tighten
/// their width, re-stack parts inside one) but it must never merge two
/// of them: a connector sharing a column with the regulator it feeds
/// reads as one block, and the left-to-right power flow that the
/// layering exists to show is gone.
///
/// The bands must therefore stay disjoint and in order.
///
/// The predicate is unit-tested below. That it is actually consulted on
/// the compaction path is covered by the two integration gates that
/// failed when this guard was missing:
/// `semantic_placement::power_flow_layer_assignment_still_holds` and
/// `scorer_gate::scorer_matches_checked_in_baselines_and_crossing_gate`.
/// A small in-crate fixture is not enough — the packer only merges bands
/// once a board is big enough for the merge to pay off on area.
fn layers_preserved(board: &Board, placements: &[ComponentPlacement]) -> bool {
    let mut bands: std::collections::BTreeMap<u32, (f64, f64)> = std::collections::BTreeMap::new();
    for placement in placements {
        let Some(component) = board.component(placement.id) else {
            continue;
        };
        bands
            .entry(crate::layer_for(component))
            .and_modify(|band| {
                band.0 = band.0.min(placement.center_mm.0);
                band.1 = band.1.max(placement.center_mm.0);
            })
            .or_insert((placement.center_mm.0, placement.center_mm.0));
    }
    bands
        .values()
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| pair[0].1 < pair[1].0)
}

pub(crate) fn compact(
    board: &Board,
    placements: &[ComponentPlacement],
) -> Option<Vec<ComponentPlacement>> {
    let boxes = cluster_boxes(board, placements);
    // One cluster cannot be compacted by moving it, and the packer's whole
    // value is in fitting several together.
    if boxes.len() < 2 {
        return None;
    }
    let original_key = page_key(board, placements);
    let original_aspect = content_aspect(board, placements);

    // Smallest bin first: the tightest bin that swallows every cluster is
    // the most compact arrangement available, and starting small is what
    // makes this a compaction rather than a shuffle.
    let mut best: Option<((f64, f64, f64), Vec<ComponentPlacement>)> = None;
    for bin in candidate_bins(bounds_of_boxes(&boxes)) {
        for heuristic in Heuristic::ALL {
            let Some(arrangement) = try_pack(board, placements, &boxes, bin, heuristic) else {
                continue;
            };
            let key = page_key(board, &arrangement);
            // Strictly better than the arrangement we were handed. Anything
            // equal or worse is discarded, so compaction can never make a
            // sheet worse — it only ever removes dead space.
            if key >= original_key {
                continue;
            }
            // A guard the ranking cannot express. Ranking on area alone has a
            // systematic bias: a bin that is small in one axis fits everything
            // and produces a tall thin stripe, which wins on area and loses
            // on the page. A 133x48 mm drawing became a 20x154 mm column —
            // half the sheet area, and a stripe down one side of an A4. The
            // packer is allowed to reshape, just not to squash.
            let aspect = content_aspect(board, &arrangement);
            if original_aspect > 0.0
                && aspect > 0.0
                && (aspect / original_aspect).min(original_aspect / aspect) < MIN_ASPECT_RETENTION
            {
                continue;
            }
            // A guard for the property the ranking cannot see: the
            // semantic power-flow layering. Ranking on area has no reason
            // to keep a connector in its own column, so without this the
            // packer happily merges adjacent layers and the sheet stops
            // reading left to right.
            if !layers_preserved(board, &arrangement) {
                continue;
            }
            if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
                best = Some((key, arrangement));
            }
        }
    }
    best.map(|(_, arrangement)| arrangement)
}

/// How far an arrangement's aspect may drift from the one it replaces,
/// as a ratio.
///
/// 0.6 means the new drawing must keep at least 60% of the original's
/// proportions in whichever direction it moves.
const MIN_ASPECT_RETENTION: f64 = 0.6;

/// Aspect ratio (w/h) of the drawn bodies.
fn content_aspect(board: &Board, placements: &[ComponentPlacement]) -> f64 {
    let b = bounds_of_placements(board, placements);
    let (w, h) = (b.1 - b.0, b.3 - b.2);
    if h > 0.0 {
        w / h
    } else {
        0.0
    }
}

/// Would this packing keep each declared group contiguous?
///
/// A group is contiguous when no cluster that does not belong to it sits
/// inside the area its own clusters occupy. MaxRects has no notion of
/// groups and will happily drop one group's cluster inside another's box,
/// which the renderer then titles with parts that do not belong to it
/// (`E-SYNTH-SCHEM-013`, "component outside its group region").
///
/// Clusters carry [`CLUSTER_PAD`] on every side, so this tests the padded
/// rectangles and is stricter than the renderer's body-level test — the
/// right direction for a guard.
///
/// A cluster whose members declare *different* groups is treated as
/// belonging to none: guessing a group for it would be worse than letting
/// the caller's own check decide.
fn keeps_groups_contiguous(
    board: &Board,
    placements: &[ComponentPlacement],
    boxes: &[ClusterBox],
    target: &[Option<Rect>],
) -> bool {
    let group_of_cluster = |b: &ClusterBox| -> Option<String> {
        let mut names: Vec<String> = b
            .members
            .iter()
            .filter_map(|&i| {
                let id = placements.get(i)?.id;
                crate::effective_group(board, id).map(str::to_string)
            })
            .collect();
        names.sort();
        names.dedup();
        if names.len() == 1 {
            names.pop()
        } else {
            None
        }
    };

    let placed: Vec<(Option<String>, Rect)> = boxes
        .iter()
        .zip(target)
        .filter_map(|(b, t)| t.as_ref().map(|r| (group_of_cluster(b), *r)))
        .collect();

    // Union of each group's own clusters.
    let mut regions: Vec<(String, (f64, f64, f64, f64))> = Vec::new();
    for (group, rect) in &placed {
        let Some(group) = group else { continue };
        regions.push((
            group.clone(),
            (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h),
        ));
    }

    for (group, rect) in &placed {
        let Some(group) = group else { continue };
        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
        for (other, region) in &regions {
            if other == group {
                continue;
            }
            let disjoint = x1 <= region.0 || region.2 <= x0 || y1 <= region.1 || region.3 <= y0;
            if disjoint {
                continue;
            }
            return false;
        }
    }
    true
}

/// Rank an arrangement, lower being better: the page it needs, then how much
/// of that page the drawing covers, then how well its aspect matches.
///
/// The ordering matters, and getting it wrong fails in both directions. Page
/// first, because dropping a sheet is the win an author actually sees.
/// Content area second, so compaction is the tie-break among arrangements
/// that fit the same page — and so an aspect "improvement" can never buy a
/// *larger* drawing: a 211x116 mm arrangement is worse than a 198x114 mm one
/// on the same A4 even though it happens to be squarer. Aspect last, purely
/// to choose between two arrangements of identical size, where the one that
/// sits better on the page is the nicer sheet.
fn page_key(board: &Board, placements: &[ComponentPlacement]) -> (f64, f64, f64) {
    let b = bounds_of_placements(board, placements);
    let (need_w, need_h) = crate::sheet_needs(b.0, b.1, b.2, b.3);
    let page = crate::sheet_size_for(need_w, need_h);
    let (page_w, page_h) = page.dims_mm();
    let mismatch = if need_h > 0.0 && page_h > 0.0 {
        (need_w / need_h - page_w / page_h).abs()
    } else {
        f64::INFINITY
    };
    (page_w * page_h, need_w * need_h, mismatch)
}

fn bounds_of_boxes(boxes: &[ClusterBox]) -> (f64, f64, f64, f64) {
    let mut out = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for b in boxes {
        out.0 = out.0.min(b.rect.x);
        out.1 = out.1.max(b.rect.x + b.rect.w);
        out.2 = out.2.min(b.rect.y);
        out.3 = out.3.max(b.rect.y + b.rect.h);
    }
    out
}

fn bounds_of_placements(board: &Board, placements: &[ComponentPlacement]) -> (f64, f64, f64, f64) {
    let mut out = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in placements {
        let (w, h) = reserved_size(board, p.id);
        out.0 = out.0.min(p.center_mm.0 - w / 2.0);
        out.1 = out.1.max(p.center_mm.0 + w / 2.0);
        out.2 = out.2.min(p.center_mm.1 - h / 2.0);
        out.3 = out.3.max(p.center_mm.1 + h / 2.0);
    }
    out
}

/// The room a component actually needs on the sheet, in each axis.
///
///
/// This must be the *text-inclusive* extent, not [`body_size`]. The placer
/// sizes its columns from these same helpers
/// ([`crate::component_text_inclusive_half_width`] and
/// [`crate::text_inclusive_half_height`]) because a part's Reference/Value
/// text is wider and taller than its symbol body, and the router marks the
/// same extent as an obstacle. Sizing a packable box from the bare body
/// instead is what let a 43-pin module be packed flush to the page margin
/// and then render half off the left edge.
fn reserved_size(board: &Board, id: ComponentId) -> (f64, f64) {
    (
        2.0 * crate::component_text_inclusive_half_width(board, id),
        2.0 * crate::text_inclusive_half_height(board, id),
    )
}

/// Reduce every cluster to a rectangle, with its members' placement indices.
fn cluster_boxes(board: &Board, placements: &[ComponentPlacement]) -> Vec<ClusterBox> {
    let clusters = crate::build_clusters(board);
    let index_of: std::collections::BTreeMap<ComponentId, usize> = placements
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();

    let mut boxes = Vec::with_capacity(clusters.len());
    for cluster in &clusters {
        let mut members: Vec<usize> = cluster
            .members
            .iter()
            .filter_map(|m| index_of.get(&m.id).copied())
            .collect();
        // The anchor is placed alongside its members but is not always
        // listed among them, so it joins the box explicitly.
        if let Some(&a) = index_of.get(&cluster.anchor) {
            members.push(a);
        }
        // A member claimed by two patterns would be translated twice.
        members.sort_unstable();
        members.dedup();
        if members.is_empty() {
            continue;
        }
        let Some(rect) = members_rect(board, placements, &members) else {
            continue;
        };
        let pinned = members
            .iter()
            .filter_map(|&i| placements.get(i))
            .filter_map(|p| board.component(p.id))
            .any(|c| c.placement_hint.is_some());
        boxes.push(ClusterBox {
            members,
            rect,
            pinned,
        });
    }
    boxes
}

/// The padded, grid-aligned rectangle covering `members`.
fn members_rect(
    board: &Board,
    placements: &[ComponentPlacement],
    members: &[usize],
) -> Option<Rect> {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &i in members {
        let p = &placements[i];
        let (w, h) = reserved_size(board, p.id);
        min_x = min_x.min(p.center_mm.0 - w / 2.0);
        max_x = max_x.max(p.center_mm.0 + w / 2.0);
        min_y = min_y.min(p.center_mm.1 - h / 2.0);
        max_y = max_y.max(p.center_mm.1 + h / 2.0);
    }
    if !min_x.is_finite() {
        return None;
    }
    // Round *outward*: rounding inward would clip the very text the padding
    // exists to keep clear of the neighbouring cluster.
    let x = snap_down(min_x - CLUSTER_PAD);
    let y = snap_down(min_y - CLUSTER_PAD);
    let right = snap_up(max_x + CLUSTER_PAD);
    let bottom = snap_up(max_y + CLUSTER_PAD);
    Some(Rect::new(x, y, right - x, bottom - y))
}

fn snap_down(v: f64) -> f64 {
    (v / GRID).floor() * GRID
}

fn snap_up(v: f64) -> f64 {
    (v / GRID).ceil() * GRID
}

fn snap_to_grid(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

/// Bin sizes to try, smallest area first.
///
/// Both the content's own box and the standard page shapes are tried: the
/// former for a modest tightening, the latter because a tight enough
/// packing can drop the design onto a smaller sheet, which is the win an
/// author actually sees.
fn candidate_bins(original: (f64, f64, f64, f64)) -> Vec<(f64, f64)> {
    let (w, h) = (original.1 - original.0, original.3 - original.2);
    let mut bins: Vec<(f64, f64)> = Vec::new();
    // Some slack matters: a bin exactly the size of the content leaves the
    // packer no freedom, so it can only reproduce the arrangement it was
    // handed and cannot compact anything.
    for pad in [GRID, 3.0 * GRID, 6.0 * GRID] {
        bins.push((snap_up(w + pad), snap_up(h + pad)));
    }
    for size in [
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
    ] {
        let (pw, ph) = size.dims_mm();
        let usable = (pw - 2.0 * crate::PAGE_MARGIN, ph - 2.0 * crate::PAGE_MARGIN);
        // A bin smaller than the content is not a tighter packing, it is a
        // squeeze: MaxRects would simply refuse items it cannot hold, so
        // this can only ever fail. Skipping it keeps the search honest and
        // makes the intent ("rearrange, do not compress") checkable.
        if usable.0 + 1e-6 >= w && usable.1 + 1e-6 >= h {
            bins.push(usable);
        }
    }
    bins.sort_by(|a, b| {
        (a.0 * a.1)
            .partial_cmp(&(b.0 * b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    });
    bins.dedup();
    bins
}

/// Pack every cluster box into `bin`, or `None` if one does not fit.
fn try_pack(
    board: &Board,
    originals: &[ComponentPlacement],
    boxes: &[ClusterBox],
    bin: (f64, f64),
    heuristic: Heuristic,
) -> Option<Vec<ComponentPlacement>> {
    let mut packer = MaxRectsBin::new(bin.0, bin.1);
    // Inserted in the placer's own left-to-right order, so the packer
    // prefers to keep neighbours together. The reverse order is not tried:
    // it yields the same area with the reading order inverted, which is
    // never an improvement.
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| {
        boxes[a]
            .rect
            .x
            .partial_cmp(&boxes[b].rect.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                boxes[a]
                    .rect
                    .y
                    .partial_cmp(&boxes[b].rect.y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    // Pinned clusters are reserved where the author put them; the bin is
    // then only free to arrange the rest around them.
    let mut target: Vec<Option<Rect>> = vec![None; boxes.len()];
    for (i, b) in boxes.iter().enumerate() {
        if b.pinned {
            packer.pre_place(i, b.rect)?;
            target[i] = Some(b.rect);
        }
    }
    for &i in &order {
        if boxes[i].pinned {
            continue;
        }
        let r = boxes[i].rect;
        target[i] = Some(packer.insert(i, r.w, r.h, heuristic)?);
    }

    // MaxRects has no notion of declared groups and will drop one group's
    // cluster inside another's box, which the renderer then titles with
    // parts that do not belong to it. Refuse the packing rather than
    // reorder around it.
    if !keeps_groups_contiguous(board, originals, boxes, &target) {
        return None;
    }

    // Translate each cluster rigidly. `members_rect` pads the box by
    // CLUSTER_PAD, so shifting by the box-centre difference moves the whole
    // reserved box and the members keep their positions inside it.
    let mut out: Vec<ComponentPlacement> =
        Vec::with_capacity(boxes.iter().map(|b| b.members.len()).sum());
    for (bi, b) in boxes.iter().enumerate() {
        let at = target[bi]?;
        let (dx, dy) = if b.pinned {
            (0.0, 0.0)
        } else {
            let from_x = b.rect.x + b.rect.w / 2.0;
            let from_y = b.rect.y + b.rect.h / 2.0;
            (
                snap_to_grid(at.x + at.w / 2.0 - from_x),
                snap_to_grid(at.y + at.h / 2.0 - from_y),
            )
        };
        for &i in &b.members {
            let src = originals.get(i)?;
            out.push(ComponentPlacement {
                id: src.id,
                center_mm: (
                    snap_to_grid(src.center_mm.0 + dx),
                    snap_to_grid(src.center_mm.1 + dy),
                ),
                rotation: src.rotation,
            });
        }
    }
    // Anchor the drawing to the page margin.
    //
    // The packer fills its bin from (0,0), which is the top-left corner of
    // the sheet rather than its first usable millimetre, so without this the
    // design sits on the paper edge. The anchor is taken from the *box*
    // extents, not the component centres: a box is wider than the component
    // inside it, so anchoring on centres lands the leftmost box short of the
    // margin and can push it off the page entirely.
    //
    // It also keeps the ranking honest. `page_key` measures absolute bounds,
    // so an arrangement that merely slid left would otherwise look like a
    // large improvement while covering exactly as much sheet.
    //
    // Skipped when anything is pinned: a pinned cluster is already where the
    // semantic placer put it, margin included, and shifting the result to
    // re-anchor the margin would drag that pinned cluster off the position
    // the author asked for.
    if !boxes.iter().any(|b| b.pinned) {
        let min_x = target
            .iter()
            .flatten()
            .map(|r| r.x)
            .fold(f64::INFINITY, f64::min);
        let min_y = target
            .iter()
            .flatten()
            .map(|r| r.y)
            .fold(f64::INFINITY, f64::min);
        let dx = snap_to_grid(crate::PAGE_MARGIN - min_x);
        let dy = snap_to_grid(crate::PAGE_MARGIN - min_y);
        if dx != 0.0 || dy != 0.0 {
            for p in &mut out {
                p.center_mm.0 += dx;
                p.center_mm.1 += dy;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board spanning several semantic layers, so the layering guard
    /// has something to lose: connector (0), regulator (1), and enough
    /// passives to give the packer room to be tempted.
    fn multilayer_board() -> Board {
        let src = r#"board "t" {
  layers 2
  component J1: connector "jst_ph_4pin"
  component J2: connector "jst_ph_4pin"
  component U1: regulator "ldo_3v3"
  component R1: resistor "r_generic_0805"
  component R2: resistor "r_generic_0805"
  component R3: resistor "r_generic_0805"
  component C1: capacitor "c_generic_0805" value "100nF"
  component C2: capacitor "c_generic_0805" value "100nF"
  component C3: capacitor "c_generic_0805" value "100nF"
}"#;
        let ast = synth_parser::parse(src, "t.synth").ast.expect("ast");
        let reg = synth_registry::load_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("registry")
                .join("parts"),
        )
        .expect("seed registry");
        synth_ir::lower(&ast, &reg, "t.synth").board.expect("board")
    }

    /// A hand-built arrangement that merges two layers is rejected by the
    /// guard, so the test above cannot pass merely because the packer
    /// happened not to produce one.
    ///
    /// Needs parts that actually land in *different* layers: a connector
    /// (layer 0) and a regulator (layer 1). Passives alone would all share
    /// one layer, where sharing a column is the layout working as intended.
    #[test]
    fn layers_preserved_rejects_merged_bands() {
        let board = multilayer_board();

        // Guard precondition: these parts really do span several layers.
        let layers: std::collections::BTreeSet<u32> =
            board.components.iter().map(crate::layer_for).collect();
        assert!(layers.len() > 1, "fixture must span several layers");

        let same_column: Vec<ComponentPlacement> = board
            .components
            .iter()
            .enumerate()
            .map(|(i, c)| ComponentPlacement {
                id: c.id,
                // One shared column, whatever the layer.
                center_mm: (50.0, 50.0 + i as f64 * 25.0),
                rotation: crate::Rotation::Zero,
            })
            .collect();
        assert!(
            !layers_preserved(&board, &same_column),
            "a single shared column must fail the layering guard"
        );

        // …and the same parts, spread into ordered columns, must pass.
        let mut ordered = same_column.clone();
        let mut x = 25.0;
        for placement in &mut ordered {
            let layer = crate::layer_for(board.component(placement.id).expect("component"));
            let slot = (layer * 100) as f64;
            if slot >= x {
                x = slot + 25.0;
            }
            placement.center_mm.0 = x;
            x += 25.0;
        }
        assert!(
            layers_preserved(&board, &ordered),
            "disjoint ordered columns must satisfy the layering guard"
        );
    }

    fn board_with(parts: usize) -> Board {
        // A board of unrelated passives: `build_clusters` claims each as its
        // own singleton, which is the case compaction has to handle.
        let mut src = String::from("board \"t\" {\n  layers 2\n");
        for i in 0..parts {
            use std::fmt::Write as _;
            let _ = writeln!(src, "  component R{i}: resistor \"r_generic_0805\"");
            let _ = writeln!(
                src,
                "  component C{i}: capacitor \"c_generic_0805\" value \"100nF\""
            );
        }
        let p = synth_parser::parse(&src, "t.synth");
        let ast = p.ast.expect("ast");
        // Unit tests run with the crate directory as CWD, so the seed
        // registry is two levels up rather than one.
        let reg = synth_registry::load_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("registry")
                .join("parts"),
        )
        .expect("seed registry");
        synth_ir::lower(&ast, &reg, "t.synth").board.expect("board")
    }

    /// A roomy grid, the way the semantic placer leaves a board before
    /// compaction runs. Spaced well past the minimum pitch, so there is
    /// genuinely slack for the packer to remove.
    fn column_layout(board: &Board) -> Vec<ComponentPlacement> {
        board
            .components
            .iter()
            .enumerate()
            .map(|(i, c)| ComponentPlacement {
                id: c.id,
                center_mm: (
                    20.0 * GRID + (i % 2) as f64 * 32.0 * GRID,
                    20.0 * GRID + (i / 2) as f64 * 32.0 * GRID,
                ),
                rotation: crate::Rotation::Zero,
            })
            .collect()
    }

    fn area_of(board: &Board, p: &[ComponentPlacement]) -> f64 {
        let b = bounds_of_placements(board, p);
        (b.1 - b.0) * (b.3 - b.2)
    }

    #[test]
    fn compaction_never_returns_a_larger_drawing() {
        // The safety property the whole pass rests on: whatever comes back
        // must beat the input on (page, area, aspect), so a board that was
        // already good comes through unchanged.
        for parts in [2usize, 4, 8] {
            let board = board_with(parts);
            let before = column_layout(&board);
            if let Some(after) = compact(&board, &before) {
                let (ba, aa) = (area_of(&board, &before), area_of(&board, &after));
                assert!(
                    aa <= ba + 1e-6,
                    "{parts} pairs: compaction grew the drawing: {ba:.1} -> {aa:.1} (before bounds {:?}, after {:?})",
                    bounds_of_placements(&board, &before), bounds_of_placements(&board, &after)
                );
                assert_eq!(
                    after.len(),
                    before.len(),
                    "compaction must not lose or duplicate a component"
                );
            }
        }
    }

    #[test]
    fn every_component_survives_and_keeps_its_identity() {
        let board = board_with(6);
        let before = column_layout(&board);
        if let Some(after) = compact(&board, &before) {
            let mut ids: Vec<u32> = after.iter().map(|p| p.id.0).collect();
            ids.sort_unstable();
            let mut want: Vec<u32> = before.iter().map(|p| p.id.0).collect();
            want.sort_unstable();
            assert_eq!(ids, want, "compaction must be a permutation");
        }
    }

    #[test]
    fn compaction_is_deterministic() {
        // Every export in the golden suite is byte-compared, so a
        // non-deterministic packer would fail CI intermittently rather than
        // obviously.
        let board = board_with(8);
        let before = column_layout(&board);
        let first = compact(&board, &before);
        for _ in 0..5 {
            assert_eq!(
                first
                    .as_ref()
                    .map(|p| p.iter().map(|q| q.center_mm).collect::<Vec<_>>()),
                compact(&board, &before)
                    .as_ref()
                    .map(|p| p.iter().map(|q| q.center_mm).collect::<Vec<_>>()),
                "compaction must be reproducible"
            );
        }
    }

    #[test]
    fn a_single_cluster_is_left_alone() {
        // One lone component: there is nothing to pack it against, so the
        // pass must decline rather than shuffle it.
        let src = "board \"t\" {\n  layers 2\n  component R0: resistor \"r_generic_0805\"\n}";
        let ast = synth_parser::parse(src, "t.synth").ast.expect("ast");
        let reg = synth_registry::load_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("registry")
                .join("parts"),
        )
        .expect("seed registry");
        let board = synth_ir::lower(&ast, &reg, "t.synth").board.expect("board");
        assert!(compact(&board, &column_layout(&board)).is_none());
    }

    #[test]
    fn packed_clusters_reserve_the_placers_minimum_pitch() {
        // Every cluster box carries CLUSTER_PAD of clearance on each side, so
        // two boxes placed flush leave that much between their *contents*.
        // Without the padding MaxRects produced a 20 mm row of four
        // components in the corner of an A4 sheet — exactly the unreadable
        // outcome the semantic placer exists to avoid.
        let board = board_with(8);
        let before = column_layout(&board);
        let Some(after) = compact(&board, &before) else {
            return;
        };
        let boxes = cluster_boxes(&board, &after);
        for b in &boxes {
            assert!(
                b.rect.w >= 2.0 * CLUSTER_PAD,
                "cluster box {:?} is not padded on both sides",
                b.rect
            );
        }
        for i in 0..boxes.len() {
            for j in (i + 1)..boxes.len() {
                let (a, b) = (boxes[i].rect, boxes[j].rect);
                let clear_x = (b.x - (a.x + a.w)).max(a.x - (b.x + b.w));
                let clear_y = (b.y - (a.y + a.h)).max(a.y - (b.y + b.h));
                assert!(
                    clear_x.max(clear_y) >= -1e-6,
                    "clusters {i} and {j} overlap after packing"
                );
            }
        }
    }

    #[test]
    fn a_placement_hint_pins_its_cluster() {
        // `placement_hint { near: U3 priority: hard }` is the author saying
        // "this part belongs beside that one". A packer that overrides it
        // discards the designer's intent while reporting a tidier sheet, and
        // the renderer then finds a component inside a region it does not
        // belong to (E-SYNTH-SCHEM-013).
        let src = "board \"t\" {\n  layers 2\n  group \"Sensors\" {\n    \
                   component U3: sensor \"bmp280_full\"\n    \
                   component C8: capacitor \"c_generic_0805\" value \"100nF\" \
                   { placement_hint { near: U3 priority: hard } }\n    \
                   component R1: resistor \"r_generic_0805\"\n    \
                   component R2: resistor \"r_generic_0805\"\n  }\n}";
        let ast = synth_parser::parse(src, "t.synth").ast.expect("ast");
        let reg = synth_registry::load_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("registry")
                .join("parts"),
        )
        .expect("seed registry");
        let board = synth_ir::lower(&ast, &reg, "t.synth").board.expect("board");

        let hinted = board
            .components
            .iter()
            .find(|c| c.placement_hint.is_some())
            .expect("the fixture must contain a hinted component")
            .id;
        let before = column_layout(&board);
        let before_pos = before
            .iter()
            .find(|p| p.id == hinted)
            .expect("hinted component is placed")
            .center_mm;

        let after = compact(&board, &before).expect("compaction should still run");
        let after_pos = after
            .iter()
            .find(|p| p.id == hinted)
            .expect("hinted component survives")
            .center_mm;
        assert!(
            (before_pos.0 - after_pos.0).abs() < 1e-6 && (before_pos.1 - after_pos.1).abs() < 1e-6,
            "a component with a placement_hint must not move: {before_pos:?} -> \
             {after_pos:?}"
        );
    }

    #[test]
    fn a_packed_drawing_starts_at_the_page_margin() {
        // The packer fills its bin from (0,0) — the paper edge — so the pass
        // has to anchor the result or the whole design sits on the trim.
        //
        // The assertion is on the cluster *boxes*, not on the drawn content:
        // a box carries CLUSTER_PAD of clearance, and that clearance is the
        // margin, so the component inside it legitimately starts left of
        // PAGE_MARGIN.
        let board = board_with(8);
        let before = column_layout(&board);
        let Some(after) = compact(&board, &before) else {
            return;
        };
        for b in cluster_boxes(&board, &after) {
            assert!(
                b.rect.x >= crate::PAGE_MARGIN - 1e-6,
                "cluster box starts at x={:.2}, inside the {}mm margin",
                b.rect.x,
                crate::PAGE_MARGIN
            );
        }
    }
}
