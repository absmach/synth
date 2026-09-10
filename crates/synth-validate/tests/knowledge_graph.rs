// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for `E-SYNTH-KG-001`: the circuit-design
//! knowledge graph flags production support circuits that are
//! missing — switch debounce, switch pull-up, LED current limiting —
//! and supplies insertion patches whose application re-validates
//! clean. Also pins the catalog contract: templates enforced by other
//! rules never double-fire.

use std::path::{Path, PathBuf};

use synth_diagnostics::Diagnostic;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn registry_dir() -> PathBuf {
    workspace_root().join("registry").join("parts")
}

fn validate(src: &str) -> (Option<synth_ir::Board>, Vec<Diagnostic>) {
    let filename = "knowledge_graph.synth".to_string();
    let parse = synth_parser::parse(src, filename.clone());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&registry_dir()).expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, &filename);
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &filename));
    }
    (lowered.board, all)
}

fn kg_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-KG-001")
        .collect()
}

const BARE_SWITCH: &str = "\
board \"t\" {
  component SW1: switch \"spst_tactile\"
  component U1: mcu \"stm32f103c8\"
  component U3: regulator \"ams1117_3v3\"
  connect SW1.p1 -> U1.pa0
  connect SW1.p2 -> U1.vss
  connect U3.vout -> U1.vbat
  connect U3.gnd -> U1.vss
}
";

#[test]
fn bare_switch_input_emits_kg_001_with_insertion_patch() {
    let (_, diags) = validate(BARE_SWITCH);
    let kg = kg_diags(&diags);
    assert!(!kg.is_empty(), "bare switch must fire E-SYNTH-KG-001");

    // Both switch templates are represented in the KG findings.
    let joined: String = kg
        .iter()
        .map(|d| d.title.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("switch_debounce_rc"),
        "debounce template must fire: {joined}"
    );
    assert!(
        joined.contains("switch_pull_up"),
        "pull-up template must fire: {joined}"
    );

    // The rationale travels with the diagnostic — the *why* is part
    // of the knowledge graph's contract.
    let any = kg[0];
    let text = format!(
        "{} {} {}",
        any.title,
        any.expected.as_deref().unwrap_or(""),
        any.found.as_deref().unwrap_or(""),
    );
    assert!(
        text.contains("bounce") || text.contains("floating") || text.contains("debounce"),
        "diagnostic must explain itself: {text}"
    );

    // At least one finding carries a machine-applicable insertion.
    assert!(
        kg.iter().any(|d| !d.suggested_fixes.is_empty()),
        "switch findings must suggest an insertion patch"
    );
}

#[test]
fn applying_kg_patch_revalidates_switch_clean() {
    let (_, diags) = validate(BARE_SWITCH);
    let kg = kg_diags(&diags);
    let patch = kg
        .iter()
        .flat_map(|d| d.suggested_fixes.iter())
        .find(|p| matches!(p.kind, synth_diagnostics::PatchKind::InsertAt { .. }))
        .expect("at least one insertion patch");

    let patched = patch.apply(BARE_SWITCH).expect("patch must apply");
    assert!(patched.contains("auto-inserted"), "{patched}");

    let (board, re_diags) = validate(&patched);
    assert!(board.is_some(), "patched source must still lower");
    let still = kg_diags(&re_diags);
    assert!(
        still.iter().all(|d| !d.title.contains("switch_debounce_rc")
            && !d.title.contains("switch_pull_up")),
        "applying the patch must clear the switch findings: {:?}",
        still.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
}

#[test]
fn debounced_and_pulled_up_switch_is_clean() {
    // Canonical production input: switch to ground, pull-up R and
    // filter C on the signal node, node into the MCU pin.
    let src = "\
board \"t\" {
  component SW1: switch \"spst_tactile\"
  component R1: resistor \"r_generic_0603\"
  component C1: capacitor \"c_generic_0603\"
  component U1: mcu \"stm32f103c8\"
  connect SW1.p1 -> U1.pa0
  connect SW1.p1 -> R1.p1
  connect SW1.p1 -> C1.p1
  connect R1.p2 -> U1.vbat
  connect C1.p2 -> U1.vss
  connect SW1.p2 -> U1.vss
}
";
    let (_, diags) = validate(src);
    let kg = kg_diags(&diags);
    assert!(
        kg.iter().all(|d| !d.title.contains("switch_debounce_rc")
            && !d.title.contains("switch_pull_up")),
        "production-debounced input must not fire the switch templates: {:?}",
        kg.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
}

#[test]
fn catalog_templates_do_not_double_flag() {
    // The MCU declares required_decoupling (E-SYNTH-POWER-001's
    // territory) and I²C pull-ups are E-SYNTH-I2C-001's — neither may
    // appear as a KG finding.
    let src = "\
board \"t\" {
  component U1: mcu \"stm32f103c8\"
  component SW1: switch \"spst_tactile\"
  connect SW1.p1 -> U1.pa0
  connect SW1.p2 -> U1.vss
}
";
    let (_, diags) = validate(src);
    let kg = kg_diags(&diags);
    assert!(
        kg.iter().all(|d| !d.title.contains("ic_decoupling")),
        "manifest-declared decoupling belongs to E-SYNTH-POWER-001, not the KG: {:?}",
        kg.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
}
