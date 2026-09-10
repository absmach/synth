// SPDX-License-Identifier: Apache-2.0

//! Snapshot tests for the layout output on real designs.
//!
//! Each snapshot records a compact summary of the full `Layout` for
//! a fixture — placements plus every routed artifact (wires,
//! junctions, power flags, net labels) and the sheet size — so
//! changes to both the placement and routing halves of the pipeline
//! are visible in code review.
//! Coordinate diffs are expected when algorithms change; structural
//! changes show up as added/removed/reordered lines, which is the
//! signal we actually want to see.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use synth_ir::{Board, ComponentId};
use synth_layout::{route::pin_terminal_xy, ComponentPlacement};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(fixture_rel_path: &str) -> Board {
    let path = workspace_root().join(fixture_rel_path);
    let source = std::fs::read_to_string(&path).unwrap();
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let ast = synth_parser::parse(&source, filename.clone()).ast.unwrap();
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    synth_ir::lower(&ast, &registry, &filename).board.unwrap()
}

/// Sort `(text, x, y, line)` entries by text then position, breaking
/// ties deterministically without ever consulting input order.
fn sort_by_text_then_position(items: &mut [(String, f64, f64, String)]) {
    items.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
}

/// Human-readable summary keyed by refdes/net name so the snapshot
/// doesn't drown in JSON. Every section is explicitly sorted — never
/// iterating a hash map or trusting construction order — and all
/// coordinates are rounded to 0.01 mm so f64 noise can't churn the
/// golden files between runs.
fn layout_summary(board: &Board, layout: &synth_layout::Layout) -> String {
    let placements: HashMap<ComponentId, &ComponentPlacement> =
        layout.components.iter().map(|p| (p.id, p)).collect();

    // Placements, sorted by refdes.
    let mut lines: Vec<String> = layout
        .components
        .iter()
        .map(|p| {
            let refdes = board
                .component(p.id)
                .map_or_else(|| format!("?{}", p.id.0), |c| c.refdes.clone());
            format!(
                "{:>4}  ({:>6.2}, {:>6.2})",
                refdes, p.center_mm.0, p.center_mm.1
            )
        })
        .collect();
    lines.sort();

    // One line per routed wire path, points rounded to 0.01 mm.
    let mut wire_lines: Vec<String> = layout
        .wires
        .iter()
        .map(|w| {
            let net = board
                .net(w.net)
                .map_or_else(|| format!("NET_{}", w.net.0), |n| n.name.clone());
            let points = w
                .points
                .iter()
                .map(|(x, y)| format!("({x:.2},{y:.2})"))
                .collect::<Vec<_>>()
                .join(" -> ");
            format!("wire {net} {points}")
        })
        .collect();
    wire_lines.sort();
    lines.extend(wire_lines);

    // Junction dots, sorted by (x, y).
    let mut junctions = layout.junctions.clone();
    junctions.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    lines.extend(
        junctions
            .iter()
            .map(|(x, y)| format!("junction ({x:.2},{y:.2})")),
    );

    // Power flags anchored at their pin terminals, sorted by label
    // then position.
    let mut flag_items: Vec<(String, f64, f64, String)> = layout
        .power_flags
        .iter()
        .filter_map(|f| {
            let (x, y) =
                pin_terminal_xy(board, f.component, f.pin, &placements).map(|t| (t.0, t.1))?;
            Some((
                f.label.clone(),
                x,
                y,
                format!("flag {:?} {} ({:.2},{:.2})", f.kind, f.label, x, y),
            ))
        })
        .collect();
    sort_by_text_then_position(&mut flag_items);
    lines.extend(flag_items.into_iter().map(|(_, _, _, line)| line));

    // Net labels anchored at their pin terminals, sorted by text
    // then position.
    let mut label_items: Vec<(String, f64, f64, String)> = layout
        .net_labels
        .iter()
        .filter_map(|l| {
            let (x, y) =
                pin_terminal_xy(board, l.component, l.pin, &placements).map(|t| (t.0, t.1))?;
            Some((
                l.label.clone(),
                x,
                y,
                format!("label {} ({:.2},{:.2})", l.label, x, y),
            ))
        })
        .collect();
    sort_by_text_then_position(&mut label_items);
    lines.extend(label_items.into_iter().map(|(_, _, _, line)| line));

    // Sheet size last.
    let (width, height) = layout.sheet_size.dims_mm();
    lines.push(format!("sheet {width:.2}x{height:.2}"));

    lines.join("\n")
}

#[test]
fn real_designs_layout_snapshots() {
    for (fixture, snapshot_name) in [
        ("examples/sensor_logger.synth", "sensor_logger_layout"),
        ("examples/env_logger.synth", "env_logger_layout"),
    ] {
        let board = board_for(fixture);
        let layout = synth_layout::layout(&board);

        // Page-fit guard: every placed component must sit inside the
        // declared sheet minus a 10 mm margin, so exports never
        // declare a page the drawing silently overflows.
        let (sheet_w, sheet_h) = layout.sheet_size.dims_mm();
        for p in &layout.components {
            assert!(
                p.center_mm.0 <= sheet_w - 10.0 && p.center_mm.1 <= sheet_h - 10.0,
                "{fixture}: {} at ({:.2},{:.2}) overflows declared sheet \
                 {sheet_w}x{sheet_h}",
                board
                    .component(p.id)
                    .map_or_else(|| format!("?{}", p.id.0), |c| c.refdes.clone()),
                p.center_mm.0,
                p.center_mm.1
            );
        }

        insta::assert_snapshot!(snapshot_name, layout_summary(&board, &layout));
    }
}

/// `sheet_size_for` escalation table (private fn — exercised through
/// the enum's public dims and the module's unit tests; this test
/// covers the observable contract end-to-end: a tiny design stays
/// A4).
#[test]
fn compact_designs_stay_on_a4() {
    let board = board_for("fixtures/ir/two_components_with_net.synth");
    let layout = synth_layout::layout(&board);
    assert_eq!(
        layout.sheet_size,
        synth_layout::SheetSize::A4,
        "a two-component design must keep the product-decision default"
    );
}
