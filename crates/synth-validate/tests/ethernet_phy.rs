// SPDX-License-Identifier: Apache-2.0

//! `fixtures/designs/ethernet_phy.synth`, a LAN8720A and an HR911105A MagJack
//! with both MDI channels as differential pairs (absmach/synth#60), passes ERC
//! with no blocking diagnostic. The export side is `synth-kicad`'s
//! `ethernet_phy_pcb` test.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

#[test]
fn the_ethernet_phy_design_validates() {
    let path = workspace_root()
        .join("fixtures")
        .join("designs")
        .join("ethernet_phy.synth");
    let source = std::fs::read_to_string(&path).expect("fixture");
    let filename = "ethernet_phy.synth".to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse.ast.as_ref().expect("fixture parses");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let board = lowered.board.as_ref().expect("fixture lowers");
    let blocking: Vec<String> = parse
        .diagnostics
        .iter()
        .chain(&lowered.diagnostics)
        .cloned()
        .chain(synth_validate::run_erc(board, &filename))
        .filter(|d| d.severity.is_blocking())
        .map(|d| format!("{}: {}", d.code, d.found.unwrap_or_default()))
        .collect();
    assert!(blocking.is_empty(), "{blocking:#?}");
}
