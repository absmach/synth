// SPDX-License-Identifier: Apache-2.0

//! An Ethernet design: a LAN8720A PHY and a HanRun HR911105A MagJack with
//! both MDI channels declared as differential pairs (absmach/synth#60). It
//! must lower, resolve both pairs to real nets, and export onto the stock
//! footprints with each leg on the pads the datasheets give it.

use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_FOOTPRINT_DIR: &str = "/usr/share/kicad/footprints";
const PHY_FOOTPRINT: &str = "Package_DFN_QFN:VQFN-24-1EP_4x4mm_P0.5mm_EP2.5x2.5mm";
const JACK_FOOTPRINT: &str = "Connector_RJ:RJ45_Hanrun_HR911105A_Horizontal";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

/// Whether the stock footprints are installed; the export embeds them, so
/// without them there is nothing to check unless `SYNTH_ALLOW_MISSING_KICAD=1`
/// says to skip.
fn stock_footprints_installed() -> bool {
    let wanted = [
        "Package_DFN_QFN.pretty/VQFN-24-1EP_4x4mm_P0.5mm_EP2.5x2.5mm.kicad_mod",
        "Connector_RJ.pretty/RJ45_Hanrun_HR911105A_Horizontal.kicad_mod",
    ];
    let found = std::env::var("KICAD_FOOTPRINT_DIR")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain([PathBuf::from(DEFAULT_FOOTPRINT_DIR)])
        .any(|dir| wanted.iter().all(|f| dir.join(f).exists()));
    if found {
        return true;
    }
    let what = format!("KICAD_FOOTPRINT_DIR, or {DEFAULT_FOOTPRINT_DIR}, with {wanted:?}");
    if std::env::var("SYNTH_ALLOW_MISSING_KICAD").as_deref() == Ok("1") {
        eprintln!("skipped on purpose (SYNTH_ALLOW_MISSING_KICAD=1): needs {what}");
        return false;
    }
    panic!("this test needs {what}; set it up, or set SYNTH_ALLOW_MISSING_KICAD=1 to skip")
}

fn board() -> synth_ir::Board {
    let path = workspace_root()
        .join("fixtures")
        .join("designs")
        .join("ethernet_phy.synth");
    let source = fs::read_to_string(&path).expect("fixture");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(&source, "ethernet_phy.synth");
    let ast = parsed.ast.as_ref().expect("fixture parses");
    let lowered = synth_ir::lower(ast, &registry, "ethernet_phy.synth");
    let board = lowered.board.expect("fixture lowers");
    let blocking: Vec<_> = parsed
        .diagnostics
        .iter()
        .chain(&lowered.diagnostics)
        .filter(|d| d.severity.is_blocking())
        .map(|d| format!("{}: {}", d.code, d.found.clone().unwrap_or_default()))
        .collect();
    // ERC is `synth-validate`'s; see its `ethernet_phy` test.
    assert!(blocking.is_empty(), "the design must lower: {blocking:?}");
    board
}

#[test]
fn both_mdi_pairs_resolve_to_nets() {
    let board = board();
    assert_eq!(board.diff_pairs.len(), 2);
    for pair in &board.diff_pairs {
        assert!(
            pair.positive_net.is_some() && pair.negative_net.is_some(),
            "{}/{} must resolve to nets",
            pair.positive,
            pair.negative
        );
    }
}

/// The net on `pad` of the footprint `reference`, read from the board text
/// with its whitespace collapsed, since the exporter splits an s-expression
/// over lines.
fn pad_net(pcb: &str, reference: &str, pad: &str) -> Option<String> {
    let pcb = pcb.split_whitespace().collect::<Vec<_>>().join(" ");
    let start = pcb.find(&format!("(property \"Reference\" \"{reference}\""))?;
    let footprint_start = pcb[..start].rfind("(footprint")?;
    let end = pcb[start..]
        .find("(footprint")
        .map_or(pcb.len(), |offset| start + offset);
    let body = &pcb[footprint_start..end];
    let needle = format!("(pad \"{pad}\"");
    let at = body.find(&needle)?;
    let pad_text = &body[at..];
    let net_at = pad_text.find("(net ")?;
    let net = &pad_text[net_at..];
    let open = net.find('"')? + 1;
    let close = net[open..].find('"')? + open;
    Some(net[open..close].to_string())
}

#[test]
fn the_pairs_export_onto_the_datasheet_pads() {
    if !stock_footprints_installed() {
        return;
    }
    let board = board();
    let out = std::env::temp_dir().join(format!("synth-kicad-ethernet-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let result = synth_kicad::export(&board, &out).expect("export");
    let pcb = fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written");
    let _ = fs::remove_dir_all(&result.out_dir);

    assert!(pcb.contains(PHY_FOOTPRINT), "U1 must use {PHY_FOOTPRINT}");
    assert!(pcb.contains(JACK_FOOTPRINT), "J1 must use {JACK_FOOTPRINT}");

    // LAN8720A TXP 21, TXN 20, RXP 23, RXN 22 to the jack's chip-side
    // TD+ 1, TD- 2, RD+ 3, RD- 6.
    for (net, phy_pad, jack_pad) in [
        ("ETH_TXP", "21", "1"),
        ("ETH_TXN", "20", "2"),
        ("ETH_RXP", "23", "3"),
        ("ETH_RXN", "22", "6"),
    ] {
        assert_eq!(
            pad_net(&pcb, "U1", phy_pad).as_deref(),
            Some(net),
            "U1.{phy_pad}"
        );
        assert_eq!(
            pad_net(&pcb, "J1", jack_pad).as_deref(),
            Some(net),
            "J1.{jack_pad}"
        );
    }
}
