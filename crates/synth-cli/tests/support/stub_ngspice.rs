// SPDX-License-Identifier: Apache-2.0

//! `synth-cli` tests link (or copy on Windows) this binary to `<scratch>/ok`,
//! so the file stem selects the replay.

use std::io::Write;
use std::path::PathBuf;

fn main() {
    let exe = std::path::absolute(std::env::args().next().expect("argv0")).expect("absolute argv0");
    let label = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if label != "ok" {
        eprintln!("stub_ngspice: unknown label `{label}`");
        std::process::exit(2);
    }
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("synth-sim")
        .join("tests")
        .join("fixtures");
    let stdout = std::fs::read_to_string(fixtures.join("divider-op.out"))
        .unwrap_or_else(|error| fail(&error.to_string()));
    let stderr = std::fs::read_to_string(fixtures.join("divider-op.err"))
        .unwrap_or_else(|error| fail(&error.to_string()));
    print!("{stdout}");
    let _ = std::io::stdout().flush();
    eprint!("{stderr}");
    let _ = std::io::stderr().flush();
}

fn fail(message: &str) -> ! {
    eprintln!("stub_ngspice: {message}");
    std::process::exit(2)
}
