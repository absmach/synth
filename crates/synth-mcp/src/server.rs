// SPDX-License-Identifier: Apache-2.0

//! MCP Server JSON-RPC 2.0 stdio & SSE transport implementation.

use axum::{
    Json, Router,
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{info, warn};

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::tools::{call_tool, list_tools};

/// Standing guidance returned to every MCP client in the `initialize`
/// result (MCP spec: `instructions` is surfaced to the model by
/// compliant hosts). Encodes the ownership doctrine from
/// `docs/kicad-workflows.md` in the rules an agent must never violate,
/// plus the gate order and a pointer to the full knowledge base.
pub const SERVER_INSTRUCTIONS: &str = "Synth compiles .synth design sources into deterministic KiCad projects. Standing rules: (1) .synth sources are the only editable design files; generated .kicad_sch/.kicad_sym/.kicad_pro/.kicad_pcb and bom.csv are build artifacts — never hand-edit them, a re-export will silently destroy the edit. (2) Layout tuning lives in TWO sidecars with the same schema but different coordinate spaces, and they must not be mixed: <design>.schematic.layout.toml holds sheet millimetres (where a part sits on the page; written by synth_mutate_layout persist=true) and <design>.placement.layout.toml holds board millimetres (where a footprint sits on the PCB; written by synth_write_layout_override / synth_place_with_hints). Passing one where the other is expected silently misplaces parts. Tune visually through these files, never through schematic coordinates. (7) A part whose kicad_footprint names a footprint that is not installed is a broken reference, not a missing part: call synth_resolve_footprints to find the real one in the local KiCad library, and synth_export applies a confident, unambiguous match automatically. Never invent pin numbers to match a footprint — a ball-grid package uses designators like A22/AC13, and E-SYNTH-PIN-001 (blocking) is what catches fictional ones. Fix those from the datasheet. Before routing, call synth_place_with_hints with run_routing=false and inspect placement_quality.visual_review; if requires_revision is true, revise connector edge placement, orientation, breakout corridors, component grouping, or board compactness and rerun the review. This is a structural visual gate. (3) Schematic readability is reviewed on the rendered sheet, not inferred: call synth_render_schematic (or synth_review_schematic for a one-pass diagnostics+render packet) and actually look at the returned image. Fix what connectivity checks cannot — overlapping labels, confusing crossings, a component outside its group box, content past the page edge. Persist refinements with synth_mutate_layout persist=true (or synth_write_layout_override), never by editing schematic coordinates. Use synth_schematic_baseline to detect drift across revisions. (4) For difficult routing, inspect the unrouted net diagnostics and the report's `recommended_routing_order`; the external engine chooses its own net order, so treat that list as output feedback, not an input. Use `--place-route-iterate` (synth_export's `place_route_iterate`) to let the loop group the hardest nets' parts. (5) Part sourcing data (mpn/lcsc_pn) lives in registry part entries; the exporter stamps hidden MPN/LCSC fields onto every schematic instance — change parts at the source, never in BOM outputs. (6) A missing part is recoverable: search the registry, import or author a datasheet-backed Tier-2 part, confirm its pins and physical assets, then revalidate; never replace it with guessed pins or a different package silently. (8) Gate order before any handoff: synth_validate with zero blocking diagnostics, placement visual review, schematic render inspection, then synth_export, then 'kicad-cli sch erc' with zero errors. Export rejects unresolved compilation diagnostics; generated artifacts from failed attempts are not release evidence. (9) Never declare `outline <width> x <height>` unless the user gave the size or a mating constraint; without one, placement fits the board to its parts, and an arbitrary outline leaves dead board that `synth validate` reports (W-SYNTH-BOARD-004). (10) Keep `priority: hard` for physical constraints only — a connector edge or a mounting hole. A hard hint on anything else blocks compaction; make it soft unless the part must stay where it is. Full review workflow, fabrication exports, Gerber review, panelization, and product-render guidance: docs/kicad-workflows.md.";

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
                    "version": "0.1.4"
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

/// Writes one message as a single locked write, so a response larger than a pipe
/// buffer cannot be split by a heartbeat line from another thread.
fn write_message(message: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(format!("{message}\n").as_bytes());
    let _ = out.flush();
}

/// How often a running tool tells the client it is still working.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Sends an MCP progress notification at a fixed interval until dropped.
///
/// A tool such as `synth_export` can run for many minutes inside the router and
/// returns nothing until it is done. A client that times a call out after a
/// period without a response or progress, as Claude Code does after 30 minutes,
/// cannot tell that from a hang, and aborts a healthy export.
struct Heartbeat {
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Heartbeat {
    fn start<F>(token: Value, interval: Duration, send: F) -> Self
    where
        F: Fn(String) + Send + 'static,
    {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let shared = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let (flag, wake) = &*shared;
            let mut beats: u64 = 0;
            loop {
                let guard = flag
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let (guard, _) = wake
                    .wait_timeout_while(guard, interval, |stopped| !*stopped)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if *guard {
                    break;
                }
                drop(guard);
                beats += 1;
                send(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/progress",
                        "params": {
                            "progressToken": token,
                            "progress": beats,
                            "message": "synth is still working"
                        }
                    })
                    .to_string(),
                );
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        let (flag, wake) = &*self.stop;
        *flag
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The token the client supplied in `params._meta.progressToken`, exactly as
/// given. MCP only allows progress notifications under a client token, so a
/// request without one gets no heartbeat. The protocol allows a string or any
/// number, so `3.0` and `1.5` are tokens too.
fn progress_token(req: &Value) -> Option<Value> {
    let token = &req["params"]["_meta"]["progressToken"];
    (token.is_string() || token.is_number()).then(|| token.clone())
}

/// Calls in flight, by request id, each with the flag `notifications/cancelled` sets.
type InFlight = Arc<Mutex<HashMap<String, synth_router::process::CancelToken>>>;

/// Where a running call's progress notifications go. stdio writes the line to
/// stdout; HTTP sends it into that call's response stream.
type Progress = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// How a finished call's response is delivered. Each call gets its own, so a
/// reply belongs to the request that asked for it: stdio writes the response
/// line from the worker, HTTP sends the JSON text into that call's response
/// stream or back to its awaiting handler.
type Reply = Box<dyn FnOnce(Value) + Send + 'static>;

/// A `tools/call` waiting for, or holding, the tool worker.
struct Job {
    req: Value,
    key: String,
    cancelled: synth_router::process::CancelToken,
    /// Alive from the moment the call arrives, so a call queued behind a long
    /// export still tells its own client it is waiting.
    heartbeat: Option<Heartbeat>,
    reply: Reply,
}

/// Key an in-flight call by connection and JSON-RPC id, so ids from two
/// HTTP clients do not collide.
fn in_flight_key(connection: &str, request_id: &str) -> String {
    format!("{connection}\u{1f}{request_id}")
}

/// The one tool worker both transports share, plus the in-flight table.
///
/// Tool calls run one at a time, in arrival order, on a single worker thread:
/// they write the same source files, sidecars and output directories and read
/// process-wide settings, so two at once could overwrite each other. The
/// handler that enqueued a call, and its transport's event loop, stay free,
/// so `notifications/cancelled` and `ping` are seen while an export runs. A
/// cancelled call sends neither further progress nor a response, and one
/// cancelled before it starts never runs.
struct Dispatcher {
    queue: Option<std::sync::mpsc::Sender<Job>>,
    in_flight: InFlight,
    handle: Arc<dyn Fn(Value) -> Value + Send + Sync + 'static>,
    interval: Duration,
    /// The shared stdio writer. HTTP calls never use it: their replies go to
    /// their own response stream.
    write: Progress,
    worker: Option<JoinHandle<()>>,
}

impl Dispatcher {
    fn new(
        handle: Arc<dyn Fn(Value) -> Value + Send + Sync + 'static>,
        interval: Duration,
        write: Progress,
    ) -> Self {
        let in_flight: InFlight = Arc::new(Mutex::new(HashMap::new()));
        let (queue, jobs) = std::sync::mpsc::channel::<Job>();
        let (worker_handle, worker_in_flight) = (Arc::clone(&handle), Arc::clone(&in_flight));
        let worker = std::thread::spawn(move || {
            for job in jobs {
                // The call's token is the one every external router and KiCad run
                // inside it watches, so a cancellation ends them, not only the reply.
                synth_router::process::set_ambient_cancel(Some(job.cancelled.clone()));
                let resp_val = if job.cancelled.is_cancelled() {
                    Value::Null
                } else {
                    worker_handle(job.req)
                };
                synth_router::process::set_ambient_cancel(None);
                drop(job.heartbeat);
                worker_in_flight
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&job.key);
                (job.reply)(resp_val);
            }
        });
        Self {
            queue: Some(queue),
            in_flight,
            handle,
            interval,
            write,
            worker: Some(worker),
        }
    }

    /// Runs a request on the calling task: the fast methods (`initialize`,
    /// `tools/list`, `ping`) that must answer even while an export runs.
    fn dispatch(&self, req: Value) -> Value {
        (self.handle)(req)
    }

    /// Cancels the call with this connection and JSON-RPC request id.
    fn cancel(&self, connection: &str, request_id: &str) {
        let key = in_flight_key(connection, request_id);
        let flag = self
            .in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned();
        if let Some(flag) = flag {
            flag.cancel();
        }
    }

    /// Enqueues a `tools/call`: registers its id for `notifications/cancelled`,
    /// starts its progress heartbeat against `progress` (a transport with no
    /// place for notifications passes none), and hands it to the worker with
    /// `reply` for the response.
    fn submit(
        &self,
        req: Value,
        connection: &str,
        cancelled: synth_router::process::CancelToken,
        progress: Option<Progress>,
        reply: Reply,
    ) {
        let key = in_flight_key(connection, &req["id"].to_string());
        self.in_flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.clone(), cancelled.clone());
        let heartbeat_cancelled = cancelled.clone();
        let heartbeat = progress_token(&req).zip(progress).map(|(token, progress)| {
            Heartbeat::start(token, self.interval, move |message| {
                if !heartbeat_cancelled.is_cancelled() {
                    progress(&message);
                }
            })
        });
        let _ = self
            .queue
            .as_ref()
            .expect("worker outlives its queue")
            .send(Job {
                req,
                key,
                cancelled,
                heartbeat,
                reply,
            });
    }

    /// A stdio call: its progress and its response line go to the shared writer.
    fn submit_stdio(&self, req: Value) {
        let cancelled = synth_router::process::CancelToken::new();
        let (write, flag) = (Arc::clone(&self.write), cancelled.clone());
        let reply: Reply = Box::new(move |resp_val| {
            if !flag.is_cancelled() && !resp_val.is_null() {
                write(&serde_json::to_string(&resp_val).unwrap_or_default());
            }
        });
        self.submit(
            req,
            "stdio",
            cancelled,
            Some(Arc::clone(&self.write)),
            reply,
        );
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        // Close the queue so the worker drains the jobs already sent, answers
        // each one, and exits.
        self.queue = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Answers a stream of request lines.
///
/// The stdio transport delegates to the shared [`Dispatcher`]: tool calls
/// queue onto its worker, everything else answers on the read loop.
async fn serve<R, W, H>(
    mut reader: tokio::io::Lines<R>,
    handle: Arc<H>,
    interval: Duration,
    write: Arc<W>,
) where
    R: tokio::io::AsyncBufRead + Unpin,
    W: Fn(&str) + Send + Sync + 'static,
    H: Fn(Value) -> Value + Send + Sync + 'static,
{
    let write: Progress = write;
    let dispatcher = Dispatcher::new(
        Arc::new(move |req: Value| handle(req)),
        interval,
        Arc::clone(&write),
    );

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req_val: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                warn!("synth-mcp: json parse error: {e}");
                let err_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                write(&serde_json::to_string(&err_resp).unwrap_or_default());
                continue;
            }
        };

        if req_val["method"] == "notifications/cancelled" {
            dispatcher.cancel("stdio", &req_val["params"]["requestId"].to_string());
            continue;
        }

        if req_val["method"] != "tools/call" || req_val["id"].is_null() {
            let resp_val = dispatcher.dispatch(req_val);
            // Null = notification, no response.
            if !resp_val.is_null() {
                write(&serde_json::to_string(&resp_val).unwrap_or_default());
            }
            continue;
        }

        dispatcher.submit_stdio(req_val);
    }
    // Dropping the dispatcher closes the queue and joins the worker, so every
    // call still queued is answered before the server exits.
}

/// Run MCP Server over standard I/O (stdin/stdout).
///
/// Uses plain newline-delimited JSON (one message per line), which is what
/// Antigravity, Claude Desktop, and Cursor all use for stdio MCP servers.
pub async fn run_stdio_server(default_registry: Option<PathBuf>) -> anyhow::Result<()> {
    let reader = BufReader::new(tokio::io::stdin()).lines();
    serve(
        reader,
        Arc::new(move |req| handle_jsonrpc_request(req, default_registry.as_deref())),
        HEARTBEAT_INTERVAL,
        Arc::new(|message: &str| write_message(message)),
    )
    .await;
    Ok(())
}

/// The JSON-RPC error the MCP spec prescribes for a request the client
/// cancelled. stdio answers a cancelled call with nothing at all; HTTP has to
/// return a response, so it answers with this instead.
fn cancelled_error(id: &Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32800, "message": "Request cancelled" }
    })
}

/// The response text a finished HTTP call delivers: the result, or the
/// cancellation error when the client cancelled the call (the worker answers a
/// cancelled call with a null result, and a cancel that lands after the tool
/// finished is one all the same).
fn http_reply(cancelled: &synth_router::process::CancelToken, id: &Value, resp: &Value) -> String {
    if cancelled.is_cancelled() || resp.is_null() {
        cancelled_error(id).to_string()
    } else {
        resp.to_string()
    }
}

/// The body of a streaming `tools/call` response: one SSE event per message
/// while the call runs, ending when the reply drops the channel's last sender.
struct CallStream(UnboundedReceiver<String>);

impl futures_core::Stream for CallStream {
    type Item = Result<Event, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.get_mut().0.poll_recv(cx) {
            std::task::Poll::Ready(Some(message)) => {
                std::task::Poll::Ready(Some(Ok(Event::default().data(message))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// Runs a `tools/call` that carried a `progressToken` and answers with an SSE
/// stream: progress events while the tool runs, the final JSON-RPC response as
/// the last event. The handler returns at once, so the connection stays open
/// while the worker runs the tool.
fn streaming_call(dispatcher: &Dispatcher, connection: &str, req: Value) -> Response {
    let id = req["id"].clone();
    let (events, receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let cancelled = synth_router::process::CancelToken::new();
    let reply_cancelled = cancelled.clone();
    let progress_events = events.clone();
    let progress: Progress = Arc::new(move |message: &str| {
        let _ = progress_events.send(message.to_string());
    });
    let reply: Reply = Box::new(move |resp_val| {
        let _ = events.send(http_reply(&reply_cancelled, &id, &resp_val));
    });
    dispatcher.submit(req, connection, cancelled, Some(progress), reply);
    Sse::new(CallStream(receiver)).into_response()
}

/// Runs a `tools/call` that carried no `progressToken` and answers with plain
/// JSON as before — but on the shared worker, so `notifications/cancelled`
/// reaches it. A cancelled call answers with the spec's cancellation error.
async fn plain_call(dispatcher: &Dispatcher, connection: &str, req: Value) -> Response {
    let id = req["id"].clone();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<Value>();
    let cancelled = synth_router::process::CancelToken::new();
    let (reply_id, reply_cancelled) = (id.clone(), cancelled.clone());
    let reply: Reply = Box::new(move |resp_val| {
        let _ = reply_tx.send(if reply_cancelled.is_cancelled() || resp_val.is_null() {
            cancelled_error(&reply_id)
        } else {
            resp_val
        });
    });
    dispatcher.submit(req, connection, cancelled, None, reply);
    let resp_val = reply_rx.await.unwrap_or_else(|_| cancelled_error(&id));
    Json(resp_val).into_response()
}

/// Handle HTTP JSON-RPC requests for SSE/HTTP endpoint.
///
/// Tool calls queue onto the same worker the stdio transport uses, one at a
/// time, so this handler and the HTTP event loop stay free: a cancellation
/// notification posted to any route is seen while an export runs. A call that
/// carried `_meta.progressToken` answers with an SSE stream of progress
/// notifications and, as the last event, the response; a call without one
/// answers with plain JSON as before.
#[allow(clippy::needless_pass_by_value)]
async fn handle_http_rpc(
    axum::extract::State(dispatcher): axum::extract::State<Arc<Dispatcher>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    // Requests on one client connection share a key, so two clients using the
    // same JSON-RPC id cannot cancel each other. A client that needs a stable
    // key behind a shared egress sets `x-mcp-connection-id`.
    let connection = headers
        .get("x-mcp-connection-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    // A cancellation notification: drop the in-flight token, send nothing back.
    if payload["method"] == "notifications/cancelled" {
        dispatcher.cancel(connection, &payload["params"]["requestId"].to_string());
        return Json(Value::Null).into_response();
    }

    if payload["method"] != "tools/call" || payload["id"].is_null() {
        return Json(dispatcher.dispatch(payload)).into_response();
    }

    if progress_token(&payload).is_some() {
        streaming_call(&dispatcher, connection, payload)
    } else {
        plain_call(&dispatcher, connection, payload).await
    }
}

/// The HTTP routes, wired to a dispatcher the way `run_sse_server` serves them.
fn sse_router(dispatcher: Arc<Dispatcher>) -> Router {
    Router::new()
        .route("/rpc", post(handle_http_rpc))
        .route("/message", post(handle_http_rpc))
        .route("/sse", get(handle_http_rpc))
        .with_state(dispatcher)
}

/// Run MCP Server over HTTP (SSE / REST transport).
pub async fn run_sse_server(port: u16, default_registry: Option<PathBuf>) -> anyhow::Result<()> {
    let dispatcher = Arc::new(Dispatcher::new(
        Arc::new(move |req| handle_jsonrpc_request(req, default_registry.as_deref())),
        HEARTBEAT_INTERVAL,
        // HTTP replies go to each call's own response stream; the stdio writer
        // is the stdio transport's, not this one's.
        Arc::new(|_: &str| {}),
    ));
    let app = sse_router(dispatcher);

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Synth MCP Server listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod heartbeat_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn a_running_call_sends_progress_until_it_returns() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&seen);
        let beat = Heartbeat::start(Value::from("t1"), Duration::from_millis(10), move |m| {
            sink.lock().unwrap().push(m);
        });
        std::thread::sleep(Duration::from_millis(80));
        drop(beat);
        let count = seen.lock().unwrap().len();
        assert!(count >= 2, "expected several beats, got {count}");
        let first: Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
        assert_eq!(first["method"], "notifications/progress");
        assert_eq!(first["params"]["progressToken"], "t1");
        assert_eq!(first["params"]["progress"], 1);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(
            seen.lock().unwrap().len(),
            count,
            "no beat after the call returned"
        );
    }

    #[test]
    fn the_token_is_the_one_the_client_gave_and_nothing_else() {
        let with = serde_json::json!({"id": 7, "params": {"_meta": {"progressToken": "abc"}}});
        assert_eq!(progress_token(&with), Some(Value::from("abc")));
        let numeric = serde_json::json!({"id": 7, "params": {"_meta": {"progressToken": 3}}});
        assert_eq!(progress_token(&numeric), Some(Value::from(3)));
        let without = serde_json::json!({"id": 7, "params": {}});
        assert_eq!(progress_token(&without), None);
        // A number that is not an integer is a token as well.
        for fractional in [1.5, 3.0] {
            let req =
                serde_json::json!({"id": 7, "params": {"_meta": {"progressToken": fractional}}});
            assert_eq!(progress_token(&req), Some(Value::from(fractional)));
        }
        // Only a string or a number is a token.
        for bogus in [
            serde_json::json!(true),
            serde_json::json!(null),
            serde_json::json!({"a": 1}),
            serde_json::json!([1]),
        ] {
            let req = serde_json::json!({"id": 7, "params": {"_meta": {"progressToken": bogus}}});
            assert_eq!(progress_token(&req), None, "{bogus}");
        }
    }

    fn run(input: &str, slow_ms: u64, interval_ms: u64) -> Vec<Value> {
        let out = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&out);
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(serve(
            BufReader::new(std::io::Cursor::new(input.as_bytes().to_vec())).lines(),
            Arc::new(move |req: Value| {
                if req["method"] == "tools/call" {
                    std::thread::sleep(Duration::from_millis(slow_ms));
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_millis(interval_ms),
            Arc::new(move |m: &str| sink.lock().unwrap().push(m.to_string())),
        ));
        let lines = out.lock().unwrap().clone();
        lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_call_with_a_token_gets_progress_and_a_response() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"t"}}}"#;
        let msgs = run(input, 120, 20);
        let beats = msgs
            .iter()
            .filter(|m| m["method"] == "notifications/progress")
            .count();
        assert!(beats >= 2, "expected heartbeats, got {beats}");
        assert!(msgs.iter().all(
            |m| m["method"] != "notifications/progress" || m["params"]["progressToken"] == "t"
        ));
        assert_eq!(msgs.last().unwrap()["id"], 1);
    }

    #[test]
    fn a_fractional_numeric_token_is_echoed_in_the_progress_notifications() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":1.5}}}"#;
        let msgs = run(input, 120, 20);
        let beats: Vec<&Value> = msgs
            .iter()
            .filter(|m| m["method"] == "notifications/progress")
            .collect();
        assert!(beats.len() >= 2, "expected heartbeats, got {}", beats.len());
        assert!(
            beats.iter().all(|m| m["params"]["progressToken"] == 1.5),
            "{beats:?}"
        );
        assert_eq!(msgs.last().unwrap()["id"], 1);
    }

    #[test]
    fn a_call_without_a_token_sends_no_progress() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#;
        let msgs = run(input, 80, 10);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["id"], 1);
    }

    #[test]
    fn cancelling_one_connection_does_not_stop_another_with_the_same_id() {
        // Two HTTP clients both use JSON-RPC id 1. A cancellation keyed only
        // by the id would let one client stop the other's call.
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let dispatcher = Dispatcher::new(
            Arc::new(move |_req| {
                let _ = wait.lock().unwrap().recv();
                Value::Null
            }),
            Duration::from_millis(10),
            Arc::new(|_: &str| {}),
        );
        let token = synth_router::process::CancelToken::new();
        let watched = token.clone();
        let reply: Reply = Box::new(|_| {});
        dispatcher.submit(
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call"}),
            "client-a",
            token,
            None,
            reply,
        );

        dispatcher.cancel("client-b", "1");
        assert!(
            !watched.is_cancelled(),
            "another connection's id must not cancel this call"
        );
        dispatcher.cancel("client-a", "1");
        assert!(
            watched.is_cancelled(),
            "the owning connection cancels its own call"
        );

        drop(release);
        drop(dispatcher);
    }

    #[test]
    fn a_cancelled_call_sends_no_response() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":"t"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            "\n"
        );
        let msgs = run(input, 150, 10);
        assert!(msgs.iter().all(|m| m["id"] != 1), "got {msgs:?}");
    }

    #[test]
    fn a_second_request_is_answered_while_a_call_runs() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n"
        );
        let msgs = run(input, 150, 1000);
        assert_eq!(msgs[0]["id"], 2, "the quick request answers first");
        assert_eq!(msgs[1]["id"], 1);
    }

    #[test]
    fn overlapping_tool_calls_run_one_at_a_time_in_arrival_order() {
        let running = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicBool::new(false));
        let out = Arc::new(Mutex::new(Vec::<String>::new()));
        let (r, o, sink) = (
            Arc::clone(&running),
            Arc::clone(&overlapped),
            Arc::clone(&out),
        );
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"synth_mutate_layout"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"synth_export"}}"#,
            "\n"
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(serve(
            BufReader::new(std::io::Cursor::new(input.as_bytes().to_vec())).lines(),
            Arc::new(move |req: Value| {
                if r.fetch_add(1, Ordering::SeqCst) != 0 {
                    o.store(true, Ordering::SeqCst);
                }
                std::thread::sleep(Duration::from_millis(80));
                r.fetch_sub(1, Ordering::SeqCst);
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_secs(60),
            Arc::new(move |m: &str| sink.lock().unwrap().push(m.to_string())),
        ));
        assert!(!overlapped.load(Ordering::SeqCst), "two calls ran at once");
        let ids: Vec<Value> = out
            .lock()
            .unwrap()
            .iter()
            .map(|l| serde_json::from_str::<Value>(l).unwrap()["id"].clone())
            .collect();
        assert_eq!(ids, vec![Value::from(1), Value::from(2)]);
    }

    /// Cancelling a call that is already running ends the external process it is
    /// waiting on; it is not merely left to finish with its reply dropped.
    #[test]
    fn cancelling_a_running_call_stops_its_external_work() {
        use tokio::io::AsyncWriteExt;
        let sink = Arc::new(Mutex::new(Vec::<String>::new()));
        let out = Arc::clone(&sink);
        let started = std::time::Instant::now();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (mut tx, rx) = tokio::io::duplex(4096);
            let writer = tokio::spawn(async move {
                tx.write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{}}\n",
                )
                .await
                .unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
                tx.write_all(
                    b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":1}}\n",
                )
                .await
                .unwrap();
            });
            serve(
                BufReader::new(rx).lines(),
                Arc::new(|req: Value| {
                    // A tool that waits on an external process for a minute.
                    let run = synth_router::process::run(
                        std::path::Path::new("/bin/sh"),
                        &["-c".to_string(), "sleep 60".to_string()],
                        Duration::from_secs(60),
                        4096,
                    );
                    let cancelled = run.is_ok_and(|r| r.cancelled);
                    serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {"cancelled": cancelled}})
                }),
                Duration::from_secs(60),
                Arc::new(move |m: &str| out.lock().unwrap().push(m.to_string())),
            )
            .await;
            writer.await.unwrap();
        });
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the sleep was waited out: {:?}",
            started.elapsed()
        );
        assert!(
            sink.lock().unwrap().is_empty(),
            "no reply for a cancelled call"
        );
    }

    #[test]
    fn a_call_cancelled_while_queued_never_runs() {
        let ran = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = Arc::clone(&ran);
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}"#,
            "\n"
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(serve(
            BufReader::new(std::io::Cursor::new(input.as_bytes().to_vec())).lines(),
            Arc::new(move |req: Value| {
                seen.lock().unwrap().push(req["id"].clone());
                std::thread::sleep(Duration::from_millis(100));
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_secs(60),
            Arc::new(|_: &str| {}),
        ));
        assert_eq!(*ran.lock().unwrap(), vec![Value::from(1)]);
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    /// Starts the HTTP routes on an ephemeral port, wired the way
    /// `run_sse_server` wires them, with a custom tool handle and heartbeat
    /// interval so tests can watch progress and cancellation.
    async fn spawn_http(
        handle: Arc<dyn Fn(Value) -> Value + Send + Sync + 'static>,
        interval: Duration,
    ) -> SocketAddr {
        let dispatcher = Arc::new(Dispatcher::new(handle, interval, Arc::new(|_: &str| {})));
        let app = sse_router(dispatcher);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    /// Sends one HTTP request and returns the response once the server closes
    /// the connection. HTTP/1.0 keeps the body free of chunked framing, so an
    /// SSE body reads back as plain events.
    async fn request(addr: SocketAddr, target: &str, body: impl Into<String>) -> (String, String) {
        let body = body.into();
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let message = format!(
            "{target} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(message.as_bytes()).await.unwrap();
        let mut raw = String::new();
        tokio::io::BufReader::new(stream)
            .read_to_string(&mut raw)
            .await
            .unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap_or((raw.as_str(), ""));
        (headers.to_string(), body.to_string())
    }

    /// The `data:` payloads of an SSE body, in arrival order.
    fn sse_data(body: &str) -> Vec<Value> {
        body.split("\n\n")
            .filter_map(|event| event.strip_prefix("data:"))
            .map(|data| data.strip_prefix(' ').unwrap_or(data))
            .map(|data| serde_json::from_str(data).unwrap())
            .collect()
    }

    fn tools_call(id: u64, extra_params: &str) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{extra_params}}}"#)
    }

    #[tokio::test]
    async fn a_call_with_a_token_streams_progress_then_its_response() {
        let addr = spawn_http(
            Arc::new(|req: Value| {
                if req["method"] == "tools/call" {
                    std::thread::sleep(Duration::from_millis(150));
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_millis(20),
        )
        .await;
        let (headers, body) = request(
            addr,
            "POST /rpc",
            tools_call(1, r#"{"_meta":{"progressToken":"t"}}"#),
        )
        .await;

        assert!(headers.contains("text/event-stream"), "got {headers}");
        let msgs = sse_data(&body);
        let beats: Vec<&Value> = msgs
            .iter()
            .filter(|m| m["method"] == "notifications/progress")
            .collect();
        assert!(beats.len() >= 2, "expected heartbeats, got {}", beats.len());
        assert!(
            beats.iter().all(|m| m["params"]["progressToken"] == "t"),
            "{beats:?}"
        );
        // The response is the last event, behind the progress.
        let last = msgs.last().unwrap();
        assert_eq!(last["id"], 1);
        assert!(last["result"].is_object(), "{last}");
    }

    #[tokio::test]
    async fn a_call_without_a_token_answers_with_plain_json() {
        let addr = spawn_http(
            Arc::new(
                |req: Value| serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}}),
            ),
            HEARTBEAT_INTERVAL,
        )
        .await;
        let (headers, body) = request(addr, "POST /message", tools_call(1, "{}")).await;
        assert!(headers.contains("application/json"), "got {headers}");
        let resp: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(resp["id"], 1);
        assert!(resp["result"].is_object(), "{resp}");
    }

    /// Cancelling a running call ends the external process it waits on, and the
    /// HTTP request answers with the cancellation error the MCP spec
    /// prescribes, where stdio would send no response at all.
    #[tokio::test]
    async fn a_cancelled_call_stops_its_work_and_answers_the_spec_error() {
        let addr = spawn_http(
            Arc::new(|req: Value| {
                if req["method"] == "tools/call" {
                    let run = synth_router::process::run(
                        std::path::Path::new("/bin/sh"),
                        &["-c".to_string(), "sleep 60".to_string()],
                        Duration::from_secs(60),
                        4096,
                    );
                    let cancelled = run.is_ok_and(|r| r.cancelled);
                    return serde_json::json!(
                        {"jsonrpc": "2.0", "id": req["id"], "result": {"cancelled": cancelled}}
                    );
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            HEARTBEAT_INTERVAL,
        )
        .await;
        let started = std::time::Instant::now();

        let call = tokio::spawn(request(addr, "POST /rpc", tools_call(1, "{}")));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = request(
            addr,
            "POST /message",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        )
        .await;
        let (headers, body) = call.await.unwrap();

        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the sleep was waited out: {:?}",
            started.elapsed()
        );
        assert!(headers.contains("application/json"), "got {headers}");
        let resp: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(resp["error"]["code"], -32800);
        assert_eq!(resp["error"]["message"], "Request cancelled");
        assert_eq!(resp["id"], 1);
    }

    #[tokio::test]
    async fn a_cancelled_call_with_a_token_answers_the_spec_error_on_its_stream() {
        let addr = spawn_http(
            Arc::new(|req: Value| {
                if req["method"] == "tools/call" {
                    let _ = synth_router::process::run(
                        std::path::Path::new("/bin/sh"),
                        &["-c".to_string(), "sleep 60".to_string()],
                        Duration::from_secs(60),
                        4096,
                    );
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            HEARTBEAT_INTERVAL,
        )
        .await;
        let started = std::time::Instant::now();

        let call = tokio::spawn(request(
            addr,
            "POST /rpc",
            tools_call(1, r#"{"_meta":{"progressToken":"t"}}"#),
        ));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = request(
            addr,
            "POST /message",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        )
        .await;
        let body = call.await.unwrap().1;

        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the sleep was waited out: {:?}",
            started.elapsed()
        );
        let msgs = sse_data(&body);
        let last = msgs.last().unwrap();
        assert_eq!(last["error"]["code"], -32800, "{msgs:?}");
        assert_eq!(last["id"], 1);
    }

    /// The fast methods answer while a call runs: the handler waits for the
    /// worker instead of blocking on it, so `tools/list` is not stuck behind
    /// an export.
    #[tokio::test]
    async fn a_second_request_is_answered_while_a_call_runs() {
        let addr = spawn_http(
            Arc::new(|req: Value| {
                if req["method"] == "tools/call" {
                    std::thread::sleep(Duration::from_millis(500));
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_secs(60),
        )
        .await;

        let call = tokio::spawn(request(addr, "POST /rpc", tools_call(1, "{}")));
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = std::time::Instant::now();
        let (_, body) = request(
            addr,
            "POST /rpc",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        )
        .await;
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "tools/list waited for the call: {:?}",
            started.elapsed()
        );
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], 2);
        let (_, body) = call.await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], 1);
    }

    /// A call queued behind a long export still answers, and one cancelled
    /// while queued never runs.
    #[tokio::test]
    async fn a_call_cancelled_while_queued_never_runs() {
        let ran = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = Arc::clone(&ran);
        let addr = spawn_http(
            Arc::new(move |req: Value| {
                seen.lock().unwrap().push(req["id"].clone());
                if req["method"] == "tools/call" {
                    std::thread::sleep(Duration::from_millis(400));
                }
                serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }),
            Duration::from_secs(60),
        )
        .await;

        let first = tokio::spawn(request(addr, "POST /rpc", tools_call(1, "{}")));
        tokio::time::sleep(Duration::from_millis(30)).await;
        let second = tokio::spawn(request(addr, "POST /rpc", tools_call(2, "{}")));
        tokio::time::sleep(Duration::from_millis(30)).await;
        let _ = request(
            addr,
            "POST /message",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}"#,
        )
        .await;
        let (_, body) = second.await.unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
            -32800
        );
        first.await.unwrap();
        assert_eq!(*ran.lock().unwrap(), vec![Value::from(1)]);
    }

    /// The production wiring: the real router, the real handler, JSON answers.
    #[tokio::test]
    async fn the_production_router_answers_tools_list_with_json() {
        let dispatcher = Arc::new(Dispatcher::new(
            Arc::new(|req| handle_jsonrpc_request(req, None)),
            HEARTBEAT_INTERVAL,
            Arc::new(|_: &str| {}),
        ));
        let app = sse_router(dispatcher);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (_, body) = request(
            addr,
            "POST /rpc",
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
        )
        .await;
        let resp: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(resp["id"], 1);
        assert!(!resp["result"]["tools"].as_array().unwrap().is_empty());
    }
}
