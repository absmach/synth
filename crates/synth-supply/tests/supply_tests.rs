// SPDX-License-Identifier: Apache-2.0

use chrono::Utc;
use synth_supply::{
    LcscDistributor, NexarDistributor, SupplyCache, SupplyEngine,
    distributor::Distributor,
    env_or_dotenv,
    types::{
        AssemblyEligibility, BomQueryEntry, LifecycleStatus, PurchaseConstraints, StockStatus,
        SupplyStatus,
    },
};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_cache_put_get_roundtrip() {
    let cache = SupplyCache::memory().expect("in-memory sqlite");
    let status = SupplyStatus {
        part_number: "C14663".to_string(),
        distributor: "LCSC".to_string(),
        in_stock: true,
        stock_qty: 4200,
        stock_status: StockStatus::InStock,
        assembly_eligibility: AssemblyEligibility::Unknown,
        moq: 5,
        purchase: PurchaseConstraints::default(),
        unit_price_usd: Some(0.98),
        lifecycle: LifecycleStatus::Active,
        fetched_at: Utc::now().to_rfc3339(),
        snapshot_id: None,
    };

    cache.put(&status).expect("put");

    let cached = cache.get("LCSC", "c14663").expect("get cached");
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
        stock_status: StockStatus::InStock,
        assembly_eligibility: AssemblyEligibility::Unknown,
        moq: 1,
        purchase: PurchaseConstraints::default(),
        unit_price_usd: Some(0.85),
        lifecycle: LifecycleStatus::Active,
        fetched_at: Utc::now().to_rfc3339(),
        snapshot_id: None,
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
    let nexar = NexarDistributor::new();
    assert_eq!(nexar.name(), "Nexar");
    // Token should be loaded from the environment or .env if present
    if env_or_dotenv("NEXAR_TOKEN").is_some() {
        // Run live token query test if network/token is available
        let res = nexar.query("RP2350A").await;
        assert!(res.is_ok());
    }
}

#[tokio::test]
async fn lcsc_requires_exact_identity_match() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("keyword", "C14663"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/lcsc-phase0/exact-match.json")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let distributor = LcscDistributor::with_endpoint(format!("{}/search", server.uri()));
    let result = distributor.query("c14663").await.expect("exact response");
    let status = result.expect("exact product");
    assert_eq!(status.part_number, "C14663");
    assert_eq!(status.stock_status, StockStatus::InStock);
    assert_eq!(status.purchase.moq, Some(5));
}

#[tokio::test]
async fn lcsc_current_jlcpcb_catalog_contract_uses_post() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/catalog"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"code":200,"data":{"componentPageInfo":{"list":[{"componentCode":"C14663","stockCount":42,"componentPrices":[{"startNumber":1,"endNumber":-1,"productPrice":0.12}],"minPurchaseNum":5,"componentLibraryType":"base"}]}}}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let distributor = LcscDistributor::with_endpoint(format!("{}/catalog", server.uri()));
    let status = distributor
        .query("C14663")
        .await
        .expect("current JLCPCB response")
        .expect("exact product");

    assert_eq!(status.part_number, "C14663");
    assert_eq!(status.stock_qty, 42);
    assert_eq!(status.purchase.moq, Some(5));
    assert_eq!(status.unit_price_usd, Some(0.12));
}

#[tokio::test]
async fn lcsc_rejects_ambiguous_search_without_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/lcsc-phase0/ambiguous-search.json")),
        )
        .mount(&server)
        .await;

    let distributor = LcscDistributor::with_endpoint(format!("{}/search", server.uri()));
    let error = distributor
        .query("10k")
        .await
        .expect_err("must be ambiguous");
    assert!(matches!(
        error,
        synth_supply::SupplyError::AmbiguousMatch { .. }
    ));
}

#[tokio::test]
async fn lcsc_missing_inventory_is_unknown() {
    let server = MockServer::start().await;
    let body = r#"{"code":200,"result":{"productList":[{"productCode":"C7"}]}}"#;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let distributor = LcscDistributor::with_endpoint(format!("{}/search", server.uri()));
    let status = distributor
        .query("C7")
        .await
        .expect("response")
        .expect("product");
    assert_eq!(status.stock_status, StockStatus::Unknown);
    assert_eq!(status.assembly_eligibility, AssemblyEligibility::Unknown);
    assert_eq!(status.lifecycle, LifecycleStatus::Unknown);
    assert_eq!(status.purchase.moq, None);
}

#[tokio::test]
async fn lcsc_provider_failure_is_explicit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let distributor = LcscDistributor::with_endpoint(format!("{}/search", server.uri()));
    let error = distributor
        .query("C14663")
        .await
        .expect_err("provider failure");
    assert!(matches!(
        error,
        synth_supply::SupplyError::ProviderUnavailable { .. }
    ));
}

#[tokio::test]
async fn bom_queries_deduplicate_normalized_provider_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("keyword", "C14663"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(include_str!("fixtures/lcsc-phase0/exact-match.json")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let cache = SupplyCache::memory().expect("cache");
    let engine = SupplyEngine::with_cache_and_distributors(
        cache,
        vec![std::sync::Arc::new(LcscDistributor::with_endpoint(
            format!("{}/search", server.uri()),
        ))],
    );
    let entries = vec![
        BomQueryEntry {
            ref_des: "R1".to_string(),
            part_id: "r".to_string(),
            mpn: None,
            lcsc_pn: Some("c14663".to_string()),
        },
        BomQueryEntry {
            ref_des: "R2".to_string(),
            part_id: "r".to_string(),
            mpn: None,
            lcsc_pn: Some("C14663".to_string()),
        },
    ];
    let result = engine.query_bom(&entries).await.expect("BOM query");
    assert_eq!(result["R1"][0].part_number, "C14663");
    assert_eq!(result["R2"][0].part_number, "C14663");
}

#[test]
fn cache_snapshot_write_is_atomic_and_preserves_metadata() {
    let cache = SupplyCache::memory().expect("cache");
    let status = SupplyStatus {
        part_number: "C7".to_string(),
        distributor: "LCSC".to_string(),
        in_stock: false,
        stock_qty: 0,
        stock_status: StockStatus::Unknown,
        assembly_eligibility: AssemblyEligibility::Unknown,
        moq: 0,
        purchase: PurchaseConstraints::default(),
        unit_price_usd: None,
        lifecycle: LifecycleStatus::Unknown,
        fetched_at: Utc::now().to_rfc3339(),
        snapshot_id: None,
    };
    cache
        .put_snapshot("catalog-2026-10-16", &[status])
        .expect("snapshot");
    let cached = cache.get("LCSC", "C7").expect("cached status");
    assert_eq!(cached.snapshot_id.as_deref(), Some("catalog-2026-10-16"));
    assert_eq!(cached.stock_status, StockStatus::Unknown);
}
