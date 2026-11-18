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

impl Routing {
    /// True when nothing was routed.
    ///
    /// Meaningful for an un-routed export, not only for a failed attempt.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.vias.is_empty()
    }

    /// Sum of segment lengths attributed to `net`, in nm.
    #[must_use]
    pub fn length_for_net(&self, net: NetId) -> i64 {
        self.segments
            .iter()
            .filter(|s| s.net == net)
            .map(|s| {
                let dx = (s.end.x_nm - s.start.x_nm).abs();
                let dy = (s.end.y_nm - s.start.y_nm).abs();
                dx + dy
            })
            .sum()
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
