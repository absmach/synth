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

/// The ground net's vias in a four-layer `jlcpcb` board that declares a 0.6 / 0.2 mm
/// `Default` class: the exporter stitches the ground pours with them.
fn ground_vias(extra: &str) -> Vec<(String, String)> {
    let source = format!(
        r#"board "stitch" {{
  manufacturer "jlcpcb"
  layers 4
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component R1: resistor "r_generic_0603" value "10k"
  connect R1.p1 -> C1.p1
  connect C1.p2 -> C2.p2 as "AGND"
  connect C2.p2 -> C3.p2
  connect R1.p2 -> C2.p1
{extra}}}
"#
    );
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(&source, "stitch.synth");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.unwrap(), &registry, "stitch.synth");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let tmp = tempdir("stitch");
    let result = synth_kicad::export(&lowered.board.unwrap(), &tmp).expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let mut vias = Vec::new();
    for chunk in pcb.split("(via").skip(1) {
        let size = chunk
            .split("(size ")
            .nth(1)
            .and_then(|s| s.split(')').next());
        let drill = chunk
            .split("(drill ")
            .nth(1)
            .and_then(|s| s.split(')').next());
        if let (Some(size), Some(drill)) = (size, drill) {
            vias.push((size.to_string(), drill.to_string()));
        }
    }
    vias
}

#[test]
fn the_ground_stitching_vias_follow_a_declared_default_drill() {
    let vias = ground_vias(
        r#"  netclass "Default" { via_diameter 0.6mm via_drill 0.2mm }
"#,
    );
    assert!(
        vias.len() >= 4,
        "a four-layer ground pour is stitched at the four corners: {vias:?}"
    );
    assert!(
        vias.iter()
            .all(|(size, drill)| size == "0.6" && drill == "0.2"),
        "every via carries the declared 0.6 / 0.2 mm: {vias:?}"
    );
}

#[test]
fn the_ground_stitching_vias_keep_the_default_when_nothing_is_declared() {
    let vias = ground_vias("");
    assert!(vias.len() >= 4, "{vias:?}");
    assert!(
        vias.iter()
            .all(|(size, drill)| size == "0.6" && drill == "0.3"),
        "{vias:?}"
    );
}

/// The drill tools and the via-pad apertures of the fabrication files a
/// four-layer `jlcpcb` board exports, through the same `kicad-cli` calls a fab
/// package makes. The board's only copper holes are vias: the ground stitching
/// vias and the fanout vias of a `PWR` plane. `None` when `kicad-cli` is absent.
fn fabricated_vias(label: &str, extra: &str) -> Option<(Vec<String>, Vec<String>)> {
    let source = format!(
        r#"board "{label}" {{
  manufacturer "jlcpcb"
  layers 4
  netclass "PWR" {{ plane {extra} }}
  netclass "Default" {{ {extra} }}
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "GND"
  connect C2.p1 -> C3.p1
  connect C1.p2 -> C2.p2 as "VBUS" class "PWR"
  connect C2.p2 -> C3.p2
}}
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
    let request = synth_kicad::FabRequest {
        gerbers: true,
        drill: true,
        step: false,
    };
    let artifacts = match synth_kicad::run_fab(&result.pcb_path, &tmp, &request) {
        Ok(artifacts) => artifacts,
        Err(synth_kicad::FabError::NotFound) => {
            eprintln!("kicad-cli unavailable; skipping the fabricated via test");
            return None;
        }
        Err(e) => panic!("fabrication export failed: {e}"),
    };
    let read_dir = |dir: &Path, suffix: &str| -> String {
        let path = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with(suffix))
            .unwrap_or_else(|| panic!("no *{suffix} in {}", dir.display()));
        fs::read_to_string(path).unwrap()
    };
    // Excellon tool definitions, `T1C0.200`: one per distinct plated hole.
    let pth = read_dir(artifacts.drill_dir.as_deref().unwrap(), "-PTH.drl");
    let drills = pth
        .lines()
        .filter(|line| line.starts_with('T') && line.contains('C'))
        .filter_map(|line| line.split_once('C').map(|(_, d)| d.to_string()))
        .collect();
    // The aperture each `ViaPad` attribute introduces on the front copper,
    // `%ADD11C,0.600000*%`.
    let front = read_dir(artifacts.gerbers_dir.as_deref().unwrap(), "-F_Cu.gbr");
    let mut pads = Vec::new();
    let mut lines = front.lines();
    while let Some(line) = lines.next() {
        if line.contains("AperFunction,ViaPad") {
            if let Some(aperture) = lines.next() {
                let (_, diameter) = aperture
                    .split_once("C,")
                    .unwrap_or_else(|| panic!("a round via pad: {aperture}"));
                pads.push(diameter.trim_end_matches("*%").to_string());
            }
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    Some((drills, pads))
}

fn mm(figure: &str) -> f64 {
    figure
        .parse()
        .unwrap_or_else(|_| panic!("a figure in mm: {figure}"))
}

/// The Gerbers and the drill file, not only the board's net classes, carry the
/// declared via: every plated hole is drilled at 0.2 mm under a 0.6 mm pad, so
/// every via has the 0.20 mm ring JLCPCB's DFM check asks for, where the
/// 0.6 / 0.3 mm default leaves the 0.15 mm ring it warns about.
#[test]
fn the_fabricated_vias_carry_the_declared_0_2_mm_drill() {
    let Some((drills, pads)) =
        fabricated_vias("fab-declared", "via_diameter 0.6mm via_drill 0.2mm")
    else {
        return;
    };
    assert!(
        !drills.is_empty() && !pads.is_empty(),
        "{drills:?} {pads:?}"
    );
    assert!(
        drills.iter().all(|d| (mm(d) - 0.2).abs() < 1e-6),
        "every plated hole is the declared 0.2 mm drill: {drills:?}"
    );
    assert!(
        pads.iter().all(|p| (mm(p) - 0.6).abs() < 1e-6),
        "every via pad is the declared 0.6 mm: {pads:?}"
    );
}

#[test]
fn the_fabricated_vias_keep_the_0_3_mm_default_when_nothing_is_declared() {
    let Some((drills, pads)) = fabricated_vias("fab-undeclared", "") else {
        return;
    };
    assert!(
        drills.iter().all(|d| (mm(d) - 0.3).abs() < 1e-6),
        "{drills:?}"
    );
    assert!(pads.iter().all(|p| (mm(p) - 0.6).abs() < 1e-6), "{pads:?}");
}
