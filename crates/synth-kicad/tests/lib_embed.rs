// SPDX-License-Identifier: Apache-2.0

//! Embedded-library integrity tests: every `lib_id` an instance
//! references must have a matching definition inside `lib_symbols`.
//!
//! This is the contract that broke on machines without a KiCad
//! install before the fallback-embed fix: instances kept their
//! registry lib_ids (`Device:R`, ...) while the synthesized fallback
//! definitions were published under `synth:<part_id>`, leaving KiCad
//! nothing to resolve and rendering `?` placeholders everywhere.

use std::fs;
use std::path::{Path, PathBuf};

/// Flatten pretty-printed s-expression text so `(symbol\n\t"X"` and
/// `(symbol "X"` both match a simple substring probe.
fn flatten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = false;
    for c in text.chars() {
        if c.is_whitespace() {
            prev_ws = true;
        } else {
            if prev_ws && !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = false;
            out.push(c);
        }
    }
    out
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "synth-kicad-libembed-{label}-{}",
        std::process::id()
    ));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

#[test]
fn every_referenced_lib_id_has_an_embedded_definition() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let mut checked = 0;

    let mut fixtures: Vec<PathBuf> = fs::read_dir(workspace_root().join("fixtures").join("ir"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    fixtures.sort();

    for path in fixtures {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let parsed = synth_parser::parse(&src, name.clone());
        let ast = parsed.ast.expect("ast");
        let board = synth_ir::lower(&ast, &registry, &name)
            .board
            .expect("board");

        // Component-less fixtures (e.g. diff_pair_with_keepout, which
        // only exercises `diff_pair`/`keepout` lowering) legitimately
        // export a schematic with no symbol instances, so there is
        // nothing to verify here.
        if board.components.is_empty() {
            continue;
        }

        let out = tempdir(&name);
        let result = synth_kicad::export(&board, &out).expect("export");
        let schematic = fs::read_to_string(&result.schematic_path).unwrap();
        let flat = flatten(&schematic);

        // Collect every instance lib_id.
        let mut lib_ids = Vec::new();
        let mut rest = flat.as_str();
        while let Some(idx) = rest.find("(lib_id \"") {
            let start = idx + "(lib_id \"".len();
            let end = rest[start..].find('"').unwrap() + start;
            lib_ids.push(rest[start..end].to_string());
            rest = &rest[end..];
        }
        assert!(
            !lib_ids.is_empty(),
            "{name}: expected at least one symbol instance"
        );

        for lib_id in &lib_ids {
            let needle = format!("(symbol \"{lib_id}\"");
            assert!(
                flat.contains(&needle),
                "{name}: instance references `{lib_id}` but no matching \
                 definition exists in lib_symbols"
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "expected at least one fixture to exercise");
}

#[test]
fn pwr_flag_fallback_publishes_power_out_pin_under_stock_name() {
    let def = synth_kicad::build_pwr_flag_fallback();
    let flat = flatten(&def.to_string_pretty());
    assert!(
        flat.contains("(symbol \"power:PWR_FLAG\""),
        "fallback must be published under the exact name instances \
         reference (`power:PWR_FLAG`), got: {flat}"
    );
    assert!(
        flat.contains("power_out"),
        "PWR_FLAG's whole ERC purpose is its power_out driver pin"
    );
}
