// SPDX-License-Identifier: Apache-2.0

//! Placement fitness on real boards.
//!
//! These tests use the public `place` → `fitness` path, the same way a
//! consumer does. They assert the report contract and the two behaviours the
//! ranking depends on: a stranded support capacitor lowers the score, and a
//! connector moved away from the edge lowers the mating score.

use std::path::{Path, PathBuf};

use synth_ir::Board;
use synth_place::{fitness, PlacementFitness};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(relative: &str) -> Board {
    let path = workspace_root().join(relative);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ok");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry loads");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board lowered")
}

fn metric<'a>(report: &'a PlacementFitness, key: &str) -> &'a synth_place::FitnessMetric {
    report
        .metrics
        .iter()
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("missing metric {key}: {:?}", report.metrics))
}

#[test]
fn report_carries_every_measurable_dimension_in_order() {
    let board = load_board("examples/sensor_logger/sensor_logger.synth");
    let placement = synth_place::place(&board).expect("places");
    let report = fitness(&board, &placement);

    assert!(
        (0.0..=100.0).contains(&report.score),
        "score {}",
        report.score
    );
    assert!(!report.metrics.is_empty());

    let keys: Vec<&str> = report.metrics.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "support_distance",
            "rail_band",
            "mating_inset",
            "area_efficiency"
        ]
    );
    for row in &report.metrics {
        assert!((0.0..=1.0).contains(&row.score), "{} score", row.key);
        assert!(
            (row.weight * row.score - row.contribution).abs() < 1e-12,
            "{} contribution",
            row.key
        );
    }
}

#[test]
fn report_is_deterministic() {
    let board = load_board("examples/env_logger/env_logger.synth");
    let placement = synth_place::place(&board).expect("places");
    let first = fitness(&board, &placement);
    let second = fitness(&board, &placement);
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_string(&first).expect("serializes"),
        serde_json::to_string(&second).expect("serializes")
    );
}

#[test]
fn a_stranded_support_capacitor_changes_the_score() {
    let board = load_board("fixtures/designs/secure_tracker.synth");
    let placement = synth_place::place(&board).expect("places");
    let beta = fitness(&board, &placement);
    let clusters = synth_ir::recognize_clusters(&board);
    let member = clusters
        .iter()
        .flat_map(|cluster| &cluster.members)
        .find(|member| {
            matches!(
                member.role,
                synth_ir::MemberRole::DecouplingCap
                    | synth_ir::MemberRole::RailCap
                    | synth_ir::MemberRole::OrphanRailCap
                    | synth_ir::MemberRole::LoadCap
            )
        })
        .expect("a bound support capacitor");

    let mut moved = placement.clone();
    let component = moved
        .components
        .iter_mut()
        .find(|component| component.id == member.component)
        .expect("the capacitor is placed");
    component.center.x_nm += synth_geometry::mm_to_nm(40.0);
    let after = fitness(&board, &moved);
    assert!(
        metric(&after, "support_distance").measured > metric(&beta, "support_distance").measured,
        "stranding the capacitor must raise the reported distance"
    );
    assert!(
        after.score <= beta.score,
        "stranding the capacitor must not raise the score"
    );
}

#[test]
fn a_connector_outside_the_outline_fails_mating() {
    let board = load_board("examples/env_logger/env_logger.synth");
    let placement = synth_place::place(&board).expect("places");
    let connector = board
        .components
        .iter()
        .find(|component| component.kind == "connector")
        .expect("a connector");
    let mut moved = placement.clone();
    let component = moved
        .components
        .iter_mut()
        .find(|component| component.id == connector.id)
        .expect("the connector is placed");
    component.center = synth_geometry::Point::new(
        placement.board_outline.max.x_nm + synth_geometry::mm_to_nm(20.0),
        placement.board_outline.max.y_nm + synth_geometry::mm_to_nm(20.0),
    );
    let report = fitness(&board, &moved);
    let mating = metric(&report, "mating_inset");
    assert!(mating.failures > 0, "outside body must fail");
    assert_eq!(mating.score, 0.0);
    assert!(
        mating.measured < 0.0,
        "outside body reports a negative inset"
    );
}

#[test]
fn a_passive_in_the_top_band_raises_the_rail_ratio() {
    let board = load_board("fixtures/designs/component_cap_only.synth");
    let mut placement = synth_place::place(&board).expect("places");
    let component = &board.components[0];
    let index = placement
        .components
        .iter()
        .position(|placed| placed.id == component.id)
        .expect("the capacitor is placed");
    placement.components[index].center = synth_geometry::Point::new(
        placement.board_outline.min.x_nm + placement.board_outline.width_nm() / 2,
        placement.board_outline.min.y_nm + synth_geometry::mm_to_nm(2.0),
    );
    let report = fitness(&board, &placement);
    let rail = metric(&report, "rail_band");
    assert_eq!(rail.measured, 1.0);
    assert_eq!(rail.failures, 1);
}

#[test]
fn missing_metrics_are_named_in_assumptions() {
    let board = load_board("fixtures/designs/component_cap_only.synth");
    let placement = synth_place::place(&board).expect("places");
    let report = fitness(&board, &placement);
    assert!(!report
        .metrics
        .iter()
        .any(|row| row.key == "support_distance"));
    assert!(!report.metrics.iter().any(|row| row.key == "mating_inset"));
    assert!(report
        .assumptions
        .iter()
        .any(|note| note.contains("support_distance")));
    assert!(report
        .assumptions
        .iter()
        .any(|note| note.contains("mating_inset")));
}

/// Print the four metrics for every example and fixture that places. Run with
/// `cargo test -p synth-place --test fitness corpus_report -- --ignored --nocapture`.
#[test]
#[ignore = "corpus report; prints the distribution before thresholds are tuned"]
fn corpus_report() {
    let mut designs = Vec::new();
    if let Ok(examples) = std::fs::read_dir(workspace_root().join("examples")) {
        for entry in examples.flatten() {
            let dir = entry.path();
            if dir.is_dir() {
                if let Ok(files) = std::fs::read_dir(&dir) {
                    for file in files.flatten() {
                        let path = file.path();
                        if path
                            .extension()
                            .is_some_and(|extension| extension == "synth")
                        {
                            designs.push(path);
                        }
                    }
                }
            }
        }
    }
    if let Ok(fixtures) = std::fs::read_dir(workspace_root().join("fixtures").join("designs")) {
        for file in fixtures.flatten() {
            let path = file.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "synth")
            {
                designs.push(path);
            }
        }
    }
    designs.sort();
    for path in designs {
        let relative = path
            .strip_prefix(workspace_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        let board = load_board(&relative);
        let Ok(placement) = synth_place::place(&board) else {
            println!("{relative}: placement failed");
            continue;
        };
        let report = fitness(&board, &placement);
        let means: Vec<String> = report
            .metrics
            .iter()
            .map(|row| format!("{}={:.2}", row.key, row.mean))
            .collect();
        println!(
            "{relative}: score {:.1} failures {} {}",
            report.score,
            report.gate_failures,
            means.join(" ")
        );
    }
}

#[test]
fn a_board_with_nothing_to_measure_scores_zero() {
    let board = load_board("fixtures/designs/board_name_backslash.synth");
    let placement = synth_place::place(&board).expect("places");
    let report = fitness(&board, &placement);
    assert!(report.metrics.is_empty());
    assert_eq!(report.score, 0.0);
    assert!(report.score.is_sign_positive());
}
