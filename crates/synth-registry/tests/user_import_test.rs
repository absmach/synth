// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use synth_registry::load_tiered;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[test]
fn test_simulated_user_imported_parts_tier() {
    let global_dir = workspace_root().join("registry").join("parts");
    let user_fixture_dir = workspace_root().join("fixtures").join("user-parts");

    assert!(global_dir.exists(), "global registry directory must exist");
    assert!(
        user_fixture_dir.exists(),
        "fixtures/user-parts directory must exist"
    );

    // Load tiered registry combining core shipped parts + simulated user imports
    let result = load_tiered(&global_dir, &user_fixture_dir, false)
        .expect("tiered load with simulated user parts must succeed");

    // Core parts must still be present
    assert!(
        result.registry.lookup("rp2350").is_some(),
        "core part rp2350 must be present in tiered registry"
    );

    // User-imported parts from fixtures must be present
    let esp = result
        .registry
        .lookup("esp32_c61_mini")
        .expect("user-imported esp32_c61_mini must be found");
    assert_eq!(esp.kind, "mcu");
    assert_eq!(
        esp.kicad_footprint.as_deref(),
        Some("RF_Module:ESP32-C61-MINI-1")
    );

    let sensor = result
        .registry
        .lookup("custom_imu_sensor")
        .expect("user-imported custom_imu_sensor must be found");
    assert_eq!(sensor.kind, "sensor");

    let header = result
        .registry
        .lookup("conn_custom_header")
        .expect("user-imported conn_custom_header must be found");
    assert_eq!(header.kind, "connector");

    // Zero unexpected warnings since user fixture parts have unique IDs
    assert!(
        result.warnings.is_empty(),
        "expected zero warnings for unique fixture parts, found: {:?}",
        result.warnings
    );
}
