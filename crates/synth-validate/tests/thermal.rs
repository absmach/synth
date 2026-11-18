// SPDX-License-Identifier: Apache-2.0

//! `W-SYNTH-THERMAL-001`: a part that declares its dissipation and its
//! junction-to-ambient resistance gets a junction-temperature estimate in the
//! gate. A part that declares neither is not checked.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Severity};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

const SOURCE: &str = r#"board "thermal" {
  layers 2
  component R1: resistor "r_generic_0603" value "10k"
  component R2: resistor "r_generic_0603" value "10k"
  connect R1.p1 -> R2.p1
  connect R1.p2 -> R2.p2
}
"#;

/// Lower the source, stamp a dissipation and thermal resistance onto every
/// part, and run the gate.
fn diagnostics_with_power(power_mw: f64, theta_c_per_w: f64) -> Vec<Diagnostic> {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let parse = synth_parser::parse(SOURCE, "thermal.synth".to_string());
    let ast = parse.ast.as_ref().expect("source must parse");
    let mut lowered = synth_ir::lower(ast, &registry, "thermal.synth");
    let board = lowered.board.as_mut().expect("board must lower");
    for component in &mut board.components {
        if let Some(part) = component.part.as_mut() {
            part.operating_conditions = Some(synth_registry::OperatingConditions {
                min_voltage_v: None,
                max_voltage_v: None,
                max_current_ma: None,
                forward_voltage_v: None,
                forward_current_ma: None,
                max_power_mw: Some(power_mw),
                thermal_resistance_c_per_w: Some(theta_c_per_w),
                ..Default::default()
            });
        }
    }
    synth_validate::run_erc(board, "thermal.synth")
}

#[test]
fn a_part_declaring_a_hot_junction_is_warned() {
    // 25 C + 1.5 W * 60 C/W = 115 C.
    let diags = diagnostics_with_power(1500.0, 60.0);
    let found: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| d.code == "W-SYNTH-THERMAL-001")
        .collect();
    assert_eq!(found.len(), 2, "{diags:?}");
    assert_eq!(found[0].severity, Severity::Warning);
}

#[test]
fn a_cool_part_is_not_warned() {
    // 25 C + 0.1 W * 50 C/W = 30 C.
    let diags = diagnostics_with_power(100.0, 50.0);
    assert!(
        diags.iter().all(|d| d.code != "W-SYNTH-THERMAL-001"),
        "{diags:?}"
    );
}

/// A regulator with no declared load has no board dissipation to estimate, so
/// the package rating must not be reported as if it were the design's.
#[test]
fn the_seed_ldo_is_not_hot_without_a_declared_load() {
    let src = r#"board "hot" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component C1: capacitor "c_generic_0805" value "10uF"
  component C2: capacitor "c_generic_0805" value "10uF"
  connect U1.vin -> C1.p1
  connect U1.vout -> C2.p1
  connect U1.gnd -> C1.p2 as "GND"
}
"#;
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let parse = synth_parser::parse(src, "hot.synth".to_string());
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, &registry, "hot.synth");
    let board = lowered.board.as_ref().expect("board must lower");
    let diags = synth_validate::run_erc(board, "hot.synth");
    assert!(
        diags.iter().all(|d| d.code != "W-SYNTH-THERMAL-001"),
        "{diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "W-SYNTH-THERMAL-002"),
        "a rail supply with no declared load is an unverified thermal claim: {diags:?}"
    );
}

/// With a declared 5 V to 3.3 V drop and a 1 A load, the regulator really does
/// dissipate about 1.7 W, so the warning follows the board's dissipation.
#[test]
fn the_seed_ldo_is_hot_when_its_rail_draws_current() {
    let src = r#"board "hot" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component U2: mcu "stm32g030f6p6"
  component C1: capacitor "c_generic_0805" value "10uF"
  component C2: capacitor "c_generic_0805" value "10uF"
  power "+5V" 5v { U1.vin }
  power "+3V3" 3.3v { U1.vout U2.vdd }
  connect U1.gnd -> C1.p2 as "GND"
  connect U2.vss -> C1.p2
  connect U1.vin -> C1.p1
  connect U1.vout -> C2.p1
}
"#;
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let parse = synth_parser::parse(src, "hot.synth".to_string());
    let ast = parse.ast.as_ref().expect("source must parse");
    let mut lowered = synth_ir::lower(ast, &registry, "hot.synth");
    let board = lowered.board.as_mut().expect("board must lower");
    let load = board
        .components
        .iter_mut()
        .find(|c| c.refdes == "U2")
        .expect("the load");
    load.part.as_mut().expect("a part").operating_conditions =
        Some(synth_registry::OperatingConditions {
            max_current_ma: Some(1000.0),
            ..Default::default()
        });
    let diags = synth_validate::run_erc(board, "hot.synth");
    assert!(
        diags.iter().any(|d| d.code == "W-SYNTH-THERMAL-001"),
        "{diags:?}"
    );
}
