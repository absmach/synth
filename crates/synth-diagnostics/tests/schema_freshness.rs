// SPDX-License-Identifier: Apache-2.0

//! Freshness check for `schemas/diagnostic.schema.json`.
//!
//! The diagnostic protocol's JSON Schema is checked into the repo at
//! `schemas/diagnostic.schema.json` so external tooling can validate
//! `synth validate --format json` output without running the compiler.
//! This test regenerates the schema from the in-source types and
//! asserts that the checked-in file matches.
//!
//! If this test fails after intentional schema changes, regenerate the
//! file with:
//!
//! ```text
//! cargo run -p synth-cli -- schema diagnostic > schemas/diagnostic.schema.json
//! ```

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[test]
fn checked_in_schema_matches_derived_schema() {
    let path = workspace_root()
        .join("schemas")
        .join("diagnostic.schema.json");
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("expected {} to exist", path.display()));
    let checked_in: serde_json::Value =
        serde_json::from_str(&checked_in).expect("checked-in schema must parse as JSON");
    let derived = synth_diagnostics::diagnostic_schema();
    assert_eq!(
        derived, checked_in,
        "diagnostic schema drift detected; regenerate via \
         `cargo run -p synth-cli -- schema diagnostic > schemas/diagnostic.schema.json`"
    );
}
