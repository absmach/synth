// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `synth_layout::ops` (§7.8.8 Stage E) against
//! a real, lowered board — not hand-built `Layout` fixtures — since
//! `apply_op`'s whole point is to be safe to call on the actual
//! output of `synth_layout::layout`.

use std::path::{Path, PathBuf};

use synth_ir::Board;
use synth_layout::ops::{apply_op, LayoutOp, LayoutOpError};
use synth_layout::Rotation;

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

#[test]
fn move_component_updates_position_and_rereroutes() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let id = layout.components[0].id;

    apply_op(
        &mut layout,
        &board,
        LayoutOp::MoveComponent {
            id,
            x_mm: 99.06, // 39 * 2.54mm — already grid-aligned
            y_mm: 50.8,  // 20 * 2.54mm — already grid-aligned
        },
    )
    .unwrap();

    let moved = layout.placement(id).unwrap();
    assert_eq!(moved.center_mm, (99.06, 50.8));

    // Re-routing after the move must still produce a well-formed,
    // total layout — every component still placed exactly once.
    assert_eq!(layout.components.len(), board.components.len());
}

#[test]
fn move_component_snaps_to_grid() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let id = layout.components[0].id;

    apply_op(
        &mut layout,
        &board,
        LayoutOp::MoveComponent {
            id,
            x_mm: 101.3,
            y_mm: 50.1,
        },
    )
    .unwrap();

    let moved = layout.placement(id).unwrap();
    // snap_grid rounds to the nearest 2.54mm multiple.
    assert!((moved.center_mm.0 / 2.54).fract().abs() < 1e-9);
    assert!((moved.center_mm.1 / 2.54).fract().abs() < 1e-9);
}

#[test]
fn move_component_unknown_id_is_an_error_and_does_not_mutate() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.clone();
    let bogus = synth_ir::ComponentId(9999);

    let result = apply_op(
        &mut layout,
        &board,
        LayoutOp::MoveComponent {
            id: bogus,
            x_mm: 0.0,
            y_mm: 0.0,
        },
    );

    assert_eq!(result, Err(LayoutOpError::UnknownComponent(bogus)));
    assert_eq!(layout, before, "layout must be untouched on error");
}

#[test]
fn rotate_updates_rotation() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let id = layout.components[0].id;
    let original_rotation = layout.placement(id).unwrap().rotation;
    let new_rotation = if original_rotation == Rotation::Zero {
        Rotation::Ninety
    } else {
        Rotation::Zero
    };

    apply_op(
        &mut layout,
        &board,
        LayoutOp::Rotate {
            id,
            rotation: new_rotation,
        },
    )
    .unwrap();

    assert_eq!(layout.placement(id).unwrap().rotation, new_rotation);
}

#[test]
fn group_block_arranges_members_in_a_column_beside_anchor() {
    let board = board_for("fixtures/layout/mcu.synth");
    let mut layout = synth_layout::layout(&board);

    // Pick an anchor and every other component as the group — the
    // exact semantic pattern doesn't matter for this test, only that
    // the op runs and produces a deterministic, well-formed column.
    let anchor = layout.components[0].id;
    let ids: Vec<_> = layout.components[1..].iter().map(|p| p.id).collect();
    assert!(!ids.is_empty(), "fixture needs at least 2 components");

    let anchor_before = layout.placement(anchor).unwrap().center_mm;

    apply_op(
        &mut layout,
        &board,
        LayoutOp::GroupBlock {
            ids: ids.clone(),
            anchor,
        },
    )
    .unwrap();

    // Anchor itself does not move.
    assert_eq!(layout.placement(anchor).unwrap().center_mm, anchor_before);

    // Every grouped member sits at the same x (a vertical column) to
    // the right of the anchor, each at a distinct y.
    let col_x = layout.placement(ids[0]).unwrap().center_mm.0;
    assert!(col_x > anchor_before.0, "column must sit right of anchor");
    let mut ys = Vec::new();
    for &id in &ids {
        let (x, y) = layout.placement(id).unwrap().center_mm;
        assert_eq!(x, col_x, "every grouped member shares the column x");
        ys.push(y);
    }
    let mut sorted_ys = ys.clone();
    sorted_ys.sort_by(f64::total_cmp);
    sorted_ys.dedup();
    assert_eq!(
        sorted_ys.len(),
        ys.len(),
        "grouped members must not overlap"
    );
}

#[test]
fn group_block_empty_ids_is_an_error() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let anchor = layout.components[0].id;

    let result = apply_op(
        &mut layout,
        &board,
        LayoutOp::GroupBlock {
            ids: Vec::new(),
            anchor,
        },
    );

    assert_eq!(result, Err(LayoutOpError::EmptyGroup));
}

/// Find a net in `layout.wires` that currently renders as a real
/// wire (not a power flag or an already-automatic label) — the
/// scenario `ReplaceWireWithLabel`/`RerouteNet` are meant to act on.
fn a_wired_net(layout: &synth_layout::Layout) -> synth_ir::NetId {
    layout
        .wires
        .first()
        .unwrap_or_else(|| panic!("fixture must have at least one routed wire to test against"))
        .net
}

#[test]
fn replace_wire_with_label_removes_the_wire_and_adds_endpoint_labels() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let net = a_wired_net(&layout);
    let net_ir = board.net(net).unwrap();

    apply_op(&mut layout, &board, LayoutOp::ReplaceWireWithLabel { net }).unwrap();

    assert!(
        !layout.wires.iter().any(|w| w.net == net),
        "wire for the forced net must be gone"
    );
    let label_count = layout.net_labels.iter().filter(|l| l.net == net).count();
    assert_eq!(
        label_count,
        net_ir.endpoints.len(),
        "one label per net endpoint"
    );
}

#[test]
fn replace_wire_with_label_unknown_net_is_an_error() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.clone();
    let bogus = synth_ir::NetId(9999);

    let result = apply_op(
        &mut layout,
        &board,
        LayoutOp::ReplaceWireWithLabel { net: bogus },
    );

    assert_eq!(result, Err(LayoutOpError::UnknownNet(bogus)));
    assert_eq!(layout, before);
}

#[test]
fn reroute_net_produces_a_well_formed_layout() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let net = a_wired_net(&layout);

    apply_op(&mut layout, &board, LayoutOp::RerouteNet { net }).unwrap();

    // Board connectivity must be untouched by a layout op.
    assert_eq!(layout.components.len(), board.components.len());
    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            let horiz = (p1.1 - p2.1).abs() < 1e-6;
            let vert = (p1.0 - p2.0).abs() < 1e-6;
            assert!(horiz || vert, "wire segments must stay orthogonal");
        }
    }
}

#[test]
fn reroute_net_unknown_net_is_an_error() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.clone();
    let bogus = synth_ir::NetId(9999);

    let result = apply_op(&mut layout, &board, LayoutOp::RerouteNet { net: bogus });

    assert_eq!(result, Err(LayoutOpError::UnknownNet(bogus)));
    assert_eq!(layout, before);
}

/// Locks in the JSON wire shape `synth_mutate_layout` (MCP) and
/// `/api/v1/layout/save` (browser) both depend on: a `kind`-tagged
/// object with snake_case field names, matching what an agent or the
/// browser would actually send.
#[test]
fn layout_op_json_shape_is_kind_tagged_snake_case() {
    let op = LayoutOp::MoveComponent {
        id: synth_ir::ComponentId(3),
        x_mm: 50.8,
        y_mm: 25.4,
    };
    let json = serde_json::to_value(&op).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"kind": "move_component", "id": 3, "x_mm": 50.8, "y_mm": 25.4})
    );

    let parsed: LayoutOp = serde_json::from_value(serde_json::json!({
        "kind": "rotate",
        "id": 7,
        "rotation": "Ninety"
    }))
    .unwrap();
    assert_eq!(
        parsed,
        LayoutOp::Rotate {
            id: synth_ir::ComponentId(7),
            rotation: Rotation::Ninety,
        }
    );
}

#[test]
fn new_repair_ops_json_shape_is_kind_tagged_snake_case() {
    let row = serde_json::json!({
        "kind": "distribute_row", "ids": [1, 2, 3], "y_mm": 50.8, "x_mm": 25.4
    });
    let parsed: LayoutOp = serde_json::from_value(row).unwrap();
    assert_eq!(
        parsed,
        LayoutOp::DistributeRow {
            ids: vec![
                synth_ir::ComponentId(1),
                synth_ir::ComponentId(2),
                synth_ir::ComponentId(3),
            ],
            y_mm: Some(50.8),
            x_mm: Some(25.4),
        }
    );

    let spread = serde_json::json!({ "kind": "spread_region", "ids": [], "scale": 1.4 });
    let parsed: LayoutOp = serde_json::from_value(spread).unwrap();
    assert_eq!(
        parsed,
        LayoutOp::SpreadRegion {
            ids: Vec::new(),
            scale: 1.4,
        }
    );
}

#[test]
fn distribute_row_lays_components_left_to_right_at_distinct_x() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let ids: Vec<_> = layout.components.iter().map(|p| p.id).collect();

    apply_op(
        &mut layout,
        &board,
        LayoutOp::DistributeRow {
            ids: ids.clone(),
            y_mm: Some(50.8),
            x_mm: Some(25.4),
        },
    )
    .unwrap();

    // Every member shares the row y...
    for &id in &ids {
        let (_, y) = layout.placement(id).unwrap().center_mm;
        assert_eq!(y, 50.8, "row members must share one y");
    }
    // ...and x strictly increases in the given order, with no overlap.
    let xs: Vec<f64> = ids
        .iter()
        .map(|&id| layout.placement(id).unwrap().center_mm.0)
        .collect();
    for w in xs.windows(2) {
        assert!(w[1] > w[0], "row must advance left-to-right: {xs:?}");
    }
    assert!(
        xs[0] >= 25.4 - 1e-9,
        "row must start at or right of the requested left edge"
    );
}

#[test]
fn distribute_row_defaults_keep_current_row_position() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let ids: Vec<_> = layout.components.iter().map(|p| p.id).collect();

    apply_op(
        &mut layout,
        &board,
        LayoutOp::DistributeRow {
            ids: ids.clone(),
            y_mm: None,
            x_mm: None,
        },
    )
    .unwrap();

    // Still a valid, fully-placed layout.
    assert_eq!(layout.components.len(), board.components.len());
    for wire in &layout.wires {
        for pair in wire.points.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            let horiz = (p1.1 - p2.1).abs() < 1e-6;
            let vert = (p1.0 - p2.0).abs() < 1e-6;
            assert!(horiz || vert, "wire segments must stay orthogonal");
        }
    }
}

#[test]
fn distribute_row_empty_ids_is_an_error() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let result = apply_op(
        &mut layout,
        &board,
        LayoutOp::DistributeRow {
            ids: Vec::new(),
            y_mm: None,
            x_mm: None,
        },
    );
    assert_eq!(result, Err(LayoutOpError::EmptyGroup));
}

#[test]
fn distribute_row_unknown_id_is_an_error_and_does_not_mutate() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.clone();
    let bogus = synth_ir::ComponentId(9999);
    let first = layout.components[0].id;

    let result = apply_op(
        &mut layout,
        &board,
        LayoutOp::DistributeRow {
            ids: vec![first, bogus],
            y_mm: None,
            x_mm: None,
        },
    );

    assert_eq!(result, Err(LayoutOpError::UnknownComponent(bogus)));
    assert_eq!(layout, before, "layout must be untouched on error");
}

#[test]
fn spread_region_increases_content_bounds_and_stays_grid_aligned() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);

    let before = synth_layout::content_bounds(&board, &layout).expect("content bounds");
    let width_before = before.1 - before.0;
    let height_before = before.3 - before.2;

    apply_op(
        &mut layout,
        &board,
        LayoutOp::SpreadRegion {
            ids: Vec::new(),
            scale: 1.4,
        },
    )
    .unwrap();

    let after = synth_layout::content_bounds(&board, &layout).expect("content bounds");
    let width_after = after.1 - after.0;
    let height_after = after.3 - after.2;

    assert!(
        width_after > width_before || height_after > height_before,
        "spread must enlarge the content box: before {width_before:.1}×{height_before:.1}, \
         after {width_after:.1}×{height_after:.1}"
    );
    for placement in &layout.components {
        assert!(
            (placement.center_mm.0 / 2.54).fract().abs() < 1e-9,
            "spread must snap x to the pin grid: {:?}",
            placement.center_mm
        );
    }
}

#[test]
fn spread_region_rejects_non_positive_and_non_finite_scale() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.clone();

    for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        let result = apply_op(
            &mut layout,
            &board,
            LayoutOp::SpreadRegion {
                ids: Vec::new(),
                scale: bad,
            },
        );
        assert!(
            matches!(result, Err(LayoutOpError::InvalidScale(_))),
            "scale {bad} must be rejected"
        );
        assert_eq!(layout, before, "layout must be untouched on error");
    }
}

#[test]
fn fit_sheet_shrinks_a_small_board_below_a4() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    let before = layout.sheet_size.dims_mm();

    apply_op(&mut layout, &board, LayoutOp::FitSheet { grow: false }).unwrap();

    let after = layout.sheet_size.dims_mm();
    assert!(
        after.0 < before.0 || after.1 < before.1,
        "fit_sheet must shrink a small design below {before:?}, got {after:?}"
    );
    // And the fit must actually contain the content.
    let (min_x, max_x, min_y, max_y) =
        synth_layout::content_bounds(&board, &layout).expect("content bounds");
    assert!(
        max_x <= after.0,
        "content must fit page width: {max_x} > {}",
        after.0
    );
    assert!(
        max_y <= after.1,
        "content must fit page height: {max_y} > {}",
        after.1
    );
    let _ = (min_x, min_y);
}

#[test]
fn fit_sheet_never_grows_without_the_grow_flag() {
    let board = board_for("fixtures/layout/led_indicator.synth");
    let mut layout = synth_layout::layout(&board);
    // Shrink first, then confirm a plain fit does not re-grow.
    apply_op(&mut layout, &board, LayoutOp::FitSheet { grow: false }).unwrap();
    let shrunk = layout.sheet_size.dims_mm();

    apply_op(&mut layout, &board, LayoutOp::FitSheet { grow: false }).unwrap();
    assert_eq!(
        layout.sheet_size.dims_mm(),
        shrunk,
        "fit must be idempotent"
    );
}

#[test]
fn fit_sheet_size_any_returns_a_smaller_custom_page_for_tiny_content() {
    // A 20x15mm blob of content on the standard ladder floors at A4.
    let standard = synth_layout::fit_sheet_size(10.0, 30.0, 10.0, 25.0);
    assert!(matches!(standard, synth_layout::SheetSize::A4));

    let any = synth_layout::fit_sheet_size_any(10.0, 30.0, 10.0, 25.0);
    match any {
        synth_layout::SheetSize::Custom {
            width_mm,
            height_mm,
        } => {
            assert!(width_mm < 297.0 && height_mm < 210.0);
            assert!(
                width_mm >= 30.0 && height_mm >= 25.0,
                "must contain content"
            );
        }
        other => panic!("expected a custom page below A4, got {other:?}"),
    }
}

#[test]
fn fit_sheet_size_any_keeps_standard_ladder_for_large_content() {
    // Content needing more than A4 must stay on the standard ladder.
    let any = synth_layout::fit_sheet_size_any(0.0, 900.0, 0.0, 700.0);
    assert!(
        !matches!(any, synth_layout::SheetSize::Custom { .. }),
        "large content must not get a custom page, got {any:?}"
    );
}
