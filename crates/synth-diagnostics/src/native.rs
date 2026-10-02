// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCheckStatus {
    Pass,
    Fail,
    Unknown,
}

impl NativeCheckStatus {
    pub fn is_trusted(self) -> bool {
        matches!(self, Self::Pass)
    }

    pub fn is_blocking(self) -> bool {
        !self.is_trusted()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for NativeCheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    NotInstalled,
    SpawnFailed,
    Timeout,
    CommandFailed,
    ReportMissing,
    ReportUnreadable,
    ReportMalformed,
    ReportUnrecognized,
    UnsupportedVersion,
}

impl UnknownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::SpawnFailed => "spawn_failed",
            Self::Timeout => "timeout",
            Self::CommandFailed => "command_failed",
            Self::ReportMissing => "report_missing",
            Self::ReportUnreadable => "report_unreadable",
            Self::ReportMalformed => "report_malformed",
            Self::ReportUnrecognized => "report_unrecognized",
            Self::UnsupportedVersion => "unsupported_version",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::NotInstalled => {
                "kicad-cli was not found; install KiCad or set KICAD_CLI to its path"
            }
            Self::SpawnFailed => "the verification tool could not be started",
            Self::Timeout => "the verification tool exceeded its time budget and was terminated",
            Self::CommandFailed => {
                "the verification tool exited with a failure code (see stderr; a missing \
                 symbol/footprint library is the common cause)"
            }
            Self::ReportMissing => "the verification tool wrote no report file",
            Self::ReportUnreadable => "the report file could not be read",
            Self::ReportMalformed => "the report file is not valid JSON",
            Self::ReportUnrecognized => {
                "the report JSON has an unrecognized layout, so an empty result cannot be \
                 read as a clean run"
            }
            Self::UnsupportedVersion => "the tool version is outside the supported range",
        }
    }
}

impl std::fmt::Display for UnknownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCheckEvidence {
    pub stage: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    pub status: NativeCheckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<UnknownReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(default)]
    pub violations: usize,
}

pub const STDERR_CAPTURE_LIMIT: usize = 8 * 1024;

impl NativeCheckEvidence {
    pub fn concluded(
        stage: impl Into<String>,
        tool: impl Into<String>,
        command: Vec<String>,
        violations: usize,
    ) -> Self {
        Self {
            stage: stage.into(),
            tool: tool.into(),
            tool_version: None,
            command,
            status: if violations == 0 {
                NativeCheckStatus::Pass
            } else {
                NativeCheckStatus::Fail
            },
            reason: None,
            detail: None,
            stderr: None,
            violations,
        }
    }

    pub fn unknown(
        stage: impl Into<String>,
        tool: impl Into<String>,
        command: Vec<String>,
        reason: UnknownReason,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            stage: stage.into(),
            tool: tool.into(),
            tool_version: None,
            command,
            status: NativeCheckStatus::Unknown,
            reason: Some(reason),
            detail: Some(detail.into()),
            stderr: None,
            violations: 0,
        }
    }

    pub fn with_version(mut self, version: Option<String>) -> Self {
        self.tool_version = version;
        self
    }

    pub fn with_stderr(mut self, stderr: &str) -> Self {
        let trimmed = stderr.trim_end();
        if trimmed.is_empty() {
            self.stderr = None;
            return self;
        }
        self.stderr = Some(truncate_on_char_boundary(trimmed, STDERR_CAPTURE_LIMIT));
        self
    }

    pub fn is_trusted(&self) -> bool {
        self.status.is_trusted()
    }

    pub fn summary_line(&self) -> String {
        match (self.status, self.reason) {
            (NativeCheckStatus::Unknown, Some(reason)) => {
                format!(
                    "{}: unknown ({}) — {}",
                    self.stage,
                    reason.as_str(),
                    reason.explain()
                )
            }
            (NativeCheckStatus::Unknown, None) => format!("{}: unknown", self.stage),
            (status, _) => format!(
                "{}: {} ({} violation{})",
                self.stage,
                status.as_str(),
                self.violations,
                if self.violations == 1 { "" } else { "s" }
            ),
        }
    }
}

fn truncate_on_char_boundary(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_not_trusted_and_blocks() {
        let e = NativeCheckEvidence::unknown(
            "kicad_drc",
            "kicad-cli",
            vec!["kicad-cli".into(), "pcb".into(), "drc".into()],
            UnknownReason::NotInstalled,
            "No such file or directory",
        );
        assert!(!e.is_trusted());
        assert!(e.status.is_blocking());
        assert_eq!(e.status, NativeCheckStatus::Unknown);
    }

    #[test]
    fn clean_run_passes_and_violations_fail() {
        let clean = NativeCheckEvidence::concluded("kicad_erc", "kicad-cli", vec![], 0);
        assert!(clean.is_trusted());
        let dirty = NativeCheckEvidence::concluded("kicad_erc", "kicad-cli", vec![], 3);
        assert_eq!(dirty.status, NativeCheckStatus::Fail);
        assert!(dirty.status.is_blocking());
    }

    #[test]
    fn status_and_reason_wire_strings_are_stable() {
        assert_eq!(
            serde_json::to_string(&NativeCheckStatus::Unknown).unwrap(),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&UnknownReason::ReportUnrecognized).unwrap(),
            "\"report_unrecognized\""
        );
        assert_eq!(UnknownReason::Timeout.as_str(), "timeout");
    }

    #[test]
    fn pass_evidence_omits_unknown_only_fields() {
        let json = serde_json::to_value(NativeCheckEvidence::concluded(
            "kicad_drc",
            "kicad-cli",
            vec!["kicad-cli".into()],
            0,
        ))
        .unwrap();
        assert_eq!(json["status"], "pass");
        assert!(json.get("reason").is_none());
        assert!(json.get("detail").is_none());
        assert!(json.get("stderr").is_none());
    }

    #[test]
    fn stderr_is_truncated_on_a_char_boundary() {
        let noisy = "é".repeat(STDERR_CAPTURE_LIMIT);
        let e =
            NativeCheckEvidence::concluded("kicad_drc", "kicad-cli", vec![], 0).with_stderr(&noisy);
        let captured = e.stderr.expect("stderr retained");
        assert!(captured.ends_with("… [truncated]"));
        assert!(captured.starts_with('é'));
    }

    #[test]
    fn empty_stderr_is_dropped_rather_than_serialized() {
        let e =
            NativeCheckEvidence::concluded("kicad_drc", "kicad-cli", vec![], 0).with_stderr("  \n");
        assert!(e.stderr.is_none());
    }

    #[test]
    fn summary_line_names_the_reason_for_unknown() {
        let e = NativeCheckEvidence::unknown(
            "kicad_erc",
            "kicad-cli",
            vec![],
            UnknownReason::Timeout,
            "exceeded 60s",
        );
        let line = e.summary_line();
        assert!(line.contains("unknown"), "{line}");
        assert!(line.contains("timeout"), "{line}");
    }
}
