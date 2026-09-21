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

    // PCB file (.kicad_pcb). Closed-loop repair engine (Phase 13.5):
    // runs placement and routing with up to 5 repair iterations.
    // The sidecar (manual drags) rides along with every placement
    // attempt so routing sees the overridden positions.
    let (placement, routing) = place_and_route_with_repair(board, sidecar)
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

/// Extra courtyard margin used by the baseline placement
/// (`synth_place::place_with_sidecar`). The repair sweep starts above
/// it so no candidate duplicates the baseline.
const BASELINE_MARGIN_MM: f64 = 1.5;

/// Worker-thread count for sizing the repair waves. Set once from the
/// CLI's `--jobs` flag (which sizes the Rayon pool identically); 0 means
/// "not set", in which case the wave width falls back to the machine's
/// available parallelism (the Rayon default pool size).
static WORKER_THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Record the worker-thread count `--jobs` (or its default) resolved to.
/// Idempotent: the first call wins, so tests calling [`export`] directly
/// keep the fallback.
pub fn set_worker_threads(n: usize) {
    let _ = WORKER_THREADS.compare_exchange(
        0,
        n.max(1),
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// How many candidates a repair wave evaluates concurrently.
///
/// Measured on a 16-core box (`sensor_logger`, release): width 4 → 45 s,
/// width 8 → 65 s, width 16 → 89 s. The maze router is memory-bandwidth
/// bound and a wave waits for its slowest candidate, so concurrency past
/// a handful buys search coverage at a wall-clock cost — it never speeds
/// the wave up. Default is therefore min(threads, 8): enough distinct
/// margin × rotation × order-seed candidates to keep a typical machine
/// busy and to let the perfect-score short-circuit skip later waves,
/// without the contention collapse of full-machine width. `SYNTH_WAVE_WIDTH`
/// overrides (wider search, `--jobs` scales the pool to match); `--jobs 4`
/// or less is the fastest wall-clock for this workload.
fn wave_width() -> usize {
    if let Ok(raw) = std::env::var("SYNTH_WAVE_WIDTH") {
        if let Ok(n) = raw.parse::<usize>() {
            return n.clamp(1, 32);
        }
    }
    let configured = WORKER_THREADS.load(std::sync::atomic::Ordering::Relaxed);
    let threads = if configured != 0 {
        configured
    } else {
        std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
    };
    threads.clamp(1, 8)
}

/// One point in the repair search space: how much extra courtyard
/// margin the placer gets, which footprints are pre-rotated, and which
/// deterministic net-order variant the router runs.
#[derive(Debug)]
struct RepairCandidate {
    margin_mm: f64,
    rotations: std::collections::HashMap<synth_ir::ComponentId, synth_geometry::Rotation>,
    /// Deterministic router order variant (0 = canonical). Non-zero seeds
    /// explore different congestion resolutions in parallel; same seed →
    /// identical routes on any thread count.
    order_seed: u64,
    /// Pre-computed placement, when the caller already has one (the
    /// baseline). `None` means this candidate places itself.
    placement: Option<synth_place::Placement>,
}

/// A candidate that survived placement, with its physical score.
#[derive(Debug)]
struct ScoredCandidate {
    index: usize,
    placement: synth_place::Placement,
    routing: synth_route::Routing,
    score: (usize, usize),
}

/// Place, route, and score one candidate. Pure: reads `board` and the
/// sidecar file, touches no shared state, so a whole wave of candidates
/// evaluates concurrently.
fn evaluate_candidate(
    board: &Board,
    index: usize,
    candidate: &RepairCandidate,
    sidecar: Option<&Path>,
    profile: &synth_drc::ManufacturerProfile,
) -> Option<ScoredCandidate> {
    let placement = match &candidate.placement {
        Some(p) => p.clone(),
        None => synth_place::place_with_tuning_and_sidecar(
            board,
            candidate.margin_mm,
            &candidate.rotations,
            sidecar,
        )
        .ok()?,
    };
    let routing = synth_route::route_with_order_seed(board, &placement, candidate.order_seed);
    let score = physical_score(board, &placement, &routing, profile);
    Some(ScoredCandidate {
        index,
        placement,
        routing,
        score,
    })
}

/// Evaluate a wave of independent candidates across the Rayon pool and
/// return the winner: lowest [`physical_score`], ties broken by
/// candidate index.
///
/// Determinism is preserved despite the parallelism. Every candidate is
/// a pure function of `(board, candidate, sidecar)`, the candidate list
/// is built deterministically, and the winner is chosen by
/// `(score, index)` rather than by completion order — so the result does
/// not depend on the thread count or on scheduling.
///
/// `perfect_index` is the short-circuit: `(0, 0)` is the minimum possible
/// score, so once a candidate reaches it every *higher-indexed* candidate
/// is already beaten on the tie-break and can be abandoned before it
/// pays for a placement and a route. Lower-indexed candidates always run,
/// which is what keeps the tie-break honest.
fn evaluate_wave(
    board: &Board,
    candidates: &[RepairCandidate],
    sidecar: Option<&Path>,
    profile: &synth_drc::ManufacturerProfile,
) -> Option<ScoredCandidate> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let perfect_index = AtomicUsize::new(usize::MAX);

    candidates
        .par_iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            if index > perfect_index.load(Ordering::Relaxed) {
                return None;
            }
            let scored = evaluate_candidate(board, index, candidate, sidecar, profile)?;
            if scored.score == (0, 0) {
                perfect_index.fetch_min(index, Ordering::Relaxed);
            }
            Some(scored)
        })
        .min_by_key(|scored| (scored.score, scored.index))
}

/// Components that the router or the independent DRC engine implicates
/// in a failure, plus the exact orientations DRC rules asked for.
///
/// A routed net can still fail fabrication clearance, courtyard,
/// connector-orientation, or silkscreen rules, so an unrouted-only retry
/// loop can converge on a board KiCad rejects — both evidence sources
/// feed the rotation sweep.
fn implicated_components(
    board: &Board,
    placement: &synth_place::Placement,
    routing: &synth_route::Routing,
    profile: &synth_drc::ManufacturerProfile,
) -> (
    std::collections::BTreeSet<synth_ir::ComponentId>,
    std::collections::HashMap<synth_ir::ComponentId, synth_geometry::Rotation>,
) {
    // Avoid the more expensive independent geometry checks for a
    // candidate that has not connected every routable net yet.
    let drc = if routing.unrouted_nets.is_empty() {
        synth_drc::check(board, placement, routing, profile)
    } else {
        synth_drc::DrcReport {
            violations: Vec::new(),
            profile_name: profile.name.clone(),
        }
    };

    let mut implicated = std::collections::BTreeSet::new();
    let mut exact_rotations = std::collections::HashMap::new();

    for unrouted in &routing.unrouted_nets {
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
                // directly; the stepped rotation below is reserved for
                // violations that only identify a witness component.
                if let Some(rotation) = rotation_from_degrees(suggestion.rotation_deg) {
                    exact_rotations.insert(component.id, rotation);
                }
            }
        }
        for refdes in &violation.components {
            if let Some(component) = board.components.iter().find(|c| &c.refdes == refdes) {
                implicated.insert(component.id);
            }
        }
    }

    (implicated, exact_rotations)
}

/// Advance a footprint `steps` quarter-turns from its default orientation.
fn rotation_after_steps(steps: usize) -> synth_geometry::Rotation {
    match steps % 4 {
        0 => synth_geometry::Rotation::Zero,
        1 => synth_geometry::Rotation::Ninety,
        2 => synth_geometry::Rotation::OneEighty,
        _ => synth_geometry::Rotation::TwoSeventy,
    }
}

/// Extra courtyard margins, in millimetres, tried by the first two
/// repair waves. A dense placement can make the maze router fail before
/// DRC is even relevant; more margin buys pin escapes and component
/// corridors physical room.
const MARGIN_SWEEP_MM: [f64; 3] = [2.0, 3.0, 4.0];

/// Full margin ladder cycled by the thread-scaled wave-1 sweep (and the
/// rotation sweep). Starts with [`MARGIN_SWEEP_MM`] in order so the first
/// candidates — and therefore historic tie-breaks — are unchanged.
const SWEEP_MARGINS_MM: [f64; 8] = [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];

/// Extra courtyard margins for the last-resort wave: open the floorplan
/// up far wider, for boards the first two waves could not resolve.
const WIDE_MARGIN_SWEEP_MM: [f64; 4] = [6.0, 7.0, 8.0, 9.0];

/// Closed-loop placer-router repair search.
///
/// Placement and routing are attempted against a deterministic set of
/// candidate configurations in up to three escalating waves:
///
/// 1. the baseline placement plus a thread-scaled margin × order-seed
///    sweep over [`MARGIN_SWEEP_MM`] and roomier margins;
/// 2. those same margins with the footprints that router and DRC
///    evidence implicate rotated through a quarter-turn sequence;
/// 3. the much roomier [`WIDE_MARGIN_SWEEP_MM`] floorplans.
///
/// Each wave stops the search as soon as a candidate scores a clean
/// `(0, 0)`, so the cheap common case never pays for the later ones. The
/// placer stays responsible for legal positions; nothing here invents
/// coordinates.
///
/// The candidates within a wave are independent, so a wave is evaluated
/// in parallel across the Rayon pool (sized by `--jobs` /
/// `RAYON_NUM_THREADS`, all cores by default) rather than one candidate
/// per sequential repair iteration. Because the winner is picked by
/// `(score, candidate index)` and every candidate is a pure function of
/// its inputs, the chosen board does not depend on the thread count or on
/// scheduling.
///
/// Wave widths are deliberately near the sweet spot rather than as wide
/// as the machine allows: a wave costs as long as its slowest candidate,
/// and the router is memory-bandwidth bound, so candidates past a handful
/// slow each other down more than they add coverage.
///
/// `sidecar` (when present) is applied to every placement attempt
/// *before* routing so manual drags survive the repair search and the
/// router plans traces against the overridden footprint positions.
/// DSL `placement_hint`s need no explicit handling here: the placer
/// reads them from the IR board directly.
fn place_and_route_with_repair(
    board: &Board,
    sidecar: Option<&Path>,
) -> Result<(synth_place::Placement, synth_route::Routing), synth_place::PlaceError> {
    let profile = synth_drc::ManufacturerProfile::jlc_standard();

    // The baseline placement is the one attempt whose failure is fatal:
    // if the board cannot be placed at all, no margin sweep will help.
    // Placement is cheap next to routing, so doing it up front to keep
    // that error contract costs nothing.
    let baseline = synth_place::place_with_sidecar(board, sidecar)?;

    // ── Wave 1 — baseline first, then margin sweep ───────────────────
    // The common case is a clean baseline: route it alone before paying
    // for the margin sweep. `evaluate_candidate` is pure, so this is the
    // same winner wave 1 would pick — without burning 3 parallel routes
    // on boards that never needed them.
    let baseline_candidate = RepairCandidate {
        margin_mm: BASELINE_MARGIN_MM,
        rotations: std::collections::HashMap::new(),
        order_seed: 0,
        placement: Some(baseline),
    };
    let baseline_scored = evaluate_candidate(board, 0, &baseline_candidate, sidecar, &profile)
        .expect("baseline candidate is pre-placed, so it always scores");
    if baseline_scored.score == (0, 0) {
        return Ok((baseline_scored.placement, baseline_scored.routing));
    }
    // Thread-scaled sweep: one candidate per worker thread, cycling the
    // margin ladder and then the order seeds. The first three are exactly
    // the old (2, 3, 4 mm × canonical order) set in order, so ties still
    // break toward the historic winner regardless of thread count.
    let width = wave_width();
    eprintln!("synth: wave 1: routing {width} margin/order candidates on {width} threads");
    let sweep: Vec<RepairCandidate> = (0..width)
        .map(|i| RepairCandidate {
            margin_mm: SWEEP_MARGINS_MM[i % SWEEP_MARGINS_MM.len()],
            rotations: std::collections::HashMap::new(),
            order_seed: (i / SWEEP_MARGINS_MM.len()) as u64,
            placement: None,
        })
        .collect();
    let baseline_score = baseline_scored.score;
    let mut best = baseline_scored;
    let mut knob_helped = false;
    if let Some(candidate) = evaluate_wave(board, &sweep, sidecar, &profile) {
        // Strict improvement only: the baseline wins ties, so the sweep
        // must strictly beat it to displace it.
        if candidate.score < baseline_score {
            best = candidate;
            knob_helped = true;
        }
    }
    if best.score == (0, 0) {
        return Ok((best.placement, best.routing));
    }

    // Stagnation guard, in the same spirit as the sequential loop this
    // replaces: a wave that beat the baseline is evidence its knob is the
    // right lever, and only then is the expensive escalation worth its
    // wall-clock.

    // ── Wave 2 — the same margins, implicated footprints rotated ───────
    let (implicated, exact_rotations) =
        implicated_components(board, &best.placement, &best.routing, &profile);

    if !implicated.is_empty() {
        // First three are the old (2, 3, 4 mm × steps 1, 2, 3) set, so
        // historic tie-breaks are preserved; extras vary margin, step,
        // and order seed to keep every core on distinct work.
        eprintln!("synth: wave 2: routing {width} rotation candidates on {width} threads");
        let rotated: Vec<RepairCandidate> = (0..width)
            .map(|i| RepairCandidate {
                margin_mm: SWEEP_MARGINS_MM[i % MARGIN_SWEEP_MM.len()],
                rotations: rotation_overrides(&implicated, &exact_rotations, (i % 3) + 1),
                order_seed: (i / (MARGIN_SWEEP_MM.len() * 3)) as u64,
                placement: None,
            })
            .collect();

        if let Some(candidate) = evaluate_wave(board, &rotated, sidecar, &profile) {
            // Strict improvement only: the earlier wave's winner is kept
            // on a tie, so a rotation sweep never displaces an equally
            // good un-rotated board.
            if candidate.score < best.score {
                best = candidate;
                knob_helped = true;
            }
        }
        if best.score == (0, 0) {
            return Ok((best.placement, best.routing));
        }
    }

    // Neither more room nor a rotation moved the score off the baseline,
    // so the board is not margin-limited and a far roomier floorplan will
    // not help either. Stop rather than burn another wave on it.
    if !knob_helped {
        return Ok((best.placement, best.routing));
    }

    // ── Wave 3 — last resort: a much roomier floorplan ─────────────────
    // Rotations a DRC rule named exactly (a connector mating edge, say)
    // ride along; the speculative quarter-turns do not, since wave 2
    // already ruled them out at these implicated components.
    eprintln!("synth: wave 3: routing {width} wide-floorplan candidates on {width} threads");
    let wide: Vec<RepairCandidate> = (0..width)
        .map(|i| RepairCandidate {
            margin_mm: WIDE_MARGIN_SWEEP_MM[i % WIDE_MARGIN_SWEEP_MM.len()],
            rotations: exact_rotations.clone(),
            order_seed: (i / WIDE_MARGIN_SWEEP_MM.len()) as u64,
            placement: None,
        })
        .collect();

    if let Some(candidate) = evaluate_wave(board, &wide, sidecar, &profile) {
        if candidate.score < best.score {
            best = candidate;
        }
    }

    Ok((best.placement, best.routing))
}

/// Build the rotation map for a wave-2 candidate: every implicated
/// footprint turned `steps` quarter-turns, except those a DRC rule pinned
/// to an exact orientation.
fn rotation_overrides(
    implicated: &std::collections::BTreeSet<synth_ir::ComponentId>,
    exact_rotations: &std::collections::HashMap<synth_ir::ComponentId, synth_geometry::Rotation>,
    steps: usize,
) -> std::collections::HashMap<synth_ir::ComponentId, synth_geometry::Rotation> {
    implicated
        .iter()
        .map(|component| {
            let rotation = exact_rotations
                .get(component)
                .copied()
                .unwrap_or_else(|| rotation_after_steps(steps));
            (*component, rotation)
        })
        .collect()
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
    // Routing completeness is the primary gate. Avoid running the more
    // expensive independent geometry checks for candidates that have not
    // connected every routable net yet.
    if !routing.unrouted_nets.is_empty() {
        return (routing.unrouted_nets.len(), usize::MAX);
    }
    (
        0,
        synth_drc::check(board, placement, routing, profile)
            .violations
            .len(),
    )
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
