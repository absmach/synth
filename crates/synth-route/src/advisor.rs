// SPDX-License-Identifier: Apache-2.0

//! Congestion advisor traits for routing guidance.
//!
//! Provides the architectural boundary for open-source default
//! heuristics versus enterprise EBM (Energy-Based Model) cloud guidance.

use synth_geometry::Point;
use synth_ir::Board;

/// Cost field guidance provided by an advisor for grid routing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CongestionCost {
    /// Additive penalty score for traversing a target cell.
    pub penalty: f32,
    /// Multiplicative scale factor applied to cell traversal cost.
    pub cost_multiplier: f32,
}

impl Default for CongestionCost {
    fn default() -> Self {
        Self {
            penalty: 0.0,
            cost_multiplier: 1.0,
        }
    }
}

/// Trait implemented by congestion advisors.
///
/// Open-source builds use [`DefaultCongestionAdvisor`], which returns
/// zero penalty and 1.0 multiplier (pure deterministic routing).
/// Optional implementations can provide model-based guidance.
pub trait CongestionAdvisor: Send + Sync {
    /// Evaluate the congestion penalty for a point on a given layer.
    fn evaluate_cell_cost(&self, board: &Board, point: Point, layer_index: usize)
        -> CongestionCost;
}

/// Default open-source congestion advisor emitting zero penalty.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultCongestionAdvisor;

impl CongestionAdvisor for DefaultCongestionAdvisor {
    fn evaluate_cell_cost(
        &self,
        _board: &Board,
        _point: Point,
        _layer_index: usize,
    ) -> CongestionCost {
        CongestionCost::default()
    }
}
