// SPDX-License-Identifier: Apache-2.0

//! `E-SYNTH-USB-002` on a USB-C receptacle's CC pins. A floating CC pin is an
//! error: with no Rd a compliant source never turns VBUS on. A CC pin on a net
//! without a resistor stays a warning, since a CC controller may be on it.

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Severity};

fn registry_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("parts")
}

fn usb_002(src: &str) -> Vec<Diagnostic> {
    let filename = "usb_cc.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&registry_dir()).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let board = lowered.board.as_ref().expect("board must lower");
    synth_validate::run_erc(board, &filename)
        .into_iter()
        .filter(|d| d.code == "E-SYNTH-USB-002")
        .collect()
}

#[test]
fn a_receptacle_with_both_cc_pins_floating_is_an_error() {
    let found = usb_002(
        r#"board "b" {
  layers 2
  component J1: connector "usb_c_receptacle"
}"#,
    );
    assert_eq!(found.len(), 1, "one finding per receptacle: {found:?}");
    assert_eq!(found[0].severity, Severity::Error);
    let text = found[0].found.as_deref().unwrap_or_default();
    assert!(
        text.contains("J1.cc1") && text.contains("J1.cc2"),
        "both pins named: {text}"
    );
}

#[test]
fn one_floating_cc_pin_is_reported_by_name() {
    let found = usb_002(
        r#"board "b" {
  layers 2
  component J1: connector "usb_c_receptacle"
  component R1: resistor "r_generic_0603" value "5.1k"
  connect J1.cc1 -> R1.p1
  connect R1.p2 -> J1.gnd
}"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Error);
    let text = found[0].found.as_deref().unwrap_or_default();
    assert!(
        text.contains("J1.cc2") && !text.contains("J1.cc1"),
        "{text}"
    );
}

#[test]
fn a_sink_with_rd_on_both_cc_pins_is_clean() {
    let found = usb_002(
        r#"board "b" {
  layers 2
  component J1: connector "usb_c_receptacle"
  component R1: resistor "r_generic_0603" value "5.1k"
  component R2: resistor "r_generic_0603" value "5.1k"
  connect J1.cc1 -> R1.p1
  connect J1.cc2 -> R2.p1
  connect R1.p2 -> J1.gnd
  connect R2.p2 -> J1.gnd
}"#,
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_cc_pin_wired_without_a_resistor_stays_a_warning() {
    let found = usb_002(
        r#"board "b" {
  layers 2
  component J1: connector "usb_c_receptacle"
  component R2: resistor "r_generic_0603" value "5.1k"
  component U1: mcu "rp2350"
  connect J1.cc1 -> U1.gp0
  connect J1.cc2 -> R2.p1
  connect R2.p2 -> J1.gnd
}"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, Severity::Warning);
}
