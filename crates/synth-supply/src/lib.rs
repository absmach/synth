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

pub use cache::{load_env_file, SupplyCache};
pub use distributor::Distributor;
pub use lcsc::LcscDistributor;
pub use nexar::NexarDistributor;
pub use query::SupplyEngine;
pub use types::{BomQueryEntry, LifecycleStatus, SupplyError, SupplyStatus};
