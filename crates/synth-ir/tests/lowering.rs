// SPDX-License-Identifier: Apache-2.0

//! Fixture-driven lowering tests.
//!
//! Every `fixtures/ir/*.synth` is parsed, lowered against the seed
//! registry, and the resulting `Board` is snapshotted as JSON.
//! Lowering must produce zero error-level diagnostics on these
//! curated inputs; the snapshot pins the exact IR shape.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn read_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn every_ir_fixture_lowers_clean_and_stable() {
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("seed registry must load");

    let fixture_dir = workspace_root().join("fixtures").join("ir");
    let mut any = false;
    for path in read_sorted(&fixture_dir) {
        any = true;
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        let parse = synth_parser::parse(&src, filename.clone());
        assert!(
            !parse.has_errors(),
            "{stem}: parser diagnostics: {:?}",
            parse
                .diagnostics
                .iter()
                .map(|d| &d.code)
                .collect::<Vec<_>>(),
        );
        let ast = parse.ast.expect("ast must be present for clean parse");

        let lowered = synth_ir::lower(&ast, &registry, &filename);
        let codes: Vec<&str> = lowered
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            !lowered.has_errors(),
            "{stem}: lowering diagnostics: {codes:?}"
        );
        let board = lowered.board.expect("board must be present");

        insta::with_settings!(
            { snapshot_suffix => &stem, sort_maps => true },
            { insta::assert_json_snapshot!(board); }
        );
    }
    assert!(any, "no fixtures under fixtures/ir/");
}
