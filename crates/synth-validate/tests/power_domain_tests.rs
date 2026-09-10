// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

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
fn test_power_domain_clash_emits_power_005() {
    let fixture_path = workspace_root()
        .join("fixtures")
        .join("erc")
        .join("E-SYNTH-POWER-005__domain_mismatch.synth");
    let source = std::fs::read_to_string(&fixture_path).expect("fixture file exists");
    let filename = "E-SYNTH-POWER-005__domain_mismatch.synth";

    let diags = validate_once(&source, filename);
    let power_diag = diags
        .iter()
        .find(|d| d.code == "E-SYNTH-POWER-005")
        .expect("E-SYNTH-POWER-005 diagnostic must be emitted for 5V and 3.3V power output clash");

    assert!(
        power_diag.title.contains("clash") || power_diag.title.contains("mismatch"),
        "Diagnostic title should indicate voltage domain mismatch/clash"
    );
}

#[test]
fn test_level_shifter_prevents_power_005() {
    let fixture_path = workspace_root()
        .join("fixtures")
        .join("erc")
        .join("pass__power_005_level_shifter.synth");
    let source = std::fs::read_to_string(&fixture_path).expect("fixture file exists");
    let filename = "pass__power_005_level_shifter.synth";

    let diags = validate_once(&source, filename);
    let power_005_count = diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-POWER-005")
        .count();
    assert_eq!(
        power_005_count, 0,
        "Level shifter isolated domains should not trigger E-SYNTH-POWER-005"
    );
}
