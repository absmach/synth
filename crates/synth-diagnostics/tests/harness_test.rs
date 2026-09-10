// SPDX-License-Identifier: Apache-2.0

//! Integration benchmark tests for `AgentHarness` on the `fixtures/agent/` corpus.

use std::path::PathBuf;
use synth_diagnostics::harness::{AgentHarness, HarnessStrategy};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn validate_design(source: &str, file_name: &str) -> Vec<synth_diagnostics::Diagnostic> {
    let parse = synth_parser::parse(source, file_name.to_string());
    let mut diagnostics = parse.diagnostics;
    if let Some(ast) = parse.ast.as_ref() {
        let root = workspace_root();
        let loader = synth_ir::FsImportLoader {
            root: root.join("fixtures").join("agent"),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, file_name);
        diagnostics.extend(resolved.diagnostics);

        let registry_dir = root.join("registry").join("parts");
        if let Ok(reg) = synth_registry::load_dir(&registry_dir) {
            let lowered = synth_ir::lower(&resolved.program, &reg, file_name);
            diagnostics.extend(lowered.diagnostics);
            if let Some(board) = lowered.board.as_ref() {
                diagnostics.extend(synth_validate::run_erc(board, file_name));
            }
        }
    }
    diagnostics
}

fn collect_synth_files(dir: &std::path::Path, paths: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_synth_files(&path, paths);
            } else if path.extension().is_some_and(|x| x == "synth") {
                let filename = path.file_name().unwrap().to_string_lossy();
                if !filename.starts_with("noconverge__") {
                    paths.push(path);
                }
            }
        }
    }
}

#[test]
#[allow(clippy::similar_names, clippy::cast_precision_loss)]
fn agent_harness_benchmark_corpus() {
    let agent_dir = workspace_root().join("fixtures").join("agent");
    let mut fixture_paths = Vec::new();
    collect_synth_files(&agent_dir, &mut fixture_paths);
    fixture_paths.sort();

    assert!(
        !fixture_paths.is_empty(),
        "expected at least 1 agent fixture in fixtures/agent/"
    );

    let consequence_harness = AgentHarness::new(10, HarnessStrategy::ConsequenceModel);
    let confidence_harness = AgentHarness::new(10, HarnessStrategy::ConfidenceOnly);

    let mut consequence_converged = 0usize;
    let mut consequence_total_iters = 0usize;
    let mut confidence_total_iters = 0usize;
    let total_fixtures = fixture_paths.len();

    for path in &fixture_paths {
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(path).unwrap();

        let res_cons = consequence_harness.run(&source, |s| validate_design(s, &file_name));
        let res_conf = confidence_harness.run(&source, |s| validate_design(s, &file_name));

        if res_cons.converged {
            consequence_converged += 1;
        }
        consequence_total_iters += res_cons.iterations;
        confidence_total_iters += res_conf.iterations;
    }

    let convergence_rate = (consequence_converged as f64) / (total_fixtures as f64);
    println!(
        "Harness Benchmark Results: {consequence_converged}/{total_fixtures} ({:.1}%) converged in <= 10 iterations",
        convergence_rate * 100.0
    );
    println!(
        "Consequence Model Iterations: {consequence_total_iters} vs Confidence Only Iterations: {confidence_total_iters}"
    );

    // Gate: ≥80% convergence on seeded broken corpus
    assert!(
        convergence_rate >= 0.80,
        "convergence rate {:.1}% below target gate 80%",
        convergence_rate * 100.0
    );
}
