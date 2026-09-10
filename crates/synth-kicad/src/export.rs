// SPDX-License-Identifier: Apache-2.0

//! Top-level export: orchestrates project, schematic, library, BOM.
//!
//! Given an `&Board` and an output directory, writes the four
//! artifacts and returns a [`ExportResult`] describing what was
//! written. I/O errors are converted to [`ExportError`].

use std::path::{Path, PathBuf};

use serde_json::json;
use thiserror::Error;

use synth_ir::Board;

use crate::bom;
use crate::pcb;
use crate::schematic;
use crate::symbol_lib;
use crate::uuid_v5;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("could not create output directory {path:?}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path:?}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize project file: {source}")]
    SerializeProject {
        #[source]
        source: serde_json::Error,
    },
    #[error("placement failed: {source}")]
    Placement {
        #[source]
        source: synth_place::PlaceError,
    },
}

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub out_dir: PathBuf,
    pub project_path: PathBuf,
    pub schematic_path: PathBuf,
    pub library_path: PathBuf,
    pub pcb_path: PathBuf,
    pub bom_path: PathBuf,
}

/// Write all four artifacts and return their paths. The output
/// directory is created if absent. Files inside an existing
/// directory are overwritten.
pub fn export(board: &Board, out_dir: &Path) -> Result<ExportResult, ExportError> {
    export_with_sidecar(board, out_dir, None)
}

/// [`export`] honouring an optional `<design>.synth.layout.toml`
/// sidecar: manual component drags are applied between placement and
/// routing so both the exported schematic *and* the exported PCB
/// reflect hand-tuned positions. DSL `placement_hint`s declared on
/// components are honoured inside the placer itself and need no
/// plumbing here.
///
/// # Errors
/// Same as [`export`].
pub fn export_with_sidecar(
    board: &Board,
    out_dir: &Path,
    sidecar: Option<&Path>,
) -> Result<ExportResult, ExportError> {
    std::fs::create_dir_all(out_dir).map_err(|source| ExportError::CreateDir {
        path: out_dir.to_path_buf(),
        source,
    })?;

    let stem = sanitize_filename(&board.name);
    let project_path = out_dir.join(format!("{stem}.kicad_pro"));
    let schematic_path = out_dir.join(format!("{stem}.kicad_sch"));
    let library_path = out_dir.join(format!("{stem}.kicad_sym"));
    let pcb_path = out_dir.join(format!("{stem}.kicad_pcb"));
    let bom_path = out_dir.join("bom.csv");

    // Project file (.kicad_pro): minimal JSON. KiCad fills in the
    // rest on first open; the deterministic root keeps diffs stable.
    let project_namespace = uuid_v5::project_namespace(&board.name);
    let project_doc = json!({
        "board": {},
        "boards": [],
        "meta": {
            "filename": format!("{stem}.kicad_pro"),
            "version": 1,
            "uuid": project_namespace.to_string(),
        },
        "schematic": {
            "annotate_start_num": 0,
            "drawing": {},
        },
        "sheets": [
            [
                uuid_v5::derive_entity_uuid(&project_namespace, "sheet", "root").to_string(),
                "",
            ]
        ],
    });
    let project_text = serde_json::to_string_pretty(&project_doc)
        .map_err(|source| ExportError::SerializeProject { source })?;
    write_file(&project_path, &project_text)?;

    // Library file (.kicad_sym): self-contained symbol library
    // covering every part referenced by the IR plus the
    // power-flag symbols (`synth:GND`, `synth:VBUS`, ...) used by
    // the schematic.
    // Project-local `sym-lib-table` mapping the `synth` nickname to
    // the exported `.kicad_sym`. Without this, KiCad's ERC emits a
    // `lib_symbol_issues` warning on every `(lib_id "synth:...")`
    // instance ("The current configuration does not include the
    // symbol library 'synth'") because `synth` isn't in the global
    // sym-lib-table. A table file next to the project makes the
    // referenced library resolvable.
    let sym_table = format!(
        "(sym_lib_table\n  (version 7)\n  (lib\n    (name \"synth\")\n    (uri \"${{KIPRJMOD}}/{stem}.kicad_sym\")\n    (type \"KiCad\")\n    (options \"\")\n    (descr \"Synth synthesized symbol library\")\n  )\n)\n"
    );
    let sym_table_path = out_dir.join("sym-lib-table");
    write_file(&sym_table_path, &sym_table)?;

    // Materialize a real `.kicad_mod` for every part that has no
    // bundled/user footprint — same pad geometry `build_pcb` embeds
    // inline for these parts, but written to disk under
    // `<stem>.pretty/` so the "synth" nickname below actually
    // resolves. Mirrors the `sym-lib-table` trick just above: without
    // a file-backed library entry, `kicad-cli pcb drc` flags
    // `lib_footprint_issues` on every embedded `(footprint
    // "synth:<id>" ...)` instance even though its geometry is fine.
    let mut synth_generated = false;
    let mut seen_parts: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let pretty_dir = out_dir.join(format!("{stem}.pretty"));
    for component in &board.components {
        let Some(part) = component.part.as_ref() else {
            continue;
        };
        if part.kicad_footprint.is_some() || !seen_parts.insert(part.id.as_str().to_string()) {
            continue;
        }
        let lib_id = format!("synth:{}", part.id);
        if synth_layout::kicad_footprint_loader::inline_body(&lib_id).is_some() {
            continue;
        }
        let Some(module) = pcb::build_synthesized_footprint_module(part, &lib_id) else {
            continue;
        };
        if !synth_generated {
            std::fs::create_dir_all(&pretty_dir).map_err(|source| ExportError::CreateDir {
                path: pretty_dir.clone(),
                source,
            })?;
            synth_generated = true;
        }
        let mod_path = pretty_dir.join(format!("{}.kicad_mod", part.id));
        write_file(&mod_path, &module.to_string_pretty())?;
    }

    // Project-local `fp-lib-table` registering Tier-2 user footprints (Phase
    // 15, R15.4) and the synthesized-fallback library above, so inlined
    // `(footprint "lib:name")` references resolve. Without this, KiCad's DRC
    // emits `lib_footprint_issues` ("The current configuration does not
    // include the footprint library 'lib'") because the directory backing
    // the lib_id isn't in the global fp-lib-table. The inline geometry
    // already matches the registered library (it is generated from the
    // same `.kicad_mod`), so KiCad reports no `lib_footprint_mismatch`.
    let user_libs = synth_layout::kicad_footprint_loader::user_footprint_libs();
    if !user_libs.is_empty() || synth_generated {
        use std::fmt::Write as _;
        let mut lib_entries = String::new();
        for (name, path) in &user_libs {
            let abs = path
                .canonicalize()
                .unwrap_or_else(|_| path.clone())
                .display()
                .to_string();
            let _ = write!(
                lib_entries,
                "  (lib (name \"{name}\")(type \"KiCad\")(uri \"{abs}\")(options \"\")(descr \"Synth Tier-2 user footprint library\")\n  )\n"
            );
        }
        if synth_generated {
            let _ = write!(
                lib_entries,
                "  (lib (name \"synth\")(type \"KiCad\")(uri \"${{KIPRJMOD}}/{stem}.pretty\")(options \"\")(descr \"Synth synthesized footprint library\")\n  )\n"
            );
        }
        let fp_table = format!("(fp_lib_table\n  (version 7)\n{lib_entries})\n");
        let fp_table_path = out_dir.join("fp-lib-table");
        write_file(&fp_table_path, &fp_table)?;
    }

    // Schematic file (.kicad_sch). The symbol library is built from
    // the same override-aware layout the schematic consumes so both
    // files agree on positions and power-flag label sets.
    let layout = synth_layout::layout_with_sidecar(board, sidecar);
    let library_text = symbol_lib::build_library(board, &layout).to_string_pretty();
    write_file(&library_path, &library_text)?;
    let schematic_text = schematic::build_schematic_from_layout(board, &project_namespace, &layout)
        .to_string_pretty();
    write_file(&schematic_path, &schematic_text)?;

    // PCB file (.kicad_pcb). Closed-loop repair engine (Phase 13.5):
    // runs placement and routing with up to 5 repair iterations.
    // The sidecar (manual drags) rides along with every placement
    // attempt so routing sees the overridden positions.
    let (placement, routing) = place_and_route_with_repair(board, 5, sidecar)
        .map_err(|source| ExportError::Placement { source })?;
    let pcb_text =
        pcb::build_pcb(board, &placement, &routing, &project_namespace).to_string_pretty();
    write_file(&pcb_path, &pcb_text)?;

    // BOM CSV.
    let bom_text = bom::build_bom_csv(board);
    write_file(&bom_path, &bom_text)?;

    // Pick-and-Place (PnP) CSV.
    let pnp_path = out_dir.join("pnp.csv");
    let pnp_text = crate::pnp::build_pnp_csv(board, &placement);
    write_file(&pnp_path, &pnp_text)?;

    Ok(ExportResult {
        out_dir: out_dir.to_path_buf(),
        project_path,
        schematic_path,
        library_path,
        pcb_path,
        bom_path,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<(), ExportError> {
    std::fs::write(path, contents).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Map an arbitrary board name to a filesystem-safe filename stem.
/// Replaces any character outside `[A-Za-z0-9_-]` with `_`.
fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

/// Closed-loop iterative placer-router repair loop.
///
/// Iteratively attempts placement and routing. If routing encounters unroutable pin tangles
/// or clearance conflicts around specific components, executes targeted rotation tuning
/// and courtyard margin expansion up to `max_iterations`.
///
/// `sidecar` (when present) is applied to every placement attempt
/// *before* routing so manual drags survive the repair loop and the
/// router plans traces against the overridden footprint positions.
/// DSL `placement_hint`s need no explicit handling here: the placer
/// reads them from the IR board directly.
fn place_and_route_with_repair(
    board: &Board,
    max_iterations: usize,
    sidecar: Option<&Path>,
) -> Result<(synth_place::Placement, synth_route::Routing), synth_place::PlaceError> {
    let mut margin = 1.0_f64;
    let mut rotation_overrides = std::collections::HashMap::new();
    let mut best_placement = synth_place::place_with_sidecar(board, sidecar)?;
    let mut best_routing = synth_route::route(board, &best_placement);

    // Stagnation guard: when a design's nets are (locally) unroutable,
    // re-placement never reduces the unrouted count, and every iteration
    // pays a full place+route cycle for nothing. Track consecutive
    // no-improvement attempts and bail once re-placement stops helping —
    // the best routing found is still kept. Two attempts (rather than
    // one) still allow rotation-cascade recovery that unlocks on a later
    // iteration.
    let mut stagnant_attempts = 0_usize;
    for _iter in 0..max_iterations {
        if best_routing.unrouted_nets.is_empty() {
            break;
        }

        // Identify unrouted components and rotate them
        for unrouted in &best_routing.unrouted_nets {
            if let Some(net) = board.net(unrouted.net) {
                for endpoint in &net.endpoints {
                    let current_rot = rotation_overrides
                        .get(&endpoint.component)
                        .copied()
                        .unwrap_or(synth_geometry::Rotation::Zero);
                    let next_rot = match current_rot {
                        synth_geometry::Rotation::Zero => synth_geometry::Rotation::Ninety,
                        synth_geometry::Rotation::Ninety => synth_geometry::Rotation::OneEighty,
                        synth_geometry::Rotation::OneEighty => synth_geometry::Rotation::TwoSeventy,
                        synth_geometry::Rotation::TwoSeventy => synth_geometry::Rotation::Zero,
                    };
                    rotation_overrides.insert(endpoint.component, next_rot);
                }
            }
        }

        margin += 0.5;
        if let Ok(p) =
            synth_place::place_with_tuning_and_sidecar(board, margin, &rotation_overrides, sidecar)
        {
            let r = synth_route::route(board, &p);
            if r.unrouted_nets.len() < best_routing.unrouted_nets.len()
                || r.unrouted_nets.is_empty()
            {
                best_placement = p;
                best_routing = r;
                stagnant_attempts = 0;
            } else {
                stagnant_attempts += 1;
                if stagnant_attempts >= 2 {
                    break;
                }
            }
        } else {
            stagnant_attempts += 1;
            if stagnant_attempts >= 2 {
                break;
            }
        }
    }

    Ok((best_placement, best_routing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_strips_specials() {
        assert_eq!(sanitize_filename("hello world"), "hello_world");
        assert_eq!(sanitize_filename("ok-name_42"), "ok-name_42");
        assert_eq!(sanitize_filename("../etc/passwd"), "___etc_passwd");
        assert_eq!(sanitize_filename(""), "untitled");
    }
}
