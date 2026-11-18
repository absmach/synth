// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use synth_ir::Board;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(rel: &str) -> Board {
    let path = workspace_root().join(rel);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ok");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry loads");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board lowered")
}

#[test]
fn reference_designs_pass_independent_placement_validation() {
    let designs = [
        "examples/sensor_logger/sensor_logger.synth",
        "fixtures/designs/secure_tracker.synth",
        "fixtures/designs/feather_m4_express.synth",
    ];

    for design in &designs {
        let board = load_board(design);
        let placement = synth_place::place(&board).expect("placement succeeds");
        let diagnostics = synth_validate::validate_placement(&board, &placement, design);

        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, synth_diagnostics::Severity::Error))
            .collect();

        assert!(
            errors.is_empty(),
            "design {design} failed placement validation with errors: {errors:?}"
        );
    }
}

/// A courtyard that reaches into a keepout circle is reported even when its
/// centre is outside the circle: the pads can still hang into the no-route zone.
#[test]
fn a_courtyard_reaching_into_a_keepout_is_reported() {
    let source = r#"board "keepout" { layers 2
        keepout R1 { radius 5mm }
        component R1: resistor "r_generic_0603" value "10k"
        component R2: resistor "r_generic_0603" value "10k"
        connect R1.p2 -> R2.p1
    }"#;
    let file = "keepout.synth".to_string();
    let parse = synth_parser::parse(source, file.clone());
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("registry loads");
    let board = synth_ir::lower(parse.ast.as_ref().expect("parse ok"), &registry, &file)
        .board
        .expect("board lowered");

    let mut placement = synth_place::place(&board).expect("placement succeeds");
    let centre_of = |refdes: &str| {
        let id = board
            .components
            .iter()
            .find(|c| c.refdes == refdes)
            .expect("component")
            .id;
        (
            id,
            placement
                .components
                .iter()
                .find(|p| p.id == id)
                .expect("placed")
                .center,
        )
    };
    let (r1_id, r1_centre) = centre_of("R1");
    let (r2_id, _) = centre_of("R2");
    assert_ne!(r1_id, r2_id);
    // 5.4 mm from R1: outside the 5 mm circle by centre, inside it by courtyard.
    let r2 = placement
        .components
        .iter_mut()
        .find(|p| p.id == r2_id)
        .expect("R2 placed");
    r2.center = synth_geometry::Point::new(
        r1_centre.x_nm + synth_geometry::mm_to_nm(5.4),
        r1_centre.y_nm,
    );

    let diagnostics = synth_validate::validate_placement(&board, &placement, "keepout.synth");
    assert!(
        diagnostics.iter().any(|d| d.code == "E-SYNTH-PLACE-004"),
        "no keepout diagnostic: {diagnostics:?}"
    );
}
