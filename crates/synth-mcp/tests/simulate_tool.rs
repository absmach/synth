// SPDX-License-Identifier: Apache-2.0

//! The `synth_simulate` MCP tool: a thin wrapper over `synth_sim::simulate`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

#[path = "../../synth-sim/tests/support/stub_build.rs"]
mod stub_build;

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

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("fixtures")
}

fn stub_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("support")
        .join("stub_ngspice.rs")
}

fn registry_parts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("parts")
        .canonicalize()
        .expect("workspace registry/parts")
}

/// The stub reads its behaviour from its file stem and the replay fixtures
/// from `fixtures.path` beside it — same contract as the synth-sim tests.
fn stub_ngspice(label: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("stub dir");
    let built = stub_build::build_stub(&stub_source());
    let mut path = dir.path().join(label);
    if let Some(extension) = built.extension() {
        path.set_extension(extension);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&built, &path).expect("link stub");
    #[cfg(not(unix))]
    std::fs::copy(&built, &path).expect("copy stub");
    std::fs::write(
        dir.path().join("fixtures.path"),
        fixtures_dir().display().to_string(),
    )
    .expect("write fixtures.path");
    (path, dir)
}

/// A design directory with the divider and its `<design>.sim.toml` sidecar.
fn design_dir(testbench: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("design dir");
    std::fs::write(dir.path().join("divider.synth"), DIVIDER).expect("write design");
    std::fs::write(dir.path().join("divider.sim.toml"), testbench).expect("write sidecar");
    dir
}

fn args(design: &Path, ngspice: &Path) -> Value {
    json!({
        "file_path": design.join("divider.synth").display().to_string(),
        "ngspice_path": ngspice.display().to_string(),
        "registry_path": registry_parts().display().to_string(),
    })
}

fn has_code(result: &Value, code: &str) -> bool {
    result["diagnostics"].as_array().is_some_and(|diagnostics| {
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == code)
    })
}

#[test]
fn a_passing_measurement_returns_passed() {
    let design = design_dir(DIVIDER_TB);
    let (stub, _stub_dir) = stub_ngspice("ok");

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &stub), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(result["measures"][0]["name"], "vmid");
    assert_eq!(result["measures"][0]["value"], 6.0);
    assert_eq!(result["measures"][0]["passed"], true);
}

#[test]
fn an_out_of_limit_measurement_fails_with_004() {
    let design = design_dir(
        &DIVIDER_TB
            .replace("min = 5.7", "min = 4.0")
            .replace("max = 6.3", "max = 5.0"),
    );
    let (stub, _stub_dir) = stub_ngspice("ok");

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &stub), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], false, "{result}");
    assert_eq!(result["measures"][0]["passed"], false);
    assert!(has_code(&result, "E-SYNTH-SIM-004"), "{result}");
}

#[test]
fn a_missing_ngspice_is_a_tool_result_with_001() {
    let design = design_dir(DIVIDER_TB);
    let missing = design.path().join("ngspice-does-not-exist");

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &missing), None)
        .expect("a missing binary is a result, not a transport error");

    assert_eq!(result["passed"], false, "{result}");
    assert!(has_code(&result, "E-SYNTH-SIM-001"), "{result}");
}

#[test]
fn inline_source_is_rejected_because_the_sidecar_lives_beside_the_design() {
    let error = synth_mcp::call_tool(
        "synth_simulate",
        &json!({ "source": "board \"divider\" { layers 2 }" }),
        None,
    )
    .expect_err("inline source has nowhere to put the sidecar");

    assert!(error.contains("file_path"), "{error}");
    assert!(error.contains("inline"), "{error}");
    assert!(error.contains("sidecar"), "{error}");
}

#[test]
fn a_newline_in_a_measured_node_gives_007() {
    let design =
        design_dir(&DIVIDER_TB.replace("node = \"MID\"", "node = \"MID\\n.SHELL echo pwned\""));
    let (stub, _stub_dir) = stub_ngspice("ok");

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &stub), None)
        .expect("a malformed sidecar is a result, not a transport error");

    assert_eq!(result["passed"], false, "{result}");
    assert!(has_code(&result, "E-SYNTH-SIM-007"), "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("newline"))),
        "{result}"
    );
}

#[test]
fn an_explicit_testbench_path_is_used() {
    let design = design_dir(DIVIDER_TB);
    let (stub, _stub_dir) = stub_ngspice("ok");
    let explicit = design.path().join("elsewhere.toml");
    std::fs::rename(design.path().join("divider.sim.toml"), &explicit)
        .expect("move the sidecar away from its default location");
    let mut arguments = args(design.path(), &stub);
    arguments["testbench_path"] = json!(explicit.display().to_string());

    let result = synth_mcp::call_tool("synth_simulate", &arguments, None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(result["measures"][0]["value"], 6.0);
}

#[test]
fn an_unbounded_measurement_reports_measured_not_passed() {
    let sidecar = DIVIDER_TB
        .replace("min = 5.7\n", "")
        .replace("max = 6.3\n", "");
    let design = design_dir(&sidecar);
    let (stub, _stub_dir) = stub_ngspice("ok");

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &stub), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(result["measures"][0]["value"], 6.0);
    assert!(
        result["measures"][0]["passed"].is_null(),
        "a measure with no limits has nothing to pass: {result}"
    );
}

#[test]
fn a_divider_matches_real_ngspice() {
    let ngspice = PathBuf::from("/usr/bin/ngspice");
    if !ngspice.is_file() {
        eprintln!("skipping: ngspice is not installed (sudo apt install ngspice)");
        return;
    }
    let design = design_dir(DIVIDER_TB);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path(), &ngspice), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    let value = result["measures"][0]["value"].as_f64().expect("value");
    assert!(
        (value - 6.0).abs() < 0.01,
        "vmid {value} should be about 6.0 V"
    );
}
