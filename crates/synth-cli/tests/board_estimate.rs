// SPDX-License-Identifier: Apache-2.0

//! `synth validate` reports the board-size estimate: a warning for an outline
//! far larger than the parts need, an info when the board has no outline.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn validate(path: &Path, root: &Path) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["validate", "--format", "json"])
        .arg(path)
        .current_dir(root)
        .output()
        .expect("run synth validate");
    serde_json::from_slice(&out.stdout).expect("validate --format json emits json")
}

fn board_estimate(diagnostics: &serde_json::Value) -> &serde_json::Value {
    diagnostics["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .find(|d| d["code"] == "W-SYNTH-BOARD-004")
        .expect("a W-SYNTH-BOARD-004 diagnostic")
}

#[test]
fn validate_warns_on_an_oversized_outline_and_informs_when_there_is_none() {
    let root = workspace_root();
    let fixture = root
        .join("fixtures")
        .join("designs")
        .join("gateway_estimate.synth");

    let payload = validate(&fixture, &root);
    let warning = board_estimate(&payload);
    assert_eq!(warning["severity"], "warning");
    let message = warning["message"].as_str().expect("message");
    assert!(message.contains("180.0 x 120.0"), "{message}");
    assert!(message.contains("remove the outline"), "{message}");
    assert!(warning["expected"].is_string());
    assert!(warning["found"].is_string());
    assert!(!warning["suggested_fixes"].as_array().unwrap().is_empty());

    let source = std::fs::read_to_string(&fixture)
        .expect("read fixture")
        .replace("  outline 180mm x 120mm\n", "");
    assert!(
        !source.contains("outline 180mm"),
        "fixture line not removed"
    );
    let scratch = std::env::temp_dir().join(format!("synth_board_estimate_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let no_outline = scratch.join("gateway_estimate.synth");
    std::fs::write(&no_outline, source).expect("write no-outline fixture");

    let payload = validate(&no_outline, &root);
    let info = board_estimate(&payload);
    assert_eq!(info["severity"], "info");
    assert!(
        info["found"]
            .as_str()
            .is_some_and(|f| f.contains("parts need"))
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A hard hint on a non-physical part must reach the CLI report, not just the
/// library check.
#[test]
fn validate_warns_on_a_hard_hint_that_is_not_physical() {
    let root = workspace_root();
    let scratch = std::env::temp_dir().join(format!("synth_hard_hint_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let source = scratch.join("hint.synth");
    std::fs::write(
        &source,
        "board \"hint\" {\n  layers 2\n  component U1: ic \"stm32g030k8_lqfp32\" {\n    placement_hint { region centre priority: hard }\n  }\n}\n",
    )
    .expect("write hint fixture");

    let payload = validate(&source, &root);
    let warning = payload["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .find(|d| d["code"] == "W-SYNTH-BOARD-005")
        .expect("a W-SYNTH-BOARD-005 diagnostic");
    assert_eq!(warning["severity"], "warning");
    assert!(
        warning["message"]
            .as_str()
            .is_some_and(|m| m.contains("soft")),
        "{warning}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// An info estimate must not hide the `ok:` line a clean design used to print.
#[test]
fn an_info_estimate_keeps_the_ok_line() {
    let root = workspace_root();
    let fixture = root
        .join("fixtures")
        .join("erc")
        .join("pass__analog_001_isolated.synth");
    let out = Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("validate")
        .arg(&fixture)
        .current_dir(&root)
        .output()
        .expect("run synth validate");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("ok: "), "{stdout}");
    assert!(stdout.contains("W-SYNTH-BOARD-004"), "{stdout}");
}
