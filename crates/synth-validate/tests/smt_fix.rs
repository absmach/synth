// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::time::Instant;
use synth_diagnostics::PatchKind;

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

fn validate_once(source: &str, filename: &str) -> Vec<synth_diagnostics::Diagnostic> {
    let parse = synth_parser::parse(source, filename.to_string());
    let mut diagnostics = parse.diagnostics;
    if let Some(ast) = parse.ast.as_ref() {
        let loader = synth_ir::MemoryImportLoader::default();
        let resolved = synth_ir::resolve_imports(ast, &loader, filename);
        diagnostics.extend(resolved.diagnostics);
        let registry = synth_registry::load_dir(&registry_dir()).expect("seed registry must load");
        let lowered = synth_ir::lower(&resolved.program, &registry, filename);
        diagnostics.extend(lowered.diagnostics);
        if let Some(board) = lowered.board.as_ref() {
            diagnostics.extend(synth_validate::run_erc(board, filename));
        }
    }
    diagnostics
}

#[test]
fn test_smt_patch_end_to_end_diff_pair() {
    let fixture_path = workspace_root()
        .join("fixtures")
        .join("erc")
        .join("E-SYNTH-DIFF-001__no_impedance.synth");
    let source = std::fs::read_to_string(&fixture_path).expect("fixture file exists");
    let filename = "E-SYNTH-DIFF-001__no_impedance.synth";

    let diags = validate_once(&source, filename);
    let diff_diag = diags
        .iter()
        .find(|d| d.code == "E-SYNTH-DIFF-001")
        .expect("E-SYNTH-DIFF-001 diagnostic must be emitted");

    let smt_patch = diff_diag
        .suggested_fixes
        .iter()
        .find(|p| matches!(p.kind, PatchKind::SolveSmt { .. }))
        .expect("SolveSmt patch must be present");

    if let PatchKind::SolveSmt {
        constraint,
        target_range,
        replacement_template,
    } = &smt_patch.kind
    {
        assert_eq!(constraint, "(assert (= impedance 90))");
        assert!(target_range.is_some(), "target_range must be present");
        assert!(
            replacement_template.is_some(),
            "replacement_template must be present"
        );
    } else {
        panic!("expected SolveSmt patch kind");
    }

    // Apply SMT patch (timed)
    let start = Instant::now();
    let patched_source = smt_patch.apply(&source).expect("SMT patch must apply");
    let elapsed = start.elapsed();

    // Verify patch result contains solved constraint
    assert!(
        patched_source.contains("impedance 90ohm"),
        "patched source should contain 'impedance 90ohm'"
    );

    // Re-validate and verify E-SYNTH-DIFF-001 is fixed
    let re_diags = validate_once(&patched_source, filename);
    let remaining_diff_diags = re_diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-DIFF-001" && d.severity.is_blocking())
        .count();
    assert_eq!(
        remaining_diff_diags, 0,
        "patched source must resolve E-SYNTH-DIFF-001"
    );

    // Performance gate: must resolve and patch in < 1ms
    assert!(
        elapsed.as_millis() < 1,
        "SMT end-to-end solve & patch elapsed time {elapsed:?} exceeded 1ms budget"
    );
}

#[test]
fn test_smt_patch_rf_feed_impedance() {
    let fixture_path = workspace_root()
        .join("fixtures")
        .join("erc")
        .join("E-SYNTH-RF-003__rf_no_impedance.synth");
    let source = std::fs::read_to_string(&fixture_path).expect("fixture file exists");
    let filename = "E-SYNTH-RF-003__rf_no_impedance.synth";

    let diags = validate_once(&source, filename);
    let rf_diag = diags
        .iter()
        .find(|d| d.code == "E-SYNTH-RF-003")
        .expect("E-SYNTH-RF-003 diagnostic must be emitted");

    let smt_patch = rf_diag
        .suggested_fixes
        .iter()
        .find(|p| matches!(p.kind, PatchKind::SolveSmt { .. }))
        .expect("SolveSmt patch must be present");

    let patched_source = smt_patch.apply(&source).expect("SMT patch must apply");
    assert!(
        patched_source.contains("50ohm"),
        "patched source should contain '50ohm'"
    );

    let re_diags = validate_once(&patched_source, filename);
    let remaining_rf_diags = re_diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-RF-003" && d.severity.is_blocking())
        .count();
    assert_eq!(
        remaining_rf_diags, 0,
        "patched source must resolve E-SYNTH-RF-003"
    );
}
