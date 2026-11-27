// SPDX-License-Identifier: Apache-2.0

//! absmach/synth#233: a board that only packs on one of the fixed fallback
//! canvases is re-placed on the envelope its parts occupied, so its outline
//! follows the circuit rather than the canvas.

use std::fmt::Write as _;

fn load_source(name: &str, source: &str) -> synth_ir::Board {
    let file = format!("{name}.synth");
    let parse = synth_parser::parse(source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry =
        synth_registry::load_dir(std::path::Path::new("../../registry/parts")).expect("registry");
    synth_ir::lower(ast, &registry, &file).board.expect("board")
}

/// Two headers hard-hinted to opposite edges and forty series resistors held
/// near the MCU: the courtyard-sized canvases fail and the 100 x 80 mm canvas
/// is the first that packs. Measured on `main` before the fix, the outline was
/// 61.12 x 69.46 mm (4245 mm2); re-placing on the envelope gives 45.20 x
/// 61.46 mm (2778 mm2).
#[test]
#[allow(clippy::cast_precision_loss)]
fn a_board_packed_on_a_fallback_canvas_shrinks_to_its_envelope() {
    let mut source = String::from(
        "board \"fallback_canvas\" {\n  layers 2\n\
         component J1: connector \"header_1x20\" { placement_hint { region: left_edge edge: left priority: hard } }\n\
         component J2: connector \"header_1x20\" { placement_hint { region: right_edge edge: right priority: hard } }\n\
         component U1: mcu \"rp2350\"\n",
    );
    for i in 0..40 {
        writeln!(
            source,
            "  component R{i}: resistor \"r_generic_0603\" value \"33R\" {{ placement_hint {{ near: U1 priority: hard }} }}"
        )
        .expect("write");
    }
    for i in 0..20 {
        writeln!(
            source,
            "  connect J1.p{pin} -> R{i}.p1\n  connect R{i}.p2 -> J2.p{pin}",
            pin = i + 1
        )
        .expect("write");
    }
    source.push_str("}\n");
    let board = load_source("fallback_canvas", &source);
    let placement = synth_place::place(&board).expect("place");
    let outline = placement.board_outline;
    let area_mm2 = outline.width_nm() as f64 * outline.height_nm() as f64 / 1e12;
    assert!(
        area_mm2 < 3400.0,
        "outline {:.2} x {:.2} mm ({area_mm2:.0} mm2) kept the fallback canvas's spread",
        outline.width_nm() as f64 / 1e6,
        outline.height_nm() as f64 / 1e6,
    );
    // The headers still sit on opposite edges of the smaller board.
    let x_of = |refdes: &str| {
        let id = board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .expect(refdes)
            .id;
        placement
            .components
            .iter()
            .find(|p| p.id == id)
            .expect("placed")
            .center
            .x_nm
    };
    assert!(x_of("J1") < x_of("J2"), "J1 left of J2");
}
