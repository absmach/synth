// SPDX-License-Identifier: Apache-2.0

//! Tests for Phase 8 Routing Outcome Data Logger.

use std::path::{Path, PathBuf};
use synth_route::logger::RoutingOutcomeRecord;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

#[test]
fn routing_outcome_logger_writes_and_deserializes() {
    let root = workspace_root();
    let example_path = root.join("examples").join("sensor_logger.synth");
    let source = std::fs::read_to_string(&example_path).expect("read example");
    let file = example_path.to_string_lossy().to_string();

    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ast");

    let registry_dir = root.join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("load registry");

    let loader = synth_ir::FsImportLoader { root: root.clone() };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    let board = lowered.board.expect("lowered board");

    let placement = synth_place::place(&board).expect("place");
    let routing = synth_route::route(&board, &placement);

    let temp_dir = std::env::temp_dir().join("synth_route_logger_test");
    let out_file = synth_route::log_routing_outcome(&board, &placement, &routing, &temp_dir)
        .expect("log routing outcome");

    assert!(out_file.exists());
    let contents = std::fs::read_to_string(&out_file).expect("read log file");
    let record: RoutingOutcomeRecord = serde_json::from_str(&contents).expect("deserialize record");

    assert_eq!(record.board_name, board.name);
    assert_eq!(record.component_count, placement.components.len());
    assert_eq!(record.routed_segments_count, routing.segments.len());
    assert_eq!(record.unrouted_nets_count, routing.unrouted_nets.len());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn logger_determinism() {
    let root = workspace_root();
    let example_path = root.join("examples").join("sensor_logger.synth");
    let source = std::fs::read_to_string(&example_path).expect("read example");
    let file = example_path.to_string_lossy().to_string();

    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse ast");

    let registry_dir = root.join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("load registry");

    let loader = synth_ir::FsImportLoader { root };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    let lowered = synth_ir::lower(&resolved.program, &registry, &file);
    let board = lowered.board.expect("board");

    let placement = synth_place::place(&board).expect("place");
    let r1 = synth_route::route(&board, &placement);
    let r2 = synth_route::route(&board, &placement);

    assert_eq!(r1, r2);
}
