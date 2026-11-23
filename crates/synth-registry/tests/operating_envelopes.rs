// SPDX-License-Identifier: Apache-2.0

//! A declared operating envelope must be the device's, not the seed default
//! of 5 V / 100 mA (#275).

use std::path::Path;

use synth_registry::load_dir;

fn registry() -> synth_registry::Registry {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root");
    load_dir(&root.join("registry").join("parts")).expect("the registry")
}

#[test]
fn connectors_and_discretes_no_longer_carry_the_seed_envelope() {
    let reg = registry();
    let expected: &[(&str, f64, f64)] = &[
        ("jst_xh_2pin", 250.0, 3000.0),
        ("jst_ph_2pin", 100.0, 2000.0),
        ("jst_ph_4pin", 100.0, 2000.0),
        ("p_mosfet_si2301", 20.0, 3100.0),
        ("polyfuse", 16.0, 3000.0),
        ("header_1x3", 250.0, 3000.0),
        ("header_1x4", 250.0, 3000.0),
        ("header_1x20", 250.0, 3000.0),
    ];
    for &(id, volts, milliamps) in expected {
        let part = reg.lookup(id).unwrap_or_else(|| panic!("{id}"));
        let oc = part
            .operating_conditions
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no operating conditions"));
        assert_eq!(oc.max_voltage_v, Some(volts), "{id}: voltage");
        assert_eq!(oc.max_current_ma, Some(milliamps), "{id}: current");
        assert_ne!(
            (oc.max_voltage_v, oc.max_current_ma),
            (Some(5.0), Some(100.0)),
            "{id} still carries the seed default"
        );
    }
}
