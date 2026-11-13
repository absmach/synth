// SPDX-License-Identifier: Apache-2.0

//! Selecting an obsolete registry entry must fail loudly and name its live
//! substitute instead of silently using an old pin model (#373).

use std::path::Path;

#[test]
fn an_obsolete_part_is_refused_with_its_substitute() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root");
    let registry = synth_registry::load_dir(&root.join("registry").join("parts"))
        .expect("the registry must load");
    let src = r#"board "legacy" {
  layers 2
  component U1: memory "24lc256_eeprom"
}"#;
    let parsed = synth_parser::parse(src, "legacy.synth");
    let ast = parsed.ast.as_ref().expect("source parses");
    let lowered = synth_ir::lower(ast, &registry, "legacy.synth");
    let finding = lowered
        .diagnostics
        .iter()
        .find(|d| d.code == "E-SYNTH-COMP-005")
        .expect("an obsolete-part error");
    assert_eq!(finding.severity, synth_diagnostics::Severity::Error);
    let text = format!(
        "{} {}",
        finding.expected.as_deref().unwrap_or_default(),
        finding.found.as_deref().unwrap_or_default()
    );
    assert!(text.contains("24lc256_soic8"), "{text}");
}
