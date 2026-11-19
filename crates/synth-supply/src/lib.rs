// SPDX-License-Identifier: Apache-2.0

//! Live supply-chain query engine for Synth EDA.
//!
//! Provides stock, pricing, and lifecycle queries across LCSC and Nexar APIs,
//! with SQLite local caching and environment configuration.

#![forbid(unsafe_code)]

pub mod cache;
pub mod distributor;
pub mod lcsc;
pub mod nexar;
pub mod query;
pub mod types;

pub use cache::{SupplyCache, env_or_dotenv};
pub use distributor::Distributor;
pub use lcsc::LcscDistributor;
pub use nexar::NexarDistributor;
pub use query::SupplyEngine;
pub use types::{
    AssemblyEligibility, BomQueryEntry, LifecycleStatus, PriceTier, PurchaseConstraints,
    StockStatus, SupplyError, SupplyStatus, normalize_part_number,
};
