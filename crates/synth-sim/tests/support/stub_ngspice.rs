// SPDX-License-Identifier: Apache-2.0

//! Replays a captured ngspice log for the `synth-sim` tests.
//!
//! The tests link (or copy on Windows) this binary to
//! `<scratch>/<label>[.exe]`, so the file stem selects the behaviour. That
//! keeps test selection out of a process-wide environment variable, which
//! parallel tests would race on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    let exe = std::path::absolute(std::env::args().next().expect("argv0")).expect("absolute argv0");
    let label = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");

    match label.as_str() {
        "hang" => {
            let dir = exe.parent().expect("stub dir");
            let _ = std::fs::write(dir.join("hang-started"), b"started\n");
            std::thread::sleep(Duration::from_secs(60));
            let _ = std::fs::write(dir.join("hang-finished"), b"finished\n");
        }
        "ok" => replay(&fixtures, "divider-op", 0),
        "failed-meas" => replay(&fixtures, "divider-meas-failed", 0),
        "missing-meas" => replay(&fixtures, "divider-missing-value", 0),
        "missing-done" => replay(&fixtures, "divider-missing-done", 0),
        "nan" => replay(&fixtures, "divider-nan", 0),
        "error" => replay(&fixtures, "divider-error", 1),
        "singular" => replay(&fixtures, "divider-singular", 0),
        other => {
            eprintln!("stub_ngspice: unknown label `{other}`");
            std::process::exit(2);
        }
    }
}

fn replay(fixtures: &Path, stem: &str, code: i32) -> ! {
    let stdout = std::fs::read_to_string(fixtures.join(format!("{stem}.out")))
        .unwrap_or_else(|error| fail(&error.to_string()));
    let stderr = std::fs::read_to_string(fixtures.join(format!("{stem}.err")))
        .unwrap_or_else(|error| fail(&error.to_string()));
    print!("{stdout}");
    let _ = std::io::stdout().flush();
    eprint!("{stderr}");
    let _ = std::io::stderr().flush();
    std::process::exit(code)
}

fn fail(message: &str) -> ! {
    eprintln!("stub_ngspice: {message}");
    std::process::exit(2)
}
