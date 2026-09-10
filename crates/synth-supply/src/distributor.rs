// SPDX-License-Identifier: Apache-2.0

//! Abstract `Distributor` trait for live stock & pricing adapters.

use crate::types::{SupplyError, SupplyStatus};
use async_trait::async_trait;

/// Pluggable interface implemented by distributor API clients.
#[async_trait]
pub trait Distributor: std::fmt::Debug + Send + Sync {
    /// Human-readable distributor identifier (e.g. `"LCSC"`, `"Nexar"`).
    fn name(&self) -> &'static str;

    /// Query stock and pricing for a part number (MPN or LCSC C-number).
    async fn query(&self, part_number: &str) -> Result<Option<SupplyStatus>, SupplyError>;
}
