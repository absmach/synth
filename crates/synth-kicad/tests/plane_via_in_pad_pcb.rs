// SPDX-License-Identifier: Apache-2.0

//! A power plane stitched to the supply balls of a 0.8 mm-pitch BGA: the
//! via-in-pad fallback has to be judged against the actual pad. The pads of the
//! iCE40's 121-ball package are 0.35 mm, narrower than any via the process can
//! drill (0.3 mm drill plus two annular rings), so a board that asks for its
//! interior supply balls to be stitched through their own pads is refused with
//! the sizes named; and the vias the exporter does write for the balls it can
//! reach pass KiCad's design-rule check.
//!
//! These tests need the KiCad footprint library for the real pads, and
//! `kicad-cli` for the design-rule check. Both are required inputs: a test that
//! cannot find one fails and names it, so a missing library cannot turn the
//! regression into a pass. A machine without KiCad sets
//! `SYNTH_ALLOW_MISSING_KICAD=1` to skip them on purpose.

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

/// Where Synth looks for the stock footprint libraries on Linux when
/// `KICAD_FOOTPRINT_DIR` is not set, which is where the distribution package
/// puts them and where CI installs them.
const DEFAULT_FOOTPRINT_DIR: &str = "/usr/share/kicad/footprints";

/// The directories a footprint library can be in: the variable, then the
/// default install directory, the same order Synth's own loader uses.
fn footprint_dirs(variable: Option<&str>) -> Vec<PathBuf> {
    variable
        .map(PathBuf::from)
        .into_iter()
        .chain(std::iter::once(PathBuf::from(DEFAULT_FOOTPRINT_DIR)))
        .collect()
}

/// Whether a test that needs the KiCad footprint library (and, for the DRC,
/// `kicad-cli`) can run. A missing input is a failure unless the run says, with
/// `SYNTH_ALLOW_MISSING_KICAD=1`, that it should be skipped.
fn kicad_ready(footprint: &str, needs_cli: bool) -> bool {
    let missing = if !footprint_dirs(std::env::var("KICAD_FOOTPRINT_DIR").ok().as_deref())
        .iter()
        .any(|dir| dir.join(footprint).exists())
    {
        Some(format!(
            "KICAD_FOOTPRINT_DIR, or {DEFAULT_FOOTPRINT_DIR}, with {footprint}"
        ))
    } else if needs_cli && Command::new("kicad-cli").arg("version").output().is_err() {
        Some("kicad-cli on PATH".to_string())
    } else {
        None
    };
    match missing {
        None => true,
        Some(what) if std::env::var("SYNTH_ALLOW_MISSING_KICAD").as_deref() == Ok("1") => {
            eprintln!("skipped on purpose (SYNTH_ALLOW_MISSING_KICAD=1): needs {what}");
            false
        }
        Some(what) => {
            panic!("this test needs {what}; set it up, or set SYNTH_ALLOW_MISSING_KICAD=1 to skip")
        }
    }
}

const BGA_FOOTPRINT: &str =
    "Package_BGA.pretty/BGA-121_9.0x9.0mm_Layout11x11_P0.8mm_Ball0.4mm_Pad0.35mm_NSMD.kicad_mod";
const SOIC_EP_FOOTPRINT: &str =
    "Package_SO.pretty/SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm.kicad_mod";

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
    // Only a component that declares the under-pad escape may take the filled and
    // capped via, the one sized to the process's land and exempt from the plain ring.
    let escape = if with_via_in_pad {
        " { escape { method underpad } }"
    } else {
        ""
    };
    format!(
        "board \"bga_plane\" {{\n  layers 4\n{tier}  netclass \"RAIL\" {{\n    plane\n  }}\n  \
         component U1: fpga \"ice40hx4k_bg121\"{escape}\n{connections}}}\n"
    )
}

#[test]
fn interior_supply_balls_take_the_via_in_pad_land_on_the_filled_via_tier() {
    if !kicad_ready(BGA_FOOTPRINT, false) {
        return;
    }
    let board = lower(&source(true, true));
    let out = std::env::temp_dir().join(format!("synth-bga-plane-interior-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    // The tier's drill is 0.2 mm, so the via-in-pad land is 0.35 mm: the 0.35 mm
    // ball pad holds it, and the interior supply balls are not refused.
    let result = synth_kicad::export(&board, &out)
        .expect("a 0.35 mm ball pad holds the 0.35 mm via-in-pad land of the 0.2 mm drill tier");
    let pcb = fs::read_to_string(&result.pcb_path).expect("pcb");
    let _ = fs::remove_dir_all(&out);
    assert!(
        pcb.contains("(size 0.35)"),
        "the plane vias are written at the via-in-pad land, not the 0.6 mm via"
    );
}

/// A ball grid that does not declare the under-pad escape is not given the small
/// via: the DRC would judge it against the plain ring, so the exporter refuses the
/// supply ball its plane via in a 0.35 mm pad instead of writing one that fails.
/// A microvia reaches one dielectric, so a rail poured on In2 must get a via
/// spanning F.Cu-In2.Cu, not the hard-coded F.Cu-In1.Cu that leaves it open.
#[test]
fn each_rail_microvia_spans_to_the_layer_that_rail_is_poured_on() {
    if !kicad_ready(BGA_FOOTPRINT, false) {
        return;
    }
    let source = source(true, true).replace("jlc-filled-via", "jlc-advanced");
    let board = lower(&source);
    let out = std::env::temp_dir().join(format!("synth-bga-plane-spans-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let result = synth_kicad::export(&board, &out).expect("the under-pad balls are stitched");
    let pcb = fs::read_to_string(&result.pcb_path).expect("pcb");
    let _ = fs::remove_dir_all(&out);
    assert!(
        pcb.contains("(layers \"F.Cu\" \"In1.Cu\")")
            || pcb.contains("(layers \"In1.Cu\" \"F.Cu\")"),
        "the In1 rail needs a microvia that stops on In1"
    );
    assert!(
        pcb.contains("(layers \"F.Cu\" \"In2.Cu\")")
            || pcb.contains("(layers \"In2.Cu\" \"F.Cu\")"),
        "the In2 rail needs a via that reaches In2, not one that stops on In1"
    );
}

#[test]
fn a_ball_pad_takes_no_small_via_without_the_under_pad_escape() {
    if !kicad_ready(BGA_FOOTPRINT, false) {
        return;
    }
    let undeclared = source(true, true).replace(" { escape { method underpad } }", "");
    assert!(!undeclared.contains("underpad"), "{undeclared}");
    let board = lower(&undeclared);
    let out =
        std::env::temp_dir().join(format!("synth-bga-plane-undeclared-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let refusal = synth_kicad::export(&board, &out);
    let _ = fs::remove_dir_all(&out);
    let text = match refusal {
        Err(error) => format!("{error:?}"),
        Ok(_) => {
            panic!("a ball pad too small for the plane via must be refused without the escape")
        }
    };
    // The refusal names the plane via's pad (the 0.3 mm drill plus two annular
    // rings) and the one change that works.
    assert!(text.contains("0.60 mm pad"), "{text}");
    assert!(text.contains("two annular rings"), "{text}");
    assert!(text.contains("escape { method underpad }"), "{text}");
    assert!(text.contains("0.35 mm filled and capped via"), "{text}");
}

#[test]
fn the_vias_written_for_the_corner_balls_pass_the_kicad_drc() {
    if !kicad_ready(BGA_FOOTPRINT, true) {
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

/// A regulator whose exposed pad is on a declared plane, on a process that fills
/// vias in pads: the pad is large enough for the via, so the stitching via goes
/// in the pad, not beside it, and the exported board passes the design-rule
/// check. This is the successful path of the real-footprint case the BGA tests
/// refuse.
#[test]
fn an_exposed_pad_that_holds_the_via_is_stitched_in_place_and_passes_drc() {
    if !kicad_ready(SOIC_EP_FOOTPRINT, true) {
        return;
    }
    let source = "board \"ep_plane\" {\n  layers 4\n  manufacturer \"jlc-filled-via\"\n  \
                  requires via_in_pad\n  netclass \"RAIL\" {\n    plane\n  }\n  \
                  component U1: regulator \"mp2307_buck_ep\"\n  \
                  connect U1.gnd -> U1.gnd_ep as \"PGND\" class \"RAIL\"\n}\n";
    let board = lower(source);
    let out = std::env::temp_dir().join(format!("synth-ep-plane-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let result = synth_kicad::export(&board, &out).expect("an exposed pad takes its via");
    // The exporter breaks lines inside a property; one run of text is easier to read.
    let pcb = fs::read_to_string(&result.pcb_path)
        .expect("pcb")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // The exposed pad's box in board coordinates, and the vias of its net.
    let (pad_centre, half) = exposed_pad_box(&pcb, "U1", "9");
    let in_pad = vias_in(&pcb)
        .into_iter()
        .filter(|(x, y)| (x - pad_centre.0).abs() <= half.0 && (y - pad_centre.1).abs() <= half.1)
        .count();
    assert!(
        in_pad >= 1,
        "the stitching via must sit inside the exposed pad {pad_centre:?} +/- {half:?}"
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
    let bad: Vec<String> = json["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["severity"] == "error")
        .map(ToString::to_string)
        .collect();
    let _ = fs::remove_dir_all(&out);
    assert!(bad.is_empty(), "the in-pad via violates the rules: {bad:?}");
}

/// The centre and half extents, in board coordinates, of pad `number` of the
/// footprint with reference `refdes`.
fn exposed_pad_box(pcb: &str, refdes: &str, number: &str) -> ((f64, f64), (f64, f64)) {
    let property = format!("(property \"Reference\" \"{refdes}\"");
    let at_ref = pcb.find(&property).expect("footprint reference");
    let start = pcb[..at_ref].rfind("(footprint").expect("footprint start");
    let block = &pcb[start..];
    let numbers = |text: &str, key: &str| -> Vec<f64> {
        let at = text.find(key).unwrap_or_else(|| panic!("no {key}"));
        let body = &text[at + key.len()..];
        body[..body.find(')').expect("close")]
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect()
    };
    let footprint_at = numbers(block, "(at ");
    let pad_text = &block[block.find(&format!("(pad \"{number}\"")).expect("pad")..];
    let pad_at = numbers(pad_text, "(at ");
    let size = numbers(pad_text, "(size ");
    let angle = footprint_at.get(2).copied().unwrap_or(0.0).to_radians();
    let (sin, cos) = angle.sin_cos();
    // A footprint at angle t puts a local point (x, y) at (x cos t + y sin t,
    // -x sin t + y cos t) from its origin, y pointing down.
    let centre = (
        footprint_at[0] + pad_at[0] * cos + pad_at[1] * sin,
        footprint_at[1] - pad_at[0] * sin + pad_at[1] * cos,
    );
    let half = (
        (size[0] * cos.abs()).midpoint(size[1] * sin.abs()),
        (size[0] * sin.abs()).midpoint(size[1] * cos.abs()),
    );
    (centre, half)
}

/// The centre of every via in the board text.
fn vias_in(pcb: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut rest = pcb;
    while let Some(i) = rest.find("(via") {
        let block = &rest[i..];
        if let Some(a) = block.find("(at ") {
            let body = &block[a + 4..];
            let v: Vec<f64> = body[..body.find(')').unwrap_or(0)]
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            if let [x, y] = v[..] {
                out.push((x, y));
            }
        }
        rest = &block[4..];
    }
    out
}

/// On the microvia tier the in-pad via is a micro via: it spans one
/// dielectric (F.Cu to In1.Cu), not the whole stackup a through via drills.
/// A filled and capped 0.25 mm via written F.Cu–B.Cu would be drilled through
/// four layers for one layer hop.
#[test]
fn a_micro_via_in_a_ball_pad_spans_one_dielectric() {
    if !kicad_ready(BGA_FOOTPRINT, false) {
        return;
    }
    let board = lower(
        r#"board "bga_plane" {
  layers 4
  manufacturer "jlc-advanced"
  requires via_in_pad
  netclass "RAIL" { plane }
  component U1: fpga "ice40hx4k_bg121" { escape { method underpad } }
  connect U1.vccio_0 -> U1.vccio_1 as "VCCIO" class "RAIL"
  connect U1.vccio_1 -> U1.vccio_2
  connect U1.vccio_2 -> U1.vccio_3
  connect U1.vcc -> U1.vcc_2 as "VCC_CORE" class "RAIL"
}"#,
    );
    let out = std::env::temp_dir().join(format!("synth-bga-plane-micro-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let result = synth_kicad::export(&board, &out)
        .expect("the 0.25 mm microvia land fits the 0.35 mm ball pad");
    let pcb = fs::read_to_string(&result.pcb_path).expect("pcb");
    let _ = fs::remove_dir_all(&out);
    let micro = pcb
        .split("(via")
        .skip(1)
        .find(|block| block.contains("(size 0.25)"))
        .expect("the in-pad vias are written at the microvia land");
    assert!(
        micro.contains("(layers \"F.Cu\" \"In1.Cu\")")
            || micro.contains("(layers \"F.Cu\" \"In2.Cu\")"),
        "the microvia spans one dielectric, to the rail's own layer: ...{micro}"
    );
    assert!(
        !micro.contains("(layers \"F.Cu\" \"B.Cu\")"),
        "the microvia is not a through via: ...{micro}"
    );
}

#[test]
fn the_default_install_directory_is_searched_when_the_variable_is_unset() {
    assert_eq!(
        footprint_dirs(None),
        vec![PathBuf::from(DEFAULT_FOOTPRINT_DIR)]
    );
    assert_eq!(
        footprint_dirs(Some("/opt/kicad/footprints")),
        vec![
            PathBuf::from("/opt/kicad/footprints"),
            PathBuf::from(DEFAULT_FOOTPRINT_DIR)
        ],
        "the variable wins, and the default install directory is still tried"
    );
}
