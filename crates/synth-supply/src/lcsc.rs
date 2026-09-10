// SPDX-License-Identifier: Apache-2.0

//! LCSC distributor API adapter.

use crate::distributor::Distributor;
use crate::types::{LifecycleStatus, SupplyError, SupplyStatus};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;

pub const DEFAULT_LCSC_ENDPOINT: &str = "https://wwwapi.lcsc.com/v1/search/search-product-list";

/// LCSC API client.
#[derive(Debug, Clone)]
pub struct LcscDistributor {
    client: Client,
    endpoint: String,
}

impl LcscDistributor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("Synth-EDA/0.0.1 (supply-chain-query)")
                .build()
                .unwrap_or_default(),
            endpoint: DEFAULT_LCSC_ENDPOINT.to_string(),
        }
    }

    #[must_use]
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .user_agent("Synth-EDA/0.0.1 (supply-chain-query)")
                .build()
                .unwrap_or_default(),
            endpoint: endpoint.into(),
        }
    }
}

impl Default for LcscDistributor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct LcscResponse {
    code: Option<i32>,
    result: Option<LcscResult>,
}

#[derive(Debug, Deserialize)]
struct LcscResult {
    #[serde(rename = "productList")]
    product_list: Option<Vec<LcscProduct>>,
}

#[derive(Debug, Deserialize)]
struct LcscProduct {
    #[serde(rename = "productCode")]
    product_code: Option<String>,
    #[serde(rename = "stockNumber")]
    stock_number: Option<u64>,
    #[serde(rename = "minBuyNumber")]
    min_buy_number: Option<u32>,
    #[serde(rename = "productStatus")]
    product_status: Option<String>,
    #[serde(rename = "productPriceList")]
    product_price_list: Option<Vec<LcscPrice>>,
}

#[derive(Debug, Deserialize)]
struct LcscPrice {
    #[serde(rename = "productPrice")]
    product_price: Option<f64>,
}

#[async_trait]
impl Distributor for LcscDistributor {
    fn name(&self) -> &'static str {
        "LCSC"
    }

    async fn query(&self, part_number: &str) -> Result<Option<SupplyStatus>, SupplyError> {
        let pn = part_number.trim();
        if pn.is_empty() {
            return Ok(None);
        }

        let url = format!("{}?keyword={}", self.endpoint, urlencoding::encode(pn));
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let data: LcscResponse = resp.json().await?;
        if data.code != Some(200) {
            return Ok(None);
        }

        let products = match data.result.and_then(|r| r.product_list) {
            Some(list) if !list.is_empty() => list,
            _ => return Ok(None),
        };

        // Find exact or first matching product
        let product = products
            .iter()
            .find(|p| {
                p.product_code
                    .as_deref()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(pn)
            })
            .unwrap_or(&products[0]);

        let stock_qty = product.stock_number.unwrap_or(0);
        let moq = product.min_buy_number.unwrap_or(1);
        let unit_price_usd = product
            .product_price_list
            .as_ref()
            .and_then(|list| list.first())
            .and_then(|p| p.product_price);

        let status_str = product.product_status.as_deref().unwrap_or("Active");
        let lifecycle = match status_str.to_lowercase().as_str() {
            "discontinued" | "obsolete" | "eol" => LifecycleStatus::Obsolete,
            "not recommended" | "nrnd" => LifecycleStatus::Nrnd,
            _ => LifecycleStatus::Active,
        };

        Ok(Some(SupplyStatus {
            part_number: product
                .product_code
                .clone()
                .unwrap_or_else(|| pn.to_string()),
            distributor: "LCSC".to_string(),
            in_stock: stock_qty > 0,
            stock_qty,
            moq,
            unit_price_usd,
            lifecycle,
            fetched_at: Utc::now().to_rfc3339(),
        }))
    }
}
