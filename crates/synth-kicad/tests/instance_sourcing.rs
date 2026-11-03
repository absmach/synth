// SPDX-License-Identifier: Apache-2.0

//! Per-instance `lcsc_pn`/`mpn`: precedence over the registry part, flow
//! into the BOM exporters, and no change for boards without the attribute.

use std::path::Path;

fn lower(src: &str) -> synth_ir::Board {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry = synth_registry::load_dir(&root.join("registry/parts")).expect("registry");
    let parsed = synth_parser::parse(src, "t.synth");
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.unwrap(), &registry, "t.synth");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    lowered.board.unwrap()
}

fn bom(board: &synth_ir::Board) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let r = synth_kicad::export(board, dir.path()).expect("export");
    (
        std::fs::read_to_string(r.bom_path).unwrap(),
        std::fs::read_to_string(r.schematic_path).unwrap(),
    )
}

fn row<'a>(csv: &'a str, refdes: &str) -> &'a str {
    csv.lines()
        .find(|l| l.starts_with(&format!("{refdes},")))
        .unwrap()
}

const TWO_RESISTORS: &str = r#"board "b" {
    component R1: resistor "r_generic_0603" value "1k" lcsc_pn "C110776" mpn "RT0603BRD071KL"
    component R2: resistor "r_generic_0603" value "27" lcsc_pn "C123" mpn "RT0603FRE0727RL"
}"#;

#[test]
fn instance_value_reaches_bom_csv_and_schematic_once() {
    let board = lower(TWO_RESISTORS);
    let (bom, _) = bom(&board);
    assert!(
        row(&bom, "R1").ends_with(",C110776,RT0603BRD071KL"),
        "{bom}"
    );
    assert!(row(&bom, "R2").ends_with(",C123,RT0603FRE0727RL"), "{bom}");
}

#[test]
fn instance_value_overrides_registry_and_absent_falls_back() {
    let board = lower(
        r#"board "b" {
            component J1: connector "jst_ph_4pin" lcsc_pn "C999"
            component J2: connector "jst_ph_4pin"
        }"#,
    );
    let (bom, _) = bom(&board);
    let registry_lcsc = board.components[1]
        .part
        .as_ref()
        .unwrap()
        .lcsc_pn
        .clone()
        .unwrap();
    assert_ne!(registry_lcsc, "C999");
    assert!(row(&bom, "J1").contains(",C999,"), "{bom}");
    assert!(
        row(&bom, "J2").contains(&format!(",{registry_lcsc},")),
        "{bom}"
    );
    // J1 overrode only the LCSC number; mpn stays the registry's.
    assert_eq!(
        board.components[0].part.as_ref().unwrap().mpn,
        board.components[1].part.as_ref().unwrap().mpn
    );
}

#[test]
fn jlcpcb_bom_keeps_different_lcsc_numbers_on_separate_lines() {
    let board = lower(TWO_RESISTORS);
    let lock = synth_ir::lock_board(&board, None).expect("lock");
    let csv = synth_kicad::build_jlcpcb_bom_csv(&board, &lock).expect("jlc bom");
    assert_eq!(csv.lines().count(), 3, "{csv}");
    assert!(csv.contains("C110776") && csv.contains("C123"), "{csv}");
}
