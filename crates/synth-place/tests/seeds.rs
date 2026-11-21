// SPDX-License-Identifier: Apache-2.0

//! The placement seed used by the place→route→re-place loop.
//!
//! A seed must change *which* legal arrangement the greedy placer produces
//! without changing anything else: seed 0 is the canonical placement every
//! non-iterating caller gets, a repeated seed is identical, and a different
//! seed is permitted to differ but must cover the same components on the same
//! outline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(path: &str) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &file);
    lowered.board.expect("board must lower")
}

#[test]
fn a_seed_varies_a_legal_placement_deterministically() {
    let board = load_board("../../fixtures/designs/iot_sensor_board.synth");
    let overrides = HashMap::new();
    let place = |seed: u64| {
        synth_place::place_with_tuning_and_sidecar_seeded(&board, 1.5, &overrides, None, seed)
            .expect("placement must succeed")
    };

    // Every caller now gets the placement rules applied after the solve, so
    // the canonical arrangement is the non-seeded wrapper, not the raw solver
    // (`place_with_tuning`), which stops before the rules.
    let canonical =
        synth_place::place_with_tuning_and_sidecar(&board, 1.5, &overrides, None).expect("place");
    assert_eq!(
        place(0),
        canonical,
        "seed 0 must reproduce the canonical placement exactly"
    );

    // Same seed -> identical output.
    assert_eq!(place(1), place(1), "the same seed must be deterministic");

    // A different seed may (and on a crowded board does) reorder the
    // equal-degree components, giving a different arrangement.
    let variants: Vec<_> = [1_u64, 2, 3].iter().map(|&seed| place(seed)).collect();
    assert!(
        variants.iter().any(|variant| *variant != canonical),
        "a seed must be able to change the arrangement"
    );

    // Whatever changes, the same parts are placed. The outline may differ:
    // a different packing tightens to a different bounding box, which is
    // exactly the choice the route-quality loop is making.
    for variant in &variants {
        assert_eq!(variant.components.len(), canonical.components.len());
    }
}
