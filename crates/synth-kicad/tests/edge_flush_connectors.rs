// SPDX-License-Identifier: Apache-2.0

//! End-to-end edge-flush connector tests (absmach/synth#332).
//!
//! A hard `edge:` hint on a non-header connector is a mechanical contract:
//! its exported `F.CrtYd` must touch `Edge.Cuts` on the hinted edge, so a
//! mating cable or case meets the connector, not the board rim. Corner
//! mounting holes keep their own rim on the edges they own, and where a hole
//! shares a flush edge with a connector its courtyard may overhang the line —
//! but its drill never crosses `Edge.Cuts`.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "synth-kicad-edge-flush-{label}-{}",
        std::process::id()
    ));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

/// Hard-hinted external connectors on three edges, plus a mounting hole in
/// every corner: the Class A arrangement #332 was reported on.
const SOURCE: &str = r#"board "edge_flush_holes" {
  layers 2
  manufacturer "jlcpcb"

  component U1: mcu "stm32g030"
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component J1: connector "jst_ph_2pin" { placement_hint { region: left_edge edge: left priority: hard } }
  component J2: connector "jst_xh_2pin" { placement_hint { region: right_edge edge: right priority: hard } }
  component J3: connector "jst_ph_3pin" { placement_hint { region: bottom_edge edge: bottom priority: hard } }

  component H1: mounting_hole "mounting_hole_m2_5" { placement_hint { region: top_left priority: hard } }
  component H2: mounting_hole "mounting_hole_m2_5" { placement_hint { region: top_right priority: hard } }
  component H3: mounting_hole "mounting_hole_m2_5" { placement_hint { region: bottom_left priority: hard } }
  component H4: mounting_hole "mounting_hole_m2_5" { placement_hint { region: bottom_right priority: hard } }

  connect J1.p1 -> U1.vdd
  connect J1.p2 -> U1.vss
  connect U1.vdd -> C1.p1
  connect U1.vss -> C1.p2
  connect U1.vdd -> C2.p1
  connect U1.vss -> C2.p2
  connect J2.p1 -> U1.pa0
  connect J2.p2 -> U1.vss
  connect J3.p1 -> U1.pa1
  connect J3.p2 -> U1.pa2
  connect J3.p3 -> U1.vss
}
"#;

fn load_board(source: &str, name: &str) -> synth_ir::Board {
    let parse = synth_parser::parse(source, name);
    let ast = parse.ast.expect("parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry/parts")).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(&ast, &loader, name);
    let lowered = synth_ir::lower(&resolved.program, &registry, name);
    lowered.board.expect("board")
}

/// One exported footprint's measured geometry, in millimetres.
struct MeasuredFootprint {
    refdes: String,
    /// `(min_x, min_y, max_x, max_y)` of everything drawn on `F.CrtYd`,
    /// rotated into board coordinates.
    courtyard: (f64, f64, f64, f64),
    /// The same for every pad's copper — a mounting hole's NPTH drill here.
    pads: Option<(f64, f64, f64, f64)>,
}

/// The `Edge.Cuts` rectangle of an exported board, in millimetres.
fn edge_cuts_rect(pcb: &str) -> (f64, f64, f64, f64) {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for block in top_level_blocks(pcb) {
        if !block.starts_with("(gr_line") || !block.contains("\"Edge.Cuts\"") {
            continue;
        }
        for point in ["start", "end"] {
            let (x, y) = xy_in(block, point).expect("edge cut point");
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    (min_x, min_y, max_x, max_y)
}

/// KiCad rotates a footprint by its `(at x y angle)`: a local point
/// `(x, y)` lands at the offset below (board y points down, matching
/// `synth_geometry::Rotation::rotate_offset`'s tested convention).
fn rotate_local(x: f64, y: f64, angle_deg: f64) -> (f64, f64) {
    match angle_deg as i64 {
        90 => (y, -x),
        180 => (-x, -y),
        270 => (-y, x),
        _ => (x, y),
    }
}

fn measure_footprints(pcb: &str) -> Vec<MeasuredFootprint> {
    let mut out = Vec::new();
    for block in top_level_blocks(pcb) {
        if !block.starts_with("(footprint") {
            continue;
        }
        let refdes = block
            .split_once("\"Reference\"")
            .and_then(|(_, tail)| tail.split_once('"').map(|(_, rest)| rest))
            .and_then(|tail| tail.split_once('"').map(|(name, _)| name))
            .unwrap_or_default()
            .to_string();
        let (at_x, at_y, angle) = footprint_at(block);
        let mut court: Option<(f64, f64, f64, f64)> = None;
        let mut pads: Option<(f64, f64, f64, f64)> = None;
        let include = |rect: &mut Option<(f64, f64, f64, f64)>, x: f64, y: f64| {
            let (dx, dy) = rotate_local(x, y, angle);
            let point = (at_x + dx, at_y + dy);
            *rect = Some(match *rect {
                None => (point.0, point.1, point.0, point.1),
                Some((min_x, min_y, max_x, max_y)) => (
                    min_x.min(point.0),
                    min_y.min(point.1),
                    max_x.max(point.0),
                    max_y.max(point.1),
                ),
            });
        };
        for sub in top_level_blocks_at(&block[1..block.len() - 1], 0) {
            if sub.contains("\"F.CrtYd\"") {
                if let Some(point) = xy_in(sub, "start") {
                    include(&mut court, point.0, point.1);
                }
                if let Some(point) = xy_in(sub, "end") {
                    include(&mut court, point.0, point.1);
                }
                if let Some(point) = xy_in(sub, "center") {
                    // A courtyard circle: `end` is a point on the circle.
                    if let Some(end) = xy_in(sub, "end") {
                        let radius = (end.0 - point.0).abs();
                        for (dx, dy) in [
                            (-radius, -radius),
                            (radius, -radius),
                            (-radius, radius),
                            (radius, radius),
                        ] {
                            include(&mut court, point.0 + dx, point.1 + dy);
                        }
                    }
                }
            }
            if sub.starts_with("(pad") {
                let (px, py) = xy_in(sub, "at").unwrap_or((0.0, 0.0));
                let (w, h) = xy_in(sub, "size").unwrap_or((0.0, 0.0));
                for (dx, dy) in [(-w / 2.0, -h / 2.0), (w / 2.0, h / 2.0)] {
                    include(&mut pads, px + dx, py + dy);
                }
            }
        }
        out.push(MeasuredFootprint {
            refdes,
            courtyard: court.unwrap_or((f64::MAX, f64::MAX, f64::MIN, f64::MIN)),
            pads,
        });
    }
    out
}

fn footprint_at(block: &str) -> (f64, f64, f64) {
    let idx = block.find("(at ").expect("footprint at");
    let tail = &block[idx + 4..];
    let close = tail.find(')').expect("at close");
    let mut nums = tail[..close]
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok());
    let x = nums.next().expect("x");
    let y = nums.next().expect("y");
    (x, y, nums.next().unwrap_or(0.0))
}

fn xy_in(block: &str, tag: &str) -> Option<(f64, f64)> {
    let idx = block.find(&format!("({tag} "))?;
    let tail = &block[idx + tag.len() + 2..];
    let close = tail.find(')')?;
    let mut nums = tail[..close]
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok());
    Some((nums.next()?, nums.next()?))
}

/// The top-level s-expressions at depth `top` of `text`: every block that
/// opens when the parenthesis depth reaches `top`. The board file's main
/// `(kicad_pcb …)` block holds its children at depth 1; a stripped
/// `(footprint …)` block holds its own at depth 0.
fn top_level_blocks_at(text: &str, top: usize) -> Vec<&str> {
    let mut depth = 0usize;
    let mut start = None;
    let mut out = Vec::new();
    for (i, c) in text.char_indices() {
        match c {
            '(' => {
                if depth == top {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' => {
                if depth == top + 1 {
                    if let Some(s) = start {
                        out.push(&text[s..=i]);
                    }
                    start = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    out
}

fn top_level_blocks(text: &str) -> Vec<&str> {
    top_level_blocks_at(text, 1)
}

#[test]
fn a_hard_hinted_connector_touches_edge_cuts_while_holes_keep_their_drill_inside() {
    let board = load_board(SOURCE, "edge_flush_holes.synth");
    let out = tempdir("all-edges");
    let result = synth_kicad::export(&board, &out).expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect("read pcb");
    let (min_x, min_y, max_x, max_y) = edge_cuts_rect(&pcb);
    let measured = measure_footprints(&pcb);

    // Each hinted edge connector's courtyard touches Edge.Cuts on its hinted
    // edge, within the 0.1 mm the issue asks for.
    let gap_on = |footprint: &MeasuredFootprint, edge: &str| -> f64 {
        let (court, _) = match edge {
            "left" => (footprint.courtyard.0 - min_x, 0.0),
            "top" => (footprint.courtyard.1 - min_y, 0.0),
            "right" => (max_x - footprint.courtyard.2, 0.0),
            _ => (max_y - footprint.courtyard.3, 0.0),
        };
        court
    };
    for (refdes, edge) in [("J1", "left"), ("J2", "right"), ("J3", "bottom")] {
        let footprint = measured
            .iter()
            .find(|f| f.refdes == refdes)
            .unwrap_or_else(|| panic!("{refdes} exported"));
        let gap = gap_on(footprint, edge);
        assert!(
            gap.abs() <= 0.1,
            "{refdes} on {edge}: courtyard gap {gap:.3} mm to Edge.Cuts"
        );
    }

    // Edge.Cuts never crosses a mounting hole's drill: the hole's pad copper
    // stays inside every edge, with at least the 0.2 mm the exported DRU
    // demands of an NPTH pad.
    for footprint in &measured {
        if !footprint.refdes.starts_with('H') {
            continue;
        }
        let Some(pads) = footprint.pads else {
            panic!("{} has no pads", footprint.refdes);
        };
        let clearances = [
            pads.0 - min_x,
            pads.1 - min_y,
            max_x - pads.2,
            max_y - pads.3,
        ];
        for (edge, clearance) in ["left", "top", "right", "bottom"]
            .iter()
            .zip(clearances.iter())
        {
            assert!(
                *clearance >= 0.2,
                "{} drill sits {:.3} mm from the {edge} edge; Edge.Cuts must never cross a drill",
                footprint.refdes,
                clearance
            );
        }
    }

    // J3 shares the bottom edge with the corner holes H3/H4. The connector
    // stays flush there, so the fit caps the holes' rim at its face: a hole
    // on that edge may sit closer than its 2.2 mm rim, and its courtyard may
    // overhang the line — what may never happen is a drill outside Edge.Cuts,
    // which the loop above asserts for every hole on every edge.
}
