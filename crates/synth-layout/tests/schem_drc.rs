// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the wire-through-unrelated-pin DRC check
//! (§7.7 gate). These exercise [`synth_layout::route::drc_wire_crosses_unrelated_pin_terminal`]
//! against a real lowered board: the canonical router must never route
//! a wire over the pin terminal of a component/pin its net does not
//! connect to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use synth_ir::{Board, PinId};
use synth_layout::WirePath;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(fixture_rel_path: &str) -> Board {
    let path = workspace_root().join(fixture_rel_path);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let ast = synth_parser::parse(&source, filename.clone()).ast.unwrap();
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    synth_ir::lower(&ast, &registry, &filename).board.unwrap()
}

/// The §7.7 gate: on a reasonable board the canonical router must not
/// draw any wire through the pin terminal of an unrelated component.
/// Asserts the DRC check returns no violations across the fixture.
#[test]
fn test_no_wire_crosses_unrelated_pin_terminals() {
    for rel in [
        "examples/sensor_logger.synth",
        "fixtures/layout/led_indicator.synth",
        "fixtures/layout/i2c_bus.synth",
        "fixtures/layout/ldo_block.synth",
        "fixtures/layout/decoupling.synth",
        "fixtures/layout/usb_diff_pair.synth",
    ] {
        let board = board_for(rel);
        let layout = synth_layout::layout(&board);
        let violations =
            synth_layout::route::drc_wire_crosses_unrelated_pin_terminal(&board, &layout);
        assert!(
            violations.is_empty(),
            "{rel}: wire-through-unrelated-pin violations: {violations:#?}"
        );
    }
}

/// Negative control: the DRC detector must *flag* a wire deliberately
/// routed through an unrelated pin terminal, proving the pass isn't
/// vacuously true. We take a clean routed layout, inject one
/// horizontal wire on a real net that runs straight through a pin
/// terminal of a component that net does not connect to, and assert
/// the DRC reports exactly that crossing.
#[test]
fn drc_detects_an_injected_wire_through_an_unrelated_pin() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    assert!(
        synth_layout::route::drc_wire_crosses_unrelated_pin_terminal(&board, &layout).is_empty(),
        "baseline must be clean before injecting a violation"
    );

    let placements: HashMap<synth_ir::ComponentId, &synth_layout::ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();

    // Pick a multi-pin net and a component not on it.
    let net = board
        .nets
        .iter()
        .find(|n| n.endpoints.len() >= 2)
        .expect("fixture must have a 2+ endpoint net");
    let on_net = |cid: synth_ir::ComponentId| net.endpoints.iter().any(|e| e.component == cid);
    let unrelated = board
        .components
        .iter()
        .find(|c| !on_net(c.id) && c.part.is_some())
        .expect("fixture must have a component not on the chosen net");

    // Its first pin terminal is unrelated to `net`.
    let terminal =
        synth_layout::route::pin_terminal_xy(&board, unrelated.id, PinId(0), &placements)
            .expect("unrelated component must have a pin 0 terminal");
    let (tx, ty, _dx, _dy) = terminal;

    let mut mutated = layout.clone();
    mutated.wires.push(WirePath {
        net: net.id,
        points: vec![(tx - 30.0, ty), (tx + 30.0, ty)],
        junctions: Vec::new(),
    });

    let violations = synth_layout::route::drc_wire_crosses_unrelated_pin_terminal(&board, &mutated);
    let hit = violations
        .iter()
        .find(|v| v.net == net.id && v.refdes == unrelated.refdes);
    assert!(
        hit.is_some(),
        "expected a violation over {} pin 0 on net {:?}, got {violations:#?}",
        unrelated.refdes,
        net.id
    );
}
