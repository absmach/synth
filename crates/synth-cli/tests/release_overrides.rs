// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

mod stub_router;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture_root() -> PathBuf {
    workspace_root().join("fixtures").join("release-overrides")
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth_release_{}_{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn kicad_stub(dir: &Path) -> PathBuf {
    let path = dir.join("kicad-cli");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = version ]; then echo 10.0.1; exit 0; fi
out=""; prev=""
for a in "$@"; do if [ "$prev" = "--output" ]; then out="$a"; fi; prev="$a"; done
case "$1 $2" in
  "sch erc"|"pcb drc") printf '{"kicad_version":"10.0.1","violations":[]}' > "$out"; exit 0 ;;
  "pcb export") exit 0 ;;
esac
exit 0
"#,
    )
    .expect("write stub");
    let mut perms = std::fs::metadata(&path).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

fn design(dir: &Path, part_id: &str) -> PathBuf {
    let path = dir.join("board.synth");
    std::fs::write(
        &path,
        format!(
            "board \"rel\" {{\n  layers 2\n  component R1: resistor \"{part_id}\" value \"10k\"\n  \
             component R2: resistor \"{part_id}\" value \"10k\"\n  connect R1.p1 -> R2.p1\n}}\n"
        ),
    )
    .expect("write design");
    path
}

struct Run {
    code: Option<i32>,
    stderr: String,
    manifest: Option<serde_json::Value>,
}

/// The gate is fail-closed on routing, so every run here gets a stub router.
/// Without one the export stops at the routing gate and the test would only
/// ever be asserting that refusal, not the override behaviour it is about.
fn export(label: &str, part_dir: &str, part_id: &str, footprints: bool, extra: &[&str]) -> Run {
    let dir = scratch(label);
    // The gate is fail-closed on routing, so every run here gets a stub
    // router. Without one the export stops at the routing gate and the test
    // would only ever be asserting that refusal, not the override behaviour
    // it is actually about.
    let router = stub_router::install(&dir);
    let cli = kicad_stub(&dir);
    let board = design(&dir, part_id);
    let out = dir.join("out");

    let mut command = Command::new(SYNTH);
    command
        .envs(router.envs())
        .arg("export-kicad")
        .arg(&board)
        .arg("--registry")
        .arg(fixture_root().join(part_dir))
        .arg("--out")
        .arg(&out)
        .args(extra)
        .env("KICAD_CLI", &cli);
    if footprints {
        command.env("KICAD_FOOTPRINT_DIR", fixture_root().join("footprints"));
    } else {
        command.env("KICAD_FOOTPRINT_DIR", dir.join("no-footprints-here"));
    }

    let output = command.output().expect("run synth export-kicad");
    let manifest = std::fs::read_to_string(out.join("release.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    Run {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        manifest,
    }
}

fn flags(manifest: &serde_json::Value) -> Vec<String> {
    manifest["overrides"].as_array().map_or_default(|o| {
        o.iter()
            .map(|r| r["flag"].as_str().unwrap_or_default().to_string())
            .collect()
    })
}

#[test]
fn a_clean_export_is_a_production_package() {
    let run = export("clean", "reviewed", "rel_reviewed", true, &["--gerbers"]);
    let m = run.manifest.expect("manifest written beside the artifacts");
    assert_eq!(m["release_ready"], true, "{m:#}");
    assert_eq!(m["production_status"], "production", "{m:#}");
    assert!(flags(&m).is_empty(), "{m:#}");
    assert!(
        !run.stderr.contains("UNTRUSTED"),
        "a clean package must not be labelled untrusted:\n{}",
        run.stderr
    );
}

#[test]
fn a_part_with_no_kicad_symbol_is_listed_as_synthetic() {
    // The release fixture part names a footprint but no KiCad symbol, so
    // its schematic symbol is drawn from the registry pin map.
    let run = export(
        "synthetic",
        "reviewed",
        "rel_reviewed",
        true,
        &["--gerbers"],
    );
    let m = run.manifest.expect("manifest");
    assert_eq!(
        m["synthetic_symbols"],
        serde_json::json!(["rel_reviewed"]),
        "{m:#}"
    );
}

#[test]
fn unreviewed_parts_are_recorded_with_their_reviewer_state() {
    let run = export(
        "unreviewed",
        "unreviewed",
        "rel_unreviewed",
        true,
        &["--gerbers", "--allow-unverified-parts"],
    );
    let m = run.manifest.expect("manifest");
    assert_eq!(m["release_ready"], false, "{m:#}");
    assert_eq!(m["production_status"], "untrusted", "{m:#}");
    assert!(
        flags(&m).contains(&"--allow-unverified-parts".to_string()),
        "{m:#}"
    );
    assert_eq!(m["overrides"][0]["affected_parts"][0], "rel_unreviewed");
    assert_eq!(m["reviewer_state"][0]["part_id"], "rel_unreviewed");
    assert_eq!(m["reviewer_state"][0]["reviewed"], false);
    assert!(
        run.stderr.contains("UNTRUSTED / NOT FOR FABRICATION"),
        "{}",
        run.stderr
    );
}

#[test]
fn missing_footprints_forced_through_are_recorded() {
    let run = export(
        "bounding_box",
        "reviewed",
        "rel_reviewed",
        false,
        &["--gerbers", "--force"],
    );
    let m = run.manifest.expect("manifest");
    assert_eq!(m["release_ready"], false, "{m:#}");
    assert!(flags(&m).contains(&"--force".to_string()), "{m:#}");
    let codes: Vec<&str> = m["overrides"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"E-SYNTH-EXPORT-001"), "{codes:?}");
    assert!(
        run.stderr.contains("UNTRUSTED / NOT FOR FABRICATION"),
        "{}",
        run.stderr
    );
}

#[test]
fn no_override_can_produce_a_release_ready_package() {
    for (label, part_dir, part_id, footprints, extra) in [
        (
            "accept_unverified",
            "unreviewed",
            "rel_unreviewed",
            true,
            &["--gerbers", "--allow-unverified-parts"][..],
        ),
        (
            "accept_bounding_box",
            "reviewed",
            "rel_reviewed",
            false,
            &["--gerbers", "--force"][..],
        ),
        (
            "accept_both",
            "unreviewed",
            "rel_unreviewed",
            false,
            &["--gerbers", "--force", "--allow-unverified-parts"][..],
        ),
    ] {
        let run = export(label, part_dir, part_id, footprints, extra);
        let m = run
            .manifest
            .unwrap_or_else(|| panic!("{label}: manifest\n{}", run.stderr));
        assert_eq!(
            m["release_ready"], false,
            "{label}: an override must never be release ready\n{m:#}"
        );
        assert_eq!(m["production_status"], "untrusted", "{label}\n{m:#}");
    }
}

#[test]
fn safe_mode_refuses_to_sit_beside_an_override_flag() {
    let dir = scratch("safe_conflict");
    // Flag validation happens before any routing, so this one needs no stub.
    let board = design(&dir, "rel_reviewed");
    let output = Command::new(SYNTH)
        .arg("export-kicad")
        .arg(&board)
        .arg("--out")
        .arg(dir.join("out"))
        .arg("--safe")
        .arg("--force")
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("cannot be used with"),
        "safe mode must reject the flag combination outright:\n{stderr}"
    );
}

#[test]
fn safe_mode_fails_when_an_override_would_be_needed() {
    let run = export(
        "safe_needs_override",
        "reviewed",
        "rel_reviewed",
        false,
        &["--gerbers", "--safe"],
    );
    assert_ne!(run.code, Some(0), "stderr:\n{}", run.stderr);
}

#[test]
fn safe_mode_succeeds_on_a_clean_package() {
    let run = export(
        "safe_clean",
        "reviewed",
        "rel_reviewed",
        true,
        &["--gerbers", "--safe"],
    );
    let m = run.manifest.expect("manifest");
    assert_eq!(run.code, Some(0), "stderr:\n{}", run.stderr);
    assert_eq!(m["release_ready"], true, "{m:#}");
}

mod release_gate {
    use super::*;

    fn check_fab(
        label: &str,
        part_dir: &str,
        part_id: &str,
        extra: &[&str],
    ) -> (Option<i32>, serde_json::Value) {
        let dir = scratch(label);
        // `check --fab` drives `export-kicad` as a subprocess, so the stub
        // has to be in the environment for both.
        let router = stub_router::install(&dir);
        let cli = kicad_stub(&dir);
        let board = design(&dir, part_id);
        let output = Command::new(SYNTH)
            .envs(router.envs())
            .arg("check")
            .arg(&board)
            .arg("--registry")
            .arg(fixture_root().join(part_dir))
            .arg("--fab")
            .arg("--json")
            .args(extra)
            .env("KICAD_CLI", &cli)
            .env("KICAD_FOOTPRINT_DIR", fixture_root().join("footprints"))
            .output()
            .expect("run synth check");
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "{label}: check must emit JSON: {e}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code(), report)
    }

    #[test]
    fn an_unauthorized_override_is_rejected() {
        let (code, report) = check_fab(
            "gate_reject",
            "unreviewed",
            "rel_unreviewed",
            &["--allow-unverified-parts"],
        );
        let m = &report["stages"]["manufacturing"];
        assert_eq!(code, Some(1), "{report:#}");
        assert_eq!(m["status"], "fail", "{report:#}");
        assert_eq!(m["reason"], "unauthorized_overrides", "{report:#}");
        assert_eq!(m["release_ready"], false, "{report:#}");
    }

    #[test]
    fn a_recorded_exception_authorizes_the_run_without_cleaning_it() {
        let (code, report) = check_fab(
            "gate_exception",
            "unreviewed",
            "rel_unreviewed",
            &[
                "--allow-unverified-parts",
                "--override-exception",
                "prototype run, not customer hardware",
                "--authorized-by",
                "a release manager",
            ],
        );
        let m = &report["stages"]["manufacturing"];
        assert_eq!(code, Some(0), "{report:#}");
        assert_eq!(m["status"], "pass", "{report:#}");
        assert_eq!(
            m["exception_authorized_by"], "a release manager",
            "{report:#}"
        );
        assert_eq!(
            m["release_ready"], false,
            "authorizing a run must not make the package release ready\n{report:#}"
        );
        assert_eq!(m["release"]["production_status"], "untrusted", "{report:#}");
    }

    #[test]
    fn an_exception_needs_both_a_reason_and_an_authorizer() {
        let dir = scratch("gate_partial_exception");
        // Argument validation happens before any routing, so no stub needed.
        let board = design(&dir, "rel_unreviewed");
        let output = Command::new(SYNTH)
            .arg("check")
            .arg(&board)
            .arg("--fab")
            .arg("--override-exception")
            .arg("a reason with nobody behind it")
            .output()
            .expect("run");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(output.status.code(), Some(0), "{stderr}");
        assert!(
            stderr.contains("authorized-by") || stderr.contains("authorized_by"),
            "the missing authorizer must be named:\n{stderr}"
        );
    }

    #[test]
    fn a_clean_package_passes_the_gate() {
        let (code, report) = check_fab("gate_clean", "reviewed", "rel_reviewed", &[]);
        let m = &report["stages"]["manufacturing"];
        assert_eq!(code, Some(0), "{report:#}");
        assert_eq!(m["status"], "pass", "{report:#}");
        assert_eq!(m["release_ready"], true, "{report:#}");
        assert_eq!(
            m["release"]["production_status"], "production",
            "{report:#}"
        );
    }
}
