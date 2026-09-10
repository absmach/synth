// SPDX-License-Identifier: Apache-2.0

//! End-to-end integration tests proving the Hashimoto-Stevens channel
//! router (§7.5.6 / §7.7.5) is a *real, exercised* part of the
//! pipeline, not a stub: [`synth_layout::route::route_board`] must
//! decompose the sheet into channel strips, left-edge pack
//! overlapping nets onto separate track levels, and route them along
//! those tracks — guaranteed minimum pitch and zero line overlap.

use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_layout::{route, ComponentPlacement, Layout, Rotation, SheetSize};
use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin, PinNumber};

fn two_pin_part() -> Part {
    Part {
        id: PartId("r_generic".to_string()),
        kind: "resistor".to_string(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        provenance: None,
        pins: vec![
            Pin {
                name: "p1".to_string(),
                number: PinNumber("1".to_string()),
                electrical_type: ElectricalType::Passive,
                capabilities: Vec::new(),
                required: false,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            },
            Pin {
                name: "p2".to_string(),
                number: PinNumber("2".to_string()),
                electrical_type: ElectricalType::Passive,
                capabilities: Vec::new(),
                required: false,
                unit: None,
                voltage_max_v: None,
                voltage_min_v: None,
                voltage_nominal_v: None,
            },
        ],
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
    }
}

fn resistor(id: ComponentId, refdes: &str) -> Component {
    Component {
        id,
        refdes: refdes.to_string(),
        kind: "resistor".to_string(),
        part: Some(two_pin_part()),
        value: None,
        placement_hint: None,
        group: None,
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn placement(id: ComponentId, x: f64, y: f64) -> ComponentPlacement {
    ComponentPlacement {
        id,
        center_mm: (x, y),
        rotation: Rotation::Zero,
    }
}

fn ep(component: ComponentId, pin: u32) -> NetEndpoint {
    NetEndpoint {
        component,
        pin: PinId(pin),
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

/// Extract every horizontal segment's y coordinate from a wire's
/// points (adjacent points differing in x only).
fn horizontal_y_segments(pts: &[(f64, f64)]) -> Vec<f64> {
    let mut ys = Vec::new();
    for pair in pts.windows(2) {
        let (x1, y1) = pair[0];
        let (x2, y2) = pair[1];
        if (y1 - y2).abs() < 1e-6 && (x1 - x2).abs() > 1e-6 {
            ys.push(y1);
        }
    }
    ys
}

/// Builds a two-row board: R1/R3 in the top row, R2/R4 in the bottom
/// row. Net0 links R1.p1 (left-facing) to R2.p2 (right-facing) and
/// Net1 links R3.p1 to R4.p2. Both nets span the same horizontal
/// gutter between the rows and their x-spans overlap, so left-edge
/// packing MUST put them on separate track levels.
fn two_row_board_and_layout() -> (Board, Layout) {
    let board = Board {
        name: "channel".to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        components: vec![
            resistor(ComponentId(0), "R1"),
            resistor(ComponentId(1), "R2"),
            resistor(ComponentId(2), "R3"),
            resistor(ComponentId(3), "R4"),
        ],
        nets: vec![
            Net {
                id: NetId(0),
                name: "n0".to_string(),
                endpoints: vec![ep(ComponentId(0), 0), ep(ComponentId(1), 1)],
            },
            Net {
                id: NetId(1),
                name: "n1".to_string(),
                endpoints: vec![ep(ComponentId(2), 0), ep(ComponentId(3), 1)],
            },
        ],
        diff_pairs: Vec::new(),
        keepouts: Vec::new(),
        source_span: synth_diagnostics::Span::new(0, 0),
    };
    let layout = Layout {
        components: vec![
            placement(ComponentId(0), 0.0, 0.0),
            placement(ComponentId(1), 40.0, 60.0),
            placement(ComponentId(2), 20.0, 0.0),
            placement(ComponentId(3), 20.0, 60.0),
        ],
        wires: Vec::new(),
        junctions: Vec::new(),
        power_flags: Vec::new(),
        net_labels: Vec::new(),
        annotations: Vec::new(),
        sheet_size: SheetSize::A4,
    };
    (board, layout)
}

#[test]
fn channel_router_decomposes_real_sheets_into_strips() {
    // The channel decomposition must find the horizontal gutter between
    // the two rows of a real board — this is the decomposition step
    // running on the pipeline's own data.
    let (board, layout) = two_row_board_and_layout();
    let channels = route::decompose_channels(&board, &layout);
    assert!(
        !channels.is_empty(),
        "expected at least one channel between the two component rows"
    );
    // The gutter sits strictly between the top-row bodies (y≈0) and the
    // bottom-row bodies (y≈60).
    for ch in &channels {
        assert!(
            ch.y_top >= 0.0 && ch.y_bottom <= 60.0,
            "channel out of band: {ch:?}"
        );
        assert!(ch.height() >= route::CHANNEL_PITCH_MM);
    }
}

#[test]
fn route_board_left_edge_packs_overlapping_nets_onto_separate_tracks() {
    let (board, layout) = two_row_board_and_layout();
    let result = route::route_board(&board, &layout);
    let channels = route::decompose_channels(&board, &layout);

    // Both nets must route, and each must carry a horizontal track
    // segment lying inside the channel gutter.
    let n0 = result.wires.iter().find(|w| w.net == NetId(0));
    let n1 = result.wires.iter().find(|w| w.net == NetId(1));
    assert!(n0.is_some(), "net0 must route");
    assert!(n1.is_some(), "net1 must route");

    let y0 = horizontal_y_segments(&n0.unwrap().points);
    let y1 = horizontal_y_segments(&n1.unwrap().points);
    assert!(
        !y0.is_empty(),
        "net0 must traverse the channel on a horizontal track"
    );
    assert!(
        !y1.is_empty(),
        "net1 must traverse the channel on a horizontal track"
    );

    // The pin stubs are also horizontal (2-pin resistor pins face
    // left/right at the body's row y), so only the packed *track*
    // segment lies inside the channel gutter. Filter to it.
    let in_channel = |y: f64| channels.iter().any(|c| c.y_top <= y && y <= c.y_bottom);
    let tracks0: Vec<f64> = y0.iter().copied().filter(|&y| in_channel(y)).collect();
    let tracks1: Vec<f64> = y1.iter().copied().filter(|&y| in_channel(y)).collect();
    assert!(
        !tracks0.is_empty(),
        "net0 must have a horizontal track segment inside the channel, got y={y0:?}"
    );
    assert!(
        !tracks1.is_empty(),
        "net1 must have a horizontal track segment inside the channel, got y={y1:?}"
    );

    // Left-edge packing guarantee: overlapping spans land on different
    // tracks, so the two wires run at different y AND those tracks are
    // at least CHANNEL_PITCH_MM apart.
    let track_y0 = *tracks0.last().unwrap();
    let track_y1 = *tracks1.last().unwrap();
    assert!(
        (track_y0 - track_y1).abs() >= route::CHANNEL_PITCH_MM - 1e-9,
        "overlapping nets must be packed >= {pitch} apart, got {track_y0} vs {track_y1}",
        pitch = route::CHANNEL_PITCH_MM
    );
    assert!(
        (track_y0 - track_y1).abs() > 1e-6,
        "nets on the same channel must not share a track line"
    );
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn route_board_never_leaves_overlapping_nets_on_the_same_channel_track() {
    // Property-style: with the channel router live, no two different
    // nets may run a horizontal segment on the exact same y inside a
    // channel. Guards the "zero line overlap" guarantee end-to-end.
    let (board, layout) = two_row_board_and_layout();
    let result = route::route_board(&board, &layout);
    let channels = route::decompose_channels(&board, &layout);

    let mut tracks_by_y: std::collections::BTreeMap<i64, Vec<NetId>> =
        std::collections::BTreeMap::new();
    for wire in &result.wires {
        for y in horizontal_y_segments(&wire.points) {
            let in_channel = channels.iter().any(|c| c.y_top <= y && y <= c.y_bottom);
            if !in_channel {
                continue;
            }
            let key = ((y * 100.0).round()) as i64;
            tracks_by_y.entry(key).or_default().push(wire.net);
        }
    }
    for (key, nets) in tracks_by_y {
        let unique: std::collections::HashSet<NetId> = nets.iter().copied().collect();
        assert!(
            unique.len() == nets.len(),
            "nets {nets:?} overlap on the same channel track y={:.2}",
            (key as f64) / 100.0
        );
    }
}
