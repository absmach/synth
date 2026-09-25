// SPDX-License-Identifier: Apache-2.0

//! Failure corpus integration tests for `synth-place`.
//!
//! Loads every intentionally unsatisfiable fixture under `fixtures/place-errors/`
//! and asserts that:
//! 1. `place(&board)` returns an error (never `Ok(_)`)
//! 2. `err.to_diagnostics(&board, &file)` emits a valid `E-SYNTH-PLACE-*` diagnostic

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(path: &Path) -> Board {
    let source = fs::read_to_string(path).expect("read fixture");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ok");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry loads");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board lowered")
}

/// Place one intentionally-unsatisfiable fixture and verify it fails with a
/// structured `E-SYNTH-PLACE-*` diagnostic. Returns a message on regression so
/// the caller can aggregate failures instead of aborting the whole run.
fn check_placement_failure(path: &Path) -> Result<(), String> {
    let board = load_board(path);
    let file_str = path.display().to_string();

    let res = synth_place::place(&board);
    let Err(err) = res else {
        return Err(format!(
            "fixture {file_str} should have failed placement, but returned Ok"
        ));
    };

    let diagnostics = err.to_diagnostics(&board, &file_str);
    if diagnostics.is_empty() {
        return Err(format!("fixture {file_str} produced empty diagnostic list"));
    }
    let code = &diagnostics[0].code;
    if !code.starts_with("E-SYNTH-PLACE-") {
        return Err(format!(
            "fixture {file_str} diagnostic code {code} should start with E-SYNTH-PLACE-"
        ));
    }
    Ok(())
}

#[test]
fn failure_corpus_emits_structured_placement_errors() {
    let corpus_dir = workspace_root().join("fixtures").join("place-errors");
    let mut paths: Vec<PathBuf> = fs::read_dir(&corpus_dir)
        .expect("read place-errors dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();

    assert!(
        paths.len() >= 20,
        "expected at least 20 failure fixtures, found {}",
        paths.len()
    );

    // Placement of the overflow/oversized fixtures is the dominant cost and
    // each fixture is independent, so fan the corpus out across threads and
    // aggregate regressions instead of serialising the whole corpus.
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .min(paths.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = paths.get(index) else {
                    break;
                };
                if let Err(message) = check_placement_failure(path) {
                    failures.lock().expect("failure list").push(message);
                }
            });
        }
    });

    let failures = failures.into_inner().expect("failure list");
    assert!(
        failures.is_empty(),
        "placement failure corpus regressions:\n{}",
        failures.join("\n")
    );
}

#[test]
fn explicit_dimensions_are_used_and_reject_an_infeasible_outline() {
    let board = load_board(&workspace_root().join("examples/sensor_logger.synth"));

    let placement = synth_place::place_with_dimensions(&board, 160.0, 120.0)
        .expect("the reference design fits in the explicit outline");
    assert_eq!(
        placement.board_outline.width_nm(),
        synth_geometry::mm_to_nm(160.0)
    );
    assert_eq!(
        placement.board_outline.height_nm(),
        synth_geometry::mm_to_nm(120.0)
    );

    assert!(synth_place::place_with_dimensions(&board, 20.0, 20.0).is_err());
}
