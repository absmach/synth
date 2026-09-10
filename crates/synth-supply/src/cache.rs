// SPDX-License-Identifier: Apache-2.0

//! SQLite cache storage for supply chain query results.

use crate::types::{LifecycleStatus, SupplyError, SupplyStatus};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection};
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
        Ok(())
    }

    /// Query non-expired cached status for `(distributor, part_number)`.
    #[allow(clippy::cast_sign_loss)]
    pub fn get(&self, distributor: &str, part_number: &str) -> Option<SupplyStatus> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT part_number, distributor, in_stock, stock_qty, moq, unit_price_usd, lifecycle, fetched_at
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

            Some(SupplyStatus {
                part_number: row.get(0).ok()?,
                distributor: row.get(1).ok()?,
                in_stock: in_stock_int != 0,
                stock_qty: stock_qty_int.max(0) as u64,
                moq: moq_int.max(0) as u32,
                unit_price_usd: row.get(5).ok()?,
                lifecycle,
                fetched_at: fetched_at_str,
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
             (distributor, part_number, in_stock, stock_qty, moq, unit_price_usd, lifecycle, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                status.distributor,
                status.part_number,
                i32::from(status.in_stock),
                status.stock_qty as i64,
                i64::from(status.moq),
                status.unit_price_usd,
                lifecycle_str,
                status.fetched_at,
            ],
        )?;
        Ok(())
    }
}

/// Helper function to load `.env` from workspace root or current directory into environment.
pub fn load_env_file() {
    // If NEXAR_TOKEN is already set, nothing to do
    if std::env::var("NEXAR_TOKEN").is_ok() {
        return;
    }

    // Try current directory and parents for .env file
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = Some(cwd.as_path());
        while let Some(dir) = curr {
            let env_path = dir.join(".env");
            if env_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&env_path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.starts_with('#') || line.is_empty() {
                            continue;
                        }
                        if let Some((k, v)) = line.split_once('=') {
                            let k = k.trim();
                            let v = v.trim().trim_matches('"').trim_matches('\'');
                            if !k.is_empty() && std::env::var(k).is_err() {
                                std::env::set_var(k, v);
                            }
                        }
                    }
                }
                break;
            }
            curr = dir.parent();
        }
    }
}
