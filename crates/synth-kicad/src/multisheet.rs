// SPDX-License-Identifier: Apache-2.0

// Placement geometry is millimetres in `f64`, exactly as in
// `crate::schematic`; the usize/u32 → f64 casts are bounded by pin
// counts and sheet counts, and the integer→float conversions are
// intentional at the layout boundary.
#![allow(clippy::cast_precision_loss)]

//! Hierarchical multi-sheet export (§P26).
//!
//! Reached only for *large splittable* boards: the single-sheet
//! content overflows A2 and the components span at least two sheet
//! boundaries. Everything else takes the historic single-file path
//! byte for byte.
//!
//! Each sub-sheet becomes its own `.kicad_sch` file; the root file
//! keeps the unassigned components plus one `(sheet …)` instance per
//! sub-sheet. Cross-sheet *signal* nets join through hierarchical
//! labels (one per in-sheet endpoint), matching `(pin …)`s on the
//! parent instance, and same-named local labels on the root (one per
//! sheet pin, plus one per root endpoint of the same net). Cross-sheet
//! *power* nets need no labels at all — power symbols already connect
//! globally by value. One `power:PWR_FLAG` drives each undriven rail
//! project-wide (on its anchor's sheet), exactly like the
//! single-sheet path.
//!
//! All uuids derive from sheet + net names, so re-export is
//! byte-identical. Every emitted file is validated by loading it in
//! `kicad-cli sch erc` (see the `multisheet_kicad_loads` test).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use synth_ir::{Board, ComponentId};
use synth_layout::route::snap_grid_127;
use synth_layout::sheets::SheetLayout;
use uuid::Uuid;

use crate::export::{write_file, ExportError};
use crate::schematic::{
    build_sheet_schematic, PlacedSheetInstance, PowerRefAllocator, RootFurniture, RootWire,
    SheetRender,
};
use crate::uuid_v5::derive_entity_uuid;

/// Left margin for the sheet-instance row; gap below the root
/// content before it; instance height; minimum instance width.
const INSTANCE_MARGIN: f64 = 20.0;
const INSTANCE_GAP: f64 = 25.0;
const INSTANCE_H: f64 = 60.0;
const INSTANCE_MIN_W: f64 = 80.0;
/// Pin spacing granularity along the box bottom edge, and channel
/// pitch for root inter-pin wires.
const PIN_PITCH: f64 = 12.0;
/// Stub length from a sheet pin down to its local label (4 grid
/// units, so the label anchor stays on the connection grid).
const PIN_STUB: f64 = 5.08;

/// A sub-sheet ready to write: its file name, uuid, and member set.
#[derive(Debug)]
pub struct SheetFile {
    pub name: String,
    pub uuid: Uuid,
    pub file: String,
    pub members: HashSet<ComponentId>,
}

/// File name for a sub-sheet: `<stem>_<sanitized sheet>.kicad_sch`.
pub fn sheet_filename(stem: &str, sheet: &str) -> String {
    format!("{stem}_{}.kicad_sch", sanitize_sheet(sheet))
}

/// Sheet names may carry spaces and symbols; file names may not.
/// Mirrors the project stem sanitizer.
pub fn sanitize_sheet(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

/// Stable uuid for a sub-sheet file.
pub fn sheet_uuid(project: &Uuid, sheet: &str) -> Uuid {
    derive_entity_uuid(project, "sheet", sheet)
}

/// Write one `.kicad_sch` per sheet: the root file plus one file
/// per sub-sheet. Power-symbol references share one allocator across
/// all files so `#PWR…` refdes stay unique project-wide. The root
/// file additionally carries the sheet instances and pin labels from
/// [`plan_multisheet`], and grows to fit them.
pub(crate) fn export_sheets(
    board: &Board,
    out_dir: &Path,
    stem: &str,
    project: &Uuid,
    mut sheets: Vec<SheetLayout>,
) -> Result<(), ExportError> {
    let root_bounds = content_bounds(board, &sheets[0].layout);
    let plan = plan_multisheet(board, project, stem, &sheets, root_bounds);
    // Size the root sheet around its content plus the instance row
    // and wire channels.
    {
        let root = &mut sheets[0].layout;
        let (mut min_x, mut max_x, mut min_y, mut max_y) = root_bounds;
        for instance in &plan.furniture.instances {
            min_x = min_x.min(instance.at_mm.0);
            max_x = max_x.max(instance.at_mm.0 + instance.size_mm.0);
            min_y = min_y.min(instance.at_mm.1);
            max_y = max_y.max(instance.at_mm.1 + instance.size_mm.1);
        }
        for wire in &plan.furniture.wires {
            for (x, y) in [wire.a_mm, wire.b_mm] {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        let needed = synth_layout::fit_sheet_size(min_x, max_x, min_y, max_y);
        let (have_w, have_h) = root.sheet_size.dims_mm();
        let (want_w, want_h) = needed.dims_mm();
        if want_w > have_w || want_h > have_h {
            root.sheet_size = needed;
        }
    }

    let mut power_refs = PowerRefAllocator::default();
    // Root first (index 0), matching `drivers_per_sheet` order.
    let root_drivers = plan.drivers_per_sheet.first();
    let root_members: HashSet<ComponentId> =
        sheets[0].layout.components.iter().map(|p| p.id).collect();
    let root_uuid = derive_entity_uuid(project, "sheet", "root");
    let root_text = build_sheet_schematic(
        board,
        project,
        &sheets[0].layout,
        &SheetRender {
            name: None,
            uuid: root_uuid,
            project_name: stem,
            // A hierarchy gives *every* symbol an instance block,
            // including the root's (dev-docs symbol section: "Every
            // symbol will have at least one instance"). The single
            // sheet path leaves this `None`, which is what keeps it
            // byte-identical to the pre-§P26 output.
            symbol_path: Some(format!("/{root_uuid}")),
            members: Some(&root_members),
            power_drivers: root_drivers.map(Vec::as_slice),
            root_furniture: Some(&plan.furniture),
        },
        &mut power_refs,
    )
    .to_string_pretty();
    write_file(&out_dir.join(format!("{stem}.kicad_sch")), &root_text)?;

    for (file, sheet) in plan.files.iter().zip(sheets.iter().skip(1)) {
        let members: HashSet<ComponentId> = sheet.layout.components.iter().map(|p| p.id).collect();
        let drivers = plan
            .drivers_per_sheet
            .get(
                sheets
                    .iter()
                    .position(|s| s.name.as_deref() == Some(file.name.as_str()))
                    .unwrap_or(0),
            )
            .map(Vec::as_slice);
        let sub_uuid = file.uuid;
        let text = build_sheet_schematic(
            board,
            project,
            &sheet.layout,
            &SheetRender {
                name: Some(&file.name),
                uuid: sub_uuid,
                project_name: stem,
                symbol_path: Some(format!(
                    "/{}/{sub_uuid}",
                    derive_entity_uuid(project, "sheet", "root")
                )),
                members: Some(&members),
                power_drivers: drivers,
                root_furniture: None,
            },
            &mut power_refs,
        )
        .to_string_pretty();
        write_file(&out_dir.join(&file.file), &text)?;
    }
    Ok(())
}

/// Content bounds `(min_x, max_x, min_y, max_y)` of a laid-out sheet:
/// bodies, wires, annotations, and group boxes. Mirrors the bound
/// pass in `grow_sheet_to_fit`.
fn content_bounds(board: &Board, layout: &synth_layout::Layout) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for placement in &layout.components {
        let (cx, cy) = placement.center_mm;
        let (bw, bh) = board
            .component(placement.id)
            .and_then(|c| c.part.as_ref())
            .map_or((15.0, 10.0), synth_layout::body_size_for_part);
        min_x = min_x.min(cx - bw / 2.0);
        max_x = max_x.max(cx + bw / 2.0);
        min_y = min_y.min(cy - bh / 2.0);
        max_y = max_y.max(cy + bh / 2.0);
    }
    for wire in &layout.wires {
        for &(x, y) in &wire.points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    for text in &layout.annotations {
        let (x, y) = text.at_mm;
        let width = text.text.chars().count() as f64 * text.size_mm * 0.72;
        min_x = min_x.min(x);
        max_x = max_x.max(x + width);
        min_y = min_y.min(y - text.size_mm);
        max_y = max_y.max(y);
    }
    for box_ in &layout.group_boxes {
        min_x = min_x.min(box_.min_mm.0);
        max_x = max_x.max(box_.max_mm.0);
        min_y = min_y.min(box_.min_mm.1);
        max_y = max_y.max(box_.max_mm.1);
    }
    (min_x, max_x, min_y, max_y)
}

/// Everything the multi-sheet export needs: per-sheet files and
/// members, root furniture, and per-sheet power drivers.
#[derive(Debug)]
pub struct MultiSheetPlan {
    pub files: Vec<SheetFile>,
    pub furniture: RootFurniture,
    /// Power drivers per sheet index (root first), deduped by net so
    /// one flag drives each merged hierarchical rail.
    pub drivers_per_sheet: Vec<Vec<crate::pin_reconcile::PowerDriver>>,
}

/// Plan root furniture and per-sheet drivers for `sheets` (root
/// first). `stem` names the sub-sheet files; `root_bounds` is the
/// root layout's content `(min_x, max_x, min_y, max_y)`.
pub fn plan_multisheet(
    board: &Board,
    project: &Uuid,
    stem: &str,
    sheets: &[SheetLayout],
    root_bounds: (f64, f64, f64, f64),
) -> MultiSheetPlan {
    // Cross-sheet signal nets: hierarchical labels exist exactly for
    // these (power rails join globally and carry none), so the label
    // sets are the authoritative cross-net inventory.
    let mut cross: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, sheet) in sheets.iter().enumerate() {
        if sheet.name.is_none() {
            continue;
        }
        let members: HashSet<ComponentId> = sheet.layout.components.iter().map(|p| p.id).collect();
        for hier in &sheet.layout.hierarchical_labels {
            if members.contains(&hier.component) {
                cross.entry(hier.label.clone()).or_default().push(index);
            }
        }
    }
    let mut cross_nets: Vec<String> = cross.keys().cloned().collect();
    cross_nets.sort();

    // Instance boxes in a row below the root content (or at the
    // margin when the root sheet holds no components of its own).
    // Every coordinate that carries copper (pins, stubs, channels)
    // snaps to the 1.27 mm connection grid — KiCad ERC exact-matches
    // wire ends to pins, so fractional positions read as
    // `endpoint_off_grid` plus lost connections.
    let (_min_x, _max_x, _min_y, root_max_y) = root_bounds;
    let mut cursor_x = snap_grid_127(INSTANCE_MARGIN);
    let box_y = snap_grid_127(if root_max_y.is_finite() {
        root_max_y + INSTANCE_GAP
    } else {
        INSTANCE_MARGIN
    });
    // Pin row is snapped independently; the box height is then
    // derived from it so the bottom edge lands exactly on the pins.
    let pin_y = snap_grid_127(box_y + INSTANCE_H);
    let box_h = pin_y - box_y;
    let mut instances = Vec::new();
    let mut files = Vec::new();
    for (index, sheet) in sheets.iter().enumerate() {
        let Some(name) = sheet.name.as_deref() else {
            continue;
        };
        let members: HashSet<ComponentId> = sheet.layout.components.iter().map(|p| p.id).collect();
        let pins_here: Vec<&String> = cross_nets
            .iter()
            .filter(|net| cross[net.as_str()].contains(&index))
            .collect();
        let width =
            snap_grid_127(INSTANCE_MIN_W.max(pins_here.len() as f64 * PIN_PITCH + 2.0 * PIN_PITCH));
        let uuid = sheet_uuid(project, name);
        let mut pins = Vec::new();
        for (pi, net) in pins_here.iter().enumerate() {
            let px =
                snap_grid_127(cursor_x + (pi + 1) as f64 * width / (pins_here.len() + 1) as f64);
            let pin_uuid = derive_entity_uuid(project, "sheet_pin", &format!("{name}/{net}"));
            pins.push(((*net).clone(), (px, pin_y), pin_uuid));
        }
        instances.push(PlacedSheetInstance {
            name: name.to_string(),
            file: sheet_filename(stem, name),
            uuid,
            at_mm: (cursor_x, box_y),
            size_mm: (width, box_h),
            pins,
        });
        files.push(SheetFile {
            name: name.to_string(),
            uuid,
            file: sheet_filename(stem, name),
            members,
        });
        cursor_x += width + INSTANCE_GAP;
    }

    // Root wires: one channel per net below the boxes, vertical stubs
    // from each pin plus a horizontal span. Channels never collide
    // (distinct y) and clear all content (below everything).
    // Each sheet pin gets a short stub plus a local label carrying the
    // net name. Same-named local labels on the root sheet are one net,
    // so a pin joins both the root's own cross-sheet endpoints (which
    // the schematic emits as local labels) and every other pin of the
    // same net. This replaces an earlier wire-channel scheme that
    // silently dropped nets with fewer than two sheet pins — i.e. any
    // signal crossing between the root and a single sub-sheet.
    let mut wires = Vec::new();
    let mut pin_labels = Vec::new();
    for instance in &instances {
        for (pin_net, (px, py), _) in &instance.pins {
            let stub_end = snap_grid_127(py + PIN_STUB);
            wires.push(RootWire {
                net: pin_net.clone(),
                a_mm: (*px, *py),
                b_mm: (*px, stub_end),
            });
            pin_labels.push((pin_net.clone(), (*px, stub_end)));
        }
    }

    // One project-wide undriven-rail pass per sheet, deduped by net
    // (root first): a rail driven anywhere must not gain a second
    // PWR_FLAG on another sheet.
    let mut claimed: HashSet<synth_ir::NetId> = HashSet::new();
    let mut drivers_per_sheet = Vec::new();
    for sheet in sheets {
        let placements: HashMap<ComponentId, &synth_layout::ComponentPlacement> =
            sheet.layout.components.iter().map(|p| (p.id, p)).collect();
        let mut drivers = Vec::new();
        for driver in crate::pin_reconcile::undriven_power_nets(board, &placements) {
            if claimed.insert(driver.net) {
                drivers.push(driver);
            }
        }
        drivers_per_sheet.push(drivers);
    }

    MultiSheetPlan {
        files,
        furniture: RootFurniture {
            instances,
            wires,
            junctions: Vec::new(),
            pin_labels,
        },
        drivers_per_sheet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use synth_diagnostics::Span;
    use synth_ir::{Component, ComponentId, Net};
    use synth_layout::sheets::layout_sheets;
    use synth_layout::{ComponentPlacement, Layout, Rotation, SheetSize};
    use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

    fn pin(name: &str) -> RegPin {
        RegPin {
            name: name.to_string(),
            number: PinNumber(name.to_string()),
            electrical_type: ElectricalType::Passive,
            capabilities: Vec::new(),
            required: false,
            unit: None,
            voltage_max_v: None,
            voltage_min_v: None,
            voltage_nominal_v: None,
        }
    }

    fn part(pins: Vec<RegPin>) -> Part {
        Part {
            id: PartId("r".to_string()),
            kind: "resistor".to_string(),
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
        }
    }

    fn component(id: u32, sheet: Option<&str>) -> Component {
        Component {
            id: ComponentId(id),
            refdes: format!("R{id}"),
            kind: "resistor".to_string(),
            part: Some(part(vec![pin("p1"), pin("p2")])),
            value: None,
            dnp: false,
            properties: std::collections::BTreeMap::new(),
            placement_hint: None,
            group: None,
            sheet: sheet.map(str::to_string),
            source_span: Span::new(0, 0),
        }
    }

    fn board(components: Vec<Component>, nets: Vec<Net>) -> Board {
        Board {
            legends: false,
            name: "test".to_string(),
            layers: 2,
            manufacturer: None,
            revision: None,
            company: None,
            components,
            nets,
            diff_pairs: Vec::new(),
            notes: Vec::new(),
            keepouts: Vec::new(),
            netclasses: vec![],
            buses: vec![],
            modules: vec![],
            variants: vec![],
            source_span: Span::new(0, 0),
        }
    }

    /// Global layout with components at the given centres (no wires).
    fn layout_at(centres: &[(u32, f64, f64)]) -> Layout {
        Layout {
            components: centres
                .iter()
                .map(|&(id, x, y)| ComponentPlacement {
                    id: ComponentId(id),
                    center_mm: (x, y),
                    rotation: Rotation::Zero,
                })
                .collect(),
            wires: Vec::new(),
            junctions: Vec::new(),
            power_flags: Vec::new(),
            net_labels: Vec::new(),
            hierarchical_labels: Vec::new(),
            annotations: Vec::new(),
            group_boxes: Vec::new(),
            sheet_size: SheetSize::A2,
        }
    }

    #[test]
    fn small_board_stays_single_file() {
        let b = board(
            vec![component(0, None), component(1, Some("Power"))],
            vec![],
        );
        let global = layout_at(&[(0, 50.0, 50.0), (1, 100.0, 50.0)]);
        let sheets = layout_sheets(&b, global);
        assert_eq!(sheets.len(), 1, "small boards keep the single-file path");
        assert_eq!(sheets[0].name, None);
    }

    #[test]
    fn large_unsplittable_board_stays_single_file() {
        // Overflows A2 (components far past the page) but has no
        // sheet boundaries, so §P26 leaves it alone.
        let b = board(
            vec![component(0, None), component(1, None), component(2, None)],
            vec![],
        );
        let global = layout_at(&[(0, 50.0, 50.0), (1, 900.0, 50.0), (2, 1700.0, 50.0)]);
        let sheets = layout_sheets(&b, global);
        assert_eq!(
            sheets.len(),
            1,
            "an unsplittable large board keeps one (overflowing) sheet"
        );
    }

    #[test]
    fn large_splittable_board_splits_per_sheet() {
        let b = board(
            vec![
                component(0, Some("A")),
                component(1, Some("A")),
                component(2, Some("B")),
                component(3, Some("B")),
            ],
            vec![],
        );
        // Spread the two sheets' parts across a page wider than A2.
        let global = layout_at(&[
            (0, 50.0, 50.0),
            (1, 120.0, 50.0),
            (2, 800.0, 50.0),
            (3, 900.0, 50.0),
        ]);
        let sheets = layout_sheets(&b, global);
        assert_eq!(sheets.len(), 3, "root + A + B");
        assert_eq!(sheets[0].name, None);
        let names: Vec<_> = sheets[1..].iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, vec![Some("A".to_string()), Some("B".to_string())]);
        for sheet in &sheets {
            for placement in &sheet.layout.components {
                assert!(placement.center_mm.0 >= 20.0 - 1e-6);
                assert!(placement.center_mm.1 >= 20.0 - 1e-6);
            }
        }
    }

    #[test]
    fn split_is_deterministic() {
        let b = board(
            vec![
                component(0, None),
                component(1, Some("A")),
                component(2, Some("B")),
            ],
            vec![],
        );
        let mk = || layout_at(&[(0, 50.0, 50.0), (1, 800.0, 50.0), (2, 900.0, 50.0)]);
        let a = layout_sheets(&b, mk());
        let z = layout_sheets(&b, mk());
        assert_eq!(a, z, "split output must be byte-deterministic");
    }

    #[test]
    fn sheet_filename_sanitizes() {
        assert_eq!(sheet_filename("board", "Power"), "board_Power.kicad_sch");
        assert_eq!(
            sheet_filename("board", "+3V3 rails"),
            "board__3V3_rails.kicad_sch"
        );
    }

    /// End-to-end: a real multi-sheet board writes one file per sheet
    /// and KiCad loads the hierarchy with zero ERC violations.
    /// Skips when `kicad-cli` is unavailable so the suite stays green
    /// on machines without KiCad.
    #[test]
    fn multisheet_kicad_loads() {
        use std::fmt::Write as _;
        if std::process::Command::new("kicad-cli")
            .arg("version")
            .output()
            .is_err()
        {
            eprintln!("skipping: kicad-cli not on PATH");
            return;
        }
        // Generated rather than stored: the board must be large
        // enough to overflow A2 (the §P26 split trigger), which makes
        // a fixture file big and its IR snapshot bigger.
        //
        // It deliberately mixes every multi-sheet feature: root
        // components (a regulator + its decoupling), a declared power
        // rail consumed on a sub-sheet (cross-sheet power, no pins),
        // and cross-sheet *signal* links (hierarchical labels on
        // sub-sheets, local labels on the root).
        let mut src = String::from("board \"multisheet\" {\n  layers 2\n");
        src.push_str("  component U1: regulator \"ams1117_3v3\"\n");
        src.push_str("  component C1: capacitor \"c_generic_0805\" value \"10u\"\n");
        src.push_str("  component C2: capacitor \"c_generic_0805\" value \"10u\"\n");
        src.push_str("  power \"+3V3\" 3.3v\n");
        src.push_str("  connect U1.vout -> C1.p1 as \"+3V3\"\n");
        src.push_str("  connect U1.vin -> C2.p1\n");
        src.push_str("  connect U1.gnd -> C1.p2\n");
        src.push_str("  connect U1.gnd -> C2.p2\n");
        // A root component whose two pins cross into a sub-sheet on
        // *signal* (not power) nets: the case that used to leave the
        // root pins and their sheet pins dangling.
        src.push_str("  component R900: resistor \"r_generic_0603\"\n");
        src.push_str("  connect R900.p1 -> R20.p2\n");
        src.push_str("  connect R900.p2 -> R21.p2\n");
        for sheet in 0..4 {
            let _ = writeln!(src, "  sheet \"S{sheet}\" {{");
            for i in 0..45 {
                let n = sheet * 45 + i;
                let _ = writeln!(src, "    component R{n}: resistor \"r_generic_0603\"");
            }
            for i in 0..44 {
                let (a, b) = (sheet * 45 + i, sheet * 45 + i + 1);
                let _ = writeln!(src, "    connect R{a}.p2 -> R{b}.p1");
            }
            let (first, last) = (sheet * 45, sheet * 45 + 44);
            let _ = writeln!(src, "    connect R{first}.p1 -> R{last}.p2");
            src.push_str("  }\n");
        }
        // Cross-sheet signal links exercise hierarchical labels/pins.
        src.push_str("  connect R0.p1 -> R45.p1\n");
        src.push_str("  connect R45.p2 -> R90.p1\n");
        src.push_str("  connect R90.p2 -> R135.p1\n");
        // Cross-sheet power: joins the root rail to a sub-sheet part
        // through global power symbols, never through sheet pins.
        src.push_str("  connect R135.p2 -> U1.vout as \"+3V3\"\n}\n");

        let parsed = synth_parser::parse(&src, "multisheet.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = synth_registry::load_dir(&root.join("registry/parts")).unwrap();
        let board = synth_ir::lower(&parsed.ast.unwrap(), &registry, "multisheet.synth")
            .board
            .unwrap();
        let global = synth_layout::layout(&board);
        let sheets = synth_layout::sheets::layout_sheets(&board, global);
        assert!(
            sheets.len() > 1,
            "board must be large enough to split, got {} sheet(s)",
            sheets.len()
        );
        let dir = std::env::temp_dir().join(format!("synth-ms-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = crate::uuid_v5::project_namespace(&board.name);
        export_sheets(&board, &dir, &board.name, &project, sheets).unwrap();
        // A minimal `.kicad_pro` is required for KiCad to treat the
        // directory as a project and load `sym-lib-table` /
        // `${KIPRJMOD}` — without it every `synth:` power-flag symbol
        // reports `lib_symbol_issues`. `export.rs` always writes one.
        let project_doc = format!(
            "{{\n  \"meta\": {{\n    \"filename\": \"multisheet.kicad_pro\",\n    \
             \"version\": 1,\n    \"uuid\": \"{project}\"\n  }},\n  \
             \"sheets\": []\n}}\n"
        );
        std::fs::write(dir.join("multisheet.kicad_pro"), project_doc).unwrap();
        // The symbol library and its sym-lib-table must sit beside
        // the sheets, exactly as `export.rs` writes them, or KiCad
        // warns that the `synth` nickname is unresolvable for the
        // power-flag symbols the schematic references.
        let layout = synth_layout::layout(&board);
        let library = crate::symbol_lib::build_library(&board, &layout).to_string_pretty();
        std::fs::write(dir.join("multisheet.kicad_sym"), library).unwrap();
        std::fs::write(
            dir.join("sym-lib-table"),
            "(sym_lib_table\n  (version 7)\n  (lib\n    (name \"synth\")\n    \
             (uri \"${KIPRJMOD}/multisheet.kicad_sym\")\n    (type \"KiCad\")\n    \
             (options \"\")\n    (descr \"Synth synthesized symbol library\")\n  )\n)\n",
        )
        .unwrap();
        let output = std::process::Command::new("kicad-cli")
            .args(["sch", "erc"])
            .arg(dir.join("multisheet.kicad_sch"))
            .args(["--output"])
            .arg(dir.join("erc.json"))
            .args(["--format", "json"])
            .output()
            .expect("kicad-cli runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Found 0 violations"),
            "multi-sheet export must be ERC-clean:\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A hierarchy needs `instances` on *every* symbol, root's
    /// included (dev-docs symbol section). Regression guard for the
    /// mixed root+sheets case, where a root symbol without its
    /// instance block makes KiCad refuse the whole project.
    #[test]
    fn hierarchical_export_gives_root_symbols_instance_paths() {
        use synth_diagnostics::Span;
        let b = board(
            vec![
                component(0, None),
                component(1, Some("A")),
                component(2, Some("A")),
            ],
            vec![],
        );
        let global = layout_at(&[(0, 50.0, 50.0), (1, 800.0, 50.0), (2, 900.0, 50.0)]);
        let sheets = layout_sheets(&b, global);
        assert!(sheets.len() > 1, "precondition: the board splits");
        let project = crate::uuid_v5::project_namespace(&b.name);
        let root_uuid = derive_entity_uuid(&project, "sheet", "root");

        // Multi-sheet root: instances present, keyed to the root path.
        let multi = crate::schematic::build_sheet_schematic(
            &b,
            &project,
            &sheets[0].layout,
            &crate::schematic::SheetRender {
                name: None,
                uuid: root_uuid,
                project_name: &b.name,
                symbol_path: Some(format!("/{root_uuid}")),
                members: None,
                power_drivers: None,
                root_furniture: None,
            },
            &mut crate::schematic::PowerRefAllocator::default(),
        )
        .to_string_pretty();
        assert_eq!(
            multi.matches("(instances").count(),
            1,
            "the root symbol must carry an instance block:\n{multi}"
        );
        assert!(multi.contains(&root_uuid.to_string()), "root path present");

        // Single-sheet export stays bare — this is what keeps it
        // byte-identical to the pre-§P26 output.
        let single = crate::schematic::build_schematic(&b, &project).to_string_pretty();
        assert_eq!(single.matches("(instances").count(), 0);
        let _ = Span::new(0, 0);
    }

    /// A cross-sheet **signal** net with a root endpoint must be
    /// labelled on the root too. Regression guard: the root used to
    /// get no label (hierarchical labels were emitted for sub-sheets
    /// only), leaving the root pin and the sub-sheet's sheet pin
    /// `pin_not_connected`.
    #[test]
    fn root_cross_signal_nets_get_root_labels() {
        use synth_diagnostics::Span;
        use synth_ir::{Net, NetEndpoint, NetId, PinId};

        let mut r0 = component(0, None);
        r0.refdes = "R0".to_string();
        let mut r1 = component(1, Some("A"));
        r1.refdes = "R1".to_string();
        let b = board(
            vec![r0, r1],
            vec![Net {
                id: NetId(0),
                name: "SIG".to_string(),
                endpoints: vec![
                    NetEndpoint {
                        component: ComponentId(0),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                    NetEndpoint {
                        component: ComponentId(1),
                        pin: PinId(0),
                        source_span: Span::new(0, 0),
                    },
                ],
                netclass: None,
                voltage: None,
            }],
        );
        // A root layout carrying the cross-sheet label for R0.
        let mut root = layout_at(&[(0, 40.0, 40.0)]);
        root.hierarchical_labels = vec![synth_layout::HierarchicalLabel {
            net: NetId(0),
            component: ComponentId(0),
            pin: PinId(0),
            label: "SIG".to_string(),
        }];
        let project = crate::uuid_v5::project_namespace(&b.name);
        let text = crate::schematic::build_sheet_schematic(
            &b,
            &project,
            &root,
            &crate::schematic::SheetRender {
                name: None,
                uuid: derive_entity_uuid(&project, "sheet", "root"),
                project_name: &b.name,
                symbol_path: None,
                members: None,
                power_drivers: None,
                root_furniture: None,
            },
            &mut crate::schematic::PowerRefAllocator::default(),
        )
        .to_string_pretty();
        assert!(
            text.contains("(label"),
            "root cross-sheet endpoint must carry a local label:\n{text}"
        );
        assert!(text.contains("\"SIG\""));
        assert_eq!(
            text.matches("hierarchical_label").count(),
            0,
            "root has no parent sheet pin, so it uses a local label"
        );
    }
}
