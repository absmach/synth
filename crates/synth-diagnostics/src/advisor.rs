// SPDX-License-Identifier: Apache-2.0

//! Diagnostic patch consequence advisor traits.
//!
//! Provides the architectural boundary for open-source diagnostic repair loops
//! versus enterprise Patch-Consequence model predictions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Diagnostic, Patch};

/// Predicted diagnostic consequences resulting from applying a patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PatchConsequencePreview {
    /// Expected diagnostic codes that will be resolved by the patch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicted_resolved_diagnostics: Vec<String>,
    /// Expected new diagnostic codes that may be introduced downstream.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicted_new_diagnostics: Vec<String>,
    /// Model confidence in `[0.0, 1.0]`.
    #[serde(default)]
    pub model_confidence: f32,
}

/// Trait implemented by patch consequence advisors.
///
/// Open-source builds use [`DefaultPatchConsequenceAdvisor`], which returns heuristic
/// previews. Enterprise builds use learned Patch-Consequence models to re-rank patch suggestions.
pub trait PatchConsequenceAdvisor: Send + Sync {
    /// Predict downstream diagnostic impacts of applying `patch` given `current_diagnostics`.
    fn predict_consequences(
        &self,
        current_diagnostics: &[Diagnostic],
        patch: &Patch,
    ) -> PatchConsequencePreview;
}

/// Default open-source patch consequence advisor.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPatchConsequenceAdvisor;

impl PatchConsequenceAdvisor for DefaultPatchConsequenceAdvisor {
    fn predict_consequences(
        &self,
        current_diagnostics: &[Diagnostic],
        patch: &Patch,
    ) -> PatchConsequencePreview {
        // Heuristic consequence prediction for open-source builds
        let mut resolved = Vec::new();
        for diag in current_diagnostics {
            if diag.suggested_fixes.iter().any(|p| p.kind == patch.kind) {
                resolved.push(diag.code.clone());
            }
        }
        let confidence = if patch.confidence > 0.0 {
            patch.confidence
        } else {
            0.85
        };
        PatchConsequencePreview {
            predicted_resolved_diagnostics: resolved,
            predicted_new_diagnostics: Vec::new(),
            model_confidence: confidence,
        }
    }
}

impl DefaultPatchConsequenceAdvisor {
    /// Returns the predicted consequence preview as `Some` if model confidence >= 0.6,
    /// or `None` if below threshold (per plan §8.3).
    pub fn predict_consequences_option(
        &self,
        current_diagnostics: &[Diagnostic],
        patch: &Patch,
    ) -> Option<PatchConsequencePreview> {
        let preview = self.predict_consequences(current_diagnostics, patch);
        if preview.model_confidence >= 0.6 {
            Some(preview)
        } else {
            None
        }
    }
}
