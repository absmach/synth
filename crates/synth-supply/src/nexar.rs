// SPDX-License-Identifier: Apache-2.0

//! Nexar GraphQL distributor API adapter (covers Mouser, DigiKey, Arrow, Farnell).

use crate::cache::load_env_file;
use crate::distributor::Distributor;
use crate::types::{LifecycleStatus, SupplyError, SupplyStatus};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

pub const DEFAULT_NEXAR_ENDPOINT: &str = "https://api.nexar.com/graphql";

/// Nexar API client. Reads access token from `NEXAR_TOKEN` env var or `.env` file.
#[derive(Debug, Clone)]
pub struct NexarDistributor {
    client: Client,
    endpoint: String,
    token: Option<String>,
}

impl NexarDistributor {
    #[must_use]
    pub fn new() -> Self {
        load_env_file();
        let token = std::env::var("NEXAR_TOKEN").ok();
        Self {
            client: Client::builder()
                .user_agent("Synth-EDA/0.0.1 (supply-chain-query)")
                .build()
                .unwrap_or_default(),
            endpoint: DEFAULT_NEXAR_ENDPOINT.to_string(),
            token,
        }
    }

    #[must_use]
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .user_agent("Synth-EDA/0.0.1 (supply-chain-query)")
                .build()
                .unwrap_or_default(),
            endpoint: DEFAULT_NEXAR_ENDPOINT.to_string(),
            token: Some(token.into()),
        }
    }

    #[must_use]
    pub fn with_endpoint_and_token(endpoint: impl Into<String>, token: Option<String>) -> Self {
        Self {
            client: Client::builder()
                .user_agent("Synth-EDA/0.0.1 (supply-chain-query)")
                .build()
                .unwrap_or_default(),
            endpoint: endpoint.into(),
            token,
        }
    }
}

impl Default for NexarDistributor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct NexarGqlResponse {
    data: Option<NexarGqlData>,
}

#[derive(Debug, Deserialize)]
struct NexarGqlData {
    #[serde(rename = "supSearch")]
    sup_search: Option<NexarSupSearch>,
}

#[derive(Debug, Deserialize)]
struct NexarSupSearch {
    results: Option<Vec<NexarResult>>,
}

#[derive(Debug, Deserialize)]
struct NexarResult {
    item: Option<NexarItem>,
}

#[derive(Debug, Deserialize)]
struct NexarItem {
    mpn: Option<String>,
    offers: Option<Vec<NexarOffer>>,
}

#[derive(Debug, Deserialize)]
struct NexarOffer {
    seller: Option<NexarSeller>,
    #[serde(rename = "inventoryLevel")]
    inventory_level: Option<u64>,
    moq: Option<u32>,
    prices: Option<Vec<NexarPrice>>,
}

#[derive(Debug, Deserialize)]
struct NexarSeller {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NexarPrice {
    price: Option<f64>,
}

#[async_trait]
impl Distributor for NexarDistributor {
    fn name(&self) -> &'static str {
        "Nexar"
    }

    async fn query(&self, part_number: &str) -> Result<Option<SupplyStatus>, SupplyError> {
        let token = match &self.token {
            Some(t) if !t.trim().is_empty() => t.trim(),
            _ => return Ok(None), // Token not set, skip Nexar query gracefully
        };

        let pn = part_number.trim();
        if pn.is_empty() {
            return Ok(None);
        }

        let query_str = r"
            query SupSearch($q: String!) {
              supSearch(q: $q, limit: 1) {
                results {
                  item {
                    mpn
                    offers {
                      seller { name }
                      inventoryLevel
                      moq
                      prices { price }
                    }
                  }
                }
              }
            }
        ";

        let body = json!({
            "query": query_str,
            "variables": { "q": pn }
        });

        let resp = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let gql: NexarGqlResponse = resp.json().await?;
        let results = match gql.data.and_then(|d| d.sup_search).and_then(|s| s.results) {
            Some(r) if !r.is_empty() => r,
            _ => return Ok(None),
        };

        let Some(item) = results.first().and_then(|r| r.item.as_ref()) else {
            return Ok(None);
        };

        let offers = match &item.offers {
            Some(o) if !o.is_empty() => o,
            _ => return Ok(None),
        };

        // Pick offer with highest stock
        let best_offer = offers
            .iter()
            .max_by_key(|o| o.inventory_level.unwrap_or(0))
            .unwrap_or(&offers[0]);

        let stock_qty = best_offer.inventory_level.unwrap_or(0);
        let moq = best_offer.moq.unwrap_or(1);
        let unit_price_usd = best_offer
            .prices
            .as_ref()
            .and_then(|p| p.first())
            .and_then(|p| p.price);

        let seller_name = best_offer
            .seller
            .as_ref()
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| "Nexar".to_string());

        Ok(Some(SupplyStatus {
            part_number: item.mpn.clone().unwrap_or_else(|| pn.to_string()),
            distributor: format!("Nexar ({seller_name})"),
            in_stock: stock_qty > 0,
            stock_qty,
            moq,
            unit_price_usd,
            lifecycle: LifecycleStatus::Active,
            fetched_at: Utc::now().to_rfc3339(),
        }))
    }
}
