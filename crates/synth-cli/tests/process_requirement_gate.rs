// SPDX-License-Identifier: Apache-2.0

//! A declared fabrication process the named profile does not offer must stop
//! the run, on every path that reaches the router.
//!
//! `synth export-kicad` runs ERC, which already refuses this. `synth route`
//! and `synth drc` build their request without running ERC, so
//! `E-SYNTH-PROCESS-002` is not guaranteed to have been emitted upstream and
//! those are the paths these tests drive. The geometry gate downstream is not
//! a substitute: it refuses a via that lands in a pad, and a board can declare
//! `requires via_in_pad` and then route without needing one, so the
//! requirement would be granted by luck.

#![cfg(unix)]

mod stub_router;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

fn scratch(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("synth_process_req_{}_{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Two resistors on `jlc-standard`, which does not fill a via, declaring that
/// it needs one filled. Nothing here will route into a pad, so the geometry
/// gate has nothing to catch: the requirement is the only thing wrong.
fn board_source() -> &'static str {
    r#"board "needs_filled_via" {
  layers 2
  manufacturer "jlcpcb"
  requires via_in_pad

  component R1: resistor "r_generic_0603" value "10k"
  component R2: resistor "r_generic_0603" value "10k"

  connect R1.p1 -> R2.p1
  connect R1.p2 -> R2.p2
}
"#
}

fn write_board(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("needs_filled_via.synth");
    fs::write(&path, board_source()).expect("write board");
    path
}

#[test]
fn an_unmet_process_requirement_refuses_the_route() {
    let dir = scratch("refuses");
    let board = write_board(&dir);
    let out = Command::new(SYNTH)
        .args(["route", board.to_str().unwrap()])
        .output()
        .expect("run synth");

    assert!(
        !out.status.success(),
        "a board requiring a process the profile does not offer must not route"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("E-SYNTH-PROCESS-002"),
        "the refusal must name the diagnostic; got:\n{err}"
    );
    assert!(
        err.contains("via_in_pad"),
        "the refusal must name the requirement; got:\n{err}"
    );
    assert!(
        err.contains("--krt-allow-via-in-pad"),
        "the refusal must name the override that proceeds anyway; got:\n{err}"
    );
}

#[test]
fn the_operator_override_proceeds_and_still_reports_it_unmet() {
    let dir = scratch("override");
    let board = write_board(&dir);
    let out = stub_router::synth_with_stub_router(
        SYNTH,
        &dir,
        &["route", board.to_str().unwrap(), "--krt-allow-via-in-pad"],
    );

    let err = String::from_utf8_lossy(&out.stderr);
    // The override is a promise about the panel, not a claim that the profile
    // changed, so the requirement is still reported unmet.
    assert!(
        err.contains("E-SYNTH-PROCESS-002"),
        "the override must still report the requirement unmet; got:\n{err}"
    );
    assert!(
        !err.contains("--krt-allow-via-in-pad to"),
        "with the override given, the refusal must not be printed; got:\n{err}"
    );
}
