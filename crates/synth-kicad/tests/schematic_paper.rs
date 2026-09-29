//! The page a design is drawn on is the one the author asked for.
//!
//! `schematic { paper = "…" }` is easy to add and easy to break silently:
//! the layout can compact, grow, or re-fit the page after the setting is
//! read, and the only way to notice is to look at the emitted
//! `(paper …)` sexp. These tests go through the real export so a regression
//! in any downstream pass shows up here.

use std::path::{Path, PathBuf};
use synth_ir::SchematicPaper;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn tempdir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("synth-paper-{label}-{}", std::process::id()));
    if p.exists() {
        let _ = std::fs::remove_dir_all(&p);
    }
    p
}

fn fixture_source() -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("fixtures")
            .join("kicad-reference")
            .join("ref_battery_charger.synth"),
    )
    .expect("reference fixture must exist")
}

fn load_board(src: &str) -> synth_ir::Board {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let filename = "paper.synth".to_string();
    let parsed = synth_parser::parse(src, filename.clone());
    assert!(
        parsed.diagnostics.is_empty(),
        "fixture must parse: {:?}",
        parsed.diagnostics
    );
    synth_ir::lower(&parsed.ast.expect("ast"), &registry, &filename)
        .board
        .expect("board")
}

fn exported_paper(src: &str) -> String {
    let board = load_board(src);
    let dir = tempdir("export");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out");
    // No sidecar: this is about the page the design asks for, not about a
    // hand-tuned layout overriding it.
    synth_kicad::export_schematic_only(&board, &out, None).expect("schematic export must succeed");
    let mut sheets: Vec<PathBuf> = std::fs::read_dir(&out)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_sch"))
        .collect();
    sheets.sort();
    assert!(!sheets.is_empty(), "export must produce a schematic");
    let sch = std::fs::read_to_string(&sheets[0]).expect("exported schematic must be readable");
    let paper_line = sch
        .lines()
        .find(|l| l.contains("(paper"))
        .expect("exported schematic must declare a paper");
    paper_line
        .trim()
        .trim_start_matches("(paper ")
        .trim_end_matches(')')
        .trim_matches('"')
        .to_string()
}

fn with_paper(paper: Option<&str>) -> String {
    let block = paper.map_or(String::new(), |p| {
        format!("  schematic {{ paper = \"{p}\" }}\n")
    });
    fixture_source().replacen("  layers 2\n", &format!("  layers 2\n{block}"), 1)
}

/// A design small enough that every page in the ladder holds it.
fn tiny_source(paper: &str) -> String {
    format!(
        r#"board "tiny" {{
  layers 2
  schematic {{ paper = "{paper}" }}
  component R1: resistor "r_generic_0603"
  component C1: capacitor "c_generic_0603" value "100nF"
  connect R1.p1 -> C1.p1
  connect R1.p2 -> C1.p2
}}"#
    )
}

#[test]
fn every_standard_paper_reaches_the_exported_schematic() {
    for want in ["A5", "A4", "A3", "A2", "A1", "A0"] {
        assert_eq!(
            exported_paper(&tiny_source(want)),
            want,
            "schematic {{ paper = \"{want}\" }} must be honoured end to end"
        );
    }
}

#[test]
fn a_page_too_small_for_the_content_grows_instead_of_clipping() {
    // The reference fixture does not fit A5, and the contract is that
    // overflow enlarges the page rather than dropping parts off the edge or
    // blocking the export. A4 is the next rung up.
    assert_eq!(exported_paper(&with_paper(Some("A5"))), "A4");
}

#[test]
fn a_design_that_asks_for_nothing_is_drawn_on_a4() {
    assert_eq!(exported_paper(&with_paper(None)), "A4");
}

#[test]
fn the_request_survives_lowering() {
    for (text, want) in [
        ("A5", SchematicPaper::A5),
        ("A4", SchematicPaper::A4),
        ("A3", SchematicPaper::A3),
        ("A2", SchematicPaper::A2),
    ] {
        let board = load_board(&with_paper(Some(text)));
        assert_eq!(
            board.schematic_paper,
            Some(want),
            "lowering lost paper = {text}"
        );
    }
    assert_eq!(load_board(&with_paper(None)).schematic_paper, None);
}

#[test]
fn the_overflow_policy_survives_lowering() {
    for (text, want) in [
        ("grow", synth_ir::SchematicOverflow::Grow),
        ("hierarchy", synth_ir::SchematicOverflow::Hierarchy),
    ] {
        let src = with_paper(Some("A4")).replace(
            "  schematic { paper = \"A4\" }\n",
            &format!("  schematic {{ paper = \"A4\" overflow = \"{text}\" }}\n"),
        );
        let board = load_board(&src);
        assert_eq!(board.schematic_overflow, Some(want), "{text}");
    }
    // Omitting the block leaves both settings to their defaults, which the
    // layout resolves to A4 + Grow.
    let board = load_board(&with_paper(None));
    assert_eq!(board.schematic_paper, None);
    assert_eq!(board.schematic_overflow, None);
    assert_eq!(
        synth_layout::requested_sheet(&board),
        synth_layout::SheetSize::A4
    );
    assert_eq!(
        synth_layout::max_single_sheet(&board),
        synth_layout::SheetSize::A0
    );
}
