//! Financial data cache — SQLite-backed store for financial statements,
//! market data, and key metrics keyed by internal metric identifiers.
//!
//! Two-layer cache:
//! - **Layer 1 (raw)**: Stores raw API responses keyed by (symbol, endpoint,
//!   params_hash) to avoid re-fetching from FMP/EODHD. TTL varies by endpoint
//!   type — annual financial statements are cached longer than real-time
//!   quotes.
//! - **Layer 2 (metrics)**: Parses raw responses into (symbol, metric,
//!   period, value) tuples using the field-to-metric mapping from `fibo.rs`.
//!   Enables cross-company queries by metric without re-parsing raw
//!   JSON. The metric keys are hKask-internal canonical names, NOT FIBO
//!   URIs — FIBO publishes no terms for financial ratios (verified
//!   2026-08-29).
//!
//! Design: the cache sits between the MCP tool handlers and the provider
//! abstraction (`providers::companies_get`). On a cache hit (fresh entry
//! within TTL), the raw response is returned without an HTTP call. On a miss,
//! the provider fetch is stored in both layers. `CompaniesServer` versions its
//! acquisition keys (`normalized-v1:`) and stores a payload/provider/warnings
//! envelope in layer 1; layer 2 extracts only the normalized financial payload.
//! Old unversioned entries are not read by that acquisition path. The concept store is
//! populated opportunistically — if extraction fails, the raw cache still
//! serves the response.
//!
//! Path: `{kask_data_dir}/mcp/companies/fibo-cache/{owner}/master.db`
//! (databases live in the internal data dir — only artifact files and
//! outputs go to the visible artifacts dir under {server}-mcp/).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::fibo;

// ── TTL by endpoint (seconds) ───────────────────────────────────────

const TTL_FINANCIAL_STATEMENT: u64 = 24 * 60 * 60; // 24h — annual data, rarely changes
const TTL_KEY_METRICS: u64 = 24 * 60 * 60; // 24h
const TTL_COMPANY_PROFILE: u64 = 24 * 60 * 60; // 24h
const TTL_HISTORICAL_PRICE: u64 = 60 * 60; // 1h
const TTL_STOCK_QUOTE: u64 = 5 * 60; // 5 min — near real-time
const TTL_DEFAULT: u64 = 60 * 60; // 1h

fn ttl_for_endpoint(endpoint: &str) -> u64 {
    match endpoint {
        "income_statement" | "balance_sheet" | "cash_flow_statement" => TTL_FINANCIAL_STATEMENT,
        "key_metrics" => TTL_KEY_METRICS,
        "company_profile" => TTL_COMPANY_PROFILE,
        "historical_price" => TTL_HISTORICAL_PRICE,
        "stock_quote" => TTL_STOCK_QUOTE,
        _ => TTL_DEFAULT,
    }
}

// ── Schema ──────────────────────────────────────────────────────────

const SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS fibo_raw_cache (
    symbol TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    params_hash TEXT NOT NULL,
    raw_response TEXT NOT NULL,
    provider TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (symbol, endpoint, params_hash)
);
CREATE INDEX IF NOT EXISTS idx_fibo_raw_symbol ON fibo_raw_cache(symbol);
CREATE TABLE IF NOT EXISTS fibo_concept_store (
    symbol TEXT NOT NULL,
    fibo_concept TEXT NOT NULL,
    period TEXT NOT NULL,
    field_name TEXT NOT NULL,
    value REAL,
    endpoint TEXT NOT NULL,
    provider TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (symbol, fibo_concept, period)
);
-- The fibo_concept column name predates the 2026-08-29 FIBO remediation;
-- it stores internal metric identifiers (e.g. revenue_growth_rate), not
-- FIBO URIs. The name is kept for existing-database compatibility.
CREATE INDEX IF NOT EXISTS idx_fibo_concept_symbol ON fibo_concept_store(symbol);
CREATE INDEX IF NOT EXISTS idx_fibo_concept_concept ON fibo_concept_store(fibo_concept);
CREATE INDEX IF NOT EXISTS idx_fibo_concept_provider ON fibo_concept_store(provider);";

// ── Cache struct ────────────────────────────────────────────────────

/// FIBO-aligned financial data cache backed by SQLite.
///
/// Thread-safe via a `Mutex<Connection>` — all access serializes through
/// the lock. This is acceptable because the cache is a local SQLite file
/// and queries are sub-millisecond; the bottleneck is the network call to
/// FMP/EODHD that the cache avoids.
pub(crate) struct FiboDataCache {
    conn: Mutex<Connection>,
}

/// A raw cache entry — the stored JSON response and its fetch timestamp.
struct RawCacheEntry {
    raw_response: String,
    fetched_at: String,
}

/// A metric data point extracted from a financial statement.
#[derive(Debug, Clone)]
pub(crate) struct ConceptPoint {
    pub symbol: String,
    pub metric: String,
    pub period: String,
    pub field_name: String,
    pub value: Option<f64>,
    pub endpoint: String,
    pub provider: String,
}

/// A FIBO cache open/resolve failure. Cache failures are non-fatal by
/// design — the server logs the Display message and runs uncached — but the
/// kind is structured so a future caller can distinguish a bad path from a
/// corrupt database.
#[derive(Debug, thiserror::Error)]
pub enum FiboCacheError {
    #[error("failed to create fibo-cache directory: {source}")]
    CreateDir {
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open fibo-cache DB: {source}")]
    Open {
        #[source]
        source: rusqlite::Error,
    },
    #[error("failed to initialize fibo-cache schema: {source}")]
    InitSchema {
        #[source]
        source: rusqlite::Error,
    },
}

impl FiboDataCache {
    /// Open (or create) the cache database at the given path.
    pub fn open(db_path: &Path) -> Result<Self, FiboCacheError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| FiboCacheError::CreateDir { source: e })?;
        }
        let conn = Connection::open(db_path).map_err(|e| FiboCacheError::Open { source: e })?;
        conn.execute_batch(SCHEMA_DDL)
            .map_err(|e| FiboCacheError::InitSchema { source: e })?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Check the raw cache for a fresh entry. Returns the cached JSON
    /// response if the entry exists and is within its endpoint-specific TTL.
    /// Returns `None` on miss, stale entry, or DB error (cache failures are
    /// silent — the caller falls through to a live fetch).
    pub fn get_raw(&self, symbol: &str, endpoint: &str, params_hash: &str) -> Option<Value> {
        let conn = self.conn.lock().ok()?;
        let entry: RawCacheEntry = conn
            .query_row(
                "SELECT raw_response, fetched_at FROM fibo_raw_cache \
                 WHERE symbol = ?1 AND endpoint = ?2 AND params_hash = ?3",
                params![symbol, endpoint, params_hash],
                |row| {
                    Ok(RawCacheEntry {
                        raw_response: row.get(0)?,
                        fetched_at: row.get(1)?,
                    })
                },
            )
            .ok()?;

        if !is_fresh(&entry.fetched_at, ttl_for_endpoint(endpoint)) {
            return None;
        }
        serde_json::from_str(&entry.raw_response).ok()
    }

    /// Store a raw API response in the cache, replacing any existing entry
    /// for the same (symbol, endpoint, params_hash) key. Logs a warning on
    /// failure but does not propagate — a cache write failure should not
    /// break the tool call.
    pub fn store_raw(
        &self,
        symbol: &str,
        endpoint: &str,
        params_hash: &str,
        response: &Value,
        provider: &str,
    ) {
        let raw_json = match serde_json::to_string(response) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("fibo_cache: failed to serialize response: {e}");
                return;
            }
        };
        let now = now_iso();
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("fibo_cache: mutex poisoned: {e}");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO fibo_raw_cache \
             (symbol, endpoint, params_hash, raw_response, provider, fetched_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![symbol, endpoint, params_hash, &raw_json, provider, &now],
        ) {
            tracing::warn!("fibo_cache: failed to store raw response: {e}");
        }
    }

    /// Extract metric-tagged data points from a raw API response and store
    /// them in the concept store. The extraction iterates over the response
    /// fields and maps each known field name to its internal metric
    /// identifier using `fibo::fmp_field_to_metric`.
    ///
    /// For financial statements (income, balance sheet, cash flow), the
    /// response is an array of period objects — each object is one fiscal
    /// period. For key metrics, the same shape applies. The `period` field
    /// in each object (FMP uses `calendarYear` or `date`) identifies the
    /// reporting period.
    pub fn extract_and_store_concepts(
        &self,
        symbol: &str,
        endpoint: &str,
        response: &Value,
        provider: &str,
    ) {
        let points = extract_concepts(symbol, endpoint, response, provider);
        if points.is_empty() {
            return;
        }
        let now = now_iso();
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("fibo_cache: mutex poisoned during concept store: {e}");
                return;
            }
        };
        for point in &points {
            if let Err(e) = conn.execute(
                "INSERT OR REPLACE INTO fibo_concept_store \
                 (symbol, fibo_concept, period, field_name, value, endpoint, provider, fetched_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    &point.symbol,
                    &point.metric,
                    &point.period,
                    &point.field_name,
                    point.value,
                    &point.endpoint,
                    &point.provider,
                    &now,
                ],
            ) {
                tracing::warn!(
                    "fibo_cache: failed to store concept {}/{}: {e}",
                    point.metric,
                    point.period
                );
            }
        }
        tracing::debug!(
            "fibo_cache: stored {} concept points for {} {}",
            points.len(),
            symbol,
            endpoint
        );
    }

    /// Query the concept store for a specific metric and symbol.
    /// Returns the most recent value, or `None` if not found.
    #[allow(dead_code)]
    pub fn get_concept(&self, symbol: &str, metric: &str) -> Option<(f64, String)> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT value, period FROM fibo_concept_store \
             WHERE symbol = ?1 AND fibo_concept = ?2 \
             ORDER BY period DESC LIMIT 1",
            params![symbol, metric],
            |row| {
                let value: Option<f64> = row.get(0)?;
                let period: String = row.get(1)?;
                Ok((value.unwrap_or(0.0), period))
            },
        )
        .ok()
    }

    /// Query the concept store for all periods of a metric for a
    /// symbol. Returns (value, period) pairs ordered newest-first.
    #[allow(dead_code)]
    pub fn get_concept_series(&self, symbol: &str, metric: &str) -> Vec<(f64, String)> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(
            "SELECT value, period FROM fibo_concept_store \
             WHERE symbol = ?1 AND fibo_concept = ?2 \
             ORDER BY period DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map(params![symbol, metric], |row| {
                let value: Option<f64> = row.get(0)?;
                let period: String = row.get(1)?;
                Ok((value.unwrap_or(0.0), period))
            })
            .ok();
        match rows {
            Some(mapped) => mapped.filter_map(|r| r.ok()).collect(),
            None => Vec::new(),
        }
    }

    /// Convenience: get the latest revenue growth rate for a symbol from the
    /// concept store.
    #[allow(dead_code)]
    pub fn revenue_growth(&self, symbol: &str) -> Option<f64> {
        self.get_concept(symbol, fibo::METRIC_REVENUE_GROWTH_RATE)
            .map(|(v, _)| v)
    }

    /// Convenience: get the latest ROIC for a symbol from the concept store.
    #[allow(dead_code)]
    pub fn roic(&self, symbol: &str) -> Option<f64> {
        self.get_concept(symbol, fibo::METRIC_RETURN_ON_INVESTED_CAPITAL)
            .map(|(v, _)| v)
    }

    /// Convenience: get the latest gross profit margin.
    #[allow(dead_code)]
    pub fn gross_margin(&self, symbol: &str) -> Option<f64> {
        self.get_concept(symbol, fibo::METRIC_GROSS_PROFIT_MARGIN)
            .map(|(v, _)| v)
    }
}

// ── Concept extraction ──────────────────────────────────────────────

/// Extract metric-tagged data points from a raw API response.
///
/// FMP financial statements and key-metrics responses are arrays of period
/// objects. Each object has a period identifier (`calendarYear`, `date`, or
/// `fillingDate`) and field values that map to internal metric identifiers
/// via `fibo::fmp_field_to_metric`.
fn extract_concepts(
    symbol: &str,
    endpoint: &str,
    response: &Value,
    provider: &str,
) -> Vec<ConceptPoint> {
    let periods = match response.as_array() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut points = Vec::new();
    for period_obj in periods {
        let period = extract_period(period_obj).unwrap_or_default();
        if let Some(obj) = period_obj.as_object() {
            for (field_name, field_value) in obj {
                if let Some(metric) = fibo::fmp_field_to_metric(field_name) {
                    let value = field_value.as_f64();
                    points.push(ConceptPoint {
                        symbol: symbol.to_string(),
                        metric: metric.to_string(),
                        period: period.clone(),
                        field_name: field_name.clone(),
                        value,
                        endpoint: endpoint.to_string(),
                        provider: provider.to_string(),
                    });
                }
            }
        }
    }
    points
}

/// Extract the period identifier from a financial statement object.
/// FMP uses `calendarYear` (e.g. "2025"), `date` (e.g. "2025-12-31"), or
/// `fillingDate` (e.g. "2026-02-15").
fn extract_period(obj: &Value) -> Option<String> {
    obj.get("calendarYear")
        .or_else(|| obj.get("date"))
        .or_else(|| obj.get("fillingDate"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ── Helpers ────────────────────────────────────────────────────────

/// Check if a cached entry's timestamp is within the TTL. Compares against
/// the current UTC time.
fn is_fresh(fetched_at: &str, ttl_seconds: u64) -> bool {
    let parsed = chrono::DateTime::parse_from_rfc3339(fetched_at).or_else(|_| {
        chrono::NaiveDateTime::parse_from_str(fetched_at, "%Y-%m-%d %H:%M:%S").map(|dt| {
            chrono::DateTime::parse_from_rfc3339(&dt.format("%Y-%m-%dT%H:%M:%S+00:00").to_string())
                .unwrap_or_else(|_| {
                    chrono::DateTime::from_timestamp(0, 0)
                        .expect("epoch is valid")
                        .fixed_offset()
                })
        })
    });
    let parsed = match parsed {
        Ok(dt) => dt,
        Err(_) => return false,
    };
    let now = chrono::Utc::now();
    let age = now.signed_duration_since(parsed.with_timezone(&chrono::Utc));
    age.num_seconds() < ttl_seconds as i64
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Hash the extra params into a stable string for use as a cache key.
/// Sorting the params ensures that different orderings of the same params
/// produce the same hash.
pub(crate) fn hash_params(extra: &[(&str, &str)]) -> String {
    if extra.is_empty() {
        return "none".to_string();
    }
    let mut sorted: Vec<(&str, &str)> = extra.to_vec();
    sorted.sort_by_key(|(k, _)| *k);
    sorted
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

// ── Path resolution ────────────────────────────────────────────────

/// Resolve the FIBO cache DB path for an owner.
/// `{kask_data_dir}/mcp/companies/fibo-cache/{owner}/master.db`
///
/// The FIBO cache is a database, so it lives under `mcp/companies/` in the
/// internal data dir — databases are the one artifact class that stays
/// hidden; artifact files and outputs go to the visible artifacts dir.
pub(crate) fn resolve_cache_db_path(owner: &str) -> Result<PathBuf, FiboCacheError> {
    let mut path = hkask_types::agent_paths::resolve_under_data_dir(
        &hkask_types::agent_paths::mcp_server_subdir("companies", "fibo-cache"),
    );
    path.push(sanitize_name(owner));
    std::fs::create_dir_all(&path).map_err(|e| FiboCacheError::CreateDir { source: e })?;
    Ok(path.join("master.db"))
}

fn sanitize_name(name: &str) -> String {
    name.replace(['/', '\\', ':', '?', '*', '"', '<', '>', '|'], "_")
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_cache() -> FiboDataCache {
        let dir = std::env::temp_dir().join(format!("fibo-cache-test-{}", uuid::Uuid::new_v4()));
        FiboDataCache::open(&dir.join("test.db")).expect("open temp cache")
    }

    #[test]
    fn raw_cache_round_trip() {
        let cache = temp_cache();
        let response = json!([{"symbol": "AAPL", "revenue": 100000}]);

        // Miss before store
        assert!(cache.get_raw("AAPL", "income_statement", "none").is_none());

        // Store and hit
        cache.store_raw("AAPL", "income_statement", "none", &response, "FMP");
        let hit = cache
            .get_raw("AAPL", "income_statement", "none")
            .expect("cache hit after store");
        assert_eq!(hit, response);
    }

    #[test]
    fn concept_extraction_from_key_metrics() {
        let cache = temp_cache();
        let response = json!([
            {
                "symbol": "AAPL",
                "calendarYear": "2025",
                "revenueGrowth": 0.15,
                "roic": 0.32,
                "grossProfitMargin": 0.46,
                "peRatio": 28.5,
            }
        ]);

        cache.extract_and_store_concepts("AAPL", "key_metrics", &response, "FMP");

        let growth = cache.revenue_growth("AAPL");
        assert!(growth.is_some(), "revenue growth should be cached");
        assert!(
            (growth.unwrap() - 0.15).abs() < 0.001,
            "revenue growth value should match"
        );

        let roic = cache.roic("AAPL");
        assert!(roic.is_some(), "ROIC should be cached");
        assert!((roic.unwrap() - 0.32).abs() < 0.001);

        let margin = cache.gross_margin("AAPL");
        assert!(margin.is_some(), "gross margin should be cached");
        assert!((margin.unwrap() - 0.46).abs() < 0.001);
    }

    #[test]
    fn concept_series_returns_multiple_periods() {
        let cache = temp_cache();
        let response = json!([
            {"calendarYear": "2025", "revenueGrowth": 0.12},
            {"calendarYear": "2024", "revenueGrowth": 0.08},
            {"calendarYear": "2023", "revenueGrowth": 0.05}
        ]);

        cache.extract_and_store_concepts("MSFT", "key_metrics", &response, "FMP");

        let series = cache.get_concept_series("MSFT", fibo::METRIC_REVENUE_GROWTH_RATE);
        assert_eq!(series.len(), 3, "should have 3 periods");
        assert!((series[0].0 - 0.12).abs() < 0.001, "newest period first");
    }

    #[test]
    fn hash_params_is_order_independent() {
        let a = hash_params(&[("limit", "5"), ("period", "annual")]);
        let b = hash_params(&[("period", "annual"), ("limit", "5")]);
        assert_eq!(a, b, "param hash should be order-independent");
    }

    #[test]
    fn hash_params_empty() {
        assert_eq!(hash_params(&[]), "none");
    }

    #[test]
    fn store_raw_replaces_existing() {
        let cache = temp_cache();
        let v1 = json!([{"revenue": 100}]);
        let v2 = json!([{"revenue": 200}]);

        cache.store_raw("TSLA", "income_statement", "none", &v1, "FMP");
        cache.store_raw("TSLA", "income_statement", "none", &v2, "FMP");

        let hit = cache
            .get_raw("TSLA", "income_statement", "none")
            .expect("hit");
        assert_eq!(hit, v2, "second store should replace first");
    }

    #[test]
    fn stale_entry_returns_none() {
        let cache = temp_cache();
        let response = json!([{"price": 100}]);

        // Store with a very old timestamp by direct insert
        {
            let conn = cache.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO fibo_raw_cache \
                 (symbol, endpoint, params_hash, raw_response, provider, fetched_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "OLD",
                    "stock_quote",
                    "none",
                    serde_json::to_string(&response).unwrap(),
                    "FMP",
                    "2020-01-01T00:00:00+00:00"
                ],
            )
            .unwrap();
        }

        assert!(
            cache.get_raw("OLD", "stock_quote", "none").is_none(),
            "stale entry should not be returned"
        );
    }
}
