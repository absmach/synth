// SPDX-License-Identifier: Apache-2.0

//! Behaviour of the `simulate` seam: a compiled board plus its testbench
//! sidecar produces a deck, a run and a pass/fail report.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

/// Two capacitors in series give `MID` two connections but no DC path.
const SERIES_CAPACITOR: &str = r#"
board "series-capacitor" {
  layers 2
  component C1: capacitor "c_generic_0805" value "1u"
  component C2: capacitor "c_generic_0805" value "1u"
  net "VCC" { C1.p1 }
  net "GND" { C2.p2 }
  connect C1.p2 -> C2.p1 as "MID"
}
"#;

/// `FLOAT` has one connection; its only part still reaches ground, so this
/// isolates the two-connection rule from the DC-path rule.
const FLOATING: &str = r#"
board "floating" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  component R2: resistor "r_generic_0805" value "10k"
  component R3: resistor "r_generic_0805" value "10k"
  net "VCC" { R1.p1 }
  net "GND" { R2.p2, R3.p2 }
  connect R1.p2 -> R2.p1 as "MID"
  net "FLOAT" { R3.p1 }
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

fn testbench(source: &str) -> synth_sim::Testbench {
    synth_sim::Testbench::from_toml(source).expect("testbench parses")
}

/// A scratch directory removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("synth_sim_{label}_{}_{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("stub dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Link the stub binary into a scratch directory under `label`, which the
/// stub reads from its own file name. The directory is unique per call so
/// parallel tests never share a stub or its hang marker.
fn stub_ngspice(label: &str) -> (PathBuf, Scratch) {
    let scratch = Scratch::new(label);
    let built = PathBuf::from(env!("CARGO_BIN_EXE_synth_sim_stub"));
    let mut path = scratch.path().join(label);
    if let Some(extension) = built.extension() {
        path.set_extension(extension);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&built, &path).expect("link stub");
    #[cfg(not(unix))]
    std::fs::copy(&built, &path).expect("copy stub");
    (path, scratch)
}

fn options(stub: &Path) -> synth_sim::SimOptions {
    synth_sim::SimOptions {
        ngspice: Some(stub.to_path_buf()),
    }
}

fn assert_explained(diagnostics: &[synth_diagnostics::Diagnostic]) {
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic.explanation_url.as_deref(),
            Some(format!("synth.docs/diagnostics/{}", diagnostic.code).as_str()),
            "diagnostic {} has no documentation link",
            diagnostic.code
        );
    }
}

#[test]
fn emits_golden_divider_deck() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");
    let expected = std::fs::read_to_string(fixture("divider-op.cir")).expect("golden deck");

    assert_eq!(deck, expected);
}

#[test]
fn divider_measures_pass() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("ok");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);
    assert_explained(&report.diagnostics);
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
fn invalid_timeout_gives_007() {
    for timeout in ["0", "-1", "1e20"] {
        let source = DIVIDER_TB.replace(
            "[analysis]",
            &format!("[options]\ntimeout = {timeout}\n\n[analysis]"),
        );
        let error = synth_sim::Testbench::from_toml(&source).expect_err("bad timeout");
        assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007, "timeout {timeout}");
        assert_explained(std::slice::from_ref(&error));
    }
}

#[test]
fn a_sidecar_error_carries_its_path_and_docs_link() {
    let scratch = Scratch::new("sidecar-path");
    let missing = scratch.path().join("missing.sim.toml");
    let error = synth_sim::Testbench::from_path(&missing).expect_err("missing sidecar");
    assert_eq!(error.code, synth_sim::E_SYNTH_SIM_007);
    assert_explained(std::slice::from_ref(&error));
    assert_eq!(
        error
            .location
            .as_ref()
            .map(|location| location.file.as_str()),
        Some(missing.to_str().expect("path is UTF-8"))
    );
}

#[test]
fn sanitized_name_collision_gives_007() {
    let board = board(COLLISION);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("name collision");

    assert_explained(&diagnostics);
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
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("floating pin");

    assert_explained(&diagnostics);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_008 && d.severity.is_blocking()),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_series_capacitor_gives_008_naming_the_net() {
    let board = board(SERIES_CAPACITOR);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("no DC path");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_008
                && d.message.as_deref().is_some_and(|m| m.contains("MID"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_single_connection_net_gives_008_naming_the_net() {
    let board = board(FLOATING);
    let testbench = testbench(DIVIDER_TB);

    let diagnostics = synth_sim::emit_netlist(&board, &testbench).expect_err("floating net");

    assert_explained(&diagnostics);
    assert!(
        diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_008
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("FLOAT") && m.contains("1 connection"))
        }),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_megaohm_resistor_emits_an_exponent() {
    let board = board(&DIVIDER.replace("value \"10k\"", "value \"1M\""));
    let testbench = testbench(DIVIDER_TB);

    let deck = synth_sim::emit_netlist(&board, &testbench).expect("deck emits");

    assert!(
        deck.contains("RR1 n_VCC n_MID 1e6"),
        "deck did not emit 1e6:\n{deck}"
    );
}

#[test]
fn out_of_limit_value_gives_004() {
    let board = board(DIVIDER);
    let testbench = testbench(
        &DIVIDER_TB
            .replace("min = 5.7", "min = 4.0")
            .replace("max = 6.3", "max = 5.5"),
    );
    let (stub, _dir) = stub_ngspice("ok");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
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

/// ngspice prints six significant digits, so a limit within the last printed
/// digit must not flip a result that is equal at that precision.
#[test]
fn limit_comparison_uses_the_six_digit_print_precision() {
    let board = board(DIVIDER);
    let (stub, _dir) = stub_ngspice("ok");

    let rounded = testbench(&DIVIDER_TB.replace("min = 5.7", "min = 6.000004"));
    let report = synth_sim::simulate(&board, &rounded, &options(&stub));
    assert!(report.passed(), "diagnostics: {:?}", report.diagnostics);

    let outside = testbench(&DIVIDER_TB.replace("min = 5.7", "min = 6.00001"));
    let report = synth_sim::simulate(&board, &outside, &options(&stub));
    assert!(!report.passed(), "diagnostics: {:?}", report.diagnostics);
}

#[test]
fn missing_measurement_value_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("missing-meas");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003 && d.severity.is_blocking()),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn failed_measurement_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("failed-meas");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("failed measurement"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn missing_done_sentinel_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("missing-done");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("SYNTH_DONE"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn non_finite_measurement_gives_003() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("nan");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_003
                && d.severity.is_blocking()
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("not finite"))),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn ngspice_error_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("error");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_006
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("unknown subckt"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

/// A singular matrix exits 0 and still prints a value. The warning on stderr
/// is the only evidence the run is wrong.
#[test]
fn singular_matrix_with_exit_zero_gives_006() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let (stub, _dir) = stub_ngspice("singular");

    let report = synth_sim::simulate(&board, &testbench, &options(&stub));

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_006
                && d.message
                    .as_deref()
                    .is_some_and(|m| m.contains("singular matrix"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn missing_binary_gives_001() {
    let board = board(DIVIDER);
    let testbench = testbench(DIVIDER_TB);
    let scratch = Scratch::new("missing-binary");
    let missing = scratch.path().join("ngspice-does-not-exist");

    let report = synth_sim::simulate(
        &board,
        &testbench,
        &synth_sim::SimOptions {
            ngspice: Some(missing),
        },
    );

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == synth_sim::E_SYNTH_SIM_001
                && d.message.as_deref().is_some_and(|m| m.contains("install"))
        }),
        "diagnostics: {:?}",
        report.diagnostics
    );
}

#[test]
fn timeout_gives_005_and_kills_the_process() {
    let board = board(DIVIDER);
    let testbench =
        testbench(&DIVIDER_TB.replace("[analysis]", "[options]\ntimeout = 1.0\n\n[analysis]"));
    let (stub, scratch) = stub_ngspice("hang");

    let started = Instant::now();
    let report = synth_sim::simulate(&board, &testbench, &options(&stub));
    let elapsed = started.elapsed();

    assert!(!report.passed());
    assert_explained(&report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == synth_sim::E_SYNTH_SIM_005),
        "diagnostics: {:?}",
        report.diagnostics
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout took {elapsed:?}, expected about 1s"
    );
    assert!(
        scratch.path().join("hang-started").exists(),
        "the stub never started"
    );
    assert!(
        !scratch.path().join("hang-finished").exists(),
        "the process survived the timeout"
    );
}
