// SPDX-License-Identifier: Apache-2.0

//! `no_connect` declares pins that are intentionally unused (#431).

use std::path::Path;

fn board(src: &str) -> synth_ir::Board {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
    let parse = synth_parser::parse(src, "nc.synth");
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics);
    synth_ir::lower(&parse.ast.unwrap(), &registry, "nc.synth")
        .board
        .expect("a board")
}

#[test]
fn declared_no_connects_reach_the_ir() {
    let board = board(
        r#"board "b" {
  layers 2
  component U1: memory "24lc256_soic8"
  no_connect U1.a0, U1.a1
}"#,
    );
    assert_eq!(board.no_connects.len(), 2);
    assert_eq!(board.no_connects[0].refdes, "U1");
    assert_eq!(board.no_connects[0].pin, "a0");
    assert_eq!(board.no_connects[1].pin, "a1");
}

#[test]
fn an_unknown_no_connect_pin_is_an_error() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
    let parse = synth_parser::parse(
        r#"board "b" {
  layers 2
  component U1: memory "24lc256_soic8"
  no_connect U1.no_such_pin
}"#,
        "nc.synth",
    );
    let lowered = synth_ir::lower(&parse.ast.unwrap(), &registry, "nc.synth");
    assert!(
        lowered
            .diagnostics
            .iter()
            .any(|d| d.code == "E-SYNTH-COMP-006"),
        "{:?}",
        lowered.diagnostics
    );
}
