// SPDX-License-Identifier: Apache-2.0

//! `synth check` runs a testbench when one exists and reports the
//! `simulation` stage without turning absent evidence into a pass.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "../../synth-sim/tests/support/stub_build.rs"]
mod stub_build;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn example_design() -> PathBuf {
    workspace_root()
        .join("examples")
        .join("divider")
        .join("divider.synth")
}

fn example_sidecar() -> PathBuf {
    workspace_root()
        .join("examples")
        .join("divider")
        .join("divider.sim.toml")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "synth_check_simulation_{label}_{}_{nonce}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Self(path)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stub_ngspice(dir: &Path, label: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("support")
        .join("stub_ngspice.rs");
    let built = stub_build::build_stub(&source);
    let mut path = dir.join(label);
    if let Some(extension) = built.extension() {
        path.set_extension(extension);
    }
    std::os::unix::fs::symlink(&built, &path).expect("link stub");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("fixtures");
    std::fs::write(dir.join("fixtures.path"), fixtures.display().to_string())
        .expect("write fixtures.path");
    path
}

fn design_with_sidecar(dir: &Path) -> PathBuf {
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    std::fs::copy(example_sidecar(), dir.join("divider.sim.toml")).expect("copy sidecar");
    design
}

fn run_check(design: &Path, ngspice: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("check")
        .arg(design)
        .arg("--json")
        .env("NGSPICE", ngspice)
        .env("SYNTH_CHECK_STAGE_BUDGET_SECS", "10")
        .output()
        .expect("run synth check")
}

fn report(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "check must emit JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn simulation(report: &serde_json::Value) -> &serde_json::Value {
    &report["stages"]["simulation"]
}

#[test]
fn no_sidecar_is_skipped_not_failed() {
    let dir = Scratch::new("no-sidecar");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    let stub = stub_ngspice(&dir, "ok");

    let output = run_check(&design, &stub);
    let report = report(&output);

    assert_eq!(simulation(&report)["status"], "skipped", "{report:#}");
    assert_eq!(
        simulation(&report)["reason"],
        "no testbench sidecar",
        "{report:#}"
    );
}

#[test]
fn a_passing_sidecar_makes_the_simulation_stage_pass() {
    let dir = Scratch::new("pass");
    let design = design_with_sidecar(&dir);
    let stub = stub_ngspice(&dir, "ok");

    let output = run_check(&design, &stub);
    let report = report(&output);

    assert_eq!(simulation(&report)["status"], "pass", "{report:#}");
    assert_eq!(simulation(&report)["result"]["passed"], true, "{report:#}");
    assert_eq!(
        simulation(&report)["result"]["coverage"]["modelled"],
        serde_json::json!(["R1", "R2"]),
        "{report:#}"
    );
}

#[test]
fn an_out_of_limit_measure_makes_the_simulation_stage_fail() {
    let dir = Scratch::new("limit");
    let design = design_with_sidecar(&dir);
    let sidecar = std::fs::read_to_string(dir.join("divider.sim.toml"))
        .expect("read sidecar")
        .replace("max = 6.3", "max = 5.9");
    std::fs::write(dir.join("divider.sim.toml"), sidecar).expect("write sidecar");
    let stub = stub_ngspice(&dir, "ok");

    let output = run_check(&design, &stub);
    let report = report(&output);

    assert_eq!(simulation(&report)["status"], "fail", "{report:#}");
    assert!(
        simulation(&report)["result"]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "E-SYNTH-SIM-004"),
        "{report:#}"
    );
}

#[test]
fn a_missing_ngspice_makes_the_simulation_stage_unknown() {
    let dir = Scratch::new("missing-ngspice");
    let design = design_with_sidecar(&dir);
    let missing = dir.join("ngspice-does-not-exist");

    let output = run_check(&design, &missing);
    let report = report(&output);

    assert_eq!(simulation(&report)["status"], "unknown", "{report:#}");
    assert_eq!(report["status"], "fail", "{report:#}");
}

#[test]
fn a_malformed_sidecar_makes_the_simulation_stage_fail() {
    let dir = Scratch::new("malformed");
    let design = design_with_sidecar(&dir);
    std::fs::write(dir.join("divider.sim.toml"), "gronud = \"GND\"\n").expect("write sidecar");
    let stub = stub_ngspice(&dir, "ok");

    let output = run_check(&design, &stub);
    let report = report(&output);

    assert_eq!(simulation(&report)["status"], "fail", "{report:#}");
    assert!(
        simulation(&report)["result"]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "E-SYNTH-SIM-007"),
        "{report:#}"
    );
}

#[test]
fn a_source_failure_skips_simulation() {
    let dir = Scratch::new("source");
    let design = dir.join("broken.synth");
    std::fs::write(&design, "board \"broken\" {\n").expect("write design");
    let stub = stub_ngspice(&dir, "ok");

    let output = run_check(&design, &stub);
    let report = report(&output);

    assert_eq!(report["stages"]["source"]["status"], "fail", "{report:#}");
    assert_eq!(simulation(&report)["status"], "skipped", "{report:#}");
    assert_eq!(
        simulation(&report)["reason"],
        "source gate failed",
        "{report:#}"
    );
}
