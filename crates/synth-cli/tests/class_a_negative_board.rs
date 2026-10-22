// SPDX-License-Identifier: Apache-2.0

//! The Class-A negative board is only worth keeping if it keeps failing the
//! way its spec sheet says it does.
//!
//! Four defects are planted in
//! `qualification/class-a/negative-broken-mcu-board`, and the README
//! tabulates the five error-level diagnostics they produce. Nothing asserted
//! that, so a rule that stopped firing would have read as silence: the board
//! would still exit non-zero on the remaining defects and the missing finding
//! would not show up anywhere.
//!
//! The comparison is an exact multiset, in both directions. A code that
//! vanishes is the regression this test exists for; a code that appears means
//! the board acquired a defect nobody declared, and the README's table is
//! then wrong too.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const SYNTH: &str = env!("CARGO_BIN_EXE_synth");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The diagnostics the board's README tabulates, code to count.
fn declared() -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        // U1.vss and U1.nrst are both required and both left floating.
        ("E-SYNTH-CONNECT-001", 2),
        // C1 is declared twice; the duplicate is reported against each.
        ("E-SYNTH-NAME-001", 2),
        // R1 is a generic passive with no value.
        ("E-SYNTH-VALUE-001", 1),
    ])
}

#[test]
fn the_negative_board_reports_exactly_the_diagnostics_it_declares() {
    let root = repo_root();
    let board = root
        .join("qualification/class-a/negative-broken-mcu-board")
        .join("negative_broken_mcu_board.synth");
    assert!(
        board.is_file(),
        "board source missing at {}",
        board.display()
    );

    let out = Command::new(SYNTH)
        .args(["validate", "--format", "json"])
        .arg(&board)
        .current_dir(&root)
        .output()
        .expect("run synth validate");

    assert_eq!(
        out.status.code(),
        Some(1),
        "a board that is meant to fail must exit 1\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let payload: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("validate --format json emits json");
    let mut observed: BTreeMap<&str, usize> = BTreeMap::new();
    for d in payload["diagnostics"]
        .as_array()
        .expect("diagnostics array")
    {
        let severity = d["severity"].as_str().expect("severity");
        if severity != "error" && severity != "fatal" {
            continue;
        }
        let code = d["code"].as_str().expect("code");
        *observed.entry(code).or_default() += 1;
    }

    assert_eq!(
        observed,
        declared(),
        "the negative board's error-level diagnostics no longer match its README"
    );
}
