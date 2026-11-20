// SPDX-License-Identifier: Apache-2.0

//! Issue #142: a keepout may declare what it is centred on and which nets
//! it must let through. The parser accepts `anchor "REFDES"` and a
//! comma-separated `allow "NET", …` list, and lowering carries both into
//! the IR so the exporter can place the rule area and the custom rules can
//! key on it.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn lower_keepouts(src: &str) -> Vec<synth_ir::Keepout> {
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("seed registry must load");
    let parse = synth_parser::parse(src, "inline.synth");
    assert!(
        !parse.has_errors(),
        "keepout source must parse: {:?}",
        parse
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    let ast = parse.ast.expect("ast present for a clean parse");
    let lowered = synth_ir::lower(&ast, &registry, "inline.synth");
    assert!(
        !lowered.has_errors(),
        "keepout source must lower: {:?}",
        lowered
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    lowered.board.expect("board present").keepouts
}

#[test]
fn an_anchor_and_an_allow_list_reach_the_ir() {
    let keepouts = lower_keepouts(
        r#"board "b" {
            layers 2
            keepout tmds_corridor {
                radius 6mm
                anchor "U1"
                allow "TMDS_D2+", "TMDS_D2-"
            }
        }"#,
    );
    let [ko] = keepouts.as_slice() else {
        panic!("one keepout expected, got {keepouts:?}");
    };
    assert_eq!(ko.name, "tmds_corridor");
    assert_eq!(ko.anchor.as_deref(), Some("U1"));
    assert_eq!(ko.allowed_nets, vec!["TMDS_D2+", "TMDS_D2-"]);
    assert!(ko.radius.is_some(), "the radius is lowered as before");
}

#[test]
fn a_keepout_without_the_new_attributes_keeps_them_empty() {
    let keepouts = lower_keepouts(
        r#"board "b" {
            layers 2
            keepout antenna { radius 5mm }
        }"#,
    );
    let [ko] = keepouts.as_slice() else {
        panic!("one keepout expected, got {keepouts:?}");
    };
    assert_eq!(ko.anchor, None);
    assert!(ko.allowed_nets.is_empty());
}

#[test]
fn allow_may_name_one_net() {
    let keepouts = lower_keepouts(
        r#"board "b" {
            layers 2
            keepout rf_feed { radius 4mm anchor "ANT" allow "ANT_IN" }
        }"#,
    );
    let [ko] = keepouts.as_slice() else {
        panic!("one keepout expected, got {keepouts:?}");
    };
    assert_eq!(ko.allowed_nets, vec!["ANT_IN".to_string()]);
}

#[test]
fn an_anchor_coordinate_and_layers_reach_the_ir() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parse = synth_parser::parse(
        r#"board "b" {
  layers 4
  component U1: mcu "rp2350"
  keepout hot_buck { radius 5mm anchor "U1" at 10mm 12mm layers "F.Cu", "B.Cu" }
}"#,
        "keepout.synth",
    );
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics);
    let board = synth_ir::lower(&parse.ast.unwrap(), &registry, "keepout.synth")
        .board
        .expect("a board");
    let keepout = board.keepouts.first().expect("a keepout");
    assert_eq!(keepout.anchor.as_deref(), Some("U1"));
    assert_eq!(
        keepout.at.map(|(x, y)| (x.to_mm(), y.to_mm())),
        Some((10.0, 12.0))
    );
    assert_eq!(keepout.layers, vec!["F.Cu", "B.Cu"]);
}
