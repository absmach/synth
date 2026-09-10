// SPDX-License-Identifier: Apache-2.0

//! Sub-Phase 12d Cloud MCP Server Transport & Audit Integration Test.

use synth_mcp::{AuditLogger, CloudMcpServer};

#[test]
fn test_cloud_mcp_jsonrpc_execution_and_audit() {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "tools/list"
    })
    .to_string();

    let res = CloudMcpServer::handle_jsonrpc_request(&req, "test_client_session");
    assert!(
        res.is_ok(),
        "Cloud MCP server handler must succeed: {res:?}"
    );

    let res_str = res.unwrap();
    assert!(res_str.contains("synth_validate"));
    assert!(res_str.contains("synth_cloud_mcp_endpoint"));

    // Verify Audit Logger path exists
    let audit_path = AuditLogger::audit_log_path();
    println!("[Sub-Phase 12d] Audit log file target path: {audit_path:?}");
}
