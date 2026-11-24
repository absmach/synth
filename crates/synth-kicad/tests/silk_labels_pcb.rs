// SPDX-License-Identifier: Apache-2.0

//! Reference labels in the exported `.kicad_pcb` must stay on the board.
//!
//! A header flush with the bottom edge used to get its label at a fixed
//! offset below it, past the outline. The export now moves such a label to a
//! side that fits; this test reads the emitted file back and checks every
//! label centre against the `Edge.Cuts` outline.

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
    p.push(format!("synth-kicad-labels-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

/// Three headers hinted to the bottom edge, wired to an MCU.
const SOURCE: &str = r#"board "label_edge_export" {
  layers 2

  component U1: mcu "rp2350"
  component J1: connector "header_1x3" {
    placement_hint { edge: bottom  priority: hard }
  }
  component J2: connector "header_1x3" {
    placement_hint { edge: bottom  priority: hard }
  }
  component J3: connector "header_1x3" {
    placement_hint { edge: bottom  priority: hard }
  }

  connect U1.gp0 -> J1.p1
  connect U1.gp1 -> J2.p1
  connect U1.gp2 -> J3.p1
}
"#;

fn lower_source(source: &str, filename: &str) -> synth_ir::Board {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(source, filename);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, filename);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    lowered.board.expect("board")
}

/// Numbers following `keyword`, up to the closing parenthesis.
fn numbers_after(text: &str, keyword: &str) -> Option<Vec<f64>> {
    let start = text.find(keyword)? + keyword.len();
    let end = text[start..].find(')')? + start;
    Some(
        text[start..end]
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect(),
    )
}

/// Board outline `(min_x, min_y, max_x, max_y)` from the `Edge.Cuts` segments.
fn edge_cuts_bbox(pcb: &str) -> (f64, f64, f64, f64) {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for block in pcb.split("(gr_line").skip(1) {
        let block = block.split("(gr_line").next().unwrap_or(block);
        if !block.contains("\"Edge.Cuts\"") {
            continue;
        }
        for key in ["(start ", "(end "] {
            let n = numbers_after(block, key).expect("segment endpoint");
            min_x = min_x.min(n[0]);
            max_x = max_x.max(n[0]);
            min_y = min_y.min(n[1]);
            max_y = max_y.max(n[1]);
        }
    }
    (min_x, min_y, max_x, max_y)
}

/// Board-coordinate box of every reference label.
#[allow(clippy::cast_precision_loss)] // a designator is a handful of characters
fn labels(pcb: &str) -> Vec<Label> {
    let mut out = Vec::new();
    for block in pcb.split("\n\t(footprint\n").skip(1) {
        let at = numbers_after(block, "(at ").expect("footprint position");
        let rot = at.get(2).copied().unwrap_or(0.0).rem_euclid(360.0).round() as i32;
        // `(property "Reference" "J1" (at x y angle) ...)`, across lines.
        let prop = block.find("\"Reference\"").expect("reference property") + "\"Reference\"".len();
        let prop = &block[prop..];
        let name = prop.find('"').unwrap() + 1;
        let refdes = prop[name..name + prop[name..].find('"').unwrap()].to_string();
        let local = numbers_after(prop, "(at ").expect("label offset");
        // KiCad's footprint convention: a positive quarter turn maps local
        // (x, y) to (y, -x).
        let (dx, dy) = match rot {
            90 => (local[1], -local[0]),
            180 => (-local[0], -local[1]),
            270 => (-local[1], local[0]),
            _ => (local[0], local[1]),
        };
        // The text angle is absolute: a quarter turn stands the label up.
        let upright = local.get(2).is_some_and(|a| a.abs().round() as i32 == 90);
        // Half the text box at the export's 0.8 mm font (1.04 mm line height).
        let half_len = refdes.chars().count() as f64 * 0.8 / 2.0;
        let (half_w, half_h) = if upright {
            (0.52, half_len)
        } else {
            (half_len, 0.52)
        };
        out.push(Label {
            refdes,
            hidden: prop[..prop.find("(property").unwrap_or(prop.len())].contains("(hide yes)"),
            x: at[0] + dx,
            y: at[1] + dy,
            half_w,
            half_h,
        });
    }
    out
}

struct Label {
    refdes: String,
    hidden: bool,
    x: f64,
    y: f64,
    half_w: f64,
    half_h: f64,
}

/// No two shown labels may share a spot.
fn assert_no_label_overlaps(pcb: &str) {
    let shown: Vec<Label> = labels(pcb).into_iter().filter(|l| !l.hidden).collect();
    for (i, a) in shown.iter().enumerate() {
        for b in &shown[i + 1..] {
            let apart = a.x + a.half_w <= b.x - b.half_w
                || b.x + b.half_w <= a.x - a.half_w
                || a.y + a.half_h <= b.y - b.half_h
                || b.y + b.half_h <= a.y - a.half_h;
            assert!(
                apart,
                "labels {} at ({:.2}, {:.2}) and {} at ({:.2}, {:.2}) overlap",
                a.refdes, a.x, a.y, b.refdes, b.x, b.y
            );
        }
    }
}

/// Export `SOURCE` with a placement sidecar that pins the parts.
fn export_with_placement(source: &str, sidecar_toml: &str, tag: &str) -> String {
    let board = lower_source(source, "label_placement.synth");
    let tmp = tempdir(tag);
    fs::create_dir_all(&tmp).expect("temp dir");
    let sidecar = tmp.join("label_placement.placement.layout.toml");
    fs::write(&sidecar, sidecar_toml).expect("sidecar written");
    let result = synth_kicad::export_with_sidecars(
        &board,
        &tmp,
        &synth_kicad::Sidecars {
            schematic: None,
            placement: Some(sidecar),
        },
    )
    .expect("export");
    fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written")
}

/// A minor part with room around it gets a visible label.
const FREE_MINOR: &str = r#"board "label_free_minor" {
  layers 2

  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0603" value "1k"

  connect U1.gp0 -> R1.p1
}
"#;

/// `U1` at board (20, 20); the others placed by offset from it.
fn pinned_u1(offsets: &[(&str, f64, f64)]) -> String {
    use std::fmt::Write as _;

    let mut toml = String::from(
        "schema_version = 2\n\n[components.U1]\nx = 20.0\ny = 20.0\nrotation = 0\npriority = \"hard\"\n",
    );
    for (refdes, dx, dy) in offsets {
        let _ = writeln!(
            toml,
            "\n[components.{refdes}]\nrelative_to = \"U1\"\ndx = {dx}\ndy = {dy}\nrotation = 0\npriority = \"hard\""
        );
    }
    toml
}

/// Export `SOURCE` with `sidecar_toml` as its placement sidecar.
fn export_with_sidecar(sidecar_toml: &str, tag: &str) -> String {
    let board = lower_source(SOURCE, "label_edge_export.synth");
    let tmp = tempdir(tag);
    fs::create_dir_all(&tmp).expect("temp dir");
    let sidecar = tmp.join("labels.placement.layout.toml");
    fs::write(&sidecar, sidecar_toml).expect("sidecar written");
    let result = synth_kicad::export_with_sidecars(
        &board,
        &tmp,
        &synth_kicad::Sidecars {
            schematic: None,
            placement: Some(sidecar),
        },
    )
    .expect("export");
    fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written")
}

/// Whether `refdes`'s reference property carries `(hide yes)`.
fn label_is_hidden(pcb: &str, refdes: &str) -> bool {
    let needle = format!("\"Reference\"\n\t\t\t\"{refdes}\"");
    let start = pcb
        .find(&needle)
        .unwrap_or_else(|| panic!("{refdes} reference property"));
    let property = &pcb[start..];
    let end = property[1..]
        .find("(property")
        .map_or(property.len(), |i| i + 1);
    property[..end].contains("(hide yes)")
}

#[test]
fn a_sidecar_that_names_no_part_changes_nothing() {
    // Compared with an empty sidecar, not with none: giving the export any
    // placement sidecar takes a different placement path than giving none.
    let empty = export_with_sidecar("schema_version = 2\n", "empty");
    let stray = export_with_sidecar(
        "schema_version = 2\n\n[labels.NOSUCHPART]\nhide = true\ndx = 4.0\n",
        "stray",
    );
    assert_eq!(empty, stray, "an unknown designator must be ignored");
}

#[test]
fn a_sidecar_can_hide_one_label_and_leave_the_others() {
    let pcb = export_with_sidecar("[labels.J1]\nhide = true\n", "hide");
    assert!(label_is_hidden(&pcb, "J1"));
    assert!(!label_is_hidden(&pcb, "J2"));
}

#[test]
fn a_hard_sidecar_offset_moves_the_label_by_exactly_that_amount() {
    let at = |toml: &str, tag: &str| {
        let pcb = export_with_sidecar(toml, tag);
        let l = labels(&pcb)
            .into_iter()
            .find(|l| l.refdes == "J3")
            .expect("J3 label");
        (l.x, l.y)
    };
    let base = at(
        "[labels.J3]\ndx = 0.0\ndy = 0.0\npriority = \"hard\"\n",
        "hard-base",
    );
    // The fitted rim is tight, so the offset stays inside the board's room
    // below the part rather than asking the edge guard to clamp it.
    let moved = at(
        "[labels.J3]\ndx = 3.0\ndy = -1.0\npriority = \"hard\"\n",
        "hard-moved",
    );
    assert!(
        (moved.0 - base.0 - 3.0).abs() < 0.01 && (moved.1 - base.1 + 1.0).abs() < 0.01,
        "label moved from {base:?} to {moved:?}, expected a shift of (3.0, -1.0)"
    );
}

#[test]
fn every_reference_label_sits_inside_the_exported_outline() {
    let board = lower_source(SOURCE, "label_edge_export.synth");
    let tmp = tempdir("edge");
    let result = synth_kicad::export(&board, &tmp).expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");

    let (min_x, min_y, max_x, max_y) = edge_cuts_bbox(&pcb);
    let labels = labels(&pcb);
    assert!(labels.len() >= 4, "{} labels parsed", labels.len());
    for l in &labels {
        assert!(
            l.x - l.half_w > min_x
                && l.x + l.half_w < max_x
                && l.y - l.half_h > min_y
                && l.y + l.half_h < max_y,
            "label {} at ({:.2}, {:.2}) reaches off the board ({min_x:.2}..{max_x:.2}, {min_y:.2}..{max_y:.2})",
            l.refdes,
            l.x,
            l.y
        );
    }
}

/// A minor part with a free side keeps its silk reference.
#[test]
fn minor_parts_keep_a_visible_reference_when_there_is_room() {
    let pcb = export_with_placement(FREE_MINOR, &pinned_u1(&[("R1", 8.0, 0.0)]), "minor-room");
    assert!(!label_is_hidden(&pcb, "U1"));
    assert!(!label_is_hidden(&pcb, "R1"));
    assert_no_label_overlaps(&pcb);
}

/// In a crowded cluster the minor labels are laid on the least-clashing free
/// spot and never overlap each other. Crowding alone does not hide a label:
/// hiding is only what the part file or the design asks for.
#[test]
fn crowded_minor_labels_never_overlap() {
    let pcb = export_with_placement(
        r#"board "label_crowded" {
  layers 2

  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  component R1: resistor "r_generic_0603" value "1k"
  component R2: resistor "r_generic_0603" value "1k"
  component R3: resistor "r_generic_0603" value "1k"

  connect U1.gp0 -> R1.p1
  connect R1.p2 -> R2.p1
  connect R2.p2 -> R3.p1
  connect U2.gp0 -> R3.p2
}
"#,
        &pinned_u1(&[
            ("U2", 0.0, 12.0),
            ("R1", 7.2, 10.0),
            ("R2", 7.2, 12.8),
            ("R3", 7.2, 15.6),
        ]),
        "minor-crowded",
    );
    assert!(!label_is_hidden(&pcb, "R1"), "R1 has room above it");
    assert_no_label_overlaps(&pcb);
}

/// A part alone on the board is labelled even when the nearest free spot is
/// not the first one tried.
#[test]
fn a_lone_part_always_keeps_a_visible_reference() {
    let pcb = export_with_placement(FREE_MINOR, &pinned_u1(&[("R1", 14.0, 0.0)]), "minor-lone");
    assert!(!label_is_hidden(&pcb, "R1"));
    let label = labels(&pcb)
        .into_iter()
        .find(|l| l.refdes == "R1")
        .expect("R1 label");
    // R1's courtyard at (34, 20) is 3.46 x 1.96 mm: the label must not cover it.
    let (court_w, court_h) = (3.46 / 2.0, 1.96 / 2.0);
    assert!(
        label.x + label.half_w <= 34.0 - court_w
            || label.x - label.half_w >= 34.0 + court_w
            || label.y + label.half_h <= 20.0 - court_h
            || label.y - label.half_h >= 20.0 + court_h,
        "label at ({:.2}, {:.2}) sits on R1's pads",
        label.x,
        label.y
    );
    assert_no_label_overlaps(&pcb);
}

/// A design that hides a label still wins over a free slot.
#[test]
fn a_hidden_label_wins_over_a_free_slot() {
    let mut sidecar = pinned_u1(&[("R1", 8.0, 0.0)]);
    sidecar.push_str("\n[labels.R1]\nhide = true\n");
    let pcb = export_with_placement(FREE_MINOR, &sidecar, "minor-hidden");
    assert!(label_is_hidden(&pcb, "R1"));
}
