// SPDX-License-Identifier: Apache-2.0

use synth_diagnostics::Span;
use synth_geometry::{Point, Rect};
use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, PinId};
use synth_place::cem::cem_region_assign;

fn create_test_board() -> Board {
    let dummy_span = Span::new(0, 0);

    let c1 = Component {
        id: ComponentId(1),
        refdes: "U1".to_string(),
        kind: "mcu".to_string(),
        part: None,
        value: Some("RP2350".to_string()),
        placement_hint: None,
        group: None,
        source_span: dummy_span,
    };
    let c2 = Component {
        id: ComponentId(2),
        refdes: "J1".to_string(),
        kind: "connector".to_string(),
        part: None,
        value: Some("USB-C".to_string()),
        placement_hint: None,
        group: None,
        source_span: dummy_span,
    };
    let c3 = Component {
        id: ComponentId(3),
        refdes: "U2".to_string(),
        kind: "regulator".to_string(),
        part: None,
        value: Some("LDO".to_string()),
        placement_hint: None,
        group: None,
        source_span: dummy_span,
    };

    let net1 = Net {
        id: synth_ir::NetId(1),
        name: "VBUS".to_string(),
        endpoints: vec![
            NetEndpoint {
                component: ComponentId(2),
                pin: PinId(1),
                source_span: dummy_span,
            },
            NetEndpoint {
                component: ComponentId(3),
                pin: PinId(1),
                source_span: dummy_span,
            },
        ],
    };

    Board {
        name: "cem_test_board".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components: vec![c1, c2, c3],
        nets: vec![net1],
        diff_pairs: vec![],
        keepouts: vec![],
        source_span: dummy_span,
    }
}

#[test]
fn test_cem_assigns_hints_within_usable_bounds() {
    let board = create_test_board();
    let usable = Rect::new(
        Point::new(5_000_000, 5_000_000),
        Point::new(95_000_000, 75_000_000),
    );
    let assign = cem_region_assign(&board, usable);

    assert_eq!(assign.hints.len(), 3);
    for (&id, &pt) in &assign.hints {
        assert!(
            pt.x_nm >= usable.min.x_nm && pt.x_nm <= usable.max.x_nm,
            "x out of bounds for {id:?}"
        );
        assert!(
            pt.y_nm >= usable.min.y_nm && pt.y_nm <= usable.max.y_nm,
            "y out of bounds for {id:?}"
        );
    }
}

#[test]
fn test_cem_determinism() {
    let board = create_test_board();
    let usable = Rect::new(
        Point::new(5_000_000, 5_000_000),
        Point::new(95_000_000, 75_000_000),
    );

    let assign1 = cem_region_assign(&board, usable);
    let assign2 = cem_region_assign(&board, usable);

    assert_eq!(assign1, assign2);
}
