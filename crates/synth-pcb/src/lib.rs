// SPDX-License-Identifier: Apache-2.0

//! Copper geometry, as a value.
//!
//! These types are the shape every surface agrees on: the exporter writes
//! them into a `.kicad_pcb`, the DRC engine checks them, and an external
//! router's output is read back into them.
//!
//! They deliberately live apart from any router implementation. The
//! external routers are the producers of this shape, so a consumer that only needs to describe
//! copper should not have to depend on the code that produces it. Keeping
//! the value separate from the producer is what lets the production export
//! path emit an un-routed board without pulling in a router it will not use.
//!
//! An empty [`Routing`] is therefore a normal, meaningful value — it is
//! what an un-routed export carries — not a placeholder for a router that
//! has not run yet.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use synth_diagnostics::{Diagnostic, DiagnosticBuilder, Location, Severity, Span};
use synth_geometry::{Layer, Point};
use synth_ir::NetId;

/// A single straight copper segment on one layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Segment {
    pub net: NetId,
    pub layer: Layer,
    pub start: Point,
    pub end: Point,
    pub width_nm: i64,
}

/// A through-hole via at a grid intersection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Via {
    pub net: NetId,
    pub at: Point,
    pub drill_nm: i64,
    pub pad_diameter_nm: i64,
}

/// Per-pair length / skew metric attached to a [`Routing`]. One entry per
/// resolved differential pair, in IR declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairReport {
    pub positive: NetId,
    pub negative: NetId,
    /// Total trace length on the positive half, in nm. `0` when the half
    /// carries no copper.
    pub positive_length_nm: i64,
    /// Total trace length on the negative half, in nm.
    pub negative_length_nm: i64,
    /// Absolute skew between the two halves, in nm.
    pub skew_nm: i64,
}

/// One net that carries no end-to-end copper.
///
/// Carries enough context to explain the failure: the net's id and name,
/// its declaration span, and one pad pair that could not be joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnroutedNet {
    pub net: NetId,
    pub net_name: String,
    pub source_span: Span,
    /// Pad the search started from, in nm.
    pub source_pad_nm: Point,
    /// Pad the search could not reach, in nm.
    pub target_pad_nm: Point,
}

/// A complete copper description.
///
/// Produced by a router, consumed by the exporter and the DRC engine. The
/// statistics a producer records about its own search are kept alongside
/// the geometry but are never the same thing as the geometry: an engine
/// reporting `unrouted_nets: 0` has not thereby proven that every net is
/// connected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub segments: Vec<Segment>,
    pub vias: Vec<Via>,
    /// Per-differential-pair length and skew metrics.
    #[serde(default)]
    pub diff_pair_reports: Vec<PairReport>,
    /// Nets the producer could not route end to end.
    #[serde(default)]
    pub unrouted_nets: Vec<UnroutedNet>,
    /// Number of search cells the producer evaluated.
    #[serde(default)]
    pub cells_expanded: u64,
    /// Paths the producer found and then discarded because the emitted
    /// geometry ran too close to a foreign pad.
    ///
    /// This separates the two ways a net fails, which look identical from
    /// the outside and need opposite responses: a net that never finds a
    /// path is blocked by congestion, while a net that repeatedly finds one
    /// and has it rejected cannot escape its own pads.
    #[serde(default)]
    pub pad_escape_rejections: usize,
}

/// Dijkstra from `start`, returning the distance to every node it reaches.
fn distances(
    graph: &CopperGraph,
    start: (i64, i64),
) -> std::collections::BTreeMap<(i64, i64), i64> {
    use std::cmp::Reverse;
    use std::collections::{BTreeMap, BinaryHeap};
    let mut dist: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    let mut heap: BinaryHeap<Reverse<(i64, (i64, i64))>> = BinaryHeap::new();
    dist.insert(start, 0);
    heap.push(Reverse((0, start)));
    while let Some(Reverse((distance, node))) = heap.pop() {
        if dist.get(&node).is_some_and(|known| *known < distance) {
            continue;
        }
        for (next, weight) in graph.get(&node).into_iter().flatten() {
            let candidate = distance.saturating_add(*weight);
            if dist.get(next).is_none_or(|known| candidate < *known) {
                dist.insert(*next, candidate);
                heap.push(Reverse((candidate, *next)));
            }
        }
    }
    dist
}

/// The longest distance Dijkstra from `start` finds, in nm.
fn longest_from(graph: &CopperGraph, start: (i64, i64)) -> i64 {
    distances(graph, start).into_values().max().unwrap_or(0)
}

/// The node `start` is farthest from, by the shortest-path metric.
fn farthest_node(graph: &CopperGraph, start: (i64, i64)) -> (i64, i64) {
    distances(graph, start)
        .into_iter()
        .max_by_key(|(node, distance)| (*distance, *node))
        .map_or(start, |(node, _distance)| node)
}

/// Undirected point-to-point copper graph of one net, with Euclidean weights.
type CopperGraph = std::collections::BTreeMap<(i64, i64), Vec<((i64, i64), i64)>>;

impl Routing {
    /// True when nothing was routed.
    ///
    /// Meaningful for an un-routed export, not only for a failed attempt.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.vias.is_empty()
    }

    /// Length of the longest routed path on `net`, in nm.
    ///
    /// A delay budget is about the path from a driver to the farthest load,
    /// not the sum of every branch: a T-shaped clock with two loads has two
    /// paths, and the longer one is the delay. The copper is treated as an
    /// undirected weighted graph of segment endpoints; the result is its
    /// diameter. Physical (Euclidean) length is used, so diagonal copper is
    /// not overstated by Manhattan arithmetic.
    ///
    /// Two farthest-point sweeps are the exact diameter of a tree, which is
    /// what normal routing is. Copper can also close a loop or split into
    /// more than one island, and there a sweep from one node can miss a
    /// longer shortest path the sweep never starts from — so a graph that is
    /// not a single tree is measured from every node instead.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn length_for_net(&self, net: NetId) -> i64 {
        let mut graph: CopperGraph = std::collections::BTreeMap::new();
        for segment in self.segments.iter().filter(|s| s.net == net) {
            let a = (segment.start.x_nm, segment.start.y_nm);
            let b = (segment.end.x_nm, segment.end.y_nm);
            let dx = (b.0 - a.0) as f64;
            let dy = (b.1 - a.1) as f64;
            let length = dx.hypot(dy).round() as i64;
            graph.entry(a).or_default().push((b, length));
            graph.entry(b).or_default().push((a, length));
        }
        let Some(start) = graph.keys().next().copied() else {
            return 0;
        };
        // An undirected graph is a single spanning tree exactly when it
        // holds `nodes - 1` edges and one Dijkstra reaches every node; two
        // sweeps are its exact diameter. Anything else — a loop, or more
        // than one island — is measured from every node, because a sweep
        // can miss a longest shortest path it never starts from.
        let nodes = graph.len();
        let edges = graph.values().map(Vec::len).sum::<usize>() / 2;
        let reached = distances(&graph, start).len();
        if edges == nodes.saturating_sub(1) && reached == nodes {
            let far = farthest_node(&graph, start);
            return longest_from(&graph, far);
        }
        graph
            .keys()
            .copied()
            .map(|node| longest_from(&graph, node))
            .max()
            .unwrap_or(0)
    }

    /// Convert every [`UnroutedNet`] into an `E-SYNTH-ROUTE-001` diagnostic.
    #[must_use]
    pub fn to_diagnostics(&self, file: &str) -> Vec<Diagnostic> {
        self.unrouted_nets
            .iter()
            .map(|u| {
                DiagnosticBuilder::new(
                    "E-SYNTH-ROUTE-001",
                    Severity::Error,
                    format!("Net `{}` could not be routed", u.net_name),
                )
                .message(format!(
                    "No path joins the pad at ({:.2}, {:.2}) mm to the pad at \
                     ({:.2}, {:.2}) mm on net `{}`.",
                    synth_geometry::nm_to_mm(u.source_pad_nm.x_nm),
                    synth_geometry::nm_to_mm(u.source_pad_nm.y_nm),
                    synth_geometry::nm_to_mm(u.target_pad_nm.x_nm),
                    synth_geometry::nm_to_mm(u.target_pad_nm.y_nm),
                    u.net_name,
                ))
                .location(Location::from_span(file, u.source_span))
                .build()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrouted_export_is_a_normal_empty_routing() {
        // This is the value the export boundary emits, so it has to be
        // meaningful rather than a stand-in for "not computed yet".
        let routing = Routing::default();
        assert!(routing.is_empty());
        assert!(routing.unrouted_nets.is_empty());
        assert!(routing.to_diagnostics("board.synth").is_empty());
    }

    #[test]
    fn a_diagonal_track_is_its_physical_length_not_manhattan() {
        let routing = Routing {
            segments: vec![Segment {
                net: NetId(1),
                layer: Layer::Top,
                start: Point::new(0, 0),
                end: Point::new(3_000_000, 4_000_000),
                width_nm: 127_000,
            }],
            ..Routing::default()
        };
        assert_eq!(routing.length_for_net(NetId(1)), 5_000_000);
    }

    #[test]
    fn a_branched_net_is_measured_by_its_longest_path_not_its_sum() {
        // 0 -> 10 mm -> 20 mm (20 mm path), with a 5 mm spur at the middle.
        // Summing every branch would report 25 mm; the delay is 20 mm.
        let routing = Routing {
            segments: vec![
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(0, 0),
                    end: Point::new(10_000_000, 0),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(10_000_000, 0),
                    end: Point::new(20_000_000, 0),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(10_000_000, 0),
                    end: Point::new(10_000_000, 5_000_000),
                    width_nm: 127_000,
                },
            ],
            ..Routing::default()
        };
        assert_eq!(routing.length_for_net(NetId(1)), 20_000_000);
    }

    #[test]
    fn a_looped_net_is_measured_by_the_longest_path_the_loop_carries() {
        // A square loop of 10 mm sides with a diagonal chord: the longest
        // shortest path runs from one far corner to the other, 20 mm along
        // the loop's rim. Sweeps from a corner only ever reach the chord's
        // far end, 14.14 mm, which would understate the delay the loop
        // carries.
        let routing = Routing {
            segments: vec![
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(0, 0),
                    end: Point::new(10_000_000, 0),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(10_000_000, 0),
                    end: Point::new(10_000_000, 10_000_000),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(10_000_000, 10_000_000),
                    end: Point::new(0, 10_000_000),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(0, 10_000_000),
                    end: Point::new(0, 0),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(0, 0),
                    end: Point::new(10_000_000, 10_000_000),
                    width_nm: 127_000,
                },
            ],
            ..Routing::default()
        };
        assert_eq!(routing.length_for_net(NetId(1)), 20_000_000);
    }

    #[test]
    fn length_is_attributed_per_net() {
        let routing = Routing {
            segments: vec![
                Segment {
                    net: NetId(1),
                    layer: Layer::Top,
                    start: Point::new(0, 0),
                    end: Point::new(3_000_000, 0),
                    width_nm: 127_000,
                },
                Segment {
                    net: NetId(2),
                    layer: Layer::Top,
                    start: Point::new(0, 0),
                    end: Point::new(0, 1_000_000),
                    width_nm: 127_000,
                },
            ],
            ..Routing::default()
        };
        assert_eq!(routing.length_for_net(NetId(1)), 3_000_000);
        assert_eq!(routing.length_for_net(NetId(2)), 1_000_000);
        assert!(!routing.is_empty());
    }

    #[test]
    fn every_unrouted_net_becomes_a_blocking_diagnostic() {
        let routing = Routing {
            unrouted_nets: vec![UnroutedNet {
                net: NetId(7),
                net_name: "SDA".to_string(),
                source_span: Span::new(10, 20),
                source_pad_nm: Point::new(1_000_000, 2_000_000),
                target_pad_nm: Point::new(3_000_000, 4_000_000),
            }],
            ..Routing::default()
        };
        let diagnostics = routing.to_diagnostics("board.synth");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E-SYNTH-ROUTE-001");
        assert!(diagnostics[0].severity.is_blocking());
    }

    #[test]
    fn the_shape_round_trips_through_json() {
        let routing = Routing {
            segments: vec![Segment {
                net: NetId(3),
                layer: Layer::Bottom,
                start: Point::new(0, 0),
                end: Point::new(1_000_000, 0),
                width_nm: 200_000,
            }],
            vias: vec![Via {
                net: NetId(3),
                at: Point::new(1_000_000, 0),
                drill_nm: 300_000,
                pad_diameter_nm: 600_000,
            }],
            ..Routing::default()
        };
        let text = serde_json::to_string(&routing).expect("serialise");
        assert_eq!(
            serde_json::from_str::<Routing>(&text).expect("deserialise"),
            routing
        );
    }
}
