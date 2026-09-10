// SPDX-License-Identifier: Apache-2.0

//! Data types for supply-chain stock, pricing, and lifecycle queries.

use serde::{Deserialize, Serialize};

/// Component manufacturing lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatus {
    #[default]
    Active,
    /// Not Recommended for New Designs
    Nrnd,
    /// End-of-Life / Discontinued
    Obsolete,
    Unknown,
}

impl LifecycleStatus {
    #[must_use]
    pub fn is_problematic(self) -> bool {
        matches!(self, Self::Nrnd | Self::Obsolete)
    }
}

/// Consolidated supply status for a part from a specific distributor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupplyStatus {
    /// Part number queried (LCSC PN or MPN).
    pub part_number: String,
    /// Distributor name (e.g. `"LCSC"`, `"Nexar"`, `"Mouser"`, `"DigiKey"`).
    pub distributor: String,
    /// True if available stock quantity > 0.
    pub in_stock: bool,
    /// Total available stock quantity.
    pub stock_qty: u64,
    /// Minimum order quantity.
    pub moq: u32,
    /// Unit price in USD at minimum order quantity (if available).
    #[serde(default)]
    pub unit_price_usd: Option<f64>,
    /// Component lifecycle status.
    pub lifecycle: LifecycleStatus,
    /// Timestamp when this record was fetched or cached (ISO-8601).
    pub fetched_at: String,
}

/// Request payload item when querying supply for a design's BOM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BomQueryEntry {
    /// Component reference designator (e.g. `"U1"`, `"C5"`).
    pub ref_des: String,
    /// Synth registry Part ID (e.g. `"rp2350"`).
    pub part_id: String,
    /// Manufacturer Part Number (e.g. `"RP2350A"`).
    #[serde(default)]
    pub mpn: Option<String>,
    /// LCSC part number (e.g. `"C14663"`).
    #[serde(default)]
    pub lcsc_pn: Option<String>,
}

/// Error type returned by supply-chain queries.
#[derive(Debug, thiserror::Error)]
pub enum SupplyError {
    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Database cache error: {0}")]
    Cache(#[from] rusqlite::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("No distributor configured or part number empty")]
    InvalidQuery,
}
