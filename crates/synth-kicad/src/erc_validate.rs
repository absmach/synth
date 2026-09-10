// SPDX-License-Identifier: Apache-2.0

//! KiCad schematic Electrical Rules Check (ERC) integration via `kicad-cli`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Monotonic counter so concurrent ERC runs in one process write to
/// distinct temp report paths (a process-id-only name raced when the
/// golden ERC tests ran in parallel threads, both removing/reading the
/// same file).
static ERC_RUN_SEQ: AtomicU64 = AtomicU64::new(0);

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

#[derive(Debug, Clone, Deserialize)]
struct KicadErcReport {
    /// KiCad 10 nests violations under per-sheet entries
    /// (`sheets[].violations`) — see `resources/schemas/erc.v1.json`
    /// in the KiCad source.
    #[serde(default)]
    pub sheets: Vec<KicadErcSheet>,
}

#[derive(Debug, Clone, Deserialize)]
struct KicadErcSheet {
    #[serde(default)]
    pub violations: Vec<KicadErcViolation>,
}

#[derive(Debug, Error)]
pub enum ErcRunError {
    #[error("kicad-cli command not found: {source}")]
    NotInstalled {
        #[source]
        source: std::io::Error,
    },
    #[error("kicad-cli returned failure code ({code}): {stderr}")]
    CommandFailed { code: i32, stderr: String },
    #[error("could not read output file {path:?}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse ERC JSON report: {source}")]
    ParseOutput {
        #[source]
        source: serde_json::Error,
    },
}

/// Shell out to `kicad-cli sch erc` to run KiCad's internal schematic ERC.
/// Returns the list of violations parsed from the JSON report.
#[allow(clippy::missing_panics_doc)]
pub fn run_kicad_erc(sch_path: &Path) -> Result<Vec<KicadErcViolation>, ErcRunError> {
    let out_dir = std::env::temp_dir();
    let unique_id = ERC_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_path = out_dir.join(format!(
        "synth_kicad_erc_{}_{unique_id}.json",
        std::process::id()
    ));

    let output = Command::new("kicad-cli")
        .args([
            "sch",
            "erc",
            "--format",
            "json",
            "--output",
            out_path.to_str().unwrap(),
            "--severity-all",
            sch_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|source| ErcRunError::NotInstalled { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        let _ = std::fs::remove_file(&out_path);
        return Err(ErcRunError::CommandFailed { code, stderr });
    }

    let content = std::fs::read_to_string(&out_path).map_err(|source| ErcRunError::ReadFile {
        path: out_path.clone(),
        source,
    })?;

    let _ = std::fs::remove_file(&out_path);

    let report: KicadErcReport =
        serde_json::from_str(&content).map_err(|source| ErcRunError::ParseOutput { source })?;

    Ok(report
        .sheets
        .into_iter()
        .flat_map(|s| s.violations)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kicad_erc_json_structure() {
        let sample_json = r#"{
            "sheets": [
                {
                    "path": "/",
                    "uuid_path": "/abc",
                    "violations": [
                        {
                            "type": "endpoint_off_grid",
                            "severity": "warning",
                            "description": "Symbol pin or wire end off connection grid",
                            "items": [
                                {"description": "Symbol U1 Pin 1"}
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let report: KicadErcReport = serde_json::from_str(sample_json).unwrap();
        let violations: Vec<_> = report
            .sheets
            .into_iter()
            .flat_map(|s| s.violations)
            .collect();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].violation_type, "endpoint_off_grid");
        assert_eq!(violations[0].severity, "warning");
        assert_eq!(violations[0].items[0].description, "Symbol U1 Pin 1");
    }
}
