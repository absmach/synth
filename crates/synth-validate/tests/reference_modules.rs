// SPDX-License-Identifier: Apache-2.0

//! Every reference module in `registry/modules/` must validate clean in its
//! own fixture, and a deviation from a fixed value must be recorded.

use synth_diagnostics::Diagnostic;
use synth_registry::reference_modules::{self, ReferenceModule};

fn registry() -> synth_registry::Registry {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/parts");
    synth_registry::load_dir(&dir).expect("registry loads")
}

/// Place the module declaration inside the fixture's board, as
/// `synth_instantiate_module` does.
fn compose(module: &ReferenceModule, design: &str) -> String {
    let open = design.find('{').expect("fixture has a board") + 1;
    format!(
        "{}\n{}\n{}",
        &design[..open],
        module.source,
        &design[open..]
    )
}

fn validate(source: &str, registry: &synth_registry::Registry) -> Vec<Diagnostic> {
    let file = "fixture.synth".to_string();
    let parse = synth_parser::parse(source, file.clone());
    let mut all = parse.diagnostics.clone();
    let Some(ast) = parse.ast.as_ref() else {
        return all;
    };
    let lowered = synth_ir::lower(ast, registry, &file);
    all.extend(lowered.diagnostics);
    if let Some(board) = lowered.board.as_ref() {
        all.extend(synth_validate::run_erc(board, &file));
    }
    all
}

fn modules() -> Vec<ReferenceModule> {
    reference_modules::load_modules(&reference_modules::default_modules_dir())
        .expect("module library loads")
}

#[test]
fn every_reference_module_fixture_validates_clean() {
    let registry = registry();
    for module in modules() {
        let diags = validate(&compose(&module, &module.fixture), &registry);
        let blocking: Vec<String> = diags
            .iter()
            .filter(|d| d.severity.is_blocking())
            .map(|d| format!("{}: {} {:?}", d.code, d.title, d.found))
            .collect();
        assert!(
            blocking.is_empty(),
            "{}: blocking diagnostics: {blocking:#?}",
            module.id
        );
    }
}

fn ldo_with(params: &str) -> String {
    format!(
        r#"board "b" {{
  layers 2
  component J1: connector "header_1x4"
  power "VIN" 5v {{ J1.p1 }}
  power "+3V3" 3.3v {{ J1.p2 }}
  net "GND" {{ J1.p3 }}
  no_connect J1.p4
  use "LdoAms1117" as LDO {params} {{
    vin -> "VIN"
    vout -> "+3V3"
    gnd -> "GND"
  }}
}}"#
    )
}

fn ldo() -> ReferenceModule {
    modules()
        .into_iter()
        .find(|m| m.id == "ldo_ams1117_3v3")
        .expect("ldo module")
}

#[test]
fn a_fixed_value_overridden_without_a_deviation_is_an_error() {
    let m = ldo();
    let diags = validate(&compose(&m, &ldo_with("(c_out = 47uF)")), &registry());
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E-SYNTH-MODULE-008" && d.severity.is_blocking()),
        "{diags:#?}"
    );
}

#[test]
fn every_deviation_is_listed_in_one_info_diagnostic() {
    let m = ldo();
    let src = ldo_with(
        r#"(c_out = 47uF deviation "polymer part with the ESR the data sheet asks for", deviation "SOT-223 swapped for the thermal pad layout")"#,
    );
    let diags = validate(&compose(&m, &src), &registry());
    assert!(
        diags.iter().all(|d| d.code != "E-SYNTH-MODULE-008"),
        "{diags:#?}"
    );
    let listed: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| d.code == "E-SYNTH-MODULE-009")
        .collect();
    assert_eq!(listed.len(), 1, "{diags:#?}");
    let text = listed[0].found.as_deref().unwrap_or_default();
    assert!(
        text.contains("LDO (LdoAms1117): `c_out` = 47uf: polymer part"),
        "{text}"
    );
    assert!(text.contains("the instance: SOT-223 swapped"), "{text}");
    assert!(!listed[0].severity.is_blocking());
}

#[test]
fn a_design_with_no_deviation_gets_no_listing() {
    let m = ldo();
    let diags = validate(&compose(&m, &ldo_with("")), &registry());
    assert!(diags.iter().all(|d| d.code != "E-SYNTH-MODULE-009"));
}
