// SPDX-License-Identifier: Apache-2.0

//! Property-based fuzz harnesses for the parser pipeline.
//!
//! The invariant under test is the simplest, most important one: **no
//! input ever panics the compiler**. Bad input produces structured
//! diagnostics; it never crashes. Plan §3.5 calls for 1000 iterations
//! across three surfaces (lexer, parser, parser+IR); each test below
//! is configured for exactly that.
//!
//! These run as part of `cargo test`, so CI catches regressions
//! automatically on every PR. A future cargo-fuzz setup (under
//! `crates-internal/synth-fuzz/`) will use libFuzzer's coverage-
//! guided exploration on top of this foundation.

use proptest::prelude::*;

/// Random arbitrary string — covers ASCII, Unicode, and weird control
/// characters. `.*` is proptest's regex for any string of any length.
fn fuzzy_input() -> impl Strategy<Value = String> {
    ".*"
}

/// Random byte sequence — proptest occasionally produces invalid
/// UTF-8 inside the regex above; this gives us full byte coverage
/// for the byte-oriented lexer.
fn fuzzy_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..512)
}

// =============================================================================
// 1. Lexer
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// Lexer must never panic on any string input.
    #[test]
    fn lexer_never_panics_on_strings(input in fuzzy_input()) {
        let _ = synth_parser::lex(&input);
    }

    /// Lexer must never panic on any byte sequence interpreted as
    /// UTF-8 source (or lossily converted from raw bytes).
    #[test]
    fn lexer_never_panics_on_lossy_bytes(bytes in fuzzy_bytes()) {
        let s = String::from_utf8_lossy(&bytes);
        let _ = synth_parser::lex(&s);
    }
}

// =============================================================================
// 2. Parser
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// Parser must never panic on any string input.
    #[test]
    fn parser_never_panics(input in fuzzy_input()) {
        let _ = synth_parser::parse(&input, "fuzz.synth");
    }

    /// Parser must never panic on lossy-UTF-8 byte input either.
    #[test]
    fn parser_never_panics_on_lossy_bytes(bytes in fuzzy_bytes()) {
        let s = String::from_utf8_lossy(&bytes);
        let _ = synth_parser::parse(&s, "fuzz.synth");
    }
}

// =============================================================================
// 3. Parser + IR lowering
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// Parse + lower pipeline must never panic on any string input,
    /// even when the parser produces a partial AST and the registry
    /// is empty (so every component resolution fails).
    #[test]
    fn pipeline_never_panics(input in fuzzy_input()) {
        let parsed = synth_parser::parse(&input, "fuzz.synth");
        if let Some(ast) = parsed.ast.as_ref() {
            let registry = synth_registry::Registry::new();
            let _ = synth_ir::lower(ast, &registry, "fuzz.synth");
        }
    }
}

// =============================================================================
// 4. JSON Ingestion & AST Round-trip
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// JSON ingestion frontend must never panic on any string input.
    #[test]
    fn json_parser_never_panics(input in fuzzy_input()) {
        let _ = synth_parser::parse_json(&input, "fuzz.json");
    }
}

#[test]
fn json_ast_round_trip_for_all_fixtures() {
    use std::fs;
    use std::path::Path;

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
        .join("fixtures")
        .join("designs");

    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "synth") {
            let src = fs::read_to_string(&path).unwrap();
            let parsed = synth_parser::parse(&src, path.file_name().unwrap().to_string_lossy());
            assert!(
                !parsed.has_errors(),
                "DSL parse failed for {}",
                path.display()
            );
            let ast = parsed.ast.expect("missing AST");

            // Round trip: AST -> JSON -> parse_json -> AST
            let json_str = serde_json::to_string(&ast).expect("failed to serialize AST to JSON");
            let json_parsed = synth_parser::parse_json(&json_str, "roundtrip.json");
            assert!(
                !json_parsed.has_errors(),
                "JSON parse failed for {}",
                path.display()
            );
            let roundtripped_ast = json_parsed.ast.expect("missing AST from JSON parse");

            assert_eq!(
                ast,
                roundtripped_ast,
                "Round-trip mismatch for {}",
                path.display()
            );
        }
    }
}
