// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
        Some(model_facts(&declared_models(&body), &default_path, |p| {
            Path::new(p).exists()
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

    fn pin_names(&self, lib_id: &str) -> Option<BTreeMap<String, BTreeSet<String>>> {
        synth_registry::kicad_pin_names(lib_id)
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
}
