//! Temporary interior-repack placement report. Local experiment only.

use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn load_board(relative: &str) -> synth_ir::Board {
    let path = workspace_root().join(relative);
    let source = std::fs::read_to_string(&path).expect("read board");
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

fn describe(label: &str, board: &synth_ir::Board, placement: &synth_place::Placement) {
    let score = synth_place::score::score_placement(board, placement);
    let review = synth_place::describe_placement(board, placement).visual_review;
    println!(
        "{label}: {:.2}x{:.2} mm ({:.0} mm2) hpwl {:.1} visual {}",
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
}

#[test]
#[ignore = "local repack report"]
fn interior_repack_placement_report() {
    for design in [
        "fixtures/designs/feather_m4_express.synth",
        "fixtures/designs/secure_tracker.synth",
    ] {
        let board = load_board(design);
        let sidecars = synth_kicad::Sidecars {
            schematic: None,
            placement: None,
        };
        let base = synth_kicad::placement_candidates(&board, &sidecars).expect("base");
        let repacked =
            synth_kicad::repacked_placement_candidates(&board, &sidecars).expect("repacked");
        println!(
            "--- {design}: base {} repacked {}",
            base.len(),
            repacked.len()
        );
        for (label, candidates) in [("base", &base), ("repacked", &repacked)] {
            let best = candidates
                .iter()
                .min_by_key(|candidate| {
                    candidate.board_outline.width_nm() * candidate.board_outline.height_nm()
                })
                .expect("candidate");
            describe(&format!("{label} smallest"), &board, best);
            describe(&format!("{label} first"), &board, &candidates[0]);
        }
    }
}
