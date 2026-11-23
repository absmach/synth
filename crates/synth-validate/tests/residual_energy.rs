// SPDX-License-Identifier: Apache-2.0

//! One connector's paralleled VBUS contacts are one device, not six loads
//! (#276).

use std::path::Path;

fn codes(src: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
    let parse = synth_parser::parse(src, "residual.synth");
    let ast = parse.ast.as_ref().expect("source parses");
    let lowered = synth_ir::lower(ast, &registry, "residual.synth");
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, "residual.synth"));
    }
    all.into_iter().map(|d| d.code).collect()
}

#[test]
fn four_vbus_contacts_on_one_connector_are_not_a_load_multiplier() {
    let src = r#"board "vbus" {
  layers 2
  component J1: connector "usb_c_receptacle"
  component U1: mcu "rp2350"
  component C1: capacitor "c_generic_0603" value "100nf"
  power "GND" 0v { J1.gnd C1.p2 }
  connect J1.vbus -> J1.vbus_b4
  connect J1.vbus -> J1.vbus_a9
  connect J1.vbus -> J1.vbus_b9
  connect J1.vbus -> U1.iovdd
  connect J1.vbus -> C1.p1
}"#;
    assert!(
        !codes(src).iter().any(|c| c == "E-SYNTH-POWER-004"),
        "one connector must not look like six loads"
    );
}

#[test]
fn three_real_loads_with_no_decoupling_still_anomalous() {
    let src = r#"board "loads" {
  layers 2
  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  component U3: mcu "rp2350"
  component J1: connector "header_1x4"
  connect J1.p1 -> U1.iovdd
  connect J1.p1 -> U2.iovdd
  connect J1.p1 -> U3.iovdd
}"#;
    assert!(codes(src).iter().any(|c| c == "E-SYNTH-POWER-004"));
}
