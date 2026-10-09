// SPDX-License-Identifier: Apache-2.0

//! A synthesized IC symbol (a part with no `kicad_symbol`) must be wired so
//! KiCad itself sees every pin connected. Wires that stop short of a pin tip
//! leave the netlist full of `unconnected-(U1-…)` nets even though the design
//! and the PCB netlist are fine, so the check goes through `kicad-cli`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const HEADER: &str = include_str!("../../../registry/parts/connectors/header_1x4.synth.toml");

const FOOTPRINT: &str = "Package_DFN_QFN:QFN-24-1EP_4x4mm_P0.5mm_EP2.6x2.6mm";

fn pin_names(top: usize, left: usize, right: usize) -> Vec<(String, &'static str)> {
    (0..top)
        .map(|i| (format!("t{i}"), "power_input"))
        .chain((0..left).map(|i| (format!("l{i}"), "input")))
        .chain((0..right).map(|i| (format!("r{i}"), "output")))
        .collect()
}

fn synthesized_ic(pins: &[(String, &str)]) -> String {
    let mut toml = format!(
        "id = \"probe_ic\"\nkind = \"ic\"\nversion = 0\nlifecycle = \"active\"\n\
         description = \"synthesized-symbol test IC\"\nkicad_footprint = \"{FOOTPRINT}\"\n\n\
         [footprint_dimensions]\nwidth_mm = 4.0\nheight_mm = 4.0\ncourtyard_margin_mm = 0.25\n\n\
         [operating_conditions]\nmin_voltage_v = 1.7\nmax_voltage_v = 5.5\nmax_current_ma = 15.0\n"
    );
    for (n, (name, ty)) in pins.iter().enumerate() {
        write!(
            toml,
            "\n[[pins]]\nname = \"{name}\"\nnumber = \"{}\"\nelectrical_type = \"{ty}\"\n",
            n + 1
        )
        .unwrap();
    }
    toml
}

fn design(pins: &[(String, &str)]) -> String {
    let mut src = String::from(
        "board \"netlist_probe\" {\n  layers 2\n  component U1: ic \"probe_ic\"\n  \
         component J1: connector \"header_1x4\"\n  component J2: connector \"header_1x4\"\n",
    );
    for (i, (pin, _)) in pins.iter().enumerate() {
        let header = if i < 4 { "J1" } else { "J2" };
        writeln!(src, "  connect {header}.p{} -> U1.{pin}", i % 4 + 1).unwrap();
    }
    src.push_str("}\n");
    src
}

fn export_netlist(top: usize, left: usize, right: usize, dir: &Path) -> Option<String> {
    let pins = pin_names(top, left, right);
    let ic = synthesized_ic(&pins);
    let registry = synth_registry::load_from_strs(&[
        ("probe_ic.synth.toml", &ic),
        ("header_1x4.synth.toml", HEADER),
    ])
    .expect("probe registry must load");
    let src = design(&pins);
    let parsed = synth_parser::parse(&src, "probe.synth".to_string());
    let board = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "probe.synth")
        .board
        .expect("board");
    let result =
        synth_kicad::export_schematic_only(&board, &dir.join("out"), None).expect("export");
    let netlist = dir.join("probe.net");
    let run = Command::new("kicad-cli")
        .args([
            "sch",
            "export",
            "netlist",
            "--format",
            "kicadsexpr",
            "--output",
        ])
        .arg(&netlist)
        .arg(&result.schematic_path)
        .output();
    let Ok(run) = run else {
        eprintln!("kicad-cli not installed; skipping netlist test");
        return None;
    };
    assert!(
        run.status.success(),
        "kicad-cli sch export netlist failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    Some(std::fs::read_to_string(&netlist).expect("netlist written"))
}

fn unconnected_ic_pins(netlist: &str) -> Vec<&str> {
    netlist
        .lines()
        .filter_map(|line| line.trim().strip_prefix("(name \"unconnected-(U1-"))
        .map(|rest| rest.trim_end_matches(")\")"))
        .collect()
}

#[test]
fn every_pin_of_a_synthesized_ic_is_connected_in_the_kicad_netlist() {
    for (top, left, right) in [
        (1, 2, 2),
        (2, 2, 2),
        (3, 2, 2),
        (3, 0, 0),
        (4, 3, 1),
        (5, 4, 4),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let Some(netlist) = export_netlist(top, left, right, dir.path()) else {
            return;
        };
        let unconnected = unconnected_ic_pins(&netlist);
        assert!(
            unconnected.is_empty(),
            "{top} top / {left} left / {right} right pins: KiCad sees unconnected U1 pins {unconnected:?}"
        );
    }
}
