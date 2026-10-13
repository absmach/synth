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
                max_power_mw: Some(power_mw),
                thermal_resistance_c_per_w: Some(theta_c_per_w),
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
