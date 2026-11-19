// SPDX-License-Identifier: Apache-2.0

//! SQLite cache storage for supply chain query results.

use crate::types::{
    AssemblyEligibility, LifecycleStatus, PurchaseConstraints, StockStatus, SupplyError,
    SupplyStatus, normalize_part_number,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Default TTL for active stock records: 24 hours.
pub const CACHE_TTL_ACTIVE_HOURS: i64 = 24;

/// Default TTL for obsolete / EOL records: 72 hours.
pub const CACHE_TTL_OBSOLETE_HOURS: i64 = 72;

/// SQLite supply cache. Thread-safe wrapper over `rusqlite::Connection`.
#[derive(Clone, Debug)]
pub struct SupplyCache {
    conn: Arc<Mutex<Connection>>,
}

impl SupplyCache {
    /// Create a cache using an in-memory SQLite database (ideal for tests).
    pub fn memory() -> Result<Self, SupplyError> {
        let conn = Connection::open_in_memory()?;
        Self::init_db(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create or open a cache at the default location (`~/.synth/supply_cache.db`).
    pub fn default_location() -> Result<Self, SupplyError> {
        let mut path = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push(".synth");
        std::fs::create_dir_all(&path).ok();
        path.push("supply_cache.db");
        Self::open(&path)
    }

    /// Open a cache at a specific file path.
    pub fn open(path: &Path) -> Result<Self, SupplyError> {
        let conn = Connection::open(path)?;
        Self::init_db(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn init_db(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS supply_cache (
                distributor TEXT NOT NULL,
                part_number TEXT NOT NULL,
                in_stock INTEGER NOT NULL,
                stock_qty INTEGER NOT NULL,
                moq INTEGER NOT NULL,
                unit_price_usd REAL,
                lifecycle TEXT NOT NULL,
                fetched_at TEXT NOT NULL,
                PRIMARY KEY (distributor, part_number)
            )",
            [],
        )?;
        // Additive migrations keep existing user caches readable while making
        // explicit unknown/purchasing/snapshot state durable. Do not ignore a
        // migration error: a cache that cannot store the new contract must be
        // rejected before a later write fails with a misleading SQL error.
        for (column, definition) in [
            ("stock_status", "TEXT"),
            ("assembly_eligibility", "TEXT"),
            ("purchase_json", "TEXT"),
            ("snapshot_id", "TEXT"),
        ] {
            let present = conn
                .prepare("SELECT 1 FROM pragma_table_info('supply_cache') WHERE name = ?1")?
                .exists([column])?;
            if !present {
                conn.execute(
                    &format!("ALTER TABLE supply_cache ADD COLUMN {column} {definition}"),
                    [],
                )?;
            }
        }
        Ok(())
    }

    /// Query non-expired cached status for `(distributor, part_number)`.
    #[allow(clippy::cast_sign_loss)]
    pub fn get(&self, distributor: &str, part_number: &str) -> Option<SupplyStatus> {
        let part_number = normalize_part_number(part_number);
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT part_number, distributor, in_stock, stock_qty, moq, unit_price_usd, lifecycle, fetched_at,
                        stock_status, assembly_eligibility, purchase_json, snapshot_id
                 FROM supply_cache WHERE distributor = ?1 AND part_number = ?2",
            )
            .ok()?;

        let mut rows = stmt.query(params![distributor, part_number]).ok()?;
        if let Some(row) = rows.next().ok()? {
            let fetched_at_str: String = row.get(7).ok()?;
            let fetched_at = DateTime::parse_from_rfc3339(&fetched_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()?;

            let lifecycle_str: String = row.get(6).ok()?;
            let lifecycle = match lifecycle_str.as_str() {
                "active" => LifecycleStatus::Active,
                "nrnd" => LifecycleStatus::Nrnd,
                "obsolete" => LifecycleStatus::Obsolete,
                _ => LifecycleStatus::Unknown,
            };

            let ttl_hours = if lifecycle.is_problematic() {
                CACHE_TTL_OBSOLETE_HOURS
            } else {
                CACHE_TTL_ACTIVE_HOURS
            };

            if Utc::now() - fetched_at > Duration::hours(ttl_hours) {
                return None; // Expired
            }

            let in_stock_int: i64 = row.get(2).ok()?;
            let stock_qty_int: i64 = row.get(3).ok()?;
            let moq_int: i64 = row.get(4).ok()?;
            let stock_status = match row.get::<_, Option<String>>(8).ok()?.as_deref() {
                Some("in_stock") => StockStatus::InStock,
                Some("out_of_stock") => StockStatus::OutOfStock,
                _ if in_stock_int != 0 => StockStatus::InStock,
                _ => StockStatus::Unknown,
            };
            let assembly_eligibility = match row.get::<_, Option<String>>(9).ok()?.as_deref() {
                Some("eligible") => AssemblyEligibility::Eligible,
                Some("ineligible") => AssemblyEligibility::Ineligible,
                _ => AssemblyEligibility::Unknown,
            };
            let purchase = row
                .get::<_, Option<String>>(10)
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(|| PurchaseConstraints {
                    moq: (moq_int > 0).then_some(moq_int as u32),
                    ..PurchaseConstraints::default()
                });

            Some(SupplyStatus {
                part_number: row.get(0).ok()?,
                distributor: row.get(1).ok()?,
                in_stock: in_stock_int != 0,
                stock_qty: stock_qty_int.max(0) as u64,
                stock_status,
                assembly_eligibility,
                moq: moq_int.max(0) as u32,
                purchase,
                unit_price_usd: row.get(5).ok()?,
                lifecycle,
                fetched_at: fetched_at_str,
                snapshot_id: row.get(11).ok()?,
            })
        } else {
            None
        }
    }

    /// Insert or update a supply status entry in the cache.
    #[allow(clippy::cast_possible_wrap)]
    pub fn put(&self, status: &SupplyStatus) -> Result<(), SupplyError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SupplyError::Cache(rusqlite::Error::ExecuteReturnedResults))?;

        let lifecycle_str = match status.lifecycle {
            LifecycleStatus::Active => "active",
            LifecycleStatus::Nrnd => "nrnd",
            LifecycleStatus::Obsolete => "obsolete",
            LifecycleStatus::Unknown => "unknown",
        };

        conn.execute(
            "INSERT OR REPLACE INTO supply_cache
            (distributor, part_number, in_stock, stock_qty, moq, unit_price_usd, lifecycle, fetched_at,
             stock_status, assembly_eligibility, purchase_json, snapshot_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                status.distributor,
                normalize_part_number(&status.part_number),
                i32::from(status.in_stock),
                status.stock_qty as i64,
                i64::from(status.moq),
                status.unit_price_usd,
                lifecycle_str,
                status.fetched_at,
                match status.stock_status {
                    StockStatus::InStock => "in_stock",
                    StockStatus::OutOfStock => "out_of_stock",
                    StockStatus::Unknown => "unknown",
                },
                match status.assembly_eligibility {
                    AssemblyEligibility::Eligible => "eligible",
                    AssemblyEligibility::Ineligible => "ineligible",
                    AssemblyEligibility::Unknown => "unknown",
                },
                serde_json::to_string(&status.purchase)?,
                status.snapshot_id,
            ],
        )?;
        Ok(())
    }

    /// Atomically persist a catalog snapshot. Every row receives the same
    /// snapshot identity, so readers never observe a partially imported set.
    #[allow(clippy::cast_possible_wrap)]
    pub fn put_snapshot(
        &self,
        snapshot_id: &str,
        statuses: &[SupplyStatus],
    ) -> Result<(), SupplyError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| SupplyError::Cache(rusqlite::Error::ExecuteReturnedResults))?;
        let tx = conn.unchecked_transaction()?;
        for status in statuses {
            let mut snapshot_status = status.clone();
            snapshot_status.snapshot_id = Some(snapshot_id.to_string());
            let lifecycle_str = match snapshot_status.lifecycle {
                LifecycleStatus::Active => "active",
                LifecycleStatus::Nrnd => "nrnd",
                LifecycleStatus::Obsolete => "obsolete",
                LifecycleStatus::Unknown => "unknown",
            };
            tx.execute(
                "INSERT OR REPLACE INTO supply_cache
                 (distributor, part_number, in_stock, stock_qty, moq, unit_price_usd, lifecycle, fetched_at,
                  stock_status, assembly_eligibility, purchase_json, snapshot_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    snapshot_status.distributor,
                    normalize_part_number(&snapshot_status.part_number),
                    i32::from(snapshot_status.in_stock),
                    snapshot_status.stock_qty as i64,
                    i64::from(snapshot_status.moq),
                    snapshot_status.unit_price_usd,
                    lifecycle_str,
                    snapshot_status.fetched_at,
                    match snapshot_status.stock_status {
                        StockStatus::InStock => "in_stock",
                        StockStatus::OutOfStock => "out_of_stock",
                        StockStatus::Unknown => "unknown",
                    },
                    match snapshot_status.assembly_eligibility {
                        AssemblyEligibility::Eligible => "eligible",
                        AssemblyEligibility::Ineligible => "ineligible",
                        AssemblyEligibility::Unknown => "unknown",
                    },
                    serde_json::to_string(&snapshot_status.purchase)?,
                    snapshot_status.snapshot_id,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// The value of `key` from the environment, else from the nearest `.env`
/// file in the current directory or one of its parents.
///
/// The `.env` file is read, never exported: loading it into the process
/// environment would race with any thread reading the environment.
#[must_use]
pub fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let cwd = std::env::current_dir().ok()?;
    let env_file = cwd
        .ancestors()
        .map(|dir| dir.join(".env"))
        .find(|path| path.is_file())?;
    dotenv_value(&std::fs::read_to_string(env_file).ok()?, key)
}

/// The value `content`, in `.env` syntax, assigns to `key`.
///
/// `KEY=value` lines, optionally quoted; blank lines and `#` comments are
/// skipped. The first assignment wins.
fn dotenv_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            return None;
        }
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

#[cfg(test)]
mod dotenv_tests {
    use super::dotenv_value;

    #[test]
    fn a_dotenv_value_is_read_unquoted() {
        let content = "# comment\n\nOTHER=1\nNEXAR_TOKEN = \"abc123\"\nNEXAR_TOKEN=later\n";
        assert_eq!(
            dotenv_value(content, "NEXAR_TOKEN"),
            Some("abc123".to_string())
        );
        assert_eq!(dotenv_value(content, "OTHER"), Some("1".to_string()));
        assert_eq!(
            dotenv_value("KEY='single'", "KEY"),
            Some("single".to_string())
        );
    }

    #[test]
    fn a_missing_or_commented_key_has_no_value() {
        assert_eq!(dotenv_value("#NEXAR_TOKEN=abc\n", "NEXAR_TOKEN"), None);
        assert_eq!(dotenv_value("NEXAR_TOKEN_2=abc\n", "NEXAR_TOKEN"), None);
    }
}
