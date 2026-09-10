// SPDX-License-Identifier: Apache-2.0

//! Reference agent harness — Phase 6 gate test.
//!
//! Each fixture under `fixtures/agent/` is a broken `.synth` design.
//! The harness simulates the agent loop:
//!
//! 1. Run the full validate pipeline (parse → import-resolve → lower → ERC).
//! 2. Pick the highest-confidence `suggested_fix` per diagnostic.
//! 3. Apply patches in reverse byte order (so earlier patches don't
//!    shift later byte offsets).
//! 4. Re-validate.
//! 5. Stop on a clean run (no blocking diagnostics) or after
//!    [`MAX_ITERATIONS`] passes.
//!
//! Gate: convergence on ≥80% of fixtures in ≤5 iterations.
//!
//! This is not a real agent — it never reads candidates, it never
//! chooses between alternatives, and it never refuses to apply a
//! patch. It is the *floor* of agent capability: the contract is that
//! a deterministic patch-picker converges. Smarter agents do better.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Patch, PatchKind};

const MAX_ITERATIONS: usize = 5;
const CONVERGENCE_GATE: f64 = 0.80;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn agent_fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures").join("agent")
}

fn registry_dir() -> PathBuf {
    workspace_root().join("registry").join("parts")
}

fn read_fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_synth_files(dir, &mut paths);
    paths.sort();
    paths
}

fn collect_synth_files(dir: &Path, paths: &mut Vec<PathBuf>) {
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

/// One pass of the validate pipeline. Returns every diagnostic
/// emitted — the harness ignores severity ordering and just looks for
/// any blocking entry.
fn validate_once(source: &str, filename: &str) -> Vec<Diagnostic> {
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

fn patch_anchor(p: &Patch) -> u32 {
    match &p.kind {
        PatchKind::ReplaceRange { range, .. } | PatchKind::DeleteRange { range } => {
            range.byte_start
        }
        PatchKind::InsertAt { at, .. } => *at,
        PatchKind::SolveSmt {
            target_range: Some(range),
            ..
        } => range.byte_start,
        PatchKind::AddStatement { .. }
        | PatchKind::RemoveStatement { .. }
        | PatchKind::SolveSmt { .. } => u32::MAX,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ConvergeResult {
    Clean { iterations: usize },
    Stalled { iterations: usize, remaining: usize },
    MaxIterations { remaining: usize },
}

fn try_converge(source: &str, filename: &str) -> ConvergeResult {
    let mut current = source.to_string();
    for iter in 0..MAX_ITERATIONS {
        let diags = validate_once(&current, filename);
        let blocking = diags.iter().filter(|d| d.severity.is_blocking()).count();
        if blocking == 0 {
            return ConvergeResult::Clean { iterations: iter };
        }
        let mut patches: Vec<Patch> = diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .filter_map(|d| d.suggested_fixes.first().cloned())
            .collect();
        if patches.is_empty() {
            return ConvergeResult::Stalled {
                iterations: iter,
                remaining: blocking,
            };
        }
        patches.sort_by_key(|p| std::cmp::Reverse(patch_anchor(p)));
        let mut applied_any = false;
        for patch in &patches {
            if let Ok(next) = patch.apply(&current) {
                current = next;
                applied_any = true;
            }
        }
        if !applied_any {
            return ConvergeResult::Stalled {
                iterations: iter,
                remaining: blocking,
            };
        }
    }
    let final_diags = validate_once(&current, filename);
    let remaining = final_diags
        .iter()
        .filter(|d| d.severity.is_blocking())
        .count();
    ConvergeResult::MaxIterations { remaining }
}

#[test]
fn agent_harness_converges_on_corpus() {
    let fixtures = read_fixtures(&agent_fixtures_dir());
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under fixtures/agent/"
    );

    let mut converged = 0_usize;
    let mut report: Vec<String> = Vec::new();
    for path in &fixtures {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let outcome = try_converge(&source, &filename);
        match outcome {
            ConvergeResult::Clean { iterations } => {
                converged += 1;
                report.push(format!("  ✓ {stem} (clean after {iterations} iter)"));
            }
            ConvergeResult::Stalled {
                iterations,
                remaining,
            } => {
                report.push(format!(
                    "  ✗ {stem} stalled at iter {iterations} with {remaining} blocking diag(s)"
                ));
            }
            ConvergeResult::MaxIterations { remaining } => {
                report.push(format!(
                    "  ✗ {stem} did not converge in {MAX_ITERATIONS} iter ({remaining} blocking)"
                ));
            }
        }
    }
    let total = fixtures.len();
    #[allow(clippy::cast_precision_loss)] // ≪ 2^52 fixtures
    let rate = converged as f64 / total as f64;
    let report = report.join("\n");
    println!(
        "agent harness convergence: {converged}/{total} = {rate:.0}%\n{report}",
        rate = rate * 100.0,
    );
    assert!(
        rate >= CONVERGENCE_GATE,
        "convergence rate {rate:.2} < gate {CONVERGENCE_GATE:.2} ({converged}/{total})\n{report}"
    );
}

#[test]
fn convergence_is_deterministic() {
    // Run the harness twice over the corpus; outcomes must match.
    let fixtures = read_fixtures(&agent_fixtures_dir());
    for path in &fixtures {
        let source = std::fs::read_to_string(path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let r1 = try_converge(&source, &filename);
        let r2 = try_converge(&source, &filename);
        assert_eq!(
            r1,
            r2,
            "agent loop is non-deterministic for {}",
            path.display()
        );
    }
}

#[test]
#[allow(clippy::similar_names)]
fn consequence_model_harness_evaluation() {
    use synth_diagnostics::harness::{AgentHarness, HarnessStrategy};

    let fixtures = read_fixtures(&agent_fixtures_dir());
    let consequence_harness = AgentHarness::new(5, HarnessStrategy::ConsequenceModel);
    let confidence_harness = AgentHarness::new(5, HarnessStrategy::ConfidenceOnly);

    let mut cons_iters = 0_usize;
    let mut conf_iters = 0_usize;

    for path in &fixtures {
        let source = std::fs::read_to_string(path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        let r_cons = consequence_harness.run(&source, |src| validate_once(src, &filename));
        let r_conf = confidence_harness.run(&source, |src| validate_once(src, &filename));

        cons_iters += r_cons.iterations;
        conf_iters += r_conf.iterations;
    }

    println!(
        "Harness Strategy comparison over {} fixtures: ConsequenceModel={cons_iters} total iters, ConfidenceOnly={conf_iters} total iters",
        fixtures.len()
    );
    assert!(
        cons_iters <= conf_iters,
        "ConsequenceModel total iterations ({cons_iters}) should be <= ConfidenceOnly ({conf_iters})"
    );
}
