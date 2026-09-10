// SPDX-License-Identifier: Apache-2.0

//! Property tests for the canonical schematic router
//! (`synth_layout::route::route_board`, exercised via
//! `synth_layout::layout`).
//!
//! Invariants the router MUST satisfy on every fixture:
//!
//! - **Orthogonal.** Every `WirePath.points` segment is horizontal or
//!   vertical, never diagonal.
//! - **No body crossings.** No wire segment passes through a
//!   component body (mirrors `sensor_logger_no_body_crossing_wires`
//!   in `synth-kicad` at the layout level).
//! - **Deterministic.** Identical input produces identical routing
//!   output across runs.

use std::path::{Path, PathBuf};

use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(fixture_rel_path: &str) -> Board {
    let path = workspace_root().join(fixture_rel_path);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse.ast.expect("fixture must parse");
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let lowered = synth_ir::lower(&ast, &registry, &filename);
    lowered.board.expect("fixture must lower")
}

/// Every consecutive point pair in every `WirePath` must differ in
/// exactly one axis (orthogonal routing) by a nonzero grid amount.
fn assert_all_wires_orthogonal(layout: &synth_layout::Layout) {
    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            let (x1, y1) = pair[0];
            let (x2, y2) = pair[1];
            let dx = (x2 - x1).abs();
            let dy = (y2 - y1).abs();
            assert!(
                (dx > 1e-6 && dy < 1e-6) || (dy > 1e-6 && dx < 1e-6),
                "non-orthogonal wire segment on net {:?}: ({x1}, {y1}) -> ({x2}, {y2})",
                wire.net
            );
        }
    }
}

/// Whether an axis-aligned segment `(p1, p2)` passes through the
/// body rect of `component` (with a small margin), using the same
/// body geometry as the router itself.
fn segment_crosses_body(
    p1: (f64, f64),
    p2: (f64, f64),
    component: &synth_ir::Component,
    placement: &synth_layout::ComponentPlacement,
) -> bool {
    let Some(part) = component.part.as_ref() else {
        return false;
    };
    let (cx, cy) = placement.center_mm;
    let (bw, bh) = synth_layout::body_size_for_part(part);
    let bx = cx - bw / 2.0;
    let by = cy - bh / 2.0;

    let margin = 1.0;
    let lo_x = bx - margin;
    let hi_x = bx + bw + margin;
    let lo_y = by - margin;
    let hi_y = by + bh + margin;

    let x_min = p1.0.min(p2.0);
    let x_max = p1.0.max(p2.0);
    let y_min = p1.1.min(p2.1);
    let y_max = p1.1.max(p2.1);

    if (p1.1 - p2.1).abs() < 0.01 {
        let y = p1.1;
        y > lo_y && y < hi_y && x_max > lo_x && x_min < hi_x
    } else if (p1.0 - p2.0).abs() < 0.01 {
        let x = p1.0;
        x > lo_x && x < hi_x && y_max > lo_y && y_min < hi_y
    } else {
        false
    }
}

/// Which component ids a net's endpoints touch, so the body-crossing
/// check can exempt a wire from crossing the very parts it connects
/// to (a wire must reach its own pins, so the terminal stub ends on
/// the connected component's body boundary — that's expected, not a
/// violation).
fn net_endpoint_ids(board: &Board, net_id: synth_ir::NetId) -> Vec<synth_ir::ComponentId> {
    let Some(net) = board.net(net_id) else {
        return Vec::new();
    };
    let mut ids: Vec<synth_ir::ComponentId> = net.endpoints.iter().map(|ep| ep.component).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// No wire segment may pass through the body of a component the
/// wire's net does **not** connect to. Segments reaching the wire's
/// own endpoint components are fine (they end at that body's pin
/// terminal on the boundary, which the router's corridor punching
/// deliberately aims at).
fn assert_no_wires_cross_unrelated_bodies(board: &Board, layout: &synth_layout::Layout) {
    for wire in &layout.wires {
        let own_ids = net_endpoint_ids(board, wire.net);
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            for component in &board.components {
                if own_ids.contains(&component.id) {
                    continue; // this wire is allowed to touch this part
                }
                let Some(placement) = layout.placement(component.id) else {
                    continue;
                };
                assert!(
                    !segment_crosses_body(p1, p2, component, placement),
                    "wire on net {:?} crosses unrelated body of {}: ({:?}) -> ({:?})",
                    wire.net,
                    component.refdes,
                    p1,
                    p2
                );
            }
        }
    }
}

#[test]
fn sensor_logger_wires_are_orthogonal() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    assert_all_wires_orthogonal(&layout);
}

#[test]
fn sensor_logger_wires_do_not_cross_component_bodies() {
    let board = board_for("examples/sensor_logger.synth");
    let layout = synth_layout::layout(&board);
    assert_no_wires_cross_unrelated_bodies(&board, &layout);
}

#[test]
fn sensor_logger_routing_is_deterministic() {
    let board = board_for("examples/sensor_logger.synth");
    let a = synth_layout::layout(&board);
    let b = synth_layout::layout(&board);
    assert_eq!(a.wires, b.wires, "routing is not deterministic");
    assert_eq!(
        a.junctions, b.junctions,
        "junction dots are not deterministic"
    );
}

/// Every signal net must terminate: ≥1 wire, or ≥2 per-endpoint net
/// labels (the truncate-to-labels escape hatch), or power-flag
/// handling. The silent-drop regression this guards against counted a
/// net as "routed" while producing neither wires nor labels —
/// connectivity silently vanished from the schematic.
#[test]
fn every_signal_net_terminates_in_wires_or_labels() {
    let mut paths: Vec<PathBuf> = vec![workspace_root().join("examples/sensor_logger.synth")];
    let dir = workspace_root().join("fixtures").join("layout");
    paths.extend(
        std::fs::read_dir(&dir)
            .expect("fixtures/layout must exist")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "synth")),
    );

    for path in paths {
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .to_string();
        let board = board_for(&rel);
        let layout = synth_layout::layout(&board);
        let power_nets = layout.power_net_ids();
        let labeled_nets = layout.labeled_net_ids();
        for net in &board.nets {
            if power_nets.contains(&net.id)
                || labeled_nets.contains(&net.id)
                || net.endpoints.len() < 2
            {
                continue;
            }
            let wire_count = layout.wires.iter().filter(|w| w.net == net.id).count();
            let label_count = layout.net_labels.iter().filter(|l| l.net == net.id).count();
            assert!(
                wire_count >= 1 || label_count >= 2,
                "{rel}: net {} terminated with {wire_count} wires and {label_count} labels",
                net.id.0
            );
        }
    }
}

#[test]
fn fixture_wires_are_orthogonal_and_clear_of_bodies() {
    // Sweep every corpus fixture through the router: the layout
    // corpus covers one design per recognized motif, so this guards
    // the router against motif-specific regressions (e.g. USB ESD
    // diodes forcing a diagonal, LED chains tangling).
    let dir = workspace_root().join("fixtures").join("layout");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures/layout must exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "synth"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "expected fixtures/layout to be populated"
    );

    for path in paths {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .to_string();
        let board = board_for(&rel);
        let layout = synth_layout::layout(&board);
        assert_all_wires_orthogonal(&layout);
        assert_no_wires_cross_unrelated_bodies(&board, &layout);
        let _ = stem;
    }
}
