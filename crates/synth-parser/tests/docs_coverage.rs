// SPDX-License-Identifier: Apache-2.0

//! Verification that every emitted diagnostic code has a corresponding markdown doc.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[test]
fn all_emitted_parse_diagnostic_codes_have_documentation() {
    let root = workspace_root();
    let src_dir = root.join("crates").join("synth-parser").join("src");
    let docs_dir = root.join("docs").join("diagnostics");

    let mut emitted_codes = BTreeSet::new();

    // Scan all .rs files in synth-parser/src
    for entry in fs::read_dir(&src_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let content = fs::read_to_string(&path).unwrap();
            for line in content.lines() {
                let mut rest = line;
                while let Some(start) = rest.find("E-SYNTH-") {
                    let candidate = &rest[start..];
                    // Slice until quote or non-alphanumeric/hyphen
                    let end = candidate
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                        .unwrap_or(candidate.len());
                    let code = &candidate[..end];
                    if code.starts_with("E-SYNTH-PARSE-") {
                        emitted_codes.insert(code.to_string());
                    }
                    rest = &candidate[end..];
                }
            }
        }
    }

    assert!(
        !emitted_codes.is_empty(),
        "Expected to find emitted E-SYNTH-PARSE-* codes in synth-parser source files"
    );

    let mut missing_docs = Vec::new();
    for code in &emitted_codes {
        let doc_path = docs_dir.join(format!("{code}.md"));
        if doc_path.exists() {
            let content = fs::read_to_string(&doc_path).unwrap();
            if content.trim().is_empty() {
                missing_docs.push(format!("{code} (empty file: {})", doc_path.display()));
            }
        } else {
            missing_docs.push(format!("{code} (missing file: {})", doc_path.display()));
        }
    }

    assert!(
        missing_docs.is_empty(),
        "Diagnostic codes missing documentation:\n{missing_docs:#?}"
    );
}
