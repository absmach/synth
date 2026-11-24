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
