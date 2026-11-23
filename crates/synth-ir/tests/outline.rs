// SPDX-License-Identifier: Apache-2.0

//! `outline 60mm x 30mm` lowering: a fixed board rectangle, or none.

fn lower(src: &str) -> (Option<synth_ir::Board>, Vec<String>) {
    let parse = synth_parser::parse(src, "outline.synth");
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics);
    let ast = parse.ast.expect("clean parse yields an ast");
    let lowered = synth_ir::lower(&ast, &synth_registry::Registry::default(), "outline.synth");
    (
        lowered.board,
        lowered.diagnostics.iter().map(|d| d.code.clone()).collect(),
    )
}

#[test]
fn an_outline_lowers_to_a_fixed_rectangle() {
    let (board, codes) = lower("board \"b\" {\n layers 4\n outline 60mm x 30mm\n}");
    assert!(codes.is_empty(), "{codes:?}");
    let outline = board.unwrap().outline.expect("outline declared");
    assert_eq!(outline.width, synth_ir::Length(60_000_000));
    assert_eq!(outline.height, synth_ir::Length(30_000_000));
}

#[test]
fn a_design_without_an_outline_has_none() {
    let (board, codes) = lower("board \"b\" {\n layers 2\n}");
    assert!(codes.is_empty(), "{codes:?}");
    assert!(board.unwrap().outline.is_none());
}

#[test]
fn an_outline_with_a_wrong_unit_is_rejected() {
    let (board, codes) = lower("board \"b\" {\n outline 60ohm x 30mm\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-UNIT-001"),
        "{codes:?}"
    );
    assert!(board.unwrap().outline.is_none());
}

#[test]
fn a_second_outline_is_an_error_and_the_first_stands() {
    let (board, codes) =
        lower("board \"b\" {\n layers 2\n outline 60mm x 30mm\n outline 80mm x 40mm\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-BOARD-003"),
        "{codes:?}"
    );
    let outline = board.unwrap().outline.expect("the first outline stands");
    assert_eq!(outline.width, synth_ir::Length(60_000_000));
    assert_eq!(outline.height, synth_ir::Length(30_000_000));
}

#[test]
fn a_failing_second_outline_does_not_undo_the_first() {
    let (board, codes) =
        lower("board \"b\" {\n layers 2\n outline 60mm x 30mm\n outline 80ohm x 40mm\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-UNIT-001"),
        "{codes:?}"
    );
    let outline = board.unwrap().outline.expect("the first outline stands");
    assert_eq!(outline.width, synth_ir::Length(60_000_000));
    assert_eq!(outline.height, synth_ir::Length(30_000_000));
}
