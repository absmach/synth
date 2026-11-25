// SPDX-License-Identifier: Apache-2.0

//! Report-only check for hard placement hints that fix a board span. A hard
//! `near` at a pin is electrical proximity and stays hard; a hard `edge` or
//! `region` on a part with no mechanical reason, or a `near` chain ending in
//! one, pins the cluster against a board side. Measured on the gateway: 157.4 x
//! 113.4 mm (17 849 mm2) with the hints as written, unchanged with the 38
//! span-fixing hints soft. Softening every non-physical hard hint instead also
//! relaxes the electrical `near` hints at pins, which this rule leaves hard.

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
    assert!(message.contains("enclosure"), "{message}");
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
            component J2: jack "rj45_hr911105a" {
                placement_hint { edge right priority: hard }
            }
            component J3: receptacle "usb_c_receptacle" {
                placement_hint { edge bottom priority: hard }
            }
            component H1: mounting_hole "mounting_hole_m2_5" {
                placement_hint { region top_left priority: hard }
            }
        }"#,
        "physical.synth",
    );
    assert!(hint_warnings(&board).is_empty());
}

/// A plain `near` does not fix a board span, so a mounting hole chained to a
/// part with no span-fixing hint is not reported either.
#[test]
fn a_plain_hard_near_on_a_mounting_hole_is_not_reported() {
    let board = lower(
        r#"board "hole_near" {
            layers 2
            component U1: ic "stm32g030k8_lqfp32"
            component H1: mounting_hole "mounting_hole_m2_5" {
                placement_hint { near: "U1" priority: hard }
            }
        }"#,
        "hole_near.synth",
    );
    assert!(hint_warnings(&board).is_empty());
}

/// Only an edge hint makes a board-edge part physical; a region hint on the
/// same kind still pins it without a mechanical reason.
#[test]
fn a_jack_region_hint_is_still_reported() {
    let board = lower(
        r#"board "jack" {
            layers 2
            component J1: jack "rj45_hr911105a" {
                placement_hint { region centre priority: hard }
            }
        }"#,
        "jack.synth",
    );
    assert_eq!(hint_warnings(&board).len(), 1);
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
    assert!(message.contains("it pins the part"), "{message}");
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
    assert!(
        message.contains("follows a part pinned"),
        "a near chain must not claim the part itself is edge-pinned: {message}"
    );
}

// ---------------------------------------------------------------------------
// Audit: which hard hints W-SYNTH-BOARD-005 flags, and which ones still block
// a compact placement. Measured with the placer on the same fixture, because
// the rule's own reading is the hypothesis under test.
// ---------------------------------------------------------------------------

fn fitted_size(board: &Board) -> Option<(f64, f64)> {
    synth_place::place(board).ok().map(|placement| {
        (
            synth_geometry::nm_to_mm(placement.board_outline.width_nm()),
            synth_geometry::nm_to_mm(placement.board_outline.height_nm()),
        )
    })
}

fn without_hints(board: &Board) -> Board {
    let mut bare = board.clone();
    for component in &mut bare.components {
        component.placement_hint = None;
    }
    bare
}

fn area((w, h): (f64, f64)) -> f64 {
    w * h
}

/// Whether the hinted fixture takes a larger fitted canvas than the same
/// parts with every hint removed. `None` when the hinted board does not place.
fn blocks_compaction(board: &Board) -> Option<bool> {
    let hinted = fitted_size(board)?;
    let bare = fitted_size(&without_hints(board))?;
    Some(area(hinted) > area(bare) + 1.0)
}

fn audit(source: &str) -> (bool, Option<bool>) {
    let board = lower(source, "audit.synth");
    let hinted = fitted_size(&board);
    let bare = fitted_size(&without_hints(&board));
    println!("    hinted canvas {hinted:?}, without hints {bare:?}");
    (!hint_warnings(&board).is_empty(), blocks_compaction(&board))
}

/// a. A hard `near` on a decoupling part whose anchor is not edge-pinned is
/// electrical proximity. The rule leaves it alone; the placer still pays for
/// it because the hint moves the part out of the passive packer.
#[test]
fn audit_a_decoupling_near_an_unpinned_mcu_is_not_reported() {
    let (fires, blocks) = audit(
        r#"board "a" {
            layers 2
            component U1: ic "stm32g030k8_lqfp32"
            component C1: capacitor "c_generic_0603" { placement_hint { near: "U1" priority: hard } }
            component C2: capacitor "c_generic_0603"
            component R1: resistor "r_generic_0603"
            component R2: resistor "r_generic_0603"
            connect U1.vdd -> C1.p1
            connect U1.vdd -> C2.p1
            connect U1.vdd -> R1.p1
            connect U1.vdd -> R2.p1
            connect U1.vss -> C1.p2
            connect U1.vss -> C2.p2
            connect U1.vss -> R1.p2
            connect U1.vss -> R2.p2
        }"#,
    );
    assert!(
        !fires,
        "a near chain without a span-fixing anchor is not W-005"
    );
    println!("a. W-005 no, blocks compaction: {blocks:?}");
}

/// b. A hard `near` on a wired test point is electrical too.
#[test]
fn audit_b_a_test_point_near_the_mcu_is_not_reported() {
    let source = r#"board "b" {
        layers 2
        component U1: ic "stm32g030k8_lqfp32"
        component TP1: testpoint "testpoint_pad_1_5" { placement_hint { near: "U1" priority: hard } }
        connect U1.vdd -> TP1.p1
    }"#;
    let board = lower(source, "audit.synth");
    assert!(hint_warnings(&board).is_empty());
    let (fires, blocks) = audit(source);
    assert!(!fires);
    println!("b. W-005 no, blocks compaction: {blocks:?}");
}

/// c. A hard `near` on a regulator names its support capacitor: electrical.
#[test]
fn audit_c_a_regulator_near_its_capacitor_is_not_reported() {
    let source = r#"board "c" {
        layers 2
        component U2: regulator "ams1117_3v3"
        component C1: capacitor "c_generic_0603" { placement_hint { near: "U2" priority: hard } }
        component C2: capacitor "c_generic_0603"
        component R1: resistor "r_generic_0603"
        connect U2.vin -> C1.p1
        connect U2.vout -> C2.p1
        connect U2.gnd -> R1.p1
    }"#;
    let (fires, blocks) = audit(source);
    assert!(!fires);
    println!("c. W-005 no, blocks compaction: {blocks:?}");
}

/// d. A hard `region: centre` fixes the part and is reported.
#[test]
fn audit_d_a_hard_centre_hint_is_reported() {
    let source = r#"board "d" {
        layers 2
        component U1: ic "stm32g030k8_lqfp32" { placement_hint { region centre priority: hard } }
        component C1: capacitor "c_generic_0603"
        component C2: capacitor "c_generic_0603"
        connect U1.vdd -> C1.p1
        connect U1.vdd -> C2.p1
        connect U1.vss -> C1.p2
        connect U1.vss -> C2.p2
    }"#;
    let (fires, blocks) = audit(source);
    assert!(fires, "a centre hint fixes a board span");
    println!("d. W-005 yes, blocks compaction: {blocks:?}");
}

/// e. A connector's edge is physical by design, so the rule skips it even
/// when the enclosure does not exist and the edge costs board area.
#[test]
fn audit_e_a_connector_edge_is_not_reported() {
    let source = r#"board "e" {
        layers 2
        component J1: connector "header_1x4" { placement_hint { edge left priority: hard } }
        component R1: resistor "r_generic_0603"
        component R2: resistor "r_generic_0603"
        connect J1.p1 -> R1.p1
        connect R1.p2 -> R2.p1
        connect R2.p2 -> J1.p2
    }"#;
    let (fires, blocks) = audit(source);
    assert!(
        !fires,
        "a connector edge is treated as a physical constraint"
    );
    println!("e. W-005 no, blocks compaction: {blocks:?}");
}
