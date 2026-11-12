// SPDX-License-Identifier: Apache-2.0

//! The `synth_simulate` MCP tool: a thin wrapper over `synth_sim::simulate`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

#[path = "../../synth-sim/tests/support/stub_build.rs"]
mod stub_build;

#[path = "../../synth-sim/tests/support/real_ngspice.rs"]
mod real_ngspice_support;

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

/// A valid divider whose group colour is invalid: lowering reports a warning,
/// the board still compiles and the simulation passes.
const WARNED_DIVIDER: &str = r##"
board "divider" {
  layers 2
  group "power" color "#zzzzzz" {
    component R1: resistor "r_generic_0805" value "10k"
    component R2: resistor "r_generic_0805" value "10k"
  }
  net "VCC" { R1.p1 }
  net "GND" { R2.p2 }
  connect R1.p2 -> R2.p1 as "MID"
}
"##;

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
    design_dir_with(DIVIDER, testbench)
}

fn design_dir_with(design: &str, testbench: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("design dir");
    std::fs::write(dir.path().join("divider.synth"), design).expect("write design");
    std::fs::write(dir.path().join("divider.sim.toml"), testbench).expect("write sidecar");
    dir
}

/// Point the tool at a stub for this test. The schema deliberately has no
/// argument for this, so a remote caller cannot choose the executable.
fn use_ngspice(ngspice: &Path) {
    synth_mcp::set_test_ngspice(Some(ngspice.to_path_buf()));
}

fn args(design: &Path) -> Value {
    json!({
        "file_path": design.join("divider.synth").display().to_string(),
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
    use_ngspice(&stub);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(result["measures"][0]["name"], "vmid");
    assert_eq!(result["measures"][0]["value"], 6.0);
    assert_eq!(result["measures"][0]["unit"], "V");
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
    use_ngspice(&stub);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], false, "{result}");
    assert_eq!(result["measures"][0]["passed"], false);
    assert!(has_code(&result, "E-SYNTH-SIM-004"), "{result}");
}

#[test]
fn a_missing_ngspice_is_a_tool_result_with_001() {
    let design = design_dir(DIVIDER_TB);
    let missing = design.path().join("ngspice-does-not-exist");
    use_ngspice(&missing);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
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
    use_ngspice(&stub);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
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
    use_ngspice(&stub);
    let explicit = design.path().join("elsewhere.toml");
    std::fs::rename(design.path().join("divider.sim.toml"), &explicit)
        .expect("move the sidecar away from its default location");
    let mut arguments = args(design.path());
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
    use_ngspice(&stub);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert_eq!(result["measures"][0]["value"], 6.0);
    assert!(
        result["measures"][0]["passed"].is_null(),
        "a measure with no limits has nothing to pass: {result}"
    );
}

#[test]
fn the_schema_does_not_let_a_caller_choose_the_executable() {
    let tool = synth_mcp::list_tools()
        .into_iter()
        .find(|tool| tool.name == "synth_simulate")
        .expect("synth_simulate is listed");
    assert!(
        tool.input_schema["properties"]
            .get("ngspice_path")
            .is_none(),
        "a remote caller must not choose the binary: {}",
        tool.input_schema
    );
}

#[test]
fn an_ngspice_path_argument_cannot_choose_the_executable() {
    if real_ngspice_support::real_ngspice().is_none() {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    }
    let design = design_dir(DIVIDER_TB);
    let (stub, stub_dir) = stub_ngspice("marker");
    let marker = stub_dir.path().join("marker.ran");
    synth_mcp::set_test_ngspice(None);
    let mut arguments = args(design.path());
    arguments["ngspice_path"] = json!(stub.display().to_string());

    let result = synth_mcp::call_tool("synth_simulate", &arguments, None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert!(!marker.exists(), "the tool ran the ngspice_path argument");
}

#[test]
fn compile_warnings_are_returned_with_a_passing_simulation() {
    let design = design_dir_with(WARNED_DIVIDER, DIVIDER_TB);
    let (stub, _stub_dir) = stub_ngspice("ok");
    use_ngspice(&stub);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    assert!(has_code(&result, "E-SYNTH-NAME-011"), "{result}");
}

#[test]
fn a_divider_matches_real_ngspice() {
    let Some(ngspice) = real_ngspice_support::real_ngspice() else {
        eprintln!("{}", real_ngspice_support::SKIP_MESSAGE);
        return;
    };
    let design = design_dir(DIVIDER_TB);
    use_ngspice(&ngspice);

    let result = synth_mcp::call_tool("synth_simulate", &args(design.path()), None)
        .expect("the tool returns a result");

    assert_eq!(result["passed"], true, "{result}");
    let value = result["measures"][0]["value"].as_f64().expect("value");
    assert!(
        (value - 6.0).abs() < 0.01,
        "vmid {value} should be about 6.0 V"
    );
}
