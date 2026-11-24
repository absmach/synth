// SPDX-License-Identifier: Apache-2.0

//! `outline <w> x <h> reason "<text>"` — an optional stated reason.

use synth_ast::StatementAst;
use synth_parser::parse;

fn outline_reason(source: &str) -> (Option<String>, Vec<String>) {
    let parsed = parse(source, "outline.synth".to_string());
    let reason = parsed.ast.as_ref().and_then(|ast| {
        ast.board
            .statements
            .iter()
            .find_map(|statement| match statement {
                StatementAst::Outline(outline) => Some(outline.reason.clone()),
                _ => None,
            })
    });
    (
        reason.flatten(),
        parsed.diagnostics.iter().map(|d| d.code.clone()).collect(),
    )
}

#[test]
fn an_outline_reason_reaches_the_ast() {
    let (reason, codes) = outline_reason(
        "board \"b\" {\n layers 2\n outline 60mm x 30mm reason \"fits the enclosure\"\n}",
    );
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(reason.as_deref(), Some("fits the enclosure"));
}

#[test]
fn an_outline_without_a_reason_has_none() {
    let (reason, codes) = outline_reason("board \"b\" {\n layers 2\n outline 60mm x 30mm\n}");
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(reason, None);
}

#[test]
fn an_empty_outline_reason_is_a_parse_error() {
    let (_, codes) =
        outline_reason("board \"b\" {\n layers 2\n outline 60mm x 30mm reason \"\"\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-PARSE-047"),
        "{codes:?}"
    );
}

#[test]
fn a_reason_without_a_string_is_a_parse_error() {
    let (_, codes) = outline_reason("board \"b\" {\n layers 2\n outline 60mm x 30mm reason\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-PARSE-047"),
        "{codes:?}"
    );
}

#[test]
fn a_whitespace_only_reason_is_a_parse_error() {
    let (_, codes) =
        outline_reason("board \"b\" {\n layers 2\n outline 60mm x 30mm reason \"   \"\n}");
    assert!(
        codes.iter().any(|code| code == "E-SYNTH-PARSE-047"),
        "{codes:?}"
    );
}

#[test]
fn a_reason_is_followed_by_the_next_statement() {
    let (reason, codes) = outline_reason(
        "board \"b\" {\n layers 2\n outline 60mm x 30mm reason \"fits the enclosure\"\n \
         component R1: resistor \"r_generic_0603\"\n}",
    );
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(reason.as_deref(), Some("fits the enclosure"));
}
