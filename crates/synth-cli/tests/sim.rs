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

fn run_suggest(design: &Path, force: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_synth"));
    command.arg("sim").arg("--suggest").arg(design);
    if force {
        command.arg("--force");
    }
    command.output().expect("run synth sim --suggest")
}

#[test]
fn suggest_writes_a_parseable_sidecar() {
    let dir = scratch("suggest");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");

    let output = run_suggest(&design, false);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("wrote"), "{stderr}");
    let sidecar = dir.join("divider.sim.toml");
    let testbench =
        synth_sim::Testbench::from_path(&sidecar).expect("the suggested sidecar parses");
    let vmid = testbench
        .measure
        .iter()
        .find(|measure| measure.name == "vmid")
        .expect("vmid");
    assert_eq!(vmid.min, Some(5.7));
    assert_eq!(vmid.max, Some(6.3));
}

#[test]
fn suggest_refuses_to_overwrite_without_force() {
    let dir = scratch("suggest-overwrite");
    let design = dir.join("divider.synth");
    std::fs::copy(example_design(), &design).expect("copy design");
    let sidecar = dir.join("divider.sim.toml");
    std::fs::write(&sidecar, "sentinel = true\n").expect("write sentinel");

    let refused = run_suggest(&design, false);

    assert_eq!(refused.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("refusing to overwrite"), "{stderr}");
    assert!(stderr.contains("--force"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&sidecar).expect("sentinel"),
        "sentinel = true\n"
    );

    let forced = run_suggest(&design, true);

    assert!(
        forced.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&forced.stderr)
    );
    let written = std::fs::read_to_string(&sidecar).expect("overwritten sidecar");
    assert_ne!(written, "sentinel = true\n");
    synth_sim::Testbench::from_path(&sidecar).expect("the overwritten sidecar parses");
}

#[test]
fn suggest_with_no_shape_writes_nothing_and_says_so() {
    let dir = scratch("suggest-none");
    let design = dir.join("none.synth");
    std::fs::write(
        &design,
        r#"
board "none" {
  layers 2
  component R1: resistor "r_generic_0805" value "10k"
  power "VCC" 12v
  net "VCC" { R1.p1 }
  net "GND" { R1.p2 }
}
"#,
    )
    .expect("write design");

    let output = run_suggest(&design, false);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.join("none.sim.toml").exists(), "no file is written");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing written"), "{stderr}");
}

#[test]
fn suggest_prints_the_overdrive_line_and_writes_a_failing_band() {
    let dir = scratch("suggest-overdrive");
    let design = dir.join("overdrive.synth");
    std::fs::write(
        &design,
        r#"
board "overdrive" {
  layers 2
  component R1: resistor "r_generic_0805" value "10"
  component LED1: led "ltst_c190krkt"
  power "VCC" 24v
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#,
    )
    .expect("write design");

    let output = run_suggest(&design, false);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.lines().any(|line| line.starts_with("overdrive:")),
        "{stderr}"
    );
    assert!(!stderr.contains("not covered:"), "{stderr}");
    let testbench = synth_sim::Testbench::from_path(&dir.join("overdrive.sim.toml"))
        .expect("the suggested sidecar parses");
    let iled = testbench
        .measure
        .iter()
        .find(|measure| measure.name == "iled")
        .expect("iled");
    assert_eq!(iled.max, Some(0.025));
}

#[test]
fn suggest_reports_a_part_without_a_datasheet() {
    let dir = scratch("suggest-no-datasheet");
    let design = dir.join("no_datasheet.synth");
    std::fs::write(
        &design,
        r#"
board "no-datasheet" {
  layers 2
  component R1: resistor "r_generic_0805" value "150"
  component LED1: led "led_red_0603"
  power "VCC" 5v
  net "VCC" { R1.p1 }
  net "GND" { LED1.cathode }
  connect R1.p2 -> LED1.anode as "MID"
}
"#,
    )
    .expect("write design");

    let output = run_suggest(&design, false);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.join("no_datasheet.sim.toml").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not covered: LED1 (led_red_0603), no datasheet"),
        "{stderr}"
    );
}
