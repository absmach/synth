// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Serialize)]
struct FeatureRecord {
    file: String,
    is_clean: bool,
    features: Vec<f64>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn registry_dir() -> PathBuf {
    workspace_root().join("registry").join("parts")
}

fn find_all_synth_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext == "synth" || ext == "synthj")
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn dump_features() {
    let erc_dir = workspace_root().join("fixtures").join("erc");
    let designs_dir = workspace_root().join("fixtures").join("designs");

    let mut files = find_all_synth_files(&erc_dir);
    files.extend(find_all_synth_files(&designs_dir));

    let registry = synth_registry::load_dir(&registry_dir()).expect("load registry");

    for file_path in files {
        let filename = file_path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(src) = fs::read_to_string(&file_path) else {
            continue;
        };

        let parse = if src.trim_start().starts_with('{') {
            synth_parser::json::parse_json(&src, filename.clone())
        } else {
            synth_parser::parse(&src, filename.clone())
        };

        let Some(ast) = parse.ast.as_ref() else {
            continue;
        };

        let loader = synth_ir::MemoryImportLoader::default();
        let resolved = synth_ir::resolve_imports(ast, &loader, &filename);
        let lowered = synth_ir::lower(&resolved.program, &registry, &filename);

        let Some(board) = lowered.board.as_ref() else {
            continue;
        };

        let mut diags = parse.diagnostics;
        diags.extend(resolved.diagnostics);
        diags.extend(lowered.diagnostics);
        // Exclude GraphAnomalyDetectorRule when checking if design is clean
        let erc_diags = synth_validate::run_erc(board, &filename);
        for d in erc_diags {
            if d.code != "W-SYNTH-ANOMALY-001" {
                diags.push(d);
            }
        }

        let is_clean = !diags.iter().any(|d| d.severity.is_blocking());
        let f_vec = synth_validate::extract_features(board);

        let rec = FeatureRecord {
            file: filename,
            is_clean,
            features: f_vec.0.to_vec(),
        };

        if let Ok(json_line) = serde_json::to_string(&rec) {
            println!("FEATURE_DUMP:{json_line}");
        }
    }
}
