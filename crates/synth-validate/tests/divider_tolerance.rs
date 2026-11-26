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
    run_with(regulator, regulator_id, window, tol, "")
}

fn run_with(
    regulator: &str,
    regulator_id: &str,
    window: (f64, f64),
    tol: Option<&str>,
    value_suffix: &str,
) -> Vec<Diagnostic> {
    run_values(
        regulator,
        regulator_id,
        window,
        tol,
        (&format!("10k{value_suffix}"), &format!("20k{value_suffix}")),
    )
}

/// Like [`run_with`], with the two leg values written out in full.
fn run_values(
    regulator: &str,
    regulator_id: &str,
    window: (f64, f64),
    tol: Option<&str>,
    (r1, r2): (&str, &str),
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
  component R1: resistor "t_res" value "{r1}"{tol}
  component R2: resistor "t_res" value "{r2}"{tol}
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
        .filter(|d| {
            matches!(
                d.code.as_str(),
                "E-SYNTH-POWER-013" | "E-SYNTH-POWER-014" | "E-SYNTH-POWER-015"
            )
        })
        .collect()
}

/// Like [`run`], with the tolerance written into the value string.
fn run_value(
    regulator: &str,
    regulator_id: &str,
    window: (f64, f64),
    tol: &str,
) -> Vec<Diagnostic> {
    run_with(regulator, regulator_id, window, None, &format!(" {tol}"))
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
    assert_eq!(d[0].code, "E-SYNTH-POWER-013");
    let found = d[0].found.as_deref().expect("found text");
    assert!(found.contains("1.1861 V to 1.2141 V"), "{found}");
    // 0.5 % parts: 0.796 * (1 + 9.95k / 20.1k) = 1.19004 V and
    // 0.804 * (1 + 10.05k / 19.9k) = 1.21004 V, the loosest class that fits.
    assert!(
        found.contains("0.5 % resistors give 1.1900 V to 1.2100 V"),
        "{found}"
    );
}

#[test]
fn tenth_percent_resistors_clear_the_same_window() {
    // 1.193205 V is 5.2 mV above 1.188 V, inside the default 0.5 % (6 mV)
    // margin, so the error becomes a thin-margin warning.
    let d = run(REGULATOR, "t_adj_reg", (1.188, 1.212), Some("0.1%"));
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E-SYNTH-POWER-014", "{d:?}");
    assert!(!d[0].severity.is_blocking());
}

#[test]
fn a_wide_margin_gives_no_warning() {
    let d = run(REGULATOR, "t_adj_reg", (1.164, 1.236), Some("0.1%"));
    assert!(d.is_empty(), "{d:?}");
}

/// The acceptance case: a buck with 1 % parts feeding a core rail. The window
/// is 1.2 V +/-1 % here because a +/-0.5 % reference already uses half of a
/// tight window; the 1 % parts fail it and 0.1 % parts pass.
#[test]
fn switching_to_tenth_percent_clears_the_error() {
    let one = run(REGULATOR, "t_adj_reg", (1.188, 1.212), Some("1%"));
    assert!(one.iter().any(|d| d.code == "E-SYNTH-POWER-013"), "{one:?}");
    let tenth = run(REGULATOR, "t_adj_reg", (1.188, 1.212), Some("0.1%"));
    assert!(
        tenth.iter().all(|d| d.code != "E-SYNTH-POWER-013"),
        "{tenth:?}"
    );
}

#[test]
fn missing_tolerance_is_incomplete_evidence() {
    let d = run(REGULATOR, "t_adj_reg", (1.188, 1.212), None);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E-SYNTH-POWER-015");
    let found = d[0].found.as_deref().expect("found text");
    assert!(found.contains("a tolerance on R1"), "{found}");
    assert!(found.contains("a tolerance on R2"), "{found}");
}

/// A leg whose value does not read as a resistance leaves the divider
/// unanalysed; that is reported as missing evidence, not skipped.
#[test]
fn an_unreadable_leg_value_is_incomplete_evidence() {
    let d = run_values(
        REGULATOR,
        "t_adj_reg",
        (1.188, 1.212),
        Some("1%"),
        ("10k", "DNP"),
    );
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E-SYNTH-POWER-015");
    let found = d[0].found.as_deref().expect("found text");
    assert!(
        found.contains("a readable resistance on R2 (value `DNP` does not parse)"),
        "{found}"
    );
}

#[test]
fn missing_reference_range_is_incomplete_evidence() {
    let d = run(
        REGULATOR_NO_VREF,
        "t_adj_reg_novref",
        (1.188, 1.212),
        Some("1%"),
    );
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E-SYNTH-POWER-015");
    assert!(
        d[0].found
            .as_deref()
            .is_some_and(|f| f.contains("reference range")),
        "{d:?}"
    );
}

#[test]
fn a_tolerance_in_the_value_string_counts() {
    let d = run_value(REGULATOR, "t_adj_reg", (1.188, 1.212), "1%");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E-SYNTH-POWER-013");
}
