// SPDX-License-Identifier: Apache-2.0

//! `synth validate` runs the real placer through its bounded ladder: it
//! reports the board it gets, and fails only when no canvas and no hint level
//! inside the budget could place it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

struct Run {
    payload: serde_json::Value,
    success: bool,
}

fn validate(path: &Path, root: &Path, extra: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["validate", "--format", "json"])
        .args(extra)
        .arg(path)
        .current_dir(root)
        .output()
        .expect("run synth validate");
    Run {
        payload: serde_json::from_slice(&out.stdout).expect("validate --format json emits json"),
        success: out.status.success(),
    }
}

fn find<'a>(payload: &'a serde_json::Value, code: &str) -> Vec<&'a serde_json::Value> {
    payload["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter(|d| d["code"] == code)
        .collect()
}

/// A design that cannot place even on the largest canvas: the keepout covers
/// every point, so the diagnostic is a proven failure, not a budget cut-off.
const UNPLACEABLE: &str = r#"board "unplaceable" {
    layers 2
    component J1: connector "header_1x20" {
        placement_hint { edge left priority: hard }
    }
    component C1: capacitor "c_generic_0603" {
        placement_hint { near: "J1" priority: hard }
    }
    connect J1.p1 -> C1.p1
    keepout board_blocker {
        radius 600mm
    }
}"#;

const PLACEABLE: &str = r#"board "placeable" {
    layers 2
    component U1: regulator "ams1117_3v3"
    component C1: capacitor "c_generic_0603"
    component C2: capacitor "c_generic_0603"
    component R1: resistor "r_generic_0603"
    connect U1.vin -> C1.p1
    connect U1.vout -> C2.p1
    connect U1.gnd -> R1.p1
}"#;

fn write(scratch: &Path, name: &str, source: &str) -> PathBuf {
    std::fs::create_dir_all(scratch).expect("scratch dir");
    let path = scratch.join(name);
    std::fs::write(&path, source).expect("write fixture");
    path
}

#[test]
fn validate_reports_the_placement_it_gets() {
    let root = workspace_root();
    let scratch = std::env::temp_dir().join(format!("synth_preflight_ok_{}", std::process::id()));
    let path = write(&scratch, "placeable.synth", PLACEABLE);

    let run = validate(&path, &root, &[]);
    assert!(
        find(&run.payload, "E-SYNTH-PLACE-002").is_empty(),
        "a board that places must not report a placement failure"
    );
    let reports = find(&run.payload, "W-SYNTH-PLACE-004");
    assert_eq!(reports.len(), 1, "{:?}", run.payload);
    let report = reports[0];
    assert_eq!(report["severity"], "info");
    let message = report["message"].as_str().expect("message");
    assert!(message.contains("legal placement"), "{message}");
    assert!(message.contains("mm2"), "{message}");

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn an_unplaceable_board_fails_and_names_the_part() {
    let root = workspace_root();
    let scratch = std::env::temp_dir().join(format!("synth_preflight_bad_{}", std::process::id()));
    let path = write(&scratch, "unplaceable.synth", UNPLACEABLE);

    let run = validate(&path, &root, &[]);
    assert!(
        !run.success,
        "a board no canvas can hold must fail validate"
    );
    let failures = find(&run.payload, "E-SYNTH-PLACE-002");
    assert_eq!(failures.len(), 1, "{:?}", run.payload);
    let message = failures[0]["message"].as_str().expect("message");
    assert!(
        message.contains("`J1`") || message.contains("`C1`"),
        "{message}"
    );
    assert!(message.contains("placement_hint"), "{message}");
    assert!(message.contains("need about"), "{message}");
    assert!(message.contains("canvas"), "{message}");

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_escape_flag_skips_the_preflight() {
    let root = workspace_root();
    let scratch = std::env::temp_dir().join(format!("synth_preflight_skip_{}", std::process::id()));
    let path = write(&scratch, "unplaceable.synth", UNPLACEABLE);

    let run = validate(&path, &root, &["--no-placement-preflight"]);
    assert!(find(&run.payload, "E-SYNTH-PLACE-002").is_empty());
    assert!(find(&run.payload, "W-SYNTH-PLACE-004").is_empty());

    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_sidecar_board_reports_both_placements() {
    let root = workspace_root();
    let scratch = std::env::temp_dir().join(format!("synth_preflight_side_{}", std::process::id()));
    let path = write(&scratch, "placeable.synth", PLACEABLE);
    let sidecar = scratch.join("placeable.placement.layout.toml");
    std::fs::write(
        &sidecar,
        "schema_version = 2\n\n[components.U1]\nx = 20.0\ny = 15.0\nrotation = 0\nsource = \"human_drag\"\npriority = \"hard\"\n",
    )
    .expect("sidecar written");

    let run = validate(&path, &root, &[]);
    let reports = find(&run.payload, "W-SYNTH-PLACE-004");
    assert_eq!(
        reports.len(),
        2,
        "one sidecar report and one from-scratch report: {:?}",
        run.payload
    );
    let titles: Vec<&str> = reports
        .iter()
        .map(|report| report["title"].as_str().unwrap_or_default())
        .collect();
    assert!(
        titles.iter().any(|title| title.contains("sidecar")),
        "{titles:?}"
    );
    assert!(
        titles.iter().any(|title| title.contains("preflight")),
        "{titles:?}"
    );
    let sidecar_report = reports
        .iter()
        .find(|report| {
            report["title"]
                .as_str()
                .is_some_and(|title| title.contains("sidecar"))
        })
        .expect("sidecar report");
    let message = sidecar_report["message"].as_str().expect("message");
    assert!(
        message.contains("3 part(s) are missing"),
        "a stale sidecar must say which parts it does not pin: {message}"
    );
    assert!(message.contains("dead area"), "{message}");

    let _ = std::fs::remove_dir_all(&scratch);
}
