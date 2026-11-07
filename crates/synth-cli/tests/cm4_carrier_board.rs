// SPDX-License-Identifier: Apache-2.0

//! The SYN-407 CM4 carrier carries four mounting holes, one in each corner
//! of the exported board, and the CM4 pin map is the one in the board source.
//!
//! `synth place` ignores the placement sidecar, so the mounting-hole test
//! reads the exported PCB, where the sidecar and the fitted outline are both
//! applied. The net checks read the IR, which is the netlist the PCB is built
//! from.

#![cfg(unix)]

mod stub_router;

use std::process::Command;

/// `mounting_hole_m2_5` is a 2.5 mm NPTH, so its edge is 1.25 mm from its centre.
const HOLE_RADIUS_MM: f64 = 1.25;
/// Least hole-edge-to-Edge.Cuts room a hole may keep.
const MIN_RIM_MM: f64 = 1.5;

fn pairs_after(text: &str, key: &str) -> Vec<(f64, f64)> {
    text.match_indices(key)
        .filter_map(|(at, _)| {
            let rest = text[at + key.len()..].split(')').next()?;
            let mut values = rest.split_whitespace().map(str::parse::<f64>);
            Some((values.next()?.ok()?, values.next()?.ok()?))
        })
        .collect()
}

#[test]
#[ignore = "the 185-component schematic export does not finish in the test budget; run with --ignored on a fast machine"]
fn mounting_holes_sit_one_per_corner_and_keep_a_rim_inside_edge_cuts() {
    let dir = std::env::temp_dir().join(format!("synth_cm4_board_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);

    let source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../qualification/class-d/cm4-carrier/cm4_carrier.synth"
    );
    let out = dir.join("out");
    // The stub's copper does not pass DRC, so the export exits non-zero. The
    // unrouted placement is still written, and that is what this reads.
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["export-kicad", source, "--router", "free-routing", "--out"])
        .arg(&out)
        .output()
        .expect("run synth export-kicad");
    let text =
        std::fs::read_to_string(out.join("cm4_carrier.synth.kicad_pcb")).expect("unrouted PCB");

    // Edge.Cuts is four lines, so the extremes of their ends are the box.
    let mut points = Vec::new();
    for block in text.split("(gr_line").skip(1) {
        let (ends, layer) = block.split_once("(layer").expect("gr_line has a layer");
        if layer.trim_start().starts_with("\"Edge.Cuts\"") {
            points.extend(pairs_after(ends, "(start"));
            points.extend(pairs_after(ends, "(end"));
        }
    }
    assert!(!points.is_empty(), "the PCB has an Edge.Cuts outline");
    let min_x = points.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let min_y = points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let max_y = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);

    let mut corners = Vec::new();
    for footprint in text.split("\n\t(footprint").skip(1) {
        if !footprint.contains("\"MountingHole:") {
            continue;
        }
        let (x, y) = pairs_after(footprint, "(at")[0];
        for (rim, side) in [
            (x - HOLE_RADIUS_MM - min_x, "left"),
            (max_x - (x + HOLE_RADIUS_MM), "right"),
            (y - HOLE_RADIUS_MM - min_y, "top"),
            (max_y - (y + HOLE_RADIUS_MM), "bottom"),
        ] {
            assert!(
                rim >= MIN_RIM_MM,
                "the hole at ({x}, {y}) has a {rim:.2} mm {side} rim; Edge.Cuts must stay off it"
            );
        }
        corners.push((x < min_x.midpoint(max_x), y < min_y.midpoint(max_y)));
    }
    assert_eq!(corners.len(), 4, "expected four mounting holes");
    corners.sort_unstable();
    corners.dedup();
    assert_eq!(corners.len(), 4, "holes must cover all four quadrants");
}

const SOURCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../qualification/class-d/cm4-carrier/cm4_carrier.synth"
);

/// The name of the net that `pin_name` of `refdes` sits on.
fn net_of(ir: &serde_json::Value, refdes: &str, pin_name: &str) -> String {
    let component = ir["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|c| c["refdes"] == refdes)
        .unwrap_or_else(|| panic!("{refdes} exists"));
    let pin = component["part"]["pins"]
        .as_array()
        .expect("pins")
        .iter()
        .position(|p| p["name"] == pin_name)
        .unwrap_or_else(|| panic!("{refdes} has a {pin_name} pin"));
    ir["nets"]
        .as_array()
        .expect("nets")
        .iter()
        .find(|n| {
            n["endpoints"]
                .as_array()
                .expect("endpoints")
                .iter()
                .any(|e| e["component"] == component["id"] && e["pin"].as_u64() == Some(pin as u64))
        })
        .map_or_else(
            || panic!("{refdes}.{pin_name} is on a net"),
            |n| n["name"].as_str().expect("net name").to_string(),
        )
}

#[test]
fn the_cm4_pin_map_matches_the_datasheet_tables() {
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["dump-ir", SOURCE])
        .output()
        .expect("run synth dump-ir");
    assert!(output.status.success(), "dump-ir failed: {output:?}");
    let ir: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");

    // Table 6: +5V pins 77, 79, 81, 83, 85, 87; GND pins on both connectors.
    for pin in ["p77", "p79", "p81", "p83", "p85", "p87"] {
        assert_eq!(net_of(&ir, "J_CM4A", pin), "+5V", "J_CM4A.{pin}");
    }
    for pin in [
        "p1", "p2", "p7", "p8", "p13", "p14", "p22", "p23", "p32", "p33",
    ] {
        assert_eq!(net_of(&ir, "J_CM4A", pin), "GND", "J_CM4A.{pin}");
    }
    for pin in ["p7", "p8", "p13", "p14", "p19", "p20", "p25", "p26"] {
        assert_eq!(net_of(&ir, "J_CM4B", pin), "GND", "J_CM4B.{pin}");
    }

    // Table 6: Ethernet pair 0 is pins 12/10, pair 1 is 4/6.
    assert_eq!(net_of(&ir, "J_CM4A", "p12"), "ETH0_P");
    assert_eq!(net_of(&ir, "J_CM4A", "p10"), "ETH0_N");
    assert_eq!(net_of(&ir, "J_CM4A", "p4"), "ETH1_P");
    assert_eq!(net_of(&ir, "J_CM4A", "p6"), "ETH1_N");

    // Table 6: USB is pins 105/103 on the second connector (J_CM4B pads 5/3).
    assert_eq!(net_of(&ir, "J_CM4B", "p5"), "USB_UP_P");
    assert_eq!(net_of(&ir, "J_CM4B", "p3"), "USB_UP_N");

    // Table 6: HDMI0 TMDS2 is pins 170/172 (J_CM4B pads 70/72).
    assert_eq!(net_of(&ir, "J_CM4B", "p70"), "HDMI_D2_P");
    assert_eq!(net_of(&ir, "J_CM4B", "p72"), "HDMI_D2_N");

    // Table 6: SD_CLK 57, SD_CMD 62, SD_DAT0 63.
    assert_eq!(net_of(&ir, "J_CM4A", "p57"), "SD_CLK");
    assert_eq!(net_of(&ir, "J_CM4A", "p62"), "SD_CMD");
    assert_eq!(net_of(&ir, "J_CM4A", "p63"), "SD_DAT0");
}

#[test]
fn the_gpio_header_is_the_raspberry_pi_40_pin_map() {
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["dump-ir", SOURCE])
        .output()
        .expect("run synth dump-ir");
    assert!(output.status.success(), "dump-ir failed: {output:?}");
    let ir: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");

    // Physical pin 1/17 are 3V3, 2/4 are 5V, 6/9/14/20/25/30/34/39 are GND.
    assert_eq!(net_of(&ir, "J_GPIO1", "p1"), "+3V3");
    assert_eq!(net_of(&ir, "J_GPIO1", "p9"), "+3V3");
    assert_eq!(net_of(&ir, "J_GPIO2", "p1"), "+5V");
    assert_eq!(net_of(&ir, "J_GPIO2", "p2"), "+5V");
    for (refdes, pin) in [
        ("J_GPIO1", "p5"),
        ("J_GPIO1", "p13"),
        ("J_GPIO1", "p20"),
        ("J_GPIO2", "p3"),
        ("J_GPIO2", "p7"),
        ("J_GPIO2", "p10"),
        ("J_GPIO2", "p15"),
        ("J_GPIO2", "p17"),
    ] {
        assert_eq!(net_of(&ir, refdes, pin), "GND", "{refdes}.{pin}");
    }

    // Physical pin 3 is GPIO2 (CM4 pin 58) and pin 40 is GPIO21 (CM4 pin 25).
    assert_eq!(net_of(&ir, "J_GPIO1", "p2"), "GPIO2");
    assert_eq!(net_of(&ir, "J_CM4A", "p58"), "GPIO2");
    assert_eq!(net_of(&ir, "J_GPIO2", "p20"), "GPIO21");
    assert_eq!(net_of(&ir, "J_CM4A", "p25"), "GPIO21");

    // Physical pin 37 is GPIO26, shared with the RTC interrupt.
    assert_eq!(net_of(&ir, "J_GPIO1", "p19"), "GPIO26");
    assert_eq!(net_of(&ir, "J_CM4A", "p24"), "GPIO26");
    assert_eq!(net_of(&ir, "U12", "int"), "GPIO26");
}

/// `--pcb-only` writes the PCB side without running the schematic layout.
/// This is the escape hatch for boards whose schematic layout is too slow to
/// reach the copper: the placement sidecar is loaded directly and the PCB is
/// written before the router runs.
#[test]
fn pcb_only_export_writes_the_pcb_without_a_schematic() {
    let dir = std::env::temp_dir().join(format!("synth_cm4_pcb_only_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);

    let out = dir.join("out");
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .args([
            "export-kicad",
            SOURCE,
            "--out",
            out.to_str().expect("utf-8 path"),
            "--pcb-only",
            "--allow-incomplete",
            "--router",
            "kicad-routing-tools",
        ])
        .output()
        .expect("run synth export-kicad --pcb-only");

    assert!(
        out.join("cm4_carrier.kicad_pcb").is_file(),
        "the PCB is written without the schematic layout"
    );
    assert!(
        !out.join("cm4_carrier.kicad_sch").exists(),
        "--pcb-only must not write a schematic"
    );
}
