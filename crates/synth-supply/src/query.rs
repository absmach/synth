// SPDX-License-Identifier: Apache-2.0

//! High-level supply-chain query engine combining distributors and local cache.

use crate::cache::SupplyCache;
use crate::distributor::Distributor;
use crate::lcsc::LcscDistributor;
use crate::nexar::NexarDistributor;
use crate::types::{BomQueryEntry, SupplyError, SupplyStatus, normalize_part_number};
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
        let clean_pn = normalize_part_number(pn);
        if clean_pn.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        let mut first_error = None;

        for dist in &self.distributors {
            let dist_name = dist.name();
            // Check cache first
            if let Some(cached) = self.cache.get(dist_name, &clean_pn) {
                results.push(cached);
                continue;
            }

            // Cache miss: query distributor API
            match dist.query(&clean_pn).await {
                Ok(Some(status)) => {
                    self.cache.put(&status)?;
                    results.push(status);
                }
                Ok(None) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        if results.is_empty()
            && let Some(error) = first_error
        {
            return Err(error);
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
        let mut queried = HashMap::<String, Vec<SupplyStatus>>::new();

        for entry in entries {
            let mut statuses = Vec::new();

            // Try LCSC PN first if present, fallback to MPN
            let target_pn = entry
                .lcsc_pn
                .as_deref()
                .or(entry.mpn.as_deref())
                .unwrap_or("");

            let normalized = normalize_part_number(target_pn);
            if !normalized.is_empty() {
                if let Some(cached_result) = queried.get(&normalized) {
                    statuses.extend(cached_result.iter().cloned());
                } else {
                    let result = self.query_part(&normalized).await?;
                    queried.insert(normalized, result.clone());
                    statuses.extend(result);
                }
            }

            map.insert(entry.ref_des.clone(), statuses);
        }

        Ok(map)
    }
}
