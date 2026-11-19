// SPDX-License-Identifier: Apache-2.0

//! `export-kicad --spacing` and `placement { spacing }`: the file sets the
//! value, the flag overrides it, and a value outside 1.0 to 5.0 mm is a
//! blocking diagnostic that `--force` lets through with a warning.

use std::path::PathBuf;
use std::process::{Command, Output};

fn board_source(spacing: Option<&str>) -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../synth-place/tests/fixtures/rtd_afe.synth"
    );
    let mut source = std::fs::read_to_string(path).expect("fixture");
    if let Some(value) = spacing {
        source = source.replacen(
            "  layers 4\n",
            &format!("  layers 4\n  placement {{ spacing = {value} }}\n"),
            1,
        );
    }
    source
}

/// Run the export up to the point the router is needed: the checks that matter
/// here all run before it, and a missing router checkout ends the run.
fn export(label: &str, file_spacing: Option<&str>, args: &[&str]) -> (Output, String) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("synth_spacing_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let input = dir.join("board.synth");
    std::fs::write(&input, board_source(file_spacing)).expect("write");
    let output = Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("export-kicad")
        .arg(&input)
        .arg("--out")
        .arg(dir.join("out"))
        .args(["--router", "kicad-routing-tools"])
        .args(["--kicad-routing-tools-repo", "/nonexistent/krt"])
        .args(args)
        .output()
        .expect("run synth");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    (output, stderr)
}

const CODE: &str = "E-SYNTH-PLACE-003";

#[test]
fn the_flag_overrides_the_file_value() {
    // A bad flag wins over a good file value ...
    let (_, bad_flag) = export("flag_bad", Some("2.0mm"), &["--spacing", "0.5"]);
    assert!(bad_flag.contains(CODE), "{bad_flag}");
    // ... and a good flag wins over a bad file value.
    let (_, good_flag) = export("flag_good", Some("0.5mm"), &["--spacing", "2.0"]);
    assert!(!good_flag.contains(CODE), "{good_flag}");
}

#[test]
fn a_value_outside_the_range_blocks_the_export_and_names_the_range() {
    for value in ["0.5", "6.0"] {
        let (output, stderr) = export(&format!("range_{value}"), None, &["--spacing", value]);
        assert!(!output.status.success(), "{value}: {stderr}");
        assert!(stderr.contains(CODE), "{value}: {stderr}");
        assert!(
            stderr.contains("1.0 mm") && stderr.contains("5.0 mm"),
            "the range is named: {stderr}"
        );
    }
}

#[test]
fn force_lets_the_typed_value_through_with_a_warning() {
    let (_, stderr) = export("force", None, &["--spacing", "0.5", "--force"]);
    assert!(stderr.contains(CODE), "{stderr}");
    assert!(stderr.contains("because of --force"), "{stderr}");
    // The export went on past the check, to the router.
    assert!(
        stderr.contains("kicad-routing-tools is not available"),
        "{stderr}"
    );
}

#[test]
fn a_value_inside_the_range_raises_nothing() {
    let (_, stderr) = export("inside", Some("1.0mm"), &[]);
    assert!(!stderr.contains(CODE), "{stderr}");
}
