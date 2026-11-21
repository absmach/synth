// SPDX-License-Identifier: Apache-2.0

//! The `placement { spacing }` setting: parsed, lowered, and honoured by both
//! placement paths as the least courtyard gap between any two parts.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use synth_geometry::{Rect, nm_to_mm};
use synth_ir::Board;
use synth_place::Placement;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn lower(source: &str) -> Board {
    let file = "spacing.synth".to_string();
    let parse = synth_parser::parse(source, file.clone());
    assert!(parse.diagnostics.is_empty(), "{:?}", parse.diagnostics);
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry");
    synth_ir::lower(&parse.ast.expect("ast"), &registry, &file)
        .board
        .expect("board")
}

fn rtd_source() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rtd_afe.synth"),
    )
    .expect("fixture")
}

fn rtd_afe(spacing: Option<&str>) -> Board {
    let mut source = rtd_source();
    if let Some(value) = spacing {
        source = source.replacen(
            "  layers 4\n",
            &format!("  layers 4\n  placement {{ spacing = {value} }}\n"),
            1,
        );
    }
    lower(&source)
}

fn with_spacing_mm(mm: f64) -> Board {
    let mut board = rtd_afe(None);
    board.placement_spacing_nm = Some(synth_geometry::mm_to_nm(mm));
    board
}

fn courtyards(board: &Board, placement: &Placement) -> Vec<(String, Option<String>, Rect)> {
    placement
        .components
        .iter()
        .map(|placed| {
            let component = board.component(placed.id).expect("component");
            let (w, h) = component
                .part
                .as_ref()
                .map_or(synth_place::fallback_courtyard(&component.kind), |part| {
                    synth_layout::pcb_courtyard_geometry_for_part(part).1
                });
            let (w, h) = if placed.rotation.swaps_extents() {
                (h, w)
            } else {
                (w, h)
            };
            let half = |mm: f64| synth_geometry::mm_to_nm(mm) / 2;
            (
                component.refdes.clone(),
                component.group.clone(),
                Rect::from_center_half_extents(placed.center, half(w), half(h)),
            )
        })
        .collect()
}

fn gap_mm(a: &Rect, b: &Rect) -> f64 {
    let gx = (b.min.x_nm - a.max.x_nm).max(a.min.x_nm - b.max.x_nm);
    let gy = (b.min.y_nm - a.max.y_nm).max(a.min.y_nm - b.max.y_nm);
    nm_to_mm(gx.max(gy))
}

#[test]
fn the_setting_is_parsed_and_lowered() {
    assert_eq!(rtd_afe(Some("2.0mm")).placement_spacing_nm, Some(2_000_000));
    assert_eq!(rtd_afe(Some("2.5mm")).placement_spacing_nm, Some(2_500_000));
    assert_eq!(rtd_afe(None).placement_spacing_nm, None);
}

/// A stable fingerprint of the serialized placement.
fn placement_fingerprint(placement: &Placement) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(placement)
        .expect("placement json")
        .hash(&mut hasher);
    hasher.finish()
}

/// Unset (no `placement` block) must produce the placement from before the
/// setting existed, not merely another unset board.
#[test]
fn unset_places_exactly_as_before() {
    let board = rtd_afe(None);
    let placement = synth_place::place(&board).expect("place");
    assert_eq!(
        placement_fingerprint(&placement),
        7_819_567_515_846_993_930,
        "unset placement changed; re-measure the pre-spacing golden"
    );
}

/// The label room ramps in below 2 mm instead of jumping a whole label height.
#[test]
fn the_gap_ramps_continuously_at_the_two_mm_knee() {
    let gap = |mm| synth_place::spacing_gap_mm(&with_spacing_mm(mm)).expect("spacing");
    assert!(
        (gap(2.0) - gap(1.99)).abs() < 0.1,
        "1.99 -> {:.3}, 2.0 -> {:.3}",
        gap(1.99),
        gap(2.0)
    );
    assert!((gap(2.0) - 3.3).abs() < 1e-9);
}

/// A unit value is converted, and a non-numeric spacing is rejected by
/// lowering without becoming a silent zero.
#[test]
fn mil_values_convert_and_non_finite_values_are_rejected() {
    assert_eq!(
        rtd_afe(Some("100mil")).placement_spacing_nm,
        Some(2_540_000)
    );
    let source = rtd_source().replacen(
        "  layers 4\n",
        "  layers 4\n  placement { spacing = nan }\n",
        1,
    );
    let parse = synth_parser::parse(&source, "spacing.synth");
    assert!(
        parse
            .diagnostics
            .iter()
            .any(|d| d.code == "E-SYNTH-PARSE-026"),
        "non-numeric spacing must be a parse error: {:?}",
        parse.diagnostics
    );
}

/// `groups` also checks the group-to-group gap.
fn check_spacing(label: &str, board: &Board, placement: &Placement, spacing_mm: f64, groups: bool) {
    let parts = courtyards(board, placement);
    let mut least = f64::MAX;
    for (i, (a, _, ra)) in parts.iter().enumerate() {
        for (b, _, rb) in &parts[i + 1..] {
            let gap = gap_mm(ra, rb);
            least = least.min(gap);
            assert!(
                gap >= spacing_mm - 0.05,
                "{label} at {spacing_mm} mm: {a} and {b} are {gap:.2} mm apart"
            );
        }
    }
    eprintln!("SPACING {label} {spacing_mm}: least part gap {least:.2} mm");
    if !groups {
        return;
    }
    let want = (spacing_mm + 1.0).max(2.0);
    let groups = synth_place::group_boxes(board, placement);
    let names: Vec<_> = groups.keys().cloned().collect();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            let gap = gap_mm(&groups[a], &groups[b]);
            assert!(
                gap >= want - 0.05,
                "{label} at {spacing_mm} mm: groups {a} and {b} are {gap:.2} mm apart, want {want:.2}"
            );
        }
    }
}

/// One spacing case. Split so nextest runs the four solves in parallel and
/// each stays under the 960 s terminate-after.
fn check_default_spacing(mm: f64) {
    let board = with_spacing_mm(mm);
    let placement = synth_place::place(&board).expect("place");
    check_spacing("default", &board, &placement, mm, true);
}

#[test]
fn default_path_keeps_every_pair_at_1_0_mm() {
    check_default_spacing(1.0);
}

#[test]
fn default_path_keeps_every_pair_at_2_5_mm() {
    check_default_spacing(2.5);
}

#[test]
fn default_path_keeps_every_pair_at_4_0_mm() {
    check_default_spacing(4.0);
}

#[test]
fn default_path_keeps_every_pair_at_5_0_mm() {
    check_default_spacing(5.0);
}

/// One RePlAce spacing case, split for the same per-test timeout as the
/// default path.
fn check_replace_spacing(mm: f64) {
    let board = with_spacing_mm(mm);
    let placement = synth_place::place_with_replace(&board).expect("place");
    check_spacing("replace", &board, &placement, mm, true);
}

#[test]
fn replace_path_keeps_every_pair_at_1_0_mm() {
    check_replace_spacing(1.0);
}

#[test]
fn replace_path_keeps_every_pair_at_2_5_mm() {
    check_replace_spacing(2.5);
}

#[test]
fn replace_path_keeps_every_pair_at_4_0_mm() {
    check_replace_spacing(4.0);
}

#[test]
fn replace_path_keeps_every_pair_at_5_0_mm() {
    check_replace_spacing(5.0);
}
