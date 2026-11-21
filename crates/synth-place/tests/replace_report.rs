//! Temporary RePlAce placement report. Local experiment only.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(relative: &str) -> synth_ir::Board {
    let path = workspace_root().join(relative);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = workspace_root().join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: workspace_root(),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    lowered.board.expect("board")
}

#[test]
#[ignore = "local RePlAce report"]
fn replace_placement_report() {
    for design in [
        "examples/sensor_logger/sensor_logger.synth",
        "fixtures/designs/secure_tracker.synth",
    ] {
        let board = load_board(design);
        let default = synth_place::place(&board).expect("default");
        let replace = synth_place::place_with_replace(&board).expect("replace");
        let describe = |label: &str, placement: &synth_place::Placement| {
            let score = synth_place::score::score_placement(&board, placement);
            let review = synth_place::describe_placement(&board, placement).visual_review;
            println!(
                "{design} {label}: {:.2}x{:.2} mm ({:.0} mm2) hpwl {:.1} visual {}",
                synth_geometry::nm_to_mm(placement.board_outline.width_nm()),
                synth_geometry::nm_to_mm(placement.board_outline.height_nm()),
                synth_geometry::nm_to_mm(placement.board_outline.width_nm())
                    * synth_geometry::nm_to_mm(placement.board_outline.height_nm()),
                synth_geometry::nm_to_mm(score.hpwl_total_nm),
                if review.requires_revision {
                    "REVISION"
                } else {
                    "clean"
                }
            );
        };
        describe("default", &default);
        describe("replace", &replace);
    }
}
