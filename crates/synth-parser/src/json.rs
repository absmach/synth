// SPDX-License-Identifier: Apache-2.0

//! JSON ingestion path for Synth AST.
//!
//! Deserializes a JSON string into a [`ProgramAst`], allowing tools and
//! agents to pass structured JSON AST representations directly instead of
//! generating DSL code.

use synth_ast::ProgramAst;
use synth_diagnostics::{DiagnosticBuilder, Location, Severity, Span};

use crate::parser::ParseResult;

/// Parse a JSON string into a [`ProgramAst`].
///
/// Returns a [`ParseResult`] containing the deserialized AST on success,
/// or a diagnostic with code `E-SYNTH-PARSE-027` if the input is not valid JSON
/// or does not match the [`ProgramAst`] schema.
pub fn parse_json(source: &str, file: impl Into<String>) -> ParseResult {
    let file = file.into();
    match serde_json::from_str::<ProgramAst>(source) {
        Ok(ast) => ParseResult {
            ast: Some(ast),
            diagnostics: Vec::new(),
        },
        Err(err) => {
            let line = err.line() as u32;
            let col = err.column() as u32;
            let msg = err.to_string();

            let span = Span::new(0, source.len() as u32);
            let loc = Location {
                file: file.clone(),
                span,
                line_start: line,
                col_start: col,
                line_end: line,
                col_end: col,
            };

            let diagnostic = DiagnosticBuilder::new(
                "E-SYNTH-PARSE-027",
                Severity::Error,
                "invalid JSON or AST schema error",
            )
            .location(loc)
            .expected("valid JSON matching ProgramAst schema")
            .found(msg)
            .explanation_url("synth.docs/diagnostics/E-SYNTH-PARSE-027")
            .build();

            ParseResult {
                ast: None,
                diagnostics: vec![diagnostic],
            }
        }
    }
}
