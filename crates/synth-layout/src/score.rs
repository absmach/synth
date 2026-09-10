// SPDX-License-Identifier: Apache-2.0
//! Deterministic layout quality metrics (§7.8.7 of synth_implementation_plan.md).

use serde::{Deserialize, Serialize};
use synth_ir::{Board, NetId};

use crate::Layout;

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
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            components: Vec::new(),
            nets: Vec::new(),
            diff_pairs: Vec::new(),
            keepouts: Vec::new(),
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
