// SPDX-License-Identifier: Apache-2.0

//! SYN-408: the two boards of the BLDC stack must agree through J_B2B.
//!
//! The good mapping reports every one of the 20 pins matched, each negative
//! variant is rejected, and the two placement sidecars put the connector and
//! the four mounting holes at the same board coordinates.

#![cfg(unix)]

use std::path::PathBuf;

use serde_json::{json, Value};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn board_dir() -> PathBuf {
    repo().join("qualification/class-d/bldc-stack")
}

fn registry() -> PathBuf {
    repo().join("registry/parts")
}

fn tool(args: &Value) -> Value {
    synth_mcp::call_tool("synth_export_multiboard", args, Some(&registry()))
        .expect("synth_export_multiboard runs")
}

/// Load a mapping file and make its board paths absolute, so the test does not
/// depend on the working directory Cargo happens to use.
fn mapping_file(name: &str) -> Value {
    let text = std::fs::read_to_string(board_dir().join(name)).expect("mapping file");
    let mut value: Value = serde_json::from_str(&text).expect("mapping JSON");
    for board in value["boards"].as_array_mut().expect("boards") {
        let relative = board["path"].as_str().expect("board path");
        board["path"] = json!(repo().join(relative).display().to_string());
    }
    value["registry_path"] = json!(registry().display().to_string());
    value["user_registry"] = json!("/tmp/synth-bldc-stack-no-user-registry");
    value
}

fn codes(result: &Value) -> Vec<&str> {
    result["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect()
}

#[test]
fn every_j_b2b_pin_is_matched_once() {
    let result = tool(&mapping_file("interboard_mappings.json"));

    assert_eq!(result["status"], "ok", "{result}");
    assert_eq!(result["total_mappings"], 20);
    assert_eq!(result["matched_pins"], 20);
    assert_eq!(result["matching_percentage"], 100.0);
    assert!(codes(&result).is_empty(), "{result}");
}

#[test]
fn swapping_two_mapped_pins_is_rejected() {
    let result = tool(&mapping_file("mappings_neg_swap.json"));

    assert_eq!(result["status"], "warning");
    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-003"),
        "{result}"
    );
    assert!(result["matching_percentage"].as_f64().unwrap() < 100.0);
}

#[test]
fn a_dropped_mapping_is_rejected() {
    let result = tool(&mapping_file("mappings_neg_drop.json"));

    assert_eq!(result["status"], "warning");
    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-004"),
        "{result}"
    );
}

#[test]
fn a_12v_pin_mapped_to_a_3v3_pin_is_rejected() {
    let result = tool(&mapping_file("mappings_neg_voltage.json"));

    assert_eq!(result["status"], "warning");
    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-003")
            || codes(&result).contains(&"E-SYNTH-MULTIBOARD-005"),
        "{result}"
    );
}

#[test]
fn a_control_source_with_swapped_connector_pins_is_rejected() {
    let mut args = mapping_file("interboard_mappings.json");
    let boards = args["boards"].as_array_mut().expect("boards");
    boards[0]["path"] = json!(board_dir()
        .join("control_neg_swap.synth")
        .display()
        .to_string());

    let result = tool(&args);
    assert_eq!(result["status"], "warning");
    assert!(
        codes(&result).contains(&"E-SYNTH-MULTIBOARD-003"),
        "{result}"
    );
}

/// 0.1 mm is the acceptance limit for the mechanical alignment.
const MAX_MISMATCH_MM: f64 = 0.1;

#[test]
fn the_connector_and_mounting_holes_line_up() {
    let control = synth_layout::sidecar::SidecarLayout::load_from_file(
        &board_dir().join("control.placement.layout.toml"),
    )
    .expect("control placement sidecar");
    let power = synth_layout::sidecar::SidecarLayout::load_from_file(
        &board_dir().join("power.placement.layout.toml"),
    )
    .expect("power placement sidecar");

    for refdes in ["J_B2B", "H1", "H2", "H3", "H4"] {
        let a = control.components.get(refdes).expect("control placement");
        let b = power.components.get(refdes).expect("power placement");
        let dx = (a.x - b.x).abs();
        let dy = (a.y - b.y).abs();
        assert!(
            dx <= MAX_MISMATCH_MM && dy <= MAX_MISMATCH_MM,
            "{refdes} is at ({}, {}) on control and ({}, {}) on power ({dx:.3} mm, {dy:.3} mm apart)",
            a.x,
            a.y,
            b.x,
            b.y
        );
        assert_eq!(
            a.rotation, b.rotation,
            "{refdes} rotation differs between the boards"
        );
    }
}
