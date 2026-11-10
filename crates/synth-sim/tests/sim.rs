// SPDX-License-Identifier: Apache-2.0

//! Behaviour of the `simulate` seam: a compiled board plus its testbench
//! sidecar produces a deck, a run and a pass/fail report.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const DIVIDER: &str = r#"
board "divider" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"#;

const DIVIDER_TB: &str = r#"
ground = "GND"

[[source]]
net = "VCC"
kind = "dc"
dc = 12.0

[analysis]
op = true

[[measure]]
name = "vmid"
kind = "node_voltage"
node = "MID"
min = 5.7
max = 6.3
"#;

const COLLISION: &str = r#"
board "collision" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  component R4: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "mid" { R3.p1, R4.p2 }
  connect R3.p2 -> R4.p1 as "OTHER"
}
"#;

const UNCONNECTED: &str = r#"
board "unconnected" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "MID" { R3.p1 }
}
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn board(source: &str) -> synth_ir::Board {
    let file = "divider.synth".to_string();
    let parsed = synth_parser::parse(source, file.clone());
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("ast");
    let resolved = synth_ir::resolve_imports(
        &ast,
        &synth_ir::FsImportLoader {
            root: PathBuf::from("."),
        },
        &file,
    );
    assert!(
        resolved
            .diagnostics
            .iter()
            .all(|d| !d.severity.is_blocking()),
        "imports: {:?}",
        resolved.diagnostics
    );
    let lowered = synth_ir::lower(
        &resolved.program,
        synth_registry::embedded_registry(),
        &file,
    );
    assert!(
        lowered
            .diagnostics
            .iter()
            .all(|d| !d.severity.is_blocking()),
        "lower: {:?}",
        lowered.diagnostics
    );
    lowered.board.expect("board")
}

#[test]
fn emits_golden_divider_deck() {
    let board = board(DIVIDER);
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");
    let expected = std::fs::read_to_string(fixture("divider-op.cir")).expect("golden deck");

    assert_eq!(deck, expected);
}

#[cfg(unix)]
#[test]
fn divider_measures_pass() {
    let board = board(DIVIDER);
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");
    let options = synth_sim::SimOptions {
        ngspice: Some(stub_ngspice("pass", "divider-op.out")),
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_004 && !d.severity.is_blocking()),
        "a passing limit must be an info diagnostic: {:?}",
        report.diagnostics
    );
    assert_eq!(report.measures.len(), 1);
    assert_eq!(report.measures[0].name, "vmid");
    assert!((report.measures[0].value - 6.0).abs() < 1e-9);
    assert!(report.measures[0].passed());
}

#[test]
fn malformed_testbench_gives_007() {
    let missing = synth_sim::Testbench::from_toml("ground = \"GND\"\n\n[analysis]\nop = true\n")
        .expect_err("no sources or measures");
    assert_eq!(missing.code, synth_sim::E_SYNTH_SIM_007);

    let typo = synth_sim::Testbench::from_toml("gronud = \"GND\"\n").expect_err("typo");
    assert_eq!(typo.code, synth_sim::E_SYNTH_SIM_007);
}

#[test]
fn sanitized_name_collision_gives_007() {
    let board = board(COLLISION);
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("name collision");

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_007 && d.severity.is_blocking()),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn an_unconnected_pin_gives_008() {
    let board = board(UNCONNECTED);
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("floating pin");

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_008 && d.severity.is_blocking()),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_megaohm_resistor_emits_an_exponent() {
    let board = board(&DIVIDER.replace("value \"10k\"", "value \"1M\""));
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(
        deck.contains("RR1 n_VCC n_MID 1e6"),
        "deck did not emit 1e6:\n{deck}"
    );
}

#[cfg(unix)]
#[test]
fn out_of_limit_value_gives_004() {
    let board = board(DIVIDER);
    let testbench = synth_sim::Testbench::from_toml(
        &DIVIDER_TB
            .replace("min = 5.7", "min = 4.0")
            .replace("max = 6.3", "max = 5.5"),
    )
    .expect("testbench parses");
    let options = synth_sim::SimOptions {
        ngspice: Some(stub_ngspice("limit", "divider-op.out")),
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    assert!(!report.passed());
    assert!((report.measures[0].value - 6.0).abs() < 1e-9);
    assert!(!report.measures[0].passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_004 && d.severity.is_blocking()),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[cfg(unix)]
#[test]
fn missing_measurement_value_gives_003() {
    let board = board(DIVIDER);
    let testbench = synth_sim::Testbench::from_toml(DIVIDER_TB).expect("testbench parses");
    let options = synth_sim::SimOptions {
        ngspice: Some(stub_ngspice("missing", "divider-missing-value.out")),
    };

    let report = synth_sim::simulate(&board, &testbench, &options);

    assert!(!report.passed());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003 && d.severity.is_blocking()),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A stub stands in for ngspice, replaying a real captured log.
#[cfg(unix)]
fn stub_ngspice(label: &str, fixture_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth_sim_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("stub dir");
    let path = dir.join("ngspice");
    std::fs::write(
        &path,
        format!("#!/bin/sh\ncat '{}'\n", fixture(fixture_name).display()),
    )
    .expect("write stub");
    let mut permissions = std::fs::metadata(&path).expect("stat stub").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod stub");
    path
}
