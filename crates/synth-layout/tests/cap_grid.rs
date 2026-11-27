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

use synth_ir::{Board, Component, ComponentId, Net, NetEndpoint, NetId, PinId, SchematicPaper};
use synth_layout::{Layout, SheetSize};
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
        ground_domains: Vec::new(),
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

/// The ticket-01 singleton fixture plus a second group large enough that the
/// global single-sheet layout overflows A0 under the default `Grow` policy.
///
/// That is the mechanism that splits Jeff's SDRAM design: its root page is
/// A0 (see the exported `ice40_sdram_node.kicad_sch`), so the hierarchy comes
/// from content overflow, not from declared `sheet` blocks or an explicit
/// `overflow = "hierarchy"`. The bank holds 12 five-unit parts: their stacked
/// bodies make the global single-sheet drawing overflow A0.
///
/// The FPGA content is given an explicit `sheet` name so the merged-onto-root
/// rule cannot turn it into the root page: the page under test is a sub-sheet,
/// as the FPGA is in the real design (its root carries the merged first
/// group). The page under test is one group either way.
fn split_fixture(name: &str) -> Option<Board> {
    let mut b = cap_fixture(name, 16, false)?;
    for component in &mut b.components {
        if component.group.as_deref() == Some("FPGA") {
            component.sheet = Some("FPGA".to_string());
        }
    }
    let first = u32::try_from(b.components.len()).unwrap();
    for i in 0..12_u32 {
        b.components.push(component(
            first + i,
            &format!("B{}", i + 1),
            fpga(Vec::new(), ElectricalType::Passive),
            Some("BANK"),
        ));
    }
    Some(b)
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

/// The FPGA sheet of a split design holds one group, so the floorplan stage
/// skips it. Its page must still fit the wrapped content instead of keeping
/// the A1 the single long cap row produced.
#[test]
fn one_group_split_sheet_fits_its_content() {
    let Some(board) = split_fixture("split_page") else {
        return;
    };
    let global = synth_layout::layout(&board);
    let sheets = synth_layout::sheets::layout_sheets(&board, global, None);
    assert!(
        sheets.len() > 1,
        "the second group must force a split; got {} page(s)",
        sheets.len()
    );
    let fpga = sheets
        .iter()
        .find(|s| s.name.as_deref() == Some("FPGA"))
        .expect("the FPGA sheet");
    assert_eq!(
        fpga.layout
            .group_boxes
            .iter()
            .filter(|b| b.group == "FPGA")
            .count(),
        1,
        "the FPGA sheet is the one-group sheet"
    );

    // Cap geometry must survive the page change.
    let caps = ids(&board, "C");
    let cap_rects = rects(&board, &fpga.layout, &caps);
    let rows = rows_of(&cap_rects);
    assert!(rows.len() > 1, "the caps must still wrap");
    assert_rows_within_limit(&rows);
    assert_no_overlaps(&cap_rects);

    // The group box hugs the anchor body and every cap body, sits inside the
    // page frame, and stays clear of the title block (the same rule
    // `page_needs` uses: the box clears it horizontally or vertically).
    let anchor = body_rect(&board, &fpga.layout, ComponentId(0));
    let group_box = fpga
        .layout
        .group_boxes
        .iter()
        .find(|b| b.group == "FPGA")
        .expect("the FPGA group box");
    let (pw, ph) = fpga.layout.sheet_size.dims_mm();
    for (i, r) in cap_rects.iter().chain(std::iter::once(&anchor)).enumerate() {
        assert!(
            group_box.min_mm.0 <= r.0 + 1e-6
                && group_box.min_mm.1 <= r.1 + 1e-6
                && group_box.max_mm.0 >= r.2 - 1e-6
                && group_box.max_mm.1 >= r.3 - 1e-6,
            "body {i} at {r:?} is outside the group box {:?}",
            (group_box.min_mm, group_box.max_mm)
        );
    }
    assert!(
        group_box.max_mm.0 <= pw - 10.0 + 1e-6 && group_box.max_mm.1 <= ph - 10.0 + 1e-6,
        "group box {:?} runs past the page frame on {pw} × {ph}",
        (group_box.min_mm, group_box.max_mm)
    );
    let (block_x, block_y) = synth_layout::title_block_origin((pw, ph));
    assert!(
        group_box.max_mm.0 <= block_x + 1e-6 || group_box.max_mm.1 <= block_y + 1e-6,
        "group box {:?} overlaps the title block at ({block_x}, {block_y})",
        (group_box.min_mm, group_box.max_mm)
    );

    // Page area against the drawn content area. A1 (841 × 594 = 499,554 mm²)
    // with the wrapped ~226 × ~522 mm content is a ratio of about 4.3; a
    // measured page holding the content plus its borders and title-block band
    // is about 250 × 580 mm, a ratio of about 1.2. A factor of 2 therefore
    // fails on the old A1 page and passes on the fitted one. (The content is
    // 522 mm tall because the five stacked units extend below the anchor body,
    // so A2's 420 mm turns out not to fit it; the fit has to be measured.)
    let (cx0, cx1, cy0, cy1) =
        synth_layout::content_bounds(&board, &fpga.layout).expect("the FPGA sheet draws content");
    let page_area = pw * ph;
    let content_area = (cx1 - cx0) * (cy1 - cy0);
    let ratio = page_area / content_area;
    assert!(
        ratio <= 2.0,
        "page {pw} × {ph} is {ratio:.2}× the {:.2} × {:.2} mm content; the old A1 page would be ~4.3×",
        cx1 - cx0,
        cy1 - cy0
    );
    assert_ne!(
        fpga.layout.sheet_size,
        SheetSize::A1,
        "the one-group sheet kept the old A1 for content that fits a smaller page"
    );
}

/// A declared `schematic { paper }` is the page, not a bound: it stays
/// verbatim when the one-group page's content fits it and grows only when
/// the content overflows.
#[test]
fn declared_paper_is_kept_and_grown_only_on_overflow() {
    let page_of = |paper: SchematicPaper| {
        let mut board = split_fixture("declared_paper").expect("the FPGA symbol is installed");
        board.schematic_paper = Some(paper);
        let global = synth_layout::layout(&board);
        let sheets = synth_layout::sheets::layout_sheets(&board, global, None);
        assert!(sheets.len() > 1, "the fixture must split");
        let fpga = sheets
            .iter()
            .find(|s| s.name.as_deref() == Some("FPGA"))
            .expect("the FPGA sheet");
        let size = fpga.layout.sheet_size;
        let bounds = synth_layout::content_bounds(&board, &fpga.layout)
            .expect("the FPGA sheet draws content");
        (size, bounds)
    };

    // A0 holds the 226 × 522 mm page with room to spare, so it is kept.
    let (size, _) = page_of(SchematicPaper::A0);
    assert_eq!(
        size,
        SheetSize::A0,
        "a declared A0 was shrunk although the content fits it"
    );

    // A3 (420 × 297 mm) cannot hold the 522 mm-tall stack, so the page grows
    // to the smallest standard page that does, and the content lands inside it.
    let (size, (_, max_x, _, max_y)) = page_of(SchematicPaper::A3);
    assert_ne!(
        size,
        SheetSize::A3,
        "the page did not grow although the content overflows a declared A3"
    );
    let (width, height) = size.dims_mm();
    assert!(
        max_x <= width - 10.0 + 1e-6 && max_y <= height - 10.0 + 1e-6,
        "grown page {width} × {height} still clips the content to ({max_x:.2}, {max_y:.2})"
    );
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

/// A singleton row can be over the limit at the placer's pitch while a
/// tighter wrap fits (10 caps). The over-limit row must be re-spaced, not
/// left as one long line.
#[test]
fn over_limit_singleton_row_stays_within_the_limit() {
    let Some(board) = cap_fixture("singleton_ten", 10, false) else {
        return;
    };
    let layout = synth_layout::layout(&board);
    let caps = ids(&board, "C");
    assert_eq!(caps.len(), 10);
    let rects = rects(&board, &layout, &caps);
    let rows = rows_of(&rects);
    assert!(!rows.is_empty(), "the caps must be placed");
    assert_rows_within_limit(&rows);
    assert_no_overlaps(&rects);

    let anchor = body_rect(&board, &layout, ComponentId(0));
    for (i, r) in rects.iter().enumerate() {
        assert!(
            r.0 >= anchor.2 - 1e-6,
            "cap {i} at {r:?} is not beside the stacked FPGA body {anchor:?}"
        );
    }
}

/// The wrap boundary at the default 20.32 mm pitch: 13 caps fit exactly one
/// row, 14 do not. Zero and one cap keep a single row too.
#[test]
fn cap_row_wraps_only_above_the_limit() {
    for cap_count in [0, 1, 13] {
        let Some(board) = cap_fixture("at_the_limit", cap_count, true) else {
            return;
        };
        let layout = synth_layout::layout(&board);
        let caps = ids(&board, "C");
        let rows = rows_of(&rects(&board, &layout, &caps));
        assert!(
            rows.len() <= 1,
            "{cap_count} caps at 20.32 mm pitch fit one row; got {} row(s)",
            rows.len()
        );
        assert_rows_within_limit(&rows);
    }

    let Some(board) = cap_fixture("over_the_limit", 14, true) else {
        return;
    };
    let layout = synth_layout::layout(&board);
    let caps = ids(&board, "C");
    let rows = rows_of(&rects(&board, &layout, &caps));
    assert!(
        rows.len() > 1,
        "14 caps exceed the 13-cap row and must wrap; got {} row(s)",
        rows.len()
    );
    assert_rows_within_limit(&rows);
}

/// A long refdes raises the member pitch to 25.4 mm. The row capacity must
/// then count the cap body as well as the centres, or an 11-cap row is
/// 261.62 mm wide against the 256.54 mm limit.
#[test]
fn long_refdes_rows_stay_within_the_limit() {
    for as_members in [true, false] {
        let Some(mut board) = cap_fixture("long_refdes", 22, as_members) else {
            return;
        };
        for component in &mut board.components {
            if component.refdes.starts_with('C') {
                component.refdes = format!("CAPDECOUPLINGVCCIO{:08}", component.id.0);
            }
        }
        let layout = synth_layout::layout(&board);
        let caps = ids(&board, "C");
        assert_eq!(caps.len(), 22);
        let rects = rects(&board, &layout, &caps);
        let rows = rows_of(&rects);
        assert!(
            rows.len() > 1,
            "22 caps at a 25.4 mm pitch must wrap; got {} row(s)",
            rows.len()
        );
        assert_rows_within_limit(&rows);
        assert_no_overlaps(&rects);
    }
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
