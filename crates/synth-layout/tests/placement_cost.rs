// SPDX-License-Identifier: Apache-2.0

//! `placement_cost` is the only schematic quality metric the placement
//! search can actually use: `score` needs a routed `Layout`, and the
//! placer runs long before a wire is drawn. These tests pin its
//! behaviour against real designs from `fixtures/layout/` rather than
//! hand-built IR, because the interesting behaviour is all in the
//! interaction with real pin electrical types and real net shapes.

use std::path::{Path, PathBuf};

use synth_layout::Layout;
use synth_layout::score::placement_cost;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn board_for(stem: &str) -> (synth_ir::Board, Layout) {
    let path = workspace_root()
        .join("fixtures")
        .join("layout")
        .join(format!("{stem}.synth"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let filename = format!("{stem}.synth");
    let parse = synth_parser::parse(&source, filename.clone());
    let ast = parse.ast.unwrap_or_else(|| panic!("{stem} must parse"));
    let registry =
        synth_registry::load_dir(&workspace_root().join("registry").join("parts")).unwrap();
    let board = synth_ir::lower(&ast, &registry, &filename)
        .board
        .unwrap_or_else(|| panic!("{stem} must lower"));
    let layout = synth_layout::layout(&board);
    (board, layout)
}

/// Move every component along `x` by `dx`, leaving `y` alone.
fn shifted(layout: &Layout, dx: f64) -> Vec<synth_layout::ComponentPlacement> {
    layout
        .components
        .iter()
        .map(|c| synth_layout::ComponentPlacement {
            id: c.id,
            center_mm: (c.center_mm.0 + dx, c.center_mm.1),
            rotation: c.rotation,
        })
        .collect()
}

/// The metric must be a pure function of the positions it is handed.
#[test]
fn cost_is_reproducible_across_repeated_calls() {
    for stem in ["divider", "led_indicator", "mcu", "multi_pattern_combo"] {
        let (board, layout) = board_for(stem);
        let first = placement_cost(&board, &layout.components);
        for _ in 0..16 {
            assert_eq!(
                placement_cost(&board, &layout.components),
                first,
                "{stem}: placement_cost is not reproducible"
            );
        }
    }
}

/// A uniform translation cannot change any *relative* distance, so the
/// cost is translation-invariant. If it were not, the metric would be
/// rewarding or punishing a drawing for where it sits on the page,
/// which is `E-SYNTH-SCHEM-012`'s job, not this one's.
#[test]
fn cost_is_translation_invariant() {
    for stem in ["divider", "led_indicator", "multi_pattern_combo"] {
        let (board, layout) = board_for(stem);
        let at_origin = placement_cost(&board, &layout.components);
        let moved = placement_cost(&board, &shifted(&layout, 137.0));
        assert!(
            (at_origin.net_span_mm - moved.net_span_mm).abs() < 1e-9
                && (at_origin.net_shear_mm - moved.net_shear_mm).abs() < 1e-9,
            "{stem}: translating every component changed the cost \
             ({} / {} -> {} / {})",
            at_origin.net_span_mm,
            at_origin.net_shear_mm,
            moved.net_span_mm,
            moved.net_shear_mm
        );
    }
}

/// A net drawn far apart is a long wire, and a long wire is worse than
/// a short one. Without this, `net_span_mm` could be measuring nothing.
#[test]
fn spreading_a_design_apart_costs_more() {
    for stem in ["divider", "led_indicator", "multi_pattern_combo"] {
        let (board, layout) = board_for(stem);
        let tight = placement_cost(&board, &layout.components);

        // Multiply the x spread by 3 by pushing everything right of the
        // centroid further away. Preserves y, so shear is untouched and
        // the assertion isolates the span term.
        // A mean over a small, bounded component count; the cast is
        // lossless in practice and this is a test-only centroid.
        #[allow(clippy::cast_precision_loss)]
        let cx = {
            let sum: f64 = layout.components.iter().map(|c| c.center_mm.0).sum();
            sum / layout.components.len() as f64
        };
        let spread: Vec<synth_layout::ComponentPlacement> = layout
            .components
            .iter()
            .map(|c| synth_layout::ComponentPlacement {
                id: c.id,
                center_mm: (cx + 3.0 * (c.center_mm.0 - cx), c.center_mm.1),
                rotation: c.rotation,
            })
            .collect();
        let wide = placement_cost(&board, &spread);

        assert!(
            wide.net_span_mm > tight.net_span_mm,
            "{stem}: tripling the x spread did not raise net_span_mm \
             ({} -> {})",
            tight.net_span_mm,
            wide.net_span_mm
        );
    }
}

/// The whole point of excluding rails: `decoupling` is one MCU, one
/// capacitor, and the two nets joining them are `vcc` and `gnd`. Both
/// render as power symbols rather than wires, so the design's drawing
/// is three symbols and some rail flags — and its wiring cost must be
/// zero. A metric that counted rails would report a large cost for a
/// trivially sparse drawing and rank it worse than a dense one.
#[test]
fn an_all_rail_design_costs_nothing() {
    let (board, layout) = board_for("decoupling");
    let cost = placement_cost(&board, &layout.components);
    // A net is a rail when *any* endpoint sits on a power pin — the cap's
    // own passive pins are on the same net as `U1.vcc`, so the
    // predicate is `any`, not `all`.
    assert!(
        !board.nets.is_empty()
            && board.nets.iter().all(|net| {
                net.endpoints.iter().any(|ep| {
                    board.pin(ep.component, ep.pin).is_some_and(|p| {
                        matches!(
                            p.electrical_type,
                            synth_registry::ElectricalType::PowerInput
                                | synth_registry::ElectricalType::PowerOutput
                                | synth_registry::ElectricalType::GroundReference
                        )
                    })
                })
            }),
        "decoupling is no longer an all-rail fixture; this test needs a new \
         subject"
    );
    assert_eq!(
        (cost.net_span_mm, cost.net_shear_mm),
        (0.0, 0.0),
        "rail nets must contribute nothing — they render as power symbols"
    );
}

/// Conversely, a real signal net must register. `divider`'s mid net
/// joins two resistors and is a signal, not a rail.
#[test]
fn a_signal_net_registers_even_in_a_power_fed_design() {
    let (board, layout) = board_for("divider");
    let cost = placement_cost(&board, &layout.components);
    assert!(
        cost.net_span_mm > 0.0 && cost.net_shear_mm > 0.0,
        "divider's mid net is a signal between two resistors, so both \
         terms must be non-zero; got {cost:?}"
    );
}

/// `total()` is what a search would minimise, and it must actually
/// respond to the shear term — otherwise `SHEAR_WEIGHT` is decorative.
#[test]
fn total_weights_shear_above_span() {
    let cost = synth_layout::score::PlacementCost {
        net_span_mm: 100.0,
        net_shear_mm: 0.0,
    };
    let equal_span_less_shear = synth_layout::score::PlacementCost {
        net_span_mm: 100.0,
        net_shear_mm: 10.0,
    };
    assert!(equal_span_less_shear.total() > cost.total());

    // And 10 mm of shear outweighs 19 mm of span, i.e. the weight is
    // meaningfully above 1 rather than a token multiplier.
    let less_span_more_shear = synth_layout::score::PlacementCost {
        net_span_mm: cost.net_span_mm - 19.0,
        net_shear_mm: 10.0,
    };
    assert!(less_span_more_shear.total() > cost.total());
}
