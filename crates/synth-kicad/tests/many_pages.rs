//! Many-sheet schematics: page numbers, the root sheet-symbol row, and
//! sub-sheet file names.
//!
//! These go through the real export and read the emitted files, because
//! every number under test (the page in each sheet instance, the paper
//! size, the file table in the project) is only observable there.

use std::collections::BTreeMap;
use std::path::PathBuf;

use synth_diagnostics::Span;
use synth_ir::PinId;
use synth_ir::{
    Board, Component, ComponentId, Group, Net, NetEndpoint, NetId, SchematicOverflow,
    SchematicPaper,
};
use synth_layout::SheetSize;
use synth_registry::{ElectricalType, Lifecycle, Part, PartId, Pin as RegPin, PinNumber};

struct Spec {
    group: Option<String>,
    sheet: Option<String>,
    pins: usize,
}

impl Spec {
    fn group(group: &str, pins: usize) -> Self {
        Self {
            group: Some(group.to_string()),
            sheet: None,
            pins,
        }
    }

    fn sheet(sheet: &str, pins: usize) -> Self {
        Self {
            group: None,
            sheet: Some(sheet.to_string()),
            pins,
        }
    }

    fn root(pins: usize) -> Self {
        Self {
            group: None,
            sheet: None,
            pins,
        }
    }
}

fn reg_pin(name: &str) -> RegPin {
    RegPin {
        name: name.to_string(),
        number: PinNumber(name.to_string()),
        electrical_type: ElectricalType::Passive,
        capabilities: Vec::new(),
        required: false,
        footprint_only: false,
        unit: None,
        voltage_max_v: None,
        voltage_min_v: None,
        voltage_nominal_v: None,
        drive: None,
    }
}

fn part(pins: usize) -> Part {
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
        pins: (0..pins).map(|i| reg_pin(&format!("p{i}"))).collect(),
        required_decoupling: Vec::new(),
        kicad_symbol: None,
        kicad_footprint: None,
        footprint_dimensions: None,
        operating_conditions: None,
        spice: None,
    }
}

fn endpoint(component: u32, pin: u32) -> NetEndpoint {
    NetEndpoint {
        component: ComponentId(component),
        pin: PinId(pin),
        source_span: Span::new(0, 0),
    }
}

fn build(
    name: &str,
    specs: &[Spec],
    links: &[(u32, u32, u32, u32, String)],
    paper: Option<SchematicPaper>,
) -> Board {
    let components = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| Component {
            id: ComponentId(i as u32),
            refdes: format!("R{i}"),
            kind: "resistor".to_string(),
            part: Some(part(spec.pins)),
            value: None,
            dnp: false,
            properties: BTreeMap::new(),
            placement_hint: None,
            escape: None,
            group: spec.group.clone(),
            sheet: spec.sheet.clone(),
            source_span: Span::new(0, 0),
        })
        .collect();
    let nets = links
        .iter()
        .enumerate()
        .map(|(i, (a, pa, b, pb, name))| Net {
            id: NetId(i as u32),
            name: name.clone(),
            auto_named: false,
            endpoints: vec![endpoint(*a, *pa), endpoint(*b, *pb)],
            netclass: None,
            voltage: None,
        })
        .collect();
    Board {
        outline: None,
        schematic_overflow: Some(SchematicOverflow::Hierarchy),
        schematic_paper: paper,
        groups: Vec::new(),
        legends: false,
        name: name.to_string(),
        layers: 2,
        manufacturer: None,
        revision: None,
        company: None,
        components,
        nets,
        diff_pairs: Vec::new(),
        notes: Vec::new(),
        keepouts: Vec::new(),
        stackup: None,
        netclasses: vec![],
        buses: vec![],
        modules: vec![],
        variants: vec![],
        required_processes: std::collections::BTreeSet::new(),
        source_span: Span::new(0, 0),
    }
}

/// `groups` groups, each 1-3 two-pin parts, chained by one cross-sheet net
/// to the next group. A groups-only split puts the first group on the root.
fn grouped(name: &str, groups: usize) -> Board {
    let mut specs = Vec::new();
    let mut links = Vec::new();
    let mut next = 0u32;
    let mut first = Vec::new();
    let mut last = Vec::new();
    for g in 0..groups {
        let count = 1 + g % 3;
        let start = next;
        for _ in 0..count {
            specs.push(Spec::group(&format!("G{g:02}"), 2));
            next += 1;
        }
        first.push(start);
        last.push(next - 1);
        for pair in start..(next - 1) {
            links.push((pair, 1, pair + 1, 0, format!("G{g:02}_LOCAL")));
        }
    }
    for g in 0..groups.saturating_sub(1) {
        links.push((last[g], 1, first[g + 1], 0, format!("LINK_{g:02}")));
    }
    build(name, &specs, &links, None)
}

struct Export {
    dir: PathBuf,
    root: String,
    project: String,
}

fn export(board: &Board) -> Export {
    let dir = std::env::temp_dir().join(format!(
        "synth-many-pages-{}-{}",
        std::process::id(),
        board.name
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    synth_kicad::export_schematic_only(board, &dir, None).expect("schematic export");
    let stem = &board.name;
    let root = std::fs::read_to_string(dir.join(format!("{stem}.kicad_sch"))).unwrap();
    let project = std::fs::read_to_string(dir.join(format!("{stem}.kicad_pro"))).unwrap();
    Export { dir, root, project }
}

/// One `(sheet …)` block from the root as `(text, x, y, width, height)`.
fn sheet_blocks(root: &str) -> Vec<(&str, f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for chunk in root.split("\n\t(sheet\n").skip(1) {
        let block = &chunk[..chunk.find("\n\t)").expect("sheet block closes")];
        let at = block
            .split("(at ")
            .nth(1)
            .and_then(|r| r.split(')').next())
            .expect("sheet has a position");
        let size = block
            .split("(size ")
            .nth(1)
            .and_then(|r| r.split(')').next())
            .expect("sheet has a size");
        let nums = |s: &str| -> Vec<f64> {
            s.split_whitespace()
                .take(2)
                .map(|v| v.parse().unwrap())
                .collect()
        };
        let (x, y) = {
            let n = nums(at);
            (n[0], n[1])
        };
        let (w, h) = {
            let n = nums(size);
            (n[0], n[1])
        };
        out.push((block, x, y, w, h));
    }
    out
}

fn pages(root: &str) -> Vec<u32> {
    sheet_blocks(root)
        .iter()
        .map(|(block, ..)| {
            let page = block
                .split("(page \"")
                .nth(1)
                .expect("a sheet instance carries a page");
            page[..page.find('"').unwrap()]
                .parse()
                .expect("page number is an integer")
        })
        .collect()
}

fn sheet_files(root: &str) -> Vec<String> {
    root.split("\"Sheetfile\"")
        .skip(1)
        .map(|r| {
            let rest = r.trim_start();
            let start = rest.find('"').unwrap() + 1;
            rest[start..start + rest[start..].find('"').unwrap()].to_string()
        })
        .collect()
}

fn project_files(project: &str) -> Vec<String> {
    let doc: serde_json::Value = serde_json::from_str(project).unwrap();
    doc["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry[1].as_str())
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

fn unique(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

fn sheet_name(block: &str) -> String {
    let rest = block
        .split("\"Sheetname\"")
        .nth(1)
        .expect("sheet has a name")
        .trim_start();
    let start = rest.find('"').unwrap() + 1;
    rest[start..start + rest[start..].find('"').unwrap()].to_string()
}

fn assert_boxes_fit(boxes: &[(&str, f64, f64, f64, f64)], width: f64, height: f64) {
    assert!(
        boxes
            .iter()
            .all(|b| b.1 + b.3 <= width && b.2 + b.4 <= height),
        "a sheet box is past the {width} x {height} border: {boxes:?}"
    );
}

fn assert_boxes_do_not_overlap(boxes: &[(&str, f64, f64, f64, f64)]) {
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let overlap = a.1 < b.1 + b.3 && b.1 < a.1 + a.3 && a.2 < b.2 + b.4 && b.2 < a.2 + a.4;
            assert!(
                !overlap,
                "sheet boxes {} and {} overlap: {a:?} {b:?}",
                sheet_name(a.0),
                sheet_name(b.0)
            );
        }
    }
}

fn assert_pages_follow_reading_order(boxes: &[(&str, f64, f64, f64, f64)], pages: &[u32]) {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| {
        boxes[a]
            .2
            .total_cmp(&boxes[b].2)
            .then(boxes[a].1.total_cmp(&boxes[b].1))
    });
    for (rank, &index) in order.iter().enumerate() {
        assert_eq!(
            pages[index],
            rank as u32 + 2,
            "page {} is not the {}th sheet read left to right, top to bottom",
            pages[index],
            rank + 1
        );
    }
}

#[test]
fn colliding_sheet_names_get_distinct_project_entries() {
    let names = ["A / B", "A + B", "A_B_2", "Z"];
    let mut specs = Vec::new();
    for name in names {
        for _ in 0..8 {
            specs.push(Spec::sheet(name, 2));
        }
    }
    let board = build("collide3", &specs, &[], None);
    let out = export(&board);

    let files = sheet_files(&out.root);
    assert_eq!(files.len(), names.len(), "one Sheetfile per sub-sheet");
    assert_eq!(
        unique(files.clone()).len(),
        files.len(),
        "the root points two instances at one file: {files:?}"
    );
    for file in &files {
        assert!(
            out.dir.join(file).exists(),
            "the root names a file the export did not write: {file}"
        );
    }

    let project = project_files(&out.project);
    assert_eq!(
        unique(project.clone()).len(),
        project.len(),
        "the project's sheet table has duplicate file names: {project:?}"
    );
    assert_eq!(
        unique(project.clone()),
        unique(files.clone()),
        "the project table and the root disagree about the sheet files"
    );
}

#[test]
fn the_root_declares_its_own_page() {
    let board = grouped("rootpage", 3);
    let out = export(&board);
    assert!(
        out.root.contains("(sheet_instances"),
        "the root has no sheet_instances block:\n{}",
        &out.root[..out.root.len().min(400)]
    );
    let root_page = out
        .root
        .split("(sheet_instances")
        .nth(1)
        .expect("the root has no sheet_instances block");
    assert!(
        root_page.contains("\"/\"") && root_page.contains("(page \"1\")"),
        "the root does not declare itself as page 1: {root_page}"
    );
}

#[test]
fn each_sub_sheet_page_is_keyed_to_its_parent_instance() {
    let board = grouped("pagepaths", 25);
    let out = export(&board);
    let root_uuid = out
        .root
        .split("(uuid \"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("the root has a uuid");
    for (block, ..) in sheet_blocks(&out.root) {
        let after = block.split("(path").nth(1).expect("a sheet instance path");
        let start = after.find('"').expect("the path is quoted") + 1;
        let path = &after[start..start + after[start..].find('"').unwrap()];
        assert_eq!(
            path,
            format!("/{root_uuid}"),
            "sheet {} is keyed to {path}, so KiCad ignores its page number",
            sheet_name(block)
        );
    }
}

#[test]
fn a_hierarchy_with_one_sub_sheet_keeps_page_two() {
    // 40-pin parts are tall enough that one explicit sheet of them no longer
    // fits the requested A4 page, so the design really splits.
    let mut specs = vec![Spec::root(40)];
    for _ in 0..8 {
        specs.push(Spec::sheet("Only", 40));
    }
    let board = build("onesub", &specs, &[], None);
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2]);
    let boxes = sheet_blocks(&out.root);
    assert_eq!(boxes.len(), 1);
    assert_eq!(sheet_name(boxes[0].0), "Only");
}

#[test]
fn pages_are_unique_and_contiguous_at_twelve_sheets() {
    let board = grouped("pages12", 12);
    let out = export(&board);
    let pages = pages(&out.root);
    assert_eq!(pages, (2..=12).collect::<Vec<_>>(), "pages: {pages:?}");
    assert!(
        out.root.contains("(paper \"A0\")"),
        "root page grew or shrank"
    );
    let boxes = sheet_blocks(&out.root);
    assert_eq!(boxes.len(), 11);
    assert!(
        boxes.windows(2).all(|w| w[0].1 < w[1].1),
        "the boxes are not laid out left to right"
    );
    assert_boxes_fit(&boxes, SheetSize::A0.dims_mm().0, SheetSize::A0.dims_mm().1);
    assert_boxes_do_not_overlap(&boxes);
}

#[test]
fn pages_are_unique_and_contiguous_at_twenty_five_sheets() {
    let board = grouped("pages25", 25);
    let out = export(&board);
    let pages = pages(&out.root);
    assert_eq!(pages, (2..=25).collect::<Vec<_>>(), "pages: {pages:?}");
    assert!(
        !out.root.contains("(paper \"User\""),
        "the root row invented a custom page wider than A0"
    );
    let boxes = sheet_blocks(&out.root);
    assert!(
        boxes
            .iter()
            .map(|b| b.2)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w[0] != w[1]),
        "24 sheet symbols are still one unwrapped row"
    );
    assert_boxes_fit(&boxes, SheetSize::A0.dims_mm().0, SheetSize::A0.dims_mm().1);
    assert_boxes_do_not_overlap(&boxes);
    assert_pages_follow_reading_order(&boxes, &pages);
}

#[test]
fn pages_are_unique_and_contiguous_at_sixty_sheets() {
    let board = grouped("pages60", 60);
    let out = export(&board);
    let pages = pages(&out.root);
    assert_eq!(pages, (2..=60).collect::<Vec<_>>(), "pages: {pages:?}");
    assert!(
        !out.root.contains("(paper \"User\""),
        "the root row invented a custom page wider than A0"
    );
    let boxes = sheet_blocks(&out.root);
    assert!(
        boxes
            .iter()
            .map(|b| b.2)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w[0] != w[1]),
        "59 sheet symbols are still one unwrapped row"
    );
    assert_boxes_fit(&boxes, SheetSize::A0.dims_mm().0, SheetSize::A0.dims_mm().1);
    assert_boxes_do_not_overlap(&boxes);
    assert_pages_follow_reading_order(&boxes, &pages);
}

#[test]
fn the_first_group_on_the_root_keeps_page_two_for_the_first_sub_sheet() {
    let board = grouped("merged", 3);
    let out = export(&board);
    let pages = pages(&out.root);
    assert_eq!(
        pages,
        vec![2, 3],
        "the root absorbed a group and renumbered"
    );
    // The merged group's parts live on the root page, not in a sub-sheet.
    assert!(
        out.root.contains("\"R0\""),
        "the first group did not move onto the root"
    );
    assert!(!out.dir.join("merged_G00.kicad_sch").exists());
}

#[test]
fn a_sheet_with_no_cross_nets_keeps_its_page() {
    let specs = (0..4)
        .map(|g| Spec::group(&format!("G{g:02}"), 2))
        .collect::<Vec<_>>();
    let board = build("zeropins", &specs, &[], None);
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2, 3, 4]);
    for (block, ..) in sheet_blocks(&out.root) {
        assert!(
            !block.contains("(pin \""),
            "a sheet with no cross nets still carries a pin:\n{block}"
        );
    }
}

#[test]
fn many_cross_nets_stay_on_the_box_bottom_edge() {
    let specs = (0..4)
        .map(|g| Spec::group(&format!("G{g:02}"), 20))
        .collect::<Vec<_>>();
    let mut links = Vec::new();
    for pin in 0..20 {
        links.push((0, pin, 1, pin, format!("CROSS_{pin:02}")));
    }
    links.push((1, 19, 2, 0, "L01".to_string()));
    links.push((2, 19, 3, 0, "L12".to_string()));
    let board = build("manypins", &specs, &links, None);
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2, 3, 4]);

    let boxes = sheet_blocks(&out.root);
    let (block, box_x, box_y, box_w, box_h) = boxes
        .iter()
        .find(|(block, ..)| sheet_name(block) == "G01")
        .copied()
        .expect("the G01 instance");
    assert!(box_w > 80.0, "20 pins did not widen the box: {box_w}");
    let pins: Vec<&str> = block.split("\n\t\t(pin\n").skip(1).collect();
    assert_eq!(pins.len(), 21, "20 cross nets plus the link to G02");
    for pin in 0..20 {
        assert!(
            block.contains(&format!("\"CROSS_{pin:02}\"")),
            "cross net CROSS_{pin:02} has no sheet pin"
        );
    }
    for pin in pins {
        let at = pin
            .split("(at ")
            .nth(1)
            .and_then(|r| r.split(')').next())
            .expect("a pin has a position");
        let pos: Vec<f64> = at
            .split_whitespace()
            .take(2)
            .map(|v| v.parse().unwrap())
            .collect();
        assert!(
            pos[0] >= box_x && pos[0] <= box_x + box_w,
            "pin {} is outside the box horizontally",
            pos[0]
        );
        assert!(
            (pos[1] - (box_y + box_h)).abs() < 0.01,
            "pin at y={} is not on the box bottom edge {}",
            pos[1],
            box_y + box_h
        );
    }
    assert_boxes_fit(&boxes, SheetSize::A0.dims_mm().0, SheetSize::A0.dims_mm().1);
    assert_boxes_do_not_overlap(&boxes);
}

#[test]
fn explicit_sheets_and_groups_mix_keep_contiguous_pages() {
    let mut specs = vec![Spec::root(2)];
    for sheet in ["S1", "S2", "S3"] {
        for _ in 0..8 {
            specs.push(Spec::sheet(sheet, 2));
        }
    }
    for _ in 0..4 {
        specs.push(Spec::group("G", 2));
    }
    let board = build("mixed", &specs, &[], None);
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2, 3, 4, 5]);
    assert_eq!(
        sheet_blocks(&out.root)
            .iter()
            .map(|(block, ..)| sheet_name(block))
            .collect::<Vec<_>>(),
        ["S1", "S2", "S3", "G"]
    );
}

#[test]
fn a_requested_a4_page_is_a_floor() {
    let mut board = grouped("a4forced", 3);
    board.schematic_paper = Some(SchematicPaper::A4);
    let out = export(&board);
    assert!(out.root.contains("(paper \"A4\")"), "A4 was not honoured");
    assert_eq!(pages(&out.root), vec![2, 3]);
    let boxes = sheet_blocks(&out.root);
    assert_boxes_fit(&boxes, SheetSize::A4.dims_mm().0, SheetSize::A4.dims_mm().1);
}

#[test]
fn non_ascii_names_that_sanitise_alike_are_disambiguated() {
    let names = ["电路板电源", "电路板地线", "Z"];
    let mut specs = Vec::new();
    for name in names {
        for _ in 0..8 {
            specs.push(Spec::sheet(name, 2));
        }
    }
    let board = build("cjknames", &specs, &[], None);
    let out = export(&board);
    let files = sheet_files(&out.root);
    assert_eq!(unique(files.clone()).len(), files.len(), "{files:?}");
    for file in &files {
        assert!(out.dir.join(file).exists(), "{file} was not written");
    }
    assert_eq!(
        unique(project_files(&out.project)),
        unique(files),
        "the project table disagrees with the root"
    );
}

#[test]
fn a_very_long_name_is_sanitized_into_a_real_file() {
    let long = "x".repeat(120);
    let mut specs = Vec::new();
    for name in [long.as_str(), "Z"] {
        for _ in 0..8 {
            specs.push(Spec::sheet(name, 2));
        }
    }
    let board = build("longname", &specs, &[], None);
    let out = export(&board);
    let files = sheet_files(&out.root);
    assert_eq!(files.len(), 2);
    for file in &files {
        assert!(out.dir.join(file).exists(), "{file} was not written");
    }
    assert!(files.iter().any(|file| file.contains(&"x".repeat(120))));
}

#[test]
fn an_empty_group_does_not_take_a_page() {
    let mut board = grouped("emptygroup", 3);
    board.groups.push(Group {
        name: "Empty".to_string(),
        title: None,
        color: None,
        region: None,
        source_span: Span::new(0, 0),
    });
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2, 3]);
    assert!(
        !sheet_files(&out.root)
            .iter()
            .any(|file| file.contains("Empty")),
        "an empty group got a file"
    );
}
