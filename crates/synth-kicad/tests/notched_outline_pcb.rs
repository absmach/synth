// SPDX-License-Identifier: Apache-2.0

//! A sidecar `[[outline]]` that is not a rectangle (an L-shaped board) must
//! reach Edge.Cuts as-is, and nothing the exporter generates may land in the
//! notch: not the ground fence vias, and not the ground pour.

use std::fs;
use std::path::{Path, PathBuf};

const SOURCE: &str = r#"board "notched" {
  layers 4
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  connect C1.p2 -> C2.p2 as "GND"
  connect C2.p2 -> C3.p2
}
"#;

/// An L: the square (0,0)..(40,40) without its (20,20)..(40,40) quarter.
const L_SHAPE: [(f64, f64); 6] = [
    (0.0, 0.0),
    (40.0, 0.0),
    (40.0, 20.0),
    (20.0, 20.0),
    (20.0, 40.0),
    (0.0, 40.0),
];

/// The fence of an unnotched square has a via at each of its four corners; the
/// notch removes at least one, so fewer than this many must remain.
const SQUARE_FENCE_CORNERS: usize = 4;

fn in_notch((x, y): (f64, f64)) -> bool {
    x > 20.0 && y > 20.0
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn export_pcb() -> String {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(SOURCE, "notched.synth");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "notched.synth");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let board = lowered.board.expect("board");

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let sidecar: String = L_SHAPE
        .iter()
        .fold(String::from("schema_version = 2\n"), |text, (x, y)| {
            text + &format!("[[outline]]\nx = {x}\ny = {y}\n")
        });
    let sidecar_path = dir.join("notched.placement.layout.toml");
    fs::write(&sidecar_path, sidecar).unwrap();
    let result = synth_kicad::export_with_sidecars(
        &board,
        &dir.join("out"),
        &synth_kicad::Sidecars {
            schematic: None,
            placement: Some(sidecar_path),
        },
    )
    .expect("export");
    fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written")
}

/// Every `(<key> X Y)` pair in `text`.
fn points(text: &str, key: &str) -> Vec<(f64, f64)> {
    text.split(key)
        .skip(1)
        .filter_map(|rest| {
            let mut words = rest.split(')').next()?.split_whitespace();
            Some((words.next()?.parse().ok()?, words.next()?.parse().ok()?))
        })
        .collect()
}

#[test]
fn an_l_shaped_outline_is_written_to_edge_cuts_and_keeps_generated_copper_out_of_the_notch() {
    let pcb = export_pcb();

    let starts: Vec<(f64, f64)> = pcb
        .split("(gr_line")
        .skip(1)
        .filter_map(|line| {
            let (geometry, rest) = line.split_once("(layer")?;
            rest.trim_start()
                .starts_with("\"Edge.Cuts\"")
                .then_some(geometry)
        })
        .flat_map(|geometry| points(geometry, "(start "))
        .collect();
    assert_eq!(starts, L_SHAPE, "the outline is written vertex for vertex");

    let fence: Vec<(f64, f64)> = pcb
        .split("(via")
        .skip(1)
        .filter(|via| via.starts_with(char::is_whitespace))
        .flat_map(|via| points(via.split("(uuid").next().unwrap_or(via), "(at "))
        .collect();
    assert!(!fence.is_empty(), "the ground fence is written");
    assert!(
        fence.len() < SQUARE_FENCE_CORNERS,
        "the corner in the notch is dropped: {fence:?}"
    );
    for at in &fence {
        assert!(!in_notch(*at), "fence via in the notch: {at:?}");
    }

    let zones: Vec<&str> = pcb.split("(zone").skip(1).collect();
    assert!(!zones.is_empty(), "ground zones are written");
    for zone in zones {
        for at in points(zone.split("(fill").next().unwrap_or(zone), "(xy ") {
            assert!(!in_notch(at), "zone vertex in the notch: {at:?}");
        }
    }
}
