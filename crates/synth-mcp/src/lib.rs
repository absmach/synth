// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::similar_names)]

//! Synth Model Context Protocol (MCP) Server.
//!
//! Provides stdio and HTTP JSON-RPC 2.0 servers exposing Synth EDA
//! compilation, validation, patching, preview layout, and KiCad export
//! tools to AI agents and LLM clients.

pub mod audit;
pub mod cloud_endpoint;
pub mod server;
pub mod tools;

pub use audit::{AuditEvent, AuditLogger};
pub use cloud_endpoint::{CloudMcpServer, RateLimiter};
pub use server::{handle_jsonrpc_request, run_sse_server, run_stdio_server, SERVER_INSTRUCTIONS};
pub use tools::{call_tool, list_tools, McpToolInfo};
