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
/// stubs share the compiled artifact. Test runners such as nextest start one
/// process per test, so the artifact is compiled in a pid-unique directory
/// and published into the shared path with an atomic rename.
pub fn build_stub(source: &Path) -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join(format!("synth_stub_bin_{}", env!("CARGO_PKG_NAME")));
            std::fs::create_dir_all(&dir).expect("stub binary dir");
            let binary = dir.join(format!("stub{}", std::env::consts::EXE_SUFFIX));
            if is_newer_than(&binary, source) {
                return binary;
            }

            // Compile beside the target, then rename: concurrent test
            // processes race to publish, and rename replaces atomically.
            let staging = dir.join(format!("build-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).expect("stub build dir");
            let staged = staging.join(format!("stub{}", std::env::consts::EXE_SUFFIX));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let status = Command::new(rustc)
                .arg("--edition=2021")
                .arg(source)
                .arg("-o")
                .arg(&staged)
                .status()
                .expect("run rustc for the ngspice stub");
            assert!(status.success(), "could not compile the ngspice stub");
            // Windows cannot replace a stub another process is executing;
            // an existing stub is still a usable executable, so keep it.
            let published = std::fs::rename(&staged, &binary).is_ok() || binary.is_file();
            assert!(published, "could not publish the ngspice stub");
            let _ = std::fs::remove_dir_all(&staging);
            binary
        })
        .clone()
}

fn is_newer_than(binary: &Path, source: &Path) -> bool {
    let Ok(binary_time) = binary.metadata().and_then(|meta| meta.modified()) else {
        return false;
    };
    let Ok(source_time) = source.metadata().and_then(|meta| meta.modified()) else {
        return false;
    };
    binary_time > source_time
}
