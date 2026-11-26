// SPDX-License-Identifier: Apache-2.0

//! Spacing reserves room for silk reference labels: from 2 mm up no label may
//! sit on a courtyard or on another label.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn rtd_afe(spacing_mm: Option<f64>) -> synth_ir::Board {
    let path = root().join("crates/synth-place/tests/fixtures/rtd_afe.synth");
    let source = std::fs::read_to_string(&path).expect("fixture");
    let registry =
        synth_registry::load_dir(&root().join("registry").join("parts")).expect("registry");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let mut board = synth_ir::lower(&parse.ast.expect("ast"), &registry, &file)
        .board
        .expect("board");
    board.placement_spacing_nm = spacing_mm.map(synth_geometry::mm_to_nm);
    board
}

#[test]
fn label_overlaps_at_2_5_mm_are_reported_next_to_unset() {
    let unset = rtd_afe(None);
    let unset_count =
        synth_kicad::silk_label_overlaps(&unset, &synth_place::place(&unset).unwrap());
    let spaced = rtd_afe(Some(2.5));
    let spaced_count =
        synth_kicad::silk_label_overlaps(&spaced, &synth_place::place(&spaced).unwrap());
    eprintln!("LABELS overlaps: unset {unset_count}, 2.5 mm {spaced_count}");
    assert_eq!(spaced_count, 0, "unset has {unset_count}");
}
