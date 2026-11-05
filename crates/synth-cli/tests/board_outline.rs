// SPDX-License-Identifier: Apache-2.0

//! An `outline` statement fixes Edge.Cuts. A mounting hole may sit on that
//! edge, and the ground fence must not leave a via beside it.

#![cfg(unix)]

mod stub_router;

use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = r#"board "outline_fixture" {
    layers 4
    outline 60mm x 30mm
    component H1: mounting_hole "mounting_hole_m2_5"
    component R1: resistor "r_generic_0603" { value "10k" }
    component R2: resistor "r_generic_0603" { value "10k" }
    component R3: resistor "r_generic_0603" { value "10k" }
    component J1: connector "header_1x4"
    connect R1.p1 -> R2.p1 as "GND"
    connect R2.p1 -> R3.p1 as "GND"
    connect R3.p1 -> J1.p4 as "GND"
}"#;

const SIDECAR: &str = r#"schema_version = 2
forced_net_labels = []
fit_sheet = false

[components.H1]
x = 2.75
y = 2.75
rotation = 0
source = "agent"
priority = "hard"
timestamp = "2026-11-04T00:00:00+00:00"

[components.R1]
x = 20
y = 10
rotation = 0
source = "agent"
priority = "hard"
timestamp = "2026-11-04T00:00:00+00:00"

[components.R2]
x = 25
y = 10
rotation = 0
source = "agent"
priority = "hard"
timestamp = "2026-11-04T00:00:00+00:00"

[components.R3]
x = 30
y = 10
rotation = 0
source = "agent"
priority = "hard"
timestamp = "2026-11-04T00:00:00+00:00"

[components.J1]
x = 50
y = 15
rotation = 0
source = "agent"
priority = "hard"
timestamp = "2026-11-04T00:00:00+00:00"
"#;

fn pairs_after(text: &str, key: &str) -> Vec<(f64, f64)> {
    text.match_indices(key)
        .filter_map(|(at, _)| {
            let rest = text[at + key.len()..].split(')').next()?;
            let mut values = rest.split_whitespace().map(str::parse::<f64>);
            Some((values.next()?.ok()?, values.next()?.ok()?))
        })
        .collect()
}

fn edge_cuts(text: &str) -> (f64, f64, f64, f64) {
    let mut points = Vec::new();
    for block in text.split("(gr_line").skip(1) {
        let (ends, layer) = block.split_once("(layer").expect("gr_line has a layer");
        if layer.trim_start().starts_with("\"Edge.Cuts\"") {
            points.extend(pairs_after(ends, "(start"));
            points.extend(pairs_after(ends, "(end"));
        }
    }
    assert!(!points.is_empty(), "the PCB has an Edge.Cuts outline");
    (
        points.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        points.iter().map(|p| p.0).fold(f64::MIN, f64::max),
        points.iter().map(|p| p.1).fold(f64::MAX, f64::min),
        points.iter().map(|p| p.1).fold(f64::MIN, f64::max),
    )
}

fn export(out: &Path) -> String {
    let source = out.join("outline_fixture.synth");
    std::fs::write(&source, SOURCE).expect("write source");
    std::fs::write(out.join("outline_fixture.placement.layout.toml"), SIDECAR)
        .expect("write sidecar");
    // The stub's copper does not pass DRC, so the export exits non-zero. The
    // unrouted placement is still written, and that is what this reads.
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["export-kicad"])
        .arg(&source)
        .args(["--router", "free-routing", "--out"])
        .arg(out.join("kicad"))
        .output()
        .expect("run synth export-kicad");
    std::fs::read_to_string(out.join("kicad/outline_fixture.synth.kicad_pcb"))
        .expect("unrouted PCB")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth_outline_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn the_declared_outline_is_the_exported_edge() {
    let dir = scratch("edge");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);
    let text = export(&dir);

    let (min_x, max_x, min_y, max_y) = edge_cuts(&text);
    for (actual, expected, side) in [
        (min_x, 0.0, "left"),
        (min_y, 0.0, "bottom"),
        (max_x, 60.0, "right"),
        (max_y, 30.0, "top"),
    ] {
        assert!(
            (actual - expected).abs() < 0.001,
            "the {side} edge is {actual} mm, not {expected} mm\n{text}"
        );
    }
}

#[test]
fn a_part_outside_the_declared_outline_is_reported() {
    let dir = scratch("outside");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);
    let source = dir.join("outline_fixture.synth");
    std::fs::write(&source, SOURCE).expect("write source");
    std::fs::write(
        dir.join("outline_fixture.placement.layout.toml"),
        SIDECAR.replace("x = 20", "x = 65"),
    )
    .expect("write sidecar");
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["export-kicad"])
        .arg(&source)
        .args(["--router", "free-routing", "--out"])
        .arg(dir.join("kicad"))
        .output()
        .expect("run synth export-kicad");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("E-SYNTH-PLACE-003"),
        "the out-of-outline part is reported:\n{text}"
    );
    assert!(
        text.contains("R1"),
        "the diagnostic names the part:\n{text}"
    );
}

#[test]
fn a_mounting_hole_keeps_its_edge_coordinate_and_has_no_via_beside_it() {
    let dir = scratch("hole");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);
    let text = export(&dir);

    let footprint = text
        .split("\n\t(footprint")
        .skip(1)
        .find(|block| block.contains("\"MountingHole:"))
        .expect("the exported PCB has the mounting hole");
    let (x, y) = pairs_after(footprint, "(at")[0];
    assert!(
        (x - 2.75).abs() < 0.001 && (y - 2.75).abs() < 0.001,
        "the mounting hole is at ({x}, {y}), not its sidecar coordinate (2.75, 2.75)"
    );

    // 2.5 mm hole, 1.5 mm screw-head keepout, 0.3 mm via pad.
    let reach = 1.25 + 1.5 + 0.3;
    let vias = pairs_after(&text, "(via\n\t\t(at ");
    assert!(
        !vias.is_empty(),
        "a four-layer board stitches its ground planes"
    );
    for (vx, vy) in vias {
        let gap = (vx - x).hypot(vy - y);
        assert!(
            gap >= reach,
            "a via at ({vx}, {vy}) is {gap:.2} mm from the mounting hole; it reads as a second hole"
        );
    }
}
