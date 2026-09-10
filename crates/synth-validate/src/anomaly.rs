// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use synth_diagnostics::{DiagnosticBuilder, Location, Severity};
use synth_ir::{Board, PinId};
use synth_registry::PinCapability;

use crate::{ErcCategory, ErcRule};

/// 15-dimensional feature vector for board graph statistical representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardFeatureVec(pub [f64; 15]);

/// Serialized One-Class SVM anomaly model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyModel {
    pub feature_names: Vec<String>,
    pub scaler_mean: Vec<f64>,
    pub scaler_std: Vec<f64>,
    pub support_vectors: Vec<Vec<f64>>,
    pub dual_coef: Vec<f64>,
    pub intercept: f64,
    pub gamma: f64,
    pub nu: f64,
    pub training_corpus_size: usize,
    pub held_out_size: usize,
    pub held_out_fpr: f64,
}

static MODEL: OnceLock<Option<AnomalyModel>> = OnceLock::new();

pub fn get_anomaly_model() -> &'static Option<AnomalyModel> {
    MODEL.get_or_init(|| {
        let raw = include_str!("anomaly_model.json");
        serde_json::from_str(raw).ok()
    })
}

impl AnomalyModel {
    /// Evaluates the RBF decision score for a 15-dimensional raw feature vector.
    /// Returns score where < 0.0 indicates an anomaly.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn evaluate(&self, raw_features: &[f64; 15]) -> f64 {
        if self.support_vectors.is_empty()
            || self.scaler_mean.len() < 15
            || self.scaler_std.len() < 15
        {
            return 1.0; // Default to non-anomaly if uninitialized or dummy model
        }

        // Standardize features
        let mut scaled = [0.0; 15];
        for i in 0..15 {
            let std_dev = if self.scaler_std[i].abs() < 1e-12 {
                1.0
            } else {
                self.scaler_std[i]
            };
            scaled[i] = (raw_features[i] - self.scaler_mean[i]) / std_dev;
        }

        // Compute RBF SVM decision score: intercept + sum(coef_i * exp(-gamma * ||scaled - sv_i||^2))
        let mut score = self.intercept;
        for (sv_idx, sv) in self.support_vectors.iter().enumerate() {
            if sv_idx >= self.dual_coef.len() || sv.len() < 15 {
                continue;
            }
            let mut sq_dist = 0.0;
            for j in 0..15 {
                let diff = scaled[j] - sv[j];
                sq_dist += diff * diff;
            }
            let k_val = (-self.gamma * sq_dist).exp();
            score += self.dual_coef[sv_idx] * k_val;
        }

        score
    }
}

/// Extract the 15-feature representation from a [`Board`].
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn extract_features(board: &Board) -> BoardFeatureVec {
    let component_count = board.components.len() as f64;
    let net_count = board.nets.len() as f64;

    let total_endpoints: usize = board.nets.iter().map(|n| n.endpoints.len()).sum();
    let avg_net_degree = if board.nets.is_empty() {
        0.0
    } else {
        total_endpoints as f64 / board.nets.len() as f64
    };

    let max_net_degree = board
        .nets
        .iter()
        .map(|n| n.endpoints.len())
        .max()
        .unwrap_or(0) as f64;

    let passive_count = board
        .components
        .iter()
        .filter(|c| {
            let k = c.part.as_ref().map_or(c.kind.as_str(), |p| p.kind.as_str());
            matches!(
                k,
                "capacitor" | "resistor" | "inductor" | "diode" | "ferrite"
            )
        })
        .count();

    let passive_ratio = if board.components.is_empty() {
        0.0
    } else {
        passive_count as f64 / component_count
    };

    let total_pins: usize = board
        .components
        .iter()
        .filter_map(|c| c.part.as_ref())
        .map(|p| p.pins.len())
        .sum();

    let power_pins: usize = board
        .components
        .iter()
        .filter_map(|c| c.part.as_ref())
        .flat_map(|p| &p.pins)
        .filter(|p| {
            matches!(
                p.electrical_type,
                synth_registry::ElectricalType::PowerInput
                    | synth_registry::ElectricalType::PowerOutput
            )
        })
        .count();

    let power_pin_ratio = if total_pins == 0 {
        0.0
    } else {
        power_pins as f64 / total_pins as f64
    };

    let diff_pair_count = board.diff_pairs.len() as f64;
    let keepout_count = board.keepouts.len() as f64;

    let usb_pin_count = board
        .nets
        .iter()
        .flat_map(|n| &n.endpoints)
        .filter(|e| {
            board.pin(e.component, e.pin).is_some_and(|p| {
                p.capabilities
                    .iter()
                    .any(|c| matches!(c, PinCapability::UsbDp | PinCapability::UsbDn))
            })
        })
        .count() as f64;

    let i2c_pin_count = board
        .nets
        .iter()
        .flat_map(|n| &n.endpoints)
        .filter(|e| {
            board.pin(e.component, e.pin).is_some_and(|p| {
                p.capabilities
                    .iter()
                    .any(|c| matches!(c, PinCapability::I2cSda | PinCapability::I2cScl))
            })
        })
        .count() as f64;

    let spi_pin_count = board
        .nets
        .iter()
        .flat_map(|n| &n.endpoints)
        .filter(|e| {
            board.pin(e.component, e.pin).is_some_and(|p| {
                p.capabilities.iter().any(|c| {
                    matches!(
                        c,
                        PinCapability::SpiMosi
                            | PinCapability::SpiMiso
                            | PinCapability::SpiSck
                            | PinCapability::SpiCs
                    )
                })
            })
        })
        .count() as f64;

    let cap_count = board
        .components
        .iter()
        .filter(|c| c.part.as_ref().is_some_and(|p| p.kind == "capacitor") || c.kind == "capacitor")
        .count();

    let non_passive_count = board.components.len().saturating_sub(passive_count);
    let decoupling_density = cap_count as f64 / non_passive_count.max(1) as f64;

    let mut total_req_pins = 0_usize;
    let mut connected_req_pins = 0_usize;
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        for (idx, pin) in part.pins.iter().enumerate() {
            if pin.required {
                total_req_pins += 1;
                let pid = PinId(idx as u32);
                if board.nets_containing(component.id, pid).next().is_some() {
                    connected_req_pins += 1;
                }
            }
        }
    }

    let required_connected_fraction = if total_req_pins == 0 {
        1.0
    } else {
        connected_req_pins as f64 / total_req_pins as f64
    };

    let mcu_count = board
        .components
        .iter()
        .filter(|c| c.kind == "mcu" || c.part.as_ref().is_some_and(|p| p.kind == "mcu"))
        .count() as f64;

    let board_layers = f64::from(board.layers);

    BoardFeatureVec([
        component_count,
        net_count,
        avg_net_degree,
        max_net_degree,
        passive_ratio,
        power_pin_ratio,
        diff_pair_count,
        keepout_count,
        usb_pin_count,
        i2c_pin_count,
        spi_pin_count,
        decoupling_density,
        required_connected_fraction,
        mcu_count,
        board_layers,
    ])
}

/// W-SYNTH-ANOMALY-001 — One-class SVM statistical anomaly detector rule.
#[derive(Debug, Default)]
pub struct GraphAnomalyDetectorRule;

impl GraphAnomalyDetectorRule {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ErcRule for GraphAnomalyDetectorRule {
    fn code(&self) -> &'static str {
        "W-SYNTH-ANOMALY-001"
    }

    fn category(&self) -> ErcCategory {
        ErcCategory::Board
    }

    fn check(&self, board: &Board, file: &str) -> Vec<synth_diagnostics::Diagnostic> {
        let Some(model) = get_anomaly_model() else {
            return Vec::new();
        };

        if model.support_vectors.is_empty() {
            return Vec::new();
        }

        let features = extract_features(board);
        let decision_score = model.evaluate(&features.0);

        if decision_score < 0.0 {
            vec![DiagnosticBuilder::new(
                self.code(),
                Severity::Warning,
                "statistically unusual design structure detected",
            )
            .location(Location::from_span(file.to_string(), board.source_span))
            .expected("board graph topology matching baseline patterns".to_string())
            .found(format!(
                "design structure produces an anomaly score of {decision_score:.3} (< 0.0 baseline boundary)"
            ))
            .explanation_url(format!("synth.docs/diagnostics/{}", self.code()))
            .build()]
        } else {
            Vec::new()
        }
    }
}
