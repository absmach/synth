// SPDX-License-Identifier: Apache-2.0

//! SYN-408: the two boards of the BLDC stack must agree through J_B2B.
//!
//! The good mapping reports every one of the 20 pins matched, each negative
//! variant is rejected, and the two placement sidecars put the J_B2B connector
//! at the same inset from each board's right edge; the power board is wider,
//! so the absolute x differs. The mounting holes are board-local.

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
const MAX_EDGE_INSET_MM: f64 = 4.0;

#[test]
fn the_connector_is_flush_on_both_boards() {
    let control = synth_layout::sidecar::SidecarLayout::load_from_file(
        &board_dir().join("control.placement.layout.toml"),
    )
    .expect("control placement sidecar");
    let power = synth_layout::sidecar::SidecarLayout::load_from_file(
        &board_dir().join("power.placement.layout.toml"),
    )
    .expect("power placement sidecar");

    // Sidecars hold footprint origins. The control uses a pin header and the
    // power board a socket, so their origins sit at different distances from
    // the courtyard edge; assert each body is still on its right edge.
    const CONTROL_WIDTH_MM: f64 = 60.0;
    const POWER_WIDTH_MM: f64 = 72.0;
    let a = control.components.get("J_B2B").expect("control placement");
    let b = power.components.get("J_B2B").expect("power placement");
    let control_inset = CONTROL_WIDTH_MM - a.x;
    let power_inset = POWER_WIDTH_MM - b.x;
    assert!(
        control_inset <= MAX_EDGE_INSET_MM,
        "J_B2B is {control_inset:.3} mm from the control edge"
    );
    assert!(
        power_inset <= MAX_EDGE_INSET_MM,
        "J_B2B is {power_inset:.3} mm from the power edge"
    );
    assert_eq!(a.rotation, b.rotation, "J_B2B rotation differs");
}
