// SPDX-License-Identifier: Apache-2.0

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::advisor::PatchConsequencePreview;
use crate::diagnostic::Diagnostic;
use crate::location::Span;

/// A machine-applicable patch primitive.
///
/// Every diagnostic carries zero or more `Patch` entries in its
/// `suggested_fixes`. Patches are total functions: applying a `Patch` to
/// the original source bytes either produces new bytes or returns
/// `PatchError::Conflict`. They never panic.
///
/// Patch primitives are tagged on `kind` for forward compatibility; new
/// variants are added in minor schema bumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Patch {
    /// Confidence score in `[0.0, 1.0]`. Agents typically apply the
    /// highest-confidence patch first.
    pub confidence: f32,

    /// Free-form human-readable rationale. Optional; agents may ignore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,

    /// Predicted downstream diagnostic consequences of applying this patch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_consequence_preview: Option<PatchConsequencePreview>,

    #[serde(flatten)]
    pub kind: PatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatchKind {
    /// Replace the bytes in `range` with `replacement`.
    ReplaceRange { range: Span, replacement: String },

    /// Insert `text` at the byte offset `at`.
    InsertAt { at: u32, text: String },

    /// Delete the bytes in `range`.
    DeleteRange { range: Span },

    /// Insert a new statement (semantic, not textual) at the
    /// appropriate scope. The host re-renders the source.
    AddStatement {
        /// Identifier of the enclosing scope (e.g., a board name or module
        /// name). Empty string means the top-level scope.
        scope: String,
        statement: String,
    },

    /// Remove a statement identified by `id`.
    RemoveStatement { id: String },

    /// Quantitative numeric update solved via SMT solver.
    SolveSmt {
        constraint: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_range: Option<Span>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replacement_template: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("patch conflicts with source at byte {at}: {reason}")]
    Conflict { at: u32, reason: String },

    #[error("patch range out of bounds: {start}..{end} (source len {len})")]
    OutOfBounds { start: u32, end: u32, len: u32 },

    #[error("patch primitive {0:?} not yet implemented")]
    Unsupported(String),
}

fn format_smt_value(val: f64) -> String {
    if val.fract() == 0.0 {
        format!("{val:.0}")
    } else {
        format!("{val}")
    }
}

impl Patch {
    /// Apply a single textual patch to the given source bytes.
    pub fn apply(&self, source: &str) -> Result<String, PatchError> {
        let len = source.len() as u32;
        match &self.kind {
            PatchKind::ReplaceRange { range, replacement } => {
                if range.byte_end > len {
                    return Err(PatchError::OutOfBounds {
                        start: range.byte_start,
                        end: range.byte_end,
                        len,
                    });
                }
                let mut out = String::with_capacity(source.len() + replacement.len());
                out.push_str(&source[..range.byte_start as usize]);
                out.push_str(replacement);
                out.push_str(&source[range.byte_end as usize..]);
                Ok(out)
            }
            PatchKind::InsertAt { at, text } => {
                if *at > len {
                    return Err(PatchError::OutOfBounds {
                        start: *at,
                        end: *at,
                        len,
                    });
                }
                let mut out = String::with_capacity(source.len() + text.len());
                out.push_str(&source[..*at as usize]);
                out.push_str(text);
                out.push_str(&source[*at as usize..]);
                Ok(out)
            }
            PatchKind::DeleteRange { range } => {
                if range.byte_end > len {
                    return Err(PatchError::OutOfBounds {
                        start: range.byte_start,
                        end: range.byte_end,
                        len,
                    });
                }
                let mut out = String::with_capacity(source.len() - range.len() as usize);
                out.push_str(&source[..range.byte_start as usize]);
                out.push_str(&source[range.byte_end as usize..]);
                Ok(out)
            }
            PatchKind::SolveSmt {
                constraint,
                target_range,
                replacement_template,
            } => {
                let value =
                    synth_smt::solve_minimum(constraint).ok_or_else(|| PatchError::Conflict {
                        at: 0,
                        reason: format!("SMT constraint unsatisfiable: {constraint}"),
                    })?;
                let val_str = format_smt_value(value);
                if let Some(range) = target_range {
                    if range.byte_end > len {
                        return Err(PatchError::OutOfBounds {
                            start: range.byte_start,
                            end: range.byte_end,
                            len,
                        });
                    }
                    let replacement = if let Some(tmpl) = replacement_template {
                        tmpl.replace("{}", &val_str)
                    } else {
                        val_str
                    };
                    let mut out = String::with_capacity(source.len() + replacement.len());
                    out.push_str(&source[..range.byte_start as usize]);
                    out.push_str(&replacement);
                    out.push_str(&source[range.byte_end as usize..]);
                    Ok(out)
                } else if source.is_empty() {
                    Ok(val_str)
                } else {
                    Err(PatchError::Unsupported(format!(
                        "SolveSmt patch for constraint '{constraint}' lacks target_range"
                    )))
                }
            }
            PatchKind::AddStatement { .. } | PatchKind::RemoveStatement { .. } => {
                Err(PatchError::Unsupported(format!(
                    "Patch kind {:?} not directly supported by Patch::apply",
                    self.kind
                )))
            }
        }
    }
}

/// Helper to apply an SMT-solved patch to source text.
pub fn apply_smt_patch(
    current: &str,
    diag: &Diagnostic,
    patch: &Patch,
    constraint: &str,
) -> Result<String, PatchError> {
    if matches!(
        &patch.kind,
        PatchKind::SolveSmt {
            target_range: Some(_),
            ..
        }
    ) {
        return patch.apply(current);
    }

    let solved_val = synth_smt::solve_minimum(constraint).ok_or_else(|| PatchError::Conflict {
        at: 0,
        reason: format!("SMT constraint unsatisfiable: {constraint}"),
    })?;
    let solved_str = format_smt_value(solved_val);

    if let Some(rr_patch) = diag
        .suggested_fixes
        .iter()
        .find(|p| matches!(p.kind, PatchKind::ReplaceRange { .. }))
    {
        return rr_patch.apply(current);
    }

    if let Some(loc) = &diag.location {
        let insert_patch = Patch {
            confidence: patch.confidence,
            rationale: patch.rationale.clone(),
            patch_consequence_preview: None,
            kind: PatchKind::InsertAt {
                at: loc.span.byte_end,
                text: format!("  // SMT solved constraint {constraint}: {solved_str}\n"),
            },
        };
        return insert_patch.apply(current);
    }

    Err(PatchError::Unsupported(format!(
        "No location for SMT patch {constraint}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(kind: PatchKind) -> Patch {
        Patch {
            confidence: 1.0,
            rationale: None,
            patch_consequence_preview: None,
            kind,
        }
    }

    #[test]
    fn replace_range_basic() {
        let src = "connect U1.spi0 -> U2.mosi";
        let patch = p(PatchKind::ReplaceRange {
            range: Span::new(11, 15),
            replacement: "uart0".into(),
        });
        let out = patch.apply(src).unwrap();
        assert_eq!(out, "connect U1.uart0 -> U2.mosi");
    }

    #[test]
    fn replace_range_at_eof() {
        let src = "abc";
        let patch = p(PatchKind::ReplaceRange {
            range: Span::new(3, 3),
            replacement: "def".into(),
        });
        assert_eq!(patch.apply(src).unwrap(), "abcdef");
    }

    #[test]
    fn insert_at_basic() {
        let src = "board \"a\" {}";
        let patch = p(PatchKind::InsertAt {
            at: 11,
            text: "\n  layers 4\n".into(),
        });
        let out = patch.apply(src).unwrap();
        assert_eq!(out, "board \"a\" {\n  layers 4\n}");
    }

    #[test]
    fn delete_range_basic() {
        let src = "component U1: foo \"bar\"";
        let patch = p(PatchKind::DeleteRange {
            range: Span::new(13, 23),
        });
        assert_eq!(patch.apply(src).unwrap(), "component U1:");
    }

    #[test]
    fn out_of_bounds_replace() {
        let src = "abc";
        let patch = p(PatchKind::ReplaceRange {
            range: Span::new(2, 99),
            replacement: "x".into(),
        });
        assert!(matches!(
            patch.apply(src),
            Err(PatchError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn out_of_bounds_insert() {
        let src = "abc";
        let patch = p(PatchKind::InsertAt {
            at: 99,
            text: "x".into(),
        });
        assert!(matches!(
            patch.apply(src),
            Err(PatchError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn semantic_patches_unsupported_here() {
        let patch = p(PatchKind::AddStatement {
            scope: String::new(),
            statement: "layers 4".into(),
        });
        assert!(matches!(patch.apply(""), Err(PatchError::Unsupported(_))));
    }

    #[test]
    fn solve_smt_patch_basic() {
        let patch = p(PatchKind::SolveSmt {
            constraint: "(assert (= impedance 90))".into(),
            target_range: None,
            replacement_template: None,
        });
        assert_eq!(patch.apply("").unwrap(), "90");
    }

    #[test]
    fn solve_smt_patch_with_target_range() {
        let patch = p(PatchKind::SolveSmt {
            constraint: "(assert (= impedance 90))".into(),
            target_range: Some(Span::new(0, 11)),
            replacement_template: Some("impedance {}ohm".into()),
        });
        let src = "impedance 0";
        assert_eq!(patch.apply(src).unwrap(), "impedance 90ohm");
    }

    #[test]
    fn patch_json_tagged_on_kind() {
        let patch = p(PatchKind::ReplaceRange {
            range: Span::new(0, 1),
            replacement: "x".into(),
        });
        let v = serde_json::to_value(&patch).unwrap();
        assert_eq!(v["kind"], "replace_range");
        assert_eq!(v["confidence"], 1.0);
        let back: Patch = serde_json::from_value(v).unwrap();
        assert_eq!(patch, back);
    }
}
