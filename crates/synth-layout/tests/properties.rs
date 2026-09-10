// SPDX-License-Identifier: Apache-2.0

//! Property tests for `synth_layout::layout`.
//!
//! Invariants the layout MUST satisfy, regardless of which slice's
//! algorithm is active:
//!
//! - **Total.** Every component in the input `Board` has exactly one
//!   `ComponentPlacement` in the output.
//! - **Deterministic.** Identical input produces identical output,
//!   byte-for-byte across runs.
//! - **Finite coordinates.** No NaN/infinity centres slip through.
//! - **Distinct positions.** No two components land at the same `(x, y)`
//!   (the grid step is > 0, so this is mechanically true; the test
//!   guards against future regressions like off-by-one column math).
//!
//! Slice 1A's grid is too simple to need a property-test generator;
//! fixed seed inputs already exercise every branch. Once Sugiyama
//! lands in slice 1B, this file will gain proptest generators that
//! synthesise arbitrary boards.

use std::path::{Path, PathBuf};

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// Lower a `.synth` fixture to a `Board`. Skips ERC; layout doesn't
/// depend on ERC results.
fn board_for(fixture_rel_path: &str) -> Board {
    let path = workspace_root().join(fixture_rel_path);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse.ast.expect("fixture must parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, &filename);
    lowered.board.expect("fixture must lower")
}

/// Lower an inline `.synth` source to a `Board`, for cases that are
/// about the *shape* of a design (how many parts, how tall their
/// symbols are) rather than about any checked-in example.
fn board_from_source(name: &str, source: &str) -> Board {
    let parse = synth_parser::parse(source, name.to_string());
    let ast = parse.ast.expect("source must parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, name);
    lowered.board.expect("source must lower")
}

#[test]
fn every_component_is_placed_exactly_once() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    assert_eq!(layout.components.len(), board.components.len());
    let mut ids: Vec<u32> = layout.components.iter().map(|p| p.id.0).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        board.components.len(),
        "duplicate ids in placements"
    );
}

#[test]
fn layout_is_deterministic_across_runs() {
    let board = board_for("examples/sensor_logger.synth");
    let a = synth_layout::layout(&board);
    let b = synth_layout::layout(&board);
    assert_eq!(a, b, "layout is not deterministic");
}

#[test]
fn all_coordinates_are_finite() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    for p in &layout.components {
        assert!(p.center_mm.0.is_finite(), "{p:?} x is not finite");
        assert!(p.center_mm.1.is_finite(), "{p:?} y is not finite");
    }
}

#[test]
fn no_two_components_share_a_position() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    let mut positions: Vec<(i64, i64)> = layout
        .components
        .iter()
        // Convert to micrometre integers so we can hash without
        // worrying about f64 equality.
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
    assert_eq!(positions.len(), before, "two components share a position");
}

#[test]
fn sheet_size_escalates_with_content_size() {
    // The sheet starts at A4 and escalates to the smallest ISO size
    // (A3, then A2) whose landscape dims contain the content bounding
    // box — the canonical `sensor_logger` demo overflows A4 (x≈565 mm
    // against 297 mm), so it must report a larger page rather than
    // silently clipping. See `sheet_size_for` in `lib.rs`; this is
    // also the invariant `E-SYNTH-SCHEM-007` (page overflow) relies on.
    let small = board_for("fixtures/ir/two_components_with_net.synth");
    let small_layout = synth_layout::layout(&small);
    let big = board_for("examples/sensor_logger.synth");
    let big_layout = synth_layout::layout(&big);
    assert_eq!(small_layout.sheet_size, synth_layout::SheetSize::A4);
    // Escalation only ever grows the page: never smaller than A4, and
    // the oversized demo genuinely outgrows it.
    let (w, h) = big_layout.sheet_size.dims_mm();
    assert!(w >= 297.0 && h >= 210.0, "sheet must never shrink below A4");
    assert_ne!(
        big_layout.sheet_size,
        synth_layout::SheetSize::A4,
        "sensor_logger content overflows A4, so the sheet must escalate"
    );
}

#[test]
fn wide_passive_heavy_board_stays_on_its_sheet() {
    // Regression: a board that is mostly small passives hung off one
    // tall symbol used to be laid out as a two-row strip that ran
    // clean off the page — a 23-part USB-C power module reached
    // x ≈ 1031 mm on a 594 mm-wide A2, with the bottom two thirds of
    // the sheet empty.
    //
    // Cause: the USB-C receptacle's tall symbol forces a large
    // `grid_h`, only two rows of which fit above A4's title block, so
    // every additional cluster wrapped into another column and the
    // content grew sideways without bound. Sheets stop escalating at
    // A2, so that is real overflow (`E-SYNTH-SCHEM-007`), not just an
    // unbalanced aspect ratio. The placer now retries with taller
    // columns until the content fits the page it declares.
    let source = r#"
board "wide_passive_heavy" {
  layers 2
  component J1: connector "usb_c_receptacle"
  component J2: connector "header_1x4"
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0603"
  component R2: resistor "r_generic_0603"
  component R3: resistor "r_generic_0603"
  component R4: resistor "r_generic_0603"
  component C1: capacitor "c_generic_0805"
  component C2: capacitor "c_generic_0805"
  component C3: capacitor "c_generic_0805"
  component C4: capacitor "c_generic_0805"
  component C5: capacitor "c_generic_0805"
  component C6: capacitor "c_generic_0603"
  component C7: capacitor "c_generic_0603"
  component C8: capacitor "c_generic_0603"
  component D1: diode "d_tvs_5v"
  component D2: diode "led_green_0603"
  component D3: diode "led_amber_0603"
  component F1: fuse "polyfuse"

  connect J1.cc1 -> R1.p1
  connect J1.gnd -> R1.p2
  connect J1.cc2 -> R2.p1
  connect J1.gnd -> R2.p2
  connect J1.vbus -> C1.p1
  connect J1.gnd -> C1.p2
  connect J1.vbus -> C6.p1
  connect J1.gnd -> C6.p2
  connect J1.vbus -> D1.cathode
  connect J1.gnd -> D1.anode
  connect J1.vbus -> F1.p1
  connect F1.p2 -> C2.p1
  connect J1.gnd -> C2.p2
  connect F1.p2 -> C7.p1
  connect J1.gnd -> C7.p2
  connect F1.p2 -> R3.p1
  connect R3.p2 -> D2.anode
  connect D2.cathode -> J1.gnd
  connect F1.p2 -> U1.vin
  connect J1.gnd -> U1.gnd
  connect U1.vin -> C3.p1
  connect U1.gnd -> C3.p2
  connect U1.vout -> C4.p1
  connect U1.gnd -> C4.p2
  connect U1.vout -> C5.p1
  connect U1.gnd -> C5.p2
  connect U1.vout -> C8.p1
  connect U1.gnd -> C8.p2
  connect U1.vout -> R4.p1
  connect R4.p2 -> D3.anode
  connect D3.cathode -> U1.gnd
  connect F1.p2 -> J2.p1
  connect J1.gnd -> J2.p2
  connect U1.vout -> J2.p3
  connect U1.gnd -> J2.p4
}
"#;
    let board = board_from_source("wide_passive_heavy.synth", source);
    let layout = synth_layout::layout(&board);
    let (sheet_w, sheet_h) = layout.sheet_size.dims_mm();
    for p in &layout.components {
        let (x, y) = p.center_mm;
        assert!(
            x >= 0.0 && x <= sheet_w && y >= 0.0 && y <= sheet_h,
            "{p:?} lies outside the declared {sheet_w:.0} × {sheet_h:.0} mm sheet"
        );
    }
}

#[test]
fn declared_groups_are_captioned_and_contiguous() {
    // A `group` is an annotation, but an annotation that lies is worse
    // than none: a caption sits above its group's bounding box, so a
    // group scattered across the sheet would title a region full of
    // other groups' parts. Placement bands by declared group ahead of
    // power-flow layer to keep each one contiguous, and every declared
    // group gets exactly one caption.
    let source = r#"
board "grouped" {
  layers 2
  group "Input" {
    component J1: connector "usb_c_receptacle"
    component R1: resistor "r_generic_0603"
    connect J1.cc1 -> R1.p1
    connect J1.gnd -> R1.p2
  }
  group "Regulation" {
    component U1: regulator "ams1117_3v3"
    component C1: capacitor "c_generic_0805"
    component C2: capacitor "c_generic_0805"
    connect J1.vbus -> U1.vin
    connect J1.gnd -> U1.gnd
    connect U1.vout -> C1.p1
    connect U1.gnd -> C1.p2
    connect U1.vout -> C2.p1
    connect U1.gnd -> C2.p2
  }
  group "Output" {
    component J2: connector "header_1x4"
    connect U1.vout -> J2.p1
    connect U1.gnd -> J2.p2
  }
}
"#;
    let board = board_from_source("grouped.synth", source);

    // Every component knows the group it was declared inside, and a
    // group never becomes a scope: `connect` reaches across freely.
    let group_of = |refdes: &str| -> Option<String> {
        board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .and_then(|c| c.group.clone())
    };
    assert_eq!(group_of("J1").as_deref(), Some("Input"));
    assert_eq!(group_of("U1").as_deref(), Some("Regulation"));
    assert_eq!(group_of("J2").as_deref(), Some("Output"));

    let layout = synth_layout::layout(&board);

    let captions: Vec<&str> = layout.annotations.iter().map(|a| a.text.as_str()).collect();
    assert_eq!(captions, vec!["Input", "Regulation", "Output"]);

    // Each group occupies its own horizontal band, in declaration order.
    let mut spans: Vec<(String, f64, f64)> = Vec::new();
    for placement in &layout.components {
        let Some(group) = board.component(placement.id).and_then(|c| c.group.clone()) else {
            continue;
        };
        let x = placement.center_mm.0;
        match spans.iter_mut().find(|(g, _, _)| *g == group) {
            Some(entry) => {
                entry.1 = entry.1.min(x);
                entry.2 = entry.2.max(x);
            }
            None => spans.push((group, x, x)),
        }
    }
    spans.sort_by(|a, b| a.1.total_cmp(&b.1));
    for pair in spans.windows(2) {
        let (ref left, _, left_max) = pair[0];
        let (ref right, right_min, _) = pair[1];
        assert!(
            left_max < right_min,
            "groups {left:?} and {right:?} overlap horizontally \
             ({left_max} >= {right_min}); a caption would title the wrong parts"
        );
    }

    // Captions stay on the page they are drawn on.
    let (sheet_w, sheet_h) = layout.sheet_size.dims_mm();
    for annotation in &layout.annotations {
        let (x, y) = annotation.at_mm;
        // Character counts here are far below f64's 52-bit mantissa.
        #[allow(clippy::cast_precision_loss)]
        let width = annotation.text.chars().count() as f64 * annotation.size_mm * 0.72;
        assert!(
            x >= 0.0 && x + width <= sheet_w && y >= 0.0 && y <= sheet_h,
            "caption {:?} runs off the {sheet_w:.0} x {sheet_h:.0} mm sheet",
            annotation.text
        );
    }
}
