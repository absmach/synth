// SPDX-License-Identifier: Apache-2.0

//! A power plane stitched to the supply balls of a 0.8 mm-pitch BGA: the
//! via-in-pad fallback has to be judged against the actual pad. The pads of the
//! iCE40's 121-ball package are 0.35 mm, narrower than any via the process can
//! drill (0.3 mm drill plus two annular rings), so a board that asks for its
//! interior supply balls to be stitched through their own pads is refused with
//! the sizes named; and the vias the exporter does write for the balls it can
//! reach pass KiCad's design-rule check.
//!
//! These tests need the KiCad footprint library for the real BGA pads (CI has
//! it); without it they say so and pass, since the synthesized fallback pads of
//! a part are not a BGA.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

/// Whether the KiCad BGA footprint the iCE40 part names can be read.
fn bga_footprint_available() -> bool {
    std::env::var("KICAD_FOOTPRINT_DIR").is_ok_and(|dir| {
        Path::new(&dir)
            .join("Package_BGA.pretty")
            .join("BGA-121_9.0x9.0mm_Layout11x11_P0.8mm_Ball0.4mm_Pad0.35mm_NSMD.kicad_mod")
            .exists()
    })
}

fn lower(source: &str) -> synth_ir::Board {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(source, "bga_plane.synth");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "bga_plane.synth");
    lowered.board.expect("board")
}

/// The iCE40 with the given balls on a declared plane. `interior` ties its
/// core and I/O supply balls, all inside the array; `corners` ties the four
/// corner balls, which have the whole board outside them.
fn source(with_via_in_pad: bool, interior: bool) -> String {
    let tier = if with_via_in_pad {
        "  manufacturer \"jlc-filled-via\"\n  requires via_in_pad\n"
    } else {
        "  manufacturer \"jlc-standard\"\n"
    };
    let connections = if interior {
        "  connect U1.vccio_0 -> U1.vccio_1 as \"VCCIO\" class \"RAIL\"\n  \
           connect U1.vccio_1 -> U1.vccio_2\n  connect U1.vccio_2 -> U1.vccio_3\n  \
           connect U1.vcc -> U1.vcc_2 as \"VCC_CORE\" class \"RAIL\"\n"
    } else {
        // Balls A1, A11, L1 and L11.
        "  connect U1.iot_225 -> U1.iot_168 as \"CORNERS\" class \"RAIL\"\n  \
           connect U1.iot_168 -> U1.iob_56\n  connect U1.iob_56 -> U1.vcc_spi\n"
    };
    format!(
        "board \"bga_plane\" {{\n  layers 4\n{tier}  netclass \"RAIL\" {{\n    plane\n  }}\n  \
         component U1: fpga \"ice40hx4k_bg121\"\n{connections}}}\n"
    )
}

#[test]
fn a_ball_pad_narrower_than_the_via_is_refused_with_the_sizes_named() {
    if !bga_footprint_available() {
        eprintln!("skipped: no KICAD_FOOTPRINT_DIR with the BGA footprint");
        return;
    }
    let board = lower(&source(true, true));
    let out = std::env::temp_dir().join(format!("synth-bga-plane-refused-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let error = synth_kicad::export(&board, &out).expect_err(
        "interior supply balls are boxed in, and 0.35 mm pads cannot hold a 0.6 mm via",
    );
    let text = error.to_string();
    let _ = fs::remove_dir_all(&out);
    assert!(
        text.contains("0.35 x 0.35 mm") && text.contains("0.60 mm pad"),
        "the error names the pad and the via it cannot hold: {text}"
    );
}

#[test]
fn the_vias_written_for_the_corner_balls_pass_the_kicad_drc() {
    if !bga_footprint_available() {
        eprintln!("skipped: no KICAD_FOOTPRINT_DIR with the BGA footprint");
        return;
    }
    if Command::new("kicad-cli").arg("version").output().is_err() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    // The corner balls have room outside the array, so each gets a via beside
    // its pad, with a stub: no via-in-pad is needed.
    let board = lower(&source(false, false));
    let out = std::env::temp_dir().join(format!("synth-bga-plane-drc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let result = synth_kicad::export(&board, &out).expect("corner balls can be stitched");
    let pcb = fs::read_to_string(&result.pcb_path).expect("pcb");
    let via_count = pcb.matches("(via").count();
    assert!(
        via_count >= 4,
        "each corner ball is stitched: {via_count} via(s)"
    );
    let report = out.join("drc.json");
    let status = Command::new("kicad-cli")
        .args([
            "pcb",
            "drc",
            "--severity-all",
            "--format",
            "json",
            "--output",
        ])
        .arg(&report)
        .arg(&result.pcb_path)
        .status()
        .expect("run kicad-cli");
    assert!(status.success() || report.exists(), "kicad-cli did not run");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).expect("drc report")).expect("json");
    let via_geometry = [
        "clearance",
        "hole_clearance",
        "annular_width",
        "via_diameter",
        "drill_out_of_range",
        "hole_to_hole",
        "shorting_items",
    ];
    let bad: Vec<String> = json["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| via_geometry.contains(&v["type"].as_str().unwrap_or("")))
        .filter(|v| {
            v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|i| i["description"].as_str().unwrap_or("").starts_with("Via"))
        })
        .map(ToString::to_string)
        .collect();
    let _ = fs::remove_dir_all(&out);
    assert!(bad.is_empty(), "stitching vias violate the rules: {bad:?}");
}
