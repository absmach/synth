// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use synth_registry::qualify::{FootprintFacts, ModelFacts, PadFacts, PadSide, SymbolFacts};

use crate::kicad_footprint_loader::{self, PadCopperLayers, MODEL_DIR};

#[derive(Debug, Default, Clone, Copy)]
pub struct InstalledKicad;

/// Every 3D model file a footprint body names, in order. Matches `(model `
/// the way the export decides whether a footprint already has one.
fn declared_models(body: &str) -> Vec<&str> {
    body.split("(model ")
        .skip(1)
        .filter_map(|rest| rest.trim_start().strip_prefix('"')?.split('"').next())
        .collect()
}

/// A footprint is `Found` when any model it names is on disk. One that names
/// none gets the library's `default_path`, as the export does.
fn model_facts(declared: &[&str], default_path: &str, exists: impl Fn(&str) -> bool) -> ModelFacts {
    match declared {
        [] if exists(default_path) => ModelFacts::Found,
        [] => ModelFacts::NoneDeclared,
        _ if declared.iter().any(|p| exists(p)) => ModelFacts::Found,
        [first, ..] => ModelFacts::FileMissing(file_name(first)),
    }
}

/// Colon-separated extra directories of model files, searched like a
/// model directory.
const STEP_DIRS_ENV: &str = "SYNTH_STEP_MODEL_DIRS";

/// Whether `name` is a 3D model file (STEP, STP or WRL, any case).
fn is_model_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            ["step", "stp", "wrl"]
                .iter()
                .any(|m| e.eq_ignore_ascii_case(m))
        })
}

/// Adds every model file name under `dir`. Unreadable or missing dirs add nothing.
fn collect_files(dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // `file_type` does not follow symlinks, so a link cycle cannot loop.
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_files(&entry.path(), out),
            Ok(t) if t.is_file() => {
                let name = entry.file_name().to_string_lossy().into_owned();
                if is_model_file(&name) {
                    out.insert(name);
                }
            }
            _ => {}
        }
    }
}

/// Directory names that hold a qualification package's 3D models: `models/` is
/// the standard package layout and `step-models/` is the one earlier packages use.
const MODEL_DIR_NAMES: [&str; 2] = ["models", "step-models"];

/// Adds the files of every model directory (`models` or `step-models`) at or
/// below `dir`.
fn collect_step_models(dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if MODEL_DIR_NAMES
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            collect_files(&entry.path(), out);
        } else {
            collect_step_models(&entry.path(), out);
        }
    }
}

/// File names of the model files committed under `qualification_roots`
/// (any `models/` or `step-models/` directory) or placed in `extra_dirs`.
fn step_model_names(qualification_roots: &[PathBuf], extra_dirs: &[PathBuf]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for root in qualification_roots {
        collect_step_models(root, &mut names);
    }
    for dir in extra_dirs {
        collect_files(dir, &mut names);
    }
    names
}

/// The repo's `qualification/` tree: the nearest ancestor of the working
/// directory holding `qualification/` or `.git`.
fn repo_qualification_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let root = cwd
        .ancestors()
        .find(|d| d.join("qualification").is_dir() || d.join(".git").exists())?;
    Some(root.join("qualification"))
}

/// Scanned once per process, so qualifying a whole registry stays fast.
fn committed_step_models() -> &'static BTreeSet<String> {
    static NAMES: OnceLock<BTreeSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let extra: Vec<PathBuf> = std::env::var_os(STEP_DIRS_ENV)
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        step_model_names(
            &repo_qualification_root().into_iter().collect::<Vec<_>>(),
            &extra,
        )
    })
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

fn side(layers: PadCopperLayers) -> PadSide {
    match layers {
        PadCopperLayers::Front => PadSide::Front,
        PadCopperLayers::Back => PadSide::Back,
        PadCopperLayers::Both => PadSide::Both,
        PadCopperLayers::None => PadSide::NoCopper,
    }
}

impl FootprintFacts for InstalledKicad {
    /// Mirrors the export: a stock footprint that names no model gets the
    /// library's `<lib>.3dshapes/<name>.step`, so that file counts too.
    fn model(&self, lib_id: &str) -> Option<ModelFacts> {
        let Some(body) = kicad_footprint_loader::inline_body(lib_id) else {
            return Some(ModelFacts::NoFootprint);
        };
        let (lib, name) = lib_id.split_once(':')?;
        let default_path = format!("{MODEL_DIR}/{lib}.3dshapes/{name}.step");
        // A committed qualification STEP file with the same name also counts.
        Some(model_facts(&declared_models(&body), &default_path, |p| {
            Path::new(p).exists() || committed_step_models().contains(&file_name(p))
        }))
    }

    fn pads(&self, lib_id: &str) -> Option<Vec<PadFacts>> {
        Some(
            kicad_footprint_loader::pads(lib_id)?
                .into_iter()
                .map(|pad| PadFacts {
                    number: pad.number,
                    center_mm: pad.center_mm,
                    size_mm: pad.size_mm,
                    side: side(pad.copper_layers),
                    is_npth: pad.is_npth,
                })
                .collect(),
        )
    }
}

impl SymbolFacts for InstalledKicad {
    fn pin_numbers(&self, lib_id: &str) -> Option<BTreeSet<String>> {
        synth_registry::kicad_pin_numbers(lib_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_MODELS: &str = r#"(footprint "Pico"
  (model "/m/Pico_A.step" (offset (xyz 0 0 0)))
  (model "/m/Pico B.step" (offset (xyz 0 0 0)))
)"#;

    #[test]
    fn every_model_a_footprint_names_is_found_in_order() {
        assert_eq!(
            declared_models(TWO_MODELS),
            vec!["/m/Pico_A.step", "/m/Pico B.step"]
        );
        assert!(declared_models("(footprint \"X\" (pad \"1\" smd rect))").is_empty());
    }

    #[test]
    fn one_installed_model_is_enough_when_a_footprint_names_several() {
        let only_second = |p: &str| p == "/m/Pico B.step";
        assert_eq!(
            model_facts(&declared_models(TWO_MODELS), "/d/x.step", only_second),
            ModelFacts::Found
        );
    }

    #[test]
    fn no_installed_model_names_the_first_missing_file() {
        assert_eq!(
            model_facts(&declared_models(TWO_MODELS), "/d/x.step", |_| false),
            ModelFacts::FileMissing("Pico_A.step".into())
        );
    }

    #[test]
    fn a_footprint_with_no_model_falls_back_to_the_library_default() {
        assert_eq!(
            model_facts(&[], "/d/x.step", |p| p == "/d/x.step"),
            ModelFacts::Found
        );
        assert_eq!(
            model_facts(&[], "/d/x.step", |_| false),
            ModelFacts::NoneDeclared
        );
    }

    #[test]
    fn committed_step_models_count_from_any_step_models_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            tmp.path().join("q/a/step-models"),
            tmp.path().join("q/b/x/step-models"),
            tmp.path().join("q/c/models"),
        );
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(tmp.path().join("q/other")).unwrap();
        std::fs::write(a.join("Pico_A.step"), "").unwrap();
        std::fs::write(a.join("Dup.step"), "").unwrap();
        std::fs::write(b.join("Dup.step"), "").unwrap();
        std::fs::write(tmp.path().join("q/other/Stray.step"), "").unwrap();
        std::fs::write(a.join("notes.txt"), "").unwrap();
        std::fs::write(a.join("Upper.STP"), "").unwrap();
        // The standard package layout keeps 3D files in `models/`.
        std::fs::write(c.join("Std.step"), "").unwrap();
        let names = step_model_names(
            &[tmp.path().join("q"), tmp.path().join("nope")],
            &[tmp.path().join("missing")],
        );
        assert_eq!(
            names.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Dup.step", "Pico_A.step", "Std.step", "Upper.STP"]
        );
        let in_step = |p: &str| names.contains(&file_name(p));
        assert_eq!(
            model_facts(&declared_models(TWO_MODELS), "/d/x.step", in_step),
            ModelFacts::Found
        );
        assert_eq!(model_facts(&[], "/d/Dup.step", in_step), ModelFacts::Found);
        assert_eq!(
            model_facts(&[], "/d/Gone.step", in_step),
            ModelFacts::NoneDeclared
        );
        assert_eq!(
            model_facts(
                &declared_models("(model \"/m/Gone.step\")"),
                "/d/x.step",
                in_step
            ),
            ModelFacts::FileMissing("Gone.step".into())
        );
    }
}
