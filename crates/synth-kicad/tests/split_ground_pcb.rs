// SPDX-License-Identifier: Apache-2.0

//! A design that declares separate analog and digital grounds must keep both
//! names in the exported `.kicad_pcb`, and every generated ground zone must
//! carry its own uuid.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const SPLIT_GROUND_SOURCE: &str = r#"board "split_gnd" {
  layers 4
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"
  component C5: capacitor "c_generic_0603" value "100nF"
  component C6: capacitor "c_generic_0603" value "100nF"
  connect C1.p2 -> C2.p2 as "AGND"
  connect C2.p2 -> C3.p2
  connect C4.p2 -> C5.p2 as "DGND"
  connect C5.p2 -> C6.p2
}
"#;

const SINGLE_GROUND_SOURCE: &str = r#"board "single_gnd" {
  layers 4
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component R1: resistor "r_generic_0603" value "10k"
  connect R1.p1 -> C1.p1
  connect C1.p2 -> C2.p2 as "AGND"
  connect R1.p2 -> C2.p1
}
"#;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn export_board(source: &str, label: &str) -> synth_kicad::ExportResult {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(source, "split_gnd.synth");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "split_gnd.synth");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let board = lowered.board.expect("board");

    let out = std::env::temp_dir().join(format!(
        "synth-kicad-split-gnd-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&out);
    synth_kicad::export(&board, &out).expect("export")
}

fn export_pcb(source: &str, label: &str) -> String {
    let result = export_board(source, label);
    let text = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let _ = fs::remove_dir_all(&result.out_dir);
    text
}

fn quoted_values_after<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    text.match_indices(key)
        .filter_map(|(at, _)| text[at + key.len()..].split('"').next())
        .collect()
}

fn declared_nets(pcb: &str) -> Vec<&str> {
    pcb.lines()
        .filter_map(|line| line.trim().strip_prefix("(net "))
        .filter_map(|rest| rest.split('"').nth(1))
        .collect()
}

#[test]
fn split_grounds_keep_their_names_and_each_zone_its_own_uuid() {
    let pcb = export_pcb(SPLIT_GROUND_SOURCE, "split");

    let declared = declared_nets(&pcb);
    for name in ["AGND", "DGND"] {
        assert!(declared.contains(&name), "{name} not in {declared:?}");
    }
    assert!(!declared.contains(&"GND"), "AGND was renamed: {declared:?}");

    let zones: Vec<&str> = pcb.split("(zone").skip(1).collect();
    assert_eq!(zones.len(), 8, "expected AGND and DGND zones on 4 layers");
    let uuids: HashSet<&str> = zones
        .iter()
        .filter_map(|zone| quoted_values_after(zone, "(uuid \"").first().copied())
        .collect();
    assert_eq!(uuids.len(), zones.len(), "zones share uuids: {uuids:?}");

    let legacy = [
        "d72fbbe7-0d2e-5ba1-b608-354074f9a01a",
        "39953160-305c-53c9-9c17-4dc783a1353e",
        "c4b942ca-ffc3-5780-a79b-737734c39a95",
        "841f9dfc-bed1-5d60-93e1-7d4bb17f4269",
    ];
    for uuid in legacy {
        assert!(uuids.contains(uuid), "legacy zone uuid {uuid} changed");
    }
}

#[test]
fn a_lone_ground_net_is_still_named_gnd() {
    let pcb = export_pcb(SINGLE_GROUND_SOURCE, "single");

    let declared = declared_nets(&pcb);
    assert!(declared.contains(&"GND"), "{declared:?}");
    assert!(!declared.contains(&"AGND"), "{declared:?}");
}

/// A rail declared as a plane: `netclass "PWR" { plane }` floods a zone for
/// each member net, not just for ground.
const PLANE_SOURCE: &str = r#"board "rail_plane" {
  layers 4
  netclass "PWR" {
    plane
  }
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "VBUS" class "PWR"
}
"#;

/// The same board with no `plane` attribute: the class still routes, but no
/// zone is flooded for it.
const NO_PLANE_SOURCE: &str = r#"board "rail_no_plane" {
  layers 4
  netclass "PWR" {
    trace_width 0.5mm
  }
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "VBUS" class "PWR"
}
"#;

#[test]
fn a_declared_plane_floods_a_zone_for_its_net() {
    let pcb = export_pcb(PLANE_SOURCE, "rail-plane");
    assert!(
        pcb.contains("net_name \"VBUS\""),
        "the declared plane must flood a VBUS zone: {pcb}"
    );
}

#[test]
fn a_class_without_the_plane_attribute_floods_no_zone() {
    let pcb = export_pcb(NO_PLANE_SOURCE, "rail-no-plane");
    assert!(
        !pcb.contains("net_name \"VBUS\""),
        "a class that does not declare a plane must not flood one: {pcb}"
    );
}

/// Six decoupling capacitors between a ground net (auto plane) and a declared
/// power plane. Ground keeps the outer layers and the power plane takes the
/// inner ones, so the two never compete for a layer.
const PLANE_PRIORITY_SOURCE: &str = r#"board "plane_priority" {
  layers 4
  netclass "PWR" {
    plane
  }
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"
  component C5: capacitor "c_generic_0603" value "100nF"
  component C6: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "GND"
  connect C2.p1 -> C3.p1
  connect C3.p1 -> C4.p1
  connect C4.p1 -> C5.p1
  connect C5.p1 -> C6.p1
  connect C1.p2 -> C2.p2 as "VBUS" class "PWR"
  connect C2.p2 -> C3.p2
  connect C3.p2 -> C4.p2
  connect C4.p2 -> C5.p2
  connect C5.p2 -> C6.p2
}
"#;

/// The `(priority N)` of the zone whose `net_name` is `net`, or `None`.
fn zone_priority(pcb: &str, net: &str) -> Option<i64> {
    let needle = format!("net_name \"{net}\"");
    for chunk in pcb.split("(zone") {
        if !chunk.contains(&needle) {
            continue;
        }
        let start = chunk.find("(priority ")? + "(priority ".len();
        let end = chunk[start..].find(')')? + start;
        return chunk[start..end].trim().parse().ok();
    }
    None
}

#[test]
fn ground_planes_outrank_declared_power_planes() {
    let pcb = export_pcb(PLANE_PRIORITY_SOURCE, "priorities");
    let ground = zone_priority(&pcb, "GND").expect("ground plane must carry a priority");
    let power = zone_priority(&pcb, "VBUS").expect("power plane must carry a priority");
    assert!(
        ground > power,
        "ground priority {ground} must exceed power priority {power} so the fill order is \
         deterministic and neither net is left with islands"
    );
}

/// The layers a net's zones are on, in emission order.
fn zone_layers<'a>(pcb: &'a str, net: &str) -> Vec<&'a str> {
    let needle = format!("net_name \"{net}\"");
    let mut out = Vec::new();
    for chunk in pcb.split("(zone") {
        if !chunk.contains(&needle) {
            continue;
        }
        if let Some(start) = chunk.find("(layer \"") {
            let s = start + "(layer \"".len();
            if let Some(end) = chunk[s..].find('"') {
                out.push(&chunk[s..s + end]);
            }
        }
    }
    out
}

#[test]
fn declared_power_planes_use_the_inner_layers() {
    let pcb = export_pcb(PLANE_PRIORITY_SOURCE, "layers");
    let ground = zone_layers(&pcb, "GND");
    let power = zone_layers(&pcb, "VBUS");
    assert!(
        ground.contains(&"F.Cu") && ground.contains(&"B.Cu"),
        "ground must keep the outer layers: {ground:?}"
    );
    assert!(
        !ground.iter().any(|layer| layer.starts_with("In")),
        "ground must give up the inner layers when a power plane is declared, else the two \
         compete on one layer and one starves: {ground:?}"
    );
    assert!(
        power.contains(&"In1.Cu") && power.contains(&"In2.Cu"),
        "a declared power plane must use the inner layers: {power:?}"
    );
    assert!(
        !power
            .iter()
            .any(|layer| *layer == "F.Cu" || *layer == "B.Cu"),
        "a power plane must not compete with ground on the outer layers: {power:?}"
    );
}

/// Two declared rails on a four-layer board, so there is exactly one inner
/// layer for each.
const TWO_RAIL_SOURCE: &str = r#"board "two_rail" {
  layers 4
  netclass "CORE" {
    plane
  }
  netclass "IO" {
    plane
  }
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"
  component C5: capacitor "c_generic_0603" value "100nF"
  component C6: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "GND"
  connect C2.p1 -> C3.p1
  connect C3.p1 -> C4.p1
  connect C1.p2 -> C2.p2 as "V1P2" class "CORE"
  connect C2.p2 -> C3.p2
  connect C4.p2 -> C5.p2 as "V3P3" class "IO"
  connect C5.p2 -> C6.p2
}
"#;

/// Each rail gets an inner layer of its own. Pouring both on both puts two
/// nets at the same priority on the same copper, and KiCad fills one across
/// the layer and starves the other — the starvation the ground/power split
/// already avoids, one level down. On the Class-C board it left the 3.3 V
/// rail's thirty pads stranded.
#[test]
fn two_declared_rails_do_not_share_an_inner_layer() {
    let pcb = export_pcb(TWO_RAIL_SOURCE, "two-rail");
    let core = zone_layers(&pcb, "V1P2");
    let io = zone_layers(&pcb, "V3P3");
    assert!(!core.is_empty(), "the core rail must be poured: {core:?}");
    assert!(!io.is_empty(), "the I/O rail must be poured: {io:?}");
    assert!(
        core.iter().all(|layer| layer.starts_with("In")),
        "a rail plane belongs on the inner layers: {core:?}"
    );
    assert!(
        io.iter().all(|layer| layer.starts_with("In")),
        "a rail plane belongs on the inner layers: {io:?}"
    );
    assert!(
        !core.iter().any(|layer| io.contains(layer)),
        "two rails must not share a layer, or one starves the other: \
         core {core:?}, io {io:?}"
    );
}

/// Every pad on one ground net, so the only copper needed to connect them is
/// the ground plane.
const GROUND_FILL_SOURCE: &str = r#"board "ground_fill" {
  layers 4
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"
  component C5: capacitor "c_generic_0603" value "100nF"
  component C6: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "GND"
  connect C2.p1 -> C3.p1
  connect C3.p1 -> C4.p1
  connect C4.p1 -> C5.p1
  connect C5.p1 -> C6.p1
  connect C1.p2 -> C2.p2 as "GND"
  connect C2.p2 -> C3.p2
  connect C3.p2 -> C4.p2
  connect C4.p2 -> C5.p2
  connect C5.p2 -> C6.p2
}
"#;

/// With zones refilled, the ground plane must connect its pads: an un-routed
/// board whose only copper is the plane has no unconnected item when the plane
/// fills. Guarded on `kicad-cli`.
#[test]
fn ground_plane_connects_its_pads_after_refill() {
    let result = export_board(GROUND_FILL_SOURCE, "ground-fill");
    let outcome = synth_drc::run_kicad_cli_drc(&result.pcb_path);
    if outcome.evidence.reason.is_some() {
        eprintln!(
            "kicad-cli unavailable; skipping plane fill test: {}",
            outcome.evidence.summary_line()
        );
        let _ = fs::remove_dir_all(&result.out_dir);
        return;
    }
    assert_eq!(
        outcome.counts.unconnected,
        0,
        "the ground plane must connect its pads: {}",
        outcome.evidence.summary_line()
    );
    let _ = fs::remove_dir_all(&result.out_dir);
}

/// A declared power plane sits on the inner layers while its pads are on the
/// outer copper; its stitching vias must join the two, so after a refill the
/// power net has nothing unconnected. Guarded on `kicad-cli`.
#[test]
fn power_plane_connects_its_pads_after_refill() {
    let result = export_board(PLANE_PRIORITY_SOURCE, "power-fill");
    let outcome = synth_drc::run_kicad_cli_drc(&result.pcb_path);
    if outcome.evidence.reason.is_some() {
        eprintln!(
            "kicad-cli unavailable; skipping power plane fill test: {}",
            outcome.evidence.summary_line()
        );
        let _ = fs::remove_dir_all(&result.out_dir);
        return;
    }
    assert_eq!(
        outcome.counts.unconnected,
        0,
        "a power plane's stitching vias must connect its pads: {}",
        outcome.evidence.summary_line()
    );
    let _ = fs::remove_dir_all(&result.out_dir);
}

/// Three declared rails on a six-layer board, so there are four inner layers
/// for three rails: one rail takes two, and no rail shares.
const THREE_RAIL_SOURCE: &str = r#"board "three_rail" {
  layers 6
  netclass "CORE" {
    plane
  }
  netclass "IO" {
    plane
  }
  netclass "ANALOG" {
    plane
  }
  component C1: capacitor "c_generic_0603" value "100nF"
  component C2: capacitor "c_generic_0603" value "100nF"
  component C3: capacitor "c_generic_0603" value "100nF"
  component C4: capacitor "c_generic_0603" value "100nF"
  component C5: capacitor "c_generic_0603" value "100nF"
  component C6: capacitor "c_generic_0603" value "100nF"
  component C7: capacitor "c_generic_0603" value "100nF"
  component C8: capacitor "c_generic_0603" value "100nF"
  connect C1.p1 -> C2.p1 as "GND"
  connect C2.p1 -> C3.p1
  connect C3.p1 -> C4.p1
  connect C4.p1 -> C5.p1
  connect C1.p2 -> C2.p2 as "V1P2" class "CORE"
  connect C3.p2 -> C4.p2 as "V3P3" class "IO"
  connect C5.p2 -> C6.p2 as "V1P8" class "ANALOG"
  connect C7.p2 -> C8.p2 as "SPARE"
}
"#;

/// Rails must be dealt across the inner layers with a stride, not in
/// contiguous blocks. Four layers shared out to three rails in blocks of
/// `ceil(4/3) = 2` gives the first rail In1/In2, the second In3/In4, and the
/// third In1/In2 again — so the first and third starve each other on both,
/// which is the bug this is supposed to fix.
#[test]
fn three_declared_rails_do_not_share_an_inner_layer() {
    let pcb = export_pcb(THREE_RAIL_SOURCE, "three-rail");
    let core = zone_layers(&pcb, "V1P2");
    let io = zone_layers(&pcb, "V3P3");
    let analog = zone_layers(&pcb, "V1P8");
    for (name, layers) in [("core", &core), ("io", &io), ("analog", &analog)] {
        assert!(!layers.is_empty(), "the {name} rail must be poured");
        assert!(
            layers.iter().all(|layer| layer.starts_with("In")),
            "a rail plane belongs on the inner layers: {name} {layers:?}"
        );
    }
    for (a_name, a, b_name, b) in [
        ("core", &core, "io", &io),
        ("core", &core, "analog", &analog),
        ("io", &io, "analog", &analog),
    ] {
        assert!(
            !a.iter().any(|layer| b.contains(layer)),
            "two rails must not share a layer, or one starves the other: \
             {a_name} {a:?}, {b_name} {b:?}"
        );
    }
    let mut all: Vec<&str> = core.iter().chain(&io).chain(&analog).copied().collect();
    all.sort_unstable();
    assert_eq!(
        all,
        ["In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu"],
        "three rails over four inner layers must use all four exactly once"
    );
}
