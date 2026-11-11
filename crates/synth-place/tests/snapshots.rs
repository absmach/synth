// SPDX-License-Identifier: Apache-2.0

//! Bit-exact regression tests for `synth-place`. Snapshot is the
//! `Placement` of `examples/sensor_logger/sensor_logger.synth` rendered as a
//! short, human-readable summary. Updates land via
//! `cargo insta review` when the algorithm intentionally changes.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use synth_geometry::nm_to_mm;
use synth_ir::Board;

const FEATHER_M4_EXPRESS_BASELINE_OUTLINE_AREA_MM2_115_04_X_95_40: f64 = 115.04 * 95.40;
const OUTLINE_AREA_TOLERANCE: f64 = 0.10;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(rel: &str) -> Board {
    let path = workspace_root().join(rel);
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

fn render_placement_snapshot(board: &Board, placement: &synth_place::Placement) -> String {
    let mut body = String::new();
    writeln!(
        body,
        "board_outline_mm: {:.2} x {:.2}\n",
        nm_to_mm(placement.board_outline.width_nm()),
        nm_to_mm(placement.board_outline.height_nm()),
    )
    .unwrap();
    for p in &placement.components {
        let refdes = board
            .components
            .iter()
            .find(|c| c.id == p.id)
            .map_or_else(|| format!("#{}", p.id.0), |c| c.refdes.clone());
        writeln!(
            body,
            "{:>4}  ({:7.2}, {:7.2}) mm  rot={:?} layer={:?}",
            refdes,
            nm_to_mm(p.center.x_nm),
            nm_to_mm(p.center.y_nm),
            p.rotation,
            p.layer,
        )
        .unwrap();
    }
    body
}

#[test]
fn sensor_logger_placement_snapshot() {
    let board = load_board("examples/sensor_logger/sensor_logger.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("sensor_logger_placement", snapshot);
}

#[test]
fn secure_tracker_placement_snapshot() {
    let board = load_board("fixtures/designs/secure_tracker.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("secure_tracker_placement", snapshot);
}

#[test]
fn feather_m4_express_placement_snapshot() {
    let board = load_board("fixtures/designs/feather_m4_express.synth");
    let placement = synth_place::place(&board).expect("place");
    let snapshot = render_placement_snapshot(&board, &placement);
    insta::assert_snapshot!("feather_m4_express_placement", snapshot);
}

#[test]
fn feather_m4_express_outline_area_does_not_grow_past_baseline() {
    let board = load_board("fixtures/designs/feather_m4_express.synth");
    let placement = synth_place::place(&board).expect("place");
    let area = nm_to_mm(placement.board_outline.width_nm())
        * nm_to_mm(placement.board_outline.height_nm());
    let baseline = FEATHER_M4_EXPRESS_BASELINE_OUTLINE_AREA_MM2_115_04_X_95_40;
    // The guard is against growth. Global compaction now shrinks this board,
    // which is the point, so only an outline above the baseline fails.
    assert!(
        area <= baseline * (1.0 + OUTLINE_AREA_TOLERANCE),
        "outline area {area:.1} mm2 is more than 10% above baseline {baseline:.1} mm2"
    );
}

/// `secure_tracker`'s outline before the 25 MHz oscillator's courtyard was
/// The canvas fix in #158 re-places this board from 64.90 x 63.85 mm
/// (4143.9 mm2) to 62.25 x 45.19 mm (2813.1 mm2). That is the corrected
/// output, so the guard is re-baselined to it: a later canvas or part change
/// that silently inflates the outline still fails here, while the pre-fix
/// value no longer describes what the placer should produce.
const SECURE_TRACKER_BASELINE_OUTLINE_AREA_MM2: f64 = 62.25 * 45.19;

/// A 1 mm part correction must not move the board 44%.
///
/// Correcting one courtyard by 1 mm once took this board from 4143.9 mm2 to
/// 5960.9 mm2, and nothing failed on it: the insta snapshot records whatever
/// the placer does, so the non-monotonicity in #158 would have been baked in
/// silently. This bounds the corrected output against silent growth.
///
/// The sweep in #158 had the board oscillating between 64.90 and 93.90 mm
/// wide as one part grew in 0.2 mm steps. The cause was the canvas choice,
/// not the search: the square and wider canvases finished within a couple of
/// percent of each other in area while their widths differed by 18 mm, and
/// selecting the strictly smallest flipped the aspect when a part changed by
/// 0.2 mm. `choose_canvas` now requires a wider canvas to be 3% smaller
/// before it displaces the square family; the 0.2 mm sweep is monotonic,
/// 64.90 mm wide from 5.0 to 7.0 mm and 65.90 mm at 8.0. This guard is the
/// regression check on the corrected placement.
#[test]
fn secure_tracker_outline_area_stays_within_baseline_tolerance() {
    let board = load_board("fixtures/kicad-reference/secure_tracker.synth");
    let placement = synth_place::place(&board).expect("place");
    let area = nm_to_mm(placement.board_outline.width_nm())
        * nm_to_mm(placement.board_outline.height_nm());
    let baseline = SECURE_TRACKER_BASELINE_OUTLINE_AREA_MM2;
    assert!(
        (area - baseline).abs() <= baseline * OUTLINE_AREA_TOLERANCE,
        "outline area {area:.1} mm2 deviates more than 10% from baseline \
         {baseline:.1} mm2 — see #158"
    );
}

/// The external placement contract publishes the footprint origin (KiCad
/// `at`), not the internal courtyard centre. For an asymmetric footprint the
/// two differ by the courtyard offset; publishing the internal value shifts
/// every downstream consumer (notably synth-ee) relative to the exported PCB.
#[test]
fn external_placement_publishes_footprint_origin_not_courtyard_centre() {
    let board = load_board("examples/sensor_logger/sensor_logger.synth");
    let placement = synth_place::place(&board).expect("place");
    let external = synth_place::to_external(&board, &placement);

    let mut shifted = Vec::new();
    for internal in &placement.components {
        let part = board
            .component(internal.id)
            .and_then(|component| component.part.as_ref());
        let origin = synth_place::footprint_origin(part, internal);
        let published = external
            .components
            .iter()
            .find(|component| component.id == internal.id)
            .expect("every internal placement is published");
        assert_eq!(published.center, origin);
        if published.center != internal.center {
            shifted.push(internal.id);
        }
    }

    // J1 is the Micro-USB receptacle: its courtyard is offset from the
    // footprint origin, so this design must exercise the conversion. If this
    // asserts, the footprint library did not resolve and the test is vacuous.
    let usb = board
        .components
        .iter()
        .find(|component| component.refdes == "J1")
        .expect("Micro-USB component");
    assert!(
        shifted.contains(&usb.id),
        "expected J1's courtyard centre to differ from its footprint origin; \
         are the KiCad footprint libraries installed?"
    );
}

#[test]
fn usb_c_orientation_is_derived_and_wrong_override_is_detected() {
    let board = load_board("fixtures/designs/iot_sensor_board.synth");
    let placement = synth_place::place(&board).expect("place");
    let usb = board
        .components
        .iter()
        .find(|component| component.refdes == "J1")
        .expect("USB-C component");
    let usb_placement = placement
        .components
        .iter()
        .find(|component| component.id == usb.id)
        .expect("USB-C placement");
    assert_eq!(usb_placement.rotation, synth_geometry::Rotation::OneEighty);
    assert!(synth_place::floorplan::connector_orientation_issues(
        &board,
        placement.board_outline,
        &placement.components,
    )
    .is_empty());

    let mut incorrect = placement.clone();
    incorrect
        .components
        .iter_mut()
        .find(|component| component.id == usb.id)
        .expect("USB-C placement")
        .rotation = synth_geometry::Rotation::Zero;
    let issues = synth_place::floorplan::connector_orientation_issues(
        &board,
        incorrect.board_outline,
        &incorrect.components,
    );
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].refdes, "J1");
    assert_eq!(issues[0].expected, synth_geometry::Rotation::OneEighty);
}

/// A connector whose hard `edge:` hint names its edge is checked against that
/// edge wherever it sits. A pinned placement that leaves it far from the edge
/// with its opening turned inward is reported, not skipped as an interior part.
#[test]
fn a_connector_with_an_edge_hint_is_checked_however_far_it_sits_from_the_edge() {
    use synth_place::floorplan::{rotation_for_mating_edge, BoardEdge};

    let board = load_board("fixtures/designs/edge_jst_connectors.synth");
    let placement = synth_place::place(&board).expect("place");
    let left = board
        .components
        .iter()
        .find(|component| component.refdes == "J1")
        .expect("J1 asks for the left edge");
    let face = left
        .part
        .as_ref()
        .and_then(|part| part.footprint_dimensions.as_ref())
        .and_then(|dimensions| dimensions.mating_face)
        .expect("J1 declares its mating face");
    let mut pinned = placement.clone();
    let part = pinned
        .components
        .iter_mut()
        .find(|component| component.id == left.id)
        .expect("J1 placement");
    // Opening toward the right edge, and far from the left one.
    part.rotation = rotation_for_mating_edge(face, BoardEdge::Right);
    part.center = synth_geometry::Point::new(
        pinned.board_outline.min.x_nm + synth_geometry::mm_to_nm(25.0),
        part.center.y_nm,
    );
    let issues = synth_place::floorplan::connector_orientation_issues(
        &board,
        pinned.board_outline,
        &pinned.components,
    );
    assert!(
        issues.iter().any(|issue| issue.refdes == "J1"),
        "{issues:?}"
    );
}

/// A JST connector's plug opens outward on every edge, not only on the one the
/// floorplan used to assume. Each connector here asks for a different edge; the
/// rotation that points its declared mating face out of that edge is derived
/// from the part, and the orientation check agrees.
#[test]
fn jst_connectors_face_outward_on_every_edge() {
    use synth_geometry::Rotation;
    use synth_place::floorplan::{rotation_for_mating_edge, BoardEdge};
    use synth_registry::MatingFace;

    let board = load_board("fixtures/designs/edge_jst_connectors.synth");
    let placement = synth_place::place(&board).expect("place");

    let asked = [
        ("J1", BoardEdge::Left),
        ("J2", BoardEdge::Right),
        ("J3", BoardEdge::Bottom),
        ("J4", BoardEdge::Top),
    ];
    for (refdes, edge) in asked {
        let component = board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .expect("connector");
        let face = component
            .part
            .as_ref()
            .and_then(|part| part.footprint_dimensions.as_ref())
            .and_then(|dimensions| dimensions.mating_face);
        assert_eq!(
            face,
            Some(MatingFace::Bottom),
            "{refdes}'s part must declare which way its plug opens"
        );
        let placed = placement
            .components
            .iter()
            .find(|c| c.id == component.id)
            .expect("placement");
        let expected: Rotation = rotation_for_mating_edge(MatingFace::Bottom, edge);
        assert_eq!(
            placed.rotation, expected,
            "{refdes} asked for the {edge:?} edge and must open outward"
        );
    }

    let issues = synth_place::floorplan::connector_orientation_issues(
        &board,
        placement.board_outline,
        &placement.components,
    );
    assert!(
        issues.is_empty(),
        "an edge-mounted connector faces into the board: {issues:?}\noutline {:?}\n{:?}",
        placement.board_outline,
        placement
            .components
            .iter()
            .filter(|c| board
                .component(c.id)
                .is_some_and(|k| k.refdes.starts_with('J')))
            .map(|c| (
                board.component(c.id).map(|k| k.refdes.clone()),
                c.center,
                c.rotation
            ))
            .collect::<Vec<_>>()
    );
}

const VERTICAL_JST_XH: [&str; 2] = ["jst_b2b_xh_a_2pin", "jst_b4b_xh_a_4pin"];

/// A connector part that goes on a board edge says which way its plug opens, or
/// the placer has nothing to orient it by and the DRC cannot check it. Vertical
/// pin headers are the exception: their opening faces up, not out of an edge.
/// The two top-entry JST XH headers are vertical too, so they are listed by id.
#[test]
fn every_edge_connector_declares_which_way_its_plug_opens() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads");
    let mut missing = Vec::new();
    let mut checked = 0;
    for (_, part) in registry.iter() {
        if !matches!(part.kind.as_str(), "connector" | "jack")
            || part.id.0.starts_with("header_")
            || VERTICAL_JST_XH.contains(&part.id.0.as_str())
        {
            continue;
        }
        checked += 1;
        let face = part
            .footprint_dimensions
            .as_ref()
            .and_then(|dimensions| dimensions.mating_face);
        if face.is_none() {
            missing.push(part.id.0.clone());
        }
    }
    assert!(checked >= 8, "found only {checked} edge connectors");
    assert!(
        missing.is_empty(),
        "edge connectors with no mating_face: {missing:?}"
    );
}
