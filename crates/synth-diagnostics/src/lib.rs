// SPDX-License-Identifier: Apache-2.0

//! Machine-readable diagnostics for the Synth EDA compiler.
//!
//! Every error emitted by any stage of the Synth pipeline (lexer, parser,
//! semantic analysis, ERC, placement, routing, DRC, manufacturing export)
//! is represented as a [`Diagnostic`]. The wire format is stable and
//! versioned via [`SCHEMA_VERSION`].
//!
//! The diagnostic contract is one of the three product invariants stated
//! in the implementation plan (§0.1): every error carries a stable code,
//! a byte-accurate location, and at least one machine-applicable patch.

#![forbid(unsafe_code)]

mod advisor;
mod diagnostic;
pub mod harness;
mod location;
mod patch;
mod severity;

pub use advisor::{
    DefaultPatchConsequenceAdvisor, PatchConsequenceAdvisor, PatchConsequencePreview,
};
pub use diagnostic::{
    Candidate, Diagnostic, DiagnosticBuilder, EntityRef, EntityRole, SuggestedAction,
};
pub use harness::{AgentHarness, HarnessRunResult, HarnessStrategy};
pub use location::{ByteOffset, FileId, LineCol, Location, Span};
pub use patch::{apply_smt_patch, Patch, PatchError, PatchKind};
pub use severity::Severity;

/// Stable schema version of the diagnostic JSON wire format.
///
/// Increment major when removing or renaming fields, minor when adding
/// optional fields. Agents may pin to a major version.
pub const SCHEMA_VERSION: &str = "1.3";

/// Returns the JSON Schema (draft-07) for the [`Diagnostic`] type as a
/// `serde_json::Value`. Re-derives every call — cheap (microseconds),
/// not worth caching.
///
/// Use this to generate `schemas/diagnostic.schema.json` and to expose
/// the schema via the `synth schema diagnostic` CLI subcommand.
///
/// # Panics
///
/// Panics if `schemars`'s `schema_for!(Diagnostic)` produces output
/// that `serde_json` rejects. That can only happen on internal bugs
/// in `schemars` — the `Diagnostic` type tree uses only standard
/// derive-compatible shapes.
pub fn diagnostic_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(Diagnostic);
    serde_json::to_value(schema).expect("schemars output is JSON-compatible")
}
