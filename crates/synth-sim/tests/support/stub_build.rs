// SPDX-License-Identifier: Apache-2.0

//! Compiles the Rust ngspice stub once per test binary.
//!
//! The stub is a test fixture, not a production binary: compiling it at test
//! time keeps it out of `cargo build`, `cargo install` and `cargo metadata`
//! while still giving `SimOptions` a real executable.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Compile `source` and return the stub executable.
///
/// The source is fixed per test binary, so the first call wins and later
/// stubs share the compiled artifact.
pub fn build_stub(source: &Path) -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("synth_stub_bin");
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("stub binary dir");
            let binary = dir.join(format!("stub{}", std::env::consts::EXE_SUFFIX));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let status = Command::new(rustc)
                .arg("--edition=2021")
                .arg(source)
                .arg("-o")
                .arg(&binary)
                .status()
                .expect("run rustc for the ngspice stub");
            assert!(status.success(), "could not compile the ngspice stub");
            binary
        })
        .clone()
}
