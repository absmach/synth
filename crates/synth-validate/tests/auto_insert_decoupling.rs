// SPDX-License-Identifier: Apache-2.0

//! End-to-end test for `E-SYNTH-POWER-001` auto-insertion: a design
//! missing a required decoupling capacitor emits a `PatchKind::InsertAt`
//! fix that, when applied to the source, adds the cap + connects so a
//! re-validation is clean.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, PatchKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn registry_dir() -> PathBuf {
    workspace_root().join("registry").join("parts")
}

fn validate(src: &str) -> (Option<synth_ir::Board>, Vec<Diagnostic>) {
    let filename = "auto_insert_decoupling.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&registry_dir()).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &filename));
    }
    (lowered.board, all)
}

fn find_power_001(diags: &[Diagnostic]) -> Option<&Diagnostic> {
    diags.iter().find(|d| d.code == "E-SYNTH-POWER-001")
}

#[test]
fn missing_decoupling_emits_insertable_patch() {
    let src = "\
board \"t\" {
  component U1: sensor \"bme680_env\"
  component R1: resistor \"r_generic_0603\"
  connect U1.gnd -> R1.p1
  connect U1.vdd -> R1.p2
}
";
    let (_board, diags) = validate(src);

    let diag = find_power_001(&diags).expect("missing cap must emit E-SYNTH-POWER-001");
    let patch = diag
        .suggested_fixes
        .first()
        .expect("auto-insert patch must be present");
    assert!(
        matches!(patch.kind, PatchKind::InsertAt { .. }),
        "expected InsertAt patch, got {:?}",
        patch.kind
    );
    assert!(
        patch.confidence >= 0.9,
        "auto-insert patch should be high confidence"
    );
}

#[test]
fn applying_patch_adds_cap_and_revalidates_clean() {
    let src = "\
board \"t\" {
  component U1: sensor \"bme680_env\"
  component R1: resistor \"r_generic_0603\"
  connect U1.gnd -> R1.p1
  connect U1.vdd -> R1.p2
}
";
    let (_, diags) = validate(src);
    let diag = find_power_001(&diags).expect("missing cap must emit E-SYNTH-POWER-001");
    let patch = diag
        .suggested_fixes
        .first()
        .expect("auto-insert patch must be present");

    let patched = patch
        .apply(src)
        .expect("InsertAt patch must apply without conflict");
    assert!(patched.contains("component C1: capacitor"), "{patched}");

    let (board, re_diags) = validate(&patched);
    assert!(
        board.is_some(),
        "patched source must still lower to a board"
    );
    assert!(
        find_power_001(&re_diags).is_none(),
        "decoupling cap present, so E-SYNTH-POWER-001 must clear"
    );

    // The inserted cap must actually sit on the vdd net.
    let board = board.unwrap();
    let u1 = board.components.iter().find(|c| c.refdes == "U1").unwrap();
    let vdd_pin = u1
        .part
        .as_ref()
        .unwrap()
        .pins
        .iter()
        .position(|p| p.name == "vdd")
        .unwrap();
    let caps_on_vdd = board
        .nets_containing(u1.id, synth_ir::PinId(vdd_pin as u32))
        .flat_map(|(_, net)| net.endpoints.iter())
        .filter(|e| {
            board
                .component(e.component)
                .and_then(|c| c.part.as_ref())
                .is_some_and(|p| p.kind == "capacitor")
        })
        .count();
    assert_eq!(caps_on_vdd, 1, "exactly one decoupling cap on U1.vdd");
}

#[test]
fn decoupling_present_is_clean() {
    let src = "\
board \"t\" {
  component U1: sensor \"bme680_env\"
  component C1: capacitor \"c_generic_0603\"
  component R1: resistor \"r_generic_0603\"
  connect U1.gnd -> C1.p2
  connect U1.vdd -> C1.p1
  connect U1.gnd -> R1.p1
  connect U1.vdd -> R1.p2
}
";
    let (_, diags) = validate(src);
    assert!(
        find_power_001(&diags).is_none(),
        "cap present: no E-SYNTH-POWER-001"
    );
}
