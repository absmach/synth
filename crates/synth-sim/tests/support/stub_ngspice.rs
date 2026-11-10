// SPDX-License-Identifier: Apache-2.0

//! Replays a captured ngspice log for the `synth-sim` tests.
//!
//! The tests link (or copy on Windows) this binary to
//! `<scratch>/<label>[.exe]`, so the file stem selects the behaviour. The
//! scratch directory also carries `fixtures.path`, so the same source works
//! from either test crate without a compile-time crate path.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    let exe = std::path::absolute(std::env::args().next().expect("argv0")).expect("absolute argv0");
    let dir = exe.parent().expect("stub dir");
    if std::env::args().any(|arg| arg == "--hang-child") {
        let _ = std::fs::write(dir.join("hang-child.pid"), std::process::id().to_string());
        std::thread::sleep(Duration::from_secs(60));
        return;
    }

    let label = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let fixtures = std::fs::read_to_string(dir.join("fixtures.path"))
        .map(PathBuf::from)
        .unwrap_or_else(|error| fail(&format!("could not read fixtures.path: {error}")));

    match label.as_str() {
        "hang" => {
            let _ = std::fs::write(dir.join("hang.pid"), std::process::id().to_string());
            #[cfg(unix)]
            {
                let _ = std::process::Command::new(&exe)
                    .arg("--hang-child")
                    .spawn();
            }
            std::thread::sleep(Duration::from_secs(60));
        }
        "truncated" => {
            // 2 MiB, comfortably above synth-sim's 1 MiB output cap.
            let chunk = vec![b'e'; 8192];
            let mut stderr = std::io::stderr();
            for _ in 0..256 {
                let _ = stderr.write_all(&chunk);
            }
            let _ = stderr.flush();
            print!("SYNTH_MEAS vmid 6\nSYNTH_DONE\n");
            let _ = std::io::stdout().flush();
        }
        "ok" => replay(&fixtures, "divider-op", 0),
        "led" => replay(&fixtures, "led-op", 0),
        "diode" => replay(&fixtures, "diode-op", 0),
        "ldo" => replay(&fixtures, "ldo-op", 0),
        "failed-meas" => replay(&fixtures, "divider-meas-failed", 0),
        "missing-meas" => replay(&fixtures, "divider-missing-value", 0),
        "missing-done" => replay(&fixtures, "divider-missing-done", 0),
        "nan" => replay(&fixtures, "divider-nan", 0),
        "error" => replay(&fixtures, "divider-error", 1),
        "singular" => replay(&fixtures, "divider-singular", 0),
        "singular-no-done" => replay(&fixtures, "divider-singular-no-done", 0),
        "singular-missing-value" => replay(&fixtures, "divider-singular-missing-value", 0),
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
