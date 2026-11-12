// SPDX-License-Identifier: Apache-2.0

//! A declared keepout is a real KiCad rule area, on the declared layers and at
//! the declared anchor (#142, #449).

use std::path::Path;

use synth_geometry::{mm_to_nm, Point, Rect};

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
fn a_keepout_with_an_at_and_layers_is_exported() {
    let board = board(
        r#"board "b" {
  layers 4
  keepout antenna { radius 5mm at 10mm 12mm layers "F.Cu", "B.Cu" }
}"#,
    );
    let placement = synth_place::Placement {
        board_outline: Rect::from_center_half_extents(
            Point::new(0, 0),
            mm_to_nm(25.0),
            mm_to_nm(25.0),
        ),
        components: vec![],
    };
    let text = synth_kicad::build_pcb(
        &board,
        &placement,
        &synth_pcb::Routing::default(),
        &uuid::Uuid::nil(),
        &std::collections::HashMap::new(),
        &[],
        &[],
    )
    .expect("plane fanout")
    .to_string_pretty();
    assert!(text.contains("(name \"antenna keepout\")"), "{text}");
    assert!(text.contains("(tracks not_allowed)"), "{text}");
    assert!(text.contains("\"F.Cu\""), "{text}");
    assert!(text.contains("\"B.Cu\""), "{text}");
    // Centred on the declared coordinate, not the board centre.
    assert!(
        text.contains("(xy 15 "),
        "the 5 mm radius reaches x=15: {text}"
    );
}
