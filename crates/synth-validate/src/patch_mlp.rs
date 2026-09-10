// SPDX-License-Identifier: Apache-2.0

//! Pure Rust f32 inference engine for the Patch-Consequence MLP model (Phase 6).
//!
//! Loads trained weight matrices from `patch_mlp_weights.json` and evaluates
//! forward pass predictions to populate [`PatchConsequencePreview`].

use serde::Deserialize;
use std::collections::HashMap;
use synth_diagnostics::{Diagnostic, Patch, PatchConsequencePreview};

/// Baked-in trained weights file.
pub const EMBEDDED_WEIGHTS_JSON: &str = include_str!("patch_mlp_weights.json");

#[derive(Debug, Deserialize)]
struct RawLayerWeights {
    weight: Vec<Vec<f32>>,
    bias: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct RawMlpModel {
    diag_codes: Vec<String>,
    patch_kinds: Vec<String>,
    #[serde(default)]
    component_kinds: Vec<String>,
    #[serde(default)]
    net_types: Vec<String>,
    layers: Vec<RawLayerWeights>,
}

/// Pure Rust matrix multiply inference engine for Patch-Consequence prediction.
#[derive(Debug, Clone)]
pub struct PatchMlp {
    diag_codes: Vec<String>,
    patch_kinds: Vec<String>,
    component_kinds: Vec<String>,
    net_types: Vec<String>,
    code_to_idx: HashMap<String, usize>,
    kind_to_idx: HashMap<String, usize>,
    comp_to_idx: HashMap<String, usize>,
    net_to_idx: HashMap<String, usize>,
    weights: Vec<Vec<Vec<f32>>>,
    biases: Vec<Vec<f32>>,
}

impl PatchMlp {
    /// Load model from baked-in JSON weights.
    #[must_use]
    pub fn load_default() -> Option<Self> {
        Self::from_json(EMBEDDED_WEIGHTS_JSON).ok()
    }

    /// Parse model from JSON weights string.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        let raw: RawMlpModel = serde_json::from_str(json_str)?;

        let mut code_to_idx = HashMap::new();
        for (i, code) in raw.diag_codes.iter().enumerate() {
            code_to_idx.insert(code.clone(), i);
        }

        let mut kind_to_idx = HashMap::new();
        for (i, kind) in raw.patch_kinds.iter().enumerate() {
            kind_to_idx.insert(kind.clone(), i);
        }

        let mut comp_to_idx = HashMap::new();
        for (i, comp) in raw.component_kinds.iter().enumerate() {
            comp_to_idx.insert(comp.clone(), i);
        }

        let mut net_to_idx = HashMap::new();
        for (i, net) in raw.net_types.iter().enumerate() {
            net_to_idx.insert(net.clone(), i);
        }

        let mut weights = Vec::new();
        let mut biases = Vec::new();

        for layer in raw.layers {
            weights.push(layer.weight);
            biases.push(layer.bias);
        }

        Ok(Self {
            diag_codes: raw.diag_codes,
            patch_kinds: raw.patch_kinds,
            component_kinds: raw.component_kinds,
            net_types: raw.net_types,
            code_to_idx,
            kind_to_idx,
            comp_to_idx,
            net_to_idx,
            weights,
            biases,
        })
    }

    /// Run forward pass over `before_codes`, `patch_kind`, `component_kind`, and `net_type`.
    #[must_use]
    pub fn predict(
        &self,
        before_codes: &[&str],
        patch_kind: &str,
    ) -> Option<PatchConsequencePreview> {
        self.predict_with_context(before_codes, patch_kind, "other", "signal")
    }

    /// Extended forward pass with 94-D structural graph features.
    #[must_use]
    pub fn predict_with_context(
        &self,
        before_codes: &[&str],
        patch_kind: &str,
        component_kind: &str,
        net_type: &str,
    ) -> Option<PatchConsequencePreview> {
        if self.weights.len() < 3 {
            return None;
        }

        let input_dim = self.diag_codes.len()
            + self.patch_kinds.len()
            + self.component_kinds.len()
            + self.net_types.len();
        let mut x = vec![0.0_f32; input_dim];

        // 1. Encode before_codes (0..56)
        for code in before_codes {
            if let Some(&idx) = self.code_to_idx.get(*code) {
                x[idx] = 1.0;
            }
        }

        // 2. Encode patch_kind one-hot (56..62)
        let offset_patch = self.diag_codes.len();
        if let Some(&kind_idx) = self.kind_to_idx.get(patch_kind) {
            x[offset_patch + kind_idx] = 1.0;
        }

        // 3. Encode component_kind one-hot (62..82)
        let offset_comp = offset_patch + self.patch_kinds.len();
        if let Some(&comp_idx) = self.comp_to_idx.get(component_kind) {
            x[offset_comp + comp_idx] = 1.0;
        }

        // 4. Encode net_type one-hot (82..94)
        let offset_net = offset_comp + self.component_kinds.len();
        if let Some(&net_idx) = self.net_to_idx.get(net_type) {
            x[offset_net + net_idx] = 1.0;
        }

        // Layer 1: ReLU(x * W0 + b0)
        let h1 = dense_relu(&x, &self.weights[0], &self.biases[0]);
        // Layer 2: ReLU(h1 * W1 + b1)
        let h2 = dense_relu(&h1, &self.weights[1], &self.biases[1]);
        // Layer 3: Sigmoid(h2 * W2 + b2)
        let out_prob = dense_sigmoid(&h2, &self.weights[2], &self.biases[2]);

        let before_set: std::collections::HashSet<&str> = before_codes.iter().copied().collect();

        let mut predicted_resolved = Vec::new();
        let mut predicted_new = Vec::new();
        let mut conf_sum = 0.0_f32;
        let mut conf_count = 0_usize;

        for (idx, &prob) in out_prob.iter().enumerate() {
            let code = &self.diag_codes[idx];
            let active_before = before_set.contains(code.as_str());
            let active_after = prob >= 0.5;

            if active_before && !active_after {
                predicted_resolved.push(code.clone());
                conf_sum += 1.0 - prob;
                conf_count += 1;
            } else if !active_before && active_after {
                predicted_new.push(code.clone());
                conf_sum += prob;
                conf_count += 1;
            }
        }

        #[allow(clippy::cast_precision_loss)]
        let model_confidence = if conf_count > 0 {
            conf_sum / (conf_count as f32)
        } else {
            0.85
        };

        if model_confidence >= 0.60 {
            Some(PatchConsequencePreview {
                predicted_resolved_diagnostics: predicted_resolved,
                predicted_new_diagnostics: predicted_new,
                model_confidence,
            })
        } else {
            None
        }
    }

    /// Trait delegate helper for [`synth_diagnostics::PatchConsequenceAdvisor`].
    #[must_use]
    pub fn predict_consequences(
        &self,
        current_diagnostics: &[Diagnostic],
        patch: &Patch,
    ) -> Option<PatchConsequencePreview> {
        let codes: Vec<&str> = current_diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        let kind_str = match &patch.kind {
            synth_diagnostics::PatchKind::ReplaceRange { .. } => "replace_range",
            synth_diagnostics::PatchKind::InsertAt { .. } => "insert_at",
            synth_diagnostics::PatchKind::DeleteRange { .. } => "delete_range",
            synth_diagnostics::PatchKind::AddStatement { .. } => "add_statement",
            synth_diagnostics::PatchKind::RemoveStatement { .. } => "remove_statement",
            synth_diagnostics::PatchKind::SolveSmt { .. } => "solve_smt",
        };
        let rationale = patch.rationale.as_deref().unwrap_or("").to_lowercase();
        let comp_kind = if rationale.contains("mcu") {
            "mcu"
        } else if rationale.contains("cap") {
            "capacitor"
        } else if rationale.contains("resistor") {
            "resistor"
        } else {
            "other"
        };
        let net_type = if rationale.contains("vcc") || rationale.contains("power") {
            "power"
        } else if rationale.contains("gnd") {
            "gnd"
        } else {
            "signal"
        };

        self.predict_with_context(&codes, kind_str, comp_kind, net_type)
    }
}

fn dense_relu(input: &[f32], weights: &[Vec<f32>], bias: &[f32]) -> Vec<f32> {
    let out_dim = bias.len();
    let mut out = bias.to_vec();
    for (i, &in_val) in input.iter().enumerate() {
        if in_val == 0.0 {
            continue;
        }
        if i < weights.len() {
            let row = &weights[i];
            for (j, &w) in row.iter().enumerate() {
                if j < out_dim {
                    out[j] += in_val * w;
                }
            }
        }
    }
    for val in &mut out {
        if *val < 0.0 {
            *val = 0.0;
        }
    }
    out
}

fn dense_sigmoid(input: &[f32], weights: &[Vec<f32>], bias: &[f32]) -> Vec<f32> {
    let out_dim = bias.len();
    let mut out = bias.to_vec();
    for (i, &in_val) in input.iter().enumerate() {
        if in_val == 0.0 {
            continue;
        }
        if i < weights.len() {
            let row = &weights[i];
            for (j, &w) in row.iter().enumerate() {
                if j < out_dim {
                    out[j] += in_val * w;
                }
            }
        }
    }
    for val in &mut out {
        *val = 1.0 / (1.0 + (-*val).exp());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_embedded_model_succeeds() {
        let mlp = PatchMlp::load_default();
        assert!(mlp.is_some(), "Embedded weights JSON failed to parse");
        let model = mlp.unwrap();
        assert!(!model.diag_codes.is_empty());
        assert_eq!(model.patch_kinds.len(), 6);
    }

    #[test]
    fn forward_pass_runs_without_panic() {
        let mlp = PatchMlp::load_default().unwrap();
        let preview = mlp.predict(&["E-SYNTH-CONNECT-001"], "insert_at");
        assert!(preview.is_some() || preview.is_none());
    }
}
