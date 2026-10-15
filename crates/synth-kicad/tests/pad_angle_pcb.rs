// SPDX-License-Identifier: Apache-2.0

//! A footprint placed with a rotation must carry that rotation on its pads.
//! KiCad reads a pad's `(at x y angle)` as the absolute pad orientation, so
//! the library footprint's pads need the footprint angle added or a long pad
//! keeps pointing the way the library drew it (absmach/synth#199).

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

const SOURCE: &str = r#"board "pad_angle" {
  layers 2

  component Q1: transistor "n_mosfet_irlml6244"
  component R1: resistor "r_generic_0603"

  connect Q1.d -> R1.p1
}
"#;

/// Export `SOURCE` with Q1 turned by `rotation` degrees and return the
/// `(pad ...)` blocks of Q1 as `(number, angle)` pairs. A pad whose `at` has
/// no third number has angle 0.
fn q1_pad_angles(rotation: i32) -> Vec<(String, f64)> {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(SOURCE, "pad_angle.synth");
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "pad_angle.synth");
    let board = lowered.board.expect("board");

    let tmp =
        std::env::temp_dir().join(format!("synth-pad-angle-{rotation}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let sidecar = tmp.join("placement.toml");
    fs::write(
        &sidecar,
        format!(
            "schema_version = 2\n\n[components.Q1]\nx = 20.0\ny = 20.0\nrotation = {rotation}\nsource = \"human_drag\"\npriority = \"hard\"\n"
        ),
    )
    .unwrap();
    let result = synth_kicad::export_with_sidecars(
        &board,
        &tmp,
        &synth_kicad::Sidecars {
            schematic: None,
            placement: Some(sidecar),
        },
    )
    .expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");

    // The footprint block that holds Q1's reference.
    let q1_ref = pcb.find("\"Q1\"").expect("Q1 in the board");
    let start = pcb[..q1_ref].rfind("(footprint").expect("footprint head");
    let end = pcb[q1_ref..]
        .find("\n\t(footprint")
        .map_or(pcb.len(), |rel| q1_ref + rel);
    let block = &pcb[start..end];

    let mut pads = Vec::new();
    let mut rest = block;
    while let Some(i) = rest.find("(pad \"") {
        rest = &rest[i + "(pad \"".len()..];
        let number = rest[..rest.find('"').unwrap()].to_string();
        let at = rest.find("(at ").expect("pad has an at");
        let inside = &rest[at + "(at ".len()..];
        let inside = &inside[..inside.find(')').unwrap()];
        let angle = inside
            .split_whitespace()
            .nth(2)
            .map_or(0.0, |s| s.parse().unwrap());
        pads.push((number, angle));
    }
    pads
}

#[test]
fn pads_of_a_rotated_footprint_carry_the_footprint_angle() {
    for rotation in [90, 180, 270] {
        let pads = q1_pad_angles(rotation);
        assert_eq!(pads.len(), 3, "SOT-23 has three pads");
        for (number, angle) in pads {
            assert!(
                (angle - f64::from(rotation)).abs() < 1e-9,
                "pad {number} at footprint rotation {rotation} has angle {angle}"
            );
        }
    }
}

#[test]
fn pads_of_an_unrotated_footprint_keep_angle_zero() {
    for (number, angle) in q1_pad_angles(0) {
        assert!(angle.abs() < 1e-9, "pad {number} has angle {angle}");
    }
}
