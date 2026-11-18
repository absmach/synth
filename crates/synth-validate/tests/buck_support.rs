// SPDX-License-Identifier: Apache-2.0

//! The MP2307's support network is machine-checked: the bootstrap capacitor
//! from BS to SW and the output capacitor behind the inductor are required
//! by the part, not just described in its datasheet. Each test builds a buck
//! converter from the shipped registry and runs the real ERC.

use std::path::{Path, PathBuf};

use synth_diagnostics::Diagnostic;
use synth_validate::ErcConfig;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn validate(src: &str) -> Vec<Diagnostic> {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let filename = "buck.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc_with_config(
            board,
            &filename,
            &ErcConfig::default(),
        ));
    }
    all
}

fn codes<'a>(diags: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

/// A 12 V to 3.3 V buck with every support part, minus what `without` removes
/// and with the output capacitor and the bootstrap return as given.
fn buck(bootstrap: &str, output_cap: &str, output_value: &str) -> String {
    format!(
        r#"board "t" {{
  layers 2
  component U1: regulator "mp2307_buck_ep"
  component L1: inductor "l_generic_1210" value "10uH"
  component C1: capacitor "c_generic_0805" value "10uF"
  component C2: capacitor "c_generic_0805" value "10uF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component R1: resistor "r_generic_0603" value "31.6k"
  component R2: resistor "r_generic_0603" value "10k"
  component C5: capacitor "c_generic_0603" value "10nF"
  component COUT: capacitor "c_generic_0805" value "{output_value}"
  connect U1.in -> C1.p1
  connect U1.in -> C2.p1
  connect U1.in -> C3.p1
  connect U1.en -> C1.p1
  connect C1.p2 -> U1.gnd
  connect C2.p2 -> U1.gnd
  connect C3.p2 -> U1.gnd
  connect U1.ep -> U1.gnd
  connect U1.sw -> L1.p1
  connect U1.bs -> C5.p1
  connect {bootstrap}
  connect L1.p2 -> R1.p1
  connect R1.p2 -> U1.fb
  connect U1.fb -> R2.p1
  connect R2.p2 -> U1.gnd
  {output_cap}
}}"#
    )
}

const GOOD_BOOTSTRAP: &str = "C5.p2 -> U1.sw";
const GOOD_OUTPUT: &str = "connect L1.p2 -> COUT.p1\n  connect COUT.p2 -> U1.gnd";

#[test]
fn a_complete_buck_network_raises_no_power_001() {
    let diags = validate(&buck(GOOD_BOOTSTRAP, GOOD_OUTPUT, "22uF"));
    assert!(
        codes(&diags, "E-SYNTH-POWER-001").is_empty(),
        "{:?}",
        codes(&diags, "E-SYNTH-POWER-001")
    );
    assert!(codes(&diags, "E-SYNTH-POWER-006").is_empty());
}

#[test]
fn a_bootstrap_capacitor_that_goes_to_ground_is_not_a_bootstrap_capacitor() {
    let diags = validate(&buck("C5.p2 -> U1.gnd", GOOD_OUTPUT, "22uF"));
    let found = codes(&diags, "E-SYNTH-POWER-001");
    assert!(
        found
            .iter()
            .any(|d| d.expected.as_deref().is_some_and(|e| e.contains("between"))),
        "a capacitor from BS to ground is not from BS to SW: {found:?}"
    );
}

#[test]
fn a_boot_pin_wired_to_the_switch_node_through_no_capacitor_is_reported() {
    // BS goes to SW through a resistor, which is not a bootstrap capacitor.
    let src = buck(GOOD_BOOTSTRAP, GOOD_OUTPUT, "22uF")
        .replace(
            "  component C5: capacitor \"c_generic_0603\" value \"10nF\"\n",
            "  component R3: resistor \"r_generic_0603\" value \"0\"\n",
        )
        .replace("  connect U1.bs -> C5.p1\n", "  connect U1.bs -> R3.p1\n")
        .replace("  connect C5.p2 -> U1.sw\n", "  connect R3.p2 -> U1.sw\n");
    let found = validate(&src);
    let found = codes(&found, "E-SYNTH-POWER-001");
    assert!(
        found
            .iter()
            .any(|d| d.expected.as_deref().is_some_and(|e| e.contains("between"))),
        "{found:?}"
    );
}

#[test]
fn a_boot_pin_left_unconnected_is_reported_by_the_required_pin_rule() {
    // With no capacitor and nothing on BS the pin floats: the pin is required,
    // so E-SYNTH-CONNECT-001 reports it and POWER-001 does not say it twice.
    let src = buck(GOOD_BOOTSTRAP, GOOD_OUTPUT, "22uF")
        .replace(
            "  component C5: capacitor \"c_generic_0603\" value \"10nF\"\n",
            "",
        )
        .replace("  connect U1.bs -> C5.p1\n", "")
        .replace("  connect C5.p2 -> U1.sw\n", "");
    let found = validate(&src);
    assert!(
        !codes(&found, "E-SYNTH-CONNECT-001").is_empty(),
        "a floating required BS pin is reported: {found:?}"
    );
}

#[test]
fn an_omitted_output_capacitor_is_reported() {
    let src = buck(GOOD_BOOTSTRAP, "", "22uF").replace(
        "  component COUT: capacitor \"c_generic_0805\" value \"22uF\"\n",
        "",
    );
    let found = validate(&src);
    let found = codes(&found, "E-SYNTH-POWER-001");
    assert!(
        found.iter().any(|d| d
            .expected
            .as_deref()
            .is_some_and(|e| e.contains("behind the `inductor`"))),
        "{found:?}"
    );
}

#[test]
fn an_output_capacitor_below_the_datasheet_value_is_weighed() {
    let found = validate(&buck(GOOD_BOOTSTRAP, GOOD_OUTPUT, "1uF"));
    assert!(
        !codes(&found, "E-SYNTH-POWER-006").is_empty(),
        "1 uF on the output is below the 22 uF the part asks for"
    );
}
