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
    export_with_sidecars(board, out_dir, &Sidecars::default())
}

/// The two layout-override files an export applies, one per coordinate
/// space.
///
/// They share a file schema and nothing else: `schematic` positions are
/// sheet millimetres and drive the drawn sheet, `placement` positions are
/// board millimetres and drive the placer and router. They are separate
/// fields because passing one file for both is exactly the bug this split
/// fixed — the placement sidecar was auto-resolved for the schematic too, so
/// board-millimetre positions landed on the page, overflowed A2, and split
/// the export into one near-empty sheet per group.
#[derive(Debug, Clone, Default)]
pub struct Sidecars {
    /// `<design>.schematic.layout.toml` — sheet millimetres.
    pub schematic: Option<PathBuf>,
    /// `<design>.placement.layout.toml` — board millimetres.
    pub placement: Option<PathBuf>,
}

impl Sidecars {
    /// Both namespaces resolved from a `.synth` design path, honouring each
    /// kind's own convention.
    ///
    /// The preferred entry point: it is the only place that decides which
    /// file belongs to which coordinate space.
    pub fn resolve_for(design: &Path) -> Self {
        Self {
            schematic: synth_layout::schematic_sidecar_path(design),
            placement: synth_layout::placement_sidecar_path(design),
        }
    }

    fn schematic_path(&self) -> Option<&Path> {
        self.schematic.as_deref()
    }

    fn placement_path(&self) -> Option<&Path> {
        self.placement.as_deref()
    }
}

/// [`export`] honouring a single **schematic** layout sidecar (sheet
/// millimetres).
///
/// The path is treated as schematic-only: feeding board-millimetre
/// coordinates to the sheet is the failure this crate's two-namespace split
/// exists to prevent, so a lone path is never guessed to be a placement
/// sidecar. Pass [`Sidecars::placement`] explicitly when footprints must be
/// overridden too — [`export_with_sidecars`] is the form that can express
/// both. DSL `placement_hint`s declared on components are honoured inside
/// the placer itself and need no plumbing here.
///
/// # Errors
/// Same as [`export`].
pub fn export_with_sidecar(
    board: &Board,
    out_dir: &Path,
    sidecar: Option<&Path>,
) -> Result<ExportResult, ExportError> {
    export_with_sidecars(
        board,
        out_dir,
        &Sidecars {
            schematic: sidecar.map(Path::to_path_buf),
            placement: None,
        },
    )
}

/// [`export`] applying both layout-override namespaces independently.
///
/// # Errors
/// Same as [`export`].
pub fn export_with_sidecars(
    board: &Board,
    out_dir: &Path,
    sidecars: &Sidecars,
) -> Result<ExportResult, ExportError> {
    export_with_sidecars_and_routing_order(board, out_dir, sidecars, None)
}

/// Export with an optional agent-selected routing order, honouring a single
/// **schematic** layout sidecar.
///
/// The route used for export must be the same route that the MCP gate
/// inspected; silently recomputing with the default order can discard a
/// successful recovery pass and produce a different PCB artifact. See
/// [`export_with_sidecar`] for why the lone path is schematic-only, and
/// [`export_with_sidecars_and_routing_order`] for the both-namespaces form.
pub fn export_with_sidecar_and_routing_order(
    board: &Board,
    out_dir: &Path,
    sidecar: Option<&Path>,
    routing_order: Option<&[String]>,
) -> Result<ExportResult, ExportError> {
    export_with_sidecars_and_routing_order(
        board,
        out_dir,
        &Sidecars {
            schematic: sidecar.map(Path::to_path_buf),
            placement: None,
        },
        routing_order,
    )
}

/// [`export_with_sidecars_and_routing_order`] with both namespaces resolved
/// from a `.synth` design path.
///
/// # Errors
/// Same as [`export`].
pub fn export_for_design(
    board: &Board,
    out_dir: &Path,
    design: &Path,
    routing_order: Option<&[String]>,
) -> Result<ExportResult, ExportError> {
    let sidecars = Sidecars::resolve_for(design);
    export_with_sidecars_and_routing_order(board, out_dir, &sidecars, routing_order)
}

/// [`export`] applying both layout-override namespaces independently.
///
/// `routing_order` is accepted for source compatibility with callers that
/// still pass it and is deliberately not used. It selected the order in
/// which Synth's own router attempted nets, and that router no longer runs
/// on the export path: copper comes from an external engine, which chooses
/// its own order from the board it is given. The parameter stays so that an
/// MCP client sending it is not rejected; anything that wants a specific
/// net order says so to the router, not to the exporter.
pub fn export_with_sidecars_and_routing_order(
    board: &Board,
    out_dir: &Path,
    sidecars: &Sidecars,
    routing_order: Option<&[String]>,
) -> Result<ExportResult, ExportError> {
    let _ = routing_order;
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

    // Layout first: the sheet plan (§P26) decides whether the
    // schematic exports as one file (small boards, byte-identical to
    // before) or one file per sheet boundary (large boards only).
    // The project file below lists every sheet, so it comes after.
    let global_layout = synth_layout::layout_with_sidecar(board, sidecars.schematic_path());
    let sheets = synth_layout::sheets::layout_sheets(board, global_layout);
    let project_namespace = uuid_v5::project_namespace(&board.name);
    // Project file (.kicad_pro): minimal JSON. KiCad fills in the
    // rest on first open; the deterministic root keeps diffs stable.
    // Multi-sheet exports list every sub-sheet (uuid + file) after
    // the root entry; single-sheet keeps the historic one-entry list
    // byte for byte.
    let mut sheet_entries = vec![vec![
        uuid_v5::derive_entity_uuid(&project_namespace, "sheet", "root").to_string(),
        String::new(),
    ]];
    if sheets.len() > 1 {
        for sheet in &sheets {
            if let Some(name) = sheet.name.as_deref() {
                sheet_entries.push(vec![
                    crate::multisheet::sheet_uuid(&project_namespace, name).to_string(),
                    crate::multisheet::sheet_filename(&stem, name),
                ]);
            }
        }
    }
    // Design variants (KiCad 10): the project file carries the variant
    // *names* (with optional descriptions); the per-symbol population
    // overrides live in the schematic's `(variant …)` blocks. KiCad
    // always writes the array, empty when there are no variants.
    let variant_entries: Vec<serde_json::Value> = board
        .variants
        .iter()
        .map(|v| {
            let mut entry = json!({ "name": v.name });
            if let Some(desc) = &v.description {
                entry["description"] = json!(desc);
            }
            entry
        })
        .collect();
    // Net-class colours (schematic-quality plan Phase B1): KiCad reads
    // `net_settings` and colours wires *and* labels by class
    // automatically, so the encoding survives user edits and shows in
    // the netlist UI — unlike per-wire `(stroke (color …))`.
    // Across every sheet: a net's power symbol or label may live on
    // any page, and the project file is board-wide.
    let net_settings = build_net_settings(board, sheets.iter().map(|s| &s.layout));
    let project_doc = json!({
        // Every minimum the board's manufacturer profile carries, so
        // native DRC, the external router, and Synth's own validation all
        // read the same contract. See `design_settings`.
        "board": design_settings(board),
        "boards": [],
        "meta": {
            "filename": format!("{stem}.kicad_pro"),
            "version": 1,
            "uuid": project_namespace.to_string(),
        },
        "net_settings": net_settings,
        "schematic": {
            "annotate_start_num": 0,
            "drawing": {},
            "variants": variant_entries,
        },
        "sheets": sheet_entries,
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

    // Schematic file(s). The symbol library is built from the same
    // override-aware layout the schematic consumes so both files
    // agree on positions and power-flag label sets. One `.kicad_sch`
    // for small boards (§P26 splits large splittable boards only, so
    // this path stays byte-identical); one file per sheet otherwise.
    let library_text = symbol_lib::build_library(board, &sheets[0].layout).to_string_pretty();
    write_file(&library_path, &library_text)?;
    if sheets.len() == 1 {
        let schematic_text =
            schematic::build_schematic_from_layout(board, &project_namespace, &sheets[0].layout)
                .to_string_pretty();
        write_file(&schematic_path, &schematic_text)?;
    } else {
        crate::multisheet::export_sheets(board, out_dir, &stem, &project_namespace, sheets)?;
    }

    // PCB file (.kicad_pcb). Export must serialize the same deterministic
    // placement/routing candidate that the route and DRC tools inspect.
    // Additional export-only repair iterations used to silently select a
    // different placement, making routing feedback and the delivered PCB
    // disagree. Repair/retry belongs to the agent loop; export is a
    // serialization boundary.
    let placement = place_for_export(board, sidecars.placement_path())
        .map_err(|source| ExportError::Placement { source })?;
    // The export is the un-routed baseline. Copper is generated by an
    // external router, which reads this file and writes its own candidate
    // back; Synth does not synthesize traces here, so nothing downstream
    // can mistake them for router output.
    let pcb_text = pcb::build_pcb(
        board,
        &placement,
        &synth_pcb::Routing::default(),
        &project_namespace,
    )
    .to_string_pretty();
    write_file(&pcb_path, &pcb_text)?;

    // BOM CSV.
    let bom_text = bom::build_bom_csv(board);
    write_file(&bom_path, &bom_text)?;
    // One BOM per declared variant: same columns, with the variant's
    // do-not-populate overrides applied. `bom.csv` stays the base build.
    for variant in &board.variants {
        let variant_path = out_dir.join(format!("bom.{}.csv", sanitize_filename(&variant.name)));
        let variant_text = bom::build_bom_csv_for_variant(board, variant);
        write_file(&variant_path, &variant_text)?;
    }

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

/// Paths written by [`export_schematic_only`].
#[derive(Debug, Clone)]
pub struct SchematicExportResult {
    pub out_dir: PathBuf,
    pub project_path: PathBuf,
    pub schematic_path: PathBuf,
    pub library_path: PathBuf,
    /// Number of schematic sheets written: 1 unless a large splittable board
    /// took the §P26 multi-sheet path.
    pub sheet_count: usize,
}

/// Write only the schematic-side artifacts — project, symbol library,
/// `sym-lib-table`, and the `.kicad_sch` (or per-sheet files) — without
/// placing, routing, or emitting the PCB.
///
/// This exists for the visual-feedback loop: `kicad-cli sch export svg` needs
/// a real `.kicad_sch`, but a schematic review has no use for a routed board,
/// and running the PCB placer/router on every render would make the loop
/// needlessly slow. The output is byte-identical to the schematic-side files
/// [`export_with_sidecar_and_routing_order`] writes for the same board; a
/// test pins the two together.
///
/// # Errors
/// Same I/O and placement-file errors as [`export`].
pub fn export_schematic_only(
    board: &Board,
    out_dir: &Path,
    schematic_sidecar: Option<&Path>,
) -> Result<SchematicExportResult, ExportError> {
    std::fs::create_dir_all(out_dir).map_err(|source| ExportError::CreateDir {
        path: out_dir.to_path_buf(),
        source,
    })?;

    let stem = sanitize_filename(&board.name);
    let project_path = out_dir.join(format!("{stem}.kicad_pro"));
    let schematic_path = out_dir.join(format!("{stem}.kicad_sch"));
    let library_path = out_dir.join(format!("{stem}.kicad_sym"));

    let global_layout = synth_layout::layout_with_sidecar(board, schematic_sidecar);
    let sheets = synth_layout::sheets::layout_sheets(board, global_layout);
    let project_namespace = uuid_v5::project_namespace(&board.name);

    let mut sheet_entries = vec![vec![
        uuid_v5::derive_entity_uuid(&project_namespace, "sheet", "root").to_string(),
        String::new(),
    ]];
    if sheets.len() > 1 {
        for sheet in &sheets {
            if let Some(name) = sheet.name.as_deref() {
                sheet_entries.push(vec![
                    crate::multisheet::sheet_uuid(&project_namespace, name).to_string(),
                    crate::multisheet::sheet_filename(&stem, name),
                ]);
            }
        }
    }
    let variant_entries: Vec<serde_json::Value> = board
        .variants
        .iter()
        .map(|v| {
            let mut entry = json!({ "name": v.name });
            if let Some(desc) = &v.description {
                entry["description"] = json!(desc);
            }
            entry
        })
        .collect();
    let net_settings = build_net_settings(board, sheets.iter().map(|s| &s.layout));
    let project_doc = json!({
        // Every minimum the board's manufacturer profile carries, so
        // native DRC, the external router, and Synth's own validation all
        // read the same contract. See `design_settings`.
        "board": design_settings(board),
        "boards": [],
        "meta": {
            "filename": format!("{stem}.kicad_pro"),
            "version": 1,
            "uuid": project_namespace.to_string(),
        },
        "net_settings": net_settings,
        "schematic": {
            "annotate_start_num": 0,
            "drawing": {},
            "variants": variant_entries,
        },
        "sheets": sheet_entries,
    });
    let project_text = serde_json::to_string_pretty(&project_doc)
        .map_err(|source| ExportError::SerializeProject { source })?;
    write_file(&project_path, &project_text)?;

    let sym_table = format!(
        "(sym_lib_table\n  (version 7)\n  (lib\n    (name \"synth\")\n    (uri \"${{KIPRJMOD}}/{stem}.kicad_sym\")\n    (type \"KiCad\")\n    (options \"\")\n    (descr \"Synth synthesized symbol library\")\n  )\n)\n"
    );
    write_file(&out_dir.join("sym-lib-table"), &sym_table)?;

    let library_text = symbol_lib::build_library(board, &sheets[0].layout).to_string_pretty();
    write_file(&library_path, &library_text)?;
    let sheet_count = sheets.len();
    if sheet_count == 1 {
        let schematic_text =
            schematic::build_schematic_from_layout(board, &project_namespace, &sheets[0].layout)
                .to_string_pretty();
        write_file(&schematic_path, &schematic_text)?;
    } else {
        crate::multisheet::export_sheets(board, out_dir, &stem, &project_namespace, sheets)?;
    }

    Ok(SchematicExportResult {
        out_dir: out_dir.to_path_buf(),
        project_path,
        schematic_path,
        library_path,
        sheet_count,
    })
}

pub(crate) fn write_file(path: &Path, contents: &str) -> Result<(), ExportError> {
    std::fs::write(path, contents).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Map an arbitrary board name to a filesystem-safe filename stem.
/// Replaces any character outside `[A-Za-z0-9_-]` with `_`.
pub(crate) fn sanitize_filename(name: &str) -> String {
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

/// Placement for the export boundary.
///
/// Runs the same deterministic tuning the placer uses elsewhere, then
/// evaluates the agent's `placement_hint`s once against the board with
/// hints removed and keeps the relaxed candidate when it is physically
/// better. That guard is about placement legality — courtyard overlap,
/// connector orientation, silkscreen — rather than routability, because
/// the export path has no router to ask about routability. Routing
/// constraints are enforced by the external router downstream, which sees
/// the finished board and can refuse it.
fn place_for_export(
    board: &Board,
    sidecar: Option<&Path>,
) -> Result<synth_place::Placement, synth_place::PlaceError> {
    // Match synth-place::place() and the MCP gates. Starting the export
    // with a different courtyard margin can select a different legal
    // placement, making the exported PCB disagree with the placement the
    // agent just reviewed.
    let margin = 1.5_f64;
    // Flash packages are oriented so their QSPI edge faces the MCU; this
    // override has to be applied here too or export reruns placement with
    // a different orientation than the placer just used.
    let rotation_overrides = flash_rotation_overrides(board);
    let hinted =
        synth_place::place_with_tuning_and_sidecar(board, margin, &rotation_overrides, sidecar)?;
    let mut candidates = vec![hinted];

    // The hinted placement is not automatically the better one. A hint pins a
    // part where the author asked, which can be worse for routing than where
    // the solver would have put it; the export path has no router to ask, so
    // it decides on legality first and on the route-aware estimate second.
    // Only a candidate that is no less legal is allowed to win.
    if hints_are_all_soft(board) {
        let mut unhinted = board.clone();
        for component in &mut unhinted.components {
            component.placement_hint = None;
        }
        if let Ok(candidate) = synth_place::place_with_tuning_and_sidecar(
            &unhinted,
            margin,
            &rotation_overrides,
            sidecar,
        ) {
            candidates.push(candidate);
        }
    }

    Ok(candidates
        .into_iter()
        .min_by_key(|placement| placement_key(board, placement))
        .expect("the hinted placement is always a candidate"))
}

/// Ordering key for candidate placements: legality, then routability.
///
/// Violations lead — an illegal placement is never preferred, however short
/// its copper — then net hazards, then total copper.
fn placement_key(board: &Board, placement: &synth_place::Placement) -> (usize, usize, u64) {
    let violations = placement_score(board, placement);
    let estimate = synth_place::routability(board, placement);
    (
        violations,
        estimate.overlong_nets,
        estimate.copper_length_nm,
    )
}

/// Orientation overrides for QSPI flash on an RP2350 board.
/// Flash packages are oriented so their QSPI edge faces the MCU; this
/// override has to be applied on the export path too, or export reruns
/// placement with a different orientation than the placer just used.
fn flash_rotation_overrides(
    board: &Board,
) -> std::collections::HashMap<synth_ir::ComponentId, synth_geometry::Rotation> {
    let has_rp2350 = board.components.iter().any(|component| {
        component
            .part
            .as_ref()
            .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("rp2350"))
    });
    board
        .components
        .iter()
        .filter(|component| {
            has_rp2350
                && component
                    .part
                    .as_ref()
                    .is_some_and(|part| part.id.0.to_ascii_lowercase().contains("w25q"))
        })
        .map(|component| (component.id, synth_geometry::Rotation::Zero))
        .collect()
}

/// Whether the board carries hints and every one of them is soft.
///
/// A hard hint is a contract — an edge connector stays on its edge, a
/// decoupling cap stays by its pin — and routability may not talk the export
/// path out of it. Soft hints are preferences, and those may be relaxed for a
/// placement that routes better.
fn hints_are_all_soft(board: &Board) -> bool {
    let mut saw_hint = false;
    for component in &board.components {
        if let Some(hint) = &component.placement_hint {
            saw_hint = true;
            if hint.priority == synth_ir::PlacementPriority::Hard {
                return false;
            }
        }
    }
    saw_hint
}

/// Count of placement-only DRC violations.
///
/// Evaluated against an empty copper set, so only the rules that do not
/// depend on traces can fire — which is exactly the set the export path
/// can act on before a router has run.
fn placement_score(board: &Board, placement: &synth_place::Placement) -> usize {
    let profile = synth_drc::ManufacturerProfile::jlc_standard();
    synth_drc::check(board, placement, &synth_pcb::Routing::default(), &profile)
        .violations
        .len()
}

/// The `board.design_settings` block for a board's declared manufacturer.
///
/// Every minimum the profile carries is written out, because three parties
/// read these numbers and they have to be the same numbers:
///
/// - `kicad-cli pcb drc`, whose rule has nothing to check against when the
///   key is absent;
/// - the external router, which is asked to preserve "the board's declared
///   minimums" and can only preserve what the project states — KiCad writes
///   `0` for *not configured*, and a router reads that as *unset*;
/// - [`synth_router::validate`], which judges the returned copper against
///   this same profile.
///
/// Writing only clearance and track width is what made the manufacturing
/// stage unpassable: with no `min_through_hole_diameter` the router had no
/// drill floor to hold, took the 0.15 mm rung its fabrication tier allows,
/// and Synth then rejected the board against the profile's 0.3 mm. Native
/// DRC passed the same board, having been given no drill rule either.
fn design_settings(board: &Board) -> serde_json::Value {
    let profile = synth_drc::ManufacturerProfile::from_name(
        board.manufacturer.as_deref().unwrap_or("jlc-standard"),
    );
    let mm = synth_geometry::nm_to_mm;
    // KiCad enforces the annular ring as (diameter - drill) / 2, so the
    // smallest legal via diameter follows from the two minima the profile
    // does carry rather than being a fourth number to pick.
    let min_via_diameter = mm(profile.min_drill_diameter_nm + 2 * profile.min_annular_ring_nm);
    let rules = json!({
        "min_clearance": mm(profile.min_copper_clearance_nm),
        "min_track_width": mm(profile.min_trace_width_nm),
        "min_through_hole_diameter": mm(profile.min_drill_diameter_nm),
        "min_via_annular_width": mm(profile.min_annular_ring_nm),
        "min_via_diameter": min_via_diameter,
        "min_copper_edge_clearance": mm(profile.min_copper_to_edge_nm),
        "min_hole_clearance": mm(profile.min_drill_to_copper_nm),
    });
    // `defaults` seeds Board Setup for a human opening the project;
    // `rules` is the set DRC and the router read. KiCad 10 may discard
    // legacy setup minima when it first saves a generated board, and an
    // empty `board` object then silently restores its own 0.2 mm
    // clearance, so both are stated.
    json!({
        "design_settings": {
            "defaults": rules.clone(),
            "rules": rules,
        }
    })
}

/// Build the `.kicad_pro` `net_settings` block (schematic-quality
/// plan Phase B1).
///
/// Emits one class entry per class the board actually uses (fixed
/// semantic classes first, then author-declared ones, `Default`
/// always present), each with `schematic_color` / `pcb_color` from the
/// deterministic palette, plus `netclass_assignments` mapping every
/// non-Default net to its class. KiCad then colours wires and labels
/// automatically and preserves the encoding across edits.
fn build_net_settings<'a>(
    board: &Board,
    layouts: impl Iterator<Item = &'a synth_layout::Layout>,
) -> serde_json::Value {
    use std::collections::BTreeMap;

    let assignments = synth_layout::netclass::classify_nets(board);
    let mut names = synth_layout::netclass::class_names(board, &assignments);
    if !names.iter().any(|n| n == "Default") {
        names.push("Default".to_string());
    }
    // Class → hue: assignment colours first (fixed palette / declared
    // override), then declared classes with no member net.
    let mut colors: BTreeMap<String, [u8; 3]> = BTreeMap::new();
    for a in &assignments {
        colors.insert(a.class.clone(), a.color);
    }
    for nc in &board.netclasses {
        colors.entry(nc.name.clone()).or_insert_with(|| {
            nc.color
                .unwrap_or_else(|| synth_layout::netclass::net_class_color(&nc.name))
        });
    }
    // Declared width/clearance per class, where the author set them.
    let mut rules: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for nc in &board.netclasses {
        rules.insert(
            nc.name.as_str(),
            (
                nc.trace_width.map_or(0.2, synth_ir::Length::to_mm),
                nc.clearance.map_or(0.2, synth_ir::Length::to_mm),
            ),
        );
    }

    let pairs: BTreeMap<&str, (f64, f64)> = board
        .netclasses
        .iter()
        .filter_map(|nc| {
            Some((
                nc.name.as_str(),
                crate::pcb::declared_pair_geometry(board, nc)?,
            ))
        })
        .collect();

    let rgba = |rgb: [u8; 3]| format!("rgba({}, {}, {}, 1.000)", rgb[0], rgb[1], rgb[2]);
    let mut classes = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let rgb = colors
            .get(name)
            .copied()
            .unwrap_or_else(|| synth_layout::netclass::net_class_color(name));
        let (track_width, clearance) = rules.get(name.as_str()).copied().unwrap_or((0.2, 0.2));
        let (pair_width, pair_gap) = pairs.get(name.as_str()).copied().unwrap_or((0.2, 0.25));
        // KiCad gives Default the max priority so it always loses to a
        // specific class; specific classes count up from 0.
        let priority = if name == "Default" {
            i64::from(i32::MAX)
        } else {
            i64::try_from(index).unwrap_or(i64::from(i32::MAX) - 1)
        };
        classes.push(json!({
            "bus_width": 12,
            "clearance": clearance,
            "diff_pair_gap": pair_gap,
            "diff_pair_via_gap": 0.25,
            "diff_pair_width": pair_width,
            "line_style": 0,
            "microvia_diameter": 0.3,
            "microvia_drill": 0.1,
            "name": name,
            "pcb_color": rgba(rgb),
            "priority": priority,
            "schematic_color": rgba(rgb),
            "track_width": track_width,
            "via_diameter": 0.6,
            "via_drill": 0.4,
            "wire_width": 6,
        }));
    }
    // Assignments are keyed on the name *KiCad* will know the net by,
    // not our IR name: KiCad derives net names from the drawing at
    // load time, so an assignment written against `net_7` matches
    // nothing. `kicad_net_names` maps a net to its power-symbol value
    // (`+3V3`) or its label spellings (`/SDA`, `SDA`); a glob pattern
    // covers the same label on a deeper sheet path. Nets with neither
    // are auto-named by KiCad and unreachable this way — the explicit
    // per-wire stroke in `schematic.rs` is what colours those.
    let mut visible: BTreeMap<synth_ir::NetId, Vec<String>> = BTreeMap::new();
    for layout in layouts {
        for (net, names) in synth_layout::netclass::kicad_net_names(layout) {
            let entry = visible.entry(net).or_default();
            entry.extend(names);
        }
    }
    for names in visible.values_mut() {
        names.sort();
        names.dedup();
    }
    let mut assignments_json = serde_json::Map::new();
    let mut patterns = Vec::new();
    for a in &assignments {
        if a.class == "Default" {
            continue;
        }
        let Some(names) = visible.get(&a.net) else {
            continue;
        };
        for name in names {
            assignments_json.insert(name.clone(), json!(a.class));
            if let Some(bare) = name.strip_prefix('/') {
                patterns.push(json!({ "netclass": a.class, "pattern": format!("/*/{bare}") }));
            }
        }
    }
    let netclass_assignments = if assignments_json.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Object(assignments_json)
    };
    json!({
        "classes": classes,
        "meta": { "version": 4 },
        "net_colors": serde_json::Value::Null,
        "netclass_assignments": netclass_assignments,
        "netclass_patterns": patterns,
    })
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

    fn board_from(src: &str) -> Board {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .unwrap();
        let registry = synth_registry::load_dir(&root.join("registry").join("parts")).unwrap();
        let parsed = synth_parser::parse(src, "inline.synth");
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        synth_ir::lower(&parsed.ast.unwrap(), &registry, "inline.synth")
            .board
            .unwrap()
    }

    #[test]
    fn net_settings_colours_i2c_and_power_classes() {
        let board = board_from(
            r#"board "b" {
                component U1: mcu "stm32f103c8"
                component U2: sensor "bme680_env"
                component R1: resistor "r_generic_0603" value "4.7k"
                component R2: resistor "r_generic_0603" value "4.7k"
                connect U1.pb6 -> U2.scl
                connect U1.pb6 -> R1.p1
                connect R1.p2 -> U2.vdd
                connect U1.pb7 -> U2.sda
                connect U1.pb7 -> R2.p1
                connect R2.p2 -> U2.vdd
                connect U2.vdd -> U1.vdd
                connect U1.vss -> U2.gnd
            }"#,
        );
        let settings = build_net_settings(&board, std::iter::once(&synth_layout::layout(&board)));
        let classes = settings["classes"].as_array().unwrap();
        let names: Vec<&str> = classes
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"I2C"), "I2C class missing: {names:?}");
        assert!(names.contains(&"Power"), "Power class missing: {names:?}");
        assert!(names.contains(&"Default"), "Default always present");
        // Deterministic palette hue, rgba-encoded.
        let i2c = classes
            .iter()
            .find(|c| c["name"] == "I2C")
            .expect("I2C class");
        assert_eq!(i2c["schematic_color"], "rgba(0, 114, 178, 1.000)");
        assert_eq!(i2c["pcb_color"], i2c["schematic_color"]);
        // Every SCL/SDA net is assigned to I2C.
        let assignments = settings["netclass_assignments"].as_object().unwrap();
        assert!(
            assignments.values().any(|v| v == "I2C"),
            "an I2C net must be assigned: {assignments:?}"
        );
    }

    /// End-to-end: a project file carrying `net_settings` alongside its
    /// schematic must load in `kicad-cli` (it reads the project for
    /// net-class colours). Skips when KiCad is not installed.
    #[test]
    fn project_with_net_settings_loads_in_kicad() {
        let board = board_from(
            r##"board "b" {
                netclass "PWR" {
                    trace_width 0.5mm
                    color "#c2410c"
                }
                component U1: regulator "ams1117_3v3"
                component C1: capacitor "c_generic_0805" value "10uF"
                component C2: capacitor "c_generic_0805" value "10uF"
                connect U1.vout -> C1.p1
                connect U1.gnd -> C1.p2
                connect U1.vout -> C2.p1 as "PWR"
                connect U1.gnd -> C2.p2
            }"##,
        );
        let dir = std::env::temp_dir().join(format!("synth-b1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stem = sanitize_filename(&board.name);
        let project = crate::uuid_v5::project_namespace(&board.name);
        let sch = crate::schematic::build_schematic(&board, &project).to_string_pretty();
        let sch_path = dir.join(format!("{stem}.kicad_sch"));
        std::fs::write(&sch_path, &sch).unwrap();
        let pro = serde_json::to_string_pretty(&json!({
            "meta": { "filename": format!("{stem}.kicad_pro"), "version": 1,
                      "uuid": project.to_string() },
            "net_settings": build_net_settings(&board, std::iter::once(&synth_layout::layout(&board))),
            "sheets": [],
        }))
        .unwrap();
        std::fs::write(dir.join(format!("{stem}.kicad_pro")), &pro).unwrap();

        let erc = crate::run_kicad_erc(&sch_path);
        match erc.evidence.reason {
            Some(synth_diagnostics::UnknownReason::NotInstalled) => {
                eprintln!("kicad-cli not installed; skipping net_settings load test");
            }
            Some(_) => panic!(
                "project with net_settings must load: {}",
                erc.evidence.summary_line()
            ),
            None => {}
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn net_settings_honours_declared_colour() {
        let board = board_from(
            r##"board "b" {
                netclass "PWR" {
                    trace_width 0.5mm
                    clearance 0.2mm
                    color "#c2410c"
                }
                component U1: regulator "ams1117_3v3"
                component C1: capacitor "c_generic_0805" value "10uF"
                component C2: capacitor "c_generic_0805" value "10uF"
                connect U1.vout -> C1.p1
                connect U1.gnd -> C1.p2
                connect U1.vout -> C2.p1 as "PWR"
                connect U1.gnd -> C2.p2
            }"##,
        );
        let settings = build_net_settings(&board, std::iter::once(&synth_layout::layout(&board)));
        let classes = settings["classes"].as_array().unwrap();
        let pwr = classes
            .iter()
            .find(|c| c["name"] == "PWR")
            .expect("declared PWR class present");
        assert_eq!(pwr["schematic_color"], "rgba(194, 65, 12, 1.000)");
        // Declared width/clearance flow through.
        assert_eq!(pwr["track_width"], 0.5);
        assert_eq!(pwr["clearance"], 0.2);
    }
}
