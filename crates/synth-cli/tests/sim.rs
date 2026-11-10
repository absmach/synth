// SPDX-License-Identifier: Apache-2.0

//! `synth sim` is a thin wrapper over `synth_sim::simulate`; these tests
//! drive the command with a Rust stub ngspice that replays a real captured log.

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

/// A scratch directory removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "synth_cli_sim_{label}_{}_{nonce}",
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

fn scratch(label: &str) -> Scratch {
    Scratch::new(label)
}

fn stub_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("support")
        .join("stub_ngspice.rs")
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("fixtures")
}

fn stub_ngspice(dir: &Path, label: &str) -> PathBuf {
    let built = stub_build::build_stub(&stub_source());
    let mut path = dir.join(label);
    if let Some(extension) = built.extension() {
        path.set_extension(extension);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&built, &path).expect("link stub");
    #[cfg(not(unix))]
    std::fs::copy(&built, &path).expect("copy stub");
    std::fs::write(
        dir.join("fixtures.path"),
        fixtures_dir().display().to_string(),
    )
    .expect("write fixtures.path");
    path
}

fn run_sim(design: &Path, stub: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("sim")
        .arg(design)
        .env("NGSPICE", stub)
        .output()
        .expect("run synth sim")
}

#[test]
fn divider_example_prints_pass() {
    let dir = scratch("pass");
    let stub = stub_ngspice(&dir, "ok");

    let output = run_sim(&example_design(), &stub);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS vmid = 6"), "{stdout}");
}

#[test]
fn out_of_limit_exits_nonzero_with_004() {
    let dir = scratch("limit");
    let stub = stub_ngspice(&dir, "ok");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    let sidecar = std::fs::read_to_string(example_sidecar())
        .expect("read example sidecar")
        .replace("min = 5.7", "min = 4.0")
        .replace("max = 6.3", "max = 5.5");
    std::fs::write(dir.join("divider.sim.toml"), sidecar).expect("write sidecar");

    let output = run_sim(&design, &stub);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FAIL vmid = 6"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E-SYNTH-SIM-004"), "{stderr}");
    assert!(!stderr.contains("(?)"), "{stderr}");
    assert!(stderr.contains("divider.sim.toml"), "{stderr}");
}

#[test]
fn malformed_sidecar_exits_nonzero_with_007() {
    let dir = scratch("malformed");
    let stub = stub_ngspice(&dir, "ok");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    std::fs::write(dir.join("divider.sim.toml"), "gronud = \"GND\"\n").expect("write sidecar");

    let output = run_sim(&design, &stub);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E-SYNTH-SIM-007"), "{stderr}");
}

#[test]
fn missing_sidecar_exits_nonzero_with_007() {
    let dir = scratch("no-sidecar");
    let stub = stub_ngspice(&dir, "ok");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");

    let output = run_sim(&design, &stub);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E-SYNTH-SIM-007"), "{stderr}");
    assert!(stderr.contains("divider.sim.toml"), "{stderr}");
}

#[test]
fn a_measure_without_limits_prints_measured() {
    let dir = scratch("measured");
    let stub = stub_ngspice(&dir, "ok");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    let sidecar = std::fs::read_to_string(example_sidecar())
        .expect("read example sidecar")
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("min =") && !line.starts_with("max =")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(dir.join("divider.sim.toml"), format!("{sidecar}\n")).expect("write sidecar");

    let output = run_sim(&design, &stub);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MEASURED vmid = 6"), "{stdout}");
    assert!(!stdout.contains("PASS vmid"), "{stdout}");
}

/// The LDO part only simulates when `synth sim` passes the discovered
/// registry directory through to model resolution.
#[test]
fn an_ldo_board_resolves_its_registry_model() {
    let dir = scratch("ldo");
    let stub = stub_ngspice(&dir, "ldo");
    let design = dir.join("ldo.synth");
    std::fs::write(
        &design,
        r#"
board "ldo" {
  layers 2
  component U1: regulator "ams1117_3v3"
  component R1: resistor "r_generic_0805" value "1k"
  net "VIN" { U1.vin }
  net "GND" { U1.gnd, R1.p2 }
  net "VOUT" { U1.vout, R1.p1 }
}
"#,
    )
    .expect("write design");
    std::fs::write(
        dir.join("ldo.sim.toml"),
        r#"
ground = "GND"

[[source]]
net = "VIN"
kind = "dc"
dc = 5.0

[analysis]
op = true

[[measure]]
name = "vout"
kind = "node_voltage"
node = "VOUT"
min = 3.25
max = 3.35
"#,
    )
    .expect("write sidecar");

    let output = run_sim(&design, &stub);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS vout = 3.29967"), "{stdout}");
}
