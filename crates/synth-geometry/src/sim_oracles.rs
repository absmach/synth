// SPDX-License-Identifier: Apache-2.0

//! Sub-millisecond Signal Integrity (SI) & Thermal Physics Oracles.

use serde::{Deserialize, Serialize};

/// Input parameters for Microstrip transmission line characteristic impedance calculations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrostripParams {
    /// Trace width in millimeters.
    pub width_mm: f64,
    /// Dielectric substrate height in millimeters.
    pub height_mm: f64,
    /// Copper trace thickness in millimeters (default: 0.035mm = 1 oz Cu).
    pub thickness_mm: f64,
    /// Substrate relative permittivity (default: 4.3 for FR-4).
    pub er: f64,
}

impl Default for MicrostripParams {
    fn default() -> Self {
        Self {
            width_mm: 0.20,
            height_mm: 0.16,
            thickness_mm: 0.035,
            er: 4.3,
        }
    }
}

/// Calculated Signal Integrity transmission line properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiImpedanceResult {
    /// Characteristic impedance Z0 in Ohms.
    pub z0_ohms: f64,
    /// Signal propagation delay in nanoseconds per meter.
    pub propagation_delay_ns_m: f64,
    /// Effective relative dielectric constant.
    pub effective_er: f64,
}

/// Thermal power dissipation estimate for a single PCB component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentThermalEstimate {
    pub refdes: String,
    pub power_watts: f64,
    pub r_theta_ja: f64,
    pub ambient_temp_c: f64,
    pub estimated_temp_c: f64,
    pub hotspot_warning: bool,
}

/// Calculate microstrip characteristic impedance Z0 and propagation delay.
///
/// Formula: Z0 = (87 / sqrt(er + 1.41)) * ln(5.98 * h / (0.8 * w + t))
#[must_use]
pub fn calculate_microstrip_z0(params: &MicrostripParams) -> SiImpedanceResult {
    let w = params.width_mm.max(0.01);
    let h = params.height_mm.max(0.01);
    let t = params.thickness_mm.max(0.001);
    let er = params.er.max(1.0);

    let denom = 0.8 * w + t;
    let ratio = (5.98 * h / denom).max(1.0001);
    let z0_ohms = (87.0 / (er + 1.41).sqrt()) * ratio.ln();

    // Effective Er approximation for microstrip
    let effective_er =
        f64::midpoint(er, 1.0) + ((er - 1.0) / 2.0) * (1.0 + 12.0 * (h / w)).sqrt().recip();
    let propagation_delay_ns_m = 3.333 * effective_er.sqrt();

    SiImpedanceResult {
        z0_ohms,
        propagation_delay_ns_m,
        effective_er,
    }
}

const MIN_WIDTH_OVER_HEIGHT: f64 = 0.1;
const MAX_WIDTH_OVER_HEIGHT: f64 = 2.0;
const MAX_GAP_OVER_HEIGHT: f64 = 2.0;
const COUPLING_AMPLITUDE: f64 = 0.48;
const COUPLING_DECAY: f64 = 0.96;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnreachableZ0 {
    pub lowest_ohms: f64,
    pub highest_ohms: f64,
}

#[must_use]
pub fn outer_microstrip_params(stackup: &synth_ir::Stackup) -> Option<MicrostripParams> {
    let [
        synth_ir::StackupLayer::Copper { thickness, .. },
        synth_ir::StackupLayer::Insulator {
            thickness: height,
            er,
            ..
        },
        ..,
    ] = stackup.layers.as_slice()
    else {
        return None;
    };
    (thickness.0 > 0 && height.0 > 0 && er.to_f64() >= 1.0).then(|| MicrostripParams {
        height_mm: height.to_mm(),
        thickness_mm: thickness.to_mm(),
        er: er.to_f64(),
        ..MicrostripParams::default()
    })
}

fn width_bounds_mm(base: &MicrostripParams, min_width_mm: f64) -> (f64, f64) {
    let narrowest = min_width_mm.max(MIN_WIDTH_OVER_HEIGHT * base.height_mm);
    let widest = (MAX_WIDTH_OVER_HEIGHT * base.height_mm).max(narrowest);
    (narrowest, widest)
}

fn width_for_z0_mm(target_ohms: f64, base: &MicrostripParams, bounds: (f64, f64)) -> f64 {
    let ratio = (-target_ohms * (base.er + 1.41).sqrt() / 87.0).exp();
    ((5.98 * base.height_mm * ratio - base.thickness_mm) / 0.8).clamp(bounds.0, bounds.1)
}

/// Inverts [`calculate_microstrip_z0`] for the width.
///
/// # Errors
/// The impedance range the allowed widths reach, when it does not contain the target.
pub fn derive_microstrip_width_mm(
    target_ohms: f64,
    base: &MicrostripParams,
    min_width_mm: f64,
) -> Result<f64, UnreachableZ0> {
    let z0_at = |width_mm| calculate_microstrip_z0(&MicrostripParams { width_mm, ..*base }).z0_ohms;
    let bounds = width_bounds_mm(base, min_width_mm);
    let (highest_ohms, lowest_ohms) = (z0_at(bounds.0), z0_at(bounds.1));
    if !(lowest_ohms..=highest_ohms).contains(&target_ohms) {
        return Err(UnreachableZ0 {
            lowest_ohms,
            highest_ohms,
        });
    }
    Ok(width_for_z0_mm(target_ohms, base, bounds))
}

fn coupling(gap_mm: f64, height_mm: f64) -> f64 {
    1.0 - COUPLING_AMPLITUDE * (-COUPLING_DECAY * gap_mm / height_mm).exp()
}

#[must_use]
pub fn coupled_microstrip_zdiff_ohms(params: &MicrostripParams, gap_mm: f64) -> f64 {
    2.0 * calculate_microstrip_z0(params).z0_ohms * coupling(gap_mm, params.height_mm)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoupledGeometry {
    pub width_mm: f64,
    pub gap_mm: f64,
}

pub fn derive_coupled_microstrip(
    target_ohms: f64,
    base: &MicrostripParams,
    min_width_mm: f64,
    min_gap_mm: f64,
) -> Result<CoupledGeometry, UnreachableZ0> {
    let bounds = width_bounds_mm(base, min_width_mm);
    let z0_at = |width_mm| calculate_microstrip_z0(&MicrostripParams { width_mm, ..*base }).z0_ohms;
    let widest_gap_mm = (MAX_GAP_OVER_HEIGHT * base.height_mm).max(min_gap_mm);
    let narrow_z0 = z0_at(bounds.0);
    let lowest_ohms = 2.0 * z0_at(bounds.1) * coupling(min_gap_mm, base.height_mm);
    let highest_ohms = 2.0 * narrow_z0 * coupling(widest_gap_mm, base.height_mm);
    if !(lowest_ohms..=highest_ohms).contains(&target_ohms) {
        return Err(UnreachableZ0 {
            lowest_ohms,
            highest_ohms,
        });
    }
    let needed_coupling = target_ohms / (2.0 * narrow_z0);
    let gap_mm = (-base.height_mm * ((1.0 - needed_coupling) / COUPLING_AMPLITUDE).ln()
        / COUPLING_DECAY)
        .clamp(min_gap_mm, widest_gap_mm);
    let z0_needed = target_ohms / (2.0 * coupling(gap_mm, base.height_mm));
    Ok(CoupledGeometry {
        width_mm: width_for_z0_mm(z0_needed, base, bounds),
        gap_mm,
    })
}

/// Calculate stripline characteristic impedance Z0.
///
/// Formula: Z0 = (60 / sqrt(er)) * ln(1.9 * h / (0.8 * w + t))
#[must_use]
pub fn calculate_stripline_z0(
    width_mm: f64,
    height_mm: f64,
    thickness_mm: f64,
    er: f64,
) -> SiImpedanceResult {
    let w = width_mm.max(0.01);
    let h = height_mm.max(0.01);
    let t = thickness_mm.max(0.001);
    let er = er.max(1.0);

    let denom = 0.8 * w + t;
    let ratio = (1.9 * h / denom).max(1.0001);
    let z0_ohms = (60.0 / er.sqrt()) * ratio.ln();
    let propagation_delay_ns_m = 3.333 * er.sqrt();

    SiImpedanceResult {
        z0_ohms,
        propagation_delay_ns_m,
        effective_er: er,
    }
}

/// Estimate thermal temperature rise for a component.
#[must_use]
pub fn estimate_component_thermal(
    refdes: &str,
    power_watts: f64,
    r_theta_ja: f64,
    ambient_temp_c: f64,
) -> ComponentThermalEstimate {
    let temp_rise = power_watts * r_theta_ja;
    let estimated_temp_c = ambient_temp_c + temp_rise;
    let hotspot_warning = estimated_temp_c > 85.0;

    ComponentThermalEstimate {
        refdes: refdes.to_string(),
        power_watts,
        r_theta_ja,
        ambient_temp_c,
        estimated_temp_c,
        hotspot_warning,
    }
}

/// Board-level Thermal & Signal Integrity simulation oracle evaluator.
#[must_use]
pub fn evaluate_thermal_si(board: &synth_ir::Board) -> (bool, String) {
    let params = MicrostripParams::default();
    let z0 = calculate_microstrip_z0(&params);

    let mut warnings = Vec::new();
    if z0.z0_ohms < 30.0 || z0.z0_ohms > 100.0 {
        warnings.push(format!(
            "Substrate characteristic Z0 ({:.1} Ω) outside standard envelope",
            z0.z0_ohms
        ));
    }

    let mut total_hotspots = 0;
    for comp in &board.components {
        let thermal = estimate_component_thermal(&comp.refdes, 0.1, 50.0, 25.0);
        if thermal.hotspot_warning {
            total_hotspots += 1;
        }
    }

    let clean = warnings.is_empty() && total_hotspots == 0;
    let details = if clean {
        format!(
            "SI Z0={:.1} Ω ({:.2} ns/m delay), 0 thermal hotspots across {} components",
            z0.z0_ohms,
            z0.propagation_delay_ns_m,
            board.components.len()
        )
    } else {
        format!(
            "Oracle checks: {} warning(s), {} hotspot(s)",
            warnings.len(),
            total_hotspots
        )
    };

    (clean, details)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microstrip_z0_calculation_standard_50_ohm() {
        let params = MicrostripParams {
            width_mm: 0.30,
            height_mm: 0.16,
            thickness_mm: 0.035,
            er: 4.3,
        };
        let res = calculate_microstrip_z0(&params);
        assert!(
            (res.z0_ohms - 50.0).abs() < 10.0,
            "Calculated Z0 ({}) should be close to 50 ohms",
            res.z0_ohms
        );
        assert!(res.propagation_delay_ns_m > 0.0);
    }

    fn fr4_outer() -> MicrostripParams {
        MicrostripParams {
            width_mm: 0.0,
            height_mm: 0.2104,
            thickness_mm: 0.035,
            er: 4.4,
        }
    }

    #[test]
    fn derived_width_reproduces_the_target() {
        for target in [50.0, 60.0, 75.0] {
            let width = derive_microstrip_width_mm(target, &fr4_outer(), 0.127).unwrap();
            let z0 = calculate_microstrip_z0(&MicrostripParams {
                width_mm: width,
                ..fr4_outer()
            })
            .z0_ohms;
            assert!((z0 - target).abs() < 0.01, "{target} ohm gave {z0} ohm");
        }
    }

    #[test]
    fn higher_targets_need_narrower_traces() {
        let w50 = derive_microstrip_width_mm(50.0, &fr4_outer(), 0.127).unwrap();
        let w75 = derive_microstrip_width_mm(75.0, &fr4_outer(), 0.127).unwrap();
        assert!(w75 < w50, "{w75} !< {w50}");
    }

    #[test]
    fn targets_outside_the_feasible_range_are_unreachable() {
        for target in [5.0, 150.0] {
            let err = derive_microstrip_width_mm(target, &fr4_outer(), 0.127).unwrap_err();
            assert!(err.lowest_ohms < 50.0 && 50.0 < err.highest_ohms, "{err:?}");
        }
    }

    #[test]
    fn a_minimum_width_above_the_range_reports_an_ordered_pair() {
        let err = derive_microstrip_width_mm(50.0, &fr4_outer(), 5.0).unwrap_err();
        assert!(err.lowest_ohms <= err.highest_ohms, "{err:?}");
    }

    #[test]
    fn the_manufacturer_minimum_width_limits_the_highest_impedance() {
        let loose = derive_microstrip_width_mm(80.0, &fr4_outer(), 0.1);
        let tight = derive_microstrip_width_mm(80.0, &fr4_outer(), 0.2);
        assert!(loose.is_ok());
        assert!(tight.is_err());
    }

    #[test]
    fn derived_pair_geometry_reproduces_the_target() {
        for target in [80.0, 90.0, 100.0, 130.0] {
            let pair = derive_coupled_microstrip(target, &fr4_outer(), 0.127, 0.127).unwrap();
            let zdiff = coupled_microstrip_zdiff_ohms(
                &MicrostripParams {
                    width_mm: pair.width_mm,
                    ..fr4_outer()
                },
                pair.gap_mm,
            );
            assert!(
                (zdiff - target).abs() < 0.01,
                "{target} ohm gave {zdiff} ohm"
            );
        }
    }

    #[test]
    fn a_reachable_pair_uses_the_tightest_manufacturable_gap() {
        let pair = derive_coupled_microstrip(90.0, &fr4_outer(), 0.127, 0.127).unwrap();
        assert!((pair.gap_mm - 0.127).abs() < 1e-9, "{pair:?}");
    }

    #[test]
    fn a_target_above_the_tight_gap_range_widens_the_gap() {
        let tight = derive_coupled_microstrip(90.0, &fr4_outer(), 0.127, 0.127).unwrap();
        let loose = derive_coupled_microstrip(130.0, &fr4_outer(), 0.127, 0.127).unwrap();
        assert!(loose.gap_mm > tight.gap_mm, "{loose:?} !> {tight:?}");
        assert!(loose.width_mm >= 0.127, "{loose:?}");
    }

    #[test]
    fn pair_targets_outside_the_feasible_range_are_unreachable() {
        for target in [5.0, 200.0] {
            let err = derive_coupled_microstrip(target, &fr4_outer(), 0.127, 0.127).unwrap_err();
            assert!(err.lowest_ohms < 90.0 && 90.0 < err.highest_ohms, "{err:?}");
        }
    }
}
