// SPDX-License-Identifier: Apache-2.0

//! Regression tests for wrapping a multi-unit IC's decoupling caps into a
//! compact grid instead of one unbounded row.
//!
//! Two paths produced the long row Jeff's SDRAM FPGA sheet shows:
//!
//! 1. decoupling caps claimed as `Below` members of an `IcBlock` cluster,
//!    fanned out in a single row centred under the anchor; and
//! 2. a part that declares no `required_decoupling`, whose caps therefore
//!    stay singleton clusters and are placed one per column beside it.
//!
//! Both wrap only when a row would exceed a fixed maximum width; short rows
//! keep the historic geometry exactly.

use std::collections::BTreeMap;

use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId};
use synth_layout::Layout;
use synth_registry::{
    ElectricalType, Lifecycle, Part, PartId, Pin, PinCapability, PinNumber, RequiredDecoupling,
};

/// Stated fixed maximum width of one cap row, in mm. Also hard-coded in
/// `synth-layout`'s `CAP_ROW_MAX_MM`; the assertion is that geometry never
/// exceeds it, so an independent literal is the source of truth.
const MAX_CAP_ROW_MM: f64 = 256.54;

/// A real five-unit FPGA symbol, present in every KiCad install the other
/// stock-symbol tests rely on.
const FPGA_SYMBOL: &str = "FPGA_Lattice:ICE40HX4K-TQ144";

fn pin(name: &str, et: ElectricalType) -> Pin {
    pin_with(name, et, Vec::new())
}

fn pin_with(name: &str, et: ElectricalType, capabilities: Vec<PinCapability>) -> Pin {
    Pin {
        name: name.to_string(),
        number: PinNumber(name.to_string()),
        electrical_type: et,
        capabilities,
        required: false,
        footprint_only: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
        drive: None,
        gate: None,
        pwm: None,
    }
}

fn part(kind: &str, pins: Vec<Pin>) -> Part {
    Part {
        id: PartId(kind.to_string()),
        kind: kind.to_string(),
        description: None,
        version: 0,
        lifecycle: Lifecycle::Active,
        signed_by: Vec::new(),
        substitutes: Vec::new(),
        mpn: None,
        lcsc_pn: None,
        provenance: None,
        pins,
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
        spice: None,
    }
}

fn component(id: u32, refdes: &str, p: Part, group: Option<&str>) -> Component {
    Component {
        id: ComponentId(id),
        refdes: refdes.to_string(),
        kind: p.kind.clone(),
        part: Some(p),
        value: None,
        dnp: false,
        properties: BTreeMap::new(),
        placement_hint: None,
        escape: None,
        group: group.map(str::to_string),
        sheet: None,
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn ep(component: u32, pin: u32) -> NetEndpoint {
    NetEndpoint {
        component: ComponentId(component),
        pin: PinId(pin),
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

fn net(id: u32, name: &str, endpoints: &[(u32, u32)]) -> Net {
    Net {
        id: NetId(id),
        name: name.to_string(),
        auto_named: false,
        endpoints: endpoints.iter().map(|&(c, p)| ep(c, p)).collect(),
        netclass: None,
        voltage: None,
    }
}

fn board(name: &str, components: Vec<Component>, nets: Vec<Net>) -> Board {
    Board {
        outline: None,
        schematic_overflow: None,
        placement_spacing_nm: None,
        schematic_paper: None,
        groups: Vec::new(),
        legends: false,
        labels: true,
        name: name.to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        components,
        nets,
        diff_pairs: Vec::new(),
        notes: vec![],
        keepouts: Vec::new(),
        no_connects: Vec::new(),
        stackup: None,
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        variants: vec![],
        required_processes: std::collections::BTreeSet::new(),
        source_span: synth_diagnostics::Span::new(0, 0),
    }
}

/// A five-unit FPGA. `power_pins` is `PowerInput` for the member fixture so
/// the rail is real, and `Passive` for the singleton fixture so the orphan
/// rail-cap sweep leaves the caps alone — mirroring a registry part that
/// declares no `required_decoupling`.
fn fpga(required_decoupling: Vec<RequiredDecoupling>, power_pins: ElectricalType) -> Part {
    let mut p = part(
        "ic",
        vec![
            pin("VCC", power_pins),
            pin("GND", power_pins),
            pin("IO", ElectricalType::Bidirectional),
        ],
    );
    p.kicad_symbol = Some(FPGA_SYMBOL.to_string());
    p.required_decoupling = required_decoupling;
    p
}

fn cap() -> Part {
    part(
        "capacitor",
        vec![
            pin("p1", ElectricalType::Passive),
            pin("p2", ElectricalType::Passive),
        ],
    )
}

/// U1 plus `cap_count` caps on VCC/GND. With `required_decoupling` set the
/// caps become Below members of an IC block; without it they stay
/// singleton clusters in one declared group.
fn cap_fixture(name: &str, cap_count: usize, as_members: bool) -> Option<Board> {
    if synth_layout::kicad_lib_loader::symbol_units(FPGA_SYMBOL).map(|(_, n)| n) != Some(5) {
        eprintln!("KiCad symbol library not installed; skipping {name}");
        return None;
    }
    let required = if as_members {
        vec![RequiredDecoupling {
            net: "VCC".to_string(),
            value: "100n".to_string(),
            count: u32::try_from(cap_count).unwrap(),
            max_distance_mm: None,
            to: None,
            through: None,
            when_supply_over_v: None,
        }]
    } else {
        Vec::new()
    };
    let power_pins = if as_members {
        ElectricalType::PowerInput
    } else {
        ElectricalType::Passive
    };
    let group = (!as_members).then_some("FPGA");
    let mut components = vec![component(0, "U1", fpga(required, power_pins), group)];
    for i in 0..cap_count {
        components.push(component(
            u32::try_from(i + 1).unwrap(),
            &format!("C{}", i + 1),
            cap(),
            group,
        ));
    }

    let mut vcc = vec![(0, 0)];
    let mut gnd = vec![(0, 1)];
    for i in 0..cap_count {
        let id = u32::try_from(i + 1).unwrap();
        vcc.push((id, 0));
        gnd.push((id, 1));
    }
    Some(board(
        name,
        components,
        vec![net(0, "VCC", &vcc), net(1, "GND", &gnd)],
    ))
}

/// U1 with 16 I²C pull-ups split across SDA and SCL: the `Above` member
/// path (the horizontal bus-pull-up row).
fn above_fixture(name: &str) -> Option<Board> {
    if synth_layout::kicad_lib_loader::symbol_units(FPGA_SYMBOL).map(|(_, n)| n) != Some(5) {
        eprintln!("KiCad symbol library not installed; skipping {name}");
        return None;
    }
    let mut ic = part(
        "ic",
        vec![
            pin("VCC", ElectricalType::PowerInput),
            pin("GND", ElectricalType::PowerInput),
            pin_with(
                "SDA",
                ElectricalType::Bidirectional,
                vec![PinCapability::I2cSda],
            ),
            pin_with(
                "SCL",
                ElectricalType::Bidirectional,
                vec![PinCapability::I2cScl],
            ),
        ],
    );
    ic.kicad_symbol = Some(FPGA_SYMBOL.to_string());
    let mut components = vec![component(0, "U1", ic, None)];
    for i in 0..16 {
        components.push(component(
            u32::try_from(i + 1).unwrap(),
            &format!("R{}", i + 1),
            part(
                "resistor",
                vec![
                    pin("p1", ElectricalType::Passive),
                    pin("p2", ElectricalType::Passive),
                ],
            ),
            None,
        ));
    }
    let mut vcc = vec![(0, 0)];
    let mut sda = vec![(0, 2)];
    let mut scl = vec![(0, 3)];
    for i in 0..16_u32 {
        let id = i + 1;
        vcc.push((id, 1));
        if i < 8 {
            sda.push((id, 0));
        } else {
            scl.push((id, 0));
        }
    }
    Some(board(
        name,
        components,
        vec![
            net(0, "VCC", &vcc),
            net(1, "SDA", &sda),
            net(2, "SCL", &scl),
        ],
    ))
}

fn body_rect(board: &Board, layout: &Layout, id: ComponentId) -> (f64, f64, f64, f64) {
    let placed = layout.placement(id).expect("component is placed");
    let p = board
        .component(id)
        .and_then(|c| c.part.as_ref())
        .expect("component has a part");
    let (w, h) = synth_layout::body_size_for_part(p);
    (
        placed.center_mm.0 - w / 2.0,
        placed.center_mm.1 - h / 2.0,
        placed.center_mm.0 + w / 2.0,
        placed.center_mm.1 + h / 2.0,
    )
}

fn ids(board: &Board, refdes_prefix: &str) -> Vec<ComponentId> {
    let mut out: Vec<ComponentId> = board
        .components
        .iter()
        .filter(|c| c.refdes.starts_with(refdes_prefix))
        .map(|c| c.id)
        .collect();
    out.sort_by_key(|id| id.0);
    out
}

fn rects(board: &Board, layout: &Layout, ids: &[ComponentId]) -> Vec<(f64, f64, f64, f64)> {
    ids.iter().map(|&id| body_rect(board, layout, id)).collect()
}

fn overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    const EPS: f64 = 1e-6;
    a.0 < b.2 - EPS && b.0 < a.2 - EPS && a.1 < b.3 - EPS && b.1 < a.3 - EPS
}

/// Cap rows keyed by snapped centre y, so "one row" cannot hide behind
/// floating-point noise.
fn rows_of(rects: &[(f64, f64, f64, f64)]) -> BTreeMap<i32, Vec<(f64, f64, f64, f64)>> {
    let mut rows: BTreeMap<i32, Vec<(f64, f64, f64, f64)>> = BTreeMap::new();
    for &r in rects {
        let key = (f64::midpoint(r.1, r.3) * 100.0).round() as i32;
        rows.entry(key).or_default().push(r);
    }
    rows
}

fn assert_rows_within_limit(rows: &BTreeMap<i32, Vec<(f64, f64, f64, f64)>>) {
    for (y, row) in rows {
        let left = row.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
        let right = row.iter().map(|r| r.2).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            right - left <= MAX_CAP_ROW_MM + 1e-6,
            "row at y={:.2} is {:.2} mm wide, over the {MAX_CAP_ROW_MM} mm limit",
            f64::from(*y) / 100.0,
            right - left
        );
    }
}

fn assert_no_overlaps(rects: &[(f64, f64, f64, f64)]) {
    for (i, &a) in rects.iter().enumerate() {
        for (j, &b) in rects.iter().enumerate().skip(i + 1) {
            assert!(
                !overlap(a, b),
                "cap bodies {i} and {j} overlap: {a:?} {b:?}"
            );
        }
    }
}

/// Fixture A: 16 caps claimed as `Below` decoupling members. They must wrap
/// into more than one row, each row no wider than the limit, centred on the
/// anchor, clear of the full stacked unit body.
#[test]
fn member_caps_wrap_below_the_unit_stack() {
    let Some(board) = cap_fixture("member_caps", 16, true) else {
        return;
    };
    let layout = synth_layout::layout(&board);
    // The 16-cap row sets the drawing's left edge, so wrapping it moves the
    // page's anchor point: the anchor keeps its y and the drawing keeps its
    // left edge, and the x shift is exactly the narrower row. Fixture B
    // asserts the anchor's exact centre, where no row edge defines the page.
    let anchor_center = layout.placement(ComponentId(0)).unwrap().center_mm;
    assert_eq!(
        anchor_center.1, MEMBER_ANCHOR.1,
        "wrapping the row must not move the anchor vertically"
    );
    let caps = ids(&board, "C");
    assert_eq!(caps.len(), 16);
    let rects = rects(&board, &layout, &caps);
    let rows = rows_of(&rects);
    assert!(
        rows.len() > 1,
        "16 caps must wrap; got {} row(s), total span {:.2} mm",
        rows.len(),
        rects.iter().map(|r| r.2).fold(f64::NEG_INFINITY, f64::max)
            - rects.iter().map(|r| r.0).fold(f64::INFINITY, f64::min)
    );
    assert_rows_within_limit(&rows);
    assert_no_overlaps(&rects);

    // The drawing's left edge (the widest row's left body edge) is the same
    // as before the wrap, so the page re-anchor is the only x shift.
    let drawn_left = rects
        .iter()
        .map(|r| r.0)
        .fold(f64::INFINITY, f64::min)
        .min(anchor_center.0);
    assert!(
        (drawn_left - MEMBER_CONTENT_LEFT).abs() <= 1e-6,
        "the drawing's left edge moved: {drawn_left} vs {MEMBER_CONTENT_LEFT}"
    );

    let anchor = body_rect(&board, &layout, ComponentId(0));
    for (i, r) in rects.iter().enumerate() {
        assert!(
            !overlap(*r, anchor),
            "cap {i} at {r:?} overlaps the stacked FPGA body {anchor:?}"
        );
        assert!(
            r.1 >= anchor.3 - 1e-6,
            "cap {i} at {r:?} is not below the stacked FPGA body {anchor:?}"
        );
    }

    // Rows are centred on the anchor's x: the midpoint of each row's
    // outermost centres matches the anchor centre.
    let anchor_x = layout.placement(ComponentId(0)).unwrap().center_mm.0;
    for (y, row) in &rows {
        let mid = f64::midpoint(
            row.iter().map(|r| r.0).fold(f64::INFINITY, f64::min),
            row.iter().map(|r| r.2).fold(f64::NEG_INFINITY, f64::max),
        );
        assert!(
            (mid - anchor_x).abs() <= 1e-6,
            "row at y={:.2} is centred at x={mid:.2}, anchor at {anchor_x:.2}",
            f64::from(*y) / 100.0
        );
    }
}

/// Fixture B: the same part with no `required_decoupling`, so its 16 caps
/// stay singleton clusters in the declared `FPGA` group — Jeff's design.
/// They must wrap into a compact grid beside the unit stack, not below it
/// and not over it.
#[test]
fn singleton_caps_wrap_beside_the_unit_stack() {
    let Some(board) = cap_fixture("singleton_caps", 16, false) else {
        return;
    };
    let layout = synth_layout::layout(&board);
    assert_eq!(
        layout.placement(ComponentId(0)).unwrap().center_mm,
        SINGLETON_ANCHOR,
        "wrapping caps beside the stack must not move the anchor"
    );
    let caps = ids(&board, "C");
    assert_eq!(caps.len(), 16);
    let rects = rects(&board, &layout, &caps);
    let rows = rows_of(&rects);
    assert!(
        rows.len() > 1,
        "16 singleton caps must wrap; got {} row(s), total span {:.2} mm",
        rows.len(),
        rects.iter().map(|r| r.2).fold(f64::NEG_INFINITY, f64::max)
            - rects.iter().map(|r| r.0).fold(f64::INFINITY, f64::min)
    );
    assert_rows_within_limit(&rows);
    assert_no_overlaps(&rects);

    let anchor = body_rect(&board, &layout, ComponentId(0));
    for (i, r) in rects.iter().enumerate() {
        assert!(
            !overlap(*r, anchor),
            "cap {i} at {r:?} overlaps the stacked FPGA body {anchor:?}"
        );
        assert!(
            r.0 >= anchor.2 - 1e-6,
            "cap {i} at {r:?} is not beside the stacked FPGA body {anchor:?}"
        );
        assert!(
            r.1 >= anchor.1 - 1e-6 && r.3 <= anchor.3 + 1e-6,
            "cap {i} at {r:?} is below/above the stacked FPGA body {anchor:?}"
        );
    }
}

/// Fixture C: 16 `Above` members (I²C pull-ups). The horizontal row above
/// the anchor wraps the same way, upwards and centred.
#[test]
fn above_members_wrap_over_the_unit_stack() {
    let Some(board) = above_fixture("above_members") else {
        return;
    };
    let layout = synth_layout::layout(&board);
    let members = ids(&board, "R");
    assert_eq!(members.len(), 16);
    let rects = rects(&board, &layout, &members);
    let rows = rows_of(&rects);
    assert!(
        rows.len() > 1,
        "16 above members must wrap; got {} row(s), total span {:.2} mm",
        rows.len(),
        rects.iter().map(|r| r.2).fold(f64::NEG_INFINITY, f64::max)
            - rects.iter().map(|r| r.0).fold(f64::INFINITY, f64::min)
    );
    assert_rows_within_limit(&rows);
    assert_no_overlaps(&rects);

    let anchor = body_rect(&board, &layout, ComponentId(0));
    for (i, r) in rects.iter().enumerate() {
        assert!(
            !overlap(*r, anchor),
            "member {i} at {r:?} overlaps the stacked FPGA body {anchor:?}"
        );
        assert!(
            r.3 <= anchor.1 + 1e-6,
            "member {i} at {r:?} is not above the stacked FPGA body {anchor:?}"
        );
    }

    // Deterministic order: the row nearest the anchor starts with the
    // lowest member id (rows are keyed by y, so the last is the base row).
    let base_row = rows.values().last().unwrap();
    let leftmost = base_row.iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
    let r1 = body_rect(&board, &layout, ComponentId(1));
    assert_eq!(
        *leftmost, r1,
        "the base row must start with the lowest member id"
    );
}

/// A row that fits keeps the exact pre-change geometry. Positions below were
/// captured from the layout before the wrap change landed.
#[test]
fn short_cap_row_does_not_move() {
    let Some(board) = cap_fixture("short_row", 4, true) else {
        return;
    };
    let layout = synth_layout::layout(&board);
    let caps = ids(&board, "C");
    let got: Vec<(f64, f64)> = caps
        .iter()
        .map(|&id| layout.placement(id).unwrap().center_mm)
        .collect();
    assert_eq!(
        got,
        SHORT_ROW_EXPECTED.to_vec(),
        "a 4-cap row fits the limit and must not move"
    );
    assert_eq!(
        layout.placement(ComponentId(0)).unwrap().center_mm,
        SHORT_ROW_ANCHOR,
        "the anchor must not move when no row wraps"
    );
}

const SHORT_ROW_EXPECTED: [(f64, f64); 4] = [
    (40.64, 424.18),
    (60.96, 424.18),
    (81.28, 424.18),
    (101.600_000_000_000_01, 424.18),
];
const SHORT_ROW_ANCHOR: (f64, f64) = (71.12, 215.9);
const MEMBER_ANCHOR: (f64, f64) = (193.04, 215.9);
/// Left body edge of the pre-change 16-cap member row; the page margin
/// anchors the drawing on it, so it must survive the wrap.
const MEMBER_CONTENT_LEFT: f64 = 36.83;
const SINGLETON_ANCHOR: (f64, f64) = (53.34, 223.52);
