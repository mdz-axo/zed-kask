//! hKask MCP Companies — Portfolio tracking (companies-specific layer).
//!
//! The general-purpose transaction ledger, holdings, and returns live in
//! `hkask-mcp-portfolio`. This module holds the companies-specific layer:
//! research notes, file attachments, and DCF forecast snapshots — all keyed
//! by stock symbol. It delegates portfolio CRUD, ledger reads, and returns
//! computation to [`hkask_mcp_portfolio::PortfolioStore`].
//!
//! The `portfolio_returns` tool seeds the store's price cache from FMP/EODHD
//! before delegating to [`hkask_mcp_portfolio::returns`], keeping the
//! portfolio crate provider-agnostic.

use hkask_mcp_portfolio::{
    AssetType, CachedPriceResolver, LedgerFilter, PortfolioStore, export_csv, export_json,
    import_csv, import_json, returns,
};
// Re-export the general types the tool layer imports from this module.
pub use hkask_mcp_portfolio::{PortfolioError, Transaction, TxType};
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;

/// DDL for the companies-specific tables (notes, files, forecasts) that
/// live alongside the portfolio crate's schema in the same SQLite DB.
/// The portfolio crate owns the `portfolios`, `transactions`, `price_cache`,
/// `daily_holdings`, and `daily_returns` tables; this module owns the rest.
const COMPANIES_SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    title TEXT NOT NULL,
                    body TEXT NOT NULL,
                    tags TEXT DEFAULT '[]',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_notes_portfolio ON notes(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_notes_symbol ON notes(symbol);
                CREATE TABLE IF NOT EXISTS files (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    path TEXT NOT NULL,
                    notes TEXT DEFAULT '',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_files_portfolio ON files(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_files_symbol ON files(symbol);
                CREATE TABLE IF NOT EXISTS forecasts (
                    id TEXT PRIMARY KEY,
                    symbol TEXT NOT NULL,
                    revision_of TEXT,
                    snapshot TEXT NOT NULL,
                    outcomes TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_forecasts_symbol ON forecasts(symbol);";

const MAX_ENCODED_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_DECODED_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;

/// Resolve the SQLite DB path for an owner, mirroring the portfolio crate's
/// `PortfolioStore::new` path resolution. The companies module opens its
/// own connection to the same DB for notes/files/forecasts tables.
///
/// D28 — Standardized Artifact Storage. Path is
/// `{kask_data_dir}/mcp/portfolio/{owner}/master.db`.
fn resolve_db_path(owner: &WebID) -> Result<PathBuf, PortfolioError> {
    let mut path = hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
        hkask_types::agent_paths::MCP_DIR,
    ))
    .join("portfolio");
    path.push(sanitize_name(&owner.to_string()));
    // A read-only data dir, a full disk, or a permissions error must surface
    // as an error the server can report, not abort the process at startup.
    // Mirrors `PortfolioStore::new`, which already propagates.
    std::fs::create_dir_all(&path).map_err(|e| {
        PortfolioError::from(format!(
            "failed to create portfolio directory {}: {e}",
            path.display()
        ))
    })?;
    Ok(path.join("master.db"))
}

/// Owner-scoped forecast persisted as structured JSON for later reconstruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedForecast {
    pub id: String,
    pub symbol: String,
    pub revision_of: Option<String>,
    pub snapshot: serde_json::Value,
    #[serde(default)]
    pub outcomes: Vec<serde_json::Value>,
    pub created_at: String,
}

fn parse_forecast_json<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            value.len(),
            rusqlite::types::Type::Text,
            Box::new(e),
        )
    })
}

fn row_to_persisted_forecast(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedForecast> {
    let snapshot: String = row.get(3)?;
    let outcomes: String = row.get(4)?;
    Ok(PersistedForecast {
        id: row.get(0)?,
        symbol: row.get(1)?,
        revision_of: row.get(2)?,
        snapshot: parse_forecast_json(snapshot)?,
        outcomes: parse_forecast_json(outcomes)?,
        created_at: row.get(5)?,
    })
}

// ── Re-exports for tool-layer compatibility ───────────────────────────
//
// The companies tool layer (`tools/portfolio.rs`) imports `PortfolioManager`,
// `PortfolioError`, `Transaction`, `TxType`, and `ValidationReport` from this
// module. We re-export the general types from the portfolio crate so the tool
// layer keeps compiling without changes, and provide a thin `PortfolioManager`
// that delegates the general ops while owning the companies-specific ones.

pub use hkask_mcp_portfolio::{Transaction as PortfolioTransaction, TxType as PortfolioTxType};

/// Companies-side portfolio manager. Holds a [`PortfolioStore`] (the
/// general-purpose ledger/holdings/returns engine from `hkask-mcp-portfolio`)
/// and adds companies-specific research artifacts: notes, files, and DCF
/// forecast snapshots.
#[derive(Clone)]
pub struct PortfolioManager {
    /// The general-purpose store (owns the SQLite DB + schema).
    store: PortfolioStore,
    /// Path to the same SQLite DB the store uses, for companies-specific
    /// tables (notes, files, forecasts). Mirrored at construction so this
    /// module can open its own connection without reaching into the store.
    db_path: PathBuf,
}

impl PortfolioManager {
    /// Creates storage scoped to the authenticated server owner. The
    /// portfolio crate creates the DB and the general schema; this module
    /// adds the companies-specific tables (notes, files, forecasts) on top.
    pub fn new(owner: WebID) -> Result<Self, PortfolioError> {
        let store = PortfolioStore::new(owner)?;
        let db_path = resolve_db_path(&owner)?;
        let manager = Self { store, db_path };
        manager.ensure_companies_schema()?;
        Ok(manager)
    }

    #[cfg(test)]
    pub fn with_dir_for_owner(base_dir: PathBuf, owner: WebID) -> Self {
        Self::with_dir(base_dir.join(sanitize_name(&owner.to_string())))
    }

    #[cfg(test)]
    pub fn with_dir(base_dir: PathBuf) -> Self {
        let db_path = base_dir.join("master.db");
        let store = PortfolioStore::with_dir(base_dir);
        let manager = Self { store, db_path };
        manager
            .ensure_companies_schema()
            .expect("failed to initialize companies schema");
        manager
    }

    /// Open a connection to the same DB the store uses, for companies-specific
    /// tables (notes, files, forecasts).
    fn open(&self) -> Result<Connection, PortfolioError> {
        Connection::open(&self.db_path).map_err(|e| format!("db open: {e}").into())
    }

    fn ensure_companies_schema(&self) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        conn.execute_batch(COMPANIES_SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize companies schema: {e}"))?;
        Ok(())
    }

    /// The underlying general-purpose store. The tool layer uses this for
    /// portfolio CRUD, ledger reads, and returns delegation.
    pub fn store(&self) -> &PortfolioStore {
        &self.store
    }

    // ── General ops (delegated to the portfolio crate) ──────────────

    pub fn create(&self, name: &str) -> Result<(), PortfolioError> {
        self.store.create(name, AssetType::Stock)
    }

    pub fn delete(&self, name: &str) -> Result<(), PortfolioError> {
        self.store.delete(name)
    }

    pub fn list(&self) -> Result<Vec<String>, PortfolioError> {
        self.store.list()
    }

    pub fn add_transaction(&self, name: &str, tx: &Transaction) -> Result<(), PortfolioError> {
        self.store.apply(name, tx)
    }

    pub fn append_note(&self, name: &str, tx_id: &str, note: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let existing: String = conn
            .query_row(
                "SELECT notes FROM transactions WHERE id = ?1 AND portfolio_name = ?2",
                params![tx_id, name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("lookup: {e}"))?
            .unwrap_or_default();
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let updated = if existing.is_empty() {
            format!("[{timestamp}] {note}")
        } else {
            format!("{existing}\n[{timestamp}] {note}")
        };
        conn.execute(
            "UPDATE transactions SET notes = ?1 WHERE id = ?2 AND portfolio_name = ?3",
            params![updated, tx_id, name],
        )
        .map_err(|e| format!("update: {e}"))?;
        Ok(())
    }

    pub fn get_transactions(
        &self,
        name: &str,
        symbol: Option<&str>,
        tx_type: Option<&str>,
        from_date: Option<&str>,
        to_date: Option<&str>,
    ) -> Result<Vec<Transaction>, PortfolioError> {
        self.store.ledger(
            name,
            LedgerFilter {
                symbol,
                tx_type,
                asset_type: None,
                from_date,
                to_date,
            },
        )
    }

    pub fn validate(&self, name: &str) -> Result<ValidationReport, PortfolioError> {
        let txs = self.store.ledger(name, LedgerFilter::all())?;
        let mut issues = Vec::new();
        let mut positions: std::collections::HashMap<String, (f64, f64)> =
            std::collections::HashMap::new();
        let mut cash = 0.0f64;

        for tx in &txs {
            match tx.tx_type {
                TxType::Buy => {
                    let qty = tx.quantity.unwrap_or(0.0);
                    let price = tx.price.unwrap_or(0.0);
                    let comm = tx.commission.unwrap_or(0.0);
                    if qty <= 0.0 {
                        issues.push(format!("{}: buy with non-positive quantity {}", tx.id, qty));
                    }
                    if price <= 0.0 {
                        issues.push(format!("{}: buy with non-positive price {}", tx.id, price));
                    }
                    if let Some(ref sym) = tx.symbol {
                        let entry = positions.entry(sym.clone()).or_insert((0.0, 0.0));
                        entry.0 += qty;
                    }
                    cash -= qty * price + comm;
                }
                TxType::Sell => {
                    let qty = tx.quantity.unwrap_or(0.0);
                    let price = tx.price.unwrap_or(0.0);
                    let comm = tx.commission.unwrap_or(0.0);
                    if qty <= 0.0 {
                        issues.push(format!(
                            "{}: sell with non-positive quantity {}",
                            tx.id, qty
                        ));
                    }
                    if price <= 0.0 {
                        issues.push(format!("{}: sell with non-positive price {}", tx.id, price));
                    }
                    if let Some(ref sym) = tx.symbol {
                        let entry = positions.entry(sym.clone()).or_insert((0.0, 0.0));
                        entry.1 += qty;
                    }
                    cash += qty * price - comm;
                }
                TxType::Dividend => {
                    cash += tx.amount.unwrap_or(0.0);
                }
                TxType::Deposit => {
                    let amt = tx.amount.unwrap_or(0.0);
                    if amt <= 0.0 {
                        issues.push(format!(
                            "{}: deposit with non-positive amount {}",
                            tx.id, amt
                        ));
                    }
                    cash += amt;
                }
                TxType::Withdrawal => {
                    let amt = tx.amount.unwrap_or(0.0);
                    if amt <= 0.0 {
                        issues.push(format!(
                            "{}: withdrawal with non-positive amount {}",
                            tx.id, amt
                        ));
                    }
                    cash -= amt;
                }
                // Rolls and weight adjustments are CMP-index operations not
                // used by stock portfolios; they contribute no cash here.
                TxType::Roll | TxType::WeightAdjust => {}
            }
        }

        let position_summaries: Vec<PositionSummary> = positions
            .into_iter()
            .map(|(symbol, (buys, sells))| PositionSummary {
                symbol,
                shares: buys - sells,
                total_buys: buys,
                total_sells: sells,
            })
            .filter(|p| p.shares.abs() > 0.0001 || p.total_buys > 0.0 || p.total_sells > 0.0)
            .collect();

        Ok(ValidationReport {
            valid: issues.is_empty(),
            transaction_count: txs.len(),
            positions: position_summaries,
            cash_balance: cash,
            issues,
        })
    }

    pub fn import_json(&self, name: &str, json: &str) -> Result<Vec<String>, PortfolioError> {
        import_json(&self.store, name, AssetType::Stock, json)
    }

    pub fn import_csv(&self, name: &str, csv: &str) -> Result<Vec<String>, PortfolioError> {
        import_csv(&self.store, name, AssetType::Stock, csv)
    }

    pub fn export_json(&self, name: &str) -> Result<String, PortfolioError> {
        export_json(&self.store, name)
    }

    pub fn export_csv(&self, name: &str) -> Result<String, PortfolioError> {
        export_csv(&self.store, name)
    }

    pub fn get_symbols(&self, name: &str) -> Result<Vec<String>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT symbol FROM transactions WHERE portfolio_name = ?1 AND symbol IS NOT NULL AND symbol != ''")
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params![name], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query: {e}"))?;
        let mut symbols = Vec::new();
        for row in rows {
            symbols.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(symbols)
    }

    pub fn get_prices(
        &self,
        name: &str,
        symbol: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<(String, f64, String)>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT date, close, source FROM price_cache WHERE portfolio_name = ?1 AND symbol = ?2 AND date >= ?3 AND date <= ?4 ORDER BY date")
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params![name, symbol, from, to], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("query: {e}"))?;
        let mut prices = Vec::new();
        for row in rows {
            prices.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(prices)
    }

    /// Seed the price cache for a (portfolio, symbol, date) triple. Used by
    /// the `portfolio_returns` tool after a successful FMP/EODHD fetch so the
    /// portfolio crate's returns computation reads from the cache.
    pub fn seed_price_cache(
        &self,
        portfolio: &str,
        symbol: &str,
        date: &str,
        close: f64,
        source: &str,
    ) -> Result<(), PortfolioError> {
        let resolver = CachedPriceResolver::new(&self.store, portfolio);
        resolver.seed_cache(symbol, date, close, source)
    }

    /// Compute returns by delegating to the portfolio crate. The caller is
    /// responsible for seeding the price cache first (the companies
    /// `portfolio_returns` tool does this from FMP/EODHD).
    pub fn compute_returns(
        &self,
        name: &str,
        from: &str,
        to: &str,
    ) -> Result<hkask_mcp_portfolio::ReturnsReport, PortfolioError> {
        let resolver = CachedPriceResolver::new(&self.store, name);
        returns(&self.store, name, from, to, &resolver)
    }

    pub fn compare(&self, name_a: &str, name_b: &str) -> Result<serde_json::Value, PortfolioError> {
        let report_a = self.validate(name_a)?;
        let report_b = self.validate(name_b)?;

        let positions_a: std::collections::HashMap<&str, &PositionSummary> = report_a
            .positions
            .iter()
            .map(|p| (p.symbol.as_str(), p))
            .collect();
        let positions_b: std::collections::HashMap<&str, &PositionSummary> = report_b
            .positions
            .iter()
            .map(|p| (p.symbol.as_str(), p))
            .collect();

        let all_symbols: std::collections::BTreeSet<&str> = positions_a
            .keys()
            .chain(positions_b.keys())
            .copied()
            .collect();

        let mut shared = Vec::new();
        let mut only_a = Vec::new();
        let mut only_b = Vec::new();

        for sym in &all_symbols {
            match (positions_a.get(sym), positions_b.get(sym)) {
                (Some(pa), Some(pb)) => shared.push(serde_json::json!({
                    "symbol": sym,
                    "shares_a": pa.shares,
                    "shares_b": pb.shares,
                    "buys_a": pa.total_buys,
                    "sells_a": pa.total_sells,
                    "buys_b": pb.total_buys,
                    "sells_b": pb.total_sells,
                })),
                (Some(pa), None) => only_a.push(serde_json::json!({
                    "symbol": sym,
                    "shares": pa.shares,
                    "buys": pa.total_buys,
                    "sells": pa.total_sells,
                })),
                (None, Some(pb)) => only_b.push(serde_json::json!({
                    "symbol": sym,
                    "shares": pb.shares,
                    "buys": pb.total_buys,
                    "sells": pb.total_sells,
                })),
                (None, None) => unreachable!(),
            }
        }

        Ok(serde_json::json!({
            "portfolio_a": {
                "name": name_a,
                "transactions": report_a.transaction_count,
                "positions": report_a.positions.len(),
                "cash": report_a.cash_balance,
            },
            "portfolio_b": {
                "name": name_b,
                "transactions": report_b.transaction_count,
                "positions": report_b.positions.len(),
                "cash": report_b.cash_balance,
            },
            "shared_positions": shared,
            "only_in_a": only_a,
            "only_in_b": only_b,
        }))
    }

    // ── Companies-specific: forecasts ──────────────────────────────

    pub fn save_forecast(&self, forecast: &PersistedForecast) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let snapshot = serde_json::to_string(&forecast.snapshot)
            .map_err(|e| format!("serialize forecast snapshot: {e}"))?;
        let outcomes = serde_json::to_string(&forecast.outcomes)
            .map_err(|e| format!("serialize forecast outcomes: {e}"))?;
        conn.execute(
            "INSERT INTO forecasts (id, symbol, revision_of, snapshot, outcomes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                forecast.id,
                forecast.symbol,
                forecast.revision_of,
                snapshot,
                outcomes,
                forecast.created_at,
            ],
        )
        .map_err(|e| format!("save forecast: {e}"))?;
        Ok(())
    }

    pub fn get_forecast(&self, id: &str) -> Result<Option<PersistedForecast>, PortfolioError> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT id, symbol, revision_of, snapshot, outcomes, created_at FROM forecasts WHERE id = ?1",
            params![id],
            row_to_persisted_forecast,
        )
        .optional()
        .map_err(|e| format!("get forecast: {e}").into())
    }

    pub fn list_forecasts(&self, symbol: &str) -> Result<Vec<PersistedForecast>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, symbol, revision_of, snapshot, outcomes, created_at
                 FROM forecasts WHERE symbol = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| format!("list forecasts: {e}"))?;
        let rows = stmt
            .query_map(params![symbol], row_to_persisted_forecast)
            .map_err(|e| format!("list forecasts: {e}"))?;
        rows.map(|row| row.map_err(|e| format!("forecast row: {e}").into()))
            .collect()
    }

    pub fn validate_forecast_revision(&self, id: &str, symbol: &str) -> Result<(), PortfolioError> {
        let Some(parent) = self.get_forecast(id)? else {
            return Err(format!("forecast '{id}' not found for this owner").into());
        };
        if parent.symbol != symbol {
            return Err(format!(
                "forecast '{id}' belongs to symbol '{}', not '{symbol}'",
                parent.symbol
            )
            .into());
        }
        Ok(())
    }

    pub fn record_forecast_outcome(
        &self,
        id: &str,
        outcome: serde_json::Value,
    ) -> Result<(), PortfolioError> {
        let mut forecast = self
            .get_forecast(id)?
            .ok_or_else(|| format!("forecast '{id}' not found for this owner"))?;
        forecast.outcomes.push(outcome);
        let outcomes = serde_json::to_string(&forecast.outcomes)
            .map_err(|e| format!("serialize forecast outcomes: {e}"))?;
        let conn = self.open()?;
        conn.execute(
            "UPDATE forecasts SET outcomes = ?1 WHERE id = ?2",
            params![outcomes, id],
        )
        .map_err(|e| format!("record forecast outcome: {e}"))?;
        Ok(())
    }

    // ── Companies-specific: notes ───────────────────────────────────

    pub fn add_note(
        &self,
        portfolio: &str,
        symbol: &str,
        date: &str,
        title: &str,
        body: &str,
        tags: &[String],
    ) -> Result<String, PortfolioError> {
        let conn = self.open()?;
        let id = uuid::Uuid::new_v4().to_string();
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
        let now = now_rfc3339();
        conn.execute(
            "INSERT INTO notes (id, portfolio_name, symbol, date, title, body, tags, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, portfolio, symbol, date, title, body, tags_json, now],
        )
        .map_err(|e| format!("add_note: {e}"))?;
        Ok(id)
    }

    pub fn list_notes(
        &self,
        portfolio: &str,
        symbol: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        let mut sql = "SELECT id, symbol, date, title, body, tags, created_at FROM notes WHERE portfolio_name = ?1 AND symbol = ?2".to_string();
        let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(portfolio.to_string()),
            Box::new(symbol.to_string()),
        ];

        if let Some(f) = date_from {
            bind_values.push(Box::new(f.to_string()));
            sql.push_str(&format!(" AND date >= ?{}", bind_values.len()));
        }
        if let Some(t) = date_to {
            bind_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND date <= ?{}", bind_values.len()));
        }
        sql.push_str(" ORDER BY date DESC");

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            bind_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let tags_str: String = row.get::<_, String>(5).unwrap_or_default();
                let parsed_tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "symbol": row.get::<_, String>(1)?,
                    "date": row.get::<_, String>(2)?,
                    "title": row.get::<_, String>(3)?,
                    "body": row.get::<_, String>(4)?,
                    "tags": parsed_tags,
                    "created_at": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut notes = Vec::new();
        for row in rows {
            let note = row.map_err(|e| format!("row: {e}"))?;
            if let Some(filter_tags) = tags {
                let note_tags: Vec<&str> = note["tags"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let has_any = filter_tags.iter().any(|t| note_tags.contains(&t.as_str()));
                if !has_any {
                    continue;
                }
            }
            notes.push(note);
        }
        Ok(notes)
    }

    pub fn delete_note(&self, note_id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let rows = conn
            .execute("DELETE FROM notes WHERE id = ?1", params![note_id])
            .map_err(|e| format!("delete_note: {e}"))?;
        if rows == 0 {
            return Err(format!("note '{note_id}' not found").into());
        }
        Ok(())
    }

    // ── Companies-specific: file attachments ───────────────────────

    fn base_dir(&self) -> &std::path::Path {
        self.db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
    }

    pub fn attach_file(
        &self,
        portfolio: &str,
        symbol: &str,
        date: &str,
        filename: &str,
        mime_type: &str,
        data_b64: &str,
        notes: &str,
    ) -> Result<String, PortfolioError> {
        if data_b64.len() > MAX_ENCODED_ATTACHMENT_BYTES {
            return Err(format!(
                "encoded attachment exceeds maximum of {MAX_ENCODED_ATTACHMENT_BYTES} bytes"
            )
            .into());
        }
        let conn = self.open()?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_b64)
            .map_err(|e| format!("invalid base64 data: {e}"))?;
        if bytes.len() > MAX_DECODED_ATTACHMENT_BYTES {
            return Err(format!(
                "decoded attachment exceeds maximum of {MAX_DECODED_ATTACHMENT_BYTES} bytes"
            )
            .into());
        }

        let id = uuid::Uuid::new_v4().to_string();
        let safe_filename = format!("{id}_{}", sanitize_name(filename));
        let files_dir = self.base_dir().join(portfolio).join("files");
        if let Err(error) = std::fs::create_dir_all(&files_dir) {
            tracing::warn!(target: "hkask.mcp.companies", %error, "failed to create files dir");
        }
        let file_path = files_dir.join(&safe_filename);

        std::fs::write(&file_path, &bytes).map_err(|e| format!("write file: {e}"))?;

        let path_str = file_path.to_string_lossy().to_string();
        let size = bytes.len() as i64;
        let now = now_rfc3339();

        if let Err(error) = conn.execute(
            "INSERT INTO files (id, portfolio_name, symbol, date, filename, mime_type, size, path, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, portfolio, symbol, date, filename, mime_type, size, path_str, notes, now],
        ) {
            if let Err(cleanup_error) = std::fs::remove_file(&file_path) {
                return Err(format!(
                    "attach_file: {error}; failed to remove written file '{}': {cleanup_error}",
                    file_path.display()
                ).into());
            }
            return Err(format!("attach_file: {error}").into());
        }

        Ok(id)
    }

    pub fn list_files(
        &self,
        portfolio: &str,
        symbol: &str,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, symbol, date, filename, mime_type, size, path, notes, created_at FROM files WHERE portfolio_name = ?1 AND symbol = ?2 ORDER BY date DESC",
            )
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params![portfolio, symbol], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "symbol": row.get::<_, String>(1)?,
                    "date": row.get::<_, String>(2)?,
                    "filename": row.get::<_, String>(3)?,
                    "mime_type": row.get::<_, String>(4)?,
                    "size": row.get::<_, i64>(5)?,
                    "path": row.get::<_, String>(6)?,
                    "notes": row.get::<_, String>(7)?,
                    "created_at": row.get::<_, String>(8)?,
                }))
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(files)
    }

    pub fn delete_file(&self, file_id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let path: String = conn
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("lookup: {e}"))?;

        let rows = conn
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])
            .map_err(|e| format!("delete_file: metadata deletion failed: {e}"))?;
        if rows == 0 {
            return Err(format!("delete_file: metadata for '{file_id}' was not found").into());
        }

        std::fs::remove_file(&path).map_err(|e| {
            format!("delete_file: metadata removed but failed to delete file '{path}': {e}")
        })?;

        Ok(())
    }
}

// ── Validation report (companies-side view) ───────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub transaction_count: usize,
    pub positions: Vec<PositionSummary>,
    pub cash_balance: f64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionSummary {
    pub symbol: String,
    pub shares: f64,
    pub total_buys: f64,
    pub total_sells: f64,
}

// Re-export the portfolio crate's error and transaction types so the tool
// layer's imports (`use crate::portfolio::{PortfolioError, Transaction, TxType}`)
// keep resolving. (Declared at the top of this module.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_constructs_and_creates_portfolio() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        manager.create("test").unwrap();
        assert!(manager.list().unwrap().contains(&"test".to_string()));
    }

    #[test]
    fn transaction_round_trips_through_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        manager.create("p").unwrap();
        let tx = Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            date: "2024-01-02".to_string(),
            tx_type: TxType::Deposit,
            asset_type: AssetType::Stock,
            symbol: None,
            quantity: None,
            price: None,
            commission: None,
            amount: Some(1000.0),
            weight: None,
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        manager.add_transaction("p", &tx).unwrap();
        let txs = manager
            .get_transactions("p", None, None, None, None)
            .unwrap();
        assert_eq!(txs.len(), 1);
    }

    #[test]
    fn notes_and_files_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        manager.create("p").unwrap();
        let note_id = manager
            .add_note("p", "AAPL", "2024-01-01", "t", "b", &[])
            .unwrap();
        let notes = manager.list_notes("p", "AAPL", None, None, None).unwrap();
        assert_eq!(notes.len(), 1);
        manager.delete_note(&note_id).unwrap();
    }

    #[test]
    fn forecasts_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        let forecast = PersistedForecast {
            id: "f1".to_string(),
            symbol: "AAPL".to_string(),
            revision_of: None,
            snapshot: serde_json::json!({"a": 1}),
            outcomes: vec![],
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        manager.save_forecast(&forecast).unwrap();
        let got = manager.get_forecast("f1").unwrap().unwrap();
        assert_eq!(got.symbol, "AAPL");
    }

    #[test]
    fn compute_returns_delegates_to_portfolio_crate() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        manager.create("p").unwrap();
        manager
            .add_transaction(
                "p",
                &Transaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    date: "2024-01-02".to_string(),
                    tx_type: TxType::Deposit,
                    asset_type: AssetType::Stock,
                    symbol: None,
                    quantity: None,
                    price: None,
                    commission: None,
                    amount: Some(20000.0),
                    weight: None,
                    currency: "USD".to_string(),
                    notes: String::new(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        manager
            .add_transaction(
                "p",
                &Transaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    date: "2024-01-15".to_string(),
                    tx_type: TxType::Buy,
                    asset_type: AssetType::Stock,
                    symbol: Some("AAPL".to_string()),
                    quantity: Some(100.0),
                    price: Some(150.0),
                    commission: Some(0.0),
                    amount: None,
                    weight: None,
                    currency: "USD".to_string(),
                    notes: String::new(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        // Seed the price cache (the companies tool does this from FMP/EODHD).
        manager
            .seed_price_cache("p", "AAPL", "2024-01-02", 150.0, "test")
            .unwrap();
        manager
            .seed_price_cache("p", "AAPL", "2024-03-31", 165.0, "test")
            .unwrap();
        let report = manager
            .compute_returns("p", "2024-01-02", "2024-03-31")
            .unwrap();
        assert!(
            (report.total_return - 0.075).abs() < 0.0001,
            "total_return = {}",
            report.total_return
        );
        assert!(report.irr_converged);
    }

    /// Pin the delegation seam: the companies `PortfolioManager` must hold a
    /// `hkask_mcp_portfolio::PortfolioStore` and delegate general ops
    /// (create/list/apply/ledger/returns) through it, not re-implement them.
    /// If someone re-inlines the returns computation or drops the store field,
    /// this test fails — the delegation is a deliberate architectural
    /// decision (the companies server depends on the portfolio server for
    /// portfolio operations, per the extraction).
    #[test]
    fn portfolio_manager_delegates_to_portfolio_store() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PortfolioManager::with_dir(dir.path().to_path_buf());
        // The store() accessor returns the underlying PortfolioStore —
        // the delegation seam. If this field is removed, the test won't compile.
        let _store: &hkask_mcp_portfolio::PortfolioStore = manager.store();
        // create + list delegate through the store.
        manager.create("seam-test").unwrap();
        let names = manager.list().unwrap();
        assert!(names.contains(&"seam-test".to_string()));
        // The store sees the same portfolio (same DB).
        let store_names = manager.store().list().unwrap();
        assert!(store_names.contains(&"seam-test".to_string()));
    }
}
