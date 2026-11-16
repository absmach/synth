// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use synth_diagnostics::{Diagnostic, Severity};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn fixture_source(fixture: &str) -> String {
    let path = workspace_root()
        .join("fixtures")
        .join("erc")
        .join(format!("{fixture}.synth"));
    std::fs::read_to_string(path).expect("fixture must exist")
}

fn diagnostics(fixture: &str) -> Vec<Diagnostic> {
    diagnostics_of(&fixture_source(fixture), &format!("{fixture}.synth"))
}

fn diagnostics_of(src: &str, filename: &str) -> Vec<Diagnostic> {
    let parse = synth_parser::parse(src, filename.to_string());
    let ast = parse.ast.as_ref().expect("source must parse");
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry must load");
    let lowered = synth_ir::lower(ast, &registry, filename);
    synth_validate::run_erc(lowered.board.as_ref().expect("board must lower"), filename)
}

fn with_code<'a>(diags: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

fn says_estimate(d: &Diagnostic) {
    assert!(
        d.title.contains("estimate")
            && d.title.contains("about 10 percent")
            && d.title.contains("expert analysis"),
        "{}",
        d.title
    );
}

fn says_pair_estimate(d: &Diagnostic) {
    assert!(
        d.title.contains("estimate")
            && d.title.contains("coupled-pair approximation")
            && d.title.contains("expert analysis")
            && !d.title.contains("about 10 percent"),
        "{}",
        d.title
    );
}

fn names_the_pair(text: Option<&str>) {
    let text = text.unwrap();
    assert!(text.contains("pair `USB_DP`/`USB_DN`"), "{text}");
}

#[test]
fn a_width_that_gives_the_target_or_no_declared_width_is_not_flagged() {
    for fixture in [
        "pass__impedance_width_ok",
        "pass__impedance_derived",
        "pass__impedance_class_without_width",
        "pass__impedance_pair_width_gap_ok",
        "pass__impedance_pair_no_stackup",
    ] {
        let diags = diagnostics(fixture);
        assert!(
            diags.iter().all(|d| !d.code.contains("IMPEDANCE")),
            "{fixture}: {diags:?}"
        );
    }
}

#[test]
fn a_width_over_twenty_percent_off_is_an_error_that_says_estimate() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-001__width_error");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Error);
    says_estimate(flagged[0]);
}

#[test]
fn a_width_ten_to_twenty_percent_off_is_a_warning() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-001__width_warning");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Warning);
    says_estimate(flagged[0]);
}

#[test]
fn an_unreachable_target_is_an_error_that_says_estimate() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-002__unreachable_target");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-002");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Error);
    says_estimate(flagged[0]);
    assert!(
        flagged[0]
            .title
            .contains("outside the range this estimate covers"),
        "{}",
        flagged[0].title
    );
    assert!(
        with_code(&diags, "E-SYNTH-IMPEDANCE-001").is_empty(),
        "{diags:?}"
    );
}

#[test]
fn a_net_that_may_use_inner_layers_is_labelled_not_verified() {
    let diags = diagnostics("W-SYNTH-IMPEDANCE-001__inner_layers");
    let flagged = with_code(&diags, "W-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Warning);
    says_estimate(flagged[0]);
    assert!(
        flagged[0]
            .found
            .as_deref()
            .unwrap()
            .contains("not verified")
    );
}

#[test]
fn a_board_without_a_stackup_is_not_checked() {
    let diags = diagnostics("pass__rf_003_with_impedance");
    assert!(
        diags.iter().all(|d| !d.code.contains("IMPEDANCE")),
        "{diags:?}"
    );
}

#[test]
fn a_pair_gap_over_twenty_percent_off_is_an_error_that_says_estimate() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-001__pair_gap_error");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Error);
    says_pair_estimate(flagged[0]);
    names_the_pair(flagged[0].expected.as_deref());
    let found = flagged[0].found.as_deref().unwrap();
    assert!(found.contains("0.5 mm gap"), "{found}");
}

#[test]
fn a_pair_width_ten_to_twenty_percent_off_is_a_warning() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-001__pair_width_warning");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Warning);
    says_pair_estimate(flagged[0]);
}

#[test]
fn a_pair_class_without_a_clearance_is_checked_at_the_tightest_gap() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-001__pair_class_without_clearance");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Error);
    says_pair_estimate(flagged[0]);
}

#[test]
fn legs_that_declare_different_or_partial_geometry_are_an_error() {
    for fixture in [
        "E-SYNTH-IMPEDANCE-001__pair_legs_mismatch",
        "E-SYNTH-IMPEDANCE-001__pair_half_declared",
        "E-SYNTH-IMPEDANCE-001__pair_legs_mismatch_no_stackup",
        "E-SYNTH-IMPEDANCE-001__pair_half_declared_no_stackup",
    ] {
        let diags = diagnostics(fixture);
        let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
        assert_eq!(flagged.len(), 1, "{fixture}: {diags:?}");
        assert_eq!(flagged[0].severity, Severity::Error, "{fixture}");
        says_pair_estimate(flagged[0]);
        names_the_pair(flagged[0].expected.as_deref());
        assert!(flagged[0].title.contains("legs"), "{}", flagged[0].title);
    }
}

#[test]
fn the_verdict_does_not_depend_on_the_order_of_the_legs() {
    for fixture in [
        "E-SYNTH-IMPEDANCE-001__pair_legs_mismatch",
        "E-SYNTH-IMPEDANCE-001__pair_half_declared",
        "E-SYNTH-IMPEDANCE-001__pair_legs_mismatch_no_stackup",
        "E-SYNTH-IMPEDANCE-001__pair_half_declared_no_stackup",
        "E-SYNTH-IMPEDANCE-001__pair_gap_error",
        "E-SYNTH-IMPEDANCE-001__pair_width_warning",
    ] {
        let src = fixture_source(fixture);
        let swapped = src.replace("diff_pair USB_DP USB_DN", "diff_pair USB_DN USB_DP");
        assert_ne!(src, swapped);
        let verdict = |src: &str| {
            diagnostics_of(src, "swap.synth")
                .into_iter()
                .filter(|d| d.code.contains("IMPEDANCE"))
                .map(|d| (d.code, d.severity))
                .collect::<Vec<_>>()
        };
        let original = verdict(&src);
        assert!(!original.is_empty(), "{fixture}");
        assert_eq!(original, verdict(&swapped), "{fixture}");
    }
}

#[test]
fn an_unreachable_pair_target_is_an_error_that_says_estimate() {
    let diags = diagnostics("E-SYNTH-IMPEDANCE-002__pair_unreachable_target");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-002");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    assert_eq!(flagged[0].severity, Severity::Error);
    says_pair_estimate(flagged[0]);
    names_the_pair(flagged[0].found.as_deref());
    assert!(
        with_code(&diags, "E-SYNTH-IMPEDANCE-001").is_empty(),
        "{diags:?}"
    );
}

#[test]
fn a_pair_that_may_use_inner_layers_is_labelled_not_verified_once() {
    let diags = diagnostics("W-SYNTH-IMPEDANCE-001__pair_inner_layers");
    let flagged = with_code(&diags, "W-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    says_pair_estimate(flagged[0]);
    names_the_pair(flagged[0].found.as_deref());
    assert!(
        flagged[0]
            .found
            .as_deref()
            .unwrap()
            .contains("not verified")
    );
}

/// Single-ended controlled impedance declared on a netclass, which has no
/// `diff_pair` to carry the target.
const SINGLE_ENDED_OFF_TARGET: &str = r#"board "single_ended" {
  layers 2

  netclass "RF" {
    trace_width 0.12mm
    impedance 50ohm
  }

  component U1: antenna "ant_chip_2g4"
  component R1: resistor "r_generic_0603" value "10k"

  connect U1.feed -> R1.p1 as "RF_IN" class "RF"

  keepout antenna {
    radius 10mm
  }

  stackup {
    copper 0.035mm
    insulator 0.2104mm er 4.4
    copper 0.035mm
  }
}
"#;

#[test]
fn a_single_ended_netclass_width_off_target_is_flagged() {
    let diags = diagnostics_of(SINGLE_ENDED_OFF_TARGET, "single_ended.synth");
    let flagged = with_code(&diags, "E-SYNTH-IMPEDANCE-001");
    assert_eq!(flagged.len(), 1, "{diags:?}");
    let expected = flagged[0].expected.as_deref().unwrap();
    assert!(
        expected.contains("net `RF_IN` in netclass `RF`"),
        "{expected}"
    );
}

#[test]
fn a_single_ended_netclass_without_a_width_is_not_flagged() {
    let src = SINGLE_ENDED_OFF_TARGET.replace("    trace_width 0.12mm\n", "");
    let diags = diagnostics_of(&src, "single_ended_derived.synth");
    assert!(
        diags.iter().all(|d| !d.code.contains("IMPEDANCE-001")),
        "{diags:?}"
    );
}
