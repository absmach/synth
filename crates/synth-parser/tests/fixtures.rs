// SPDX-License-Identifier: Apache-2.0

//! Fixture-driven tests.
//!
//! Every `fixtures/designs/*.synth` must parse without error; the
//! resulting AST is snapshotted as JSON via insta.
//!
//! Every `fixtures/parse-errors/<CODE>__<name>.synth` must produce at
//! least one diagnostic with the named code, with location pointing
//! back to the fixture, and recovery must keep the diagnostic count
//! bounded (≤ 4 on these small fixtures).

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
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
fn every_positive_design_parses_clean() {
    let dir = workspace_root().join("fixtures").join("designs");
    let mut any = false;
    for path in read_dir_sorted(&dir) {
        any = true;
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let result = synth_parser::parse(&src, path.file_name().unwrap().to_string_lossy());

        assert!(
            !result.has_errors(),
            "{stem}: expected clean parse but got diagnostics: {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| &d.code)
                .collect::<Vec<_>>()
        );
        let ast = result
            .ast
            .unwrap_or_else(|| panic!("{stem}: parser returned no AST"));

        // Snapshot the AST as JSON. Insta names snapshots after the test
        // function; setting a per-fixture name keeps one file per design.
        insta::with_settings!(
            { snapshot_suffix => &stem, sort_maps => true },
            { insta::assert_json_snapshot!(ast); }
        );
    }
    assert!(any, "no positive fixtures found in fixtures/designs/");
}

#[test]
fn every_parse_error_fixture_emits_its_named_code() {
    let dir = workspace_root().join("fixtures").join("parse-errors");
    let mut any = false;
    for path in read_dir_sorted(&dir) {
        any = true;
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let Some((expected_code, _)) = stem.split_once("__") else {
            panic!("fixture {stem} does not follow CODE__name.synth convention");
        };
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let result = synth_parser::parse(&src, filename.clone());

        let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&expected_code),
            "{stem}: expected diagnostic {expected_code} not emitted; got {codes:?}"
        );

        // Recovery should bound the blast radius. These small fixtures
        // contain a single mistake; allow at most 4 diagnostics so we
        // catch regressions where one error fans out.
        assert!(
            result.diagnostics.len() <= 4,
            "{stem}: diagnostic count = {} (codes: {:?})",
            result.diagnostics.len(),
            codes
        );

        // Every diagnostic must carry a location anchored to this file.
        for d in &result.diagnostics {
            let loc = d
                .location
                .as_ref()
                .unwrap_or_else(|| panic!("{stem}: diagnostic {} missing location", d.code));
            assert_eq!(
                loc.file, filename,
                "{stem}: diagnostic {} location refers to {} not {}",
                d.code, loc.file, filename
            );
        }
    }
    assert!(
        any,
        "no parse-error fixtures found in fixtures/parse-errors/"
    );
}
