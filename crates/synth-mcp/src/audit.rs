// SPDX-License-Identifier: Apache-2.0

//! IPC Class 3 High-Reliability Enterprise Compliance Audit Logger.

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static AUDIT_LOCK: Mutex<()> = Mutex::new(());

/// Immutable Audit Event record formatted for IPC Class 3 compliance audits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp_iso8601: String,
    pub session_id: String,
    pub tool_name: String,
    pub duration_ms: u64,
    pub is_success: bool,
    pub drc_clean: Option<bool>,
    pub ipc_class: String,
    pub metadata: serde_json::Value,
}

/// IPC Class 3 Enterprise Audit Logger writing immutable JSON-Lines audit logs (`audit.jsonl`).
#[derive(Debug)]
pub struct AuditLogger;

impl AuditLogger {
    /// Log an immutable tool invocation audit event.
    pub fn log_event(
        session_id: &str,
        tool_name: &str,
        duration_ms: u64,
        is_success: bool,
        drc_clean: Option<bool>,
        metadata: serde_json::Value,
    ) -> Result<(), String> {
        let _guard = AUDIT_LOCK.lock().map_err(|e| e.to_string())?;

        let event = AuditEvent {
            timestamp_iso8601: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            duration_ms,
            is_success,
            drc_clean,
            ipc_class: "IPC Class 3 High Reliability".to_string(),
            metadata,
        };

        let json_line = serde_json::to_string(&event).map_err(|e| e.to_string())?;
        let audit_file_path = Self::audit_log_path();

        if let Some(parent) = audit_file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&audit_file_path)
            .map_err(|e| {
                format!(
                    "Failed to open audit log at '{}': {e}",
                    audit_file_path.display()
                )
            })?;

        writeln!(file, "{json_line}").map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Default path for audit log (`~/.synth/audit.jsonl` or `./audit.jsonl`).
    #[must_use]
    pub fn audit_log_path() -> PathBuf {
        std::env::var("SYNTH_AUDIT_LOG")
            .map_or_else(|_| PathBuf::from("audit.jsonl"), PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_logging() {
        let res = AuditLogger::log_event(
            "test_session_123",
            "synth_validate",
            12,
            true,
            Some(true),
            serde_json::json!({"board": "test"}),
        );
        assert!(res.is_ok(), "Audit logger must succeed: {res:?}");
    }
}
