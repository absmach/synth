// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use synth_layout::qualify_facts::InstalledKicad;
use synth_registry::qualify::{CheckStatus, FindingLevel, PartQualification, qualify_part};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn fixture_root() -> PathBuf {
    workspace_root().join("fixtures").join("registry-qualify")
}

fn load(dir: &str, id: &str) -> synth_registry::Part {
    let registry = synth_registry::load_dir(&fixture_root().join(dir))
        .unwrap_or_else(|e| panic!("load {dir}: {e}"));
    registry
        .lookup(id)
        .unwrap_or_else(|| panic!("no part `{id}` in {dir}"))
        .clone()
}

fn qualify_mutation(id: &str) -> PartQualification {
    let facts = InstalledKicad;
    qualify_part(&load(&format!("mutations/{id}"), id), &facts, &facts)
}

fn codes(q: &PartQualification) -> Vec<String> {
    q.findings().map(|f| f.code.clone()).collect()
}

fn qualify(dir: &str, id: &str) -> PartQualification {
    let facts = InstalledKicad;
    qualify_part(&load(dir, id), &facts, &facts)
}

/// Set in the child process that runs the qualification checks.
const CHILD_MARKER: &str = "SYNTH_REGISTRY_QUALIFICATION_CHILD";

#[test]
fn fixtures_qualify_and_every_mutation_is_caught() {
    // The footprint index is built once per process from `KICAD_FOOTPRINT_DIR`,
    // so the fixtures have to be named before the process starts. The test
    // re-runs itself as a child with the variable set, rather than changing
    // the environment of a process whose other tests run on other threads.
    if std::env::var_os(CHILD_MARKER).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "fixtures_qualify_and_every_mutation_is_caught",
                "--exact",
                "--nocapture",
            ])
            .env(CHILD_MARKER, "1")
            .env("KICAD_FOOTPRINT_DIR", fixture_root().join("footprints"))
            .output()
            .expect("run the qualification checks in a child process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A renamed test would match nothing and "pass"; require that it ran.
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "the qualification checks failed in the child process:\n{stdout}\n{stderr}"
        );
        return;
    }
    three_d_models_are_looked_up_on_disk();
    representative_parts_qualify();
    every_mutation_is_caught();
    structural_classification_matches_the_export_gate();
}

fn three_d_models_are_looked_up_on_disk() {
    use synth_registry::qualify::{FootprintFacts, ModelFacts};
    let facts = InstalledKicad;
    assert_eq!(
        facts.model("Qual:Model-Missing"),
        Some(ModelFacts::FileMissing("Absent.step".into())),
        "a footprint that names a model file which is not installed"
    );
    assert_eq!(
        facts.model("Qual:SOT-223-3"),
        Some(ModelFacts::NoneDeclared),
        "a footprint that names no model, with none installed under its name"
    );
    assert_eq!(
        facts.model("Qual:No-Such-Footprint"),
        Some(ModelFacts::NoFootprint)
    );
}

fn representative_parts_qualify() {
    for id in [
        "qual_mcu_qfn",
        "qual_regulator",
        "qual_connector",
        "qual_rf_module",
        "qual_bga",
    ] {
        let q = qualify("parts", id);
        let structural: Vec<&str> = q.structural_defects().map(|f| f.code.as_str()).collect();
        assert!(
            structural.is_empty(),
            "{id} should be structurally sound, got {structural:?}\n{:#?}",
            q.checks
        );
        assert_eq!(
            q.status,
            CheckStatus::Pass,
            "{id} should fully qualify\n{:#?}",
            q.checks
        );
    }
}

fn every_mutation_is_caught() {
    let expectations: &[(&str, &str, FindingLevel)] = &[
        (
            "mut_missing_pad",
            "E-SYNTH-QUAL-001",
            FindingLevel::Blocking,
        ),
        (
            "mut_undeclared_exposed_pad",
            "E-SYNTH-QUAL-002",
            FindingLevel::Blocking,
        ),
        (
            "mut_wrong_units",
            "E-SYNTH-QUAL-006",
            FindingLevel::Blocking,
        ),
        (
            "mut_mirrored_footprint",
            "E-SYNTH-QUAL-012",
            FindingLevel::Blocking,
        ),
        (
            "mut_stale_provenance",
            "E-SYNTH-QUAL-015",
            FindingLevel::Blocking,
        ),
        ("mut_unreviewed", "E-SYNTH-QUAL-008", FindingLevel::Blocking),
    ];

    for (id, code, level) in expectations {
        let q = qualify_mutation(id);
        let found = codes(&q);
        assert!(
            found.iter().any(|c| c == code),
            "{id} must report {code}, got {found:?}\n{:#?}",
            q.checks
        );
        let finding = q
            .findings()
            .find(|f| f.code == *code)
            .expect("just asserted present");
        assert_eq!(finding.level, *level, "{id}: {code} level");
        assert_ne!(
            q.status,
            CheckStatus::Pass,
            "{id} must not qualify\n{:#?}",
            q.checks
        );
        assert!(
            !q.is_fabrication_safe(),
            "{id} must not be fabrication safe"
        );
    }

    let wrong_units = qualify_mutation("mut_wrong_units");
    assert!(
        wrong_units
            .findings()
            .any(|f| f.code == "E-SYNTH-QUAL-006" && f.found.contains("recorded as mils")),
        "the unit mistake should be named, not just flagged:\n{:#?}",
        wrong_units.checks
    );
}

fn structural_classification_matches_the_export_gate() {
    let connector = qualify("parts", "qual_connector");
    assert!(
        !codes(&connector).iter().any(|c| c == "E-SYNTH-QUAL-002"),
        "the NPTH mounting hole must not read as an undeclared pad:\n{:#?}",
        connector.checks
    );

    for (id, structural) in [
        ("mut_missing_pad", true),
        ("mut_undeclared_exposed_pad", true),
        ("mut_mirrored_footprint", true),
        ("mut_unreviewed", false),
        ("mut_stale_provenance", false),
    ] {
        let q = qualify_mutation(id);
        assert_eq!(
            q.has_structural_defect(),
            structural,
            "{id} structural classification drives the export gate\n{:#?}",
            q.structural_defects().collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_duplicated_pad_number_is_rejected_at_load() {
    let err = synth_registry::load_dir(&fixture_root().join("rejected-at-load"))
        .expect_err("a duplicated pad number must not load");
    let text = err.to_string();
    assert!(
        text.contains("duplicate pin number"),
        "expected a duplicate-pin-number refusal, got: {text}"
    );
}
