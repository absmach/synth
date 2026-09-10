// SPDX-License-Identifier: Apache-2.0

//! Failure corpus integration tests for `synth-place`.
//!
//! Loads every intentionally unsatisfiable fixture under `fixtures/place-errors/`
//! and asserts that:
//! 1. `place(&board)` returns an error (never `Ok(_)`)
//! 2. `err.to_diagnostics(&board, &file)` emits a valid `E-SYNTH-PLACE-*` diagnostic

use std::fs;
use std::path::{Path, PathBuf};
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

#[test]
fn failure_corpus_emits_structured_placement_errors() {
    let corpus_dir = workspace_root().join("fixtures").join("place-errors");
    let entries = fs::read_dir(&corpus_dir).expect("read place-errors dir");

    let mut tested_count = 0;
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "synth") {
            tested_count += 1;
            let board = load_board(&path);
            let file_str = path.display().to_string();

            let res = synth_place::place(&board);
            assert!(
                res.is_err(),
                "fixture {} should have failed placement, but returned Ok",
                path.display()
            );

            let err = res.unwrap_err();
            let diagnostics = err.to_diagnostics(&board, &file_str);
            assert!(
                !diagnostics.is_empty(),
                "fixture {} produced empty diagnostic list",
                path.display()
            );

            let code = &diagnostics[0].code;
            assert!(
                code.starts_with("E-SYNTH-PLACE-"),
                "fixture {} diagnostic code {} should start with E-SYNTH-PLACE-",
                path.display(),
                code
            );
        }
    }

    assert!(
        tested_count >= 20,
        "expected at least 20 failure fixtures, found {tested_count}"
    );
}
