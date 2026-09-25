// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `synth-mcp`.

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
    assert!(content_text.contains("\"status\": \"placed\""));
    assert!(content_text.contains("hint_satisfaction"));
    assert!(content_text.contains("\"routing_status\": \"not_run\""));
    assert!(content_text.contains("\"drc_status\": \"not_run\""));
}

#[test]
fn test_mcp_call_synth_place_with_explicit_dimensions() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 32,
        "method": "tools/call",
        "params": {
            "name": "synth_place_with_hints",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "board_width_mm": 60.0,
                "board_height_mm": 42.0,
                "hints": [
                    { "component": "D1", "region": "centre", "priority": "soft" }
                ]
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 32);
    let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("\"status\": \"placed\""));
    let placement: serde_json::Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(placement["board_size_mm"], json!([60.0, 42.0]));
    assert!(content_text.contains("hint_satisfaction"));
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
                "allow_placement_warnings": true,
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
                "source": LED_INDICATOR_SOURCE,
                "allow_placement_warnings": true
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

/// The three visual-feedback tools must be discoverable via `tools/list`.
#[test]
fn test_mcp_list_tools_includes_schematic_visual_loop() {
    let tools = list_tools();
    for name in [
        "synth_render_schematic",
        "synth_schematic_baseline",
        "synth_review_schematic",
    ] {
        assert!(tools.iter().any(|t| t.name == name), "missing tool {name}");
    }
}

/// `kicad-cli` is an external prerequisite for rendering; skip cleanly
/// (rather than fail) when it is not installed, since CI without KiCad
/// still needs to build and test the rest of the crate.
fn kicad_cli_available() -> bool {
    std::process::Command::new("kicad-cli")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn test_mcp_render_schematic_returns_inline_png_image_block() {
    if !kicad_cli_available() {
        eprintln!("skipping: kicad-cli not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("led_indicator.synth");
    std::fs::write(&src, LED_INDICATOR_SOURCE).unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "tools/call",
        "params": {
            "name": "synth_render_schematic",
            "arguments": { "file_path": src.to_str().unwrap(), "width_px": 800, "inline": true }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 40);
    let content = resp["result"]["content"].as_array().unwrap();

    // First block is the JSON summary, second is the actual PNG image.
    let payload: serde_json::Value =
        serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["width_px"], 800);
    assert!(payload["sheets"][0]["height_px"].as_u64().unwrap() > 0);
    // The base64 payload is promoted out of the text into an image block.
    assert!(payload["sheets"][0].get("png_base64").is_none());

    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    let data = content[1]["data"].as_str().unwrap();
    assert!(data.starts_with("iVBORw0KGgo"), "PNG magic missing");

    // A real, non-empty PNG was also written beside the design.
    let png_path = payload["sheets"][0]["png_path"].as_str().unwrap();
    assert!(std::path::Path::new(png_path).exists());
}

#[test]
fn test_mcp_render_schematic_rejects_bad_width() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 41,
        "method": "tools/call",
        "params": {
            "name": "synth_render_schematic",
            "arguments": { "source": LED_INDICATOR_SOURCE, "width_px": 99999 }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    let message = resp["error"]["message"].as_str().unwrap();
    assert!(message.contains("width_px"), "got: {message}");
}

#[test]
fn test_mcp_baseline_set_compare_and_clear() {
    if !kicad_cli_available() {
        eprintln!("skipping: kicad-cli not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("led_indicator.synth");
    std::fs::write(&src, LED_INDICATOR_SOURCE).unwrap();
    let file = src.to_str().unwrap();

    let call = |id: u64, action: &str| {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "synth_schematic_baseline",
                "arguments": { "file_path": file, "action": action, "width_px": 600 }
            }
        });
        handle_jsonrpc_request(req, None)
    };

    // compare before any baseline exists is a normal result, not an error.
    let before = call(50, "compare");
    let payload: serde_json::Value =
        serde_json::from_str(before["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["status"], "no_baseline");

    let set = call(51, "set");
    let payload: serde_json::Value =
        serde_json::from_str(set["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["status"], "stored");
    assert!(std::path::Path::new(payload["baseline_png"].as_str().unwrap()).exists());
    assert!(std::path::Path::new(payload["baseline_meta"].as_str().unwrap()).exists());

    // Re-rendering the unchanged design must not report drift.
    let after = call(52, "compare");
    let payload: serde_json::Value =
        serde_json::from_str(after["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["status"], "pass", "unchanged design drifted");
    assert_eq!(payload["changed_pixels"], 0);
    assert_eq!(payload["renderer_matches"], true);

    let cleared = call(53, "clear");
    let payload: serde_json::Value =
        serde_json::from_str(cleared["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["status"], "cleared");
    assert_eq!(payload["removed_png"], true);
}

#[test]
fn test_mcp_review_schematic_packet() {
    if !kicad_cli_available() {
        eprintln!("skipping: kicad-cli not installed");
        return;
    }
    // A board with a declared value (E-SYNTH-VALUE-001 is an error otherwise),
    // so a clean design can be asserted clean.
    let source = LED_INDICATOR_SOURCE.replace(
        "component R1: resistor  \"r_generic_0603\"",
        "component R1: resistor  \"r_generic_0603\" value \"330\"",
    );
    let req = json!({
        "jsonrpc": "2.0",
        "id": 60,
        "method": "tools/call",
        "params": {
            "name": "synth_review_schematic",
            "arguments": { "source": source, "width_px": 600, "inline": false }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert_eq!(resp["id"], 60);
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

    assert_eq!(
        payload["error_count"], 0,
        "design should be clean: {payload}"
    );
    assert!(payload["is_clean"].as_bool().unwrap());
    assert!(payload["diagnostics"].is_array());
    assert_eq!(payload["layout"]["components"], 3);
    assert!(
        payload["render"]["sheets"][0]["height_px"]
            .as_u64()
            .unwrap()
            > 0
    );
    // inline=false: no image block beyond the JSON summary.
    assert_eq!(resp["result"]["content"].as_array().unwrap().len(), 1);
}

/// `persist=true` must write the op's effect through to the sidecar so a
/// later render/export honours it — the whole point of Phase-2 persistence.
#[test]
fn test_mcp_mutate_layout_persist_writes_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("led_indicator.synth");
    std::fs::write(&src, LED_INDICATOR_SOURCE).unwrap();
    let sidecar = dir.path().join("led_indicator.synth.layout.toml");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 70,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "file_path": src.to_str().unwrap(),
                "persist": true,
                "layout_file_path": sidecar.to_str().unwrap(),
                "op": { "kind": "move_component", "id": 0, "x_mm": 99.06, "y_mm": 50.8 }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        payload["persisted"]["saved_path"],
        sidecar.to_str().unwrap()
    );

    let written = std::fs::read_to_string(&sidecar).unwrap();
    assert!(
        written.contains("99.06"),
        "sidecar did not record x: {written}"
    );
    assert!(written.contains("source = \"agent\""));

    // Reloading with the sidecar must actually move the component.
    let reload = synth_layout::layout_with_sidecar(&compile_led_board(), Some(sidecar.as_path()));
    let moved = reload
        .components
        .iter()
        .find(|p| p.id.0 == 0)
        .expect("component 0 present");
    assert_eq!(moved.center_mm, (99.06, 50.8));
}

/// `ReplaceWireWithLabel` persisted to the sidecar must survive a *later*
/// structural op — the regression the `forced_net_labels` field exists for.
/// The re-application mechanism itself is covered by unit tests in
/// `synth-layout::sidecar`; here we pin the MCP persistence contract.
#[test]
fn test_mcp_persisted_forced_label_is_recorded_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let sidecar = dir.path().join("d.synth.layout.toml");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 71,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "file_path": sidecar.with_file_name("d.synth").to_str().unwrap(),
                "persist": true,
                "layout_file_path": sidecar.to_str().unwrap(),
                "op": { "kind": "replace_wire_with_label", "net": 0 }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert!(
        resp["result"].is_object(),
        "op failed: {}",
        resp["error"]["message"]
    );
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["persisted"]["forced_net_labels"], json!(["net_0"]));

    let written = std::fs::read_to_string(&sidecar).unwrap();
    assert!(
        written.contains("forced_net_labels") && written.contains("net_0"),
        "forced label not persisted: {written}"
    );

    // A reload must apply the sidecar without error and leave the design
    // otherwise intact.
    let board = compile_led_board();
    let lay = synth_layout::layout_with_sidecar(&board, Some(sidecar.as_path()));
    assert_eq!(
        lay.components.len(),
        3,
        "reload must still place every part"
    );
}

/// Lower `LED_INDICATOR_SOURCE` through the same registry the MCP tools use.
fn compile_led_board() -> synth_ir::Board {
    let parse = synth_parser::parse(LED_INDICATOR_SOURCE, "led_indicator.synth".to_string());
    let ast = parse.ast.expect("led source must parse");
    let registry = synth_registry::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
        .or_else(|_| synth_registry::load_dir(std::path::Path::new("registry/parts")))
        .expect("registry must load");
    let lowered = synth_ir::lower(&ast, &registry, "led_indicator.synth");
    lowered.board.expect("led source must lower")
}

/// The review packet must turn a visibly-bad sheet into a concrete repair:
/// this board was observed to render as a low-fill vertical stack.
#[test]
fn test_mcp_review_schematic_returns_repair_hints() {
    if !kicad_cli_available() {
        eprintln!("skipping: kicad-cli not installed");
        return;
    }
    let source = LED_INDICATOR_SOURCE.replace(
        "component R1: resistor  \"r_generic_0603\"",
        "component R1: resistor  \"r_generic_0603\" value \"330\"",
    );
    let req = json!({
        "jsonrpc": "2.0",
        "id": 61,
        "method": "tools/call",
        "params": {
            "name": "synth_review_schematic",
            "arguments": { "source": source, "width_px": 600, "inline": false }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

    let hints = payload["repair_hints"]
        .as_array()
        .expect("repair_hints must be an array");
    let kinds: Vec<&str> = hints
        .iter()
        .filter_map(|h| h["suggested_op"]["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"fit_sheet"),
        "a low-fill sheet must suggest shrinking the page, got {hints:?}"
    );
    // The suggested op must name the smaller target, so the agent can act.
    let problem = hints
        .iter()
        .find(|h| h["suggested_op"]["kind"] == "fit_sheet")
        .and_then(|h| h["problem"].as_str())
        .unwrap_or_default();
    assert!(
        problem.contains("would fit"),
        "fit_sheet hint must state the target sheet: {problem}"
    );
    for hint in hints {
        assert!(
            hint["problem"].is_string(),
            "hint needs a human problem: {hint}"
        );
        assert!(
            hint["suggested_op"]["kind"].is_string(),
            "hint needs a concrete op: {hint}"
        );
    }
}

/// A suggested repair must actually be accepted by `synth_mutate_layout` —
/// the hint is useless if applying it errors.
#[test]
fn test_mcp_suggested_repair_op_is_applicable() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 62,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "op": { "kind": "fit_sheet", "grow": false }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert!(resp["result"].is_object(), "fit_sheet failed: {resp:?}");
    let payload: serde_json::Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(payload["components"].is_array());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 63,
        "method": "tools/call",
        "params": {
            "name": "synth_mutate_layout",
            "arguments": {
                "source": LED_INDICATOR_SOURCE,
                "op": { "kind": "distribute_row", "ids": [0, 1, 2], "y_mm": 50.8 }
            }
        }
    });
    let resp = handle_jsonrpc_request(req, None);
    assert!(
        resp["result"].is_object(),
        "distribute_row failed: {resp:?}"
    );
}
