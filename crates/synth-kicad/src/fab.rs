// SPDX-License-Identifier: Apache-2.0

//! Manufacturing artifact export — Gerber, drill, and STEP.
//!
//! Phase 9 slice 3. Per plan §11.2 ("we do not write Gerbers
//! by hand for V1") and §11.0 slice 3, this module shells out
//! to `kicad-cli pcb export {gerbers,drill,step}` against the
//! `<board>.kicad_pcb` file the synchronous exporter writes.
//!
//! Determinism caveat: the IR-driven `.kicad_pcb` output stays
//! byte-deterministic across runs (see `pcb.rs`), but
//! `kicad-cli` injects per-run timestamps into Gerber headers.
//! For V1 we accept the timestamp churn as the trade for not
//! maintaining a hand-rolled Gerber writer. A post-V1 slice may
//! either patch the headers or build a deterministic writer.
//!
//! Subprocess hardening per plan §12.2 is deliberately out of
//! scope here — the caller is responsible for any `nice` /
//! `ulimit` wrapping. The fab module assumes a trusted host.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// Which artifacts the caller wants produced. All optional;
/// the CLI exposes one flag per field. A request with every
/// field `false` is a valid no-op (returns empty artifacts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FabRequest {
    pub gerbers: bool,
    pub drill: bool,
    pub step: bool,
}

impl FabRequest {
    /// True when no artifact was requested. Callers use this
    /// to skip the entire fab subprocess path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.gerbers || self.drill || self.step)
    }
}

/// Paths to whatever artifacts the request asked for. Each
/// field is `Some` only when the matching `FabRequest` flag
/// was set and the subprocess succeeded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FabArtifacts {
    /// Directory containing one `<name>-<layer>.gbr` per
    /// plotted layer, plus a `<name>-job.gbrjob` summary.
    pub gerbers_dir: Option<PathBuf>,
    /// Directory containing `<name>-PTH.drl`, `<name>-NPTH.drl`,
    /// and a `<name>-drl_map.pdf` summary.
    pub drill_dir: Option<PathBuf>,
    /// `<name>.step`.
    pub step_path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum FabError {
    #[error("could not create output directory {path:?}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "kicad-cli not found in PATH (install KiCad 8+ or set the KICAD_CLI environment variable)"
    )]
    NotFound,
    #[error("failed to spawn kicad-cli {subcommand}: {source}")]
    Spawn {
        subcommand: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("kicad-cli {subcommand} exited with status {code}: {stderr}")]
    Subprocess {
        subcommand: &'static str,
        code: i32,
        stderr: String,
    },
}

/// Run every requested artifact path against `pcb_path`,
/// writing into subdirectories of `out_dir`. Failures short-
/// circuit: the first failing subprocess returns immediately
/// without attempting the remaining artifacts.
pub fn run(pcb_path: &Path, out_dir: &Path, req: &FabRequest) -> Result<FabArtifacts, FabError> {
    let mut artifacts = FabArtifacts::default();
    if req.is_empty() {
        return Ok(artifacts);
    }
    let stem = pcb_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("board");

    if req.gerbers {
        let dir = out_dir.join("gerbers");
        create_dir(&dir)?;
        run_kicad_cli("gerbers", &gerbers_args(pcb_path, &dir))?;
        artifacts.gerbers_dir = Some(dir);
    }
    if req.drill {
        let dir = out_dir.join("drill");
        create_dir(&dir)?;
        run_kicad_cli("drill", &drill_args(pcb_path, &dir))?;
        artifacts.drill_dir = Some(dir);
    }
    if req.step {
        let path = out_dir.join(format!("{stem}.step"));
        run_kicad_cli("step", &step_args(pcb_path, &path))?;
        artifacts.step_path = Some(path);
    }
    Ok(artifacts)
}

fn create_dir(path: &Path) -> Result<(), FabError> {
    std::fs::create_dir_all(path).map_err(|source| FabError::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}

/// Resolve the `kicad-cli` binary. The `KICAD_CLI` env var lets
/// CI pin a specific build; otherwise we trust `PATH`.
fn kicad_cli_binary() -> String {
    std::env::var("KICAD_CLI").unwrap_or_else(|_| "kicad-cli".to_string())
}

fn run_kicad_cli(subcommand: &'static str, args: &[String]) -> Result<(), FabError> {
    let binary = kicad_cli_binary();
    let mut cmd = Command::new(&binary);
    cmd.arg("pcb").arg("export").arg(subcommand);
    cmd.args(args);
    let output = cmd.output().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            FabError::NotFound
        } else {
            FabError::Spawn { subcommand, source }
        }
    })?;
    if !output.status.success() {
        return Err(FabError::Subprocess {
            subcommand,
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

/// Argument vector for `kicad-cli pcb export gerbers`. Pinned
/// flags worth justifying:
///
/// - `--no-protel-ext` standardizes filenames on `.gbr`, instead
///   of the legacy Protel per-layer extensions (`.gbl`, `.gtl`,
///   ...). JLC accepts either, but the consistent extension
///   makes downstream tooling simpler.
/// - `--subtract-soldermask` clips silk by the mask aperture so
///   silkscreen never spills onto exposed copper — what JLC
///   expects on a clean submission.
pub(crate) fn gerbers_args(pcb_path: &Path, out_dir: &Path) -> Vec<String> {
    vec![
        "--output".to_string(),
        out_dir.to_string_lossy().into_owned(),
        "--no-protel-ext".to_string(),
        "--subtract-soldermask".to_string(),
        pcb_path.to_string_lossy().into_owned(),
    ]
}

/// Argument vector for `kicad-cli pcb export drill`. Pinned
/// flags worth justifying:
///
/// - `--excellon-separate-th` splits PTH from NPTH into two
///   files — JLC's automated check rejects combined drill files
///   on assembly orders.
/// - `--generate-map` emits the PDF drill map fab houses use
///   for visual verification.
pub(crate) fn drill_args(pcb_path: &Path, out_dir: &Path) -> Vec<String> {
    vec![
        "--output".to_string(),
        out_dir.to_string_lossy().into_owned(),
        "--format".to_string(),
        "excellon".to_string(),
        "--excellon-separate-th".to_string(),
        "--generate-map".to_string(),
        "--map-format".to_string(),
        "pdf".to_string(),
        pcb_path.to_string_lossy().into_owned(),
    ]
}

/// Argument vector for `kicad-cli pcb export step`. `--force`
/// overwrites any prior `.step` file at the target path so
/// repeat invocations succeed.
pub(crate) fn step_args(pcb_path: &Path, out_path: &Path) -> Vec<String> {
    vec![
        "--force".to_string(),
        "--output".to_string(),
        out_path.to_string_lossy().into_owned(),
        pcb_path.to_string_lossy().into_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_request_is_a_noop() {
        let req = FabRequest::default();
        assert!(req.is_empty());
        let artifacts = run(Path::new("/dev/null"), Path::new("/tmp"), &req).unwrap();
        assert_eq!(artifacts, FabArtifacts::default());
    }

    #[test]
    fn gerbers_args_pin_extension_and_mask_flag() {
        let args = gerbers_args(Path::new("/tmp/board.kicad_pcb"), Path::new("/tmp/gerbers"));
        assert!(args.contains(&"--no-protel-ext".to_string()));
        assert!(args.contains(&"--subtract-soldermask".to_string()));
        assert!(args.last().unwrap().ends_with("board.kicad_pcb"));
    }

    #[test]
    fn drill_args_split_pth_and_generate_map() {
        let args = drill_args(Path::new("/tmp/board.kicad_pcb"), Path::new("/tmp/drill"));
        assert!(args.contains(&"--excellon-separate-th".to_string()));
        assert!(args.contains(&"--generate-map".to_string()));
        let map_format = args
            .iter()
            .zip(args.iter().skip(1))
            .find(|(a, _)| *a == "--map-format")
            .map(|(_, b)| b.as_str());
        assert_eq!(map_format, Some("pdf"));
    }

    #[test]
    fn step_args_include_force_for_idempotent_runs() {
        let args = step_args(
            Path::new("/tmp/board.kicad_pcb"),
            Path::new("/tmp/board.step"),
        );
        assert!(args.contains(&"--force".to_string()));
        assert!(args.last().unwrap().ends_with("board.kicad_pcb"));
    }

    /// End-to-end test that actually runs `kicad-cli`. Off by
    /// default because CI doesn't have KiCad 8 installed yet;
    /// run locally with `cargo test -p synth-kicad fab -- --ignored`.
    #[test]
    #[ignore = "requires kicad-cli on PATH"]
    fn round_trip_against_sensor_logger() {
        let tmp = std::env::temp_dir().join("synth-fab-roundtrip");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
        let registry = synth_registry::load_dir(&registry_dir).expect("registry");
        let file = "../../examples/sensor_logger.synth".to_string();
        let source = std::fs::read_to_string(&file).expect("fixture");
        let parse = synth_parser::parse(&source, file.clone());
        let ast = parse.ast.as_ref().expect("parse");
        let loader = synth_ir::FsImportLoader {
            root: std::path::PathBuf::from("../.."),
        };
        let resolved = synth_ir::resolve_imports(ast, &loader, &file);
        let lowered = synth_ir::lower(&resolved.program, &registry, &file);
        let board = lowered.board.expect("board");

        let exported = crate::export(&board, &tmp).expect("export");
        let artifacts = run(
            &exported.pcb_path,
            &tmp,
            &FabRequest {
                gerbers: true,
                drill: true,
                step: true,
            },
        )
        .expect("fab");

        let gerbers_dir = artifacts.gerbers_dir.expect("gerbers dir");
        assert!(std::fs::read_dir(&gerbers_dir).unwrap().next().is_some());
        let drill_dir = artifacts.drill_dir.expect("drill dir");
        assert!(std::fs::read_dir(&drill_dir).unwrap().next().is_some());
        let step_path = artifacts.step_path.expect("step path");
        assert!(step_path.exists());
    }
}
