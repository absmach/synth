// SPDX-License-Identifier: Apache-2.0

//! Follow-ups from the USB-C dual-UART board (#327): the CH342F modem pin
//! names, and a reviewer on the parts the board relies on.

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
fn the_ch342f_has_one_dsr_and_one_tnow_not_two_tnow_pins() {
    let reg = registry();
    let part = reg.lookup("ch342f").expect("ch342f");
    let names: Vec<&str> = part.pins.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"dsr1"), "{names:?}");
    // TNOW is multiplexed with DTR on pin 15 only; pin 14 is plain DSR.
    let pin14 = part
        .pins
        .iter()
        .find(|pin| pin.number.0.as_str() == "14")
        .expect("pin 14");
    assert_eq!(pin14.name, "dsr1");
    let tn: Vec<_> = names.iter().filter(|name| name.contains("tnow")).collect();
    assert_eq!(tn, vec![&"dtr1_tnow1", &"dtr0_tnow0"], "{names:?}");
}

#[test]
fn the_reviewed_usb_uart_parts_name_a_reviewer() {
    let reg = registry();
    for id in ["ch342f", "thvd1450d"] {
        let part = reg.lookup(id).unwrap_or_else(|| panic!("{id}"));
        let provenance = part
            .provenance
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no provenance"));
        assert!(
            provenance
                .reviewed_by
                .as_deref()
                .is_some_and(|name| !name.is_empty()),
            "{id}: no reviewer"
        );
    }
}
