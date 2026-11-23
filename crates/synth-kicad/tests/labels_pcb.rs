// SPDX-License-Identifier: Apache-2.0

//! Board-level `labels on|off` export tests (absmach/synth#374): with
//! `labels off` the exported PCB carries none of the reference-designator
//! and value text the exporter itself adds, while the library footprint's
//! own silkscreen stays byte-identical.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("synth-kicad-labels-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

fn export_pcb(labels: Option<&str>, dir_label: &str) -> String {
    let labels_stmt = labels.unwrap_or_default();
    let source = format!(
        r#"board "labels_export" {{
  layers 2
  {labels_stmt}
  component U1: mcu "stm32g030"
  component C1: capacitor "c_generic_0603" value "100nF"
  connect U1.vdd -> C1.p1
  connect U1.vss -> C1.p2
}}
"#
    );
    let parse = synth_parser::parse(&source, "labels_export.synth");
    let ast = parse.ast.expect("parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry/parts")).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(&ast, &loader, "labels_export.synth");
    let lowered = synth_ir::lower(&resolved.program, &registry, "labels_export.synth");
    let board = lowered.board.expect("board");

    let out = tempdir(dir_label);
    let result = synth_kicad::export(&board, &out).expect("export");
    fs::read_to_string(&result.pcb_path).expect("read pcb")
}

#[test]
fn labels_off_suppresses_the_exporters_own_reference_and_value_text() {
    let on = export_pcb(None, "on");
    let off = export_pcb(Some("labels off"), "off");

    // By default the exporter adds the reference designator on silkscreen
    // and the value on the fab layer for every footprint it places.
    assert!(
        on.contains("\"Reference\""),
        "the default export carries exporter-added reference text"
    );
    assert!(
        on.contains("\"Value\""),
        "the default export carries exporter-added value text"
    );

    // `labels off` removes exactly those — the instance overrides the
    // exporter adds. Everything else about the footprint (pads, nets,
    // geometry, the inline library body) stays.
    assert!(
        !off.contains("\"Reference\""),
        "labels off must not carry exporter-added reference text"
    );
    assert!(
        !off.contains("\"Value\""),
        "labels off must not carry exporter-added value text"
    );

    // The library footprint's own silkscreen survives: silk lines the
    // footprint body owns are still in the file.
    assert!(
        off.contains("\"F.SilkS\""),
        "labels off keeps the library footprint's own silkscreen"
    );
    assert!(off.contains("(pad "), "labels off keeps the pads");
    // The hidden bookkeeping properties (Datasheet/Description/MPN/LCSC)
    // are not silkscreen or fab text and stay.
    assert!(
        off.contains("\"MPN\""),
        "labels off keeps the sourcing properties"
    );
}

#[test]
fn labels_is_a_noop_when_on() {
    // `labels on` is the default spelled out: the export must be identical
    // to a board that says nothing.
    let default = export_pcb(None, "default");
    let explicit_on = export_pcb(Some("labels on"), "explicit-on");
    assert_eq!(default, explicit_on, "labels on is the default");
}
