// SPDX-License-Identifier: Apache-2.0

//! Cloud MCP JSON-RPC 2.0 server transport endpoint and rate limiting.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::audit::AuditLogger;
use crate::tools::call_tool;

/// Token bucket in-memory rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    clients: Mutex<HashMap<String, (usize, Instant)>>,
    max_tokens: usize,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(60)
    }
}

impl RateLimiter {
    #[must_use]
    pub fn new(max_tokens: usize) -> Self {
        Self {
            clients: Mutex::new(HashMap::new()),
            max_tokens,
        }
    }

    /// Check if request from `client_id` is allowed under rate limits.
    pub fn check_allow(&self, client_id: &str) -> bool {
        let Ok(mut map) = self.clients.lock() else {
            return true;
        };

        let now = Instant::now();
        let entry = map
            .entry(client_id.to_string())
            .or_insert((self.max_tokens, now));

        if now.duration_since(entry.1).as_secs() >= 60 {
            entry.0 = self.max_tokens;
            entry.1 = now;
        }

        if entry.0 > 0 {
            entry.0 -= 1;
            true
        } else {
            false
        }
    }
}

static GLOBAL_RATE_LIMITER: std::sync::LazyLock<RateLimiter> =
    std::sync::LazyLock::new(|| RateLimiter {
        clients: Mutex::new(HashMap::new()),
        max_tokens: 60,
    });

/// JSON-RPC 2.0 Request wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// Hosted Cloud MCP JSON-RPC 2.0 Server Endpoint.
#[derive(Debug)]
pub struct CloudMcpServer;

impl CloudMcpServer {
    /// Process a raw JSON-RPC 2.0 string request over HTTP POST / SSE transport.
    pub fn handle_jsonrpc_request(request_str: &str, client_id: &str) -> Result<String, String> {
        if !GLOBAL_RATE_LIMITER.check_allow(client_id) {
            return Err("429 Too Many Requests: Rate limit exceeded (60 req/min)".to_string());
        }

        let start_time = Instant::now();
        let req: JsonRpcRequest = serde_json::from_str(request_str)
            .map_err(|e| format!("Invalid JSON-RPC request format: {e}"))?;

        let (tool_name, tool_args) = match req.method.as_str() {
            "tools/call" => {
                let params = req
                    .params
                    .as_ref()
                    .ok_or_else(|| "Missing params in tools/call".to_string())?;
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| "Missing tool name in params".to_string())?;
                let args = &params["arguments"];
                (name, args.clone())
            }
            "tools/list" => {
                let tools = crate::tools::list_tools();
                let res = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": { "tools": tools }
                });
                return serde_json::to_string(&res).map_err(|e| e.to_string());
            }
            other => return Err(format!("Unsupported Cloud MCP method '{other}'")),
        };

        let result = call_tool(tool_name, &tool_args, None);
        let duration_ms = start_time.elapsed().as_millis() as u64;
        let is_success = result.is_ok();

        let _ = AuditLogger::log_event(
            client_id,
            tool_name,
            duration_ms,
            is_success,
            None,
            serde_json::json!({"request_id": req.id}),
        );

        match result {
            Ok(val) => {
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": val
                });
                serde_json::to_string(&response).map_err(|e| e.to_string())
            }
            Err(err_msg) => {
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "error": {
                        "code": -32603,
                        "message": err_msg
                    }
                });
                serde_json::to_string(&response).map_err(|e| e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_list_tools_handler() {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        })
        .to_string();

        let res = CloudMcpServer::handle_jsonrpc_request(&req, "client_unit_test");
        assert!(res.is_ok());
        let res_str = res.unwrap();
        assert!(res_str.contains("synth_validate"));
    }
}
