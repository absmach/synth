// SPDX-License-Identifier: Apache-2.0

//! Schematic SVG plotting via `kicad-cli sch export svg`.
//!
//! The SVG produced here is the input to Synth's visual-feedback loop:
//! [`crate::run_kicad_svg_export`] plots the exact sheet KiCad will draw,
//! and the caller rasterizes it (see the `synth-render` crate) so an agent
//! can inspect its own output. Rendering through KiCad rather than a
//! Synth-native renderer is deliberate — the agent must review the artifact
//! that is actually delivered, not an approximation of it.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SvgExportError {
    #[error("kicad-cli command not found: {source}")]
    NotInstalled {
        #[source]
        source: std::io::Error,
    },
    #[error("kicad-cli sch export svg failed ({code}): {stderr}")]
    CommandFailed { code: i32, stderr: String },
    #[error("could not read SVG output directory {dir:?}: {source}")]
    ReadDir {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("kicad-cli produced no SVG file in {dir:?}")]
    NoOutput { dir: PathBuf },
}

/// Plot every sheet of `sch_path` to SVG in `out_dir` and return the
/// produced files in a deterministic (name-sorted) order.
///
/// `out_dir` is created if absent. A multi-sheet schematic produces one SVG
/// per sheet; a single-sheet schematic produces exactly one.
pub fn run_kicad_svg_export(
    sch_path: &Path,
    out_dir: &Path,
) -> Result<Vec<PathBuf>, SvgExportError> {
    std::fs::create_dir_all(out_dir).map_err(|source| SvgExportError::ReadDir {
        dir: out_dir.to_path_buf(),
        source,
    })?;

    let output = Command::new("kicad-cli")
        .args(["sch", "export", "svg", "--output"])
        .arg(out_dir)
        .arg(sch_path)
        .output()
        .map_err(|source| SvgExportError::NotInstalled { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        return Err(SvgExportError::CommandFailed { code, stderr });
    }

    let entries = std::fs::read_dir(out_dir).map_err(|source| SvgExportError::ReadDir {
        dir: out_dir.to_path_buf(),
        source,
    })?;
    let mut svgs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")))
        .collect();
    svgs.sort();

    if svgs.is_empty() {
        return Err(SvgExportError::NoOutput {
            dir: out_dir.to_path_buf(),
        });
    }
    Ok(svgs)
}
