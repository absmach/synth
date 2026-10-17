// SPDX-License-Identifier: Apache-2.0
//! Deterministic layout quality metrics (§7.8.7 of synth_implementation_plan.md).

use serde::{Deserialize, Serialize};
use synth_ir::{Board, NetId};

use crate::{ComponentPlacement, Layout};

/// Deterministic quality metrics computed over a [`Layout`].
///
/// Two consumers, per §7.8.7: a CI gate that compares against a
/// checked-in baseline, and candidate ranking once a second
/// placer-style implementation exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutScore {
    /// Count of distinct segment-segment crossings between wires on
    /// different nets. Shared endpoints (junctions) don't count.
    pub crossing_count: u32,
    /// Sum of Euclidean segment lengths across every wire, in mm.
    pub total_wire_length_mm: f64,
    /// Number of net-label stubs (`Layout::net_labels.len()`).
    pub label_stub_count: u32,
    /// `E-SYNTH-SCHEM-001..007` aesthetic rule violations. Populated
    /// by `synth_kicad::schem_erc::check` (§7.7.7); empty when the
    /// sheet is clean.
    pub aesthetic_violations: Vec<String>,
}

/// A single orthogonal wire segment, tagged with the net it belongs
/// to so crossing detection can skip same-net (junction) pairs.
struct Segment {
    net: NetId,
    p1: (f64, f64),
    p2: (f64, f64),
}

/// A quality measure computable from component positions alone, with
/// no routing.
///
/// [`score`] needs a routed [`Layout`], so it cannot be used to choose
/// a *placement* — the placer runs long before `route_and_label` has
/// drawn a single wire. This is the metric a placement search can
/// actually minimise.
///
/// **It has no consumer yet.** It was built to rank candidates in
/// `place_clusters`' fitting sweep, and inserting it there was measured
/// to change nothing across the 52-design corpus: the sweep's surviving
/// candidates are three byte-identical arrangements (on an ungrouped
/// board `region_w` is inert, since there is only one region to
/// shelf-pack), so there is nothing for a cost to choose between. It
/// is kept because it is the measuring stick the *next* step needs —
/// either widening the sweep so the ordering actually varies, or the
/// schematic-side repair loop the PCB side already has. See the
/// "KNOWN LIMIT" comment on the selection key in `lib.rs`.
///
/// Two terms, both read off a column-major left-to-right layout:
///
/// - [`PlacementCost::net_span_mm`] — half-perimeter length, i.e. how
///   far the wires will have to run. A net whose endpoints sit far
///   apart in both axes is a net that either gets a very long wire or
///   degrades into a label stub, and both are bad.
/// - [`PlacementCost::net_shear_mm`] — how far a net's endpoints
///   deviate from their own mean `y`. In a columnar layout a net has
///   to travel vertically whenever its endpoints disagree on `y`, and
///   vertical travel is what crosses other nets' vertical runs. This
///   is the term that predicts crossings from positions alone.
///
/// Power and ground rails are excluded. They render as power symbols
/// rather than wires (`classify_power_flags`), so their span costs the
/// reader nothing — the same reason `build_cluster_adjacency` weights
/// them at `WEAK_NET_WEIGHT` instead of `STRONG_NET_WEIGHT`. The
/// router mirrors this: `classify_net_labels` skips power nets before
/// it ever measures a span.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlacementCost {
    /// Weighted sum of per-net half-perimeter length, millimetres.
    pub net_span_mm: f64,
    /// Weighted sum of per-endpoint `|y - mean(y)|`, millimetres.
    pub net_shear_mm: f64,
}

impl PlacementCost {
    /// A single scalar to rank candidate placements by, lower is
    /// better.
    ///
    /// `SHEAR_WEIGHT` is 2.0: a millimetre of shear is worse for
    /// readability than a millimetre of span, because span is at least
    /// visible as a long straight run on the sheet, whereas shear is
    /// what produces the crossings `E-SYNTH-SCHEM-002` reports.
    pub fn total(&self) -> f64 {
        const SHEAR_WEIGHT: f64 = 2.0;
        self.net_span_mm + SHEAR_WEIGHT * self.net_shear_mm
    }
}

/// Whether a net is a power/ground rail, which renders as a power
/// symbol rather than a wire and is therefore excluded from
/// [`placement_cost`].
///
/// Deliberately the same weak/strong test the barycenter ordering
/// uses (`net_is_weak` in the crate root): any endpoint on a
/// `PowerInput`/`PowerOutput` pin, or on a pin whose name marks a
/// ground rail. Components without a resolved part have no power pins
/// and so default to *signal* — the conservative choice, since an
/// unrecognised component's nets are more likely functional.
fn is_rail(board: &Board, net: &synth_ir::Net) -> bool {
    use synth_registry::ElectricalType;
    net.endpoints.iter().any(|endpoint| {
        let Some(pin) = board.pin(endpoint.component, endpoint.pin) else {
            return false;
        };
        let name = pin.name.to_ascii_lowercase();
        let ground_named = matches!(
            name.as_str(),
            "gnd" | "vss" | "vssa" | "gnda" | "vee" | "vneg" | "agnd" | "dgnd"
        );
        ground_named
            || matches!(
                pin.electrical_type,
                ElectricalType::PowerInput
                    | ElectricalType::PowerOutput
                    | ElectricalType::GroundReference
            )
    })
}

/// Computes [`PlacementCost`] for a set of component placements.
///
/// `placements` need not be the final layout — only the component
/// centres matter. Nets whose endpoints all resolve to placed
/// components are measured; a net with a single placed endpoint, or
/// none, contributes nothing (it cannot be drawn).
pub fn placement_cost(board: &Board, placements: &[ComponentPlacement]) -> PlacementCost {
    use std::collections::HashMap;

    let centres: HashMap<synth_ir::ComponentId, (f64, f64)> =
        placements.iter().map(|p| (p.id, p.center_mm)).collect();

    let mut net_span_mm = 0.0;
    let mut net_shear_mm = 0.0;

    for net in &board.nets {
        if is_rail(board, net) {
            continue;
        }
        // Distinct placed components on this net, ordered by x then y
        // so the half-perimeter walk and the mean are both independent
        // of the order `board.nets` happens to be stored in.
        let mut points: Vec<(f64, f64)> = net
            .endpoints
            .iter()
            .filter_map(|ep| centres.get(&ep.component).copied())
            .collect();
        points.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        points.dedup();
        if points.len() < 2 {
            continue;
        }

        // Half-perimeter: walk the ordered points and sum the
        // Manhattan step. `fold` over windows(2) so a 2-endpoint net
        // measures its single gap.
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            net_span_mm += (b.0 - a.0).abs() + (b.1 - a.1).abs();
        }

        // Shear: total deviation from the mean y. `f64` accumulation
        // over a handful of points is deterministic (same operands,
        // same order — `points` is sorted), so no epsilon tricks are
        // needed for reproducibility.
        let mean_y = points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64;
        for p in &points {
            net_shear_mm += (p.1 - mean_y).abs();
        }
    }

    PlacementCost {
        net_span_mm,
        net_shear_mm,
    }
}

/// Computes deterministic quality metrics for `layout`.
///
/// `board` is accepted for parity with §7.8.7's signature and for
/// future aesthetic-rule checks that need IR context beyond the
/// layout itself; it is unused today because `aesthetic_violations`
/// is a placeholder.
pub fn score(layout: &Layout, board: &Board) -> LayoutScore {
    let _ = board;

    let mut total_wire_length_mm = 0.0;
    let mut segments: Vec<Segment> = Vec::new();

    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            let dx = p2.0 - p1.0;
            let dy = p2.1 - p1.1;
            total_wire_length_mm += dx.hypot(dy);
            segments.push(Segment {
                net: wire.net,
                p1,
                p2,
            });
        }
    }

    let mut crossing_count = 0u32;
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            let a = &segments[i];
            let b = &segments[j];
            if a.net == b.net {
                continue;
            }
            if segments_cross(a.p1, a.p2, b.p1, b.p2) {
                crossing_count += 1;
            }
        }
    }

    LayoutScore {
        crossing_count,
        total_wire_length_mm,
        label_stub_count: layout.net_labels.len() as u32,
        aesthetic_violations: Vec::new(),
    }
}

/// Coordinates are millimetres derived from grid-snapping arithmetic
/// (`(v / 2.54).round() * 2.54`), which can leave ~1e-13 mm of float
/// noise on a value that is conceptually exact. `f64::EPSILON`
/// (~2.2e-16) is too tight to absorb that; this tolerance is well
/// below the 2.54mm grid pitch so it can't misclassify two distinct
/// grid lines as the same one.
const COORD_EPSILON_MM: f64 = 1e-6;

/// Whether two axis-aligned segments properly cross — i.e. intersect
/// at a single point interior to both, not merely touch at a shared
/// endpoint or run collinear. `WirePath` segments are always
/// orthogonal (see its doc comment), so this only needs to handle
/// the horizontal/vertical case; two parallel segments (both
/// horizontal or both vertical) never count as a crossing here.
///
/// `pub(crate)`: also used by `crate::route` to decide whether a
/// net's routed wire crosses enough *other* nets' wires to be worth
/// truncating to a label instead (§7.7.3's "> 2 crossings" trigger).
pub(crate) fn segments_cross(
    a1: (f64, f64),
    a2: (f64, f64),
    b1: (f64, f64),
    b2: (f64, f64),
) -> bool {
    let a_horizontal = (a1.1 - a2.1).abs() < COORD_EPSILON_MM;
    let b_horizontal = (b1.1 - b2.1).abs() < COORD_EPSILON_MM;

    match (a_horizontal, b_horizontal) {
        (true, false) => crosses_h_v(a1, a2, b1, b2),
        (false, true) => crosses_h_v(b1, b2, a1, a2),
        _ => false,
    }
}

/// `h1`-`h2` is horizontal, `v1`-`v2` is vertical. True if they cross
/// at a point interior to both segments.
fn crosses_h_v(h1: (f64, f64), h2: (f64, f64), v1: (f64, f64), v2: (f64, f64)) -> bool {
    let y_h = h1.1;
    let (x_h_min, x_h_max) = (h1.0.min(h2.0), h1.0.max(h2.0));
    let x_v = v1.0;
    let (y_v_min, y_v_max) = (v1.1.min(v2.1), v1.1.max(v2.1));

    x_v > x_h_min && x_v < x_h_max && y_h > y_v_min && y_h < y_v_max
}

#[cfg(test)]
mod tests {
    use synth_diagnostics::Span;
    use synth_ir::NetId;

    use super::*;
    use crate::{SheetSize, WirePath};

    fn empty_board() -> Board {
        Board {
            schematic_overflow: None,
            schematic_paper: None,
            groups: Vec::new(),
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components: Vec::new(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            notes: vec![],
            keepouts: Vec::new(),
            stackup: None,
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            required_processes: std::collections::BTreeSet::new(),
            source_span: Span::new(0, 0),
        }
    }

    fn layout_with_wires(wires: Vec<WirePath>) -> Layout {
        Layout {
            components: Vec::new(),
            wires,
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            annotations: Vec::new(),
            hierarchical_labels: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: SheetSize::A4,
        }
    }

    #[test]
    fn non_crossing_orthogonal_wires_score_zero_crossings() {
        // Two horizontal wires on different nets, stacked at
        // different y coordinates — never intersect.
        let layout = layout_with_wires(vec![
            WirePath {
                net: NetId(0),
                points: vec![(0.0, 0.0), (10.0, 0.0)],
                junctions: Vec::new(),
            },
            WirePath {
                net: NetId(1),
                points: vec![(0.0, 5.0), (10.0, 5.0)],
                junctions: Vec::new(),
            },
        ]);

        let result = score(&layout, &empty_board());
        assert_eq!(result.crossing_count, 0);
    }

    #[test]
    fn perpendicular_wires_form_clean_plus_crossing() {
        // A horizontal segment and a vertical segment on different
        // nets that cross through each other's interior.
        let layout = layout_with_wires(vec![
            WirePath {
                net: NetId(0),
                points: vec![(0.0, 5.0), (10.0, 5.0)],
                junctions: Vec::new(),
            },
            WirePath {
                net: NetId(1),
                points: vec![(5.0, 0.0), (5.0, 10.0)],
                junctions: Vec::new(),
            },
        ]);

        let result = score(&layout, &empty_board());
        assert_eq!(result.crossing_count, 1);
    }

    #[test]
    fn shared_endpoint_is_a_junction_not_a_crossing() {
        // Two wires (different nets) that meet exactly at a shared
        // endpoint — a T-junction, not a crossing.
        let layout = layout_with_wires(vec![
            WirePath {
                net: NetId(0),
                points: vec![(0.0, 0.0), (5.0, 0.0)],
                junctions: Vec::new(),
            },
            WirePath {
                net: NetId(1),
                points: vec![(5.0, 0.0), (5.0, 10.0)],
                junctions: Vec::new(),
            },
        ]);

        let result = score(&layout, &empty_board());
        assert_eq!(result.crossing_count, 0);
    }

    #[test]
    fn wire_length_sums_across_l_shaped_segments() {
        // L-shaped wire: 3 mm right, then 4 mm down => 7 mm total.
        let layout = layout_with_wires(vec![WirePath {
            net: NetId(0),
            points: vec![(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)],
            junctions: Vec::new(),
        }]);

        let result = score(&layout, &empty_board());
        assert!((result.total_wire_length_mm - 7.0).abs() < 1e-9);
        assert_eq!(result.label_stub_count, 0);
        assert!(result.aesthetic_violations.is_empty());
    }
}
