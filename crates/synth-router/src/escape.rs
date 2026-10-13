// SPDX-License-Identifier: Apache-2.0

//! Pad-escape feasibility: can this package be fanned out at the fab floor?
//!
//! Borrowed from TraceMaker's `escape` command, whose framing is the useful
//! part: a fine-pitch package can be *unroutable at the chosen process* long
//! before a router runs, and the failure presents as mysterious necked-down
//! vias rather than as a clear "this cannot escape" answer. Running an
//! authoritative router for two minutes only to learn that the MCU's pins
//! cannot take a fab-floor via is time spent not knowing something we could
//! have computed in milliseconds.
//!
//! This is a *pre-routing* warning, deliberately conservative and deliberately
//! not a gate. A router can still complete such a board by staggering vias,
//! necking tracks, or using via-in-pad when the process allows. What it cannot
//! do is change the arithmetic, so the finding names the arithmetic: the pitch
//! the package offers, the via the floor requires, and the drill that would
//! actually fit.

// Board geometry spans at most ~1e9 nm, far inside f64's 2^52 exact-integer
// range, so every nm<->mm conversion here is exact rather than lossy.
#![allow(clippy::cast_precision_loss)]

use serde::{Deserialize, Serialize};

use crate::contract::FabricationPolicy;
use crate::pcb_read::{Pad, PcbBoard};

/// A footprint is treated as a "dense package" at or above this copper-pad
/// count. Two-pin passives are never dense however close their pads are.
const DENSE_MIN_PADS: usize = 8;

/// Packages coarser than this never need the analysis: at 1.3 mm pitch a
/// fab-floor via fits between adjacent pins with room to spare.
const DENSE_MAX_PITCH_MM: f64 = 1.3;

const NM_PER_MM: f64 = 1_000_000.0;

/// Copper layers a board needs before an inner plane can rescue a dense
/// package's escapes. Two-layer boards have nowhere for a fine-pitch pin to
/// go; four give it a plane to change into.
const PLANE_LAYER_COUNT: usize = 4;

/// Pairs closer than this are coincident, not a pitch.
///
/// Connectors routinely model the same electrical pin twice — primary and
/// mirrored power/data pads for full coverage. Those duplicates sit on top of
/// each other, so a raw "closest pair" reads 0 mm and every such part looks
/// unescapable. They are not a routing barrier; they are one pad.
const COINCIDENT_EPS_NM: i64 = 50_000;

/// What stops a package's pins from reaching the rest of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscapeLimit {
    /// A fab-floor via cannot sit on adjacent pins without breaking clearance.
    /// The usual fine-pitch case: the board routes, but only with vias below
    /// the declared drill floor or with via-in-pad.
    Via,
    /// Not even a fabrication-minimum track passes between adjacent pads, so
    /// they cannot escape on their own layer either.
    Track,
}

/// One dense package that cannot be fanned out at the fabrication floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageEscape {
    /// Reference designator, e.g. `U2`.
    pub refdes: String,
    /// Footprint library id, e.g. `Package_QFP:LQFP-48_7x7mm_P0.5mm`.
    pub package: String,
    /// Copper pads on the footprint.
    pub pads: usize,
    /// Smallest centre-to-centre pad spacing, in millimetres.
    pub pitch_mm: f64,
    /// Smallest edge-to-edge pad spacing, in millimetres.
    pub gap_mm: f64,
    /// Centre-to-centre spacing two fab-floor vias need, in millimetres.
    pub via_span_mm: f64,
    /// The largest via drill whose escape vias clear each other at this pitch,
    /// in millimetres. Negative when even a zero-drill via cannot.
    pub max_drill_mm: f64,
    /// Copper layers the board actually has.
    pub layers: usize,
    /// The layer count that would give this package somewhere to escape, when
    /// the board is too thin for one. `None` when it already has enough.
    pub suggested_layers: Option<usize>,
    /// Which limit the package hits.
    pub limit: EscapeLimit,
    /// The arithmetic, in words.
    pub detail: String,
    /// What to change.
    pub remediation: String,
}

impl PackageEscape {
    /// A one-line summary for a terminal.
    #[must_use]
    pub fn summary_line(&self) -> String {
        format!(
            "{} ({}, {} pads, {:.3} mm pitch): {}",
            self.refdes, self.package, self.pads, self.pitch_mm, self.detail
        )
    }
}

/// Analyse the un-routed baseline a request names.
///
/// A missing or unreadable baseline yields no findings rather than an error:
/// the escape analysis is advisory, and the run proper already reports a
/// baseline it cannot read.
#[must_use]
pub fn analyse_baseline(request: &crate::contract::RouteRequest) -> Vec<PackageEscape> {
    let Ok(text) = std::fs::read_to_string(request.baseline_path()) else {
        return Vec::new();
    };
    match PcbBoard::parse(&text) {
        Ok(board) => analyse(&board, &request.policy),
        Err(_) => Vec::new(),
    }
}

/// Analyse every dense package on the board against the fabrication floor.
///
/// Returns one finding per package that cannot be escaped at the floor, worst
/// first. An empty result means every package can be fanned out.
#[must_use]
pub fn analyse(board: &PcbBoard, policy: &FabricationPolicy) -> Vec<PackageEscape> {
    let layers = board.copper_layers().len();
    // Two parallel tracks need their centres `width + clearance` apart; the
    // same holds for two vias, where the width is the via's copper diameter.
    let track_span_nm = policy.min_track_width_nm + policy.min_clearance_nm;
    let via_span_nm =
        policy.min_drill_diameter_nm + 2 * policy.min_annular_ring_nm + policy.min_clearance_nm;

    let mut findings: Vec<PackageEscape> = board
        .footprints
        .iter()
        .filter_map(|footprint| {
            let copper: Vec<&Pad> = footprint
                .pads
                .iter()
                .filter(|pad| pad.layers.iter().any(|layer| layer.ends_with(".Cu")))
                .collect();
            if copper.len() < DENSE_MIN_PADS {
                return None;
            }
            let (pitch_nm, gap_nm) = closest_pair_nm(&copper)?;
            if pitch_nm > mm(DENSE_MAX_PITCH_MM) {
                return None;
            }
            let via_blocked = pitch_nm < via_span_nm;
            let track_blocked = gap_nm < track_span_nm;
            if !via_blocked && !track_blocked {
                return None;
            }
            let max_drill_nm = pitch_nm - policy.min_clearance_nm - 2 * policy.min_annular_ring_nm;
            Some(PackageEscape {
                refdes: footprint.reference.clone(),
                package: footprint.lib_id.clone(),
                pads: copper.len(),
                pitch_mm: as_mm(pitch_nm),
                gap_mm: as_mm(gap_nm),
                via_span_mm: as_mm(via_span_nm),
                max_drill_mm: as_mm(max_drill_nm),
                layers,
                suggested_layers: (layers < PLANE_LAYER_COUNT).then_some(PLANE_LAYER_COUNT),
                limit: if track_blocked {
                    EscapeLimit::Track
                } else {
                    EscapeLimit::Via
                },
                detail: describe(pitch_nm, gap_nm, via_span_nm, track_span_nm, max_drill_nm),
                remediation: remediate(track_blocked, layers),
            })
        })
        .collect();

    // Worst first: a package that cannot even take a track outranks one that
    // merely forces a small via, and a finer pitch outranks a coarser one.
    findings.sort_by(|a, b| {
        let rank = |e: &PackageEscape| match e.limit {
            EscapeLimit::Track => 0,
            EscapeLimit::Via => 1,
        };
        rank(a)
            .cmp(&rank(b))
            .then(a.pitch_mm.total_cmp(&b.pitch_mm))
            .then(a.refdes.cmp(&b.refdes))
    });
    findings
}

/// [`grid_arrays`] for the request's exported baseline board.
///
/// Empty for a baseline that cannot be read. That is only safe because the
/// caller that acts on it refuses such a baseline first; see
/// [`read_baseline`].
#[must_use]
pub fn grid_arrays_in_baseline(request: &crate::contract::RouteRequest) -> Vec<String> {
    read_baseline(request)
        .map(|board| grid_arrays(&board))
        .unwrap_or_default()
}

/// The request's exported baseline board, or why it cannot be read.
///
/// # Errors
/// Returns a description naming the path when the baseline is missing,
/// unreadable, or does not parse as a board.
pub fn read_baseline(request: &crate::contract::RouteRequest) -> Result<PcbBoard, String> {
    let path = request.baseline_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("the baseline {} cannot be read: {error}", path.display()))?;
    PcbBoard::parse(&text)
        .map_err(|error| format!("the baseline {} does not parse: {error}", path.display()))
}

/// Packages worth handing to a dedicated escape engine: dense grid arrays.
///
/// Returned in reference order, as reference designators.
///
/// Deliberately a different question from [`analyse`]. That one asks whether a
/// package *cannot* be fanned out at the fabrication floor, which the iCE40's
/// 0.8 mm grid passes — 0.8 mm clears the 0.687 mm two vias need between
/// centres — so driving an escape engine from its findings fans out the 0.5 mm
/// connectors and skips the ball grid, which is backwards. What makes a
/// package need a grid escape is being a grid: a connector is one or two rows
/// of pads and a BGA is a lattice, so three distinct rows *and* three distinct
/// columns separates them without naming packages or trusting a library id.
#[must_use]
pub fn grid_arrays(board: &PcbBoard) -> Vec<String> {
    /// Rows and columns a lattice needs before it is one.
    const MIN_LINES: usize = 3;
    /// Pad centres within this distance share a row or a column. A ball grid
    /// is laid out on an exact pitch, so this only has to absorb rounding.
    const LINE_EPS_NM: i64 = 50_000;

    let lines = |values: &mut Vec<i64>| {
        values.sort_unstable();
        values.dedup_by(|a, b| (*a - *b).abs() <= LINE_EPS_NM);
        values.len()
    };

    board
        .footprints
        .iter()
        .filter_map(|footprint| {
            let copper: Vec<&Pad> = footprint
                .pads
                .iter()
                .filter(|pad| pad.layers.iter().any(|layer| layer.ends_with(".Cu")))
                .collect();
            if copper.len() < DENSE_MIN_PADS {
                return None;
            }
            let pitch_nm = min_pitch_nm(&copper)?;
            if pitch_nm > mm(DENSE_MAX_PITCH_MM) {
                return None;
            }
            let rows = lines(&mut copper.iter().map(|pad| pad.at.y_nm).collect());
            let columns = lines(&mut copper.iter().map(|pad| pad.at.x_nm).collect());
            (rows >= MIN_LINES && columns >= MIN_LINES).then(|| footprint.reference.clone())
        })
        .collect()
}

/// Pitch at or under which a perimeter package is escaped before routing.
/// A TSSOP-20 at 0.65 mm leaves 0.25 mm between 0.4 mm pads, which a maze
/// router with 0.2 mm tracks and 0.127 mm clearance cannot always leave
/// through: the same board routed or failed on a 3 % change of scale.
const PERIMETER_MAX_PITCH_MM: f64 = 0.65;

/// Fine-pitch packages that are not lattices: dense rows of pads along the
/// edge of the body (QFN, QFP, TSSOP, fine-pitch connectors).
///
/// [`grid_arrays`] asks whether a package is a lattice, and a perimeter
/// package never is, so it reached the general router with every pad's escape
/// left to chance. The engine's surface stub fan needs no via and no
/// manufacturer allowance, so these are escaped on any board.
///
/// Returned in reference order, excluding the grid arrays.
#[must_use]
pub fn fine_pitch_perimeter(board: &PcbBoard) -> Vec<String> {
    let grids = grid_arrays(board);
    board
        .footprints
        .iter()
        .filter_map(|footprint| {
            let copper: Vec<&Pad> = footprint
                .pads
                .iter()
                .filter(|pad| pad.layers.iter().any(|layer| layer.ends_with(".Cu")))
                .collect();
            if copper.len() < DENSE_MIN_PADS || grids.contains(&footprint.reference) {
                return None;
            }
            let (pitch_nm, _) = closest_pair_nm(&copper)?;
            (pitch_nm <= mm(PERIMETER_MAX_PITCH_MM) + 1_000).then(|| footprint.reference.clone())
        })
        .collect()
}

/// [`fine_pitch_perimeter`] for the request's exported baseline board.
#[must_use]
pub fn fine_pitch_perimeter_in_baseline(request: &crate::contract::RouteRequest) -> Vec<String> {
    read_baseline(request)
        .map(|board| fine_pitch_perimeter(&board))
        .unwrap_or_default()
}

fn describe(
    pitch_nm: i64,
    gap_nm: i64,
    via_span_nm: i64,
    track_span_nm: i64,
    max_drill_nm: i64,
) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "adjacent pins are {:.3} mm apart and the pads leave {:.3} mm between them; \
         a fabrication-floor via needs {:.3} mm between centres",
        as_mm(pitch_nm),
        as_mm(gap_nm),
        as_mm(via_span_nm)
    );
    if max_drill_nm > 0 {
        let _ = write!(
            text,
            ", so the drill that would fit is {:.3} mm",
            as_mm(max_drill_nm)
        );
    } else {
        text.push_str("; no via fits between adjacent pins at all");
    }
    if gap_nm < track_span_nm {
        let _ = write!(
            text,
            "; even a minimum track needs {:.3} mm of gap",
            as_mm(track_span_nm)
        );
    }
    text
}

fn remediate(track_blocked: bool, layers: usize) -> String {
    use std::fmt::Write as _;
    let mut text = if track_blocked {
        "use via-in-pad if the process allows, or choose a coarser-pitch part".to_string()
    } else {
        "allow via-in-pad, accept a sub-floor via from the router, or choose a coarser-pitch part"
            .to_string()
    };
    if layers < PLANE_LAYER_COUNT {
        let _ = write!(
            text,
            "; this board has {layers} copper layers — an inner plane at {PLANE_LAYER_COUNT} \
             layers gives these pins somewhere to change layers"
        );
    }
    text
}

/// The smallest centre-to-centre distance between two *distinct* pad sites.
/// Centre-to-centre spacing and edge-to-edge clearance of the closest pair
/// of pads, which is the pair an escape has to pass between.
///
/// Both numbers come from the same pair on purpose. Measuring the tightest
/// gap anywhere in the footprint instead reported an HDMI receptacle as
/// "adjacent pins are 0.500 mm apart and the pads leave 0.000 mm between
/// them" — a sentence that cannot be true of 0.3 mm pads, because the gap
/// was between a signal pad and a mounting pad a row away while the pitch
/// was between two signal pins. The verdict was right and the arithmetic
/// supporting it was not, which is the worse half of a diagnostic.
fn closest_pair_nm(pads: &[&Pad]) -> Option<(i64, i64)> {
    let mut best: Option<(i64, i64)> = None;
    for (i, a) in pads.iter().enumerate() {
        for b in &pads[i + 1..] {
            let dx = a.at.x_nm - b.at.x_nm;
            let dy = a.at.y_nm - b.at.y_nm;
            #[allow(clippy::cast_possible_truncation)]
            let distance = ((dx * dx + dy * dy) as f64).sqrt().round() as i64;
            // Connectors routinely model one electrical pin as two coincident
            // pads; that is one pad, not a zero-millimetre pitch.
            if distance < COINCIDENT_EPS_NM {
                continue;
            }
            if best.is_none_or(|(pitch, _)| distance < pitch) {
                best = Some((distance, rect_gap_nm(a, b)));
            }
        }
    }
    best
}

fn rect_gap_nm(a: &Pad, b: &Pad) -> i64 {
    let (ax, ay) = (a.size_nm.0 / 2, a.size_nm.1 / 2);
    let (bx, by) = (b.size_nm.0 / 2, b.size_nm.1 / 2);
    let dx = (a.at.x_nm - b.at.x_nm).abs() - (ax + bx);
    let dy = (a.at.y_nm - b.at.y_nm).abs() - (ay + by);
    if dx > 0 && dy > 0 {
        // Diagonal separation: the true rect-to-rect distance.
        ((dx * dx + dy * dy) as f64).sqrt().round() as i64
    } else {
        dx.max(dy).max(0)
    }
}

fn mm(value: f64) -> i64 {
    (value * NM_PER_MM).round() as i64
}

fn as_mm(nm: i64) -> f64 {
    nm as f64 / NM_PER_MM
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    fn board(text: &str) -> PcbBoard {
        PcbBoard::parse(text).expect("fixture parses")
    }

    /// A generic dense package: `count` pads in one row at `pitch` mm, each
    /// `pad_w` x `pad_h` mm, from `origin`.
    fn row(count: usize, pitch: f64, pad_w: f64, pad_h: f64, lib: &str) -> String {
        let mut pads = String::new();
        for index in 0..count {
            let x = index as f64 * pitch;
            let _ = write!(
                pads,
                "    (pad \"{}\" smd roundrect (at {x:.4} 0) (size {pad_w:.3} {pad_h:.3})\n      \
                 (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (net {} \"N{}\"))\n",
                index + 1,
                index + 1,
                index + 1
            );
        }
        format!(
            "(kicad_pcb\n  (version 20260206)\n  (generator \"synth-eda\")\n  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n  (net 0 \"\")\n  (footprint \"{lib}\"\n    (layer \"F.Cu\")\n    (at 10 20)\n    (property \"Reference\" \"U1\" (at 0 -1) (layer \"F.SilkS\"))\n{pads}  )\n  (gr_line (start 0 0) (end 40 0) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 0 0) (end 0 30) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 40 0) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 0 30) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n)"
        )
    }

    /// A `rows` x `cols` ball grid at `pitch` mm, each ball `ball` mm across.
    fn grid(rows: usize, cols: usize, pitch: f64, ball: f64, lib: &str) -> String {
        let mut pads = String::new();
        for r in 0..rows {
            for c in 0..cols {
                let (x, y) = (c as f64 * pitch, r as f64 * pitch);
                let n = r * cols + c + 1;
                let _ = write!(
                    pads,
                    "    (pad \"{n}\" smd circle (at {x:.4} {y:.4}) (size {ball:.3} {ball:.3})\n      \
                     (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (net {n} \"N{n}\"))\n"
                );
            }
        }
        format!(
            "(kicad_pcb\n  (version 20260206)\n  (generator \"synth-eda\")\n  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))\n  (net 0 \"\")\n  (footprint \"{lib}\"\n    (layer \"F.Cu\")\n    (at 10 20)\n    (property \"Reference\" \"U1\" (at 0 -1) (layer \"F.SilkS\"))\n{pads}  )\n  (gr_line (start 0 0) (end 40 0) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 0 0) (end 0 30) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 40 0) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n  (gr_line (start 0 30) (end 40 30) (layer \"Edge.Cuts\") (width 0.1))\n)"
        )
    }

    /// The distinction the fanout wiring turns on, and the one that was got
    /// backwards first time: a 0.8 mm ball grid passes `analyse` because a via
    /// fits between its balls, and is exactly what needs a grid escape. A
    /// fine-pitch connector fails `analyse` and is not a grid at all.
    #[test]
    fn a_ball_grid_is_a_fanout_candidate_even_though_it_escapes_at_the_floor() {
        let board = board(&grid(11, 11, 0.8, 0.4, "Package_BGA:BGA-121_11x11"));

        assert!(
            analyse(&board, &FabricationPolicy::default()).is_empty(),
            "0.8 mm clears the floor's via span, so nothing is blocked"
        );
        assert_eq!(grid_arrays(&board), vec!["U1".to_string()]);
    }

    /// A TSSOP-20 (0.65 mm) is a row on each side of the body, not a lattice:
    /// it is escaped by the perimeter stage, and a coarser SOIC or a ball grid
    /// is not.
    #[test]
    fn a_fine_pitch_perimeter_package_is_escaped_and_a_coarse_one_is_not() {
        let tssop = board(&row(
            20,
            0.65,
            0.4,
            1.5,
            "Package_SO:TSSOP-20_4.4x6.5mm_P0.65mm",
        ));
        assert_eq!(fine_pitch_perimeter(&tssop), vec!["U1".to_string()]);
        assert!(grid_arrays(&tssop).is_empty(), "a row is not a lattice");

        let soic = board(&row(
            8,
            1.27,
            0.6,
            1.5,
            "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm",
        ));
        assert!(
            fine_pitch_perimeter(&soic).is_empty(),
            "1.27 mm needs no stub"
        );

        let bga = board(&grid(11, 11, 0.8, 0.4, "Package_BGA:BGA-121_11x11"));
        assert!(
            fine_pitch_perimeter(&bga).is_empty(),
            "a ball grid has its own stage"
        );

        let passive = board(&row(2, 0.5, 0.3, 0.3, "Resistor_SMD:R_0201"));
        assert!(
            fine_pitch_perimeter(&passive).is_empty(),
            "two pads are never dense"
        );
    }

    /// A single row is not a lattice however fine its pitch, so the pitch
    /// alone never makes something a fanout candidate.
    ///
    /// Note what this does *not* claim. A real USB-C or HDMI receptacle has
    /// three or more distinct pad rows once mounting and shield pads are
    /// counted, so it does qualify, and on the Class C board all four of
    /// J1/J2/U1/U2 are escaped. That is intended: they are dense pad fields,
    /// and escaping the two connectors measurably cut DRC errors. The
    /// criterion is "is this a lattice", not "is this a BGA".
    #[test]
    fn a_single_row_package_is_not_a_grid_however_fine_its_pitch() {
        let board = board(&row(12, 0.5, 0.23, 1.5, "Connector_USB:USB_C_Receptacle"));

        assert!(
            !analyse(&board, &FabricationPolicy::default()).is_empty(),
            "0.5 mm pitch cannot take a fab-floor via"
        );
        assert!(
            grid_arrays(&board).is_empty(),
            "one row of pads is not a lattice, whatever its pitch"
        );
    }

    #[test]
    fn a_coarse_grid_is_not_a_fanout_candidate() {
        // 2.54 mm through-hole header: a lattice, but nothing a dedicated
        // escape engine is needed for.
        let board = board(&grid(3, 4, 2.54, 1.0, "Connector_PinHeader:PinHeader_3x4"));

        assert!(grid_arrays(&board).is_empty());
    }

    #[test]
    fn a_zero_point_five_millimetre_package_cannot_take_a_fab_floor_via() {
        // 0.5 mm pitch with 0.23 mm pads: a minimum track fits between the
        // pads (0.27 mm gap), but a fab-floor via on adjacent pins does not.
        // This is the case that presents, at run time, as mysterious
        // sub-floor vias rather than as a clear diagnosis.
        let board = board(&row(12, 0.5, 0.23, 1.5, "Package_QFP:LQFP-48_7x7mm_P0.5mm"));
        let findings = analyse(&board, &FabricationPolicy::default());
        assert_eq!(findings.len(), 1, "{findings:?}");
        let escape = &findings[0];
        assert_eq!(escape.limit, EscapeLimit::Via, "{escape:?}");
        assert!((escape.pitch_mm - 0.5).abs() < 1e-6, "{escape:?}");
        assert!(
            escape.max_drill_mm < 0.3,
            "the drill that fits must be below the floor: {escape:?}"
        );
        assert!(escape.detail.contains("0.500"), "{escape:?}");
        assert!(!escape.remediation.is_empty());
        assert!(escape.summary_line().contains("U1"));
        // The fixture is a 2-layer board, so the fix must name the layer count.
        assert_eq!(escape.layers, 2, "{escape:?}");
        assert_eq!(escape.suggested_layers, Some(4), "{escape:?}");
        assert!(escape.remediation.contains("4 layers"), "{escape:?}");
    }

    #[test]
    fn a_four_layer_board_is_not_told_to_add_layers() {
        let text = row(12, 0.5, 0.23, 1.5, "Package_QFP:LQFP-48_7x7mm_P0.5mm").replace(
            r#"(layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))"#,
            r#"(layers (0 "F.Cu" signal) (4 "In1.Cu" power) (6 "In2.Cu" power) (31 "B.Cu" signal) (44 "Edge.Cuts" user))"#,
        );
        let findings = analyse(&board(&text), &FabricationPolicy::default());
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].layers, 4, "{findings:?}");
        assert_eq!(findings[0].suggested_layers, None, "{findings:?}");
        assert!(
            !findings[0].remediation.contains("4 layers"),
            "{findings:?}"
        );
    }

    #[test]
    fn mirrored_connector_pins_are_not_a_zero_millimetre_pitch() {
        // Connectors model the same pin twice (primary and mirrored). A raw
        // closest-pair reads 0 mm and every such part looks unescapable; the
        // duplicate is one pad site, so the real 0.5 mm pitch is what counts.
        let text = row(10, 0.5, 0.2, 1.5, "Connector:USB_C_Receptacle").replace(
            "  )\n  (gr_line",
            "    (pad \"99\" smd roundrect (at 0.0000 0) (size 0.200 1.500)\n      \
             (layers \"F.Cu\" \"F.Paste\" \"F.Mask\") (net 99 \"N99\"))\n  )\n  (gr_line",
        );
        let findings = analyse(&board(&text), &FabricationPolicy::default());
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            (findings[0].pitch_mm - 0.5).abs() < 1e-6,
            "the duplicate must not collapse the pitch: {findings:?}"
        );
    }

    #[test]
    fn a_coarse_pitch_package_escapes_cleanly() {
        // 0.8 mm pitch is the common coarse QFP/TQFP: the same default floor
        // leaves room, so nothing is reported.
        let board = board(&row(
            12,
            0.8,
            0.4,
            1.5,
            "Package_QFP:TQFP-44_10x10mm_P0.8mm",
        ));
        assert!(analyse(&board, &FabricationPolicy::default()).is_empty());
    }

    #[test]
    fn two_pin_passives_are_never_dense() {
        let board = board(&row(2, 0.4, 0.3, 0.3, "Resistor_SMD:R_0402_1005Metric"));
        assert!(analyse(&board, &FabricationPolicy::default()).is_empty());
    }

    #[test]
    fn a_relaxed_floor_can_make_a_package_escapable() {
        // The finding is a property of the *pair* (package, process): the same
        // 0.5 mm package escapes when the process allows a finer drill and
        // clearance. Without that, the check would be a gate instead of a
        // pre-routing warning.
        let board = board(&row(12, 0.5, 0.23, 1.5, "Package_QFP:LQFP-48_7x7mm_P0.5mm"));
        assert!(!analyse(&board, &FabricationPolicy::default()).is_empty());
        let relaxed = FabricationPolicy {
            min_track_width_nm: 90_000,
            min_clearance_nm: 50_000,
            min_drill_diameter_nm: 140_000,
            min_annular_ring_nm: 130_000,
            allow_via_in_pad: true,
            process_fills_via_in_pad: true,
        };
        assert!(
            analyse(&board, &relaxed).is_empty(),
            "0.5 mm pitch, relaxed floor"
        );
    }

    #[test]
    fn a_package_too_tight_even_for_a_track_is_reported_as_track_limited() {
        // Pads 0.45 mm wide at 0.5 mm pitch leave 0.05 mm — under the 0.127 mm
        // track plus clearance, so the pins cannot escape on their own layer.
        let board = board(&row(12, 0.5, 0.45, 1.5, "Package_QFN:QFN-48_P0.5mm"));
        let findings = analyse(&board, &FabricationPolicy::default());
        assert_eq!(findings[0].limit, EscapeLimit::Track, "{findings:?}");
    }
}
