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

/// One error-level finding as `validate --format json` reports it.
struct Finding {
    code: String,
    found: String,
    expected: String,
    message: String,
    start: usize,
    end: usize,
    fixes: usize,
}

fn findings_and_source() -> (Vec<Finding>, String) {
    let root = repo_root();
    let board = root
        .join("qualification/class-a/negative-broken-mcu-board")
        .join("negative_broken_mcu_board.synth");
    let source = std::fs::read_to_string(&board).expect("read the board source");
    let out = Command::new(SYNTH)
        .args(["validate", "--format", "json"])
        .arg(&board)
        .current_dir(&root)
        .output()
        .expect("run synth validate");
    let payload: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("validate --format json emits json");
    let findings = payload["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter(|d| matches!(d["severity"].as_str(), Some("error" | "fatal")))
        .map(|d| Finding {
            code: d["code"].as_str().unwrap_or_default().to_string(),
            found: d["found"].as_str().unwrap_or_default().to_string(),
            expected: d["expected"].as_str().unwrap_or_default().to_string(),
            message: d["message"].as_str().unwrap_or_default().to_string(),
            start: usize::try_from(d["location"]["byte_start"].as_u64().expect("byte_start"))
                .expect("fits"),
            end: usize::try_from(d["location"]["byte_end"].as_u64().expect("byte_end"))
                .expect("fits"),
            fixes: d["suggested_fixes"].as_array().map_or(0, Vec::len),
        })
        .collect();
    (findings, source)
}

/// The code alone could survive a regression that stopped naming the part. Each
/// finding has to say which pin or designator is wrong, and point at the source
/// text of the declaration that has to change.
#[test]
fn every_finding_names_its_component_and_points_at_its_declaration() {
    let (findings, source) = findings_and_source();
    let at = |f: &Finding| source[f.start..f.end].to_string();

    for f in &findings {
        assert!(
            !f.found.is_empty() && !f.expected.is_empty(),
            "{} must say what was found and what was expected",
            f.code
        );
        assert!(f.end > f.start, "{} has an empty location", f.code);
    }

    // Both required pins are named, and both sit on U1's declaration.
    let floating: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.code == "E-SYNTH-CONNECT-001")
        .collect();
    for pin in ["U1.vss", "U1.nrst"] {
        let finding = floating
            .iter()
            .find(|f| f.found.contains(&format!("`{pin}`")))
            .unwrap_or_else(|| {
                panic!(
                    "no finding names {pin}: {floating:?}",
                    floating = floating.iter().map(|f| &f.found).collect::<Vec<_>>()
                )
            });
        assert!(
            finding.expected.contains(&format!("`{pin}`")),
            "the expectation for {pin} must name it too: {}",
            finding.expected
        );
        assert!(
            at(finding).starts_with("component U1"),
            "{pin}'s finding must point at U1's declaration, not {:?}",
            at(finding)
        );
    }

    // C1 is declared twice, and the two findings sit on the two declarations.
    let duplicates: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.code == "E-SYNTH-NAME-001")
        .collect();
    assert_eq!(duplicates.len(), 2, "one finding per declaration of C1");
    for f in &duplicates {
        assert!(f.found.contains("`C1`"), "names C1: {}", f.found);
        assert!(
            at(f).starts_with("component C1"),
            "points at a declaration of C1, not {:?}",
            at(f)
        );
    }
    assert_ne!(
        duplicates[0].start, duplicates[1].start,
        "the two findings must be on different declarations"
    );

    // R1 is named, points at its declaration, and offers a repair.
    let value = findings
        .iter()
        .find(|f| f.code == "E-SYNTH-VALUE-001")
        .expect("the missing value is reported");
    assert!(at(value).starts_with("component R1"), "{:?}", at(value));
    assert!(value.expected.contains("`R1`"), "{}", value.expected);
    assert!(value.message.contains("R1"), "{}", value.message);
    assert!(
        value.fixes >= 1,
        "a missing value comes with a suggested fix"
    );
}
