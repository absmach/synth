// SPDX-License-Identifier: Apache-2.0

//! Lexer and parser for SynthSpec.
//!
//! The frontend is split in two:
//!
//! - [`token`]: hand-rolled lexer producing a stream of [`Token`]s with
//!   byte-accurate spans, including string escapes, comments (line and
//!   block), and engineering unit suffixes (`20mm`, `90ohm`, ...).
//! - [`parser`]: recursive-descent parser consuming the token stream
//!   and producing a [`ProgramAst`](synth_ast::ProgramAst) plus zero
//!   or more [`Diagnostic`](synth_diagnostics::Diagnostic)s.
//!   Recovers to statement boundaries on errors so one mistake produces
//!   at most one cascade diagnostic per statement.
//!
//! Phase 1 grammar covers the PRD Chapter 25 core:
//!
//! ```text
//! program     = import* board ;
//! import      = "import" string ;
//! board       = "board" string "{" stmt* "}" ;
//! stmt        = layers | manufacturer | component | connection
//!             | diff_pair | keepout ;
//! component   = "component" ident ":" ident string [ "value" string ] ;   // concrete only in P1
//! connection  = "connect" endpoint "->" endpoint ;
//! diff_pair   = "diff_pair" ident ident "{" impedance? "}" ;
//! keepout     = "keepout" ident "{" radius? "}" ;
//! ```
//!
//! Abstract components, modules, variants, zones, and netclasses are
//! deferred to later phases.

#![forbid(unsafe_code)]

pub mod json;
pub mod parser;
pub mod token;

pub use json::parse_json;
pub use parser::{parse as parse_tokens, ParseResult};
pub use token::{lex, LexError, Token, TokenKind};

/// One-shot convenience: lex and parse a source string.
pub fn parse(source: &str, file: impl Into<String>) -> ParseResult {
    let tokens = lex(source);
    parse_tokens(tokens, file.into())
}
