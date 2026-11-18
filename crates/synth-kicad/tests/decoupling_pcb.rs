// SPDX-License-Identifier: Apache-2.0

//! A decoupling capacitor must stay close to the IC it decouples.
//!
//! `score_placement` measures centre to centre, which grows with the IC body
//! and so cannot express "under 3.0 mm from the power pin". This test reads
//! the exported board instead and measures the distance between the cap pad
//! and the IC pad on the net they share, which is the rule the comment in
//! `synth-place/src/score.rs` states.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// One exported footprint: reference designator and its pads as
/// `(net number, x mm, y mm)` in absolute board coordinates.
struct Footprint {
    refdes: String,
    pads: Vec<(u32, f64, f64)>,
}

fn load_board(path: &str) -> synth_ir::Board {
    let source = fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parsed = synth_parser::parse(&source, file.clone());
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, &file);
    lowered.board.expect("board")
}

/// The `( ... )` block starting at `start`, by paren matching.
fn block_at(text: &str, start: usize) -> &str {
    let mut depth = 0i64;
    let mut end = start;
    for (offset, byte) in text[start..].bytes().enumerate() {
        depth += i64::from(byte == b'(') - i64::from(byte == b')');
        if depth == 0 {
            end = start + offset + 1;
            break;
        }
    }
    &text[start..end]
}

fn number_after(block: &str, key: &str, index: usize) -> Option<f64> {
    let at = block.find(key)?;
    block[at + key.len()..]
        .split_whitespace()
        .nth(index)?
        .trim_end_matches(')')
        .parse()
        .ok()
}

fn footprints(pcb: &str) -> Vec<Footprint> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(offset) = pcb[from..].find("(footprint") {
        let start = from + offset;
        let block = block_at(pcb, start);
        from = start + block.len();
        let Some(refdes) = block
            .find("\"Reference\"")
            .and_then(|i| block[i + "\"Reference\"".len()..].split('"').nth(1))
        else {
            continue;
        };
        let (fx, fy) = (
            number_after(block, "(at ", 0).unwrap_or(0.0),
            number_after(block, "(at ", 1).unwrap_or(0.0),
        );
        let degrees = number_after(block, "(at ", 2).unwrap_or(0.0).to_radians();
        let mut pads = Vec::new();
        let mut pad_from = 0;
        while let Some(pad_offset) = block[pad_from..].find("(pad \"") {
            let pad_start = pad_from + pad_offset;
            let pad_block = block_at(block, pad_start);
            pad_from = pad_start + pad_block.len();
            let Some(net) = pad_block
                .find("(net ")
                .and_then(|i| pad_block[i + "(net ".len()..].split_whitespace().next())
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if net == 0 {
                continue;
            }
            let (px, py) = (
                number_after(pad_block, "(at ", 0).unwrap_or(0.0),
                number_after(pad_block, "(at ", 1).unwrap_or(0.0),
            );
            pads.push((
                net,
                fx + px * degrees.cos() - py * degrees.sin(),
                fy + px * degrees.sin() + py * degrees.cos(),
            ));
        }
        out.push(Footprint {
            refdes: refdes.to_string(),
            pads,
        });
    }
    out
}

/// Closest pad-to-pad distance between `ic` and `cap` on a net they share.
fn closest_shared_pad_mm(pcb: &str, ic: &str, cap: &str) -> Option<f64> {
    let parts = footprints(pcb);
    let ic_pads = &parts.iter().find(|p| p.refdes == ic)?.pads;
    let cap_pads = &parts.iter().find(|p| p.refdes == cap)?.pads;
    let mut best: Option<f64> = None;
    for (net_a, ax, ay) in ic_pads {
        for (net_b, bx, by) in cap_pads {
            if net_a != net_b {
                continue;
            }
            let distance = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            if best.is_none_or(|current| distance < current) {
                best = Some(distance);
            }
        }
    }
    best
}

#[test]
fn a_decoupling_cap_stays_within_three_millimetres_of_its_ic_power_pad() {
    let board = load_board("../../fixtures/layout/i2c_bus.synth");
    let dir = std::env::temp_dir().join(format!("synth-decap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let result = synth_kicad::export_with_sidecars(&board, &dir, &synth_kicad::Sidecars::default())
        .expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let _ = fs::remove_dir_all(&dir);

    let distance =
        closest_shared_pad_mm(&pcb, "U1", "C1").expect("C1 must share the decoupling net with U1");
    assert!(
        distance <= 3.0,
        "C1 sits {distance:.2} mm from U1's power pad, expected at most 3.0 mm"
    );
}
