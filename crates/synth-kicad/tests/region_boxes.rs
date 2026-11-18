// SPDX-License-Identifier: Apache-2.0

//! The region boxes of a schematic read as one box cut into sections.
//!
//! Every boundary is drawn in the same colour and only once: where two regions
//! meet there is one line, and where a region meets the sheet frame there is
//! none, because the frame already draws it. The check goes through the real
//! export and reads the lines back, then compares them with the boxes the
//! layout holds.

use std::path::{Path, PathBuf};

type Segment = (f64, f64, f64, f64);

/// Where the inner line of the sheet frame sits, in mm from the paper's edge:
/// KiCad draws an outer line at 10 and the frame proper 2 mm inside it.
const BORDER_MM: f64 = 12.0;
/// How far apart two edges can be and still be one line: the width of a stroke
/// and a little slack.
const JOIN_MM: f64 = 0.3;
/// KiCad's title block: 110 mm wide and 34 mm tall, anchored to the bottom-right
/// corner of the paper's outer frame line.
const TITLE_BLOCK_W: f64 = 110.0;
const TITLE_BLOCK_H: f64 = 34.0;
const OUTER_LINE_MM: f64 = 10.0;

const SOURCE: &str = r##"board "regions" {
  layers 2
  manufacturer "jlcpcb"

  group "BRIDGE" color "#c2410c" {
    component U1: ic "ch340g"
  }

  group "LDO" color "#0072b2" {
    component U2: regulator "lp5907_3v3"
  }

  group "HEADER" color "#7c3aed" {
    component J2: connector "header_1x3"
  }

  group "PADS" color "#0a7d4a" {
    component R1: resistor "r_generic_0603" value "1k"
    component R2: resistor "r_generic_0603" value "1k"
  }

  net "GND" {
    U1.gnd, U2.gnd, J2.p1
  }

  net "SIG" {
    R1.p1, R2.p1, J2.p2
  }
}
"##;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn load_board(source: &str) -> synth_ir::Board {
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let ast = synth_parser::parse(source, "regions.synth".to_string())
        .ast
        .unwrap();
    synth_ir::lower(&ast, &registry, "regions.synth")
        .board
        .unwrap()
}

/// The text of `sch` after its `(lib_symbols ...)` block, where the sheet's
/// own drawing starts: the library has lines of its own.
fn after_library(sch: &str) -> &str {
    let start = sch.find("(lib_symbols").expect("a symbol library");
    let mut depth = 0;
    for (i, c) in sch[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &sch[start + i + 1..];
                }
            }
            _ => {}
        }
    }
    panic!("an unclosed symbol library");
}

fn numbers(text: &str) -> Vec<f64> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

/// What `(tag ...)` holds inside `text`: its first closing parenthesis ends it.
fn field<'a>(text: &'a str, tag: &str) -> &'a str {
    let rest = text.split(tag).nth(1).unwrap_or_else(|| panic!("a {tag}"));
    rest.split(')').next().unwrap()
}

/// Every drawn line of the region outline as a segment and its stroke colour.
fn lines(sch: &str) -> Vec<(Segment, Vec<f64>)> {
    after_library(sch)
        .split("(polyline")
        .skip(1)
        .map(|block| {
            let pts = numbers(block.split("(stroke").next().unwrap());
            let stroke = block.split("(stroke").nth(1).expect("a stroke");
            (
                (pts[0], pts[1], pts[2], pts[3]),
                numbers(field(stroke, "(color")),
            )
        })
        .collect()
}

/// The paper's width and height in mm.
fn paper(sch: &str) -> (f64, f64) {
    match field(sch, "(paper").trim().trim_matches('"') {
        "A4" => (297.0, 210.0),
        "A3" => (420.0, 297.0),
        "A2" => (594.0, 420.0),
        other => panic!("paper {other}"),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < JOIN_MM
}

fn is_vertical(s: Segment) -> bool {
    near(s.0, s.2)
}

/// Whether the spans `a0..a1` and `b0..b1` share some length.
fn along(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a1.min(b1) - a0.max(b0) > JOIN_MM
}

/// Whether `segment` runs over `other`, side by side or on top of it.
fn drawn_twice(a: Segment, b: Segment) -> bool {
    match (is_vertical(a), is_vertical(b)) {
        (true, true) => {
            near(a.0, b.0) && along(a.1.min(a.3), a.1.max(a.3), b.1.min(b.3), b.1.max(b.3))
        }
        (false, false) => {
            near(a.1, b.1) && along(a.0.min(a.2), a.0.max(a.2), b.0.min(b.2), b.0.max(b.2))
        }
        _ => false,
    }
}

/// Whether some drawn line passes within a stroke's width of (`x`, `y`).
fn covered(lines: &[(Segment, Vec<f64>)], x: f64, y: f64) -> bool {
    lines.iter().any(|&((x0, y0, x1, y1), _)| {
        x >= x0.min(x1) - JOIN_MM
            && x <= x0.max(x1) + JOIN_MM
            && y >= y0.min(y1) - JOIN_MM
            && y <= y0.max(y1) + JOIN_MM
    })
}

/// The boxes hug their content, stay inside the page frame, never overlap and
/// leave a visible gap between neighbours that share a row or column.
fn assert_boxes_hug_and_clear(layout: &synth_layout::Layout, width: f64, height: f64) {
    // The whole cut is packed at the frame's top-left and leaves most of the
    // sheet unused, rather than being stretched over the page.
    let mut hull = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for b in &layout.group_boxes {
        let (x0, y0) = b.min_mm;
        let (x1, y1) = b.max_mm;
        assert!(
            x0 >= BORDER_MM - JOIN_MM
                && y0 >= BORDER_MM - JOIN_MM
                && x1 <= width - BORDER_MM + JOIN_MM
                && y1 <= height - BORDER_MM + JOIN_MM,
            "box {:?} at ({x0:.1},{y0:.1})-({x1:.1},{y1:.1}) leaves the frame",
            b.group
        );
        hull.0 = hull.0.min(x0);
        hull.1 = hull.1.min(y0);
        hull.2 = hull.2.max(x1);
        hull.3 = hull.3.max(y1);
    }
    // A stretched layout filled the frame; a hugging one leaves the page's
    // spare room outside the packed boxes.
    assert!(
        hull.2 < width - BORDER_MM - 20.0 && hull.3 < height - BORDER_MM - 20.0,
        "the boxes still fill the frame: hull ({:.1},{:.1})-({:.1},{:.1})",
        hull.0,
        hull.1,
        hull.2,
        hull.3
    );
    // ... and the packed hull starts at the frame's top-left, within a grid
    // step.
    assert!(
        (hull.0 - BORDER_MM).abs() <= 2.54 && (hull.1 - BORDER_MM).abs() <= 2.54,
        "the packed boxes start at ({:.1},{:.1}), not the frame's top-left",
        hull.0,
        hull.1
    );
    // No two boxes overlap, and neighbours that share a row or column keep a
    // visible gap instead of touching.
    for (i, a) in layout.group_boxes.iter().enumerate() {
        for c in &layout.group_boxes[i + 1..] {
            let (ax0, ay0) = a.min_mm;
            let (ax1, ay1) = a.max_mm;
            let (cx0, cy0) = c.min_mm;
            let (cx1, cy1) = c.max_mm;
            let overlap = ax0 < cx1 && cx0 < ax1 && ay0 < cy1 && cy0 < ay1;
            assert!(!overlap, "{} and {} overlap", a.group, c.group);
            let share_rows = ay0 < cy1 - JOIN_MM && cy0 < ay1 - JOIN_MM;
            let share_cols = ax0 < cx1 - JOIN_MM && cx0 < ax1 - JOIN_MM;
            let gap_x = (cx0 - ax1).max(ax0 - cx1);
            let gap_y = (cy0 - ay1).max(ay0 - cy1);
            if share_rows && gap_x < 10.0 {
                assert!(
                    gap_x >= 0.5,
                    "{} and {} are {gap_x:.2} mm apart",
                    a.group,
                    c.group
                );
            }
            if share_cols && gap_y < 10.0 {
                assert!(
                    gap_y >= 0.5,
                    "{} and {} are {gap_y:.2} mm apart",
                    a.group,
                    c.group
                );
            }
        }
    }
}

#[test]
fn the_region_boxes_read_as_one_box_cut_into_sections() {
    let board = load_board(SOURCE);
    let dir = tempfile::tempdir().unwrap();
    let result = synth_kicad::export_schematic_only(&board, dir.path(), None).expect("export");
    let sch = std::fs::read_to_string(&result.schematic_path).unwrap();
    let lines = lines(&sch);
    assert!(!lines.is_empty(), "the boundaries are drawn as lines");
    let (width, height) = paper(&sch);

    let mut found = Vec::new();
    for (segment, stroke) in &lines {
        if *stroke != lines[0].1 {
            found.push(format!("stroke {stroke:?} differs from {:?}", lines[0].1));
        }
        let (x0, y0, x1, y1) = *segment;
        let on_frame = if is_vertical(*segment) {
            near(x0, BORDER_MM) || near(x0, width - BORDER_MM)
        } else {
            near(y0, BORDER_MM) || near(y0, height - BORDER_MM)
        };
        if on_frame {
            found.push(format!("line {segment:?} is drawn over the sheet frame"));
        }
        if x0 == x1 && y0 == y1 {
            found.push(format!("line {segment:?} has no length"));
        }
    }
    for (i, (a, _)) in lines.iter().enumerate() {
        for (b, _) in &lines[i + 1..] {
            if drawn_twice(*a, *b) {
                found.push(format!("{a:?} and {b:?} are one boundary drawn twice"));
            }
        }
    }

    // Every edge of every box is drawn once, unless the sheet frame has it.
    let layout = synth_layout::layout_with_sidecar(&board, None);
    assert!(layout.group_boxes.len() >= 4, "one box per group");
    let on_frame = |v: f64, edge: f64| near(v, edge);
    let block = (
        width - OUTER_LINE_MM - TITLE_BLOCK_W,
        height - OUTER_LINE_MM - TITLE_BLOCK_H,
    );
    for b in &layout.group_boxes {
        let (x0, y0) = b.min_mm;
        let (x1, y1) = b.max_mm;
        if x1 > block.0 + JOIN_MM && y1 > block.1 + JOIN_MM {
            found.push(format!(
                "box {:?} runs into the title block at {block:?}",
                b.group
            ));
        }
        let mut at = 0.0;
        while at <= 1.0 {
            let (x, y) = (x0 + (x1 - x0) * at, y0 + (y1 - y0) * at);
            // A bottom edge on the block's top line is the block's to draw.
            let under_block = near(y1, block.1) && x > block.0 - JOIN_MM;
            let edges: [(&str, bool, f64, f64); 4] = [
                ("left", on_frame(x0, BORDER_MM), x0, y),
                ("right", on_frame(x1, width - BORDER_MM), x1, y),
                ("top", on_frame(y0, BORDER_MM), x, y0),
                (
                    "bottom",
                    on_frame(y1, height - BORDER_MM) || under_block,
                    x,
                    y1,
                ),
            ];
            for (name, on_the_frame, ex, ey) in edges {
                if !on_the_frame && !covered(&lines, ex, ey) {
                    found.push(format!(
                        "{name} edge of {:?} has no line at ({ex:.1}, {ey:.1})",
                        b.group
                    ));
                }
            }
            at += 0.05;
        }
    }
    assert_boxes_hug_and_clear(&layout, width, height);
    found.sort();
    found.dedup();
    assert!(found.is_empty(), "region outline:\n{}", found.join("\n"));
}
