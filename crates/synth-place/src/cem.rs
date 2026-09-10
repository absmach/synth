// SPDX-License-Identifier: Apache-2.0

//! Cross-Entropy Method (CEM) Coarse Macro-Floorplanning.
//!
//! Implements a numerical Cross-Entropy Method optimizer for initial macro-region assignment
//! (MCU, Power, RF, Connector, Sensor blocks) per ML plan §3.4.
//!
//! Iteratively samples macro position candidates, evaluates surrogate HPWL + macro separation,
//! selects elite candidates, and refits Gaussian parameter distributions until convergence.
//!
//! Provides region-center seed hints for the greedy placement solver to minimize
//! global wirelength (HPWL) and preserve macro separation.

use std::collections::HashMap;
use synth_geometry::{Point, Rect};
use synth_ir::{Board, ComponentId};

/// Map of ComponentId -> suggested starting center point for spatial placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionAssignment {
    pub hints: HashMap<ComponentId, Point>,
}

/// Simple XorShift64 random number generator with Box-Muller transform for normal distribution.
struct CemRng {
    state: u64,
}

impl CemRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x853c_49e6_748f_a97e
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    #[allow(clippy::cast_precision_loss)]
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }

    /// Generate a normal random sample with mean `mu` and standard deviation `sigma` using Box-Muller.
    fn next_gaussian(&mut self, mu: f64, sigma: f64) -> f64 {
        let u1 = self.next_f64().max(1e-10);
        let u2 = self.next_f64();
        let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        mu + z0 * sigma
    }
}

/// Compute optimal coarse macro-region hints for `board` within `usable` rect using CEM.
#[allow(
    clippy::missing_panics_doc,
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn cem_region_assign(board: &Board, usable: Rect) -> RegionAssignment {
    let mut hints = HashMap::new();
    let width_nm = usable.width_nm();
    let height_nm = usable.height_nm();
    let min_x = usable.min.x_nm;
    let min_y = usable.min.y_nm;
    let max_x = usable.max.x_nm;
    let max_y = usable.max.y_nm;
    let cx = min_x + width_nm / 2;
    let cy = min_y + height_nm / 2;

    // Identify macro components (MCUs, regulators, connectors, RF antennas, memory, sensors)
    let macro_ids: Vec<ComponentId> = board
        .components
        .iter()
        .filter(|c| {
            matches!(
                c.kind.as_str(),
                "mcu"
                    | "processor"
                    | "regulator"
                    | "charger"
                    | "power"
                    | "connector"
                    | "antenna"
                    | "memory"
                    | "sensor"
            )
        })
        .map(|c| c.id)
        .collect();

    if macro_ids.is_empty() {
        return RegionAssignment { hints };
    }

    // Seed RNG deterministically from board topology structure
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    for comp in &board.components {
        for b in comp.refdes.bytes() {
            seed = seed.wrapping_mul(31).wrapping_add(u64::from(b));
        }
    }
    seed = seed.wrapping_add(board.nets.len() as u64);
    let mut rng = CemRng::new(seed);

    // Initialize mean (mu) and std (sigma) for each macro component
    let mut mu_x: HashMap<ComponentId, f64> = HashMap::new();
    let mut mu_y: HashMap<ComponentId, f64> = HashMap::new();
    let mut sigma_x: HashMap<ComponentId, f64> = HashMap::new();
    let mut sigma_y: HashMap<ComponentId, f64> = HashMap::new();

    for &id in &macro_ids {
        let comp = board.components.iter().find(|c| c.id == id).unwrap();
        let target = match comp.kind.as_str() {
            "connector" | "charger" => Point::new(min_x + width_nm / 4, min_y + height_nm / 4),
            "mcu" | "processor" => Point::new(cx, cy),
            "antenna" => Point::new(min_x + (3 * width_nm) / 4, min_y + height_nm / 4),
            "regulator" | "power" => Point::new(min_x + width_nm / 4, min_y + (3 * height_nm) / 4),
            "memory" | "sensor" => {
                Point::new(min_x + (3 * width_nm) / 4, min_y + (3 * height_nm) / 4)
            }
            _ => Point::new(cx, cy),
        };

        mu_x.insert(id, target.x_nm as f64);
        mu_y.insert(id, target.y_nm as f64);
        sigma_x.insert(id, (width_nm as f64) / 4.0);
        sigma_y.insert(id, (height_nm as f64) / 4.0);
    }

    // CEM Hyperparameters
    let num_iterations = 50;
    let pop_size = 200;
    let num_elite = 40; // Top 20%
    let noise_floor_nm = 2_000_000.0; // 2 mm minimum variance

    // Pre-extract net endpoint mappings for fast surrogate HPWL evaluation
    let mut macro_nets: Vec<Vec<ComponentId>> = Vec::new();
    for net in &board.nets {
        let macros_in_net: Vec<ComponentId> = net
            .endpoints
            .iter()
            .map(|ep| ep.component)
            .filter(|cid| macro_ids.contains(cid))
            .collect();
        if macros_in_net.len() >= 2 {
            macro_nets.push(macros_in_net);
        }
    }

    for _iter in 0..num_iterations {
        // Sample candidate positions for each macro
        let mut candidates: Vec<(HashMap<ComponentId, (f64, f64)>, f64)> =
            Vec::with_capacity(pop_size);

        for _s in 0..pop_size {
            let mut sample: HashMap<ComponentId, (f64, f64)> = HashMap::new();
            for &id in &macro_ids {
                let sx = rng
                    .next_gaussian(mu_x[&id], sigma_x[&id])
                    .clamp(min_x as f64, max_x as f64);
                let sy = rng
                    .next_gaussian(mu_y[&id], sigma_y[&id])
                    .clamp(min_y as f64, max_y as f64);
                sample.insert(id, (sx, sy));
            }

            // Calculate cost J: surrogate HPWL + overlap repulsion penalty
            let mut hpwl = 0.0;
            for net in &macro_nets {
                let mut n_min_x = f64::MAX;
                let mut n_max_x = f64::MIN;
                let mut n_min_y = f64::MAX;
                let mut n_max_y = f64::MIN;
                for &id in net {
                    let (px, py) = sample[&id];
                    n_min_x = n_min_x.min(px);
                    n_max_x = n_max_x.max(px);
                    n_min_y = n_min_y.min(py);
                    n_max_y = n_max_y.max(py);
                }
                hpwl += (n_max_x - n_min_x) + (n_max_y - n_min_y);
            }

            // Macro separation penalty (keep macros at least 15mm apart to prevent stacking)
            let min_sep_nm = 15_000_000.0;
            let mut sep_penalty = 0.0;
            for i in 0..macro_ids.len() {
                for j in (i + 1)..macro_ids.len() {
                    let (p1x, p1y) = sample[&macro_ids[i]];
                    let (p2x, p2y) = sample[&macro_ids[j]];
                    let dist = (p1x - p2x).hypot(p1y - p2y);
                    if dist < min_sep_nm {
                        sep_penalty += (min_sep_nm - dist) * 10.0;
                    }
                }
            }

            let total_cost = hpwl + sep_penalty;
            candidates.push((sample, total_cost));
        }

        // Sort candidates by total cost ascending
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Refit distribution parameters on top elite samples
        let elite = &candidates[..num_elite];

        for &id in &macro_ids {
            let mean_x: f64 = elite.iter().map(|c| c.0[&id].0).sum::<f64>() / (num_elite as f64);
            let mean_y: f64 = elite.iter().map(|c| c.0[&id].1).sum::<f64>() / (num_elite as f64);

            let var_x: f64 = elite
                .iter()
                .map(|c| {
                    let diff = c.0[&id].0 - mean_x;
                    diff * diff
                })
                .sum::<f64>()
                / (num_elite as f64);

            let var_y: f64 = elite
                .iter()
                .map(|c| {
                    let diff = c.0[&id].1 - mean_y;
                    diff * diff
                })
                .sum::<f64>()
                / (num_elite as f64);

            mu_x.insert(id, mean_x);
            mu_y.insert(id, mean_y);
            sigma_x.insert(id, var_x.sqrt().max(noise_floor_nm));
            sigma_y.insert(id, var_y.sqrt().max(noise_floor_nm));
        }
    }

    // Populate final hints from converged distribution means
    for &id in &macro_ids {
        let comp = board.components.iter().find(|c| c.id == id).unwrap();
        let mut final_x = (mu_x[&id] as i64).clamp(min_x, max_x);
        let mut final_y = (mu_y[&id] as i64).clamp(min_y, max_y);

        // Edge-docking anchor for connectors: force connector mating face onto nearest board edge
        if comp.kind.as_str() == "connector" || comp.kind.as_str() == "jack" {
            let dist_left = (final_x - min_x).abs();
            let dist_right = (max_x - final_x).abs();
            let dist_top = (final_y - min_y).abs();
            let dist_bottom = (max_y - final_y).abs();

            let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);
            if min_dist == dist_left {
                final_x = min_x;
            } else if min_dist == dist_right {
                final_x = max_x;
            } else if min_dist == dist_top {
                final_y = min_y;
            } else {
                final_y = max_y;
            }
        }

        hints.insert(id, Point::new(final_x, final_y));
    }

    RegionAssignment { hints }
}
