// SPDX-License-Identifier: Apache-2.0

//! A ground symbol on a header pin must stay in its own pin's row.
//!
//! The symbol hangs one grid step out from its pin and its triangle drops one
//! grid step further, which is exactly the pitch of a header, so on pin 1 the
//! tip landed on pin 2's row: across its wire, its number and its flag. With
//! that area covered the name moved on to pin 3's row and read as pin 3's
//! ground. The check goes through the real export and reads the symbol, its
//! name, the flags and the pin positions back.

use std::path::{Path, PathBuf};

type Rect = (f64, f64, f64, f64);

/// Flag box in mm: 1.12 per character plus 2.9, and 2.65 tall.
const FLAG_CHAR_MM: f64 = 1.12;
const FLAG_PAD_MM: f64 = 2.9;
const FLAG_H_MM: f64 = 2.65;
/// Half the width of a ground triangle, and how far its tip is from the pin.
const TRIANGLE_HALF_MM: f64 = 1.27;
const TRIANGLE_TIP_MM: f64 = 2.54;
/// Half the height of a pin's row: its line, its number and its wire. Rows on a
/// 2.54 mm pitch touch.
const ROW_HALF_MM: f64 = 1.27;
/// Text of 1.27 mm: width of one character and height.
const TEXT_CHAR_MM: f64 = 1.0;
const TEXT_H_MM: f64 = 1.27;
/// How far a row reaches out from its pin, in mm.
const ROW_REACH_MM: f64 = 40.0;
/// Boxes that overlap by less than this in either direction only touch.
const TOUCH_MM: f64 = 0.2;

/// A header whose pin 1 is ground and whose pins 2 and 3 carry flagged signals.
const SOURCE: &str = r#"board "ground_rows" {
  layers 2
  manufacturer "jlcpcb"

  component U1: ic "ch340g"
  component J2: connector "header_1x3"

  net "GND" {
    U1.gnd, J2.p1
  }

  net "CTS_OUT" {
    U1.cts, U1.dsr, J2.p2
  }

  net "DTR_OUT" {
    U1.dtr, U1.rts, J2.p3
  }
}
"#;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn load_board(source: &str) -> synth_ir::Board {
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let ast = synth_parser::parse(source, "ground_rows.synth".to_string())
        .ast
        .unwrap();
    synth_ir::lower(&ast, &registry, "ground_rows.synth")
        .board
        .unwrap()
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.2.min(b.2) - a.0.max(b.0) > TOUCH_MM && a.3.min(b.3) - a.1.max(b.1) > TOUCH_MM
}

/// The numbers of the first `(at ...)` in `text`.
fn at_of(text: &str) -> (f64, f64, f64) {
    let at = text.split("(at ").nth(1).expect("an (at ...)");
    let mut nums = at
        .split(|c: char| c.is_whitespace() || c == ')')
        .filter_map(|t| t.parse::<f64>().ok());
    (
        nums.next().unwrap(),
        nums.next().unwrap(),
        nums.next().unwrap(),
    )
}

/// Every global label in `sch` as (name, box).
#[allow(clippy::cast_precision_loss)] // a net name is a handful of characters
fn flags(sch: &str) -> Vec<(String, Rect)> {
    sch.split("(global_label")
        .skip(1)
        .map(|block| {
            let name = block.split('"').nth(1).expect("label name");
            let (x, y, angle) = at_of(block);
            let w = name.chars().count() as f64 * FLAG_CHAR_MM + FLAG_PAD_MM;
            let (x0, x1) = if angle == 180.0 {
                (x - w, x)
            } else {
                (x, x + w)
            };
            (
                name.to_string(),
                (x0, y - FLAG_H_MM / 2.0, x1, y + FLAG_H_MM / 2.0),
            )
        })
        .collect()
}

/// What a ground symbol at `angle` draws: its triangle and its name. A symbol at
/// 0 points down, and KiCad turns it counter-clockwise on the sheet, so 90
/// points right and 270 left.
#[allow(clippy::cast_precision_loss)] // a net name is a handful of characters
fn drawn(block: &str) -> (Rect, Rect) {
    let (x, y, angle) = at_of(block);
    let triangle = match angle as i32 {
        0 => (
            x - TRIANGLE_HALF_MM,
            y,
            x + TRIANGLE_HALF_MM,
            y + TRIANGLE_TIP_MM,
        ),
        90 => (
            x,
            y - TRIANGLE_HALF_MM,
            x + TRIANGLE_TIP_MM,
            y + TRIANGLE_HALF_MM,
        ),
        270 => (
            x - TRIANGLE_TIP_MM,
            y - TRIANGLE_HALF_MM,
            x,
            y + TRIANGLE_HALF_MM,
        ),
        other => panic!("a ground symbol at {other} degrees"),
    };
    let value = block
        .split("\"Value\"")
        .nth(1)
        .expect("a Value property")
        .split("(property")
        .next()
        .unwrap();
    let name = value.split('"').nth(1).expect("the name");
    let (nx, ny, field_angle) = at_of(value);
    let mut justify = value
        .split("(justify ")
        .nth(1)
        .map(|j| j.split(')').next().unwrap().trim().to_string());
    // KiCad draws a string upside down the right way up and swaps its
    // justification on the way.
    if (angle + field_angle).rem_euclid(360.0) == 180.0 {
        justify = match justify.as_deref() {
            Some("left") => Some("right".to_string()),
            Some("right") => Some("left".to_string()),
            _ => justify,
        };
    }
    let w = name.chars().count() as f64 * TEXT_CHAR_MM;
    let (x0, x1) = match justify.as_deref() {
        Some("left") => (nx, nx + w),
        Some("right") => (nx - w, nx),
        _ => (nx - w / 2.0, nx + w / 2.0),
    };
    (
        triangle,
        (x0, ny - TEXT_H_MM / 2.0, x1, ny + TEXT_H_MM / 2.0),
    )
}

#[test]
fn a_ground_symbol_on_a_header_pin_stays_in_its_own_row() {
    let board = load_board(SOURCE);
    let dir = tempfile::tempdir().unwrap();
    let result = synth_kicad::export_schematic_only(&board, dir.path(), None).expect("export");
    let sch = std::fs::read_to_string(&result.schematic_path).unwrap();
    let flags = flags(&sch);
    assert!(flags.len() >= 2, "pins 2 and 3 are meant to carry flags");

    let layout = synth_layout::layout_with_sidecar(&board, None);
    let placements: std::collections::HashMap<_, _> =
        layout.components.iter().map(|p| (p.id, p)).collect();
    let header = board.components.iter().find(|c| c.refdes == "J2").unwrap();
    let terminal = |pin: u32| {
        synth_layout::route::pin_terminal_xy(&board, header.id, synth_ir::PinId(pin), &placements)
            .unwrap()
    };
    let (x1, y1, dx, _) = terminal(0);
    assert!(dx != 0.0, "the header's pins are meant to face sideways");
    // The rows of pins 2 and 3 run out from their terminals.
    let rows: Vec<Rect> = (1..3)
        .map(|pin| {
            let (x, y, ..) = terminal(pin);
            let (a, b) = (x, x + dx * ROW_REACH_MM);
            (a.min(b), y - ROW_HALF_MM, a.max(b), y + ROW_HALF_MM)
        })
        .collect();

    // The ground symbols one grid step from J2's pin 1.
    let own: Vec<&str> = sch
        .split("(lib_id \"")
        .skip(1)
        .filter(|block| block.starts_with("power:GND"))
        .filter(|block| {
            let (x, y, _) = at_of(block);
            (x - x1).abs() < 2.6 && (y - y1).abs() < 2.6
        })
        .collect();
    assert_eq!(own.len(), 1, "one ground symbol belongs to J2 pin 1");

    let (triangle, name) = drawn(own[0]);
    let mut found = Vec::new();
    for (what, rect) in [("triangle", triangle), ("name", name)] {
        for (i, row) in rows.iter().enumerate() {
            if overlap(rect, *row) {
                found.push(format!("{what} {rect:?} on pin {}'s row {row:?}", i + 2));
            }
        }
        for (flag, flag_rect) in &flags {
            if overlap(rect, *flag_rect) {
                found.push(format!("{what} {rect:?} on the {flag} flag {flag_rect:?}"));
            }
        }
    }
    // Turned along its stub, the triangle points away from the pin, not back
    // over the stub.
    let near = if dx > 0.0 {
        triangle.0 - x1
    } else {
        x1 - triangle.2
    };
    if near < TRIANGLE_TIP_MM - TOUCH_MM {
        found.push(format!("triangle {triangle:?} lies over its own stub"));
    }
    assert!(
        found.is_empty(),
        "ground symbol in the way:\n{}",
        found.join("\n")
    );
}
