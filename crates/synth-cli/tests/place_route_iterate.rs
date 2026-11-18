// SPDX-License-Identifier: Apache-2.0

//! `synth export-kicad --place-route-iterate`: the place→route→re-place loop.
//!
//! The loop routes each candidate placement with the external engine and keeps
//! the one that routes best. These tests drive it through the stub router, so
//! they measure the loop and not the engine.

#![cfg(unix)]

mod stub_router;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

fn scratch(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("synth_place_route_{}_{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A component-rich, ERC-clean, hint-free design: enough parts that the
/// placer's density margin produces genuinely different arrangements, so the
/// loop has more than one candidate to route.
///
/// Resistors in a ring: every pin is connected (so no floating-input ERC),
/// every value is set (so no `E-SYNTH-VALUE-001`), and there are no power pins
/// to leave unconnected.
fn ring_source() -> String {
    use std::fmt::Write as _;
    let mut src = String::from("board \"ring\" {\n  layers 2\n");
    for index in 1..=16 {
        let _ = writeln!(
            src,
            "  component R{index}: resistor \"r_generic_0603\" value \"10k\""
        );
    }
    for index in 1..=16 {
        let next = index % 16 + 1;
        let _ = writeln!(src, "  connect R{index}.p2 -> R{next}.p1");
    }
    src.push_str("}\n");
    src
}

fn run_export(
    design: &Path,
    out: &Path,
    extra: &[&str],
    router: &stub_router::StubRouter,
) -> std::process::Output {
    Command::new(SYNTH)
        .envs(router.envs())
        .arg("export-kicad")
        .arg(design)
        .arg("--out")
        .arg(out)
        .arg("--router")
        .arg("free-routing")
        .args(extra)
        .output()
        .expect("run synth export-kicad")
}

#[test]
fn the_loop_routes_the_candidates_and_still_exports_the_winner() {
    let dir = scratch("on");
    let router = stub_router::install(&dir);

    let design = dir.join("ring.synth");
    fs::write(&design, ring_source()).expect("write design");
    let out = dir.join("out");

    // The stub's straight-line copper does not pass KiCad DRC, so the gate
    // rightly fails and the command exits non-zero. What this test measures is
    // the loop, not the stub's routing quality: it must route every candidate
    // and still produce a run record.
    let output = run_export(&design, &out, &["--place-route-iterate"], &router);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("placement feedback: routing") && stderr.contains("candidate"),
        "the loop must report the candidates it routed:\n{stderr}"
    );
    assert!(
        out.join("ring.routing.json").is_file(),
        "the loop must still produce a routing run record"
    );
    assert!(
        out.join("ring.synth.kicad_pcb").is_file(),
        "the un-routed baseline must be preserved"
    );
}

#[test]
fn without_the_flag_the_loop_does_not_run() {
    let dir = scratch("off");
    let router = stub_router::install(&dir);

    let design = dir.join("ring.synth");
    fs::write(&design, ring_source()).expect("write design");
    let out = dir.join("out");

    let output = run_export(&design, &out, &[], &router);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("placement feedback"),
        "the loop is opt-in and must not run by default:\n{stderr}"
    );
    assert!(
        out.join("ring.routing.json").is_file(),
        "the normal single route must still run"
    );
}
