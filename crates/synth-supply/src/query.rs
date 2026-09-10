// SPDX-License-Identifier: Apache-2.0

//! High-level supply-chain query engine combining distributors and local cache.

use crate::cache::SupplyCache;
use crate::distributor::Distributor;
use crate::lcsc::LcscDistributor;
use crate::nexar::NexarDistributor;
use crate::types::{BomQueryEntry, SupplyError, SupplyStatus};
use std::collections::HashMap;
use std::sync::Arc;

/// Supply-chain query engine.
#[derive(Clone, Debug)]
pub struct SupplyEngine {
    cache: SupplyCache,
    distributors: Vec<Arc<dyn Distributor>>,
}

impl SupplyEngine {
    /// Create engine with default distributors (LCSC + Nexar) and standard SQLite cache location.
    pub fn new() -> Result<Self, SupplyError> {
        let cache = SupplyCache::default_location()?;
        let distributors: Vec<Arc<dyn Distributor>> = vec![
            Arc::new(LcscDistributor::new()),
            Arc::new(NexarDistributor::new()),
        ];
        Ok(Self {
            cache,
            distributors,
        })
    }

    /// Create engine with in-memory SQLite cache and explicit distributors (ideal for testing).
    pub fn with_cache_and_distributors(
        cache: SupplyCache,
        distributors: Vec<Arc<dyn Distributor>>,
    ) -> Self {
        Self {
            cache,
            distributors,
        }
    }

    /// Access reference to underlying cache.
    #[must_use]
    pub fn cache(&self) -> &SupplyCache {
        &self.cache
    }

    /// Query supply status for a single part number (LCSC PN or MPN).
    /// Returns one status per distributor that found a match.
    pub async fn query_part(&self, pn: &str) -> Result<Vec<SupplyStatus>, SupplyError> {
        let clean_pn = pn.trim();
        if clean_pn.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for dist in &self.distributors {
            let dist_name = dist.name();
            // Check cache first
            if let Some(cached) = self.cache.get(dist_name, clean_pn) {
                results.push(cached);
                continue;
            }

            // Cache miss: query distributor API
            if let Ok(Some(status)) = dist.query(clean_pn).await {
                let _ = self.cache.put(&status);
                results.push(status);
            }
        }

        Ok(results)
    }

    /// Query supply status for an entire BOM in parallel.
    /// Returns a map from `ref_des` to a list of supply statuses.
    pub async fn query_bom(
        &self,
        entries: &[BomQueryEntry],
    ) -> Result<HashMap<String, Vec<SupplyStatus>>, SupplyError> {
        let mut map = HashMap::new();

        for entry in entries {
            let mut statuses = Vec::new();

            // Try LCSC PN first if present, fallback to MPN
            let target_pn = entry
                .lcsc_pn
                .as_deref()
                .or(entry.mpn.as_deref())
                .unwrap_or("");

            if !target_pn.is_empty() {
                if let Ok(res) = self.query_part(target_pn).await {
                    statuses.extend(res);
                }
            }

            map.insert(entry.ref_des.clone(), statuses);
        }

        Ok(map)
    }
}
