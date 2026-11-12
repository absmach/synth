// SPDX-License-Identifier: Apache-2.0

//! Every `E-SYNTH-SIM-*` code emitted by the crate has a docs page, and every
//! such page names a code the crate still emits.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

#[test]
fn every_sim_diagnostic_code_has_a_page_and_every_page_has_a_code() {
    let root = workspace_root();
    let src_dir = root.join("crates").join("synth-sim").join("src");
    let docs_dir = root.join("docs").join("diagnostics");

    let mut emitted = BTreeSet::new();
    for entry in fs::read_dir(&src_dir).expect("sim source dir").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let content = fs::read_to_string(&path).expect("read sim source");
            collect_codes(&content, &mut emitted);
        }
    }
    assert!(
        !emitted.is_empty(),
        "expected E-SYNTH-SIM-* codes in synth-sim source"
    );

    let mut documented = BTreeSet::new();
    for entry in fs::read_dir(&docs_dir)
        .expect("diagnostics docs dir")
        .flatten()
    {
        let path = entry.path();
        let Some(code) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".md"))
            .filter(|code| code.starts_with("E-SYNTH-SIM-"))
        else {
            continue;
        };
        let content = fs::read_to_string(&path).expect("read docs page");
        assert!(
            !content.trim().is_empty(),
            "empty docs page: {}",
            path.display()
        );
        documented.insert(code.to_string());
    }

    let missing: Vec<_> = emitted.difference(&documented).collect();
    let orphaned: Vec<_> = documented.difference(&emitted).collect();
    assert!(
        missing.is_empty(),
        "E-SYNTH-SIM codes with no docs page: {missing:?}"
    );
    assert!(
        orphaned.is_empty(),
        "docs pages for codes the crate does not emit: {orphaned:?}"
    );
}

fn collect_codes(content: &str, codes: &mut BTreeSet<String>) {
    let mut rest = content;
    while let Some(start) = rest.find("E-SYNTH-SIM-") {
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(candidate.len());
        codes.insert(candidate[..end].to_string());
        rest = &candidate[end..];
    }
}
