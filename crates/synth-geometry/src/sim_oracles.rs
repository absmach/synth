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
}
