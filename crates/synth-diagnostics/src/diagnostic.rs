// SPDX-License-Identifier: Apache-2.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::location::Location;
use crate::patch::Patch;
use crate::severity::Severity;
use crate::SCHEMA_VERSION;

/// A reference to a domain entity (component, pin, net, ...) that the
/// diagnostic concerns. Tagged on `kind` for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityRef {
    Component { id: String },
    Pin { component: String, pin: String },
    Net { name: String },
    Constraint { id: String },
    Module { name: String },
}

/// An entity reference with a role: primary (the main entity the
/// diagnostic is about) or peer (a related entity for cross-reference,
/// e.g., the other pin in a conflict). Enables click-to-navigate in UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum EntityRole {
    Primary(EntityRef),
    Peer(EntityRef),
}

/// A candidate value the diagnostic suggests instead of what was found.
/// Distinct from a [`Patch`] in that candidates are *informational* —
/// they describe possibilities, not edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidate {
    #[serde(flatten)]
    pub entity: EntityRef,
    pub confidence: f32,
}

/// A non-textual next step the agent can take to resolve the diagnostic —
/// distinct from a [`Patch`] in that it does not edit the source file's
/// bytes. Applying one means calling the named MCP tool (or CLI
/// equivalent) with the given arguments, not patching `board.synth`.
///
/// Introduced for `E-SYNTH-COMP-001` (Phase 15, §18.8.4): an unknown
/// part cannot be fixed by rewriting the reference alone — the agent
/// must first grow the registry via search/import/authoring, then
/// revalidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestedAction {
    /// Search LCSC/EasyEDA for a part matching `query` via the
    /// `synth_search_registry_web` MCP tool / `synth part search` CLI.
    SearchRegistryWeb { query: String },
    /// Import a part into the Tier-2 registry via the `synth_import_part`
    /// MCP tool / `synth part import lcsc|kicad` CLI, once a search (or
    /// the user) has identified a source id for `part_id`.
    ImportPartStub { part_id: String },
    /// Write a required-pins-relaxed TOML skeleton for `part_id` into
    /// the Tier-2 registry via `synth part stub` / `create_part_stub`,
    /// then fill it in from a datasheet and revalidate.
    CreatePartStub { part_id: String },
}

/// The top-level diagnostic record. Stable wire format; see
/// [`crate::SCHEMA_VERSION`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    /// Schema version this diagnostic conforms to. Always written by
    /// builders; consumers may pin to a major version.
    pub schema_version: String,

    /// Stable diagnostic code, e.g. `E-SYNTH-PARSE-001`. Codes are
    /// documented one-per-file under `docs/diagnostics/`.
    pub code: String,

    pub severity: Severity,

    /// Short human-readable title. Stable across patch releases of the
    /// emitting rule; do not parse.
    pub title: String,

    /// Optional longer human-readable message. May contain interpolated
    /// values; not stable across patch releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Source location the diagnostic is anchored to. Required for all
    /// non-internal diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,

    /// Primary entity the diagnostic is about (for click-to-navigate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_entity: Option<EntityRef>,

    /// Related entities for cross-reference (e.g., conflicting pins).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_entities: Vec<EntityRef>,

    /// Domain entities the diagnostic concerns (legacy flat list).
    /// Kept for backward compatibility; prefer primary_entity/peer_entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityRef>,

    /// What the rule expected to find. Free-form description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,

    /// What was found instead. Free-form description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,

    /// Informational candidates (not edits — see `suggested_fixes` for
    /// edits).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,

    /// Machine-applicable patches, ordered by descending confidence.
    /// Always at least one entry for rules that have a known fix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_fixes: Vec<Patch>,

    /// Non-textual next steps (tool calls, not source edits) — see
    /// [`SuggestedAction`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_actions: Vec<SuggestedAction>,

    /// URL to the documentation page for this diagnostic code.
    /// Convention: `synth.docs/diagnostics/<code>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation_url: Option<String>,

    /// Optional SMT-LIB2 constraint payload for quantitative diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smt_constraint: Option<String>,
}

/// Builder for [`Diagnostic`]. Required fields (`code`, `severity`,
/// `title`) are taken at construction; all others are optional and
/// added via fluent methods.
#[derive(Debug)]
pub struct DiagnosticBuilder {
    diag: Diagnostic,
}

impl DiagnosticBuilder {
    pub fn new(code: impl Into<String>, severity: Severity, title: impl Into<String>) -> Self {
        Self {
            diag: Diagnostic {
                schema_version: SCHEMA_VERSION.to_string(),
                code: code.into(),
                severity,
                title: title.into(),
                message: None,
                location: None,
                primary_entity: None,
                peer_entities: Vec::new(),
                entities: Vec::new(),
                expected: None,
                found: None,
                candidates: Vec::new(),
                suggested_fixes: Vec::new(),
                suggested_actions: Vec::new(),
                explanation_url: None,
                smt_constraint: None,
            },
        }
    }

    pub fn message(mut self, msg: impl Into<String>) -> Self {
        self.diag.message = Some(msg.into());
        self
    }

    pub fn location(mut self, loc: Location) -> Self {
        self.diag.location = Some(loc);
        self
    }

    /// Set the primary entity (main subject of the diagnostic).
    pub fn primary_entity(mut self, e: EntityRef) -> Self {
        self.diag.primary_entity = Some(e);
        self
    }

    /// Add a peer entity (related entity for cross-reference).
    pub fn peer_entity(mut self, e: EntityRef) -> Self {
        self.diag.peer_entities.push(e);
        self
    }

    /// Add an entity to the legacy flat list (backward compatibility).
    pub fn entity(mut self, e: EntityRef) -> Self {
        self.diag.entities.push(e);
        self
    }

    pub fn expected(mut self, s: impl Into<String>) -> Self {
        self.diag.expected = Some(s.into());
        self
    }

    pub fn found(mut self, s: impl Into<String>) -> Self {
        self.diag.found = Some(s.into());
        self
    }

    pub fn candidate(mut self, c: Candidate) -> Self {
        self.diag.candidates.push(c);
        self
    }

    pub fn suggested_fix(mut self, p: Patch) -> Self {
        self.diag.suggested_fixes.push(p);
        self
    }

    pub fn suggested_action(mut self, a: SuggestedAction) -> Self {
        self.diag.suggested_actions.push(a);
        self
    }

    pub fn explanation_url(mut self, url: impl Into<String>) -> Self {
        self.diag.explanation_url = Some(url.into());
        self
    }

    pub fn smt_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.diag.smt_constraint = Some(constraint.into());
        self
    }

    pub fn build(mut self) -> Diagnostic {
        // Stable ordering: highest consequence model confidence / patch confidence first.
        self.diag.suggested_fixes.sort_by(|a, b| {
            let score_a = a
                .patch_consequence_preview
                .as_ref()
                .map_or(a.confidence, |p| p.model_confidence);
            let score_b = b
                .patch_consequence_preview
                .as_ref()
                .map_or(b.confidence, |p| p.model_confidence);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.diag
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::Span;
    use crate::patch::PatchKind;

    #[test]
    fn minimal_diagnostic_serializes() {
        let d = DiagnosticBuilder::new("E-SYNTH-PARSE-001", Severity::Error, "unexpected token")
            .build();
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["schema_version"], SCHEMA_VERSION);
        assert_eq!(v["code"], "E-SYNTH-PARSE-001");
        assert_eq!(v["severity"], "error");
        assert_eq!(v["title"], "unexpected token");
        // Optional fields omitted when empty.
        assert!(v.get("message").is_none());
        assert!(v.get("location").is_none());
        assert!(v.get("primary_entity").is_none());
        assert!(v.get("peer_entities").is_none());
    }

    #[test]
    fn rich_diagnostic_round_trips() {
        let d = DiagnosticBuilder::new(
            "E-SYNTH-USB-001",
            Severity::Error,
            "USB differential connection invalid",
        )
        .message("DP wire connected to non-DP-capable pin")
        .location(Location::from_span("board.synth", Span::new(412, 421)))
        .primary_entity(EntityRef::Component { id: "U1".into() })
        .peer_entity(EntityRef::Pin {
            component: "U1".into(),
            pin: "GP0".into(),
        })
        .expected("USB_DP capable target")
        .found("SPI MOSI pin")
        .candidate(Candidate {
            entity: EntityRef::Pin {
                component: "U1".into(),
                pin: "USB_DP".into(),
            },
            confidence: 0.92,
        })
        .suggested_fix(Patch {
            confidence: 0.92,
            rationale: Some("only USB_DP-capable pin on U1".into()),
            patch_consequence_preview: None,
            kind: PatchKind::ReplaceRange {
                range: Span::new(412, 421),
                replacement: "U1.usb_dp".into(),
            },
        })
        .explanation_url("synth.docs/diagnostics/E-SYNTH-USB-001")
        .build();

        let json = serde_json::to_string(&d).unwrap();
        let back: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code, d.code);
        assert_eq!(
            back.primary_entity.clone(),
            Some(EntityRef::Component { id: "U1".into() })
        );
        assert_eq!(back.peer_entities.len(), 1);
        assert_eq!(back.suggested_fixes.len(), 1);
        assert_eq!(back.suggested_fixes[0].confidence, 0.92);
    }

    #[test]
    fn suggested_fixes_sorted_by_descending_confidence() {
        let d = DiagnosticBuilder::new("E-SYNTH-TEST-001", Severity::Warning, "x")
            .suggested_fix(Patch {
                confidence: 0.3,
                rationale: None,
                patch_consequence_preview: None,
                kind: PatchKind::InsertAt {
                    at: 0,
                    text: "a".into(),
                },
            })
            .suggested_fix(Patch {
                confidence: 0.9,
                rationale: None,
                patch_consequence_preview: None,
                kind: PatchKind::InsertAt {
                    at: 0,
                    text: "b".into(),
                },
            })
            .suggested_fix(Patch {
                confidence: 0.6,
                rationale: None,
                patch_consequence_preview: None,
                kind: PatchKind::InsertAt {
                    at: 0,
                    text: "c".into(),
                },
            })
            .build();
        let confs: Vec<f32> = d.suggested_fixes.iter().map(|p| p.confidence).collect();
        assert_eq!(confs, vec![0.9, 0.6, 0.3]);
    }

    #[test]
    fn smt_constraint_serializes() {
        let d = DiagnosticBuilder::new(
            "E-SYNTH-DIFF-001",
            Severity::Error,
            "diff pair missing impedance",
        )
        .smt_constraint("(assert (= impedance 90))")
        .build();
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["smt_constraint"], "(assert (= impedance 90))");
    }

    #[test]
    fn snapshot_minimal() {
        let d = DiagnosticBuilder::new("E-SYNTH-PARSE-001", Severity::Error, "unexpected token")
            .message("expected `}` at end of board")
            .location(Location {
                file: "hello.synth".into(),
                span: Span::new(12, 13),
                line_start: 2,
                col_start: 1,
                line_end: 2,
                col_end: 2,
            })
            .build();
        insta::assert_json_snapshot!(d);
    }
}
