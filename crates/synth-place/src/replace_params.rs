// SPDX-License-Identifier: Apache-2.0

//! Tunable parameters for the RePlAce global-placement experiment.
//!
//! This file is intentionally the only place the autoresearch loop edits for
//! equation tuning. The solver reads [`RePlAceParams::default`]; the scorer and
//! corpus are frozen.
//!
//! Units are millimetres unless stated otherwise.

/// Which density force the Nesterov step uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DensityModel {
    /// Bin-overlap density (research note Option A), the stable first model.
    Bin,
    /// Poisson/electrostatic density on a DCT grid (research note Option B).
    Poisson,
}

/// One RePlAce run's equation knobs.
#[derive(Clone, Copy, Debug)]
pub struct RePlAceParams {
    /// Density force model.
    pub density_model: DensityModel,
    /// Density grid is `bin_count x bin_count` over the usable rectangle.
    pub bin_count: usize,
    /// Number of gamma/lambda stages.
    pub stages: usize,
    /// Nesterov iterations per stage.
    pub iterations_per_stage: usize,
    /// Initial weighted-average smoothing, in millimetres.
    pub gamma_start_mm: f64,
    /// Smallest smoothing value.
    pub gamma_end_mm: f64,
    /// Gamma is multiplied by this after every stage.
    pub gamma_decay: f64,
    /// Initial density weight, relative to the wirelength gradient: 1.0 makes
    /// the density force as strong as the wirelength force at the start.
    pub lambda_start: f64,
    /// Lambda is multiplied by this after every stage.
    pub lambda_growth: f64,
    /// Nesterov step per unit gradient, in millimetres.
    pub step_mm: f64,
    /// Largest single-coordinate movement per iteration, in millimetres.
    pub max_step_mm: f64,
    /// Weight of one net in the wirelength objective.
    pub net_weight: f64,
    /// Fraction of a bin the parts may fill before the density force pushes
    /// them apart (OpenROAD gpl `-target_density`). Below 1.0 leaves routing
    /// room; the force is zero in bins under the target.
    pub target_density: f64,
    /// Weight of the pairwise courtyard-overlap repulsion, which grows by
    /// `lambda_growth` each stage. The bin density has no force on a symmetric
    /// pile of parts; this term separates them directly.
    pub overlap_weight: f64,
    /// Envelope surface tension: each free part is pulled toward its nearest
    /// larger free part (largest parts toward the free-part centroid), in
    /// millimetres of force per millimetre of offset. The wirelength and bin
    /// density terms never minimize the courtyard envelope, yet the exported
    /// outline is fitted to it, so the dead and area terms both grow with it.
    /// Anchoring on a local larger neighbour rather than one global centroid
    /// keeps a decoupling passive next to its IC. The overlap repulsion and
    /// the density force hold the parts apart once the envelope is tight.
    pub compact_weight: f64,
    /// Dead-space coverage pull. The score counts a one-millimetre cell as dead
    /// when no courtyard comes within three millimetres of it, so an empty
    /// corner between the extreme parts is wasted board even though it does
    /// not grow the fitted outline. Each empty cell pulls the nearest free part
    /// toward it by one unit of force, which spreads the passives into those
    /// corners instead of leaving them clustered around the largest part.
    pub coverage_weight: f64,
    /// Nets with more pins than this (ground, power rails) are left out of
    /// the wirelength objective, as they would pull every part to the centre.
    pub max_net_degree: usize,
    /// Extra clearance the legalizer keeps around free parts, in millimetres.
    pub legalizer_gap_mm: f64,
}

impl Default for RePlAceParams {
    fn default() -> Self {
        Self {
            density_model: DensityModel::Bin,
            bin_count: 64,
            stages: 6,
            iterations_per_stage: 40,
            gamma_start_mm: 20.0,
            gamma_end_mm: 0.5,
            gamma_decay: 0.5,
            lambda_start: 0.1,
            lambda_growth: 2.0,
            step_mm: 0.2,
            max_step_mm: 1.5,
            net_weight: 1.0,
            target_density: 0.8,
            overlap_weight: 0.06,
            compact_weight: 0.12,
            coverage_weight: 0.5,
            max_net_degree: 10,
            legalizer_gap_mm: 0.25,
        }
    }
}
