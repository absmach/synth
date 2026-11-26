// SPDX-License-Identifier: Apache-2.0

//! The default export of an auto-sized board without a sidecar must run the
//! placement compression pass, not stop at the outline fit.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

fn rtd_afe() -> synth_ir::Board {
    let path = root().join("crates/synth-place/tests/fixtures/rtd_afe.synth");
    let source = std::fs::read_to_string(&path).expect("fixture");
    let registry =
        synth_registry::load_dir(&root().join("registry").join("parts")).expect("registry");
    let file = path.display().to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let mut board = synth_ir::lower(&parse.ast.expect("ast"), &registry, &file)
        .board
        .expect("board");
    board.placement_spacing_nm = None;
    board
}

fn export_area() -> f64 {
    let board = rtd_afe();
    assert!(board.outline.is_none(), "fixture must be auto-sized");
    let placement = synth_kicad::placement_for_export(&board, &synth_kicad::Sidecars::default())
        .expect("placement");
    synth_geometry::nm_to_mm(placement.board_outline.width_nm())
        * synth_geometry::nm_to_mm(placement.board_outline.height_nm())
}

#[test]
fn the_default_export_placement_is_compacted() {
    // The same candidate is placed in a child with compression off; the
    // export must be strictly smaller than that uncompressed fit.
    if std::env::var("DEFAULT_EXPORT_CHILD").is_ok() {
        println!("UNCOMPRESSED {:.4}", export_area());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "the_default_export_placement_is_compacted",
            "--nocapture",
        ])
        .env("DEFAULT_EXPORT_CHILD", "1")
        .env("SYNTH_PLACE_COMPRESS", "0")
        .output()
        .expect("child test");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let uncompressed: f64 = stdout
        .lines()
        .find_map(|line| line.strip_prefix("UNCOMPRESSED "))
        .expect("child area")
        .trim()
        .parse()
        .expect("area");
    let compressed = export_area();
    assert!(
        compressed < uncompressed,
        "export area {compressed:.1} mm2 is not below the uncompressed {uncompressed:.1} mm2"
    );
}
