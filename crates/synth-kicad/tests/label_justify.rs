// SPDX-License-Identifier: Apache-2.0

//! A net label is a boxed flag that hangs off a stub and runs away from the
//! pin, its point facing away too. KiCad 10 stores a label's text
//! justification in the file and centres the text on the anchor when there is
//! none, which puts half of it on the stub and pin, so the exporter has to
//! write it.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// Two header pairs far apart on a dev-kit sized chip: enough long nets that
/// the layout turns some of them into labels.
const SOURCE: &str = r#"board "label_justify" {
  layers 2

  component U1: mcu "rp2350"
  component HL1: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HR1: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }

  connect U1.gp0 -> HL1.p1
  connect U1.gp1 -> HL1.p2
  connect U1.gp0 -> HR1.p1
  connect U1.gp1 -> HR1.p2
}
"#;

/// `(global_label "name" (shape s) (at x y angle) (effects ... (justify j)))`
/// blocks as `(angle, justify, shape)`; `justify` is empty when the label has
/// none.
fn labels(sch: &str) -> Vec<(f64, String, String)> {
    let mut out = Vec::new();
    let mut rest = sch;
    while let Some(i) = rest.find("\n\t(global_label\n") {
        rest = &rest[i + 1..];
        let end = rest[1..].find("\n\t(").map_or(rest.len(), |e| e + 1);
        let block = &rest[..end];
        let at = block.find("(at ").expect("label has an at");
        let angle: f64 = block[at + 4..]
            .split(|c: char| c == ')' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .nth(2)
            .expect("angle")
            .parse()
            .unwrap();
        let justify = block.find("(justify ").map_or(String::new(), |j| {
            block[j + 9..]
                .split(')')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        });
        let shape = block.find("(shape ").map_or(String::new(), |j| {
            block[j + 7..].split(')').next().unwrap_or("").to_string()
        });
        out.push((angle, justify, shape));
        rest = &rest[end..];
    }
    out
}

fn export_schematic() -> String {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(SOURCE, "label_justify.synth");
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "label_justify.synth");
    let board = lowered.board.expect("board");

    let tmp = std::env::temp_dir().join(format!("synth-label-justify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let result = synth_kicad::export(&board, &tmp).expect("export");
    fs::read_to_string(&result.schematic_path).expect(".kicad_sch written")
}

#[test]
fn net_labels_are_justified_away_from_their_stub() {
    let sch = export_schematic();
    let found = labels(&sch);
    assert!(!found.is_empty(), "the design must produce net labels");
    for (angle, justify, _) in found {
        let want = if angle == 180.0 { "right" } else { "left" };
        assert_eq!(
            justify, want,
            "a label at angle {angle} must justify {want}"
        );
    }
}

#[test]
fn net_labels_are_output_flags_pointing_away_from_their_stub() {
    let sch = export_schematic();
    assert!(
        !sch.contains("\n\t(label\n"),
        "a signal net label must be a global label, not plain text"
    );
    let found = labels(&sch);
    assert!(!found.is_empty(), "the design must produce net labels");
    for side in [0.0, 180.0] {
        assert!(
            found.iter().any(|(angle, ..)| *angle == side),
            "no label on the {side} degree side"
        );
    }
    for (angle, _, shape) in found {
        assert_eq!(
            shape, "output",
            "a label at angle {angle} must be an output flag"
        );
    }
}
