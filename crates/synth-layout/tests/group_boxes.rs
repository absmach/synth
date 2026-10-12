// SPDX-License-Identifier: Apache-2.0

//! Group boxes on a real design: boxes stay apart, notes fit inside their
//! box, and part bodies keep label room to the box's side edges.

use std::path::{Path, PathBuf};

use synth_ir::Board;
use synth_layout::{body_size_for_part, GroupBox, Layout, TextKind};

/// Room a box must leave between a part body and its left/right edge, mm
/// (the requirement from the buck-board schematic review, kept independent
/// of the layout crate's own padding constant on purpose).
const LABEL_ROOM_MM: f64 = 6.0;
/// KiCad's stroke-font advance per character, in glyph heights; the same
/// ~0.72 em the layout crate estimates with, restated so the test can
/// disagree with it.
const ADVANCE_EM: f64 = 0.72;

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

fn contains(b: &GroupBox, (x, y): (f64, f64)) -> bool {
    x >= b.min_mm.0 && x <= b.max_mm.0 && y >= b.min_mm.1 && y <= b.max_mm.1
}

fn overlap(a: &GroupBox, b: &GroupBox) -> bool {
    a.min_mm.0 < b.max_mm.0
        && b.min_mm.0 < a.max_mm.0
        && a.min_mm.1 < b.max_mm.1
        && b.min_mm.1 < a.max_mm.1
}

fn env_logger() -> (Board, Layout) {
    let board = board_for("examples/env_logger.synth");
    let layout = synth_layout::layout(&board);
    assert!(layout.group_boxes.len() >= 3, "fixture must have 3 groups");
    (board, layout)
}

#[test]
fn group_boxes_do_not_overlap() {
    let (_, layout) = env_logger();
    for (i, a) in layout.group_boxes.iter().enumerate() {
        for b in &layout.group_boxes[i + 1..] {
            assert!(!overlap(a, b), "boxes overlap: {a:?} vs {b:?}");
        }
    }
}

#[test]
fn note_text_stays_inside_its_group_box() {
    let (board, layout) = env_logger();
    let expected: usize = board
        .notes
        .iter()
        .filter(|n| n.group.is_some())
        .map(|n| {
            usize::from(!n.title.is_empty()) + n.lines.iter().filter(|l| !l.is_empty()).count()
        })
        .sum();
    let mut checked = 0;
    for note in layout.annotations.iter().filter(|a| {
        matches!(a.kind, TextKind::NoteTitle | TextKind::NoteLine) && !a.text.is_empty()
    }) {
        let Some(group_box) = layout.group_boxes.iter().find(|b| contains(b, note.at_mm)) else {
            continue; // an ungrouped note, drawn below the content
        };
        #[allow(clippy::cast_precision_loss)] // a short note line, far below 2^52 characters
        let width = note.text.chars().count() as f64 * note.size_mm * ADVANCE_EM;
        assert!(
            note.at_mm.0 + width <= group_box.max_mm.0,
            "note {:?} runs past its box's right edge",
            note.text
        );
        assert!(
            note.at_mm.1 <= group_box.max_mm.1,
            "note {:?} sits below its box",
            note.text
        );
        checked += 1;
    }
    assert_eq!(
        checked, expected,
        "every group note line must sit inside a box"
    );
}

#[test]
fn part_bodies_keep_label_room_inside_their_box() {
    let (board, layout) = env_logger();
    let mut checked = 0;
    for placement in &layout.components {
        let component = board.component(placement.id).unwrap();
        let Some(group) = component.group.as_deref() else {
            continue;
        };
        let group_box = layout
            .group_boxes
            .iter()
            .find(|b| b.group == group)
            .unwrap_or_else(|| panic!("no box for group {group}"));
        let (body_w, _) = component
            .part
            .as_ref()
            .map_or((0.0, 0.0), body_size_for_part);
        let (x, _) = placement.center_mm;
        assert!(
            x - body_w / 2.0 - group_box.min_mm.0 >= LABEL_ROOM_MM - 1e-6,
            "{} too close to its box's left edge",
            component.refdes
        );
        assert!(
            group_box.max_mm.0 - (x + body_w / 2.0) >= LABEL_ROOM_MM - 1e-6,
            "{} too close to its box's right edge",
            component.refdes
        );
        checked += 1;
    }
    assert!(checked > 0, "no grouped part was checked");
}
