// SPDX-License-Identifier: Apache-2.0

//! Layout corpus test: every `.synth` fixture under `fixtures/layout/`
//! must lower cleanly and produce a well-formed `Layout`.
//!
//! `fixtures/layout/` is the reference set called for in
//! `synth_implementation_plan.md` 7.5.8 — one small design per
//! recognized schematic sub-circuit motif (decoupling, LDO block,
//! I2C bus, USB differential pair, crystal, MCU, divider, LED
//! indicator) plus a multi-pattern combo. This test doesn't re-run
//! the pattern-recognition assertions from `properties.rs` against a
//! single fixture; it runs the *same* well-formedness checks against
//! *every* fixture in the corpus, so a regression that breaks layout
//! on any cataloged motif fails here instead of only on
//! `sensor_logger`.
//!
//! Layout has no dependency on ERC passing (see `properties.rs`), so
//! fixtures only need to parse and lower — they don't need to be
//! ERC-clean.

use std::path::{Path, PathBuf};

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// Lower a `.synth` fixture to a `Board`, given a path relative to
/// the workspace root. Mirrors `board_for` in `snapshots.rs` /
/// `properties.rs`.
fn board_for(fixture_rel_path: &Path) -> Board {
    let source = std::fs::read_to_string(fixture_rel_path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", fixture_rel_path.display()));
    let filename = fixture_rel_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse
        .ast
        .unwrap_or_else(|| panic!("{} must parse", fixture_rel_path.display()));
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, &filename);
    lowered
        .board
        .unwrap_or_else(|| panic!("{} must lower", fixture_rel_path.display()))
}

/// Every `.synth` file directly under `fixtures/layout/`, sorted for
/// deterministic test iteration order.
fn corpus_fixtures() -> Vec<PathBuf> {
    let dir = workspace_root().join("fixtures").join("layout");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "fixtures/layout/ must contain at least one .synth fixture"
    );
    paths
}

/// Assert the same well-formedness invariants `properties.rs` checks
/// for `sensor_logger`, applied to an arbitrary board/layout pair.
fn assert_layout_well_formed(fixture: &Path, board: &Board) {
    let a = synth_layout::layout(board);
    let b = synth_layout::layout(board);

    // Total: every component placed exactly once.
    assert_eq!(
        a.components.len(),
        board.components.len(),
        "{}: not every component was placed",
        fixture.display()
    );
    let mut ids: Vec<u32> = a.components.iter().map(|p| p.id.0).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        board.components.len(),
        "{}: duplicate ids in placements",
        fixture.display()
    );

    // Deterministic: running twice gives identical output.
    assert_eq!(a, b, "{}: layout is not deterministic", fixture.display());

    // Finite coordinates.
    for p in &a.components {
        assert!(
            p.center_mm.0.is_finite(),
            "{}: {p:?} x is not finite",
            fixture.display()
        );
        assert!(
            p.center_mm.1.is_finite(),
            "{}: {p:?} y is not finite",
            fixture.display()
        );
    }

    // Distinct positions: no two components at the same (x, y).
    let mut positions: Vec<(i64, i64)> = a
        .components
        .iter()
        .map(|p| {
            (
                (p.center_mm.0 * 1000.0) as i64,
                (p.center_mm.1 * 1000.0) as i64,
            )
        })
        .collect();
    positions.sort_unstable();
    let before = positions.len();
    positions.dedup();
    assert_eq!(
        positions.len(),
        before,
        "{}: two components share a position",
        fixture.display()
    );

    // On-sheet: nothing placed past the edge of the page the layout
    // declares. Sheets escalate A4 → A3 → A2 from the content bounds,
    // so a layout that reports a page it does not fit inside is the
    // `E-SYNTH-SCHEM-007` overflow the placer exists to avoid — it
    // renders as symbols hanging off the printable area in KiCad.
    let (sheet_w, sheet_h) = a.sheet_size.dims_mm();
    for p in &a.components {
        let (x, y) = p.center_mm;
        assert!(
            x >= 0.0 && x <= sheet_w && y >= 0.0 && y <= sheet_h,
            "{}: {p:?} lies outside the declared {sheet_w:.0} × {sheet_h:.0} mm sheet",
            fixture.display()
        );
    }
    for wire in &a.wires {
        for &(x, y) in &wire.points {
            assert!(
                x >= 0.0 && x <= sheet_w && y >= 0.0 && y <= sheet_h,
                "{}: wire point ({x:.1}, {y:.1}) lies outside the declared \
                 {sheet_w:.0} × {sheet_h:.0} mm sheet",
                fixture.display()
            );
        }
    }
}

#[test]
fn layout_corpus_is_well_formed() {
    for fixture in corpus_fixtures() {
        let board = board_for(&fixture);
        assert_layout_well_formed(&fixture, &board);
    }
}

/// `fixtures/layout/<stem>.synth` paired with the `ClusterKind` the
/// recognition passes actually produce for it.
///
/// Mostly the fixture's namesake motif. `i2c_bus` is deliberately not:
/// its own header documents that a BMP280 declaring `required_decoupling`
/// qualifies as an IC-block anchor, so the IC-block pass claims the part
/// and both pull-ups — that is the mechanism the implementation plan
/// names for the I2C-bus motif, so IcBlock is the expected answer.
const MOTIF_KINDS: &[(&str, synth_layout::ClusterKind)] = &[
    ("ldo_block", synth_layout::ClusterKind::LdoBlock),
    ("led_indicator", synth_layout::ClusterKind::LedIndicator),
    ("i2c_bus", synth_layout::ClusterKind::IcBlock),
    ("crystal", synth_layout::ClusterKind::Crystal),
    ("divider", synth_layout::ClusterKind::Divider),
];

#[test]
fn recognized_clusters_keep_their_motif_kind() {
    // Recognition knows which motif matched, but until `Cluster` grew
    // a `kind` that answer was thrown away as soon as the passes were
    // merged — leaving every sub-circuit nameable only by its anchor
    // refdes. Each single-motif fixture must now report its own kind,
    // and name itself the way an engineer would write it on a sheet.
    for (stem, expected) in MOTIF_KINDS {
        let fixture = workspace_root()
            .join("fixtures")
            .join("layout")
            .join(format!("{stem}.synth"));
        let board = board_for(&fixture);
        let clusters = synth_layout::build_clusters(&board);
        let named = clusters
            .iter()
            .find(|c| c.kind == *expected)
            .unwrap_or_else(|| {
                panic!(
                    "{stem}.synth: no cluster recognized as {expected:?}; got {:?}",
                    clusters.iter().map(|c| c.kind).collect::<Vec<_>>()
                )
            });
        let motif = expected.display_name().expect("motif kinds are named");
        let name = named.display_name(&board);
        assert!(
            name.ends_with(motif),
            "{stem}.synth: cluster named {name:?}, expected it to end with {motif:?}"
        );
    }
}

#[test]
fn unclaimed_components_are_named_by_refdes_alone() {
    // A part no motif claims is its own singleton cluster. There is no
    // motif to name it after, so it names itself by refdes rather than
    // inventing one.
    let fixture = workspace_root()
        .join("fixtures")
        .join("layout")
        .join("decoupling.synth");
    let board = board_for(&fixture);
    for cluster in synth_layout::build_clusters(&board) {
        if cluster.kind == synth_layout::ClusterKind::Singleton {
            let refdes = &board.component(cluster.anchor).unwrap().refdes;
            assert_eq!(&cluster.display_name(&board), refdes);
        }
    }
}
