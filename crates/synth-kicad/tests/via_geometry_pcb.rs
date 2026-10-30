// SPDX-License-Identifier: Apache-2.0

//! A board for `jlcpcb` that declares a 0.6 mm via over a 0.2 mm drill is
//! exported with exactly that via: the project's net classes carry it, the PCB's
//! minimum via follows the process, and the annular ring is the 0.20 mm JLCPCB's
//! capabilities page lists. A board that declares nothing keeps the 0.6 / 0.3 mm
//! default, so the smaller drill is something a board asks for.

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
    p.push(format!("synth-kicad-via-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

/// The exported `.kicad_pcb` and the net classes of the `.kicad_pro` of a
/// two-resistor board for `jlcpcb` with `extra` declarations.
fn export(label: &str, extra: &str) -> (String, Vec<serde_json::Value>) {
    let source = format!(
        r#"board "{label}" {{
  manufacturer "jlcpcb"
  layers 2
  component R1: resistor "r_generic_0603" value "10k"
  component R2: resistor "r_generic_0603" value "10k"
  connect R1.p1 -> R2.p1
  connect R1.p2 -> R2.p2
{extra}}}
"#
    );
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(&source, format!("{label}.synth"));
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.unwrap(), &registry, &format!("{label}.synth"));
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let tmp = tempdir(label);
    let result = synth_kicad::export(&lowered.board.unwrap(), &tmp).expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let project: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&result.project_path).expect(".kicad_pro"))
            .expect("the project is JSON");
    let classes = project["net_settings"]["classes"]
        .as_array()
        .expect("net classes")
        .clone();
    (pcb, classes)
}

fn class<'a>(classes: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    classes
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no net class {name}: {classes:?}"))
}

#[test]
fn a_declared_0_6_over_0_2_via_is_exported_with_the_ring_jlcpcb_lists() {
    let (pcb, classes) = export(
        "declared",
        r#"  netclass "Default" { via_diameter 0.6mm via_drill 0.2mm }
"#,
    );
    let default = class(&classes, "Default");
    let (diameter, drill) = (
        default["via_diameter"].as_f64().unwrap(),
        default["via_drill"].as_f64().unwrap(),
    );
    assert!(
        (diameter - 0.6).abs() < 1e-9 && (drill - 0.2).abs() < 1e-9,
        "the project carries the declared via: {diameter} over {drill}"
    );
    assert!(
        (diameter - drill) / 2.0 >= 0.2 - 1e-9,
        "a 0.20 mm annular ring, the figure JLCPCB's capabilities page lists"
    );
    assert!(
        pcb.contains("(via_min_drill 0.2)"),
        "the process allows the 0.2 mm drill, so the PCB's minimum follows it"
    );
}

#[test]
fn a_board_that_declares_no_via_keeps_the_0_6_over_0_3_default() {
    let (_, classes) = export("undeclared", "");
    let default = class(&classes, "Default");
    assert_eq!(default["via_diameter"].as_f64(), Some(0.6));
    assert_eq!(default["via_drill"].as_f64(), Some(0.3));
}
