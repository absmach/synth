// SPDX-License-Identifier: Apache-2.0

//! Stage D scorer gate (§7.8.7 / §7.8.11 of synth_implementation_plan.md).
//!
//! Every fixture under `fixtures/layout/` is scored by
//! `synth_layout::score` and compared against the checked-in
//! baseline file `fixtures/layout/score_baselines.json`. Because
//! layout is deterministic, the comparison is exact — any PR that
//! changes crossing count, wire length, or label-stub count on the
//! reference corpus fails here and must either justify the change
//! or regenerate the baseline deliberately:
//!
//! ```text
//! SYNTH_REGEN_SCORE_BASELINES=1 cargo test -p synth-layout --test scorer_gate
//! ```
//!
//! This turns §7.5.8's original "regressions ≥10% block the PR"
//! criterion (written but never wired, because there was no scorer
//! to wire it to) into an even stricter bit-exact gate, and it is
//! where §7.7.8's "crossing count < 5 on the reference set" target
//! is finally asserted as a number.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use synth_ir::Board;
use synth_layout::score::{score, LayoutScore};

#[derive(serde::Deserialize, serde::Serialize)]
struct Baselines {
    /// Per-fixture metrics keyed by file stem, sorted for stable diffs.
    fixtures: BTreeMap<String, LayoutScore>,
    /// Sum of crossing_count across the corpus at baseline time; the
    /// §7.7.8 gate asserts the live total stays under 5.
    total_crossing_count: u32,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(fixture_rel_path: &Path) -> Board {
    let source = std::fs::read_to_string(fixture_rel_path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", fixture_rel_path.display()));
    let filename = fixture_rel_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse
        .ast
        .unwrap_or_else(|| panic!("{} must parse", fixture_rel_path.display()));
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, &filename);
    lowered
        .board
        .unwrap_or_else(|| panic!("{} must lower", fixture_rel_path.display()))
}

fn corpus() -> Vec<PathBuf> {
    let dir = workspace_root().join("fixtures").join("layout");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "fixtures/layout/ must not be empty");
    paths
}

fn score_corpus() -> Vec<(String, LayoutScore)> {
    corpus()
        .into_iter()
        .map(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let board = board_for(&path);
            let layout = synth_layout::layout(&board);
            (stem, score(&layout, &board))
        })
        .collect()
}

fn baseline_path() -> PathBuf {
    workspace_root()
        .join("fixtures")
        .join("layout")
        .join("score_baselines.json")
}

#[test]
fn scorer_matches_checked_in_baselines_and_crossing_gate() {
    let live = score_corpus();

    // §7.7.8's headline gate, asserted as a number: fewer than five
    // crossings across the whole reference corpus.
    let total_crossings: u32 = live.iter().map(|(_, s)| s.crossing_count).sum();
    assert!(
        total_crossings < 5,
        "corpus crossing count {total_crossings} violates the §7.7.8 < 5 gate"
    );

    if std::env::var("SYNTH_REGEN_SCORE_BASELINES").is_ok() {
        let mut fixtures = BTreeMap::new();
        for (stem, s) in &live {
            fixtures.insert(stem.clone(), s.clone());
        }
        let baselines = Baselines {
            fixtures,
            total_crossing_count: total_crossings,
        };
        let json = serde_json::to_string_pretty(&baselines).unwrap();
        std::fs::write(baseline_path(), json + "\n").expect("write baseline file");
        eprintln!(
            "regenerated {} ({} fixtures, {total_crossings} crossings)",
            baseline_path().display(),
            live.len()
        );
        return;
    }

    let raw = std::fs::read_to_string(baseline_path()).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}) — regenerate with SYNTH_REGEN_SCORE_BASELINES=1",
            baseline_path().display()
        )
    });
    let baselines: Baselines =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("invalid baseline JSON: {e}"));

    for (stem, live_score) in &live {
        let base = baselines.fixtures.get(stem).unwrap_or_else(|| {
            panic!("no baseline for `{stem}` — regenerate with SYNTH_REGEN_SCORE_BASELINES=1")
        });
        assert_eq!(
            live_score.crossing_count, base.crossing_count,
            "{stem}: crossing count regressed"
        );
        assert_eq!(
            live_score.label_stub_count, base.label_stub_count,
            "{stem}: label-stub count regressed"
        );
        assert!(
            (live_score.total_wire_length_mm - base.total_wire_length_mm).abs() < 0.001,
            "{stem}: wire length regressed (live {} vs baseline {})",
            live_score.total_wire_length_mm,
            base.total_wire_length_mm
        );
    }
    for stem in baselines.fixtures.keys() {
        assert!(
            live.iter().any(|(s, _)| s == stem),
            "baseline references `{stem}` which no longer exists in fixtures/layout/"
        );
    }
    assert_eq!(
        baselines.total_crossing_count, total_crossings,
        "corpus total crossing count moved — update the baseline deliberately"
    );
}
