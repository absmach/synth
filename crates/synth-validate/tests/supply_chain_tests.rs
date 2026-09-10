// SPDX-License-Identifier: Apache-2.0

use chrono::Utc;
use synth_supply::{LifecycleStatus, SupplyCache, SupplyStatus};

#[test]
fn test_w_synth_supply_001_fires_on_obsolete_component_in_cache() {
    let cache = SupplyCache::default_location().expect("default cache");
    let status = SupplyStatus {
        part_number: "C14663_TEST_OBSOLETE".to_string(),
        distributor: "LCSC".to_string(),
        in_stock: false,
        stock_qty: 0,
        moq: 1,
        unit_price_usd: None,
        lifecycle: LifecycleStatus::Obsolete,
        fetched_at: Utc::now().to_rfc3339(),
    };
    cache.put(&status).expect("put");

    // Load registry and lower a board using workspace relative path
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_dir = root.join("registry/parts");
    let fixture_path = root.join("fixtures/erc/pass__power_005_level_shifter.synth");
    let file = fixture_path.to_str().expect("valid path str");
    let registry = synth_registry::load_dir(&registry_dir).expect("registry");
    let source = std::fs::read_to_string(file).expect("fixture");
    let parse = synth_parser::parse(&source, file.to_string());
    let ast = parse.ast.expect("ast");
    let loader = synth_ir::FsImportLoader { root };
    let resolved = synth_ir::resolve_imports(&ast, &loader, file);
    let lowered = synth_ir::lower(&resolved.program, &registry, file);
    let mut board = lowered.board.expect("board");

    // Temporarily point one component's lcsc_pn to the test obsolete part
    if let Some(comp) = board.components.first_mut() {
        if let Some(part) = comp.part.as_mut() {
            part.lcsc_pn = Some("C14663_TEST_OBSOLETE".to_string());
        }
    }

    let diags = synth_validate::run_erc(&board, file);
    let supply_warns: Vec<_> = diags
        .iter()
        .filter(|d| d.code == "W-SYNTH-SUPPLY-001")
        .collect();

    assert_eq!(supply_warns.len(), 1);
    assert!(supply_warns[0].title.contains("marked Obsolete"));
}
