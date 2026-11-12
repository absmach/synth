// SPDX-License-Identifier: Apache-2.0

//! A declared no-connect is not a floating CMOS input (#431).

use std::path::Path;

use synth_diagnostics::Diagnostic;

fn validate(src: &str) -> Vec<Diagnostic> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
    let parse = synth_parser::parse(src, "nc.synth");
    let ast = parse.ast.as_ref().expect("source parses");
    let lowered = synth_ir::lower(ast, &registry, "nc.synth");
    let mut all = parse.diagnostics;
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, "nc.synth"));
    }
    all
}

fn floating(diags: &[Diagnostic]) -> Vec<&str> {
    diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-CONNECT-008")
        .filter_map(|d| d.expected.as_deref())
        .collect()
}

const BASE: &str = r#"board "b" {
  layers 2
  component U1: ic "ch340c"
  component C1: capacitor "c_generic_0603" value "100nf"
  power "+3V3" 3.3v { U1.vcc U1.v3 C1.p1 }
  power "GND" 0v { U1.gnd C1.p2 }
"#;

#[test]
fn one_declared_no_connect_removes_one_floating_input() {
    let without = validate(&format!("{BASE}}}"));
    let with = validate(&format!("{BASE}  no_connect U1.cts\n}}"));
    assert_eq!(
        floating(&without).len(),
        floating(&with).len() + 1,
        "declaring cts as no-connect must remove exactly its finding"
    );
}
