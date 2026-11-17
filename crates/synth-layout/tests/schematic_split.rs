// SPDX-License-Identifier: Apache-2.0

//! A group of series resistor + grounded zener clamps must stay compact.
//!
//! Twelve clamps (the 12 protected GPIO lines of the sensor-node
//! design) used to place as one tall column: a zener on the ground net
//! has no signal-net pull, so nothing drew it toward its resistor. The
//! region grew past A0, `sheet_overflow` fired, and the exporter split
//! the whole board into one sheet per group even though the parts fit
//! comfortably on a single page.

use std::path::{Path, PathBuf};

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_from_source(name: &str, source: &str) -> Board {
    let parse = synth_parser::parse(source, name.to_string());
    let ast = parse.ast.expect("source must parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, name);
    lowered.board.expect("source must lower")
}

/// One group: a 20-pin header with a 330 Ω series resistor and a 3.3 V
/// zener clamp to ground on each of its first 12 lines.
fn protected_header() -> String {
    use std::fmt::Write as _;
    let mut source = String::from(
        r#"board "protected_header" {
  layers 2
  group "PROTECTED GPIO HEADER" {
    component J4: connector "header_1x20"
"#,
    );
    for i in 0..12 {
        let _ = writeln!(
            source,
            "    component RG{i}: resistor \"r_generic_0603\" value \"330\""
        );
        let _ = writeln!(source, "    component DZ{i}: diode \"d_zener_3v3\"");
    }
    source.push_str("  }\n");
    source.push_str("  connect J4.p13 -> U1.gnd\n");
    for i in 0..12 {
        let _ = writeln!(source, "  connect J4.p{} -> RG{i}.p1", i + 1);
        let _ = writeln!(source, "  connect RG{i}.p2 -> DZ{i}.cathode");
        let _ = writeln!(source, "  connect DZ{i}.anode -> U1.gnd");
    }
    source.push_str("}\n");
    source
}

#[test]
fn grouped_clamps_stay_compact_and_on_one_sheet() {
    // `U1.gnd` makes the clamp net a power net, as it is in the real
    // design; a regulator part only supplies that pin.
    let source = protected_header().replace(
        "  layers 2\n",
        "  layers 2\n  component U1: regulator \"ams1117_3v3\"\n",
    );
    let board = board_from_source("protected_header.synth", &source);
    let layout = synth_layout::layout(&board);

    let header = layout
        .group_boxes
        .iter()
        .find(|b| b.group == "PROTECTED GPIO HEADER")
        .expect("the group has a box");
    let height = header.max_mm.1 - header.min_mm.1;
    assert!(
        height < 200.0,
        "the clamp region is {height:.1} mm tall; the parts should pack into a grid"
    );

    assert!(
        !synth_layout::sheets::sheet_overflow(&board, &layout),
        "a 26-part design must fit one sheet"
    );
    assert_eq!(
        synth_layout::sheets::layout_sheets(&board, layout, None).len(),
        1,
        "the design fits one sheet, so it must not split"
    );
}

/// A 20-pin header with 20 series resistors in one group, plus a small
/// regulator group: a tall-ish region that still fits the largest sheet.
fn tall_grouped() -> String {
    use std::fmt::Write as _;
    let mut source = String::from(
        r#"board "tall_grouped" {
  layers 2
  group "HEADER" {
    component J1: connector "header_1x20"
"#,
    );
    for i in 0..20 {
        let _ = writeln!(
            source,
            "    component R{i}: resistor \"r_generic_0603\" value \"100\""
        );
    }
    source.push_str(
        "  }\n  group \"POWER\" {\n    component U1: regulator \"ams1117_3v3\"\n    component C1: capacitor \"c_generic_0805\" value \"10uF\"\n    component C2: capacitor \"c_generic_0805\" value \"10uF\"\n  }\n",
    );
    for i in 0..20 {
        let _ = writeln!(source, "  connect J1.p{} -> R{i}.p1", i + 1);
    }
    source.push_str(
        "  connect U1.vin -> C1.p1\n  connect U1.gnd -> C1.p2\n  connect U1.vout -> C2.p1\n  connect U1.gnd -> C2.p2\n}\n",
    );
    source
}

/// A grouped design whose regions fit together stays on one sheet even
/// when one region is tall: a tall region is not an overflow.
#[test]
fn a_tall_grouped_region_that_fits_stays_on_one_sheet() {
    let board = board_from_source("tall_grouped.synth", &tall_grouped());
    let layout = synth_layout::layout(&board);
    let header = layout
        .group_boxes
        .iter()
        .find(|b| b.group == "HEADER")
        .expect("the header group has a box");
    let height = header.max_mm.1 - header.min_mm.1;
    assert!(
        height > 200.0,
        "the fixture must produce a tall-ish region, got {height:.0} mm"
    );
    assert!(
        !synth_layout::sheets::sheet_overflow(&board, &layout),
        "the content fits one sheet"
    );
    assert_eq!(
        synth_layout::sheets::layout_sheets(&board, layout, None).len(),
        1,
        "a tall region that fits must not force a split"
    );
}
