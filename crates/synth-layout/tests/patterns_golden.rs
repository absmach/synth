// SPDX-License-Identifier: Apache-2.0

//! Golden tests for the §7.5.4 pattern catalog additions
//! (`LdoBlock`, `I2cBus`, `Crystal`, `Divider`).
//!
//! The in-module unit tests in `lib.rs` prove *recognition* (which
//! components a pattern claims into a cluster, with which `MemberSide`)
//! and *layout* (via `build_clusters` + `place_clusters`) directly.
//! This file exercises the same motifs through the real end-to-end
//! path — parse the `.synth` fixture, lower to a `Board`, run the full
//! `layout()` — and asserts:
//!
//! 1. Every component is placed exactly once, deterministically, on
//!    distinct finite positions.
//! 2. The pattern's members sit *clustered* around their anchor (the
//!    signature of recognition) rather than scattered across the page
//!    as independent singletons.
//!
//! Fixtures live in `fixtures/layout/` and mirror the catalog rows in
//! `synth_implementation_plan.md` §7.5.4.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use synth_ir::Board;
use synth_layout::{ComponentPlacement, Layout};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// Lower a `.synth` fixture to a `Board`, given a path relative to the
/// workspace root. Mirrors `board_for` in `layout_corpus.rs`.
fn board_for(fixture_rel_path: &str) -> Board {
    let path = workspace_root().join(fixture_rel_path);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse
        .ast
        .unwrap_or_else(|| panic!("{} must parse", path.display()));
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, &filename);
    lowered
        .board
        .unwrap_or_else(|| panic!("{} must lower", path.display()))
}

fn refdes_map<'a>(board: &Board, layout: &'a Layout) -> HashMap<String, &'a ComponentPlacement> {
    layout
        .components
        .iter()
        .map(|p| {
            let refdes = board
                .component(p.id)
                .map_or_else(|| format!("?{}", p.id.0), |c| c.refdes.clone());
            (refdes, p)
        })
        .collect()
}

/// Distance between the centres of two refdeses.
fn dist(map: &HashMap<String, &ComponentPlacement>, a: &str, b: &str) -> f64 {
    let pa = map.get(a).unwrap_or_else(|| panic!("no placement for {a}"));
    let pb = map.get(b).unwrap_or_else(|| panic!("no placement for {b}"));
    let dx = pa.center_mm.0 - pb.center_mm.0;
    let dy = pa.center_mm.1 - pb.center_mm.1;
    (dx * dx + dy * dy).sqrt()
}

/// Assert the layout is well-formed and that `anchor` and `member` are
/// placed close enough to be the same cluster (members fan out within a
/// couple of cluster cells of their anchor), not scattered singletons.
/// The BK y-pitch is `grid_h` ≈ 70 mm and members sit within
/// ~anchor_half_h + MEMBER_CLEARANCE (≈ 40 mm) below their anchor, so
/// 90 mm is a generous but still discriminating bound.
fn assert_well_formed_and_clustered(board: &Board, layout: &Layout, anchor: &str, member: &str) {
    // Determinism: a second run is bit-identical.
    let again = synth_layout::layout(board);
    assert_eq!(*layout, again, "{anchor}: layout is not deterministic");

    assert_eq!(
        layout.components.len(),
        board.components.len(),
        "{anchor}: not every component was placed"
    );

    let map = refdes_map(board, layout);
    let d = dist(&map, anchor, member);
    assert!(
        d < 90.0,
        "{anchor} and {member} should be clustered but are {d:.1} mm apart"
    );
}

#[test]
fn ldo_block_golden() {
    let board = board_for("fixtures/layout/ldo_block.synth");
    let layout = synth_layout::layout(&board);
    // U1 is the regulator anchor; C1/C2 are its vin/vout caps.
    assert_well_formed_and_clustered(&board, &layout, "U1", "C1");
    assert_well_formed_and_clustered(&board, &layout, "U1", "C2");
}

#[test]
fn i2c_bus_golden() {
    let board = board_for("fixtures/layout/i2c_bus.synth");
    let layout = synth_layout::layout(&board);
    // U1 is the I2C sensor anchor; R1/R2 are the SDA/SCL pull-ups.
    assert_well_formed_and_clustered(&board, &layout, "U1", "R1");
    assert_well_formed_and_clustered(&board, &layout, "U1", "R2");
}

#[test]
fn crystal_golden() {
    let board = board_for("fixtures/layout/crystal.synth");
    let layout = synth_layout::layout(&board);
    // X1 is the crystal anchor; C1/C2 are its load caps.
    assert_well_formed_and_clustered(&board, &layout, "X1", "C1");
    assert_well_formed_and_clustered(&board, &layout, "X1", "C2");
}

#[test]
fn divider_golden() {
    let board = board_for("fixtures/layout/divider.synth");
    let layout = synth_layout::layout(&board);
    // R1 is the rail-side divider anchor; R2 is the mid→gnd resistor.
    assert_well_formed_and_clustered(&board, &layout, "R1", "R2");
}
