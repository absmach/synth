// SPDX-License-Identifier: Apache-2.0

//! The CH340C's V3 pin is an internal-regulator tap at 5 V and is tied to the
//! supply at 3.3 V. The registry rule is conditional, not prose.

use std::path::{Path, PathBuf};

use synth_diagnostics::Diagnostic;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn validate(src: &str) -> Vec<Diagnostic> {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let filename = "ch340c_v3.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &filename));
    }
    all
}

fn missing_v3(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-POWER-001")
        .filter(|d| {
            let found = d.found.as_deref().unwrap_or_default();
            let expected = d.expected.as_deref().unwrap_or_default();
            found.contains("V3") || expected.contains("V3")
        })
        .collect()
}

#[test]
fn at_five_volts_the_v3_pin_needs_its_own_capacitor() {
    let src = r#"board "bridge" {
  layers 2
  component U1: ic "ch340c"
  component C1: capacitor "c_generic_0603" value "100nf"
  power "+5V" 5v { U1.vcc }
  power "+3V3" 3.3v { U1.v3 }
  power "GND" 0v { U1.gnd C1.p2 }
  connect U1.vcc -> C1.p1
}"#;
    let diags = validate(src);
    assert_eq!(
        missing_v3(&diags).len(),
        1,
        "expected one V3 finding: {:?}",
        diags
            .iter()
            .map(|d| (d.code.clone(), d.found.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn at_three_point_three_volts_the_vcc_capacitor_covers_v3() {
    let src = r#"board "bridge" {
  layers 2
  component U1: ic "ch340c"
  component C1: capacitor "c_generic_0603" value "100nf"
  power "+3V3" 3.3v { U1.vcc U1.v3 }
  power "GND" 0v { U1.gnd C1.p2 }
  connect U1.vcc -> C1.p1
}"#;
    let diags = validate(src);
    assert!(
        missing_v3(&diags).is_empty(),
        "3.3 V mode must not ask for a separate V3 capacitor: {:?}",
        diags
            .iter()
            .map(|d| (d.code.clone(), d.found.clone()))
            .collect::<Vec<_>>()
    );
}
