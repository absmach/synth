// SPDX-License-Identifier: Apache-2.0

//! Embed the workspace seed registry (`registry/parts`) into the
//! binary: enumerate every `*.synth.toml` and emit an
//! `include_str!` table with absolute paths into `OUT_DIR`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let seed = Path::new(&manifest)
        .join("..")
        .join("..")
        .join("registry")
        .join("parts")
        .canonicalize()
        .expect("workspace registry/parts must exist next to the crates");
    println!("cargo:rerun-if-changed={}", seed.display());

    let mut files = Vec::new();
    walk(&seed, &mut files);
    files.sort();

    let mut src = String::from("pub static SEED_FILES: &[(&str, &str)] = &[\n");
    for f in &files {
        let rel = f
            .strip_prefix(&seed)
            .expect("walked paths live under the seed dir")
            .to_string_lossy()
            .replace('\\', "/");
        let _ = writeln!(src, "    ({rel:?}, include_str!({:?})),", f.display());
    }
    src.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(Path::new(&out_dir).join("seed_registry.rs"), src)
        .expect("write generated seed registry table");
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("seed dir is readable") {
        let path = entry.expect("seed dir entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|x| x == "toml") {
            out.push(path);
        }
    }
}
