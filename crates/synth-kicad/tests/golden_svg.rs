// SPDX-License-Identifier: Apache-2.0

//! Golden-image regression for the schematic exporter
//! (schematic-quality plan E4).
//!
//! Each reference design is lowered, rendered to a self-contained
//! `.kicad_sch`, plotted by `kicad-cli sch export svg`, and reduced to
//! a fingerprint (SHA-256 of the normalised plot plus a few structural
//! counts) compared against a committed manifest. Layout and UUIDs are
//! deterministic (UUID v5, fixed pass order), so any change to the
//! drawing changes the hash — this is the harness that keeps phases
//! A–D from silently regressing each other.
//!
//! The fingerprint is a manifest rather than the full SVG: the plots
//! are ~8 MB in total and ~10 000 lines each, so a committed copy would
//! dwarf the rest of the repo while a hash catches exactly the same
//! regressions. The counts (`width`, `height`, `paths`, `texts`) make a
//! changed hash diagnosable at a glance.
//!
//! The plot embeds the render timestamp in its `<title>`; that line is
//! stripped before hashing so the golden never goes stale on its own.
//! Regenerate with `SYNTH_REGEN_SVG_GOLDEN=1 cargo test -p synth-kicad
//! --test golden_svg`. Skips when `kicad-cli` is absent.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn manifest_path() -> PathBuf {
    workspace_root()
        .join("fixtures")
        .join("kicad-reference")
        .join("svg")
        .join("golden.json")
}

/// One design's fingerprint: the normalised plot's SHA-256 plus a few
/// structural counts that make a changed hash diagnosable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Fingerprint {
    sha256: String,
    width: String,
    height: String,
    paths: usize,
    texts: usize,
}

/// Drop the render-timestamp `<title>` line so the fingerprint is
/// stable across days. Everything else is byte-for-byte the plot.
fn normalise(svg: &str) -> String {
    svg.lines()
        .filter(|line| !line.trim_start().starts_with("<title>"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pull an `attr="value"` off the opening `<svg …>` tag.
fn svg_attr(svg: &str, attr: &str) -> String {
    let needle = format!("{attr}=\"");
    svg.find(&needle).map_or_else(String::new, |i| {
        let rest = &svg[i + needle.len()..];
        rest.find('"')
            .map_or_else(String::new, |end| rest[..end].to_string())
    })
}

fn fingerprint(svg: &str) -> Fingerprint {
    let normalised = normalise(svg);
    let mut hasher = Sha256::new();
    hasher.update(normalised.as_bytes());
    Fingerprint {
        sha256: format!("{:x}", hasher.finalize()),
        width: svg_attr(svg, "width"),
        height: svg_attr(svg, "height"),
        paths: normalised.matches("<path").count(),
        texts: normalised.matches("<text").count(),
    }
}

fn design_paths() -> Vec<PathBuf> {
    let dir = workspace_root().join("fixtures").join("kicad-reference");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "synth"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "fixtures/kicad-reference must hold reference designs"
    );
    paths
}

#[test]
fn reference_schematics_match_golden_fingerprints() {
    let registry = synth_registry::load_dir(&workspace_root().join("registry").join("parts"))
        .expect("seed registry must load");
    let regen = std::env::var("SYNTH_REGEN_SVG_GOLDEN").is_ok();
    let manifest = manifest_path();

    let tmp = std::env::temp_dir().join(format!("synth-golden-svg-{}", std::process::id()));
    fs::create_dir_all(&tmp).expect("create temp dir");

    let mut live: BTreeMap<String, Fingerprint> = BTreeMap::new();
    let mut skipped = false;
    for path in design_paths() {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let parsed = synth_parser::parse(&src, filename.clone());
        let ast = parsed.ast.expect("ast");
        let board = synth_ir::lower(&ast, &registry, &filename)
            .board
            .expect("board");

        // Self-contained schematic (lib_symbols embedded) so kicad-cli
        // needs nothing beside it.
        let project = synth_kicad::uuid_v5::project_namespace(&board.name);
        let sch = synth_kicad::schematic::build_schematic(&board, &project).to_string_pretty();
        let sch_path = tmp.join(format!("{stem}.kicad_sch"));
        fs::write(&sch_path, &sch).expect("write schematic");

        let svg_dir = tmp.join(format!("{stem}.svg.d"));
        let output = Command::new("kicad-cli")
            .args(["sch", "export", "svg", "--output"])
            .arg(&svg_dir)
            .arg(&sch_path)
            .output();
        let Ok(output) = output else {
            eprintln!("kicad-cli not installed; skipping golden SVG test");
            skipped = true;
            break;
        };
        assert!(
            output.status.success(),
            "{stem}: kicad-cli sch export svg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let produced = svg_dir.join(format!("{stem}.svg"));
        let svg = fs::read_to_string(&produced)
            .unwrap_or_else(|e| panic!("{stem}: reading plot {}: {e}", produced.display()));
        live.insert(stem, fingerprint(&svg));
    }
    let _ = fs::remove_dir_all(&tmp);

    if skipped {
        return;
    }

    if regen {
        let json = serde_json::to_string_pretty(&live).unwrap();
        if let Some(parent) = manifest.parent() {
            fs::create_dir_all(parent).expect("create manifest dir");
        }
        fs::write(&manifest, json + "\n").expect("write manifest");
        eprintln!(
            "regenerated {} ({} designs)",
            manifest.display(),
            live.len()
        );
        return;
    }

    let raw = fs::read_to_string(&manifest).unwrap_or_else(|e| {
        panic!(
            "missing {} ({e}) — regenerate with SYNTH_REGEN_SVG_GOLDEN=1",
            manifest.display()
        )
    });
    let golden: BTreeMap<String, Fingerprint> =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("invalid golden manifest: {e}"));

    for (stem, live_fp) in &live {
        let base = golden.get(stem).unwrap_or_else(|| {
            panic!("no golden for `{stem}` — regenerate with SYNTH_REGEN_SVG_GOLDEN=1")
        });
        assert_eq!(
            live_fp, base,
            "{stem}: rendered schematic differs from its golden fingerprint"
        );
    }
    for stem in golden.keys() {
        assert!(
            live.contains_key(stem),
            "golden has `{stem}` but no such reference design"
        );
    }
    assert!(!live.is_empty(), "no goldens were compared");
}
