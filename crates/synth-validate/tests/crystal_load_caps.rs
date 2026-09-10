// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for `E-SYNTH-CRYSTAL-001`: a crystal whose two
//! load capacitors are unbalanced (e.g. C1 = 22 pF, C2 = 33 pF) emits
//! the warning; a balanced pair is clean.

use std::path::{Path, PathBuf};

use synth_diagnostics::Diagnostic;

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
    let filename = "crystal_load_caps.synth".to_string();
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

fn find_crystal_001(diags: &[Diagnostic]) -> Option<&Diagnostic> {
    diags.iter().find(|d| d.code == "E-SYNTH-CRYSTAL-001")
}

/// A minimal crystal with two load caps on separate pins, tied together
/// on the far side as the common ground return.
fn crystal_board(cap1: &str, cap2: &str) -> String {
    format!(
        "\
board \"t\" {{
  component Y1: crystal \"xtal_generic\"
  component C1: capacitor \"c_generic_0603\" value \"{cap1}\"
  component C2: capacitor \"c_generic_0603\" value \"{cap2}\"
  connect Y1.p1 -> C1.p1
  connect Y1.p2 -> C2.p1
  connect C1.p2 -> C2.p2
}}
"
    )
}

#[test]
fn unbalanced_load_caps_emit_crystal_001() {
    let (_board, diags) = validate(&crystal_board("22pF", "33pF"));
    let diag =
        find_crystal_001(&diags).expect("22 pF vs 33 pF load caps must emit E-SYNTH-CRYSTAL-001");
    assert_eq!(diag.severity, synth_diagnostics::Severity::Warning);
    let found = diag.found.as_deref().unwrap_or_default();
    assert!(
        found.contains("22.000pF") && found.contains("33.000pF"),
        "{found}"
    );
}

#[test]
fn balanced_load_caps_are_clean() {
    let (_board, diags) = validate(&crystal_board("18pF", "18pF"));
    assert!(
        find_crystal_001(&diags).is_none(),
        "equal load caps must not emit E-SYNTH-CRYSTAL-001"
    );
}

#[test]
fn near_balanced_load_caps_within_tolerance_are_clean() {
    // 18 pF vs 22 pF differs by ~18% — outside tolerance.
    let (_, diags) = validate(&crystal_board("18pF", "22pF"));
    assert!(
        find_crystal_001(&diags).is_some(),
        "18 pF vs 22 pF differs by 18% and must warn"
    );

    // 10 nF vs 11 nF differs by ~9% — inside the 10% tolerance.
    let (_, diags) = validate(&crystal_board("10nF", "11nF"));
    assert!(
        find_crystal_001(&diags).is_none(),
        "10 nF vs 11 nF is within tolerance and must stay clean"
    );
}

#[test]
fn unvalued_caps_are_skipped() {
    // No value strings — the rule cannot parse, so it stays silent
    // rather than guessing.
    let (_, diags) = validate(&crystal_board("", ""));
    assert!(
        find_crystal_001(&diags).is_none(),
        "unvalued caps must not emit E-SYNTH-CRYSTAL-001"
    );
}
