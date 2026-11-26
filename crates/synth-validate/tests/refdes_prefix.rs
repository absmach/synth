// SPDX-License-Identifier: Apache-2.0

//! Tests for `E-SYNTH-NAME-004`: refdes letter vs component kind
//! (IEEE reference-designator convention). Warnings only —
//! convention, not correctness.

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
    assert_eq!(flagged.len(), 1, "refdes table: Diode/LED -> D");
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

#[test]
fn an_fpga_is_a_u_like_every_other_packaged_device() {
    let src = "\
board \"t\" {
  component R9: fpga \"ice40hx4k_bg121\"
  component R1: resistor \"r_generic_0603\"
  connect R9.iot_225 -> R1.p1
  connect R9.iot_222 -> R1.p2
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert_eq!(flagged.len(), 1, "refdes table: FPGA -> U, so R9 is wrong");
}

#[test]
fn an_fpga_called_u_is_not_flagged() {
    let src = "\
board \"t\" {
  component U9: fpga \"ice40hx4k_bg121\"
  component R1: resistor \"r_generic_0603\"
  connect U9.iot_225 -> R1.p1
  connect U9.iot_222 -> R1.p2
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert!(flagged.is_empty(), "U is the FPGA letter: {flagged:?}");
}

/// A part in a user `sheet {}` block is judged on its whole refdes: `J_SOIL1`
/// is a connector called `J`, not one called `SOIL`, and `R_FB1` a resistor.
#[test]
fn an_underscored_refdes_in_a_user_sheet_is_judged_whole() {
    let src = "\
board \"t\" {
  sheet \"Sensors\" {
    component J_SOIL1: connector \"header_1x4\"
    component R_FB1: resistor \"r_generic_0603\"
  }
  connect J_SOIL1.p1 -> R_FB1.p1
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert!(
        flagged.is_empty(),
        "a user sheet's underscored refdes must not warn: {flagged:?}"
    );
}

/// A module instance prefixes its parts (`PU_R1`): the letter after the
/// instance prefix is the one judged.
#[test]
fn a_module_instance_part_is_judged_after_its_prefix() {
    let src = "\
board \"t\" {
  module \"Pullups\" (vdd: power, sda: bidirectional) {
    component R1: resistor \"r_generic_0402\"
    connect R1.p1 -> sda
    connect R1.p2 -> vdd
  }
  component J1: connector \"header_1x4\"
  power \"+3V3\" 3.3v { J1.p1 }
  use \"Pullups\" as PU {
    vdd -> \"+3V3\"
    sda -> \"I2C_SDA\"
  }
  connect J1.p2 -> \"I2C_SDA\"
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert!(
        flagged.is_empty(),
        "a module part's local refdes must be judged: {flagged:?}"
    );
    // The same module with a misnamed part warns, so the instance really was
    // expanded and checked rather than skipped.
    let misnamed = validate(&src.replace("R1", "X1"));
    let flagged = name_004(&misnamed);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
    assert!(
        flagged[0]
            .found
            .as_deref()
            .is_some_and(|f| f.contains("PU_X1")),
        "{flagged:?}"
    );
}

/// A sheet part whose letter is wrong both whole and after the last `_` still
/// warns: the second chance does not silence a real misnaming.
#[test]
fn a_sheet_part_wrong_either_way_still_warns() {
    let src = "\
board \"t\" {
  sheet \"Power\" {
    component X_FB1: resistor \"r_generic_0603\"
    component C1: capacitor \"c_generic_0603\"
  }
  connect X_FB1.p1 -> C1.p1
}
";
    let diags = validate(src);
    let flagged = name_004(&diags);
    assert_eq!(flagged.len(), 1, "{flagged:?}");
}
