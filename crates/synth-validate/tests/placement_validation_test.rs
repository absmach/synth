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
        "examples/sensor_logger.synth",
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
