// SPDX-License-Identifier: Apache-2.0

//! `synth sim` is a thin wrapper over `synth_sim::simulate`; these tests
//! drive the command with a stub ngspice that replays a real captured log.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn fixture() -> PathBuf {
    workspace_root()
        .join("crates")
        .join("synth-sim")
        .join("tests")
        .join("fixtures")
        .join("divider-op.out")
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth_cli_sim_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn stub_ngspice(dir: &Path) -> PathBuf {
    let path = dir.join("ngspice");
    std::fs::write(&path, format!("#!/bin/sh\ncat '{}'\n", fixture().display()))
        .expect("write stub");
    let mut permissions = std::fs::metadata(&path).expect("stat stub").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod stub");
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
    let stub = stub_ngspice(&dir);

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
    let stub = stub_ngspice(&dir);
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
}

#[test]
fn malformed_sidecar_exits_nonzero_with_007() {
    let dir = scratch("malformed");
    let stub = stub_ngspice(&dir);
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    std::fs::write(dir.join("divider.sim.toml"), "gronud = \"GND\"\n").expect("write sidecar");

    let output = run_sim(&design, &stub);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E-SYNTH-SIM-007"), "{stderr}");
}
