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
    export_with_sidecar_and_routing_order(board, out_dir, sidecar, None)
}

/// Export while preserving an optional agent-selected routing order.
///
/// The route used for export must be the same route that the MCP gate
/// inspected; silently recomputing with the default order can discard a
/// successful recovery pass and produce a different PCB artifact.
pub fn export_with_sidecar_and_routing_order(
    board: &Board,
    out_dir: &Path,
    sidecar: Option<&Path>,
    routing_order: Option<&[String]>,
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
        // Keep the project-level defaults explicit. KiCad 10 may discard
        // legacy setup minima when it first saves a generated board, and an
        // empty `board` object then silently restores its own 0.2 mm
        // clearance. These values match the router's 0.2 mm signal width
        // and the 0.127 mm clearance supported by the default manufacturer
        // profile, so native DRC sees the same contract as Synth.
        "board": {
          "design_settings": {
            "defaults": {
              "min_clearance": 0.127,
              "min_track_width": 0.127,
            },
            "rules": {
              "min_clearance": 0.127,
              "min_track_width": 0.127,
            }
          }
        },
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

    // PCB file (.kicad_pcb). Export must serialize the same deterministic
    // placement/routing candidate that the route and DRC tools inspect.
    // Additional export-only repair iterations used to silently select a
    // different placement, making routing feedback and the delivered PCB
    // disagree. Repair/retry belongs to the agent loop; export is a
    // serialization boundary.
    let (placement, routing) = place_and_route_with_repair(board, 0, sidecar, routing_order)
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
fn route_with_optional_order(
    board: &Board,
    placement: &synth_place::Placement,
    routing_order: Option<&[String]>,
) -> synth_route::Routing {
    match routing_order {
        Some(order) if !order.is_empty() => synth_route::route_with_order(board, placement, order),
        _ => synth_route::route(board, placement),
    }
}

fn place_and_route_with_repair(
    board: &Board,
    max_iterations: usize,
    sidecar: Option<&Path>,
    routing_order: Option<&[String]>,
) -> Result<(synth_place::Placement, synth_route::Routing), synth_place::PlaceError> {
    // Match synth-place::place() and the MCP route/DRC gates. Starting the
    // export repair loop with a different courtyard margin can select a
    // different legal placement, causing the exported PCB to disagree with
    // the placement that routing and DRC just evaluated.
    let mut margin = 1.5_f64;
    let mut rotation_overrides = std::collections::HashMap::new();
    // Keep the export-side closed loop consistent with the normal placer.
    // Flash packages are oriented so their QSPI edge faces the MCU; if this
    // override is omitted here, export silently reruns placement with a
    // different orientation than synth_route/synth_place::place().
    let has_rp2350 = board.components.iter().any(|component| {
        component
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("rp2350"))
    });
    for component in &board.components {
        let is_flash = has_rp2350
            && component
                .part
                .as_ref()
                .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("w25q"));
        if is_flash {
            rotation_overrides.insert(component.id, synth_geometry::Rotation::Zero);
        }
    }
    let profile = synth_drc::ManufacturerProfile::jlc_standard();
    let mut best_placement =
        synth_place::place_with_tuning_and_sidecar(board, margin, &rotation_overrides, sidecar)?;
    let mut best_routing = route_with_optional_order(board, &best_placement, routing_order);
    let mut best_score = physical_score(board, &best_placement, &best_routing, &profile);

    // Agent placement hints are valuable design intent, but a hard hint can
    // create a routing dead-end when several connectors compete for the same
    // edge or when a region constraint cuts across a dense fanout. Evaluate a
    // relaxed candidate once at the same margin and keep it when the physical
    // score is better. This gives the agent authority to guide placement
    // without allowing an unrouteable hinted floorplan to win silently.
    if !has_rp2350
        && board
            .components
            .iter()
            .any(|component| component.placement_hint.is_some())
    {
        let mut relaxed_board = board.clone();
        for component in &mut relaxed_board.components {
            component.placement_hint = None;
        }
        if let Ok(relaxed_placement) = synth_place::place_with_tuning_and_sidecar(
            &relaxed_board,
            margin,
            &rotation_overrides,
            sidecar,
        ) {
            let relaxed_routing =
                route_with_optional_order(board, &relaxed_placement, routing_order);
            let relaxed_score =
                physical_score(board, &relaxed_placement, &relaxed_routing, &profile);
            if relaxed_score < best_score {
                best_placement = relaxed_placement;
                best_routing = relaxed_routing;
                best_score = relaxed_score;
            }
        }
    }

    // Stagnation guard: a candidate can be fully routed yet remain DRC
    // invalid, so track the complete physical score rather than only the
    // unrouted count. The best candidate is always retained.
    let mut stagnant_attempts = 0_usize;
    for _iter in 0..max_iterations {
        if best_score == (0, 0) {
            break;
        }

        // Target the components implicated by both router and independent
        // DRC evidence. A routed net can still fail fabrication clearance,
        // courtyard, connector-orientation, or silkscreen rules, so an
        // unrouted-only retry loop can converge on a board KiCad rejects.
        let drc = if best_routing.unrouted_nets.is_empty() {
            synth_drc::check(board, &best_placement, &best_routing, &profile)
        } else {
            synth_drc::DrcReport {
                violations: Vec::new(),
                profile_name: profile.name.clone(),
            }
        };
        let mut implicated = std::collections::BTreeSet::new();
        let mut exact_rotations = std::collections::BTreeSet::new();
        for unrouted in &best_routing.unrouted_nets {
            if let Some(net) = board.net(unrouted.net) {
                for endpoint in &net.endpoints {
                    implicated.insert(endpoint.component);
                }
            }
        }
        for violation in &drc.violations {
            if let Some(suggestion) = &violation.suggested_override {
                if let Some(component) = board
                    .components
                    .iter()
                    .find(|c| c.refdes == suggestion.refdes)
                {
                    implicated.insert(component.id);
                    // A rule may know the exact orientation required (for
                    // example, a connector mating edge). Honor that advice
                    // directly; the fallback rotation below is reserved for
                    // violations that only identify a witness component.
                    if let Some(rotation) = rotation_from_degrees(suggestion.rotation_deg) {
                        rotation_overrides.insert(component.id, rotation);
                        exact_rotations.insert(component.id);
                    }
                }
            }
            for refdes in &violation.components {
                if let Some(component) = board.components.iter().find(|c| &c.refdes == refdes) {
                    implicated.insert(component.id);
                }
            }
        }

        // Rotate implicated footprints in a deterministic sequence. The
        // placer remains responsible for legal positions; the agent is not
        // allowed to invent arbitrary coordinates in this physical phase.
        for component in implicated {
            if exact_rotations.contains(&component) {
                continue;
            }
            let current_rot = rotation_overrides
                .get(&component)
                .copied()
                .unwrap_or(synth_geometry::Rotation::Zero);
            let next_rot = match current_rot {
                synth_geometry::Rotation::Zero => synth_geometry::Rotation::Ninety,
                synth_geometry::Rotation::Ninety => synth_geometry::Rotation::OneEighty,
                synth_geometry::Rotation::OneEighty => synth_geometry::Rotation::TwoSeventy,
                synth_geometry::Rotation::TwoSeventy => synth_geometry::Rotation::Zero,
            };
            rotation_overrides.insert(component, next_rot);
        }

        // Search progressively roomier deterministic floorplans. A dense
        // placement can make the maze router fail before DRC is relevant;
        // increasing the margin gives pin escapes and component corridors
        // physical room without asking the model to guess coordinates.
        margin += 1.0;
        if let Ok(p) =
            synth_place::place_with_tuning_and_sidecar(board, margin, &rotation_overrides, sidecar)
        {
            let r = route_with_optional_order(board, &p, routing_order);
            let score = physical_score(board, &p, &r, &profile);
            if score < best_score {
                best_placement = p;
                best_routing = r;
                best_score = score;
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

fn rotation_from_degrees(degrees: u32) -> Option<synth_geometry::Rotation> {
    match degrees % 360 {
        0 => Some(synth_geometry::Rotation::Zero),
        90 => Some(synth_geometry::Rotation::Ninety),
        180 => Some(synth_geometry::Rotation::OneEighty),
        270 => Some(synth_geometry::Rotation::TwoSeventy),
        _ => None,
    }
}

/// Score a physical candidate by the two properties that matter at export:
/// all nets must be connected and the independent manufacturer DRC must be
/// clean. This deliberately does not use the router's internal clearance
/// bookkeeping, so a router regression cannot make its own candidate appear
/// valid.
fn physical_score(
    board: &Board,
    placement: &synth_place::Placement,
    routing: &synth_route::Routing,
    profile: &synth_drc::ManufacturerProfile,
) -> (usize, usize) {
    // Routing completeness remains primary, but incomplete candidates also
    // need a geometry tie-breaker. Otherwise a candidate with fewer unrouted
    // nets can win while introducing dangling/obstructed copper that native
    // KiCad later reports as many more unconnected items.
    let unrouted = routing.unrouted_nets.len();
    let synth_violations = synth_drc::check(board, placement, routing, profile)
        .violations
        .len();
    (unrouted, synth_violations)
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
