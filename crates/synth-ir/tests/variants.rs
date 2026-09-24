// SPDX-License-Identifier: Apache-2.0

//! Design variants and structured component values (§Phase 7).

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn lower(src: &str) -> (Option<synth_ir::Board>, Vec<String>) {
    let parse = synth_parser::parse(src, "variants.synth");
    assert!(
        !parse.has_errors(),
        "parser diagnostics: {:?}",
        parse
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    let ast = parse.ast.expect("clean parse yields an ast");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, "variants.synth");
    (
        lowered.board,
        lowered.diagnostics.iter().map(|d| d.code.clone()).collect(),
    )
}

const SRC: &str = r#"board "b" {
    layers 2
    component U1: mcu "rp2350"
    component U2: sensor "bmp280_pressure"
    component C1: capacitor "c_generic_0603" value "100nF" tolerance "10%" voltage "25V" dielectric "X7R"
    connect U1.gp0 -> U2.sda
    variant "lite" description "no sensor" { dnp U2 }
}"#;

#[test]
fn variant_lowers_with_its_dnp_set() {
    let (board, codes) = lower(SRC);
    assert!(
        !codes.iter().any(|c| c.starts_with("E-SYNTH-VARIANT")),
        "{codes:?}"
    );
    let board = board.expect("board");
    assert_eq!(board.variants.len(), 1);
    let v = &board.variants[0];
    assert_eq!(v.name, "lite");
    assert_eq!(v.description.as_deref(), Some("no sensor"));
    assert_eq!(v.dnp, vec!["U2"]);
}

#[test]
fn structured_values_lower_onto_the_component() {
    let (board, _) = lower(SRC);
    let board = board.expect("board");
    let c1 = board
        .components
        .iter()
        .find(|c| c.refdes == "C1")
        .expect("C1");
    assert_eq!(c1.value.as_deref(), Some("100nF"));
    assert_eq!(
        c1.properties.get("Tolerance").map(String::as_str),
        Some("10%")
    );
    assert_eq!(
        c1.properties.get("Voltage").map(String::as_str),
        Some("25V")
    );
    assert_eq!(
        c1.properties.get("Dielectric").map(String::as_str),
        Some("X7R")
    );
    // A component with no structured values carries none.
    let u1 = board
        .components
        .iter()
        .find(|c| c.refdes == "U1")
        .expect("U1");
    assert!(u1.properties.is_empty());
}
