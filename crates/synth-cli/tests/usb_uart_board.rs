// SPDX-License-Identifier: Apache-2.0

//! The SYN-66 USB-UART board carries four mounting holes, one in each corner
//! of the exported board, and each keeps a rim inside Edge.Cuts.
//!
//! `synth place` ignores the placement sidecar, so this reads the exported
//! PCB, where the sidecar and the fitted outline are both applied.

#![cfg(unix)]

mod stub_router;

use std::process::Command;

/// `mounting_hole_m2_5` is a 2.5 mm NPTH, so its edge is 1.25 mm from its centre.
const HOLE_RADIUS_MM: f64 = 1.25;
/// Least hole-edge-to-Edge.Cuts room a hole may keep. The tight fit leaves 2.1 mm.
const MIN_RIM_MM: f64 = 2.1;

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
fn mounting_holes_sit_one_per_corner_and_keep_a_rim_inside_edge_cuts() {
    let dir = std::env::temp_dir().join(format!("synth_usb_uart_board_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let _lock = stub_router::serialised();
    let _router = stub_router::install(&dir);

    let source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../qualification/class-b/usb-uart/usb_uart.synth"
    );
    let out = dir.join("out");
    // The stub's copper does not pass DRC, so the export exits non-zero. The
    // unrouted placement is still written, and that is what this reads.
    Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["export-kicad", source, "--router", "free-routing", "--out"])
        .arg(&out)
        .output()
        .expect("run synth export-kicad");
    let text = std::fs::read_to_string(out.join("usb_uart.synth.kicad_pcb")).expect("unrouted PCB");

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
    "/../../qualification/class-b/usb-uart/usb_uart.synth"
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

/// A TX LED that sources from the line is lit while the line idles high and
/// goes dark during traffic. Sinking it into the line, anode to VIO through the
/// resistor, lights it on the low bits instead.
#[test]
fn the_tx_leds_sink_into_the_txd_lines() {
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["dump-ir", SOURCE])
        .output()
        .expect("run synth dump-ir");
    assert!(output.status.success(), "dump-ir failed: {output:?}");
    let ir: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");

    for (led, resistor, line) in [("D4", "R4", "TXD0"), ("D7", "R7", "TXD1")] {
        assert_eq!(net_of(&ir, led, "cathode"), line, "{led} sinks into {line}");
        assert_eq!(
            net_of(&ir, resistor, "p1"),
            "VIO",
            "{resistor} feeds {led} from VIO"
        );
    }
}
