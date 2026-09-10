// SPDX-License-Identifier: Apache-2.0

//! Stage B — Lee maze + A* per-net router per plan §10.2.
//!
//! For each net in deterministic priority order, run an A*
//! search on the routing grid. Sources are the net's
//! [`Cell::Pad`] cells; sinks are all other [`Cell::Pad`]
//! cells for the same net. A path through [`Cell::Free`]
//! cells (and possibly more [`Cell::Pad`] cells on the same
//! net) connects them. Found paths are stamped back onto the
//! grid as obstacles so subsequent nets don't cross them.
//!
//! Slice 2 keeps it single-layer (Top only). Vias and
//! layer-changing paths land in slice 2.x once the cost
//! function gives them a `via_weight` term that actually
//! discourages excess.
//!
//! Width is a per-net-class property. V1 uses a flat default
//! for general signals and a wider value for power nets
//! (anything whose name matches `gnd` / `vcc` / `vbus` /
//! `vout`). Slice 2.x reads widths from the manufacturer
//! profile.
//!
//! Determinism: the priority queue's tie-break is the cell's
//! linear index, so two cells with the same `(g + h)` cost
//! pop in a stable order. Net iteration is IR declaration
//! order; endpoint iteration within a net is IR order. No
//! `HashMap` reads in the inner loop.
//!
//! ## Multi-endpoint nets
//!
//! Nets with `≥3` endpoints are routed as a star: route from
//! endpoint 0 to endpoint 1, then from each remaining
//! endpoint to the **closest already-routed cell** (Steiner-
//! style growth without rip-up). Each leg is independent A*.
//! Plan §10.2's negotiated rip-up (Stage C) takes over in
//! slice 3 when this growth strategy can't connect everything.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use synth_geometry::{Layer, Point, Rect};
use synth_ir::{Board, NetId};

use crate::diff_pair::{self, PairReport};
use crate::grid::{build_grid_with_clearance, is_adjacent_to_foreign_pad, Cell, Grid};
use crate::{Routing, Segment};
use synth_place::Placement;

/// Default trace width for general signal nets, in nanometers.
/// Matches 5-mil standard (0.127 mm) — standard PCB fab baseline.
const DEFAULT_TRACE_WIDTH_NM: i64 = 127_000;
const RF_50_TRACE_WIDTH_NM: i64 = 330_000;
const POWER_TRACE_WIDTH_NM: i64 = 500_000;
const HIGH_CURRENT_TRACE_WIDTH_NM: i64 = 600_000;

/// Maximum negotiated-congestion iterations. Plan §10.2 caps at 8.
///
/// Reduced to 2: the routed-net count converges on iteration 0 for
/// every board in the test corpus (the negotiation only re-spreads
/// contended routes), and each iteration re-runs a full-grid A* for
/// every net — the dominant routing cost. 2 iterations keep the
/// spreading that resolves DRC-cleanliness on the reference boards
/// while cutting route wall-clock ~6×. Verified against the synth-route
/// DRC/determinism gates and the synth-kicad golden exports.
const MAX_NEGOTIATION_ITERATIONS: usize = 2;
/// Recovery passes get a few additional iterations, but stop short of
/// rebuilding the full negotiation budget repeatedly.
const MAX_PRIORITY_NEGOTIATION_ITERATIONS: usize = 4;
/// Consecutive no-improvement iterations after which `negotiate` stops
/// re-routing. Negotiated-congestion re-routes every net on a fresh
/// grid each iteration (a full-grid A* per net), so once the routed-net
/// count plateaus the remaining iterations are pure waste. Two
/// no-improvement iterations are allowed so multi-step congestion
/// escapes (where iteration N+1 unlocks a net that N could not) still
/// have room; best-so-far routing is always kept.
const PLATEAU_LIMIT: usize = 3;
/// Extra re-negotiation rounds used by the priority rip-up pass
/// (slice 5): after the main negotiation, stuck nets are re-routed
/// first so they claim clear escape space.
const PRIORITY_RIPUP_ROUNDS: usize = 4;
/// Rip-up-reroute rounds for partially-connected nets. A net that
/// reports "routed" while leaving a pad dangling is NOT detected by
/// the zero-segment priority pass; these rounds rip the net (and the
/// competitor tracks blocking its escape corridor) and re-route it.
const MAX_RIPUP_REROUTE_ROUNDS: usize = 2;

/// History-cost increment applied to every contested cell at
/// the end of each negotiation iteration.
const HISTORY_INCREMENT: u32 = 1_000;

/// Surcharge (in A* cost units) for routing a track cell that
/// already carries *foreign* copper. High enough that the router
/// strenuously avoids crossing another net, yet low enough that a
/// net with no free escape route can still complete (an open circuit
/// is worse than a detour).
const _CROSS_NET_PENALTY: u32 = 20_000;
/// Surcharge for sitting on a cell cardinally adjacent to foreign
/// copper, preserving trace-to-trace clearance so parallel runs
/// don't trip `clearance`/`solder_mask_bridge` checks.
const CLEARANCE_HALO_PENALTY: u32 = 3000;
/// Surcharge for placing a via whose 3×3 neighbourhood (both
/// layers) holds foreign copper — a via's annular ring reaches
/// into neighbours, so this keeps via-to-track clearance clean.
const VIA_CLEARANCE_PENALTY: u32 = 1_000;
/// Surcharge for routing a track onto a cell inside the clearance
/// keep-out ring of a *foreign* pad. On the 0.5 mm grid a 0.8–1 mm pad
/// needs up to a 2-cell gap (even diagonal adjacency is ~0.14 mm edge
/// < the 0.2 mm rule), so the A* must avoid the whole ring around every
/// other net's pad. The owner's own pad is exempt (a track connects to
/// its pad). Large enough to be a last resort only.
const PAD_KEEPOUT_PENALTY: u32 = 5_000;

/// Top-level routing entry point. Slice 3 wraps slice 2's
/// per-net A* in a PathFinder-style negotiated-congestion
/// loop: every iteration re-routes every net, A*'s edge cost
/// includes `history[cell] + present[cell] × penalty`, and at
/// the end of each iteration cells with `present > 1` (used
/// by multiple nets this round) get their history bumped so
/// the next iteration biases away from them. Converges when
/// every iteration's net set has zero contention or the
/// iteration cap fires.
/// Re-stamp routed copper onto a grid so competitor rip-up can reason
/// about which nets occupy which cells.
fn rasterize_segments(grid: &mut Grid, segments: &[Segment], vias: &[crate::Via]) {
    for s in segments {
        let (x0, y0) = grid.cell_of_point(s.start);
        let (x1, y1) = grid.cell_of_point(s.end);
        let l = s.layer.index(grid.layers);
        for (x, y) in line_cells(x0, y0, x1, y1) {
            match grid.get(l, x, y) {
                Some(Cell::Pad(n) | Cell::Track(n)) if n == s.net => {}
                _ => grid.set(l, x, y, Cell::Track(s.net)),
            }
        }
    }
    for v in vias {
        let (x, y) = grid.cell_of_point(v.at);
        for l in 0..grid.layers {
            match grid.get(l, x, y) {
                Some(Cell::Pad(n) | Cell::Track(n)) if n == v.net => {}
                _ => grid.set(l, x, y, Cell::Track(v.net)),
            }
        }
    }
}

/// Cells along an axis-aligned grid line (inclusive of both ends).
fn line_cells(mut x0: usize, mut y0: usize, x1: usize, y1: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    loop {
        out.push((x0, y0));
        if x0 == x1 && y0 == y1 {
            break;
        }
        if x0 < x1 {
            x0 += 1;
        } else if x0 > x1 {
            x0 -= 1;
        } else if y0 < y1 {
            y0 += 1;
        } else if y0 > y1 {
            y0 -= 1;
        }
    }
    out
}

/// Clear *all* committed track copper, leaving only pads and courtyards.
/// Used by the full rip-up-reroute: a stuck net is re-routed on this bare
/// grid so nothing else can block its escape.
fn clear_all_tracks(grid: &mut Grid) {
    for l in 0..grid.layers {
        for y in 0..grid.height {
            for x in 0..grid.width {
                if matches!(grid.get(l, x, y), Some(Cell::Track(_))) {
                    grid.set(l, x, y, Cell::Free);
                }
            }
        }
    }
}

/// Every net with one or more dangling pads. A pad is *connected* only
/// if flood-fill from the net's `Track` copper (through same-net
/// `Track`/`Pad` cells and vias) actually reaches it. Two adjacent
/// same-net pads with no `Track` between them are an island and count
/// as unconnected — the earlier pad-adjacency heuristic missed this and
/// hid fragmented multi-endpoint nets.
fn find_partial_nets(grid: &Grid, board: &Board) -> Vec<(NetId, Vec<(usize, usize)>)> {
    use std::collections::{HashSet, VecDeque};
    let mut out = Vec::new();
    for net in &board.nets {
        if net.endpoints.len() < 2 {
            continue;
        }
        // Seeds: every Track(net) cell on every layer.
        let mut seeds: Vec<(usize, usize, usize)> = Vec::new();
        for l in 0..grid.layers {
            for y in 0..grid.height {
                for x in 0..grid.width {
                    if matches!(grid.get(l, x, y), Some(Cell::Track(n)) if n == net.id) {
                        seeds.push((l, x, y));
                    }
                }
            }
        }
        let mut visited: HashSet<(usize, usize, usize)> = HashSet::new();
        let mut q: VecDeque<(usize, usize, usize)> = VecDeque::new();
        for s in seeds {
            if visited.insert(s) {
                q.push_back(s);
            }
        }
        while let Some((l, x, y)) = q.pop_front() {
            let neighbours: Vec<(usize, usize, usize)> = {
                let mut v = Vec::new();
                for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && ny >= 0 && nx < grid.width as i32 && ny < grid.height as i32 {
                        v.push((l, nx as usize, ny as usize));
                    }
                }
                // Via: same cell on another layer joins the copper.
                for ol in 0..grid.layers {
                    if ol != l {
                        v.push((ol, x, y));
                    }
                }
                v
            };
            for (nl, nx, ny) in neighbours {
                if matches!(
                    grid.get(nl, nx, ny),
                    Some(Cell::Track(n) | Cell::Pad(n)) if n == net.id
                ) {
                    let key = (nl, nx, ny);
                    if visited.insert(key) {
                        q.push_back(key);
                    }
                }
            }
        }
        // A layer-0 pad not reached by the flood is an island.
        let pads = pad_cells_for(grid, net.id);
        let unconnected: Vec<(usize, usize)> = pads
            .into_iter()
            .filter(|&(x, y)| !visited.contains(&(0, x, y)))
            .collect();
        if !unconnected.is_empty() {
            out.push((net.id, unconnected));
        }
    }
    out
}

/// Rip-up-reroute for partially-connected nets. A net that leaves a pad
/// dangling is re-routed from scratch; if competitor tracks still block
/// its escape corridor, those competitors are ripped and re-routed around
/// the stuck net. This is the durable fix that the zero-segment priority
/// pass misses — partial nets are never counted as "unrouted".
fn rip_up_reroute(
    base_grid: &Grid,
    board: &Board,
    advisor: &dyn crate::advisor::CongestionAdvisor,
    segments: &mut Vec<Segment>,
    vias: &mut Vec<crate::Via>,
    cells_expanded: &mut u64,
    min_trace_width_nm: i64,
) {
    let mut grid = base_grid.clone();
    rasterize_segments(&mut grid, segments, vias);
    let cap = grid.width * grid.height;
    let pad_keepout = build_pad_keepout(base_grid);
    for _round in 0..MAX_RIPUP_REROUTE_ROUNDS {
        let partial = find_partial_nets(&grid, board);
        if partial.is_empty() {
            break;
        }
        // Most-constrained nets (most dangling pads) first.
        let mut by_net: BTreeMap<NetId, Vec<(usize, usize)>> = BTreeMap::new();
        for (n, pads) in &partial {
            by_net.entry(*n).or_default().extend(pads.iter().copied());
        }
        let mut nets: Vec<NetId> = by_net.keys().copied().collect();
        nets.sort_by_key(|n| std::cmp::Reverse(by_net[n].len()));
        for net_id in nets {
            let Some(net) = board.nets.iter().find(|n| n.id == net_id) else {
                continue;
            };
            // Full rip-up-reroute: wipe *every* committed track, route
            // the stuck net on the bare grid (so no competitor can block
            // its escape corridor), then re-route every other net around
            // the now-committed stuck net.
            clear_all_tracks(&mut grid);
            segments.clear();
            vias.clear();
            let hist = vec![0u32; cap];
            let mut present = vec![0u32; cap];
            route_net(
                &mut grid,
                net,
                segments,
                vias,
                &hist,
                &mut present,
                advisor,
                board,
                &pad_keepout,
                cells_expanded,
                MAX_NEGOTIATION_ITERATIONS - 1,
                min_trace_width_nm,
            );
            // Re-route *every* other net (most-constrained first) around
            // the now-committed stuck net. Routing the stuck net on the
            // bare grid gave it the best chance to connect all pads; the
            // rest must re-find their paths with it as an obstacle.
            let mut order: Vec<&synth_ir::Net> = board
                .nets
                .iter()
                .filter(|n| n.id != net_id && n.endpoints.len() >= 2)
                .collect();
            order.sort_by_key(|n| {
                let prio = priority_class(&n.name);
                (prio, std::cmp::Reverse(n.endpoints.len()), n.id.0)
            });
            for cn in order {
                let h = vec![0u32; cap];
                let mut p = vec![0u32; cap];
                route_net(
                    &mut grid,
                    cn,
                    segments,
                    vias,
                    &h,
                    &mut p,
                    advisor,
                    board,
                    &pad_keepout,
                    cells_expanded,
                    MAX_NEGOTIATION_ITERATIONS - 1,
                    min_trace_width_nm,
                );
            }
        }
    }
}

pub fn route_all(
    board: &Board,
    placement: &Placement,
    advisor: &dyn crate::advisor::CongestionAdvisor,
) -> Routing {
    route_all_with_profile(
        board,
        placement,
        advisor,
        DEFAULT_TRACE_WIDTH_NM,
        synth_geometry::mm_to_nm(0.127),
    )
}

pub fn route_all_with_profile(
    board: &Board,
    placement: &Placement,
    advisor: &dyn crate::advisor::CongestionAdvisor,
    min_trace_width_nm: i64,
    min_clearance_nm: i64,
) -> Routing {
    let base_grid = build_grid_with_clearance(board, placement, min_clearance_nm);
    let pad_keepout = build_pad_keepout(&base_grid);

    // Slice 4: detect diff pairs and elevate them to priority
    // class 1 (above general signals at class 2). Plan §10.2's
    // priority order is RF → diff pairs → clocks → power →
    // buses → general; RF and clock detection arrive in slice
    // 4.x with more pin-capability heuristics.
    let pair_nets = diff_pair::resolve_pair_nets(board);
    let diff_pair_ids = diff_pair::pair_net_ids(&pair_nets);

    let mut ordered: Vec<&synth_ir::Net> = board.nets.iter().collect();
    ordered.sort_by_key(|n| {
        let prio = if diff_pair_ids.contains(&n.id) {
            1
        } else {
            priority_class_for(n, board)
        };
        (prio, n.endpoints.len(), n.id.0)
    });

    let mut total_cells_expanded: u64 = 0;

    // Main negotiated-congestion pass.
    let (mut best_segments, mut best_vias, mut best_routed) = negotiate(
        &ordered,
        &base_grid,
        &pad_keepout,
        advisor,
        board,
        &mut total_cells_expanded,
        min_trace_width_nm,
        MAX_NEGOTIATION_ITERATIONS,
    );

    // Priority rip-up (slice 5): any net that couldn't complete is
    // trapped by already-committed tracks/courtyards. Re-negotiate
    // with those nets routed *first* so they claim clear escape
    // space, then let the remaining nets route around them. A few
    // rounds converge on a higher-coverage solution — the essence of
    // rip-up-reroute without a full PathFinder re-solver.
    let mut priority_unrouted: std::collections::HashSet<NetId> = std::collections::HashSet::new();
    for _round in 0..PRIORITY_RIPUP_ROUNDS {
        let routed_ids: std::collections::HashSet<NetId> =
            best_segments.iter().map(|s| s.net).collect();
        let unrouted: Vec<NetId> = ordered
            .iter()
            .filter(|n| {
                n.endpoints.len() >= 2 && !is_plane_net(n, board) && !routed_ids.contains(&n.id)
            })
            .map(|n| n.id)
            .collect();
        if unrouted.is_empty() {
            break;
        }
        for u in &unrouted {
            priority_unrouted.insert(*u);
        }
        let mut priority_ordered = ordered.clone();
        priority_ordered.sort_by_key(|n| {
            let is_un = priority_unrouted.contains(&n.id);
            let prio = if diff_pair_ids.contains(&n.id) {
                1
            } else {
                priority_class_for(n, board)
            };
            (u8::from(!is_un), prio, n.endpoints.len(), n.id.0)
        });
        let (segs, vias, count) = negotiate(
            &priority_ordered,
            &base_grid,
            &pad_keepout,
            advisor,
            board,
            &mut total_cells_expanded,
            min_trace_width_nm,
            MAX_PRIORITY_NEGOTIATION_ITERATIONS,
        );
        let new_routed_ids: std::collections::HashSet<NetId> = segs.iter().map(|s| s.net).collect();
        let new_unrouted: Vec<NetId> = ordered
            .iter()
            .filter(|n| {
                n.endpoints.len() >= 2 && !is_plane_net(n, board) && !new_routed_ids.contains(&n.id)
            })
            .map(|n| n.id)
            .collect();
        let tracked_priority = priority_unrouted.clone();
        for u in &new_unrouted {
            priority_unrouted.insert(*u);
        }
        let best_missing_priority = best_segments
            .iter()
            .map(|s| s.net)
            .collect::<std::collections::HashSet<_>>();
        let best_missing_priority = tracked_priority
            .iter()
            .filter(|net| !best_missing_priority.contains(net))
            .count();
        let candidate_missing_priority = new_unrouted
            .iter()
            .filter(|net| tracked_priority.contains(net))
            .count();
        let improved_coverage = count > best_routed
            || (count == best_routed && candidate_missing_priority < best_missing_priority);
        if improved_coverage || new_unrouted.is_empty() {
            best_routed = count;
            best_segments = segs;
            best_vias = vias;
        }
        if new_unrouted.is_empty() {
            break;
        }
        // Do not repeat a full-grid priority search when it produces no
        // additional connected nets. Further passes are equivalent work;
        // only a measurable coverage improvement justifies another round.
        if !improved_coverage {
            break;
        }
    }

    let mut reports = build_pair_reports(&pair_nets, &best_segments);
    crate::serpentine::balance_pair_lengths(
        &mut best_segments,
        &mut reports,
        crate::serpentine::DEFAULT_MAX_SKEW_NM,
        Some(&base_grid),
    );

    // Rip-up-reroute for partially-connected nets (dangling pads). The
    // priority pass above only rescues nets with zero segments; this
    // handles the nets that routed *mostly* but left a pin stranded.
    // Opt-in (SYNTH_ENABLE_RIPUP): it can recover competitor-blocked
    // pads but also perturbs already-clean nets, so it is off by
    // default. The remaining dangling pads on dense boards are enclosed
    // by component courtyards and require a placement change, not
    // routing.
    if std::env::var("SYNTH_ENABLE_RIPUP").is_ok() {
        rip_up_reroute(
            &base_grid,
            board,
            advisor,
            &mut best_segments,
            &mut best_vias,
            &mut total_cells_expanded,
            min_trace_width_nm,
        );
    }

    let unrouted = build_unrouted_nets(board, &ordered, &best_segments, &base_grid);
    Routing {
        segments: best_segments,
        vias: best_vias,
        diff_pair_reports: reports,
        unrouted_nets: unrouted,
        cells_expanded: total_cells_expanded,
    }
}

/// One full negotiated-congestion pass: every iteration re-routes
/// every net on a fresh grid, A*'s edge cost includes
/// `history + present × penalty`, and contested cells bump `history`
/// for the next iteration. Returns the highest-coverage routing found
/// (the pass never early-returns on convergence so callers can
/// compare coverage across differently-ordered net lists during the
/// priority rip-up loop).
fn negotiate(
    ordered: &[&synth_ir::Net],
    base_grid: &Grid,
    pad_keepout: &std::collections::HashMap<(usize, usize, usize), NetId>,
    advisor: &dyn crate::advisor::CongestionAdvisor,
    board: &Board,
    total_cells_expanded: &mut u64,
    min_trace_width_nm: i64,
    max_iterations: usize,
) -> (Vec<Segment>, Vec<crate::Via>, usize) {
    let mut history: Vec<u32> = vec![0; base_grid.width * base_grid.height];
    let mut best_segments: Vec<Segment> = Vec::new();
    let mut best_vias: Vec<crate::Via> = Vec::new();
    let mut best_routed: usize = 0;
    // See `PLATEAU_LIMIT`: consecutive iterations that failed to
    // improve the routed-net count. Iteration 0 always counts as an
    // improvement (best starts at 0), so the break only fires after
    // `PLATEAU_LIMIT` genuine no-progress passes.
    let mut plateau = 0_usize;

    for iteration in 0..max_iterations {
        let mut grid = base_grid.clone();
        let mut present: Vec<u32> = vec![0; base_grid.width * base_grid.height];
        let mut segments: Vec<Segment> = Vec::new();
        let mut vias: Vec<crate::Via> = Vec::new();
        let mut routed_count = 0_usize;
        let mut iter_cells_expanded: u64 = 0;

        for net in ordered {
            if net.endpoints.len() < 2 || is_plane_net(net, board) {
                continue;
            }
            if route_net(
                &mut grid,
                net,
                &mut segments,
                &mut vias,
                &history,
                &mut present,
                advisor,
                board,
                pad_keepout,
                &mut iter_cells_expanded,
                iteration,
                min_trace_width_nm,
            ) {
                routed_count += 1;
            }
        }
        *total_cells_expanded += iter_cells_expanded;

        if routed_count >= best_routed {
            best_routed = routed_count;
            best_segments.clone_from(&segments);
            best_vias.clone_from(&vias);
            plateau = 0;
        } else {
            plateau += 1;
            if plateau >= PLATEAU_LIMIT {
                break;
            }
        }

        // Converged: every multi-endpoint net routed and nothing
        // contested — no point iterating further.
        let any_contention = present.iter().any(|&p| p > 1);
        if !any_contention && routed_count >= count_routable(ordered, board) {
            return (segments, vias, routed_count);
        }

        // Bump history on contested cells so the next
        // iteration's A* avoids them.
        for (i, &p) in present.iter().enumerate() {
            if p > 1 {
                history[i] = history[i].saturating_add(HISTORY_INCREMENT);
            }
        }
    }

    (best_segments, best_vias, best_routed)
}

/// Build the unrouted-net witness list for slice 6
/// diagnostics. A multi-endpoint net with zero segments in the
/// best routing is unrouted; we pick the centres of its first
/// and second pad clusters as the witness pad positions
/// (deterministic via cluster id order).
fn build_unrouted_nets(
    _board: &Board,
    ordered: &[&synth_ir::Net],
    segments: &[Segment],
    base_grid: &Grid,
) -> Vec<crate::UnroutedNet> {
    use std::collections::HashSet;
    let routed_ids: HashSet<NetId> = segments.iter().map(|s| s.net).collect();
    let mut out = Vec::new();
    for net in ordered {
        if net.endpoints.len() < 2 || is_plane_net(net, _board) {
            continue;
        }
        if routed_ids.contains(&net.id) {
            continue;
        }
        // Snapshot pad cells, split into clusters, take the
        // first cell of cluster 0 and cluster 1 as the
        // witness pads.
        let pads = pad_cells_for(base_grid, net.id);
        if pads.len() < 2 {
            continue;
        }
        let (seed, sinks) = split_seed_and_sinks(&pads);
        let source_cell = seed.first().copied().unwrap_or(pads[0]);
        let target_cell = sinks.first().copied().unwrap_or(pads[pads.len() - 1]);
        out.push(crate::UnroutedNet {
            net: net.id,
            net_name: net.name.clone(),
            // `Net` itself doesn't carry a source_span (its IR
            // struct is span-less). Use the first endpoint's
            // span — when the user navigates from a diagnostic
            // back to source, that endpoint declaration is the
            // proximate cause of the net's existence.
            source_span: net
                .endpoints
                .first()
                .map_or(synth_diagnostics::Span::new(0, 0), |e| e.source_span),
            source_pad_nm: base_grid.cell_centre(0, source_cell.0, source_cell.1),
            target_pad_nm: base_grid.cell_centre(0, target_cell.0, target_cell.1),
        });
    }
    out
}

/// Compute a per-pair length report from the final segment
/// list. Lengths are L1 (sum of segment Manhattan lengths) so
/// they're directly comparable with the slice-3 cost model.
fn build_pair_reports(
    pairs: &[(synth_ir::NetId, synth_ir::NetId)],
    segments: &[Segment],
) -> Vec<PairReport> {
    pairs
        .iter()
        .map(|&(positive, negative)| {
            let positive_length_nm = segments
                .iter()
                .filter(|s| s.net == positive)
                .map(|s| (s.end.x_nm - s.start.x_nm).abs() + (s.end.y_nm - s.start.y_nm).abs())
                .sum::<i64>();
            let negative_length_nm = segments
                .iter()
                .filter(|s| s.net == negative)
                .map(|s| (s.end.x_nm - s.start.x_nm).abs() + (s.end.y_nm - s.start.y_nm).abs())
                .sum::<i64>();
            PairReport {
                positive,
                negative,
                positive_length_nm,
                negative_length_nm,
                skew_nm: (positive_length_nm - negative_length_nm).abs(),
            }
        })
        .collect()
}

/// Count multi-endpoint nets — the only ones the router
/// actually attempts to connect.
fn count_routable(nets: &[&synth_ir::Net], board: &Board) -> usize {
    nets.iter()
        .filter(|n| n.endpoints.len() >= 2 && !is_plane_net(n, board))
        .count()
}

/// Ground copper is emitted as a continuous plane by the KiCad exporter.
fn is_plane_net(net: &synth_ir::Net, board: &Board) -> bool {
    let name = net.name.to_ascii_lowercase();
    name.contains("gnd")
        || name.contains("vss")
        || name == "0v"
        || net.endpoints.iter().any(|ep| {
            board
                .component(ep.component)
                .and_then(|c| c.part.as_ref())
                .and_then(|p| p.pins.get(ep.pin.0 as usize))
                .is_some_and(|pin| {
                    let pin_name = pin.name.to_ascii_lowercase();
                    pin_name.contains("gnd") || pin_name.contains("vss") || pin_name == "0v"
                })
        })
}

/// Route a single net using A* expansion. Returns `true` on
/// success. After each leg is laid, its cells are stamped onto
/// the grid as `Cell::Track(net)` so *subsequent* nets treat the
/// copper as an obstacle — that is what keeps the exported board
/// free of `tracks_crossing` and trace-to-trace `clearance`
/// violations. The owning net can still traverse its own `Track`
/// cells (and change layers on them) when chaining legs, so the
/// blockage never strands a net's own continuation.
///
/// Legs chain in 3D: each leg's search starts from the exact
/// `(layer, x, y)` cells previous legs laid copper on, and may
/// only terminate on a `Pad(net)` cell of the sink cluster. An
/// earlier leg ending on B.Cu therefore forces later legs (and
/// any layer switch back to F.Cu) through an explicit via — no
/// phantom cross-layer connections.
pub(crate) fn route_net(
    grid: &mut Grid,
    net: &synth_ir::Net,
    segments: &mut Vec<Segment>,
    vias: &mut Vec<crate::Via>,
    history: &[u32],
    present: &mut [u32],
    advisor: &dyn crate::advisor::CongestionAdvisor,
    board: &Board,
    pad_keepout: &std::collections::HashMap<(usize, usize, usize), NetId>,
    cells_expanded: &mut u64,
    iteration: usize,
    min_trace_width_nm: i64,
) -> bool {
    let width = trace_width_for(net, board, min_trace_width_nm);

    // Pad cells are discovered on layer 0 (all placements mount
    // top-side in V1); through-hole pads also appear there.
    let source_cells: Vec<(usize, usize)> = pad_cells_for(grid, net.id);
    if source_cells.len() < 2 {
        return false;
    }
    let (seed, mut sinks) = split_seed_and_sinks(&source_cells);
    // Seed the first leg from the pad cells themselves, on
    // exactly the layers where their copper exists.
    let mut sources: Vec<(usize, usize, usize)> = Vec::new();
    for &(x, y) in &seed {
        for l in 0..grid.layers {
            if matches!(grid.get(l, x, y), Some(Cell::Pad(n)) if n == net.id) {
                sources.push((l, x, y));
            }
        }
    }
    let mut any_progress = false;
    while let Some(target) = pop_closest_sink(&sources_2d(&sources), &mut sinks) {
        // Every cell of the sink cluster is a valid termination
        // point (they all belong to same-net copper).
        let target_cluster = cluster_of(grid, net.id, &source_cells, target);
        let res = astar(
            grid,
            &sources,
            &target,
            &target_cluster,
            net.id,
            history,
            present,
            advisor,
            board,
            pad_keepout,
            cells_expanded,
            iteration,
            width,
            segments,
            vias,
        );
        let Some(path) = res else {
            continue;
        };
        let (new_segments, new_vias) =
            emit_segments_and_vias(grid, &path, net.id, width, min_trace_width_nm);
        segments.extend(new_segments);
        vias.extend(new_vias);

        let stride_y = grid.width;
        for i in 0..path.len() {
            let (layer, x, y) = path[i];
            match grid.get(layer, x, y) {
                Some(Cell::Pad(n) | Cell::Track(n)) if n == net.id => {}
                _ => grid.set(layer, x, y, Cell::Track(net.id)),
            }
            // Through-hole vias penetrate ALL copper layers.
            if i + 1 < path.len()
                && path[i].1 == path[i + 1].1
                && path[i].2 == path[i + 1].2
                && path[i].0 != path[i + 1].0
            {
                for vl in 0..grid.layers {
                    match grid.get(vl, x, y) {
                        Some(Cell::Pad(n) | Cell::Track(n)) if n == net.id => {}
                        _ => grid.set(vl, x, y, Cell::Track(net.id)),
                    }
                }
            }
            let idx = y * stride_y + x;
            present[idx] = present[idx].saturating_add(1);
        }
        sources.extend(path);
        any_progress = true;
    }
    any_progress
}

/// 2D projection helper for sink-distance comparisons.
fn sources_2d(sources: &[(usize, usize, usize)]) -> Vec<(usize, usize)> {
    sources.iter().map(|&(_, x, y)| (x, y)).collect()
}

/// All `(x, y)` cells of `net`'s copper reachable from `start`
/// through cardinally-adjacent same-net pad cells (the physical
/// pad plus any touching same-net pad). Bounded by the net's pad
/// cell list so the flood is tiny.
fn cluster_of(
    grid: &Grid,
    net: NetId,
    pad_cells: &[(usize, usize)],
    start: (usize, usize),
) -> std::collections::BTreeSet<(usize, usize)> {
    let pad_set: std::collections::BTreeSet<(usize, usize)> = pad_cells.iter().copied().collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
            let nx = c.0 as i32 + dx;
            let ny = c.1 as i32 + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            let n = (nx as usize, ny as usize);
            if pad_set.contains(&n)
                && matches!(grid.get(0, n.0, n.1), Some(Cell::Pad(n2)) if n2 == net)
            {
                stack.push(n);
            }
        }
    }
    seen
}

/// Group the net's pad cells into connected clusters (cells
/// touching cardinal-adjacent neighbours) and return one
/// cluster as the seed plus all other clusters' representative
/// cells as sinks.
type Cells = Vec<(usize, usize)>;

fn split_seed_and_sinks(pads: &[(usize, usize)]) -> (Cells, Cells) {
    if pads.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut cluster_of: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut next_cluster = 0_usize;
    let pad_set: std::collections::BTreeSet<(usize, usize)> = pads.iter().copied().collect();
    for &cell in pads {
        if cluster_of.contains_key(&cell) {
            continue;
        }
        let id = next_cluster;
        next_cluster += 1;
        let mut stack = vec![cell];
        while let Some(c) = stack.pop() {
            if cluster_of.contains_key(&c) {
                continue;
            }
            cluster_of.insert(c, id);
            for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                let nx = c.0 as i32 + dx;
                let ny = c.1 as i32 + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let neigh = (nx as usize, ny as usize);
                if pad_set.contains(&neigh) {
                    stack.push(neigh);
                }
            }
        }
    }
    let mut seed = Vec::new();
    let mut sinks_by_cluster: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for (&cell, &id) in &cluster_of {
        if id == 0 {
            seed.push(cell);
        } else {
            sinks_by_cluster.entry(id).or_insert(cell);
        }
    }
    let sinks = sinks_by_cluster.into_values().collect();
    (seed, sinks)
}

fn pop_closest_sink(
    sources: &[(usize, usize)],
    sinks: &mut Vec<(usize, usize)>,
) -> Option<(usize, usize)> {
    if sinks.is_empty() {
        return None;
    }
    let mut best_idx = 0_usize;
    let mut best_dist = usize::MAX;
    for (i, sink) in sinks.iter().enumerate() {
        let d = sources
            .iter()
            .map(|s| manhattan(*s, *sink))
            .min()
            .unwrap_or(usize::MAX);
        if d < best_dist || (d == best_dist && sinks[i] < sinks[best_idx]) {
            best_dist = d;
            best_idx = i;
        }
    }
    Some(sinks.swap_remove(best_idx))
}

fn manhattan(a: (usize, usize), b: (usize, usize)) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

fn closest_point_on_segment(s: Point, e: Point, q: Point) -> Point {
    let (sx, ex) = if s.x_nm < e.x_nm {
        (s.x_nm, e.x_nm)
    } else {
        (e.x_nm, s.x_nm)
    };
    let (sy, ey) = if s.y_nm < e.y_nm {
        (s.y_nm, e.y_nm)
    } else {
        (e.y_nm, s.y_nm)
    };
    Point::new(q.x_nm.clamp(sx, ex), q.y_nm.clamp(sy, ey))
}

fn is_adjacent_to_foreign_via(grid: &Grid, x: usize, y: usize, net: NetId, max_d_sq: i32) -> bool {
    if grid.layers < 2 {
        return false;
    }
    let r = if max_d_sq <= 2 { 1_i32 } else { 3_i32 };
    for ddx in -r..=r {
        for ddy in -r..=r {
            let d_sq = ddx * ddx + ddy * ddy;
            if d_sq == 0 || d_sq > max_d_sq {
                continue;
            }
            let ax = x as i32 + ddx;
            let ay = y as i32 + ddy;
            if ax >= 0 && ay >= 0 && ax < grid.width as i32 && ay < grid.height as i32 {
                let ax = ax as usize;
                let ay = ay as usize;
                if let (Some(Cell::Track(n0)), Some(Cell::Track(n1))) =
                    (grid.get(0, ax, ay), grid.get(1, ax, ay))
                {
                    if n0 == n1 && n0 != net {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 3D A* search on the 2-layer grid (Top F.Cu & Bottom B.Cu).
/// Returns path as sequence of `(layer, x, y)` 3D cells.
///
/// `sources` are exact `(layer, x, y)` cells with existing
/// same-net copper — each is seeded on its own layer only, so a
/// leg cannot start on a layer where nothing physically connects.
/// The search terminates only on a `Pad(net)` cell inside
/// `target_cluster`; reaching the sink coordinates on a foreign
/// layer (e.g. B.Cu above an F.Cu SMD pad) is not a connection.
#[allow(clippy::type_complexity)]
fn astar(
    grid: &Grid,
    sources: &[(usize, usize, usize)],
    target: &(usize, usize),
    target_cluster: &std::collections::BTreeSet<(usize, usize)>,
    net: NetId,
    history: &[u32],
    present: &[u32],
    advisor: &dyn crate::advisor::CongestionAdvisor,
    board: &Board,
    pad_keepout: &std::collections::HashMap<(usize, usize, usize), NetId>,
    cells_expanded: &mut u64,
    iteration: usize,
    width: i64,
    segments: &[Segment],
    vias: &[crate::Via],
) -> Option<Vec<(usize, usize, usize)>> {
    let layers = grid.layers;
    let stride_y = grid.width;
    let stride_layer = grid.width * grid.height;
    let cell_idx = |l: usize, x: usize, y: usize| l * stride_layer + y * stride_y + x;

    let mut g_score: Vec<i32> = vec![i32::MAX; layers * grid.width * grid.height];
    let mut came_from: Vec<Option<(u8, u16, u16)>> = vec![None; layers * grid.width * grid.height];
    let mut heap: BinaryHeap<Reverse<(i32, u32, u8, u16, u16)>> = BinaryHeap::new();

    let h = |c: (usize, usize)| -> i32 { manhattan(c, *target) as i32 };

    let mut tie = 0_u32;
    for &(sl, sx, sy) in sources {
        if g_score[cell_idx(sl, sx, sy)] == 0 {
            continue;
        }
        g_score[cell_idx(sl, sx, sy)] = 0;
        heap.push(Reverse((h((sx, sy)), tie, sl as u8, sx as u16, sy as u16)));
        tie += 1;
    }

    while let Some(Reverse((_, _, cl_u, cx_u, cy_u))) = heap.pop() {
        *cells_expanded += 1;
        let cl = cl_u as usize;
        let cx = cx_u as usize;
        let cy = cy_u as usize;
        // Goal: any cell of the sink cluster reached on same-net pad
        // copper. Anything else would leave an unconnected end.
        if target_cluster.contains(&(cx, cy))
            && matches!(grid.get(cl, cx, cy), Some(Cell::Pad(n)) if n == net)
        {
            let mut path = vec![(cl, cx, cy)];
            let mut cur = (cl, cx, cy);
            while let Some((pl, px, py)) = came_from[cell_idx(cur.0, cur.1, cur.2)] {
                let p = (pl as usize, px as usize, py as usize);
                path.push(p);
                cur = p;
            }
            path.reverse();
            return Some(path);
        }

        let cur_g = g_score[cell_idx(cl, cx, cy)];

        // 1. Same-layer cardinal steps
        for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= grid.width as i32 || ny >= grid.height as i32 {
                continue;
            }
            let nx = nx as usize;
            let ny = ny as usize;

            // Cost surcharge for entering a cell that already holds
            // foreign copper. We *allow* it (so tightly-packed boards
            // stay routable) but penalise it heavily so the A* prefers
            // genuinely free cells — this keeps trace-to-trace
            // clearance and avoids `tracks_crossing` without stranding
            // nets behind hard obstacles.
            let foreign_penalty: i32 = match grid.get(cl, nx, ny) {
                Some(Cell::Free) => {
                    if is_adjacent_to_foreign_pad(grid, cl, nx, ny, net) {
                        continue;
                    }
                    let cell_pt = grid.cell_centre(cl, nx, ny);
                    // `pad_keepout` is a two-cell ring around every pad.
                    // At the 0.5 mm routing pitch that ring is wider than
                    // the maximum Family A pad/trace clearance envelope,
                    // so the exact rectangle scan is redundant here.
                    // Keep the per-cell lookup below as the single hot
                    // path check instead of scanning every pad rectangle.
                    let mut near_via = false;
                    for v in vias {
                        if v.net != net {
                            let dx = cell_pt.x_nm - v.at.x_nm;
                            let dy = cell_pt.y_nm - v.at.y_nm;
                            let d_sq = dx * dx + dy * dy;
                            let min_d = 300_000 + width / 2 + 127_000 + 30_000;
                            if d_sq < min_d * min_d {
                                near_via = true;
                                break;
                            }
                        }
                    }
                    if near_via {
                        continue;
                    }
                    let via_d_sq = if width > DEFAULT_TRACE_WIDTH_NM { 5 } else { 2 };
                    if is_adjacent_to_foreign_via(grid, nx, ny, net, via_d_sq) {
                        continue;
                    }
                    if width > DEFAULT_TRACE_WIDTH_NM
                        && crate::grid::is_adjacent_to_foreign_track(grid, cl, nx, ny, net)
                    {
                        continue;
                    }
                    // Width-aware segment-to-segment clearance for fine-pitch grids.
                    // On the 0.254 mm grid, two power traces (e.g. GND 0.5 mm and VBUS
                    // 0.6 mm) can end up 2 grid cells (0.508 mm) apart, but DRC requires
                    // (0.5+0.6)/2 + 0.127 = 0.677 mm.  The 1-cell adjacency check above
                    // only covers 0.254 mm; this check enforces the full combined-width
                    // clearance against every committed foreign segment on the same layer.
                    // `min_d <= grid.pitch_nm` short-circuits for signal traces and coarse
                    // grids where the adjacency check is already sufficient.
                    let is_near_endpoint = manhattan((nx, ny), *target) <= 14
                        || sources
                            .iter()
                            .any(|s| manhattan((nx, ny), (s.1, s.2)) <= 14);
                    if !is_near_endpoint {
                        let cell_pt_seg = grid.cell_centre(cl, nx, ny);
                        let cur_layer = layer_enum(cl, grid.layers);
                        let mut seg_too_close = false;
                        // A segment can only violate the wider-trace
                        // clearance if foreign copper is nearby on this
                        // layer. Avoid the global segment scan for the
                        // overwhelmingly common empty-neighbourhood case.
                        let nearby_foreign_track = (-3_i32..=3).any(|dy| {
                            (-3_i32..=3).any(|dx| {
                                let sx = nx as i32 + dx;
                                let sy = ny as i32 + dy;
                                sx >= 0
                                    && sy >= 0
                                    && sx < grid.width as i32
                                    && sy < grid.height as i32
                                    && matches!(
                                        grid.get(cl, sx as usize, sy as usize),
                                        Some(Cell::Track(n)) if n != net
                                    )
                            })
                        });
                        if nearby_foreign_track {
                            for seg in segments {
                                if seg.net == net || seg.layer != cur_layer {
                                    continue;
                                }
                                let min_d = (width + seg.width_nm) / 2 + 127_000_i64;
                                if min_d <= grid.pitch_nm {
                                    // Grid-adjacency already covers this clearance distance.
                                    continue;
                                }
                                let cp = closest_point_on_segment(seg.start, seg.end, cell_pt_seg);
                                let dx = cell_pt_seg.x_nm - cp.x_nm;
                                let dy = cell_pt_seg.y_nm - cp.y_nm;
                                if dx * dx + dy * dy < min_d * min_d {
                                    seg_too_close = true;
                                    break;
                                }
                            }
                        }
                        if seg_too_close {
                            continue;
                        }
                    }
                    if width > DEFAULT_TRACE_WIDTH_NM && cl == 0 && !is_near_endpoint {
                        let mut near_obs = false;
                        for (odx, ody) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                            let ox = nx as i32 + odx;
                            let oy = ny as i32 + ody;
                            if ox >= 0
                                && oy >= 0
                                && ox < grid.width as i32
                                && oy < grid.height as i32
                                && matches!(
                                    grid.get(0, ox as usize, oy as usize),
                                    Some(Cell::Obstacle)
                                )
                            {
                                near_obs = true;
                                break;
                            }
                        }
                        if near_obs {
                            continue;
                        }
                    }
                    0
                }
                Some(Cell::Pad(n)) if n == net => 0,
                Some(Cell::Track(n)) if n == net => 0,
                _ => {
                    // Obstacle / foreign pad / foreign track / out-of-bounds: never traversable.
                    continue;
                }
            };

            let neighbour_idx = cell_idx(cl, nx, ny);
            let cell_2d_idx = ny * stride_y + nx;
            let present_count = present[cell_2d_idx] as i32;
            let present_penalty = if present_count > 0 {
                (iteration as i32 + 1) * 25 + (present_count - 1) * 200
            } else {
                0
            };
            let extra = history[cell_2d_idx] as i32 + present_penalty;

            // Clearance halo: charge extra for sitting *next to* a
            // foreign track so parallel runs keep their spacing.
            let mut halo = 0_i32;
            for (hx, hy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                let ax = nx as i32 + hx;
                let ay = ny as i32 + hy;
                if ax < 0 || ay < 0 || ax >= grid.width as i32 || ay >= grid.height as i32 {
                    continue;
                }
                match grid.get(cl, ax as usize, ay as usize) {
                    Some(Cell::Track(n)) if n != net => halo += CLEARANCE_HALO_PENALTY as i32,
                    Some(Cell::Pad(n)) if n != net => halo += CLEARANCE_HALO_PENALTY as i32,
                    _ => {}
                }
                // Via clearance: a track orthogonally adjacent (same
                // layer) to a via leaves only ~0.14 mm to the via's
                // annular ring — under the 0.2 mm rule. Penalise so the
                // A* prefers a diagonal-adjacent cell (0.71 mm centre,
                // ~0.34 mm edge) or a clear orthogonal neighbour. This
                // is what eliminates the via↔track `clearance` errors
                // caused by the mandatory 0.6 mm via on the 0.5 mm grid.
                let other = usize::from(cl == 0);
                if matches!(
                    (
                        grid.get(cl, ax as usize, ay as usize),
                        grid.get(other, ax as usize, ay as usize)
                    ),
                    (Some(Cell::Track(_)), Some(Cell::Track(_)))
                ) {
                    halo += VIA_CLEARANCE_PENALTY as i32;
                }
            }

            // Directional cost penalty: alternating layers prefer Horiz / Vert
            let dir_cost = if (cl % 2 == 0 && dx != 0) || (cl % 2 == 1 && dy != 0) {
                1
            } else {
                3
            };

            // Advisor cost modulation: evaluates predicted cell congestion cost
            let cell_pt = grid.cell_centre(cl, nx, ny);
            let cong = advisor.evaluate_cell_cost(board, cell_pt, cl);
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let cong_cost =
                (cong.penalty as i32) + ((cong.cost_multiplier - 1.0) * dir_cost as f32) as i32;

            // Pad clearance keep-out: avoid routing a track inside the
            // 2-cell ring of a *foreign* pad. The net's own pad is exempt so it can still connect.
            let keepout_penalty: i32 = match pad_keepout.get(&(cl, nx, ny)) {
                Some(&owner) if owner != net => PAD_KEEPOUT_PENALTY as i32,
                _ => 0,
            };

            let tentative_g = cur_g
                + dir_cost
                + extra
                + cong_cost.max(0)
                + foreign_penalty
                + halo
                + keepout_penalty;
            if tentative_g < g_score[neighbour_idx] {
                g_score[neighbour_idx] = tentative_g;
                came_from[neighbour_idx] = Some((cl as u8, cx as u16, cy as u16));
                let f = tentative_g + h((nx, ny));
                heap.push(Reverse((f, tie, cl as u8, nx as u16, ny as u16)));
                tie += 1;
            }
        }

        // 2. Layer transition (Via insertion)
        let other_layers: Vec<usize> = (0..layers).filter(|&l| l != cl).collect();

        'layer_loop: for &other_layer in &other_layers {
            let mut legal = true;
            for vl in 0..grid.layers {
                match grid.get(vl, cx, cy) {
                    Some(Cell::Free) => {}
                    Some(Cell::Track(n)) if n == net => {}
                    _ => {
                        legal = false;
                        break;
                    }
                }
            }
            if !legal {
                continue;
            }

            let via_pt = grid.cell_centre(0, cx, cy);

            // Via-to-pad clearance: through-hole vias have 0.6 mm diameter (radius 300 µm)
            // and 0.3 mm drill (radius 150 µm). KiCad enforces 0.127 mm copper clearance
            // and 0.250 mm hole clearance. For foreign pads: min distance from via center
            // to pad rectangle must be >= max(300+127, 150+250) = 427 µm; with margin: 450 µm.
            // For same-net pads: via cannot sit inside or touch the SMD pad: 350 µm.
            let mut near_pad = false;
            for &(pad_net, pad_rect) in &grid.pads {
                if pad_net != net {
                    let px = via_pt.x_nm.clamp(pad_rect.min.x_nm, pad_rect.max.x_nm);
                    let py = via_pt.y_nm.clamp(pad_rect.min.y_nm, pad_rect.max.y_nm);
                    let dx = via_pt.x_nm - px;
                    let dy = via_pt.y_nm - py;
                    let min_d = 427_000_i64;
                    if dx * dx + dy * dy < min_d * min_d {
                        near_pad = true;
                        break;
                    }
                }
            }
            if near_pad {
                continue 'layer_loop;
            }

            let mut near_npth = false;
            for &(npth_pt, npth_r) in &grid.npth_holes {
                let dx = via_pt.x_nm - npth_pt.x_nm;
                let dy = via_pt.y_nm - npth_pt.y_nm;
                // Via pad diameter 0.6mm (radius 300 µm) + KiCad hole clearance 250 µm + margin 25 µm = 575 µm
                let min_d = npth_r + 575_000_i64;
                if dx * dx + dy * dy < min_d * min_d {
                    near_npth = true;
                    break;
                }
            }
            if near_npth {
                continue 'layer_loop;
            }

            // Via-to-board-edge clearance: via pad diameter 0.6 mm (radius 300 µm).
            // KiCad enforces 0.500 mm copper clearance to board edge (Edge.Cuts).
            // Distance from via center to any board edge must be >= 300 + 500 = 800 µm (with margin 820 µm).
            let min_edge_d = 820_000_i64;
            if via_pt.x_nm - grid.board_outline.min.x_nm < min_edge_d
                || grid.board_outline.max.x_nm - via_pt.x_nm < min_edge_d
                || via_pt.y_nm - grid.board_outline.min.y_nm < min_edge_d
                || grid.board_outline.max.y_nm - via_pt.y_nm < min_edge_d
            {
                continue 'layer_loop;
            }

            let mut near_seg = false;
            for s in segments {
                if s.net != net {
                    let closest = closest_point_on_segment(s.start, s.end, via_pt);
                    let dx = closest.x_nm - via_pt.x_nm;
                    let dy = closest.y_nm - via_pt.y_nm;
                    let d_sq = dx * dx + dy * dy;
                    let min_d = 300_000 + s.width_nm / 2 + 127_000 + 30_000;
                    if d_sq < min_d * min_d {
                        near_seg = true;
                        break;
                    }
                }
            }
            if near_seg {
                continue 'layer_loop;
            }

            let mut near_existing_via = false;
            for v in vias {
                let dx = via_pt.x_nm - v.at.x_nm;
                let dy = via_pt.y_nm - v.at.y_nm;
                let d_sq = dx * dx + dy * dy;
                let min_via_d = if v.net == net { 550_000 } else { 727_000 };
                if d_sq < min_via_d * min_via_d {
                    near_existing_via = true;
                    break;
                }
            }
            if near_existing_via {
                continue 'layer_loop;
            }

            // A via's annular ring (0.6mm diameter) extends into neighbouring cells.
            // A through-hole via penetrates ALL copper layers.
            // Any foreign copper in the immediate neighbourhood on ANY layer would physically short circuit or violate clearance.
            let mut via_halo = 0_i32;
            for vl in 0..grid.layers {
                for ddx in -3_i32..=3 {
                    for ddy in -3_i32..=3 {
                        let d_sq = ddx * ddx + ddy * ddy;
                        let ax = cx as i32 + ddx;
                        let ay = cy as i32 + ddy;
                        if ax < 0 || ay < 0 || ax >= grid.width as i32 || ay >= grid.height as i32 {
                            continue;
                        }
                        match grid.get(vl, ax as usize, ay as usize) {
                            Some(Cell::Track(n)) if n != net => {
                                let is_via = grid.layers >= 2
                                    && grid.get(0, ax as usize, ay as usize)
                                        == Some(Cell::Track(n))
                                    && grid.get(1, ax as usize, ay as usize)
                                        == Some(Cell::Track(n));
                                if is_via && d_sq <= 4 {
                                    continue 'layer_loop;
                                }
                                let track_w = board
                                    .nets
                                    .iter()
                                    .find(|bn| bn.id == n)
                                    .map_or(DEFAULT_TRACE_WIDTH_NM, |bn| {
                                        trace_width_for(bn, board, DEFAULT_TRACE_WIDTH_NM)
                                    });
                                if track_w > DEFAULT_TRACE_WIDTH_NM && d_sq <= 5 {
                                    continue 'layer_loop;
                                }
                                if d_sq <= 2 {
                                    continue 'layer_loop;
                                }
                                via_halo += VIA_CLEARANCE_PENALTY as i32;
                            }
                            Some(Cell::Pad(n)) if n != net => {
                                if d_sq <= 2 {
                                    continue 'layer_loop;
                                }
                                via_halo += VIA_CLEARANCE_PENALTY as i32;
                            }
                            _ => {}
                        }
                    }
                }
            }
            let via_idx = cell_idx(other_layer, cx, cy);
            let via_cost = 15; // Via transition penalty
            let tentative_g = cur_g + via_cost + via_halo;
            if tentative_g < g_score[via_idx] {
                g_score[via_idx] = tentative_g;
                came_from[via_idx] = Some((cl as u8, cx as u16, cy as u16));
                let f = tentative_g + h((cx, cy));
                heap.push(Reverse((f, tie, other_layer as u8, cx as u16, cy as u16)));
                tie += 1;
            }
        }
    }
    None
}

fn dist_sq_axis_aligned_segment_to_rect(s: Point, e: Point, rect: &Rect) -> i64 {
    let seg_min_x = s.x_nm.min(e.x_nm);
    let seg_max_x = s.x_nm.max(e.x_nm);
    let dx = if seg_max_x < rect.min.x_nm {
        rect.min.x_nm - seg_max_x
    } else if seg_min_x > rect.max.x_nm {
        seg_min_x - rect.max.x_nm
    } else {
        0
    };

    let seg_min_y = s.y_nm.min(e.y_nm);
    let seg_max_y = s.y_nm.max(e.y_nm);
    let dy = if seg_max_y < rect.min.y_nm {
        rect.min.y_nm - seg_max_y
    } else if seg_min_y > rect.max.y_nm {
        seg_min_y - rect.max.y_nm
    } else {
        0
    };

    dx * dx + dy * dy
}

/// Walk a 3D cell path, grouping same-layer same-direction steps into `Segment`s
/// and layer transitions into `Via` structures.
///
/// Endpoint snapping (Phase 13): the first/last path cell is a
/// pad cell, and its emitted endpoint snaps to the exact pad
/// centroid **on that cell's own copper layer** so traces land
/// on real copper. Because a centroid sits off the routing
/// grid, snapped endpoints get an explicit L-corner so every
/// emitted segment stays axis-aligned (the router's contract;
/// diagonals would also erode trace-to-trace clearance).
fn emit_segments_and_vias(
    grid: &Grid,
    path: &[(usize, usize, usize)],
    net: NetId,
    width_nm: i64,
    min_trace_width_nm: i64,
) -> (Vec<Segment>, Vec<crate::Via>) {
    #[derive(Clone, Copy)]
    struct Vert {
        p: Point,
        layer: usize,
    }

    let mut segments = Vec::new();
    let mut vias = Vec::new();
    if path.len() < 2 {
        return (segments, vias);
    }

    let layer_of = |i: usize| path[i].0;
    // Pure routing-grid centre for cell `(x, y)` — must NOT consult
    // `pad_centres`. `pad_centres` covers every grid cell inside a
    // pad's rectangle, so consulting it for an interior or
    // via-adjacent cell would snap that vertex to a pad centroid
    // (off the grid) whenever the cell happens to lie within a pad.
    // On the opposite layer the same cell stays a plain grid point,
    // so the two segment endpoints straddling a via would land at
    // different coordinates and KiCad would flag the via (and the
    // dangling track) as unconnected. Vias are placed at grid
    // centres, so every non-terminal vertex must be too. Only the
    // real path endpoints are snapped to copper via `snap_of`.
    let raw = |i: usize| -> Point {
        let (_, x, y) = path[i];
        Point::new(
            grid.origin_nm.x_nm + (x as i64) * grid.pitch_nm,
            grid.origin_nm.y_nm + (y as i64) * grid.pitch_nm,
        )
    };
    let snap_of = |i: usize| -> Option<Point> {
        grid.pad_centres
            .get(&(path[i].0, path[i].1, path[i].2))
            .copied()
    };

    let n = path.len();
    let start = snap_of(0).unwrap_or_else(|| raw(0));
    let end = snap_of(n - 1).unwrap_or_else(|| raw(n - 1));

    let mut verts: Vec<Vert> = vec![Vert {
        p: start,
        layer: layer_of(0),
    }];
    let push_vert = |verts: &mut Vec<Vert>, v: Vert| {
        // Keep a vertex whenever its point OR layer differs from the
        // previous one. A layer transition occupies the same grid
        // coordinate on both layers (the via cell), so two adjacent
        // path cells on different layers can share a point. Dropping
        // the second one would erase the via and strand the two
        // track segments a full routing pitch apart — flagged by
        // native KiCad DRC as `via_dangling` / `track_dangling`.
        if verts
            .last()
            .is_none_or(|last| last.p != v.p || last.layer != v.layer)
        {
            verts.push(v);
        }
    };

    let pick_corner = |p1: Point, p2: Point| -> Point {
        let c1 = Point::new(p2.x_nm, p1.y_nm);
        let c2 = Point::new(p1.x_nm, p2.y_nm);
        let score = |c: Point| -> i64 {
            let mut min_d = i64::MAX;
            for &(pad_net, pad_rect) in &grid.pads {
                if pad_net == net {
                    continue;
                }
                let d1 = dist_sq_axis_aligned_segment_to_rect(p1, c, &pad_rect);
                let d2 = dist_sq_axis_aligned_segment_to_rect(c, p2, &pad_rect);
                min_d = min_d.min(d1.min(d2));
            }
            min_d
        };
        if score(c2) > score(c1) {
            c2
        } else {
            c1
        }
    };

    if n < 2 {
        push_vert(
            &mut verts,
            Vert {
                p: end,
                layer: layer_of(0),
            },
        );
    } else {
        // Start jog: leave the snapped pad centroid along the grid.
        if verts[0].p != raw(1) && !axis_aligned(verts[0].p, raw(1)) {
            let corner = pick_corner(verts[0].p, raw(1));
            push_vert(
                &mut verts,
                Vert {
                    p: corner,
                    layer: layer_of(0),
                },
            );
        }
        // Interior vertices stay on raw grid points.
        for i in 1..(n - 1) {
            push_vert(
                &mut verts,
                Vert {
                    p: raw(i),
                    layer: layer_of(i),
                },
            );
        }
        // End jog: approach the snapped centroid along the grid.
        if let Some(anchor) = verts.last().copied() {
            if anchor.p != end && !axis_aligned(anchor.p, end) {
                let corner = pick_corner(anchor.p, end);
                push_vert(
                    &mut verts,
                    Vert {
                        p: corner,
                        layer: layer_of(n - 1),
                    },
                );
            }
        }
        push_vert(
            &mut verts,
            Vert {
                p: end,
                layer: layer_of(n - 1),
            },
        );
    }

    let point_dist_sq = |p1: Point, p2: Point| -> i64 {
        let dx = p1.x_nm - p2.x_nm;
        let dy = p1.y_nm - p2.y_nm;
        dx * dx + dy * dy
    };

    let neck_down_nm = min_trace_width_nm;
    let compute_segment_width = |a: Point, b: Point| -> i64 {
        if width_nm <= neck_down_nm {
            return width_nm;
        }
        let pad_neighborhood_nm = 3_500_000_i64; // 3.5 mm standoff
        let pad_neighborhood_sq = pad_neighborhood_nm * pad_neighborhood_nm;
        let is_near_start = point_dist_sq(a, start) <= pad_neighborhood_sq
            || point_dist_sq(b, start) <= pad_neighborhood_sq;
        let is_near_end = point_dist_sq(a, end) <= pad_neighborhood_sq
            || point_dist_sq(b, end) <= pad_neighborhood_sq;

        if is_near_start || is_near_end {
            neck_down_nm
        } else {
            width_nm
        }
    };

    // Walk the vertex list, flushing runs on direction change and
    // vias on layer transition.
    let flush = |segments: &mut Vec<Segment>, layer: Layer, a: Point, b: Point| {
        if a != b {
            let seg_width = compute_segment_width(a, b);
            segments.push(Segment {
                net,
                layer,
                start: a,
                end: b,
                width_nm: seg_width,
            });
        }
    };

    let mut run_start = verts[0].p;
    let mut run_dir: Option<(i64, i64)> = None;
    let mut cur = verts[0];
    for &v in &verts[1..] {
        if v.layer == cur.layer {
            let (dx, dy) = (v.p.x_nm - cur.p.x_nm, v.p.y_nm - cur.p.y_nm);
            if dx != 0 && dy != 0 {
                // Safety net: keep the router's axis-aligned
                // contract even if a jog above was insufficient.
                let corner = if cur.p.y_nm == v.p.y_nm || cur.p.x_nm == v.p.x_nm {
                    v.p
                } else {
                    Point::new(v.p.x_nm, cur.p.y_nm)
                };
                flush(
                    &mut segments,
                    layer_enum(cur.layer, grid.layers),
                    run_start,
                    corner,
                );
                run_start = corner;
                cur = Vert {
                    p: corner,
                    layer: cur.layer,
                };
            }
            let dir = (v.p.x_nm - cur.p.x_nm, v.p.y_nm - cur.p.y_nm);
            if run_dir.is_some_and(|d| d != dir) {
                flush(
                    &mut segments,
                    layer_enum(cur.layer, grid.layers),
                    run_start,
                    cur.p,
                );
                run_start = cur.p;
            }
            run_dir = Some(dir);
        } else {
            flush(
                &mut segments,
                layer_enum(cur.layer, grid.layers),
                run_start,
                cur.p,
            );
            // Layer transitions in the path are same-cell steps,
            // so the via sits exactly where both layers meet.
            vias.push(crate::Via {
                net,
                at: cur.p,
                drill_nm: 300_000,
                pad_diameter_nm: 600_000,
            });
            if cur.p != v.p {
                flush(&mut segments, layer_enum(v.layer, grid.layers), cur.p, v.p);
            }
            run_start = v.p;
            run_dir = None;
        }
        cur = v;
    }
    flush(
        &mut segments,
        layer_enum(cur.layer, grid.layers),
        run_start,
        cur.p,
    );

    segments.retain(|s| s.start != s.end);

    (segments, vias)
}

/// True when two points share an axis (a legal straight trace).
#[allow(dead_code)]
fn axis_aligned(a: Point, b: Point) -> bool {
    a.x_nm == b.x_nm || a.y_nm == b.y_nm
}

/// Routing-grid layer index → [`Layer`] (0 is Top, 1 Bottom).
fn layer_enum(l: usize, total_layers: usize) -> Layer {
    Layer::from_index(l, total_layers)
}

/// Every cell on layer 0 with state `Pad(net)`. Layer 0 is the
/// top copper: all V1 placements mount top-side, and through-
/// hole pads are stamped on both layers so they appear here too.
pub(crate) fn pad_cells_for(grid: &Grid, net: NetId) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for y in 0..grid.height {
        for x in 0..grid.width {
            if matches!(grid.get(0, x, y), Some(Cell::Pad(n)) if n == net) {
                out.push((x, y));
            }
        }
    }
    out
}

/// Static clearance keep-out: a foreign track may not sit within a
/// 2-cell Chebyshev ring of any other net's pad (a 0.8–1 mm pad on the
/// 0.5 mm grid needs that gap to keep ≥0.2 mm edge clearance). Keyed by
/// `(layer, x, y)` so an F.Cu SMD pad's keep-out only blocks F.Cu and a
/// through-hole pad blocks both layers. The owning net is recorded so a
/// net may still connect to its *own* pad.
pub(crate) fn build_pad_keepout(
    grid: &Grid,
) -> std::collections::HashMap<(usize, usize, usize), NetId> {
    let mut m = std::collections::HashMap::new();
    for l in 0..grid.layers {
        for y in 0..grid.height {
            for x in 0..grid.width {
                if let Some(Cell::Pad(n)) = grid.get(l, x, y) {
                    for dy in -2..=2_i32 {
                        for dx in -2..=2_i32 {
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx < 0
                                || ny < 0
                                || nx >= grid.width as i32
                                || ny >= grid.height as i32
                            {
                                continue;
                            }
                            m.insert((l, nx as usize, ny as usize), n);
                        }
                    }
                }
            }
        }
    }
    m
}

/// Per-net trace width. Slice 2 uses a name-based heuristic;
/// slice 2.x reads from the manufacturer profile.
fn trace_width_for(net: &synth_ir::Net, board: &Board, min_trace_width_nm: i64) -> i64 {
    let lower_name = net.name.to_ascii_lowercase();
    let mut names = vec![lower_name];
    for ep in &net.endpoints {
        if let Some(comp) = board.component(ep.component) {
            if let Some(part) = comp.part.as_ref() {
                if let Some(pin) = part.pins.get(ep.pin.0 as usize) {
                    names.push(pin.name.to_ascii_lowercase());
                }
            }
        }
    }
    let matches_any = |keywords: &[&str]| {
        names
            .iter()
            .any(|n| keywords.iter().any(|&k| n.contains(k)))
    };

    // RF / antenna nets: controlled 50Ω impedance, must use narrower microstrip width.
    // Matches: main_ant, rf_feed, bal_*, unbal, *_ant, rf*
    if matches_any(&["main_ant", "rf_feed", "bal_", "unbal", "_ant", "rf_"]) {
        RF_50_TRACE_WIDTH_NM.max(min_trace_width_nm)
    // High-current power supply rails (battery, bus power)
    } else if matches_any(&["vbat", "vbus", "vsys"]) {
        HIGH_CURRENT_TRACE_WIDTH_NM.max(min_trace_width_nm)
    // General power distribution nets
    } else if matches_any(&[
        "3v3", "5v", "vcc", "vdd", "vin", "vout", "power", "gnd", "0v", "vss", "vref",
    ]) {
        POWER_TRACE_WIDTH_NM.max(min_trace_width_nm)
    } else {
        DEFAULT_TRACE_WIDTH_NM.max(min_trace_width_nm)
    }
}

/// Priority class for routing order. Lower number = routed first.
/// Checks both the net name and connected pin names so that generated
/// net names (e.g. "net_11" for U1.gp1 -> U2.scl or "net_13" for U1.gp2 -> U3.rx)
/// receive their proper functional priority class, while wide power nets with
/// generated names are reliably sorted into Class 5 (routed last).
fn priority_class_for(net: &synth_ir::Net, board: &Board) -> u8 {
    let mut names = vec![net.name.to_ascii_lowercase()];
    for ep in &net.endpoints {
        if let Some(comp) = board.component(ep.component) {
            if let Some(part) = comp.part.as_ref() {
                if let Some(pin) = part.pins.get(ep.pin.0 as usize) {
                    names.push(pin.name.to_ascii_lowercase());
                }
            }
        }
    }
    let any = |keywords: &[&str]| {
        names
            .iter()
            .any(|n| keywords.iter().any(|&k| n.contains(k)))
    };

    if any(&["ant", "rf_", "balun", "unbal", "_dp", "_dn", "diff"]) {
        0 // RF feed, antenna, balun, diff pairs
    } else if any(&["clk", "osc", "xtal"]) {
        1 // Sensitive clocks
    } else if any(&["nrst", "reset", "boot0", "pwrkey", "sw1"]) {
        2 // MCU escapes, reset & debug
    } else if any(&[
        "gnd", "vcc", "vdd", "vout", "vin", "3v3", "5v", "vbat", "vbatt", "vbus", "vsys", "power",
        "vss", "0v", "vref",
    ]) {
        5 // Power & high-current: route AFTER signals so wide traces don't block signal channels
    } else if any(&["sda", "scl", "mosi", "miso", "sck", "_cs", "tx", "rx"]) {
        3 // Buses & serial interfaces
    } else {
        4 // General signals
    }
}

/// Priority class for routing order. Lower number = routed
/// first. Net-name only fallback.
fn priority_class(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if lower.contains("ant")
        || lower.contains("rf_")
        || lower.contains("balun")
        || lower.contains("unbal")
        || lower.contains("_dp")
        || lower.contains("_dn")
        || lower.contains("diff")
    {
        0 // RF feed, antenna, balun, diff pairs
    } else if lower.contains("clk") || lower.contains("osc") || lower.contains("xtal") {
        1 // Sensitive clocks
    } else if lower.contains("nrst")
        || lower.contains("reset")
        || lower.contains("boot0")
        || lower.contains("pwrkey")
        || lower.contains("sw1")
    {
        2 // MCU escapes, reset & debug
    } else if lower.contains("sda")
        || lower.contains("scl")
        || lower.contains("mosi")
        || lower.contains("miso")
        || lower.contains("sck")
        || lower.contains("_cs")
        || lower.contains("tx")
        || lower.contains("rx")
    {
        3 // Buses & serial interfaces
    } else if matches!(
        lower.as_str(),
        "gnd" | "vcc" | "vdd" | "vbus" | "vout" | "vin" | "3v3" | "5v" | "vbatt" | "bat"
    ) || lower.contains("vbus")
        || lower.contains("vbatt")
        || lower.contains("power")
    {
        5 // Power & high-current: route after signals so wide traces don't block signal channels
    } else {
        4 // General signals
    }
}

#[cfg(test)]
mod tests {
    use super::emit_segments_and_vias;
    use crate::grid::{Cell, Grid};
    use crate::Segment;
    use std::collections::HashMap;
    use synth_geometry::{Layer, Point, Rect};
    use synth_ir::NetId;

    /// A minimal 2-layer grid with no pads, so `emit_segments_and_vias`
    /// snaps every vertex to the pure grid centre (no pad centroids).
    fn bare_grid() -> Grid {
        let w = 24;
        let h = 24;
        Grid {
            width: w,
            height: h,
            layers: 2,
            origin_nm: Point::new(0, 0),
            pitch_nm: 500_000,
            cells: vec![Cell::Free; 2 * w * h],
            pad_centres: HashMap::new(),
            pads: Vec::new(),
            npth_holes: Vec::new(),
            board_outline: Rect::new(
                Point::new(0, 0),
                Point::new(w as i64 * 500_000, h as i64 * 500_000),
            ),
        }
    }

    /// A path that runs on layer 0 and crosses to layer 1 at the
    /// SAME grid cell `[(0,12,10) -> (1,12,10)]`. The layer
    /// transition is a via; both layers' tracks must meet exactly
    /// at that cell so the via connects them. Before the
    /// layer-aware dedup fix the two same-coordinate cells were
    /// merged, dropping the transition vertex and leaving the
    /// upper-layer track starting a full pitch away -> a dangling
    /// via / track (`E-KICAD-DRC-track_dangling` /
    /// `E-KICAD-DRC-via_dangling`).
    #[test]
    fn via_step_same_cell_keeps_transition_connected() {
        let grid = bare_grid();
        let path: Vec<(usize, usize, usize)> = vec![
            (0, 10, 10),
            (0, 11, 10),
            (0, 12, 10),
            (1, 12, 10),
            (1, 13, 10),
            (1, 14, 10),
        ];
        let (segments, vias) = emit_segments_and_vias(&grid, &path, NetId(0), 127_000, 127_000);

        // Exactly one via at the transition cell (12, 10).
        assert_eq!(
            vias.len(),
            1,
            "expected a single via for the layer transition"
        );
        let via_pos = Point::new(12 * 500_000, 10 * 500_000);
        assert_eq!(vias[0].at, via_pos, "via must sit on the transition cell");

        // The layer-0 track must END on the via, and the layer-1
        // track must START on the via -- no gap on either side.
        let top_ends_on_via = segments
            .iter()
            .any(|s: &Segment| s.layer == Layer::Top && s.end == via_pos);
        let bottom_starts_on_via = segments
            .iter()
            .any(|s: &Segment| s.layer == Layer::Bottom && s.start == via_pos);
        assert!(
            top_ends_on_via,
            "top-layer segment must terminate at the via (no dangling stub)"
        );
        assert!(
            bottom_starts_on_via,
            "bottom-layer segment must start at the via (no dangling via)"
        );
    }
}
