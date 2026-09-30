// SPDX-License-Identifier: Apache-2.0

//! MaxRects rectangle packing (Jylänki), used to compact placed clusters.
//!
//! # Why this exists alongside the shelf packer
//!
//! The semantic placer (`place_clusters`) arranges clusters in columns, because the
//! left-to-right reading order carries meaning: power flows left to right,
//! a declared `group` stays contiguous under its caption, and Brandes–Köpf
//! coordinates keep a cluster's neighbours adjacent. That is a *semantic*
//! arrangement and it is not a compact one — column packing leaves a tall
//! notch of dead space beside any short column, and a board with one wide
//! MCU column and several narrow passive columns pays for the widest in
//! every row.
//!
//! MaxRects packs the same cluster rectangles far more tightly, but it knows
//! nothing about power flow, groups, or net length. So it is applied as a
//! *compaction* of the semantic arrangement rather than as a replacement for
//! it, and the existing scorer decides which of the two to keep. See
//! `MaxRectsBin` for the invariant that makes that safe.
//!
//! # The algorithm
//!
//! The bin holds a list of *maximal* free rectangles. Inserting a rectangle
//! picks the free rectangle that fits it best, places the item at that free
//! rectangle's bottom-left corner, then splits every free rectangle the new
//! item overlaps, and finally drops any free rectangle now contained in
//! another. Keeping the list maximal is what makes the result tight: a
//! rectangle strictly inside another can never be the best fit for anything,
//! so discarding it loses nothing and keeps the list from growing without
//! bound.

use std::fmt;

/// An axis-aligned rectangle in millimetres, top-left origin, matching the
/// sheet coordinate space used everywhere else in the layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub(crate) fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    #[cfg(test)]
    pub(crate) fn max_x(&self) -> f64 {
        self.x + self.w
    }

    #[cfg(test)]
    pub(crate) fn max_y(&self) -> f64 {
        self.y + self.h
    }

    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Do `a` and `b` share any interior area?
    ///
    /// Edge-touching rectangles are **not** overlapping: two clusters may sit
    /// flush against each other as long as they do not interpenetrate. A
    /// strict `<` on both axes is what makes the packing tile-able, and
    /// treating contact as a collision would reject every flush fit and drive
    /// the packer towards leaving a gap.
    fn overlaps(&self, other: &Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// Is `self` wholly inside `other`?
    fn contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

/// How to choose among the free rectangles that can hold an item.
/// Ordering key for a candidate free rectangle: lower is a better fit.
type FitKey = (f64, f64, f64, f64, f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Heuristic {
    /// Best Short Side Fit: minimise the leftover on the tighter axis, then
    /// on the looser one. Tends to leave few, large, useful gaps, which is
    /// what a sheet that still has to fit a title block wants.
    ShortSide,
    /// Best Area Fit: minimise the wasted area of the free rectangle. Places
    /// small items into small holes, and leaves the big rectangles intact
    /// for the big items.
    Area,
    /// Best Long Side Fit: the mirror of [`Heuristic::ShortSide`]. Included
    /// because it sometimes wins on wide, letterbox-ish boards where the two
    /// disagree; the scorer picks the outcome, not this function.
    LongSide,
}

impl Heuristic {
    pub(crate) const ALL: [Heuristic; 3] =
        [Heuristic::ShortSide, Heuristic::Area, Heuristic::LongSide];

    /// Sort key for the candidate free rectangles, lower is better.
    ///
    /// A named type rather than a five-tuple: the components are read at the
    /// one call site that builds it and nowhere else, and a bare tuple there
    /// is five unexplained `f64`s.
    ///
    /// The final components are a strict tie-break on position, not a loose
    /// one: without them the packer is free to pick any of several equally
    /// good free rectangles, and which one it picked would depend on the
    /// order the list happened to be in.
    fn key(self, free: Rect, item_w: f64, item_h: f64) -> FitKey {
        let leftover_w = free.w - item_w;
        let leftover_h = free.h - item_h;
        match self {
            Heuristic::ShortSide => (
                leftover_w.min(leftover_h),
                leftover_w.max(leftover_h),
                free.w * free.h,
                free.y,
                free.x,
            ),
            Heuristic::Area => (
                free.w * free.h,
                leftover_w.min(leftover_h),
                free.w,
                free.y,
                free.x,
            ),
            Heuristic::LongSide => (
                leftover_w.max(leftover_h),
                leftover_w.min(leftover_h),
                free.w * free.h,
                free.y,
                free.x,
            ),
        }
    }
}

/// A MaxRects bin: items are inserted one at a time, each into the best
/// fitting free rectangle.
///
/// The bin never grows. An item that does not fit returns `None` and the
/// caller is expected to try a bigger bin, which is what makes compaction
/// safe to attempt speculatively — a failed attempt is simply discarded, and
/// nothing already placed is disturbed.
#[derive(Debug, Clone)]
pub(crate) struct MaxRectsBin {
    free: Vec<Rect>,
    placed: Vec<(usize, Rect)>,
}

impl MaxRectsBin {
    pub(crate) fn new(width: f64, height: f64) -> Self {
        Self {
            free: vec![Rect::new(0.0, 0.0, width, height)],
            placed: Vec::new(),
        }
    }

    /// Every rectangle placed so far, in insertion order.
    #[cfg(test)]
    pub(crate) fn placed(&self) -> &[(usize, Rect)] {
        &self.placed
    }

    /// Number of maximal free rectangles left. Exposed for tests: it is the
    /// quantity the prune pass exists to keep bounded.
    #[cfg(test)]
    pub(crate) fn free_rect_count(&self) -> usize {
        self.free.len()
    }

    /// Reserve `rect` without packing it, splitting the free list around it.
    ///
    /// For an item whose position is already decided — a component carrying
    /// a `placement_hint`, say, which is the author's statement and not the
    /// packer's to overrule. The item still has to fit somewhere in the bin,
    /// so a reservation that does not fit returns `None`.
    pub(crate) fn pre_place(&mut self, item: usize, rect: Rect) -> Option<()> {
        if !self.free.iter().any(|f| {
            rect.x >= f.x
                && rect.y >= f.y
                && rect.x + rect.w <= f.right()
                && rect.y + rect.h <= f.bottom()
        }) {
            return None;
        }
        let mut next: Vec<Rect> = Vec::with_capacity(self.free.len() + 4);
        for free in self.free.drain(..) {
            if !free.overlaps(&rect) {
                next.push(free);
                continue;
            }
            if rect.x > free.x {
                next.push(Rect::new(free.x, free.y, rect.x - free.x, free.h));
            }
            if rect.right() < free.right() {
                next.push(Rect::new(
                    rect.right(),
                    free.y,
                    free.right() - rect.right(),
                    free.h,
                ));
            }
            if rect.y > free.y {
                next.push(Rect::new(free.x, free.y, free.w, rect.y - free.y));
            }
            if rect.bottom() < free.bottom() {
                next.push(Rect::new(
                    free.x,
                    rect.bottom(),
                    free.w,
                    free.bottom() - rect.bottom(),
                ));
            }
        }
        self.free = prune_contained(next);
        self.placed.push((item, rect));
        Some(())
    }

    /// Place item `item` of size `item_w` x `item_h` at the bottom-left of
    /// the best free rectangle, or `None` if it does not fit anywhere.
    pub(crate) fn insert(
        &mut self,
        item: usize,
        item_w: f64,
        item_h: f64,
        heuristic: Heuristic,
    ) -> Option<Rect> {
        let mut best: Option<(usize, FitKey)> = None;
        for (i, free) in self.free.iter().enumerate() {
            if item_w > free.w || item_h > free.h {
                continue;
            }
            let key = heuristic.key(*free, item_w, item_h);
            if best.is_none_or(|(_, best_key)| key < best_key) {
                best = Some((i, key));
            }
        }
        let (index, _) = best?;
        let target = self.free[index];
        let placed = Rect::new(target.x, target.y, item_w, item_h);

        // Split every free rectangle the new item overlaps, then drop the
        // ones that are no longer maximal. Both passes are required: split
        // alone leaves rectangles that are strictly contained in others,
        // and pruning alone would drop holes the item actually needs.
        let mut next: Vec<Rect> = Vec::with_capacity(self.free.len() + 4);
        for free in self.free.drain(..) {
            if !free.overlaps(&placed) {
                next.push(free);
                continue;
            }
            // Left strip.
            if placed.x > free.x {
                next.push(Rect::new(free.x, free.y, placed.x - free.x, free.h));
            }
            // Right strip.
            if placed.right() < free.right() {
                next.push(Rect::new(
                    placed.right(),
                    free.y,
                    free.right() - placed.right(),
                    free.h,
                ));
            }
            // Top strip.
            if placed.y > free.y {
                next.push(Rect::new(free.x, free.y, free.w, placed.y - free.y));
            }
            // Bottom strip.
            if placed.bottom() < free.bottom() {
                next.push(Rect::new(
                    free.x,
                    placed.bottom(),
                    free.w,
                    free.bottom() - placed.bottom(),
                ));
            }
        }
        self.free = prune_contained(next);
        self.placed.push((item, placed));
        Some(placed)
    }
}

/// Drop every rectangle wholly inside another.
///
/// Both directions are compared, and the list is scanned in order, so when
/// two rectangles are equal the earlier one wins. That keeps the result
/// independent of how the rectangles arrived.
fn prune_contained(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.sort_by(|a, b| {
        (a.w * a.h)
            .partial_cmp(&(b.w * b.h))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut kept: Vec<Rect> = Vec::with_capacity(rects.len());
    for rect in rects {
        // Already covered by something kept, either because it is inside a
        // previously accepted rectangle or because it duplicates one.
        if kept.iter().any(|k| k.contains(&rect)) {
            continue;
        }
        // Conversely, if the new rectangle swallows something already kept,
        // that one was never maximal and should not have survived.
        let mut i = 0;
        while i < kept.len() {
            if rect.contains(&kept[i]) {
                kept.swap_remove(i);
            } else {
                i += 1;
            }
        }
        kept.push(rect);
    }
    // Restore a deterministic order for the next search pass.
    kept.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });
    kept
}

/// Which of two placements covers less area, used to pick a winner.
impl fmt::Display for Heuristic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Heuristic::ShortSide => "bssf",
            Heuristic::Area => "baf",
            Heuristic::LongSide => "blsf",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        a.overlaps(&b)
    }

    /// The invariant the whole compaction pass rests on: whatever the
    /// heuristic or the item order, nothing placed may overlap anything else
    /// already placed.
    fn assert_no_overlaps(placed: &[(usize, Rect)]) {
        for i in 0..placed.len() {
            for j in (i + 1)..placed.len() {
                assert!(
                    !rects_overlap(placed[i].1, placed[j].1),
                    "items {} and {} overlap: {:?} vs {:?}",
                    placed[i].0,
                    placed[j].0,
                    placed[i].1,
                    placed[j].1
                );
            }
        }
    }

    #[test]
    fn an_empty_bin_is_one_free_rectangle() {
        let bin = MaxRectsBin::new(100.0, 50.0);
        assert_eq!(bin.free_rect_count(), 1);
        assert!(bin.placed().is_empty());
    }

    #[test]
    fn an_oversized_item_is_refused_rather_than_clipped() {
        let mut bin = MaxRectsBin::new(100.0, 50.0);
        assert!(bin.insert(0, 101.0, 10.0, Heuristic::ShortSide).is_none());
        assert!(bin.insert(0, 10.0, 51.0, Heuristic::ShortSide).is_none());
        assert!(
            bin.placed().is_empty(),
            "a refused item must not be recorded"
        );
    }

    #[test]
    fn an_item_exactly_filling_the_bin_is_placed_flush() {
        let mut bin = MaxRectsBin::new(100.0, 50.0);
        let placed = bin.insert(0, 100.0, 50.0, Heuristic::ShortSide).unwrap();
        assert_eq!(placed, Rect::new(0.0, 0.0, 100.0, 50.0));
        assert_eq!(bin.free_rect_count(), 0, "the bin is now full");
        assert!(bin.insert(1, 1.0, 1.0, Heuristic::ShortSide).is_none());
    }

    #[test]
    fn items_tile_without_overlap_across_heuristics_and_orders() {
        let items = [
            (30.0, 20.0),
            (45.0, 12.0),
            (18.0, 35.0),
            (60.0, 9.0),
            (25.0, 25.0),
        ];
        for h in Heuristic::ALL {
            // A big bin, so the point is overlap-freedom rather than fitting.
            let mut bin = MaxRectsBin::new(120.0, 90.0);
            for (i, (w, hh)) in items.iter().enumerate() {
                assert!(
                    bin.insert(i, *w, *hh, h).is_some(),
                    "{h}: item {i} ({w}x{hh}) should fit a 120x90 bin"
                );
            }
            assert_no_overlaps(bin.placed());
        }
    }

    #[test]
    fn placed_rects_stay_inside_the_bin() {
        let items = [(40.0, 30.0), (25.0, 25.0), (55.0, 12.0)];
        let mut bin = MaxRectsBin::new(90.0, 70.0);
        for (i, (w, h)) in items.iter().enumerate() {
            bin.insert(i, *w, *h, Heuristic::ShortSide).unwrap();
        }
        for (i, r) in bin.placed() {
            assert!(r.x >= 0.0 && r.y >= 0.0, "item {i} starts off-bin: {r:?}");
            assert!(
                r.max_x() <= 90.0 + 1e-9 && r.max_y() <= 70.0 + 1e-9,
                "item {i} runs off-bin: {r:?}"
            );
        }
    }

    #[test]
    fn packing_is_deterministic_across_repeated_runs() {
        // The placer's output has to be byte-identical run to run, so this
        // is the property the whole crate depends on.
        let items = [
            (37.0, 21.0),
            (19.0, 44.0),
            (62.0, 15.0),
            (28.0, 28.0),
            (50.0, 11.0),
        ];
        let first: Vec<Rect> = {
            let mut bin = MaxRectsBin::new(140.0, 100.0);
            items
                .iter()
                .enumerate()
                .map(|(i, (w, h))| bin.insert(i, *w, *h, Heuristic::ShortSide).unwrap())
                .collect()
        };
        for _ in 0..8 {
            let mut bin = MaxRectsBin::new(140.0, 100.0);
            let again: Vec<Rect> = items
                .iter()
                .enumerate()
                .map(|(i, (w, h))| bin.insert(i, *w, *h, Heuristic::ShortSide).unwrap())
                .collect();
            assert_eq!(first, again, "MaxRects must be deterministic");
        }
    }

    #[test]
    fn the_free_list_stays_maximal() {
        // Pruning is what stops the free list growing without bound; a long
        // run of mixed sizes would be the pathological case.
        let mut bin = MaxRectsBin::new(200.0, 150.0);
        for i in 0..24u32 {
            let w = 7.0 + (i % 6) as f64 * 3.0;
            let h = 5.0 + (i % 5) as f64 * 4.0;
            if bin.insert(i as usize, w, h, Heuristic::Area).is_some() {
                assert!(
                    bin.free_rect_count() <= 40,
                    "free list grew to {} rectangles",
                    bin.free_rect_count()
                );
            }
        }
    }

    #[test]
    fn a_tie_between_free_rectangles_is_broken_by_position_not_by_list_order() {
        // After a 90x90 item the remainder is two strips, right and bottom,
        // that both hold a 10x10 item exactly. Every heuristic ties on the
        // leftover, so the decision falls to the y-then-x tie-break. If that
        // tie-break were dropped the packer would follow whatever order the
        // free list happened to be in, and the layout would stop being
        // reproducible.
        for h in Heuristic::ALL {
            let mut bin = MaxRectsBin::new(100.0, 100.0);
            bin.insert(0, 90.0, 90.0, h).unwrap();
            let at = bin.insert(1, 10.0, 10.0, h).unwrap();
            assert_eq!(at, Rect::new(90.0, 0.0, 10.0, 10.0), "{h}");
        }
    }

    #[test]
    fn a_free_rect_strictly_inside_another_is_pruned() {
        // Tested directly rather than through `insert`: constructing a split
        // that happens to produce an interior rectangle is incidental, while
        // the prune itself is the invariant that bounds the free list.
        let outer = Rect::new(0.0, 0.0, 100.0, 100.0);
        let inside = Rect::new(20.0, 20.0, 10.0, 10.0);
        // A genuine second maximal rectangle: it overlaps `outer` but is not
        // contained by it, so exactly one of the two can survive.
        let beside = Rect::new(90.0, 0.0, 40.0, 30.0);
        let kept = prune_contained(vec![inside, outer, beside]);
        assert!(
            !kept.contains(&inside),
            "an interior rectangle can never be the best fit and must go"
        );
        assert!(
            kept.contains(&outer),
            "the larger maximal rectangle survives"
        );
        assert!(kept.contains(&beside), "an uncontained rectangle survives");
        assert_eq!(kept.len(), 2, "one interior rectangle must be dropped");
    }

    #[test]
    fn pruning_keeps_the_larger_of_two_identical_rectangles_once() {
        // Duplicates are a degenerate form of containment: keeping both would
        // let the same hole be chosen twice and waste the space.
        let a = Rect::new(5.0, 5.0, 20.0, 20.0);
        let kept = prune_contained(vec![a, a]);
        assert_eq!(kept, vec![a]);
    }

    #[test]
    fn edge_contact_is_not_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(!rects_overlap(a, Rect::new(10.0, 0.0, 10.0, 10.0)));
        assert!(!rects_overlap(a, Rect::new(0.0, 10.0, 10.0, 10.0)));
        assert!(rects_overlap(a, Rect::new(9.9, 0.0, 10.0, 10.0)));
    }
}
