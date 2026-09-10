// SPDX-License-Identifier: Apache-2.0

//! Sub-Phase 12b SI & Thermal Physics Oracles Accuracy & Latency Test.

use std::time::Instant;
use synth_geometry::{
    calculate_microstrip_z0, calculate_stripline_z0, estimate_component_thermal, MicrostripParams,
};

#[test]
fn test_microstrip_z0_accuracy_and_latency() {
    let params = MicrostripParams {
        width_mm: 0.30,
        height_mm: 0.16,
        thickness_mm: 0.035,
        er: 4.3,
    };

    let start = Instant::now();
    let res = calculate_microstrip_z0(&params);
    let duration = start.elapsed();

    println!(
        "[Sub-Phase 12b] Microstrip Z0: {:.2} ohms in {:?}",
        res.z0_ohms, duration
    );
    println!(
        "[Sub-Phase 12b] Propagation Delay: {:.3} ns/m",
        res.propagation_delay_ns_m
    );

    // 1. Latency Gate Check (<1.0ms)
    assert!(
        duration.as_micros() < 1000,
        "SI oracle execution latency ({duration:?}) must be < 1.0ms"
    );

    // 2. Accuracy Gate Check (+/- 5% of IPC-2141 microstrip reference 45.39 ohms)
    assert!(
        (res.z0_ohms - 45.39).abs() < 2.5,
        "Calculated Z0 ({:.2}) must be within +/- 5% of reference 45.39 ohms",
        res.z0_ohms
    );
}

#[test]
fn test_stripline_z0_calculation() {
    let res = calculate_stripline_z0(0.20, 0.32, 0.035, 4.3);
    assert!(res.z0_ohms > 0.0);
    assert!(res.propagation_delay_ns_m > 0.0);
}

#[test]
fn test_thermal_power_dissipation_estimate() {
    let est = estimate_component_thermal("U1", 1.5, 45.0, 25.0);
    assert_eq!(est.estimated_temp_c, 92.5);
    assert!(
        est.hotspot_warning,
        "Temperature 92.5C must trigger hotspot warning threshold > 85C"
    );
}
