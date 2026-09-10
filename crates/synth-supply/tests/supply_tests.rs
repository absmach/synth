// SPDX-License-Identifier: Apache-2.0

use chrono::Utc;
use synth_supply::{
    distributor::Distributor,
    load_env_file,
    types::{BomQueryEntry, LifecycleStatus, SupplyStatus},
    LcscDistributor, NexarDistributor, SupplyCache, SupplyEngine,
};

#[test]
fn test_cache_put_get_roundtrip() {
    let cache = SupplyCache::memory().expect("in-memory sqlite");
    let status = SupplyStatus {
        part_number: "C14663".to_string(),
        distributor: "LCSC".to_string(),
        in_stock: true,
        stock_qty: 4200,
        moq: 5,
        unit_price_usd: Some(0.98),
        lifecycle: LifecycleStatus::Active,
        fetched_at: Utc::now().to_rfc3339(),
    };

    cache.put(&status).expect("put");

    let cached = cache.get("LCSC", "C14663").expect("get cached");
    assert_eq!(cached.part_number, "C14663");
    assert_eq!(cached.distributor, "LCSC");
    assert!(cached.in_stock);
    assert_eq!(cached.stock_qty, 4200);
    assert_eq!(cached.moq, 5);
    assert_eq!(cached.unit_price_usd, Some(0.98));
    assert_eq!(cached.lifecycle, LifecycleStatus::Active);
}

#[test]
fn test_cache_miss_returns_none() {
    let cache = SupplyCache::memory().expect("in-memory sqlite");
    assert!(cache.get("LCSC", "NONEXISTENT_PN").is_none());
}

#[tokio::test]
async fn test_supply_engine_with_memory_cache() {
    let cache = SupplyCache::memory().expect("cache");
    let engine = SupplyEngine::with_cache_and_distributors(
        cache.clone(),
        vec![
            std::sync::Arc::new(LcscDistributor::new()),
            std::sync::Arc::new(NexarDistributor::new()),
        ],
    );

    // Pre-populate cache for testing deterministic query
    let status = SupplyStatus {
        part_number: "C14663".to_string(),
        distributor: "LCSC".to_string(),
        in_stock: true,
        stock_qty: 1000,
        moq: 1,
        unit_price_usd: Some(0.85),
        lifecycle: LifecycleStatus::Active,
        fetched_at: Utc::now().to_rfc3339(),
    };
    cache.put(&status).unwrap();

    let res = engine.query_part("C14663").await.expect("query_part");
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].stock_qty, 1000);

    let bom_entry = BomQueryEntry {
        ref_des: "U1".to_string(),
        part_id: "rp2350".to_string(),
        mpn: Some("RP2350A".to_string()),
        lcsc_pn: Some("C14663".to_string()),
    };

    let bom_res = engine.query_bom(&[bom_entry]).await.expect("query_bom");
    assert!(bom_res.contains_key("U1"));
    assert_eq!(bom_res["U1"][0].stock_qty, 1000);
}

#[tokio::test]
async fn test_nexar_distributor_loads_env_token() {
    load_env_file();
    let nexar = NexarDistributor::new();
    assert_eq!(nexar.name(), "Nexar");
    // Token should be loaded from .env if present
    if std::env::var("NEXAR_TOKEN").is_ok() {
        // Run live token query test if network/token is available
        let res = nexar.query("RP2350A").await;
        assert!(res.is_ok());
    }
}
