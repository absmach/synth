// SPDX-License-Identifier: Apache-2.0

//! `escape { … }` lowering: what a component declared lands on the board as a
//! typed intent.
//!
//! Asserted on the IR rather than on the absence of diagnostics. A grammar can
//! parse an attribute and lower it to nothing — an arm written for an
//! identifier the lexer never emits as one is unreachable, and the attribute
//! then either errors or is silently dropped. Only reading the value back
//! catches that.

use synth_ir::EscapeMethod;

/// The shipped registry, so the components resolve and the only diagnostics
/// left are the ones this file is about.
fn registry() -> synth_registry::Registry {
    let parts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("parts");
    synth_registry::load_dir(&parts).expect("registry must load")
}

fn lower(src: &str) -> (Option<synth_ir::Board>, Vec<String>) {
    let parse = synth_parser::parse(src, "escape.synth");
    let ast = parse.ast.expect("source must parse");
    let lowered = synth_ir::lower(&ast, &registry(), "escape.synth");
    let mut codes: Vec<String> = parse.diagnostics.iter().map(|d| d.code.clone()).collect();
    codes.extend(lowered.diagnostics.iter().map(|d| d.code.clone()));
    (lowered.board, codes)
}

fn component<'a>(board: &'a synth_ir::Board, refdes: &str) -> &'a synth_ir::Component {
    board
        .components
        .iter()
        .find(|c| c.refdes == refdes)
        .unwrap_or_else(|| panic!("no component {refdes}"))
}

const DECLARED: &str = r#"board "b" {
    layers 4
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method underpad  layers "In1.Cu", "In2.Cu" }
    }
    component U2: resistor "r_generic_0603" value "10k" {
        escape { off }
    }
    component U3: resistor "r_generic_0603" value "10k" {
        escape { method channel }
    }
    component U4: resistor "r_generic_0603" value "10k" {
    }
}"#;

#[test]
fn a_declared_method_and_layers_reach_the_board() {
    let (board, codes) = lower(DECLARED);
    let board = board.expect("a board");
    assert!(codes.is_empty(), "no diagnostics expected: {codes:?}");

    let intent = component(&board, "U1")
        .escape
        .as_ref()
        .expect("U1 declared an escape");
    assert_eq!(intent.method, Some(EscapeMethod::Underpad));
    assert_eq!(
        intent.layers,
        vec!["In1.Cu".to_string(), "In2.Cu".to_string()]
    );
    assert!(!intent.off);
}

#[test]
fn off_reaches_the_board_on_its_own() {
    let (board, _) = lower(DECLARED);
    let board = board.expect("a board");
    let intent = component(&board, "U2")
        .escape
        .as_ref()
        .expect("U2 declared an escape");
    assert!(intent.off);
    assert_eq!(intent.method, None);
    assert!(intent.layers.is_empty());
}

#[test]
fn a_method_alone_leaves_the_layers_to_the_engine() {
    let (board, _) = lower(DECLARED);
    let board = board.expect("a board");
    let intent = component(&board, "U3")
        .escape
        .as_ref()
        .expect("U3 declared an escape");
    assert_eq!(intent.method, Some(EscapeMethod::Channel));
    assert!(
        intent.layers.is_empty(),
        "an undeclared layer list is not an empty board: {:?}",
        intent.layers
    );
}

/// A component that says nothing carries no intent, so geometric selection
/// stays the default rather than being overridden by a blank declaration.
#[test]
fn a_component_that_declares_nothing_carries_no_intent() {
    let (board, _) = lower(DECLARED);
    let board = board.expect("a board");
    assert!(component(&board, "U4").escape.is_none());
}

/// `off` and a method are not contradictory in the grammar, so a board can
/// write both. `off` wins, and both halves survive lowering so the router can
/// see what was asked for.
#[test]
fn off_and_a_method_both_survive_lowering() {
    let (board, codes) = lower(
        r#"board "b" {
    layers 4
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method underpad  off }
    }
}"#,
    );
    let board = board.expect("a board");
    let intent = component(&board, "U1").escape.as_ref().expect("an escape");
    assert!(intent.off);
    assert_eq!(intent.method, Some(EscapeMethod::Underpad));
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn an_unknown_method_is_reported_rather_than_dropped() {
    let (_, codes) = lower(
        r#"board "b" {
    layers 4
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method teleport }
    }
}"#,
    );
    assert!(
        codes.iter().any(|c| c == "E-SYNTH-PROCESS-004"),
        "expected E-SYNTH-PROCESS-004: {codes:?}"
    );
}

/// A layer the board does not have is a typo, and typos have to fail here.
///
/// Passed through, the name reaches the escape engine, the fanout fails at
/// route time, and the package is routed unescaped behind a stderr warning —
/// the same silent fallback the unknown-method check exists to prevent.
#[test]
fn a_layer_the_board_does_not_have_is_reported() {
    let (_, codes) = lower(
        r#"board "b" {
    layers 2
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method underpad  layers "In1.Cu" }
    }
}"#,
    );
    assert!(
        codes.iter().any(|c| c == "E-SYNTH-PROCESS-005"),
        "expected E-SYNTH-PROCESS-005: {codes:?}"
    );
}

/// The same name on a board that has the layer is fine, so the check is about
/// the board rather than about the spelling.
#[test]
fn an_inner_layer_is_accepted_once_the_board_has_one() {
    let (board, codes) = lower(
        r#"board "b" {
    layers 4
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method underpad  layers "In1.Cu", "In2.Cu" }
    }
}"#,
    );
    assert!(
        !codes.iter().any(|c| c == "E-SYNTH-PROCESS-005"),
        "a four-layer board has In1.Cu and In2.Cu: {codes:?}"
    );
    let board = board.expect("a board");
    assert_eq!(
        component(&board, "U1")
            .escape
            .as_ref()
            .unwrap()
            .layers
            .len(),
        2
    );
}

/// The layer count may be declared after the component that names a layer,
/// which is why the check runs over the finished board rather than where the
/// block appears.
#[test]
fn the_layer_count_may_come_after_the_component() {
    let (_, codes) = lower(
        r#"board "b" {
    component U1: resistor "r_generic_0603" value "10k" {
        escape { method underpad  layers "In2.Cu" }
    }
    layers 4
}"#,
    );
    assert!(
        !codes.iter().any(|c| c == "E-SYNTH-PROCESS-005"),
        "declaration order must not decide this: {codes:?}"
    );
}
