// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for `E-SYNTH-DIFF-004` and `E-SYNTH-DIFF-005`. Each case
//! starts from a small USB board modelled on `examples/placement_and_diff_pair/placement_and_diff_pair.synth`,
//! which is clean for these rules, and changes only the `diff_pair` declarations.
//! The plain pass/fail cases also live in `fixtures/erc/`; the tests here cover the
//! named-net form and the edge cases fixtures cannot express.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Severity};

fn registry_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("parts")
}

fn diags(src: &str) -> Vec<Diagnostic> {
    let filename = "si_rules.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&registry_dir()).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let board = lowered.board.as_ref().expect("board must lower");
    synth_validate::run_erc(board, &filename)
}

fn codes(src: &str) -> Vec<String> {
    diags(src).into_iter().map(|d| d.code).collect()
}

/// The `found` text of the first diagnostic with `code`.
fn found(src: &str, code: &str) -> String {
    diags(src)
        .into_iter()
        .find(|d| d.code == code)
        .and_then(|d| d.found)
        .unwrap_or_else(|| panic!("{code} not emitted"))
}

fn has(codes: &[String], code: &str) -> bool {
    codes.iter().any(|c| c == code)
}

/// Legacy form: pair legs are `<REFDES>_<pin>` endpoint names.
fn legacy_board(pairs: &str) -> String {
    format!(
        "\
board \"t\" {{
  layers 4
  component U1: mcu \"rp2350\"
  component J1: connector \"usb_c_receptacle\"
  connect J1.gnd -> U1.gnd
  connect J1.dp -> U1.usb_dp
  connect J1.dn -> U1.usb_dn
  {pairs}
}}
"
    )
}

/// Named-net form: legs are net names resolved at lowering.
fn named_board(pairs: &str) -> String {
    format!(
        "\
board \"t\" {{
  layers 4
  component U1: mcu \"rp2350\"
  component J1: connector \"usb_c_receptacle\"
  connect J1.gnd -> U1.gnd as \"GND\"
  connect J1.dp -> U1.usb_dp as \"USB_DP\"
  connect J1.dn -> U1.usb_dn as \"USB_DN\"
  {pairs}
}}
"
    )
}

const PAIR: &str = "diff_pair U1_usb_dp U1_usb_dn { impedance 90ohm }";

#[test]
fn clean_pair_triggers_neither_rule() {
    for src in [
        legacy_board(PAIR),
        named_board("diff_pair USB_DP USB_DN { impedance 90ohm }"),
    ] {
        let c = codes(&src);
        for code in ["E-SYNTH-DIFF-004", "E-SYNTH-DIFF-005"] {
            assert!(!has(&c, code), "{code} fired on a clean pair: {c:?}");
        }
    }
}

#[test]
fn pair_declared_twice_is_reported_once() {
    // J1_dp / J1_dn are the same nets as U1_usb_dp / U1_usb_dn.
    let c = codes(&legacy_board(&format!(
        "{PAIR}\n  diff_pair J1_dp J1_dn {{ impedance 90ohm }}"
    )));
    let n = c.iter().filter(|c| *c == "E-SYNTH-DIFF-004").count();
    assert_eq!(n, 1, "one diagnostic per offending pair, got {n}: {c:?}");
}

#[test]
fn diff_004_names_the_pair_that_owns_the_net() {
    let src = legacy_board(&format!(
        "{PAIR}\n  diff_pair J1_dp J1_dn {{ impedance 90ohm }}"
    ));
    let f = found(&src, "E-SYNTH-DIFF-004");
    assert!(f.contains("U1_usb_dp") && f.contains("U1_usb_dn"), "{f}");
}

#[test]
fn one_pair_naming_a_net_twice_is_allowed() {
    // The RF-003 idiom: a pair on a single net carries an impedance target.
    let c = codes(&legacy_board(
        "diff_pair U1_usb_dp J1_dp { impedance 90ohm }",
    ));
    assert!(!has(&c, "E-SYNTH-DIFF-004"), "{c:?}");
}

#[test]
fn named_net_reused_across_pairs_emits_diff_004() {
    let c = codes(&named_board(
        "diff_pair USB_DP USB_DN { impedance 90ohm }\n  diff_pair USB_DP J1_dn { impedance 90ohm }",
    ));
    assert!(has(&c, "E-SYNTH-DIFF-004"), "{c:?}");
}

#[test]
fn same_name_legs_are_left_to_diff_002() {
    let c = codes(&legacy_board(
        "diff_pair U1_usb_dp U1_usb_dp { impedance 90ohm }",
    ));
    assert!(has(&c, "E-SYNTH-DIFF-002"), "{c:?}");
    assert!(!has(&c, "E-SYNTH-DIFF-004"), "{c:?}");
}

#[test]
fn pair_on_ground_net_emits_diff_005() {
    let src = legacy_board("diff_pair U1_usb_dp J1_gnd { impedance 90ohm }");
    let c = codes(&src);
    assert!(has(&c, "E-SYNTH-DIFF-005"), "{c:?}");
    // Names the offending leg and the power pin that put it on a rail.
    let f = found(&src, "E-SYNTH-DIFF-005");
    assert!(f.contains("J1_gnd") && f.contains(".gnd"), "{f}");
}

#[test]
fn named_ground_net_emits_diff_005() {
    let src = named_board("diff_pair USB_DP GND { impedance 90ohm }");
    let c = codes(&src);
    assert!(has(&c, "E-SYNTH-DIFF-005"), "{c:?}");
    assert!(found(&src, "E-SYNTH-DIFF-005").contains("`GND`"));
}

#[test]
fn both_rules_are_errors() {
    let d = diags(&legacy_board(&format!(
        "{PAIR}\n  diff_pair J1_dp J1_gnd {{ impedance 90ohm }}"
    )));
    for code in ["E-SYNTH-DIFF-004", "E-SYNTH-DIFF-005"] {
        let hit = d.iter().find(|d| d.code == code);
        assert_eq!(hit.map(|d| d.severity), Some(Severity::Error), "{code}");
    }
}
