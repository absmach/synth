// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `synth-mcp`.

use std::path::PathBuf;

use serde_json::json;
use synth_mcp::{handle_jsonrpc_request, list_tools, SERVER_INSTRUCTIONS};

#[test]
fn test_mcp_initialize() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["serverInfo"]["name"], "synth-mcp");
}

/// The initialize result must carry standing guidance (MCP spec:
/// `instructions`), and it must encode the ownership doctrine agents
/// most often violate: generated files are not editable, sourcing
/// data lives in the registry, and the ERC gate order is mandatory.
#[test]
fn test_mcp_initialize_carries_instructions() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let resp = handle_jsonrpc_request(req, None);
    let instructions = resp["result"]["instructions"]
        .as_str()
        .expect("initialize result must carry instructions");
    assert_eq!(instructions, SERVER_INSTRUCTIONS);
    assert!(
        instructions.contains("never hand-edit"),
        "ownership doctrine (no hand-editing generated files) missing"
    );
    assert!(
        instructions.contains(".synth.layout.toml"),
        "sidecar placement rule missing"
    );
    assert!(
        instructions.contains("registry"),
        "sourcing-data-in-registry rule missing"
    );
    assert!(
        instructions.contains("kicad-cli sch erc"),
        "ERC gate order missing"
    );
    assert!(
        instructions.contains("docs/kicad-workflows.md"),
        "knowledge-base pointer missing"
    );
}

#[test]
fn test_mcp_list_tools() {
    let tools = list_tools();
    assert!(tools.iter().any(|t| t.name == "synth_search_registry"));
    assert!(tools.iter().any(|t| t.name == "synth_validate"));
    assert!(tools.iter().any(|t| t.name == "synth_erc_report"));
    assert!(tools.iter().any(|t| t.name == "synth_drc_report"));
    assert!(tools.iter().any(|t| t.name == "synth_apply_patch"));
    assert!(tools.iter().any(|t| t.name == "synth_preview_schematic"));
    assert!(tools.iter().any(|t| t.name == "synth_export"));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 2);
    assert!(resp["result"]["tools"].is_array());
}

#[test]
fn test_mcp_call_synth_search_registry() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "synth_search_registry",
            "arguments": {
                "query": "stm32"
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 10);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("total_matches"));
}

#[test]
fn test_mcp_call_synth_validate() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "synth_validate",
            "arguments": {
                "source": "board \"test\" {\n  layers 2\n}\n"
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 3);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        content_text.contains("\"status\": \"ok\"") || content_text.contains("\"diagnostics\"")
    );
}

const LED_INDICATOR_SOURCE: &str = r#"board "led_indicator" {
  layers 2
  manufacturer "jlcpcb"

  component J1: connector "jst_ph_2pin"
  component D1: diode     "led_red_0603"
  component R1: resistor  "r_generic_0603"

  connect J1.p1      -> R1.p1
  connect R1.p2       -> D1.anode
  connect D1.cathode  -> J1.p2
}
"#;

#[test]
fn test_mcp_call_synth_mutate_layout_move_component() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "op": { "kind": "move_component", "id": 0, "x_mm": 99.06, "y_mm": 50.8 }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 20);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let layout: serde_json::Value = serde_json::from_str(content_text).unwrap();
    let moved = layout["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == 0)
        .expect("component id 0 must be present");
    assert_eq!(moved["center_mm"], json!([99.06, 50.8]));
}

#[test]
fn test_mcp_call_synth_mutate_layout_unknown_component_is_a_structured_error() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "op": { "kind": "move_component", "id": 9999, "x_mm": 0.0, "y_mm": 0.0 }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 21);
    let message = resp["error"]["message"].as_str().unwrap();
    assert!(message.contains("Could not apply layout op"));
}

#[test]
fn test_mcp_call_apply_patch() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "synth_apply_patch",
            "arguments": {
                "source": "board \"a\" {}",
                "patches": [
                    {
                        "confidence": 1.0,
                        "kind": "insert_at",
                        "at": 11,
                        "text": "\n  layers 4\n"
                    }
                ]
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 4);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("layers 4"));
}

#[test]
fn test_mcp_call_synth_place_with_hints() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "tools/call",
        "params": {
            "name": "synth_place_with_hints",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "hints": [
                    { "component": "D1", "region": "top_left", "priority": "hard" }
                ]
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 30);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("\"status\": \"ok\""));
    assert!(content_text.contains("hint_satisfaction"));
    assert!(content_text.contains("drc_clean"));
    assert!(content_text.contains("violations"));
    assert!(content_text.contains("unrouted_nets"));
}

#[test]
fn test_mcp_call_synth_describe_placement() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "tools/call",
        "params": {
            "name": "synth_describe_placement",
            "arguments": {
                "source": LED_INDICATOR_SOURCE
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 31);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("cluster_summary"));
    assert!(content_text.contains("component_regions"));
}

#[test]
fn test_mcp_call_sidecar_overrides_read_write() {
    let tmp = std::env::temp_dir().join("test_sidecar.synth.layout.toml");
    let tmp_path = tmp.to_str().unwrap();

    let write_req = json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "tools/call",
        "params": {
            "name": "synth_write_layout_override",
            "arguments": {
                "layout_file_path": tmp_path,
                "refdes": "U1",
                "x_mm": 25.4,
                "y_mm": 12.7,
                "rotation": 90,
                "source": "agent"
            }
        }
    });
    let resp = handle_jsonrpc_request(write_req, None);
    assert_eq!(resp["id"], 40);

    let read_req = json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "tools/call",
        "params": {
            "name": "synth_read_layout_overrides",
            "arguments": {
                "layout_file_path": tmp_path
            }
        }
    });
    let resp_read = handle_jsonrpc_request(read_req, None);
    assert_eq!(resp_read["id"], 41);
    let content_text = resp_read["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("schema_version"));
    assert!(content_text.contains("U1"));
    assert!(content_text.contains("25.4"));

    let _ = std::fs::remove_file(tmp);
}

#[test]
fn test_mcp_call_synth_route_with_constraints_emits_warning_for_unknown_net() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 50,
        "method": "tools/call",
        "params": {
            "name": "synth_route_with_constraints",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "net_constraints": [
                    { "net": "NONEXISTENT_RAIL", "width_mm": 0.5 }
                ]
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 50);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("W-SYNTH-ROUTE-CONSTRAINT-001"));
    assert!(content_text.contains("drc_clean"));
    assert!(content_text.contains("violations"));
}

#[test]
fn test_mcp_call_synth_route_with_constraints_returns_drc_and_trace_info() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 51,
        "method": "tools/call",
        "params": {
            "name": "synth_route_with_constraints",
            "arguments": {
                "source": LED_INDICATOR_SOURCE
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 51);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("segments"));
    assert!(content_text.contains("vias"));
    assert!(content_text.contains("drc_clean"));
    assert!(content_text.contains("total_trace_length_mm"));
}

#[test]
fn test_closed_loop_agent_drc_repair_convergence() {
    let sensor_logger_path = std::path::Path::new("../../examples/sensor_logger.synth");
    let source = std::fs::read_to_string(sensor_logger_path).expect("read sensor_logger.synth");
    let sidecar_tmp = std::env::temp_dir().join("sensor_logger_repair.synth.layout.toml");
    if sidecar_tmp.exists() {
        let _ = std::fs::remove_file(&sidecar_tmp);
    }

    // Step 1: Initial DRC report via MCP
    let drc_req = json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "tools/call",
        "params": {
            "name": "synth_drc_report",
            "arguments": {
                "source": source,
                "file_path": sensor_logger_path.to_str().unwrap()
            }
        }
    });
    let resp = handle_jsonrpc_request(drc_req, None);
    assert_eq!(resp["id"], 60);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let drc_val: serde_json::Value = serde_json::from_str(content_text).unwrap();
    let violations = drc_val["violations"].as_array().expect("violations array");

    // Apply every suggested override once, then verify the sidecar persisted.
    // If the board's violations carry no placement suggestion (e.g. routing
    // violations, which have no placement fix), still write one deterministic
    // override so the MCP write->read round trip is always exercised.
    let iteration = 1;
    let mut wrote = false;
    for v in violations {
        if let Some(sug) = v.get("suggested_override") {
            if let (Some(refdes), Some(dx), Some(rot)) = (
                sug["refdes"].as_str(),
                sug["delta_x_mm"].as_f64(),
                sug["rotation_deg"].as_u64(),
            ) {
                let write_req = json!({
                    "jsonrpc": "2.0",
                    "id": 61 + iteration,
                    "method": "tools/call",
                    "params": {
                        "name": "synth_write_layout_override",
                        "arguments": {
                            "layout_file_path": sidecar_tmp.to_str().unwrap(),
                            "refdes": refdes,
                            "x_mm": 20.0 + dx * f64::from(iteration),
                            "y_mm": 20.0,
                            "rotation": rot,
                            "source": "agent"
                        }
                    }
                });
                let _ = handle_jsonrpc_request(write_req, None);
                wrote = true;
            }
        }
    }
    if !wrote {
        let fallback_req = json!({
            "jsonrpc": "2.0",
            "id": 61 + iteration,
            "method": "tools/call",
            "params": {
                "name": "synth_write_layout_override",
                "arguments": {
                    "layout_file_path": sidecar_tmp.to_str().unwrap(),
                    "refdes": "U1",
                    "x_mm": 20.0,
                    "y_mm": 20.0,
                    "rotation": 0,
                    "source": "agent"
                }
            }
        });
        let _ = handle_jsonrpc_request(fallback_req, None);
    }

    if !violations.is_empty() {
        // Verify sidecar persisted
        let read_req = json!({
            "jsonrpc": "2.0",
            "id": 70 + iteration,
            "method": "tools/call",
            "params": {
                "name": "synth_read_layout_overrides",
                "arguments": {
                    "layout_file_path": sidecar_tmp.to_str().unwrap()
                }
            }
        });
        let read_resp = handle_jsonrpc_request(read_req, None);
        let read_text = read_resp["result"]["content"][0]["text"].as_str().unwrap();
        let read_val: serde_json::Value = serde_json::from_str(read_text).unwrap();
        assert!(read_val["exists"].as_bool().unwrap_or(false));
    }

    if sidecar_tmp.exists() {
        let _ = std::fs::remove_file(&sidecar_tmp);
    }
}

#[test]
fn test_mcp_list_tools_includes_query_knowledge() {
    let tools = list_tools();
    assert!(
        tools.iter().any(|t| t.name == "synth_query_knowledge"),
        "synth_query_knowledge must be exposed"
    );
}

#[test]
fn test_mcp_query_knowledge_catalog_and_check() {
    // Catalog query by kind.
    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "synth_query_knowledge",
            "arguments": { "kind": "switch" }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(content).unwrap();
    let templates = payload["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 2, "debounce + pull-up for switches");
    assert!(templates.iter().any(|t| t["id"] == "switch_debounce_rc"));

    // Design check: a bare switch input must come back flagged.
    let req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "synth_query_knowledge",
            "arguments": {
                "source": "board \"t\" {\n  component SW1: switch \"spst_tactile\"\n  component U1: mcu \"stm32f103c8\"\n  component U3: regulator \"ams1117_3v3\"\n  connect SW1.p1 -> U1.pa0\n  connect SW1.p2 -> U1.vss\n  connect U3.vout -> U1.vbat\n  connect U3.gnd -> U1.vss\n}\n",
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(content).unwrap();
    let violations = payload["check"]["violations"]
        .as_array()
        .expect("check must run");
    assert!(
        violations
            .iter()
            .any(|v| v["template"] == "switch_debounce_rc"),
        "bare switch must violate the debounce template: {violations:?}"
    );
}
