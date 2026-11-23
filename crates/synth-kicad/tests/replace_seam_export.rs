// SPDX-License-Identifier: Apache-2.0

//! The export seam is opt-in: unset and `SYNTH_PLACE_REPLACE=0` must write the
//! same PCB, and an existing placement sidecar must keep the default placement
//! even when the seam is on.
//!
//! The seam is read from the process environment, which edition 2024 makes
//! unsafe to set, so each variant exports in a child process.

use std::path::{Path, PathBuf};

const SOURCE: &str = r#"board "replace_seam" {
  layers 2

  component U1: mcu "rp2350"
  component HL1: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HR1: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }

  connect U1.gp0 -> HL1.p1
  connect U1.gp1 -> HL1.p2
  connect U1.gp0 -> HR1.p1
  connect U1.gp1 -> HR1.p2
}
"#;

/// A hard `near` hint vetoes the RePlAce solve, so the seam falls back while
/// the board still has real nets to place around.
const FALLBACK_SOURCE: &str = r#"board "replace_fallback" {
  layers 2

  component U1: mcu "rp2350"
  component R1: resistor "r_generic_0603" {
    placement_hint { near: "U1" priority: hard }
  }
  component HL1: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HR1: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }

  connect U1.gp0 -> HL1.p1
  connect U1.gp1 -> HL1.p2
  connect U1.gp0 -> HR1.p1
  connect U1.gp1 -> HR1.p2
}
"#;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn lower_source(source: &str, file: &str) -> synth_ir::Board {
    let registry =
        synth_registry::load_dir(&root().join("registry").join("parts")).expect("registry");
    let parsed = synth_parser::parse(source, file);
    synth_ir::lower(&parsed.ast.expect("ast"), &registry, file)
        .board
        .expect("board")
}

fn board() -> synth_ir::Board {
    lower_source(SOURCE, "replace_seam.synth")
}

fn fallback_board() -> synth_ir::Board {
    lower_source(FALLBACK_SOURCE, "replace_fallback.synth")
}

fn export_pcb(dir: &Path, sidecar: Option<&Path>) -> Vec<u8> {
    let sidecars = synth_kicad::Sidecars {
        schematic: None,
        placement: sidecar.map(Path::to_path_buf),
    };
    let board = if std::env::var_os("SYNTH_REPLACE_EXPORT_FALLBACK").is_some() {
        fallback_board()
    } else {
        board()
    };
    let result = synth_kicad::export_with_sidecars(&board, dir, &sidecars).expect("export");
    std::fs::read(result.pcb_path).expect("pcb bytes")
}

/// The same export with the default placement, whatever the seam says.
fn export_default_pcb(board: &synth_ir::Board, dir: &Path, sidecar: Option<&Path>) -> Vec<u8> {
    let sidecars = synth_kicad::Sidecars {
        schematic: None,
        placement: sidecar.map(Path::to_path_buf),
    };
    let result =
        synth_kicad::export_with_sidecars_default(board, dir, &sidecars).expect("default export");
    std::fs::read(result.pcb_path).expect("pcb bytes")
}

fn work_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "synth-replace-export-{name}-{}",
        std::process::id()
    ))
}

/// Run `test` in this binary with `value` for `SYNTH_PLACE_REPLACE` and return
/// the PCB it wrote. `None` removes the variable, so the "unset" run does not
/// inherit whatever the test process happens to have.
fn export_in_child(
    test: &str,
    name: &str,
    value: Option<&str>,
    sidecar: Option<&Path>,
    extra: &[(&str, &str)],
) -> Vec<u8> {
    let dir = work_dir(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", test, "--nocapture"])
        .env("SYNTH_REPLACE_EXPORT_CHILD", "1")
        .env("SYNTH_REPLACE_EXPORT_DIR", &dir);
    for (key, extra_value) in extra {
        command.env(key, extra_value);
    }
    match value {
        Some(value) => command.env("SYNTH_PLACE_REPLACE", value),
        None => command.env_remove("SYNTH_PLACE_REPLACE"),
    };
    if let Some(sidecar) = sidecar {
        command.env("SYNTH_REPLACE_EXPORT_SIDECAR", sidecar);
    }
    let out = command.output().expect("child export");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(dir.join("child.pcb")).expect("child pcb");
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

fn child_args() -> Option<(PathBuf, Option<PathBuf>)> {
    std::env::var_os("SYNTH_REPLACE_EXPORT_CHILD")?;
    let dir = std::env::var_os("SYNTH_REPLACE_EXPORT_DIR").map(PathBuf::from)?;
    let sidecar = std::env::var_os("SYNTH_REPLACE_EXPORT_SIDECAR").map(PathBuf::from);
    Some((dir, sidecar))
}

#[test]
fn replace_zero_is_byte_identical_to_unset() {
    if let Some((dir, _)) = child_args() {
        let bytes = export_pcb(&dir, None);
        std::fs::write(dir.join("child.pcb"), bytes).expect("child pcb write");
        return;
    }

    let unset = export_in_child(
        "replace_zero_is_byte_identical_to_unset",
        "unset",
        None,
        None,
        &[],
    );
    let zero = export_in_child(
        "replace_zero_is_byte_identical_to_unset",
        "zero",
        Some("0"),
        None,
        &[],
    );
    assert_eq!(
        unset, zero,
        "SYNTH_PLACE_REPLACE=0 changed the exported PCB"
    );
}

#[test]
fn a_placement_sidecar_wins_over_the_replace_seam() {
    if let Some((dir, sidecar)) = child_args() {
        let bytes = export_pcb(&dir, sidecar.as_deref());
        std::fs::write(dir.join("child.pcb"), bytes).expect("child pcb write");
        return;
    }

    let dir = work_dir("sidecar-file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let sidecar = dir.join("board.placement.layout.toml");
    std::fs::write(
        &sidecar,
        "schema_version = 2\n\n[components.U1]\nx = 31.0\ny = 13.0\nrotation = 90\nsource = \"human_drag\"\npriority = \"hard\"\n",
    )
    .expect("sidecar");
    let plain = export_in_child(
        "a_placement_sidecar_wins_over_the_replace_seam",
        "sidecar-default",
        None,
        Some(&sidecar),
        &[],
    );
    let replaced = export_in_child(
        "a_placement_sidecar_wins_over_the_replace_seam",
        "sidecar-on",
        Some("1"),
        Some(&sidecar),
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        plain, replaced,
        "SYNTH_PLACE_REPLACE=1 ignored an existing placement sidecar"
    );
}

/// A guard rejection must not be finished as if it were the solve: the export
/// takes the same default placement it takes when the seam is off.
#[test]
fn a_replace_fallback_exports_the_default_placement() {
    if let Some((dir, _)) = child_args() {
        let bytes = export_pcb(&dir, None);
        std::fs::write(dir.join("child.pcb"), bytes).expect("child pcb write");
        return;
    }

    let dir = work_dir("fallback-default");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let board = fallback_board();
    let default = export_default_pcb(&board, &dir, None);
    let replaced = export_in_child(
        "a_replace_fallback_exports_the_default_placement",
        "fallback-on",
        Some("1"),
        None,
        &[("SYNTH_REPLACE_EXPORT_FALLBACK", "1")],
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        replaced, default,
        "a RePlAce fallback must export the default placement"
    );
}
