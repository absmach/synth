// SPDX-License-Identifier: Apache-2.0

//! MCP Server JSON-RPC 2.0 stdio & SSE transport implementation.

use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use std::path::{Path, PathBuf};

use crate::tools::{call_tool, list_tools};

/// Standing guidance returned to every MCP client in the `initialize`
/// result (MCP spec: `instructions` is surfaced to the model by
/// compliant hosts). Encodes the ownership doctrine from
/// `docs/kicad-workflows.md` in four rules an agent must never
/// violate, plus the gate order and a pointer to the full knowledge
/// base.
pub const SERVER_INSTRUCTIONS: &str = "Synth compiles .synth design sources into deterministic KiCad projects. Standing rules: (1) .synth sources are the only editable design files; generated .kicad_sch/.kicad_sym/.kicad_pro/.kicad_pcb and bom.csv are build artifacts — never hand-edit them, a re-export will silently destroy the edit. (2) Visual placement tuning goes through <design>.synth.layout.toml (or drag-and-drop in synth preview), never through schematic coordinates. Before routing, call synth_place_with_hints with run_routing=false and inspect placement_quality.visual_review; if requires_revision is true, revise connector edge placement, orientation, breakout corridors, component grouping, or board compactness and rerun the review. This is a structural visual gate. (3) Schematic readability is reviewed on the rendered sheet, not inferred: call synth_render_schematic (or synth_review_schematic for a one-pass diagnostics+render packet) and actually look at the returned image. Fix what connectivity checks cannot — overlapping labels, confusing crossings, a component outside its group box, content past the page edge. Persist refinements with synth_mutate_layout persist=true (or synth_write_layout_override), never by editing schematic coordinates. Use synth_schematic_baseline to detect drift across revisions. (4) For difficult routing, inspect the unrouted net diagnostics and retry synth_route with routing_order listing the most constrained net names first; this is advisory and does not bypass DRC. (5) Part sourcing data (mpn/lcsc_pn) lives in registry part entries; the exporter stamps hidden MPN/LCSC fields onto every schematic instance — change parts at the source, never in BOM outputs. (6) A missing part is recoverable: search the registry, import or author a datasheet-backed Tier-2 part, confirm its pins and physical assets, then revalidate; never replace it with guessed pins or a different package silently. (7) Gate order before any handoff: synth_validate with zero blocking diagnostics, placement visual review, schematic render inspection, then synth_export, then 'kicad-cli sch erc' with zero errors. Export rejects unresolved compilation diagnostics; generated artifacts from failed attempts are not release evidence. Full review workflow, fabrication exports, Gerber review, panelization, and product-render guidance: docs/kicad-workflows.md.";

/// Promote any embedded `png_base64` payloads in a tool result into MCP
/// image content blocks, returning the pruned text value.
///
/// Recognises the shapes the schematic-review tools emit:
/// `synth_render_schematic`/`synth_review_schematic` put them under
/// `sheets[]` (and `render.sheets[]`), and `synth_schematic_baseline`
/// puts one at the top level for `inline_diff`. Promoting them means an
/// MCP host renders the sheet as an image instead of the model having to
/// decode base64 out of a JSON string.
fn extract_image_content(mut result: Value) -> (Value, Vec<String>) {
    fn take(map: &mut serde_json::Map<String, Value>, images: &mut Vec<String>) {
        if let Some(data) = map
            .remove("png_base64")
            .and_then(|v| v.as_str().map(str::to_string))
        {
            images.push(data);
        }
    }

    let mut images = Vec::new();

    if let Some(obj) = result.as_object_mut() {
        take(obj, &mut images);
    }
    if let Some(sheets) = result.get_mut("sheets").and_then(Value::as_array_mut) {
        for sheet in sheets {
            if let Some(obj) = sheet.as_object_mut() {
                take(obj, &mut images);
            }
        }
    }
    if let Some(sheets) = result
        .get_mut("render")
        .and_then(|r| r.get_mut("sheets"))
        .and_then(Value::as_array_mut)
    {
        for sheet in sheets {
            if let Some(obj) = sheet.as_object_mut() {
                take(obj, &mut images);
            }
        }
    }
    (result, images)
}

/// Handle an incoming MCP JSON-RPC request payload.
#[allow(clippy::needless_pass_by_value)]
pub fn handle_jsonrpc_request(req: Value, default_registry: Option<&Path>) -> Value {
    let jsonrpc = req["jsonrpc"].as_str().unwrap_or("2.0");
    let id = req["id"].clone();
    let method = req["method"].as_str().unwrap_or("");

    // JSON-RPC 2.0 Notification Rule: If `id` is null or missing, or if method is a notification,
    // NO response must be returned.
    if id.is_null() || method.starts_with("notifications/") {
        return Value::Null;
    }

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match method {
        "initialize" => serde_json::json!({
            "jsonrpc": jsonrpc,
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "synth-mcp",
                    "version": "0.1.0"
                },
                "instructions": SERVER_INSTRUCTIONS
            }
        }),
        "tools/list" => {
            let tools = list_tools();
            serde_json::json!({
                "jsonrpc": jsonrpc,
                "id": id,
                "result": {
                    "tools": tools
                }
            })
        }
        "tools/call" => {
            let params = &req["params"];
            let name = params["name"].as_str().unwrap_or("");
            let arguments = params["arguments"].clone();

            match call_tool(name, &arguments, default_registry) {
                Ok(result_val) => {
                    let (text_val, images) = extract_image_content(result_val);
                    let mut content = vec![serde_json::json!({
                        "type": "text",
                        "text": serde_json::to_string_pretty(&text_val).unwrap_or_default()
                    })];
                    for data in images {
                        content.push(serde_json::json!({
                            "type": "image",
                            "mimeType": "image/png",
                            "data": data
                        }));
                    }
                    serde_json::json!({
                        "jsonrpc": jsonrpc,
                        "id": id,
                        "result": { "content": content }
                    })
                }
                Err(err_msg) => serde_json::json!({
                    "jsonrpc": jsonrpc,
                    "id": id,
                    "error": {
                        "code": -32603,
                        "message": err_msg
                    }
                }),
            }
        }
        _ => serde_json::json!({
            "jsonrpc": jsonrpc,
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        }),
    }));

    match res {
        Ok(val) => val,
        Err(_) => serde_json::json!({
            "jsonrpc": jsonrpc,
            "id": id,
            "error": {
                "code": -32603,
                "message": "Internal MCP server panic caught"
            }
        }),
    }
}

/// Run MCP Server over standard I/O (stdin/stdout).
///
/// Uses plain newline-delimited JSON (one message per line), which is what
/// Antigravity, Claude Desktop, and Cursor all use for stdio MCP servers.
pub async fn run_stdio_server(default_registry: Option<PathBuf>) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req_val: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("synth-mcp: json parse error: {e}");
                let err_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let _ = stdout
                    .write_all(
                        serde_json::to_string(&err_resp)
                            .unwrap_or_default()
                            .as_bytes(),
                    )
                    .await;
                let _ = stdout.write_all(b"\n").await;
                let _ = stdout.flush().await;
                continue;
            }
        };

        let resp_val = handle_jsonrpc_request(req_val, default_registry.as_deref());
        // Null = notification, no response.
        if resp_val.is_null() {
            continue;
        }
        let _ = stdout
            .write_all(
                serde_json::to_string(&resp_val)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .await;
        let _ = stdout.write_all(b"\n").await;
        let _ = stdout.flush().await;
    }
    Ok(())
}

/// Handle HTTP JSON-RPC requests for SSE/HTTP endpoint.
pub async fn handle_http_rpc(
    axum::extract::State(default_registry): axum::extract::State<Option<PathBuf>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let resp = handle_jsonrpc_request(payload, default_registry.as_deref());
    Json(resp)
}

/// Run MCP Server over HTTP (SSE / REST transport).
pub async fn run_sse_server(port: u16, default_registry: Option<PathBuf>) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/rpc", post(handle_http_rpc))
        .route("/message", post(handle_http_rpc))
        .route("/sse", get(handle_http_rpc))
        .with_state(default_registry);

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("Synth MCP Server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
