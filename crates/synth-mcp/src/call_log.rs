// SPDX-License-Identifier: Apache-2.0

//! Agent-queryable view of the audit log: `synth_recent_calls` and
//! `synth_server_stats`.
//!
//! Both tools only read `audit.jsonl` as `audit.rs` already writes it. They
//! add no new fields to the log and never echo tool arguments.

use crate::audit::AuditLogger;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

/// Entries returned when the caller gives no `limit`.
pub const DEFAULT_LIMIT: usize = 20;
/// Largest accepted `limit`.
pub const MAX_LIMIT: usize = 200;
/// Longest `message` returned, in characters.
const MAX_MESSAGE_CHARS: usize = 200;

static SERVER_START: OnceLock<Instant> = OnceLock::new();

/// Record the server start time. Later calls do nothing.
pub fn mark_server_start() {
    let _ = SERVER_START.get_or_init(Instant::now);
}

/// One parsed audit line.
struct Entry {
    call_id: u64,
    tool: String,
    timestamp: String,
    duration_ms: u64,
    status: &'static str,
    error_kind: Option<String>,
    message: Option<String>,
    design_hash: Option<String>,
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_MESSAGE_CHARS {
        text.to_string()
    } else {
        let cut: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        format!("{cut}...")
    }
}

/// Parse one log line. `call_id` is the 1-based line number, which is stable
/// because the log is append-only. Returns `None` for a malformed line.
fn parse_line(line: &str, call_id: u64) -> Option<Entry> {
    let v: Value = serde_json::from_str(line).ok()?;
    let tool = v.get("tool_name")?.as_str()?.to_string();
    let timestamp = v.get("timestamp_iso8601")?.as_str()?.to_string();
    let duration_ms = v.get("duration_ms")?.as_u64()?;
    let is_success = v.get("is_success")?.as_bool()?;
    let meta = v.get("metadata");
    let meta_str = |key: &str| {
        meta.and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let status = if is_success {
        "ok"
    } else if meta_str("status").as_deref() == Some("refused") {
        "refused"
    } else {
        "error"
    };
    Some(Entry {
        call_id,
        tool,
        timestamp,
        duration_ms,
        status,
        error_kind: meta_str("error_kind"),
        message: meta_str("message").map(|m| truncate(&m)),
        design_hash: meta_str("design_hash"),
    })
}

/// Visit every line of the log. Returns the number of malformed lines.
fn scan(path: &Path, mut visit: impl FnMut(Entry)) -> Result<usize, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open audit log: {}", e.kind()))?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut line_no = 0u64;
    let mut malformed = 0usize;
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| format!("Failed to read audit log: {}", e.kind()))?;
        if n == 0 {
            break;
        }
        line_no += 1;
        let text = String::from_utf8_lossy(&buf);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match parse_line(text, line_no) {
            Some(entry) => visit(entry),
            None => malformed += 1,
        }
    }
    Ok(malformed)
}

/// True when the log is switched off (`SYNTH_AUDIT_LOG` set to an empty
/// string) or the file does not exist yet.
fn log_unavailable(path: &Path) -> Option<&'static str> {
    if path.as_os_str().is_empty() {
        Some("The audit log is disabled: SYNTH_AUDIT_LOG is empty.")
    } else if !path.is_file() {
        Some("The audit log does not exist yet. No calls have been recorded.")
    } else {
        None
    }
}

fn parse_limit(args: &Value) -> Result<usize, String> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(DEFAULT_LIMIT),
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| "limit must be a non-negative integer".to_string())?;
            if n == 0 || n > MAX_LIMIT as u64 {
                return Err(format!("limit must be in 1..={MAX_LIMIT}"));
            }
            Ok(n as usize)
        }
    }
}

fn parse_string_filter(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key} must be a non-empty string")),
    }
}

/// `synth_recent_calls` against the default audit log path.
pub fn execute_recent_calls(args: &Value) -> Result<Value, String> {
    recent_calls_from(&AuditLogger::audit_log_path(), args)
}

/// `synth_server_stats` against the default audit log path.
pub fn execute_server_stats(_args: &Value) -> Result<Value, String> {
    server_stats_from(&AuditLogger::audit_log_path())
}

fn recent_calls_from(path: &Path, args: &Value) -> Result<Value, String> {
    let limit = parse_limit(args)?;
    let tool = parse_string_filter(args, "tool")?;
    let status = parse_string_filter(args, "status")?;
    if let Some(s) = &status
        && !matches!(s.as_str(), "ok" | "error" | "refused")
    {
        return Err(format!(
            "Unknown status '{s}'. Use one of: ok, error, refused."
        ));
    }

    if let Some(note) = log_unavailable(path) {
        return Ok(json!({
            "calls": [],
            "count": 0,
            "malformed_lines": 0,
            "note": note,
        }));
    }

    let mut window: VecDeque<Entry> = VecDeque::with_capacity(limit);
    let malformed = scan(path, |e| {
        if tool.as_deref().is_some_and(|t| t != e.tool) {
            return;
        }
        if status.as_deref().is_some_and(|s| s != e.status) {
            return;
        }
        if window.len() == limit {
            window.pop_front();
        }
        window.push_back(e);
    })?;

    let calls: Vec<Value> = window
        .iter()
        .rev()
        .map(|e| {
            let mut obj = json!({
                "call_id": e.call_id,
                "tool": e.tool,
                "timestamp": e.timestamp,
                "duration_ms": e.duration_ms,
                "status": e.status,
                "error_kind": e.error_kind,
                "message": e.message,
            });
            if let Some(hash) = &e.design_hash {
                obj["design_hash"] = json!(hash);
            }
            obj
        })
        .collect();
    Ok(json!({
        "count": calls.len(),
        "calls": calls,
        "malformed_lines": malformed,
    }))
}

fn server_stats_from(path: &Path) -> Result<Value, String> {
    let start = SERVER_START.get_or_init(Instant::now);
    let uptime_ms = start.elapsed().as_millis() as u64;
    let mut out = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_ms": uptime_ms,
        "audit_log_path": path.display().to_string(),
    });

    if let Some(note) = log_unavailable(path) {
        out["audit_log_available"] = json!(false);
        out["total_calls"] = json!(0);
        out["error_calls"] = json!(0);
        out["refused_calls"] = json!(0);
        out["per_tool"] = json!({});
        out["last_call_timestamp"] = Value::Null;
        out["ms_since_last_call"] = Value::Null;
        out["malformed_lines"] = json!(0);
        out["note"] = json!(note);
        return Ok(out);
    }

    let mut per_tool: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let (mut total, mut errors, mut refused) = (0u64, 0u64, 0u64);
    let mut last: Option<chrono::DateTime<chrono::FixedOffset>> = None;
    let mut last_ts: Option<String> = None;
    let malformed = scan(path, |e| {
        total += 1;
        let slot = per_tool.entry(e.tool).or_default();
        slot.0 += 1;
        if e.status != "ok" {
            slot.1 += 1;
            errors += 1;
        }
        if e.status == "refused" {
            refused += 1;
        }
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&e.timestamp)
            && last.is_none_or(|l| t > l)
        {
            last = Some(t);
            last_ts = Some(e.timestamp);
        }
    })?;

    let since = last.map(|t| {
        chrono::Utc::now()
            .signed_duration_since(t)
            .num_milliseconds()
            .max(0)
    });
    out["audit_log_available"] = json!(true);
    out["total_calls"] = json!(total);
    out["error_calls"] = json!(errors);
    out["refused_calls"] = json!(refused);
    out["per_tool"] = json!(
        per_tool
            .into_iter()
            .map(|(k, (c, e))| (k, json!({ "calls": c, "errors": e })))
            .collect::<serde_json::Map<_, _>>()
    );
    out["last_call_timestamp"] = json!(last_ts);
    out["ms_since_last_call"] = json!(since);
    out["malformed_lines"] = json!(malformed);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(tool: &str, ok: bool, ms: u64, ts: &str, meta: &Value) -> String {
        json!({
            "timestamp_iso8601": ts,
            "session_id": "s",
            "tool_name": tool,
            "duration_ms": ms,
            "is_success": ok,
            "drc_clean": null,
            "ipc_class": "IPC Class 3 High Reliability",
            "metadata": meta,
        })
        .to_string()
    }

    fn sample_log(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = dir.path().join("audit.jsonl");
        let lines = [
            line(
                "synth_validate",
                true,
                10,
                "2026-01-01T00:00:00+00:00",
                &json!({}),
            ),
            line(
                "synth_export",
                false,
                20,
                "2026-01-01T00:00:01+00:00",
                &json!({"error_kind": "missing_engine", "message": "no engine"}),
            ),
            "not json at all".to_string(),
            line(
                "synth_route",
                false,
                30,
                "2026-01-01T00:00:02+00:00",
                &json!({"status": "refused", "error_kind": "invalid_argument", "design_hash": "abc"}),
            ),
            line(
                "synth_validate",
                true,
                40,
                "2026-01-01T00:00:03+00:00",
                &json!({"request_id": 7}),
            ),
            "{\"tool_name\": \"x\"}".to_string(),
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn ids(v: &Value) -> Vec<u64> {
        v["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["call_id"].as_u64().unwrap())
            .collect()
    }

    #[test]
    fn newest_first_and_malformed_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample_log(&dir);
        let r = recent_calls_from(&path, &json!({})).unwrap();
        assert_eq!(ids(&r), vec![5, 4, 2, 1]);
        assert_eq!(r["malformed_lines"], 2);
        assert_eq!(r["calls"][0]["status"], "ok");
        assert_eq!(r["calls"][1]["status"], "refused");
        assert_eq!(r["calls"][1]["design_hash"], "abc");
        assert_eq!(r["calls"][2]["status"], "error");
        assert_eq!(r["calls"][2]["error_kind"], "missing_engine");
        assert_eq!(r["calls"][2]["message"], "no engine");
        assert!(r["calls"][0].get("design_hash").is_none());
    }

    #[test]
    fn limit_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample_log(&dir);
        let r = recent_calls_from(&path, &json!({"limit": 2})).unwrap();
        assert_eq!(ids(&r), vec![5, 4]);
        let r = recent_calls_from(&path, &json!({"tool": "synth_validate"})).unwrap();
        assert_eq!(ids(&r), vec![5, 1]);
        let r = recent_calls_from(&path, &json!({"status": "error"})).unwrap();
        assert_eq!(ids(&r), vec![2]);
        let r =
            recent_calls_from(&path, &json!({"status": "refused", "tool": "synth_route"})).unwrap();
        assert_eq!(ids(&r), vec![4]);
        let r = recent_calls_from(&path, &json!({"tool": "nope"})).unwrap();
        assert_eq!(r["count"], 0);
    }

    #[test]
    fn bad_arguments_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample_log(&dir);
        for bad in [
            json!({"limit": -1}),
            json!({"limit": 1.5}),
            json!({"limit": "3"}),
            json!({"limit": 0}),
            json!({"limit": MAX_LIMIT + 1}),
            json!({"tool": 3}),
            json!({"status": "bogus"}),
        ] {
            assert!(recent_calls_from(&path, &bad).is_err(), "{bad}");
        }
        let err = recent_calls_from(&path, &json!({"status": "bogus"})).unwrap_err();
        assert!(err.contains("ok, error, refused"), "{err}");
        assert!(recent_calls_from(&path, &json!({"limit": MAX_LIMIT})).is_ok());
    }

    #[test]
    fn missing_or_disabled_log_is_an_empty_result() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.jsonl");
        let r = recent_calls_from(&missing, &json!({})).unwrap();
        assert_eq!(r["count"], 0);
        assert!(r["note"].as_str().unwrap().contains("does not exist"));
        let r = recent_calls_from(Path::new(""), &json!({})).unwrap();
        assert!(r["note"].as_str().unwrap().contains("disabled"));
        let s = server_stats_from(&missing).unwrap();
        assert_eq!(s["audit_log_available"], false);
        assert_eq!(s["total_calls"], 0);
        assert!(s["note"].is_string());
    }

    #[test]
    fn long_messages_are_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let long = "x".repeat(500);
        std::fs::write(
            &path,
            line(
                "t",
                false,
                1,
                "2026-01-01T00:00:00+00:00",
                &json!({"message": long}),
            ) + "\n",
        )
        .unwrap();
        let r = recent_calls_from(&path, &json!({})).unwrap();
        let msg = r["calls"][0]["message"].as_str().unwrap();
        assert_eq!(msg.chars().count(), MAX_MESSAGE_CHARS + 3);
    }

    #[test]
    fn stats_arithmetic() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample_log(&dir);
        let s = server_stats_from(&path).unwrap();
        assert_eq!(s["audit_log_available"], true);
        assert_eq!(s["total_calls"], 4);
        assert_eq!(s["error_calls"], 2);
        assert_eq!(s["refused_calls"], 1);
        assert_eq!(s["malformed_lines"], 2);
        assert_eq!(
            s["per_tool"]["synth_validate"],
            json!({"calls": 2, "errors": 0})
        );
        assert_eq!(
            s["per_tool"]["synth_export"],
            json!({"calls": 1, "errors": 1})
        );
        assert_eq!(
            s["per_tool"]["synth_route"],
            json!({"calls": 1, "errors": 1})
        );
        assert_eq!(s["last_call_timestamp"], "2026-01-01T00:00:03+00:00");
        assert!(s["ms_since_last_call"].as_i64().unwrap() > 0);
        assert!(s["uptime_ms"].is_u64());
        assert_eq!(s["version"], env!("CARGO_PKG_VERSION"));
    }
}
