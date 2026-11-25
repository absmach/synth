// SPDX-License-Identifier: Apache-2.0

//! `E-SYNTH-POWER-013`: worst-case output of a regulator feedback divider.
//!
//! The parts are defined here, so the tests do not depend on registry data.
//!
//! Arithmetic for every case. The regulator has Vref 0.796 V to 0.804 V
//! (0.8 V +/-0.5 %). The divider is Rtop 10k, Rbottom 20k, so the nominal
//! output is 0.8 * (1 + 10/20) = 1.2 V.
//!
//! - 1 % resistors: output 1.186118 V to 1.214121 V.
//! - 0.1 % resistors: output 1.193205 V to 1.206805 V.
//!
//! A load that allows 1.164 V to 1.236 V (1.2 V +/-3 %) accepts both. A load
//! that allows 1.188 V to 1.212 V (1.2 V +/-1 %) rejects 1 % parts on both
//! sides (1.186118 < 1.188 and 1.214121 > 1.212) and accepts 0.1 % parts.

use synth_diagnostics::Diagnostic;

const REGULATOR: &str = r#"
id = "t_adj_reg"
kind = "regulator"

[operating_conditions]
feedback_vref_min_v = 0.796
feedback_vref_max_v = 0.804

[[pins]]
name = "gnd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "fb"
number = "2"
electrical_type = "input"
capabilities = ["feedback"]
"#;

const REGULATOR_NO_VREF: &str = r#"
id = "t_adj_reg_novref"
kind = "regulator"

[[pins]]
name = "gnd"
number = "1"
electrical_type = "power_input"

[[pins]]
name = "fb"
number = "2"
electrical_type = "input"
capabilities = ["feedback"]
"#;

fn load(window: (f64, f64)) -> String {
    format!(
        r#"
id = "t_core_load"
kind = "ic"

[[pins]]
name = "vdd"
number = "1"
electrical_type = "power_input"
voltage_min_v = {}
voltage_max_v = {}

[[pins]]
name = "gnd"
number = "2"
electrical_type = "ground_reference"
"#,
        window.0, window.1
    )
}

const RESISTOR: &str = r#"
id = "t_res"
kind = "resistor"

[[pins]]
name = "p1"
number = "1"
electrical_type = "passive"

[[pins]]
name = "p2"
number = "2"
electrical_type = "passive"
"#;

fn run(
    regulator: &str,
    regulator_id: &str,
    window: (f64, f64),
    tol: Option<&str>,
) -> Vec<Diagnostic> {
    let load = load(window);
    let registry = synth_registry::load_from_strs(&[
        (&format!("{regulator_id}.synth.toml"), regulator),
        ("t_core_load.synth.toml", load.as_str()),
        ("t_res.synth.toml", RESISTOR),
    ])
    .expect("test parts must load");
    let tol = tol.map_or_default(|t| format!(r#" tolerance "{t}""#));
    let src = format!(
        r#"board "t" {{
  layers 2
  component U1: regulator "{regulator_id}"
  component U2: ic "t_core_load"
  component R1: resistor "t_res" value "10k"{tol}
  component R2: resistor "t_res" value "20k"{tol}
  connect R1.p1 -> U2.vdd
  connect R1.p2 -> U1.fb
  connect U1.fb -> R2.p1
  connect R2.p2 -> U1.gnd
  connect U1.gnd -> U2.gnd
}}"#
    );
    let filename = "divider_tolerance.synth".to_string();
    let parse = synth_parser::parse(&src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let board = lowered.board.as_ref().expect("board must lower");
    synth_validate::run_erc(board, &filename)
        .into_iter()
        .filter(|d| d.code == "E-SYNTH-POWER-013")
        .collect()
}

#[test]
fn a_window_that_holds_the_worst_case_gives_no_finding() {
    let d = run(REGULATOR, "t_adj_reg", (1.164, 1.236), Some("1%"));
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn one_percent_resistors_leave_a_one_percent_window() {
    let d = run(REGULATOR, "t_adj_reg", (1.188, 1.212), Some("1%"));
    assert_eq!(d.len(), 1, "{d:?}");
    let found = d[0].found.as_deref().expect("found text");
    assert!(found.contains("1.1861 V to 1.2141 V"), "{found}");
    assert!(
        found.contains("0.1 % resistors give 1.1932 V to 1.2068 V"),
        "{found}"
    );
}

#[test]
fn tenth_percent_resistors_clear_the_same_window() {
    let d = run(REGULATOR, "t_adj_reg", (1.188, 1.212), Some("0.1%"));
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn no_tolerance_data_means_no_finding() {
    let d = run(REGULATOR, "t_adj_reg", (1.188, 1.212), None);
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn no_reference_range_means_no_finding() {
    let d = run(
        REGULATOR_NO_VREF,
        "t_adj_reg_novref",
        (1.188, 1.212),
        Some("1%"),
    );
    assert!(d.is_empty(), "{d:?}");
}
