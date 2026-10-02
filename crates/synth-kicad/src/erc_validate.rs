// SPDX-License-Identifier: Apache-2.0

//! KiCad schematic Electrical Rules Check (ERC) integration via `kicad-cli`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use synth_diagnostics::{NativeCheckEvidence, UnknownReason};
use synth_drc::kicad_cli;

pub const ERC_STAGE: &str = "kicad_erc";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KicadErcItem {
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KicadErcViolation {
    #[serde(default, rename = "type")]
    pub violation_type: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub items: Vec<KicadErcItem>,
}

impl KicadErcViolation {
    pub fn is_error(&self) -> bool {
        self.severity.eq_ignore_ascii_case("error")
    }
}

#[derive(Debug, Clone)]
pub struct NativeErcOutcome {
    pub violations: Vec<KicadErcViolation>,
    pub evidence: NativeCheckEvidence,
}

impl NativeErcOutcome {
    pub fn errors(&self) -> impl Iterator<Item = &KicadErcViolation> {
        self.violations.iter().filter(|v| v.is_error())
    }
}

const ERC_REPORT_KEYS: [&str; 5] = [
    "sheets",
    "violations",
    "kicad_version",
    "coordinate_units",
    "$schema",
];

pub fn run_kicad_erc(sch_path: &Path) -> NativeErcOutcome {
    let tool = kicad_cli::binary();
    let version = kicad_cli::version();

    let unknown = |reason: UnknownReason, command: Vec<String>, detail: String| NativeErcOutcome {
        violations: Vec::new(),
        evidence: NativeCheckEvidence::unknown(ERC_STAGE, &tool, command, reason, detail)
            .with_version(version.clone()),
    };

    if let Some((reason, detail)) = kicad_cli::version_rejection(version.as_deref()) {
        return unknown(reason, Vec::new(), detail);
    }

    let report = kicad_cli::ScratchFile::reserve("synth_kicad_erc", "json");
    let args = vec![
        "sch".to_string(),
        "erc".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--output".to_string(),
        report.arg(),
        "--severity-all".to_string(),
        sch_path.to_string_lossy().into_owned(),
    ];

    let budget = kicad_cli::timeout();
    let run = match kicad_cli::run(&args, budget) {
        Ok(run) => run,
        Err(failure) => return unknown(failure.reason, failure.command, failure.detail),
    };

    if let Some(reason) = run.failure_reason() {
        return NativeErcOutcome {
            violations: Vec::new(),
            evidence: NativeCheckEvidence::unknown(
                ERC_STAGE,
                &tool,
                run.command.clone(),
                reason,
                run.failure_detail(budget),
            )
            .with_version(version.clone())
            .with_stderr(&run.stderr),
        };
    }

    let violations = match read_erc_report(report.path()) {
        Ok(violations) => violations,
        Err((reason, detail)) => {
            return NativeErcOutcome {
                violations: Vec::new(),
                evidence: NativeCheckEvidence::unknown(
                    ERC_STAGE,
                    &tool,
                    run.command.clone(),
                    reason,
                    detail,
                )
                .with_version(version.clone())
                .with_stderr(&run.stderr),
            }
        }
    };

    let error_count = violations.iter().filter(|v| v.is_error()).count();
    let evidence = NativeCheckEvidence::concluded(ERC_STAGE, &tool, run.command, error_count)
        .with_version(version)
        .with_stderr(&run.stderr);
    NativeErcOutcome {
        violations,
        evidence,
    }
}

fn read_erc_report(path: &Path) -> Result<Vec<KicadErcViolation>, (UnknownReason, String)> {
    if !path.exists() {
        return Err((
            UnknownReason::ReportMissing,
            format!(
                "kicad-cli exited cleanly but wrote no ERC report at {}",
                path.display()
            ),
        ));
    }
    let content = std::fs::read_to_string(path).map_err(|e| {
        (
            UnknownReason::ReportUnreadable,
            format!("could not read ERC report {}: {e}", path.display()),
        )
    })?;
    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        (
            UnknownReason::ReportMalformed,
            format!("ERC report {} is not valid JSON: {e}", path.display()),
        )
    })?;
    parse_erc_report(&json)
}

fn parse_erc_report(
    json: &serde_json::Value,
) -> Result<Vec<KicadErcViolation>, (UnknownReason, String)> {
    let recognized = json
        .as_object()
        .is_some_and(|map| ERC_REPORT_KEYS.iter().any(|key| map.contains_key(*key)));
    if !recognized {
        return Err((
            UnknownReason::ReportUnrecognized,
            "ERC report JSON has none of the keys a kicad-cli report carries, so an empty \
             result cannot be read as a clean schematic"
                .to_string(),
        ));
    }

    let mut violations = Vec::new();
    if let Some(sheets) = json.get("sheets").and_then(|s| s.as_array()) {
        for sheet in sheets {
            collect_violations(sheet.get("violations"), &mut violations);
        }
    }
    collect_violations(json.get("violations"), &mut violations);
    Ok(violations)
}

fn collect_violations(value: Option<&serde_json::Value>, out: &mut Vec<KicadErcViolation>) {
    let Some(array) = value.and_then(|v| v.as_array()) else {
        return;
    };
    out.extend(
        array
            .iter()
            .filter_map(|v| serde_json::from_value::<KicadErcViolation>(v.clone()).ok()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const KICAD10_NESTED: &str = r#"{
        "sheets": [
            {
                "path": "/",
                "uuid_path": "/abc",
                "violations": [
                    {
                        "type": "endpoint_off_grid",
                        "severity": "warning",
                        "description": "Symbol pin or wire end off connection grid",
                        "items": [{"description": "Symbol U1 Pin 1"}]
                    }
                ]
            }
        ]
    }"#;

    #[test]
    fn parses_the_kicad10_nested_schema() {
        let json: serde_json::Value = serde_json::from_str(KICAD10_NESTED).unwrap();
        let violations = parse_erc_report(&json).expect("recognized report");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].violation_type, "endpoint_off_grid");
        assert_eq!(violations[0].severity, "warning");
        assert_eq!(violations[0].items[0].description, "Symbol U1 Pin 1");
        assert!(!violations[0].is_error(), "a warning is not an error");
    }

    #[test]
    fn parses_the_flat_schema_too() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"violations": [{"type": "pin_not_driven", "severity": "error",
                                "description": "Input pin not driven"}]}"#,
        )
        .unwrap();
        let violations = parse_erc_report(&json).expect("recognized report");
        assert_eq!(violations.len(), 1);
        assert!(violations[0].is_error());
    }

    #[test]
    fn a_clean_report_parses_to_no_violations() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"kicad_version": "10.0.1", "sheets": []}"#).unwrap();
        assert!(parse_erc_report(&json)
            .expect("recognized report")
            .is_empty());
    }

    #[test]
    fn a_foreign_document_is_unrecognized_not_clean() {
        for foreign in [
            serde_json::json!({}),
            serde_json::json!({"error": "could not open schematic"}),
            serde_json::json!([]),
        ] {
            let (reason, _) = parse_erc_report(&foreign)
                .expect_err("an unrecognized document must not read as clean");
            assert_eq!(reason, UnknownReason::ReportUnrecognized, "{foreign:?}");
        }
    }

    #[test]
    fn a_missing_report_is_report_missing() {
        let absent = std::env::temp_dir().join("synth_erc_absent_report_test.json");
        let _ = std::fs::remove_file(&absent);
        let (reason, _) = read_erc_report(&absent).expect_err("a missing report is not evidence");
        assert_eq!(reason, UnknownReason::ReportMissing);
    }

    #[test]
    fn a_truncated_report_is_report_malformed() {
        let path = std::env::temp_dir().join("synth_erc_malformed_report_test.json");
        std::fs::write(&path, b"{\"sheets\": [").expect("write truncated report");
        let (reason, _) = read_erc_report(&path).expect_err("invalid JSON is not evidence");
        assert_eq!(reason, UnknownReason::ReportMalformed);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn error_severity_drives_the_evidence_status() {
        let json: serde_json::Value = serde_json::from_str(KICAD10_NESTED).unwrap();
        let violations = parse_erc_report(&json).unwrap();
        let errors = violations.iter().filter(|v| v.is_error()).count();
        let evidence = NativeCheckEvidence::concluded(ERC_STAGE, "kicad-cli", vec![], errors);
        assert!(
            evidence.is_trusted(),
            "a warning-only report must not fail the gate"
        );
    }
}
