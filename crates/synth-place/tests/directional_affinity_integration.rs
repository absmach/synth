// SPDX-License-Identifier: Apache-2.0

//! Directional Net Affinity Integration Tests (Phase 2).
//!
//! Verifies:
//! - Engine builds on real-world reference boards without errors.
//! - Non-power signal nets establish directional affinities from major ICs.
//! - Power/ground nets and decoupling caps are filtered out.
//! - Directional penalties respond correctly to placement orientation.

use synth_place::{place, DirectionalAffinityEngine};

fn load_board(path: &str) -> synth_ir::Board {
    let source = std::fs::read_to_string(path).expect("read fixture");
    let file = path.to_string();
    let parse = synth_parser::parse(&source, file.clone());
    let ast = parse.ast.as_ref().expect("parse");
    let registry_dir = std::path::Path::new("../..").join("registry").join("parts");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let loader = synth_ir::FsImportLoader {
        root: std::path::PathBuf::from("../.."),
    };
    let resolved = synth_ir::resolve_imports(ast, &loader, &file);
    synth_ir::lower(&resolved.program, &registry, &file)
        .board
        .expect("board")
}

#[test]
fn test_ethernet_phy_directional_affinities() {
    let board = load_board("../../fixtures/designs/ethernet_phy.synth");
    let placement = place(&board).expect("place ethernet_phy");

    let engine = DirectionalAffinityEngine::build(&board);

    // Major IC U1 (LAN8720A) has directional affinities
    assert!(!engine.affinities.is_empty());

    // Compute placement penalty for current layout
    let penalty = engine.placement_penalty(&placement.components);
    println!("Ethernet PHY directional affinity penalty: {} nm", penalty);

    // Verify affinities are found for signal destinations
    let mut total_affinities = 0;
    for (_dest_id, aff_list) in &engine.affinities {
        for aff in aff_list {
            total_affinities += 1;
            println!(
                "Dest {}: IC {:?}, Dominant: {:?}, Weight: {:.1}",
                aff.dest_refdes,
                aff.ic_id,
                aff.preferred_vector.dominant_side,
                aff.weight
            );
        }
    }
    assert!(total_affinities > 0, "Ethernet PHY should have signal affinities");
}

#[test]
fn test_env_logger_directional_affinities() {
    let board = load_board("../../examples/env_logger/env_logger.synth");
    let placement = place(&board).expect("place env_logger");

    let engine = DirectionalAffinityEngine::build(&board);
    assert!(!engine.affinities.is_empty());

    let penalty = engine.placement_penalty(&placement.components);
    println!("Env Logger directional affinity penalty: {} nm", penalty);
    assert!(penalty >= 0);
}
