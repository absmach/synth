// SPDX-License-Identifier: Apache-2.0

//! A keepout can name its anchor, coordinate and layers (#449).

use std::path::Path;

fn board(src: &str) -> synth_ir::Board {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
    let parse = synth_parser::parse(src, "keepout.synth");
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics);
    synth_ir::lower(&parse.ast.unwrap(), &registry, "keepout.synth")
        .board
        .expect("a board")
}

#[test]
fn an_anchor_coordinate_and_layers_reach_the_ir() {
    let board = board(
        r#"board "b" {
  layers 4
  component U1: mcu "rp2350"
  keepout hot_buck { radius 5mm anchor "U1" at 10mm 12mm layers "F.Cu", "B.Cu" }
}"#,
    );
    let keepout = board.keepouts.first().expect("a keepout");
    assert_eq!(keepout.anchor.as_deref(), Some("U1"));
    assert_eq!(
        keepout.at.map(|(x, y)| (x.to_mm(), y.to_mm())),
        Some((10.0, 12.0))
    );
    assert_eq!(keepout.layers, vec!["F.Cu", "B.Cu"]);
}
