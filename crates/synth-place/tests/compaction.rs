//! The automatic placer must not leave wide empty bands between courtyards.

fn load_board(path: &str) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: std::path::PathBuf::from("../.."),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    synth_ir::lower(&resolved.program, &registry, &file)
        .board
        .expect("board")
}

/// `ref_usb_cdc` was 54.6 x 53.0 mm (2895 mm2) with a 3.0 mm gap between
/// courtyards, and 39.5 x 40.8 mm (1612 mm2) with a 1.5 mm gap around
/// passives (3.0 mm stays around macros). The bound sits between the two.
#[test]
fn usb_cdc_outline_stays_compact() {
    let board = load_board("../../fixtures/kicad-reference/ref_usb_cdc.synth");
    let placement = synth_place::place(&board).expect("place");
    let area_mm2 = synth_geometry::nm_to_mm(placement.board_outline.width_nm())
        * synth_geometry::nm_to_mm(placement.board_outline.height_nm());
    assert!(
        area_mm2 < 2000.0,
        "outline is {area_mm2:.0} mm2, expected under 2000"
    );
}

/// A small board takes the tight 1.0 mm rim, not the 2.0 mm one it used
/// before. This fixture is under the 18-part dense tier and its `place()`
/// outline was 16.78 x 16.68 mm with a 2.0 mm rim, so 1 mm less per side
/// means neither dimension may exceed the old one minus 2 mm.
#[test]
fn a_small_board_fits_a_tighter_outline_than_the_old_rim() {
    let board = load_board("../../fixtures/layout/divider.synth");
    let mut placement = synth_place::place(&board).expect("place");
    synth_place::fit_outline_with_margin_mm(
        &board,
        &mut placement,
        synth_place::tight_edge_margin_mm(&board),
    );
    let width_mm = synth_geometry::nm_to_mm(placement.board_outline.width_nm());
    let height_mm = synth_geometry::nm_to_mm(placement.board_outline.height_nm());
    assert!(
        width_mm <= 14.78 && height_mm <= 14.68,
        "outline is {width_mm:.2} x {height_mm:.2} mm; the 2.0 mm rim fitted \
         16.78 x 16.68 mm, so the 1.0 mm rim must be 2 mm smaller on both axes"
    );
}

/// A mounting hole is a board feature, but it still needs a rim. The tight
/// export fit must not put Edge.Cuts through the hole.
#[test]
fn a_mounting_hole_keeps_a_rim_on_the_fitted_outline() {
    let board = load_board("tests/fixtures/mounting_hole_edge.synth");
    let mut placement = synth_place::place(&board).expect("place");
    synth_place::fit_outline_with_margin_mm(&board, &mut placement, 1.5);

    let h1_id = board
        .components
        .iter()
        .find(|component| component.refdes == "H1")
        .map(|component| component.id)
        .expect("H1");
    let h1 = placement
        .components
        .iter()
        .find(|component| component.id == h1_id)
        .expect("H1 placed");

    // `mounting_hole_m2_5` is a 2.5 mm NPTH, so its edge is 1.25 mm from its
    // centre. The board-feature rim is 2.2 mm, so it has to stay outside that
    // edge by at least 2.1 mm.
    let hole_edge_nm = 1_250_000;
    let left_rim_nm = (h1.center.x_nm - hole_edge_nm) - placement.board_outline.min.x_nm;
    assert!(
        left_rim_nm >= 2_100_000,
        "left rim is {:.2} mm; the board-feature rim must keep Edge.Cuts off the hole",
        synth_geometry::nm_to_mm(left_rim_nm)
    );
    let top_rim_nm = (h1.center.y_nm - hole_edge_nm) - placement.board_outline.min.y_nm;
    assert!(
        top_rim_nm >= 2_100_000,
        "top rim is {:.2} mm; the board-feature rim must keep Edge.Cuts off the hole",
        synth_geometry::nm_to_mm(top_rim_nm)
    );
}
