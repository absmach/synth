// SPDX-License-Identifier: Apache-2.0

//! `export_schematic_only` must produce schematic-side artifacts that are
//! byte-identical to the ones a full export writes for the same board.
//!
//! This is the contract the visual-feedback loop depends on: agents review
//! the cheap schematic-only render, then a release export runs the full
//! placer/router — if the two disagreed, the reviewed sheet would not be the
//! delivered sheet.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn fixtures() -> Vec<PathBuf> {
    let dir = workspace_root().join("fixtures").join("ir");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn schematic_only_matches_full_export_byte_for_byte() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let fixture_paths = fixtures();
    assert!(!fixture_paths.is_empty(), "no IR fixtures to check");

    for path in fixture_paths {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let parsed = synth_parser::parse(&src, filename.clone());
        let board = synth_ir::lower(&parsed.ast.expect("ast"), &registry, &filename)
            .board
            .expect("board");

        let full_dir = tempfile::tempdir().unwrap();
        let lite_dir = tempfile::tempdir().unwrap();

        let full = synth_kicad::export(&board, full_dir.path()).expect("full export");
        let lite = synth_kicad::export_schematic_only(&board, lite_dir.path(), None)
            .expect("schematic-only export");

        for (label, full_path, lite_path) in [
            ("project", &full.project_path, &lite.project_path),
            ("schematic", &full.schematic_path, &lite.schematic_path),
            ("library", &full.library_path, &lite.library_path),
        ] {
            let full_bytes = std::fs::read(full_path).unwrap();
            let lite_bytes = std::fs::read(lite_path).unwrap();
            assert_eq!(
                full_bytes, lite_bytes,
                "[{stem}] {label} differs between full and schematic-only export"
            );
        }

        let full_table = std::fs::read_to_string(full_dir.path().join("sym-lib-table")).unwrap();
        let lite_table = std::fs::read_to_string(lite_dir.path().join("sym-lib-table")).unwrap();
        assert_eq!(full_table, lite_table, "[{stem}] sym-lib-table differs");
    }
}
