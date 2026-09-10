// SPDX-License-Identifier: Apache-2.0

//! Fixture-driven ERC tests using the twin-pair convention.
//!
//! Every `fixtures/erc/pass__<name>.synth` must validate clean (no
//! error-level diagnostics through parse → lower → ERC).
//!
//! Every `fixtures/erc/<CODE>__<name>.synth` must emit at least one
//! diagnostic with the named `<CODE>`, anchored to the fixture file.

use std::fs;
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

fn erc_fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures").join("erc")
}

fn read_synth_fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "synth" || ext == "synthj")
        })
        .collect();
    paths.sort();
    paths
}

fn run_pipeline(fixture: &Path) -> Vec<synth_diagnostics::Diagnostic> {
    let src = fs::read_to_string(fixture).unwrap();
    let filename = fixture.file_name().unwrap().to_string_lossy().to_string();
    let parse = if src.trim_start().starts_with('{') {
        synth_parser::json::parse_json(&src, filename.clone())
    } else {
        synth_parser::parse(&src, filename.clone())
    };
    let ast = parse.ast.as_ref().unwrap_or_else(|| {
        panic!(
            "{}: parser failed; diagnostics: {:?}",
            filename, parse.diagnostics
        )
    });
    let registry = synth_registry::load_dir(&registry_dir()).expect("seed registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);

    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &filename));
    }
    all
}

#[test]
fn every_pass_fixture_validates_clean() {
    let mut any = false;
    for path in read_synth_fixtures(&erc_fixtures_dir()) {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if !stem.starts_with("pass__") {
            continue;
        }
        any = true;
        let diags = run_pipeline(&path);
        let blocking: Vec<&str> = diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            blocking.is_empty(),
            "{stem}: expected clean validate but got error-level diagnostics: {blocking:?}"
        );
    }
    assert!(any, "no pass__*.synth fixtures found under fixtures/erc/");
}

#[test]
fn every_failure_fixture_emits_its_named_code() {
    let mut any = false;
    for path in read_synth_fixtures(&erc_fixtures_dir()) {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let Some((expected_code, _)) = stem.split_once("__") else {
            panic!(
                "fixture {stem} does not follow CODE__name.synth or pass__name.synth convention"
            );
        };
        if expected_code == "pass" {
            continue;
        }
        any = true;
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let diags = run_pipeline(&path);
        let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&expected_code),
            "{stem}: expected diagnostic {expected_code} not emitted; got {codes:?}"
        );
        for d in &diags {
            let loc = d
                .location
                .as_ref()
                .unwrap_or_else(|| panic!("{stem}: diagnostic {} missing location", d.code));
            assert_eq!(
                loc.file, filename,
                "{stem}: diagnostic {} anchored to {} not {}",
                d.code, loc.file, filename
            );
        }
    }
    assert!(
        any,
        "no <CODE>__*.synth failure fixtures found under fixtures/erc/"
    );
}
