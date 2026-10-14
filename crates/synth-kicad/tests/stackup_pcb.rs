// SPDX-License-Identifier: Apache-2.0

//! A declared `stackup { … }` reaches the exported `.kicad_pcb` as a
//! `(stackup …)` section inside `(setup …)`; a design without one exports
//! no stackup and the fixed default board thickness.

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
    p.push(format!(
        "synth-kicad-stackup-{label}-{}",
        std::process::id()
    ));
    if p.exists() {
        let _ = fs::remove_dir_all(&p);
    }
    p
}

const BODY: &str = r#"
  component R1: resistor "r_generic_0603" value "10k"
  component R2: resistor "r_generic_0603" value "10k"
  connect R1.p1 -> R2.p1
  connect R1.p2 -> R2.p2
"#;

const FOUR_LAYER_STACKUP: &str = r#"
  stackup {
    copper 0.035mm
    insulator 0.2104mm er 4.4 material "FR4"
    copper 1.4mil
    insulator 1.065mm er 4.6
    copper 0.0152mm
    insulator 0.2104mm er 4.4 material "FR4"
    copper 0.035mm
  }
"#;

fn export_pcb(label: &str, layers: u32, extra: &str) -> String {
    let source = format!("board \"{label}\" {{\n  layers {layers}\n{BODY}{extra}}}\n");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(&source, format!("{label}.synth"));
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let lowered = synth_ir::lower(&parsed.ast.unwrap(), &registry, &format!("{label}.synth"));
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let tmp = tempdir(label);
    let result = synth_kicad::export(&lowered.board.unwrap(), &tmp).expect("export");
    fs::read_to_string(&result.pcb_path).expect(".kicad_pcb written")
}

fn squashed(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" )", ")")
}

#[test]
fn declared_stackup_is_written_inside_setup_with_real_values() {
    let pcb = squashed(&export_pcb("stackup_four", 4, FOUR_LAYER_STACKUP));

    let setup = pcb.find("(setup").expect("setup block");
    let stackup = pcb.find("(stackup").expect("stackup section");
    assert!(stackup > setup, "stackup must live inside setup");

    let expected = [
        "(layer \"F.Cu\" (type \"copper\") (thickness 0.035))",
        "(layer \"dielectric 1\" (type \"dielectric\") (thickness 0.2104) (material \"FR4\") \
         (epsilon_r 4.4))",
        "(layer \"In1.Cu\" (type \"copper\") (thickness 0.03556))",
        "(layer \"dielectric 2\" (type \"dielectric\") (thickness 1.065) (epsilon_r 4.6))",
        "(layer \"In2.Cu\" (type \"copper\") (thickness 0.0152))",
        "(layer \"dielectric 3\" (type \"dielectric\") (thickness 0.2104) (material \"FR4\") \
         (epsilon_r 4.4))",
        "(layer \"B.Cu\" (type \"copper\") (thickness 0.035))",
    ];
    let mut from = stackup;
    for layer in expected {
        let at = pcb[from..]
            .find(layer)
            .unwrap_or_else(|| panic!("{layer} missing or out of order in {}", &pcb[stackup..]));
        from += at + layer.len();
    }
}

#[test]
fn board_thickness_is_the_stack_total_only_when_a_stackup_is_declared() {
    let with = squashed(&export_pcb("stackup_total", 4, FOUR_LAYER_STACKUP));
    assert!(with.contains("(general (thickness 1.60656)"), "{with}");
}

#[test]
fn a_design_without_a_stackup_keeps_the_fixed_thickness_and_no_stackup() {
    let pcb = squashed(&export_pcb("stackup_none", 4, ""));
    assert!(!pcb.contains("(stackup"));
    assert!(pcb.contains("(general (thickness 1.6)"), "{pcb}");
}

/// A current-carrying class is sized from the board's declared stackup, not a
/// hardcoded 0.035 mm copper: the same rail on thinner copper needs a wider
/// trace, and the ampacity check must not fail it afterwards.
#[test]
fn a_current_class_is_sized_from_the_declared_stackup() {
    // Two declared stackups that differ only in copper thickness.
    let thick = squashed(&export_pcb(
        "ampacity_thick",
        4,
        r#"  netclass "PWR" { current 1A }
  stackup {
    copper 0.035mm
    insulator 0.2mm er 4.4 material "FR4"
    copper 0.035mm
    insulator 1.2mm er 4.6
    copper 0.035mm
    insulator 0.2mm er 4.4 material "FR4"
    copper 0.035mm
  }"#,
    ));
    let thin = squashed(&export_pcb(
        "ampacity_thin",
        4,
        r#"  netclass "PWR" { current 1A }
  stackup {
    copper 0.010mm
    insulator 0.2mm er 4.4 material "FR4"
    copper 0.010mm
    insulator 1.2mm er 4.6
    copper 0.010mm
    insulator 0.2mm er 4.4 material "FR4"
    copper 0.010mm
  }"#,
    ));
    let thick_width = declared_trace_width(&thick);
    let thin_width = declared_trace_width(&thin);
    assert!(
        thin_width > thick_width,
        "0.010 mm copper width {thin_width} mm must exceed the 0.035 mm copper width {thick_width} mm"
    );
}

/// The `(trace_width …)` of the declared `PWR` net class in an exported board.
fn declared_trace_width(pcb: &str) -> f64 {
    let class = pcb.find("(net_class \"PWR\"").expect("PWR net class");
    let rest = &pcb[class..];
    let key = "(trace_width ";
    let at = rest.find(key).expect("trace_width") + key.len();
    let end = rest[at..].find(')').expect("closing paren") + at;
    rest[at..end].trim().parse().expect("width value")
}
