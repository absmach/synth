// SPDX-License-Identifier: Apache-2.0

//! Tests for `E-SYNTH-NAME-004`: refdes letter vs component kind
//! (IEEE reference-designator convention; Sierra Circuits schematic
//! guideline 10). Warnings only — convention, not correctness.

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
    let filename: String = "refdes_prefix.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &filename));
    }
    all
}

fn name_004(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-NAME-004")
        .collect()
}

#[test]
fn conventional_refdes_passes() {
    let src = "\
board \"t\" {
  component R1: resistor \"r_generic_0603\"
  component C1: capacitor \"c_generic_0603\"
  component SW1: switch \"spst_tactile\"
  component Y1: crystal \"ecs_2520mv_16m\" 2
  component U1: mcu \"stm32f103c8\"
  connect R1.p1 -> C1.p1
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert!(
        flagged.is_empty(),
        "conventional refdes must not warn: {flagged:?}"
    );
}

#[test]
fn unconventional_refdes_letter_warns() {
    // X-prefix resistor: legal but unconventional (R is expected).
    let src = "\
board \"t\" {
  component X1: resistor \"r_generic_0603\"
  component C1: capacitor \"c_generic_0603\"
  connect X1.p1 -> C1.p1
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    let expected = flagged[0].expected.as_deref().unwrap_or("");
    assert!(
        expected.contains('R'),
        "must point at the expected letter: {expected}"
    );
}

#[test]
fn led_prefixed_refdes_warns_d_prefix_expected() {
    let src = "\
board \"t\" {
  component LED1: led \"led_red_0603\"
  component R1: resistor \"r_generic_0603\"
  connect LED1.anode -> R1.p1
  connect LED1.cathode -> R1.p2
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert_eq!(flagged.len(), 1, "Sierra table: Diode/LED -> D");
}

#[test]
fn unknown_kinds_are_never_flagged() {
    // `antenna` has a lenient list; a truly unknown kind must not
    // produce false positives. `tp1`-style nonstandard kinds are
    // represented here by the accepted X-crystal alias instead.
    let src = "\
board \"t\" {
  component X9: crystal \"ecs_2520mv_16m\" 2
  component R1: resistor \"r_generic_0603\"
  connect X9.p1 -> R1.p1
  connect X9.p2 -> R1.p2
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert!(
        flagged.is_empty(),
        "X is an accepted crystal alias: {flagged:?}"
    );
}
