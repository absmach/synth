// SPDX-License-Identifier: Apache-2.0

//! Integration test verifying preview compilation & serialization
//! performance bounds.
//!
//! The plan's §7.4 gate ("<250ms p50, <500ms p95 save-to-render") was
//! set before `synth_layout::layout` did any KiCad symbol-file I/O
//! or A* wire routing (both added by the routing work that populates
//! `Layout.wires`/`Layout.junctions`) — at that point the whole
//! pipeline really did run in ~2ms. That real geometric work is now
//! in the hot path, and its cost is dramatically different between
//! build profiles: `cargo build --release` on `examples/sensor_logger/sensor_logger.synth`
//! (31 components) comfortably meets the original 250ms budget
//! (~150ms wall clock including process startup, measured 2026-08-14),
//! but the `test` profile `cargo test` uses by default (opt-level 1,
//! debug assertions on) takes ~700ms for the same design — dominated by
//! A* pathfinding
//! over a grid sized to the design's full bounding box (tens of
//! thousands of cells), which is inherently much more expensive
//! without compiler optimizations.
//!
//! This test therefore checks a debug-build-realistic ceiling (not
//! the original release-mode product target) so it still catches
//! *new* regressions without being permanently red. If you're
//! looking at improving this: the A* neighbour expansion scans every
//! registered wire segment per cell visited
//! (`route::grid::SchematicGrid::find_path`) — indexing that scan by
//! row/column was tried and measured *slower* in debug builds
//! (HashMap lookup overhead outweighing the smaller scan at this
//! design's segment counts); a real fix needs profiling this
//! properly (flamegraph or similar), not another guess.

/// The pipeline is timed this many times and the *fastest* sample is
/// compared against the budget. A single wall-clock sample on a shared
/// CI runner picks up scheduler noise — the same commit has measured
/// ~700 ms locally and ~1250 ms on a busy CI runner, which is what made
/// this gate flaky. A genuine regression raises every sample, so the
/// minimum still catches it.
const TIMED_RUNS: usize = 3;

/// Debug-build regression ceiling. The 31-component sensor_logger runs
/// at ~700 ms (fastest of three) at `opt-level = 1`, the profile tests
/// use; a single cold run on a busy CI runner has reached ~1250 ms.
/// 2000 ms leaves room for that variance while still failing on a real
/// regression. The design has grown from 27 to 31 components since the
/// budget was first set, which is why the ceiling moved with it.
const DEBUG_BUILD_BUDGET_MS: u128 = 2000;

use std::path::{Path, PathBuf};
use std::time::Instant;

/// Run the save-to-render pipeline once, returning the elapsed
/// milliseconds, the component count, and the serialized payload.
fn run_pipeline(
    registry: &synth_registry::Registry,
    source: &str,
    filename: &str,
    import_root: &Path,
) -> (u128, usize, String) {
    let start = Instant::now();

    // 1. Parse
    let parse = synth_parser::parse(source, filename.to_string());

    // 2. Resolve
    let loader = synth_ir::FsImportLoader {
        root: import_root.to_path_buf(),
    };
    let resolved = synth_ir::resolve_imports(parse.ast.as_ref().unwrap(), &loader, filename);

    // 3. Lower
    let lowered = synth_ir::lower(&resolved.program, registry, filename);
    let board = lowered.board.expect("board lowering should succeed");

    // 4. ERC Validation
    let _erc_diags = synth_validate::run_erc(&board, filename);

    // 5. Auto-Layout
    let layout = synth_layout::layout(&board);

    // 6. JSON Serialization
    let json_payload = serde_json::to_string(&(&board, &layout)).expect("JSON serialize");

    (
        start.elapsed().as_millis(),
        board.components.len(),
        json_payload,
    )
}

#[test]
fn preview_compile_performance_benchmark() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let registry_dir = workspace_root.join("registry").join("parts");

    let registry = synth_registry::load_dir(&registry_dir)
        .expect("registry should load cleanly for performance test");

    let sample_design_path = workspace_root
        .join("examples")
        .join("sensor_logger")
        .join("sensor_logger.synth");
    assert!(
        sample_design_path.exists(),
        "sensor_logger.synth must exist at {}",
        sample_design_path.display()
    );

    let source = std::fs::read_to_string(&sample_design_path).expect("read sample design");
    let filename = sample_design_path.to_string_lossy().to_string();
    let import_root = sample_design_path.parent().unwrap();

    let mut fastest_ms = u128::MAX;
    let mut components = 0;
    let mut json_payload = String::new();

    for _ in 0..TIMED_RUNS {
        let (elapsed_ms, count, payload) = run_pipeline(&registry, &source, &filename, import_root);
        fastest_ms = fastest_ms.min(elapsed_ms);
        components = count;
        json_payload = payload;
    }

    assert!(!json_payload.is_empty(), "JSON payload must not be empty");
    assert!(
        fastest_ms < DEBUG_BUILD_BUDGET_MS,
        "Save-to-render pipeline execution took {fastest_ms} ms (fastest of {TIMED_RUNS}), \
         exceeding the {DEBUG_BUILD_BUDGET_MS} ms debug-build regression budget (see this file's \
         module doc comment for why this isn't the original 250ms release-mode target)!"
    );

    println!(
        "Preview performance benchmark passed: {fastest_ms} ms (fastest of {TIMED_RUNS}) for \
         {components} components"
    );
}
