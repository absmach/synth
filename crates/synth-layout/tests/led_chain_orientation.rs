// SPDX-License-Identifier: Apache-2.0

//! A resistor-fed LED is drawn with its anode toward the resistor and its
//! cathode away from it, whichever of the part's two pins the registry lists
//! first. Turned the other way the ground symbol hangs above the LED and a
//! wire loops from the resistor round the body to the anode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use synth_ir::{Board, PinId};
use synth_layout::Layout;
use synth_layout::route::pin_terminal_xy;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// `VCC -> R1 -> LED -> GND`, the LED being the registry part `led`.
fn led_board(led: &str) -> Board {
    let source = format!(
        r#"board "led_chain" {{
  layers 2
  manufacturer "jlcpcb"

  component D1: led "{led}"
  component R1: resistor "r_generic_0603"

  net "VCC" {{
    R1.p1
  }}

  net "GND" {{
    D1.cathode
  }}

  connect R1.p2 -> D1.anode
}}
"#
    );
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let ast = synth_parser::parse(&source, "led_chain.synth".to_string())
        .ast
        .unwrap();
    synth_ir::lower(&ast, &registry, "led_chain.synth")
        .board
        .unwrap()
}

fn pin_xy(board: &Board, layout: &Layout, refdes: &str, pin_name: &str) -> (f64, f64) {
    let component = board
        .components
        .iter()
        .find(|c| c.refdes == refdes)
        .unwrap();
    let pin = component
        .part
        .as_ref()
        .unwrap()
        .pins
        .iter()
        .position(|p| p.name == pin_name)
        .unwrap();
    let placements: HashMap<_, _> = layout.components.iter().map(|p| (p.id, p)).collect();
    pin_terminal_xy(
        board,
        component.id,
        PinId(u32::try_from(pin).unwrap()),
        &placements,
    )
    .map(|(x, y, _, _)| (x, y))
    .unwrap()
}

fn pin_y(board: &Board, layout: &Layout, refdes: &str, pin_name: &str) -> f64 {
    pin_xy(board, layout, refdes, pin_name).1
}

/// One led part with its anode listed first, one with its cathode first.
const LEDS: [&str; 2] = ["led_red_0603", "ltst_c190kgkt"];

#[test]
fn the_anode_is_above_the_cathode_under_its_resistor() {
    for led in LEDS {
        let board = led_board(led);
        let layout = synth_layout::layout(&board);
        let resistor = pin_y(&board, &layout, "R1", "p2");
        let anode = pin_y(&board, &layout, "D1", "anode");
        let cathode = pin_y(&board, &layout, "D1", "cathode");
        assert!(
            resistor < anode && anode < cathode,
            "{led}: resistor pin at {resistor}, anode at {anode}, cathode at {cathode}"
        );
    }
}

#[test]
fn the_resistor_the_led_and_the_wire_between_them_share_one_column() {
    for led in LEDS {
        let board = led_board(led);
        let layout = synth_layout::layout(&board);
        let resistor_x = pin_xy(&board, &layout, "R1", "p2").0;
        let anode_x = pin_xy(&board, &layout, "D1", "anode").0;
        assert!(
            (resistor_x - anode_x).abs() < 0.01,
            "{led}: resistor pin at x {resistor_x}, anode at x {anode_x}"
        );
        let resistor = board.components.iter().find(|c| c.refdes == "R1").unwrap();
        let net = board
            .nets
            .iter()
            .find(|n| {
                n.endpoints
                    .iter()
                    .any(|e| e.component == resistor.id && e.pin == PinId(1))
            })
            .unwrap()
            .id;
        assert!(
            layout.wires.iter().any(|w| w.net == net),
            "{led}: no wire joins the resistor and the anode"
        );
        for wire in layout.wires.iter().filter(|w| w.net == net) {
            assert!(
                wire.points.iter().all(|p| (p.0 - anode_x).abs() < 0.01),
                "{led}: wire bends: {:?}",
                wire.points
            );
        }
    }
}
