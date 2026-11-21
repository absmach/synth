// SPDX-License-Identifier: Apache-2.0

//! The route loop picks a placement by routing each candidate, with no
//! routed-result gate behind it. A RePlAce candidate must never enter it, even
//! when the experiment seam is on: the gate only exists on the export path.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

const SOURCE: &str = r#"board "replace_seam" {
  layers 2

  component U1: mcu "rp2350"
  component HL1: connector "header_1x4" {
    placement_hint { edge: left  priority: hard }
  }
  component HR1: connector "header_1x4" {
    placement_hint { edge: right  priority: hard }
  }

  connect U1.gp0 -> HL1.p1
  connect U1.gp1 -> HL1.p2
  connect U1.gp0 -> HR1.p1
  connect U1.gp1 -> HR1.p2
}
"#;

fn candidates() -> Vec<synth_place::Placement> {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let parsed = synth_parser::parse(SOURCE, "replace_seam.synth");
    let lowered = synth_ir::lower(&parsed.ast.expect("ast"), &registry, "replace_seam.synth");
    let board = lowered.board.expect("board");
    let sidecars = synth_kicad::Sidecars::default();
    synth_kicad::placement_candidates(&board, &sidecars).expect("candidates")
}

#[test]
fn the_replace_seam_adds_no_route_loop_candidate() {
    // Edition 2024 makes `set_var` unsafe and the workspace forbids unsafe, so
    // the seam runs in a child and reports its candidates through a temp file.
    let path = std::env::var_os("SYNTH_REPLACE_SEAM_PATH").map_or_else(
        || std::env::temp_dir().join(format!("synth-replace-seam-{}.txt", std::process::id())),
        PathBuf::from,
    );
    if std::env::var_os("SYNTH_PLACE_REPLACE_CHILD").is_some() {
        let seam = candidates();
        let plain = std::fs::read_to_string(&path).expect("parent candidates");
        assert_eq!(
            format!("{seam:?}"),
            plain,
            "the replace seam added a route-loop candidate"
        );
        return;
    }

    let plain = candidates();
    std::fs::write(&path, format!("{plain:?}")).expect("write parent candidates");
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "the_replace_seam_adds_no_route_loop_candidate",
            "--nocapture",
        ])
        .env("SYNTH_PLACE_REPLACE_CHILD", "1")
        .env("SYNTH_PLACE_REPLACE", "1")
        .env("SYNTH_REPLACE_SEAM_PATH", &path)
        .output()
        .expect("child test");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
