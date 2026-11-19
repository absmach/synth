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

/// A name long enough to make a real file name, not a special case.
const LONG_NAME_LEN: usize = 120;
/// Parts per explicit sheet in the collision, long-name and mixed cases.
const SHEET_PARTS: usize = 8;

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
        gate: None,
        pwm: None,
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
        labels: false,
        keepouts: Vec::new(),
        no_connects: Vec::new(),
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
///
/// For six groups or fewer, the groups after the first carry sixteen two-pin
/// parts, so the design really spills the A4 page `hierarchy` asks for; larger
/// counts spill on their own. With only 1-3 small parts per group even six
/// groups tile one page and there is no hierarchy to number.
fn grouped(name: &str, groups: usize) -> Board {
    grouped_with(name, groups, true)
}

/// [`grouped`] without the extra parts: a small board that fits one page.
fn plain_grouped(name: &str, groups: usize) -> Board {
    grouped_with(name, groups, false)
}

fn grouped_with(name: &str, groups: usize, dense: bool) -> Board {
    let mut specs = Vec::new();
    let mut links = Vec::new();
    let mut next = 0u32;
    let mut first = Vec::new();
    let mut last = Vec::new();
    for g in 0..groups {
        // With 1-3 parts per group, three to six groups still fit one A4
        // page, so the first few fixtures need groups big enough to spill it
        // (16 per group is the smallest count measured to overflow at N=3).
        // The first group merges onto the root; keep it small so the root
        // still fits A4.
        let count = if dense && groups <= 6 {
            if g == 0 { 1 } else { 16 }
        } else {
            1 + g % 3
        };
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
    root_path: PathBuf,
    root: String,
    project: String,
}

impl Drop for Export {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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
    let root_path = dir.join(format!("{stem}.kicad_sch"));
    let root = std::fs::read_to_string(&root_path).unwrap();
    let project = std::fs::read_to_string(dir.join(format!("{stem}.kicad_pro"))).unwrap();
    Export {
        dir,
        root_path,
        root,
        project,
    }
}

fn first_quoted(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    Some(&s[start..start + s[start..].find('"')?])
}

fn quoted_after<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    first_quoted(s.split(key).nth(1)?)
}

fn two_numbers(s: &str) -> Option<(f64, f64)> {
    let mut numbers = s
        .split(|c: char| c == ')' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>);
    Some((numbers.next()?.ok()?, numbers.next()?.ok()?))
}

fn parse_xy(s: &str, key: &str) -> Option<(f64, f64)> {
    two_numbers(s.split(key).nth(1)?)
}

fn paper(root: &str) -> (String, f64, f64) {
    let line = root
        .lines()
        .find(|line| line.contains("(paper"))
        .expect("the root declares a paper");
    let name = quoted_after(line, "(paper").expect("the paper has a name");
    if name == "User" {
        let (width, height) = two_numbers(line.split("\"User\"").nth(1).expect("custom size"))
            .expect("two custom dimensions");
        (name.to_string(), width, height)
    } else {
        let size = match name {
            "A5" => SheetSize::A5,
            "A4" => SheetSize::A4,
            "A3" => SheetSize::A3,
            "A2" => SheetSize::A2,
            "A1" => SheetSize::A1,
            "A0" => SheetSize::A0,
            other => panic!("unknown paper {other}"),
        };
        let (width, height) = size.dims_mm();
        (name.to_string(), width, height)
    }
}

/// One `(sheet …)` block from the root as `(text, x, y, width, height)`.
fn sheet_blocks(root: &str) -> Vec<(&str, f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for chunk in root.split("\n\t(sheet\n").skip(1) {
        let block = &chunk[..chunk.find("\n\t)").expect("sheet block closes")];
        let (x, y) = parse_xy(block, "(at ").expect("sheet has a position");
        let (w, h) = parse_xy(block, "(size ").expect("sheet has a size");
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
        .map(|chunk| first_quoted(chunk).expect("Sheetfile value").to_string())
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
    quoted_after(block, "\"Sheetname\"")
        .expect("sheet has a name")
        .to_string()
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
        for _ in 0..SHEET_PARTS {
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
fn a_single_sheet_root_has_no_sheet_instances_block() {
    let board = plain_grouped("single", 2);
    let out = export(&board);
    // Two groups still fit one page, so there is no hierarchy root.
    assert!(pages(&out.root).is_empty());
    assert!(
        !out.root.contains("(sheet_instances"),
        "a single-sheet export grew a root instance block"
    );
}

#[test]
fn each_sub_sheet_page_is_keyed_to_its_parent_instance() {
    let board = grouped("pagepaths", 25);
    let out = export(&board);
    let root_uuid = quoted_after(&out.root, "(uuid ").expect("the root has a uuid");
    for (block, ..) in sheet_blocks(&out.root) {
        let path = quoted_after(block, "(path").expect("a sheet instance path");
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
    for _ in 0..SHEET_PARTS {
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
fn the_root_page_is_the_smallest_ladder_page_that_holds_the_rows() {
    let cases = [
        (3, SheetSize::A4, 1),
        (6, SheetSize::A3, 2),
        (12, SheetSize::A2, 3),
        (13, SheetSize::A2, 3),
        (25, SheetSize::A1, 4),
        (60, SheetSize::A0, 6),
    ];
    for (n, page, rows) in cases {
        let board = grouped(&format!("pages{n}"), n);
        let out = export(&board);
        let pages = pages(&out.root);
        assert_eq!(
            pages,
            (2..=n as u32).collect::<Vec<_>>(),
            "N={n} pages: {pages:?}"
        );
        assert!(
            out.root.contains(&format!("(paper \"{page:?}\")")),
            "N={n} root is not {page:?}: {}",
            out.root
                .lines()
                .find(|l| l.contains("(paper"))
                .unwrap_or("no paper")
        );
        let boxes = sheet_blocks(&out.root);
        assert_eq!(boxes.len(), n - 1, "N={n} boxes: {}", boxes.len());
        let actual_rows = 1 + boxes.windows(2).filter(|w| w[0].2 != w[1].2).count();
        assert_eq!(actual_rows, rows, "N={n} rows");
        let (w, h) = page.dims_mm();
        assert_boxes_fit(&boxes, w, h);
        assert_boxes_do_not_overlap(&boxes);
        assert_pages_follow_reading_order(&boxes, &pages);
    }
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
fn a_large_first_group_merges_onto_the_root() {
    // Sixteen parts in the first group make its tiled box large; it still
    // moves onto the root, and the first sub-sheet keeps page 2.
    let mut specs = Vec::new();
    let mut links = Vec::new();
    for g in 0..3 {
        let start = specs.len() as u32;
        for _ in 0..16 {
            specs.push(Spec::group(&format!("G{g:02}"), 2));
        }
        for part in start..start + 11 {
            links.push((part, 1, part + 1, 0, format!("G{g:02}_LOCAL")));
        }
        if start > 0 {
            links.push((start - 1, 1, start, 0, format!("LINK_{g:02}")));
        }
    }
    let board = build("largefirst", &specs, &links, None);
    let out = export(&board);
    assert_eq!(pages(&out.root), vec![2, 3]);
    assert!(out.root.contains("\"R0\""), "the first group left the root");
    assert!(
        out.root.contains("\"R15\""),
        "the whole first group is on the root"
    );
    assert!(!out.dir.join("largefirst_G00.kicad_sch").exists());
    let boxes = sheet_blocks(&out.root);
    let (_, width, height) = paper(&out.root);
    assert_boxes_fit(&boxes, width, height);
    assert_boxes_do_not_overlap(&boxes);
    assert_pages_follow_reading_order(&boxes, &pages(&out.root));
}

#[test]
fn a_sheet_with_no_cross_nets_keeps_its_page() {
    let specs = (0..4)
        .flat_map(|g| (0..12).map(move |_| Spec::group(&format!("G{g:02}"), 2)))
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
    let mut specs = Vec::new();
    let mut linked = Vec::new();
    for g in 0..4 {
        linked.push(specs.len() as u32);
        for _ in 0..8 {
            specs.push(Spec::group(&format!("G{g:02}"), 20));
        }
    }
    let mut links = Vec::new();
    for pin in 0..20 {
        links.push((linked[0], pin, linked[1], pin, format!("CROSS_{pin:02}")));
    }
    links.push((linked[1], 19, linked[2], 0, "L01".to_string()));
    links.push((linked[2], 19, linked[3], 0, "L12".to_string()));
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
        let (px, py) = parse_xy(pin, "(at ").expect("a pin has a position");
        assert!(
            px >= box_x && px <= box_x + box_w,
            "pin {px} is outside the box horizontally"
        );
        assert!(
            (py - (box_y + box_h)).abs() < 0.01,
            "pin at y={py} is not on the box bottom edge {}",
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
        for _ in 0..SHEET_PARTS {
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
fn a_requested_a4_page_grows_for_a_many_sheet_root() {
    let mut board = grouped("a4many", 25);
    board.schematic_paper = Some(SchematicPaper::A4);
    let out = export(&board);
    assert!(
        out.root.contains("(paper \"A1\")"),
        "24 boxes need A1; A4 is a floor, not a cap: {}",
        out.root
            .lines()
            .find(|l| l.contains("(paper"))
            .unwrap_or("no paper")
    );
    let (_, width, height) = paper(&out.root);
    assert_boxes_fit(&sheet_blocks(&out.root), width, height);
}

#[test]
fn non_ascii_names_that_sanitise_alike_are_disambiguated() {
    let names = ["电路板电源", "电路板地线", "Z"];
    let mut specs = Vec::new();
    for name in names {
        for _ in 0..SHEET_PARTS {
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
fn a_long_sheet_name_is_kept_in_a_real_file_name() {
    let long = "x".repeat(LONG_NAME_LEN);
    let mut specs = Vec::new();
    for name in [long.as_str(), "Z"] {
        for _ in 0..SHEET_PARTS {
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
    assert!(
        files
            .iter()
            .any(|file| file.contains(&"x".repeat(LONG_NAME_LEN)))
    );
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

/// Once the rows no longer fit A0, the root page has to grow past the
/// ladder. It keeps A0 as the wrap width and the height becomes a custom
/// page; everything still fits and pages stay in reading order.
#[test]
fn past_a0_the_root_page_grows_past_the_ladder() {
    let board = grouped("pages101", 101);
    let out = export(&board);
    let (name, width, height) = paper(&out.root);
    assert_eq!(name, "User", "past A0 the root page is custom");
    assert!(height > SheetSize::A0.dims_mm().1, "the page did not grow");
    let pages = pages(&out.root);
    assert_eq!(pages, (2..=101).collect::<Vec<_>>(), "pages: {pages:?}");
    let boxes = sheet_blocks(&out.root);
    assert_eq!(boxes.len(), 100);
    assert_boxes_fit(&boxes, width, height);
    assert_boxes_do_not_overlap(&boxes);
    assert_pages_follow_reading_order(&boxes, &pages);
}

#[test]
fn kicad_reads_twelve_pages_in_page_order_and_erc_is_clean() {
    let kicad = std::process::Command::new("kicad-cli")
        .arg("version")
        .output();
    if !kicad.is_ok_and(|out| out.status.success()) {
        eprintln!("kicad-cli not installed; skipping the rendered page-order check");
        return;
    }
    if std::process::Command::new("pdftotext")
        .arg("-v")
        .output()
        .is_err()
    {
        eprintln!("pdftotext not installed; skipping the rendered page-order check");
        return;
    }

    // No tall ballast: its 40-pin parts leave pins unconnected, and this
    // test is the one that checks the rendered hierarchy's ERC. Twelve
    // groups already spill the page, so the hierarchy exists anyway.
    let board = plain_grouped("pdforder12", 12);
    let out = export(&board);
    let pdf = out.dir.join("order.pdf");
    let status = std::process::Command::new("kicad-cli")
        .args(["sch", "export", "pdf", "-o"])
        .arg(&pdf)
        .arg(&out.root_path)
        .output()
        .expect("kicad-cli runs");
    assert!(
        status.status.success(),
        "kicad-cli could not export the PDF"
    );

    let text = std::process::Command::new("pdftotext")
        .arg("-layout")
        .arg(&pdf)
        .arg("-")
        .output()
        .expect("pdftotext runs");
    let text = String::from_utf8_lossy(&text.stdout);
    let pages: Vec<&str> = text
        .split('\u{0c}')
        .filter(|page| page.contains("Sheet: /"))
        .collect();
    assert_eq!(pages.len(), 12, "the PDF has the wrong number of pages");
    for (index, page) in pages.iter().enumerate() {
        let sheet = if index == 0 {
            "/".to_string()
        } else {
            format!("/G{index:02}/")
        };
        assert!(
            page.contains(&format!("Sheet: {sheet}")),
            "PDF page {} is not {sheet}:\n{page}",
            index + 1
        );
        assert!(
            page.contains(&format!("Id: {}/12", index + 1)),
            "PDF page {} does not carry Id: {}/12:\n{page}",
            index + 1,
            index + 1
        );
    }

    let erc = synth_kicad::run_kicad_erc(&out.root_path);
    match erc.evidence.reason {
        Some(synth_diagnostics::UnknownReason::NotInstalled) => {
            eprintln!("kicad-cli not installed; skipping the ERC half");
        }
        Some(_) => panic!(
            "ERC produced no usable evidence: {}",
            erc.evidence.summary_line()
        ),
        None => assert!(
            erc.errors().next().is_none(),
            "twelve-sheet ERC errors: {:?}",
            erc.errors().collect::<Vec<_>>()
        ),
    }
}
