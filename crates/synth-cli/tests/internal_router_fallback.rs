// SPDX-License-Identifier: Apache-2.0

//! `synth drc` with no routing engine installed (issue #95).
//!
//! The gate used to fail on every board on a machine with no external
//! router: the DRC stage reported `router_unavailable` and no counts. The
//! stage now falls back to the workspace's own deterministic router, so
//! every board ends in a real verdict — and the `SYNTH_QUALIFY_ROUTER` pin
//! still disables the fallback, so a pinned qualification run gets the
//! pinned engine or an honest unknown.

#![cfg(unix)]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

/// A small board the workspace's maze router completes deterministically:
/// three passives, three nets, no fine-pitch package. (A QFN needs the
/// external engine's escape stage, so its internal route is honestly a
/// `validation_failed` rather than a fixture for a passing verdict.)
const RC_BOARD: &str = r#"board "rc_fallback" {
  layers 2
  component R1: resistor "r_generic_0603" value "10k"
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  connect R1.p1 -> C1.p1
  connect R1.p2 -> C2.p1
  connect C1.p2 -> C2.p2
}
"#;

/// The board that started #98: no components, no nets.
const HELLO_BOARD: &str = r#"board "hello" { layers 2 }
"#;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "synth_internal_route_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Write `source` into the scratch tree and return its path.
fn design(label: &str, source: &str) -> PathBuf {
    let dir = scratch(label);
    let path = dir.join("board.synth");
    std::fs::write(&path, source).expect("write design");
    path
}

/// A `kicad-cli` that reports a clean DRC: the internal route's native check.
fn stub_kicad_cli(dir: &Path) -> PathBuf {
    let path = dir.join("kicad-cli");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = version ]; then echo 10.0.1; exit 0; fi
prev=""
out=""
for a in "$@"; do
  if [ "$prev" = "--output" ]; then out="$a"; fi
  prev="$a"
done
case "$1 $2" in
  "pcb drc")
    printf '{"kicad_version":"10.0.1","violations":[],"unconnected_items":[]}' > "$out"
    exit 0 ;;
esac
exit 0"#,
    )
    .expect("write stub kicad-cli");
    let mut perms = std::fs::metadata(&path).expect("stat stub").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        perms.set_mode(0o755);
    }
    std::fs::set_permissions(&path, perms).expect("chmod stub");
    path
}

/// Run `synth drc --pretty` with every routing engine hidden, and parse the
/// run record.
///
/// The stub `kicad-cli` answers the native DRC the internal route's
/// validation runs; the routing environment is cleared and both KiCadRoutingTools
/// locations are pointed at empty directories, so no engine — real or
/// configured on the machine running the tests — can be found.
fn run_drc(
    label: &str,
    design_path: &Path,
    envs: &[(&str, &str)],
) -> (Option<i32>, String, serde_json::Value) {
    let dir = scratch(&format!("{label}_run"));
    let cli = stub_kicad_cli(&dir);
    // Empty trees that hide a real KiCadRoutingTools install from both the
    // configured location and the conventional one discovery falls back to.
    let no_krt = dir.join("no-krt");
    std::fs::create_dir_all(&no_krt).expect("scratch krt dir");
    let mut command = Command::new(SYNTH);
    command
        .args(["drc", "--pretty", design_path.to_string_lossy().as_ref()])
        .env("KICAD_CLI", cli)
        .env_remove("SYNTH_FREEROUTING_JAR")
        .env_remove("SYNTH_FREEROUTING_JAVA")
        .env("KICAD_ROUTING_TOOLS_REPO", no_krt.join("absent"))
        .env("XDG_DATA_HOME", &no_krt)
        .env_remove("SYNTH_KRT_PYTHON")
        .env("SYNTH_ROUTER_TIMEOUT_SECS", "60");
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut output = command.output().expect("run synth drc");
    let mut text = String::new();
    std::io::Cursor::new(&mut output.stdout)
        .read_to_string(&mut text)
        .expect("read stdout");
    let report = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        report,
    )
}

#[test]
fn a_board_with_no_engine_installed_gets_a_real_drc_verdict() {
    let design_path = design("rc", RC_BOARD);
    let (code, stderr, report) = run_drc("rc", &design_path, &[]);
    assert_eq!(report["state"], "routed", "stderr:\n{stderr}");
    assert_eq!(
        report["engine"], "internal",
        "the record names the engine that produced the copper"
    );
    assert_eq!(
        report["provenance"]["engine_version"],
        env!("CARGO_PKG_VERSION"),
        "the workspace's router is versioned like any engine"
    );
    let counts = report["validation"]["kicad_drc"]
        .as_object()
        .expect("the fallback reports DRC counts, not unknown");
    assert_eq!(counts["errors"], 0, "{report:#}");
    assert_eq!(counts["unconnected"], 0, "{report:#}");
    assert_eq!(report["statistics"]["open_nets"], 0, "{report:#}");
    assert_eq!(code, Some(0), "a routed, clean board passes: {stderr}");
}

#[test]
fn the_empty_board_routes_trivially_and_passes() {
    // `hello` is a board with no components and no nets: it must route
    // trivially and report a real pass, not `router_unavailable` (#98a).
    let design_path = design("hello", HELLO_BOARD);
    let (code, stderr, report) = run_drc("hello", &design_path, &[]);
    assert_eq!(report["state"], "routed", "stderr:\n{stderr}");
    assert_eq!(
        report["statistics"]["segments"], 0,
        "an empty board carries no copper: {report:#}"
    );
    assert_eq!(report["validation"]["kicad_drc"]["errors"], 0, "{report:#}");
    assert_eq!(code, Some(0), "stderr:\n{stderr}");
}

#[test]
fn a_pinned_router_is_not_replaced_by_the_fallback() {
    // With the qualification pin set, the run asks for the pinned engine
    // and reports it honestly missing instead of routing internally.
    let design_path = design("pinned", RC_BOARD);
    let (code, stderr, report) = run_drc(
        "pinned_missing",
        &design_path,
        &[("SYNTH_QUALIFY_ROUTER", "kicad-routing-tools")],
    );
    assert_eq!(report["state"], "router_unavailable", "stderr:\n{stderr}");
    assert_eq!(
        report["engine"], "kicad-routing-tools",
        "the pinned engine is named even though it never ran"
    );
    assert_eq!(
        report["failure"]["reason"], "checkout_missing",
        "the failure names what is missing: {report:#}"
    );
    assert_eq!(
        code,
        Some(1),
        "an unavailable engine is not a pass: {stderr}"
    );
}

#[test]
fn an_internal_pin_routes_internally_without_probing_an_engine() {
    // The pin may also select the internal router deliberately.
    let design_path = design("pinned_internal", HELLO_BOARD);
    let (code, stderr, report) = run_drc(
        "pinned_internal",
        &design_path,
        &[("SYNTH_QUALIFY_ROUTER", "internal")],
    );
    assert_eq!(report["state"], "routed", "stderr:\n{stderr}");
    assert_eq!(report["engine"], "internal", "{report:#}");
    assert_eq!(code, Some(0), "stderr:\n{stderr}");
}
