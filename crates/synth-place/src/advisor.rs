// SPDX-License-Identifier: Apache-2.0

//! Placement advisor traits for guidance.
//!
//! Provides the architectural boundary for open-source default
//! heuristics versus enterprise ML placement guidance.

use synth_geometry::Point;
use synth_ir::{Board, ComponentId};

/// Bias vector or spatial suggestion provided by a placement advisor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementBias {
    /// Preferred position suggestion for a component (if any).
    pub preferred_center: Option<Point>,
    /// Weight/priority multiplier for placing this component.
    pub priority_weight: f32,
}

impl Default for PlacementBias {
    fn default() -> Self {
        Self {
            preferred_center: None,
            priority_weight: 1.0,
        }
    }
}

/// Trait implemented by placement advisors.
///
/// Open-source builds use [`DefaultPlacementAdvisor`], which returns neutral
/// bias (pure deterministic constraint placement).
pub trait PlacementAdvisor: Send + Sync {
    /// Evaluate placement bias for a component on a board.
    fn evaluate_bias(&self, board: &Board, component_id: ComponentId) -> PlacementBias;
}

/// Default open-source placement advisor emitting neutral bias.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPlacementAdvisor;

impl PlacementAdvisor for DefaultPlacementAdvisor {
    fn evaluate_bias(&self, _board: &Board, _component_id: ComponentId) -> PlacementBias {
        PlacementBias::default()
    }
}
