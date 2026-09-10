// SPDX-License-Identifier: Apache-2.0

//! Regression tests for pin reconciliation against real KiCad symbol
//! libraries. Only meaningful when KiCad is installed locally; the
//! reconciliation silently no-ops otherwise (so these tests assert on
//! live-KiCad inputs only and are skipped in CI without KiCad).

use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn build_board(name: &str) -> synth_ir::Board {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry");
    let path = workspace_root().join("fixtures").join("kicad-reference");
    let filename = format!("{name}.synth");
    let src = std::fs::read_to_string(path.join(&filename)).unwrap();
    let parsed = synth_parser::parse(&src, filename.clone());
    let ast = parsed.ast.expect("ast");
    synth_ir::lower(&ast, &registry, &filename)
        .board
        .expect("board")
}

#[test]
fn undriven_nets_ref_usb_cdc() {
    // ref_usb_cdc's +3V3 rail is driven by U2 (AMS1117) vout, a
    // power_out, so it must NOT receive an ERC power driver flag.
    let board = build_board("ref_usb_cdc");
    let layout = synth_layout::layout(&board);
    let placements: std::collections::HashMap<_, _> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    let drivers = synth_kicad::pin_reconcile::undriven_power_nets(&board, &placements);
    for d in &drivers {
        let comp = board.component(d.anchor_component).unwrap();
        assert_ne!(
            comp.refdes, "U2",
            "ref_usb_cdc +3V3 rail (driven by U2.vout) must not receive a PWR_FLAG"
        );
    }
}

#[test]
fn reconcile_fans_out_rp2040_power_legs() {
    if synth_layout::kicad_lib_loader::physical_pins("MCU_RaspberryPi_RP2040:RP2040").is_none() {
        eprintln!("KiCad symbol library not installed locally; skipping pin reconciliation test");
        return;
    }

    // secure_tracker's RP2040 has undeclared physical power legs
    // (ADC_AVDD 43, VREG_VIN 44, USB_VDD 48) that must be fanned onto
    // the rail, and unused GPIOs that must be no-connect.
    let board = build_board("secure_tracker");
    let mut saw_fanout = false;
    for reconciled in synth_kicad::pin_reconcile::reconcile_all(&board) {
        saw_fanout |= !reconciled.power_legs.is_empty();
    }
    assert!(
        saw_fanout,
        "RP2040 power legs should be fanned onto the rail"
    );
}
