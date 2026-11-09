// SPDX-License-Identifier: Apache-2.0

//! Own test binary: `SYNTH_STEP_MODEL_DIRS` is read once per process.

use std::path::Path;

use synth_layout::qualify_facts::InstalledKicad;
use synth_registry::qualify::{qualify_registry, FootprintFacts, ModelFacts};

#[test]
fn a_part_whose_model_is_only_in_a_step_models_dir_has_no_model_finding() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/registry-qualify");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("SOT-223-3.step"), "").unwrap();
    std::env::set_var("KICAD_FOOTPRINT_DIR", root.join("footprints"));
    std::env::set_var("SYNTH_STEP_MODEL_DIRS", tmp.path());

    let facts = InstalledKicad;
    assert_eq!(facts.model("Qual:SOT-223-3"), Some(ModelFacts::Found));
    assert_eq!(
        facts.model("Qual:Model-Missing"),
        Some(ModelFacts::FileMissing("Absent.step".into()))
    );
    let registry = synth_registry::load_dir(&root.join("parts")).unwrap();
    let report = qualify_registry(&registry, &facts, &facts, "test");
    let regulator = report
        .parts
        .iter()
        .find(|p| p.part_id == "qual_regulator")
        .unwrap();
    assert!(regulator.findings().all(|f| f.code != "E-SYNTH-QUAL-016"));
}
