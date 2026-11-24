// SPDX-License-Identifier: Apache-2.0

//! Report-only check for hard placement hints that are not physical
//! constraints. Measured on the gateway: the same parts fit 157.4 x 113.4 mm
//! with the hints as written and 118.0 x 115.5 mm once every non-connector,
//! non-mounting-hole hint is soft, so the hints enlarge the board by 31% of
//! area.

use std::path::{Path, PathBuf};
use synth_diagnostics::Severity;
use synth_ir::Board;
use synth_validate::board_estimate::hard_hint_diagnostics;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn registry() -> synth_registry::Registry {
    synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads")
}

fn lower(source: &str, file: &str) -> Board {
    let parse = synth_parser::parse(source, file.to_string());
    let ast = parse.ast.as_ref().expect("parse ok");
    synth_ir::lower(ast, &registry(), file)
        .board
        .expect("board lowered")
}

fn hint_warnings(board: &Board) -> Vec<synth_diagnostics::Diagnostic> {
    hard_hint_diagnostics(board, "hints.synth")
        .into_iter()
        .filter(|d| d.code == "W-SYNTH-BOARD-005")
        .collect()
}

#[test]
fn a_hard_region_hint_on_an_ic_is_reported_with_the_estimate() {
    let board = lower(
        r#"board "hint" {
            layers 2
            component U1: ic "stm32g030k8_lqfp32" {
                placement_hint { region centre priority: hard }
            }
        }"#,
        "hints.synth",
    );
    let warnings = hint_warnings(&board);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = &warnings[0];
    assert_eq!(warning.severity, Severity::Warning);
    let message = warning.message.as_deref().unwrap_or_default();
    assert!(message.contains("U1"), "{message}");
    assert!(message.contains("region: centre"), "{message}");
    assert!(message.contains("priority: hard"), "{message}");
    assert!(message.contains("parts need about"), "{message}");
    assert!(message.contains("soft"), "{message}");
    assert!(message.contains("physical"), "{message}");
    assert!(warning.expected.is_some(), "{warning:?}");
    assert!(warning.found.is_some(), "{warning:?}");
}

#[test]
fn connector_edges_and_mounting_holes_are_not_reported() {
    let board = lower(
        r#"board "physical" {
            layers 2
            component J1: connector "header_1x4" {
                placement_hint { edge left priority: hard }
            }
            component H1: mounting_hole "mounting_hole_m2_5" {
                placement_hint { region top_left priority: hard }
            }
        }"#,
        "physical.synth",
    );
    assert!(hint_warnings(&board).is_empty());
}

#[test]
fn a_soft_hint_is_not_reported() {
    let board = lower(
        r#"board "soft" {
            layers 2
            component U1: ic "stm32g030k8_lqfp32" {
                placement_hint { region centre priority: soft }
            }
        }"#,
        "soft.synth",
    );
    assert!(hint_warnings(&board).is_empty());
}

#[test]
fn a_hard_edge_names_the_distance_it_pins_against_the_estimate() {
    let board = lower(
        r#"board "edge" {
            layers 2
            outline 180mm x 120mm
            component U1: ic "stm32g030k8_lqfp32" {
                placement_hint { edge right priority: hard }
            }
        }"#,
        "edge.synth",
    );
    let warnings = hint_warnings(&board);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let message = warnings[0].message.as_deref().unwrap_or_default();
    assert!(message.contains("180.0 mm"), "{message}");
    assert!(message.contains("parts need about"), "{message}");
}

#[test]
fn a_hard_near_hint_inherits_its_targets_cluster_distance() {
    let board = lower(
        r#"board "near" {
            layers 2
            outline 180mm x 120mm
            component U1: ic "stm32g030k8_lqfp32" {
                placement_hint { edge right priority: hard }
            }
            component C1: capacitor "c_generic_0603" {
                placement_hint { near "U1" priority: hard }
            }
        }"#,
        "near.synth",
    );
    let warnings = hint_warnings(&board);
    let near = warnings
        .iter()
        .find(|d| d.found.as_deref().is_some_and(|f| f.contains("C1")))
        .unwrap_or_else(|| panic!("no C1 hint warning: {warnings:?}"));
    let message = near.message.as_deref().unwrap_or_default();
    assert!(message.contains("180.0 mm"), "{message}");
}
