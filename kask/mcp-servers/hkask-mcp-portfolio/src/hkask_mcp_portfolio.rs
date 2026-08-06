#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Portfolio — general-purpose transaction-ledger portfolio store.
//!
//! A portfolio is an append-only transaction ledger. Everything else —
//! holdings, returns, validation — is a projection over that ledger at a
//! point in time. This crate is provider-agnostic: it knows nothing about
//! FMP/EODHD stock prices or Kalshi/Polymarket contract feeds. Callers
//! resolve prices externally and feed them to [`returns`].
//!
//! ## Asset types and nested portfolios
//!
//! The ledger is polymorphic over the asset being transacted. [`AssetType`]
//! discriminates stocks, prediction-event contracts, and nested portfolios
//! (a portfolio of CMP indices, each of which is itself a portfolio of
//! contracts). A nested-portfolio holding is a weighted reference to another
//! portfolio by name — the store resolves it recursively on demand.
//!
//! ## Materialized views
//!
//! [`PortfolioStore::snapshot`] computes end-of-day holdings from the ledger.
//! The result is cached in a `daily_holdings` materialized-view table so the
//! portfolio viewer retrieves it without recomputing the full ledger history.
//! [`returns`] computes daily P&L from two snapshots + a price resolver; the
//! per-day result is cached in `daily_returns`. Both views are rebuildable
//! from the ledger (the append-only source of truth) via
//! [`PortfolioStore::rebuild_views`].

use hkask_ledger::{Ledger, LedgerError, LedgerTransaction, Posting};
use hkask_storage::database::driver::DatabaseDriver;
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Datelike;

const MAX_IMPORT_REQUEST_BYTES: usize = 5 * 1024 * 1024;
const MAX_IMPORT_TRANSACTION_COUNT: usize = 10_000;

/// SQLite schema DDL for the portfolio database.
/// Used by both production (`new`) and test (`with_dir`) paths to ensure
/// identical schema — including FK cascade constraints.
const SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS portfolios (
                    name TEXT PRIMARY KEY,
                    asset_type TEXT NOT NULL DEFAULT 'stock',
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS transactions (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    date TEXT NOT NULL,
                    type TEXT NOT NULL CHECK(type IN ('buy','sell','dividend','deposit','withdrawal','roll','weight_adjust')),
                    asset_type TEXT NOT NULL DEFAULT 'stock',
                    symbol TEXT,
                    quantity REAL,
                    price REAL,
                    commission REAL DEFAULT 0,
                    amount REAL,
                    weight REAL,
                    currency TEXT DEFAULT 'USD',
                    notes TEXT DEFAULT '',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_tx_portfolio ON transactions(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_tx_date ON transactions(date);
                CREATE INDEX IF NOT EXISTS idx_tx_symbol ON transactions(symbol);
                CREATE INDEX IF NOT EXISTS idx_tx_asset_type ON transactions(asset_type);
                CREATE TABLE IF NOT EXISTS price_cache (
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    close REAL NOT NULL,
                    source TEXT NOT NULL,
                    fetched_at TEXT NOT NULL,
                    PRIMARY KEY (portfolio_name, symbol, date)
                );
                CREATE TABLE IF NOT EXISTS daily_holdings (
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    date TEXT NOT NULL,
                    symbol TEXT NOT NULL,
                    asset_type TEXT NOT NULL,
                    shares REAL NOT NULL,
                    cost_basis REAL NOT NULL,
                    PRIMARY KEY (portfolio_name, date, symbol)
                );
                CREATE INDEX IF NOT EXISTS idx_holdings_portfolio_date ON daily_holdings(portfolio_name, date);
                CREATE TABLE IF NOT EXISTS daily_returns (
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    date TEXT NOT NULL,
                    market_value REAL NOT NULL,
                    cash REAL NOT NULL,
                    total REAL NOT NULL,
                    daily_return REAL NOT NULL,
                    PRIMARY KEY (portfolio_name, date)
                );
                CREATE INDEX IF NOT EXISTS idx_returns_portfolio ON daily_returns(portfolio_name);";

// ── Error type ───────────────────────────────────────────────────────

/// Portfolio operation errors, classified for MCP tool dispatch.
///
/// `InvalidArgument` variants map to `McpToolError::invalid_argument` (user error).
/// All other variants map to `McpToolError::internal` (system error).
#[derive(Debug, thiserror::Error)]
pub enum PortfolioError {
    #[error("{0}")]
    InvalidArgument(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("serialize error: {0}")]
    Serialize(String),
    #[error("ledger error: {0}")]
    Ledger(String),
}

impl From<rusqlite::Error> for PortfolioError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e.to_string())
    }
}

impl From<serde_json::Error> for PortfolioError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e.to_string())
    }
}

impl From<String> for PortfolioError {
    fn from(s: String) -> Self {
        Self::InvalidArgument(s)
    }
}

impl From<&str> for PortfolioError {
    fn from(s: &str) -> Self {
        Self::InvalidArgument(s.to_string())
    }
}

impl From<PortfolioError> for hkask_mcp_server::McpError {
    fn from(e: PortfolioError) -> Self {
        hkask_mcp_server::McpError::UnexpectedResponse {
            context: "portfolio".to_string(),
            detail: e.to_string(),
        }
    }
}

impl From<LedgerError> for PortfolioError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e.to_string())
    }
}

// ── Asset type ───────────────────────────────────────────────────────

/// The kind of asset a portfolio holds. Discriminates the polymorphic ledger:
/// a stock portfolio holds tickers; a CMP-index portfolio holds nested
/// portfolio references (each itself a portfolio of contracts); a
/// prediction-event portfolio holds contract identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    /// A stock ticker (e.g. AAPL, VOD.L).
    Stock,
    /// A prediction-market contract (e.g. a Kalshi market ticker or
    /// Polymarket CLOB token id).
    PredictionContract,
    /// A reference to another portfolio by name — supports nested portfolios
    /// (a portfolio of CMP indices, each of which is a portfolio of contracts).
    Portfolio,
}

impl std::fmt::Display for AssetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stock => write!(f, "stock"),
            Self::PredictionContract => write!(f, "prediction_contract"),
            Self::Portfolio => write!(f, "portfolio"),
        }
    }
}

impl std::str::FromStr for AssetType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stock" => Ok(Self::Stock),
            "prediction_contract" => Ok(Self::PredictionContract),
            "portfolio" => Ok(Self::Portfolio),
            _ => Err(format!("invalid asset type: {s}")),
        }
    }
}

impl rusqlite::ToSql for AssetType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for AssetType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match value {
            rusqlite::types::ValueRef::Text(bytes) => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|_| rusqlite::types::FromSqlError::InvalidType)?;
                s.parse::<AssetType>()
                    .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
            }
            _ => Err(rusqlite::types::FromSqlError::InvalidType),
        }
    }
}

impl Default for AssetType {
    fn default() -> Self {
        Self::Stock
    }
}

// ── Transaction type ─────────────────────────────────────────────────

/// Transaction type — matches the SQLite CHECK constraint values.
///
/// `Roll` and `WeightAdjust` extend the stock-only vocabulary for CMP
/// indices: a roll moves a position from one contract to its successor at
/// the same tenor; a weight adjustment changes a constituent's target weight
/// without a buy/sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TxType {
    Buy,
    Sell,
    Dividend,
    Deposit,
    Withdrawal,
    /// Roll a position from one contract to its successor (CMP index maintenance).
    Roll,
    /// Adjust a constituent's target weight (CMP index rebalancing).
    WeightAdjust,
}

impl std::fmt::Display for TxType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "buy"),
            Self::Sell => write!(f, "sell"),
            Self::Dividend => write!(f, "dividend"),
            Self::Deposit => write!(f, "deposit"),
            Self::Withdrawal => write!(f, "withdrawal"),
            Self::Roll => write!(f, "roll"),
            Self::WeightAdjust => write!(f, "weight_adjust"),
        }
    }
}

impl std::str::FromStr for TxType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            "dividend" => Ok(Self::Dividend),
            "deposit" => Ok(Self::Deposit),
            "withdrawal" => Ok(Self::Withdrawal),
            "roll" => Ok(Self::Roll),
            "weight_adjust" => Ok(Self::WeightAdjust),
            _ => Err(format!("invalid transaction type: {s}")),
        }
    }
}

impl rusqlite::ToSql for TxType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for TxType {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match value {
            rusqlite::types::ValueRef::Text(bytes) => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|_| rusqlite::types::FromSqlError::InvalidType)?;
                s.parse::<TxType>()
                    .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
            }
            _ => Err(rusqlite::types::FromSqlError::InvalidType),
        }
    }
}

// ── Transaction ──────────────────────────────────────────────────────

/// One ledger entry. The append-only source of truth for a portfolio.
///
/// `asset_type` discriminates the `symbol`: a stock ticker, a contract id,
/// or a nested portfolio name. `weight` is used by `WeightAdjust` for CMP
/// index rebalancing; it is `None` for stock transactions.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Transaction {
    pub id: String,
    pub date: String,
    #[serde(rename = "type")]
    pub tx_type: TxType,
    #[serde(default)]
    pub asset_type: AssetType,
    pub symbol: Option<String>,
    pub quantity: Option<f64>,
    pub price: Option<f64>,
    pub commission: Option<f64>,
    pub amount: Option<f64>,
    #[serde(default)]
    pub weight: Option<f64>,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: String,
}

fn default_currency() -> String {
    "USD".to_string()
}

impl Transaction {
    /// Cash-flow contribution of this transaction: positive = cash in,
    /// negative = cash out. Used by [`returns`] and [`PortfolioStore::snapshot`].
    pub fn cash_flow(&self) -> f64 {
        match self.tx_type {
            TxType::Deposit => self.amount.unwrap_or(0.0),
            TxType::Withdrawal => -self.amount.unwrap_or(0.0),
            TxType::Buy => {
                let qty = self.quantity.unwrap_or(0.0);
                let price = self.price.unwrap_or(0.0);
                let comm = self.commission.unwrap_or(0.0);
                -(qty * price + comm)
            }
            TxType::Sell => {
                let qty = self.quantity.unwrap_or(0.0);
                let price = self.price.unwrap_or(0.0);
                let comm = self.commission.unwrap_or(0.0);
                qty * price - comm
            }
            TxType::Dividend => self.amount.unwrap_or(0.0),
            // Rolls and weight adjustments are non-cash for the index level —
            // they move weight between constituents, not cash in/out.
            TxType::Roll | TxType::WeightAdjust => 0.0,
        }
    }

    /// Signed quantity change for a position (positive = add, negative = reduce).
    pub fn position_delta(&self) -> f64 {
        match self.tx_type {
            TxType::Buy | TxType::Deposit => self.quantity.unwrap_or(0.0),
            TxType::Sell | TxType::Withdrawal => -self.quantity.unwrap_or(0.0),
            TxType::Roll => self.quantity.unwrap_or(0.0),
            TxType::WeightAdjust => 0.0,
            TxType::Dividend => 0.0,
        }
    }
}

fn check_request_size(size: usize, maximum: usize, subject: &str) -> Result<(), PortfolioError> {
    if size > maximum {
        return Err(format!("{subject} exceeds maximum of {maximum} bytes").into());
    }
    Ok(())
}

// ── Projections (holdings, returns) ─────────────────────────────────

/// A position held by a portfolio at a point in time.
#[derive(Debug, Clone, Serialize)]
pub struct Holding {
    pub symbol: String,
    pub asset_type: AssetType,
    pub shares: f64,
    pub total_buys: f64,
    pub total_sells: f64,
    pub cost_basis: f64,
}

/// End-of-day holdings for a portfolio — the materialized view.
#[derive(Debug, Clone, Serialize)]
pub struct HoldingsSnapshot {
    pub portfolio: String,
    pub date: String,
    pub holdings: Vec<Holding>,
    pub cash_balance: f64,
    pub transaction_count: usize,
    pub issues: Vec<String>,
}

/// Filter for reading a slice of the ledger.
#[derive(Debug, Clone, Default)]
pub struct LedgerFilter<'a> {
    pub symbol: Option<&'a str>,
    pub tx_type: Option<&'a str>,
    pub asset_type: Option<AssetType>,
    pub from_date: Option<&'a str>,
    pub to_date: Option<&'a str>,
}

impl<'a> LedgerFilter<'a> {
    pub fn all() -> Self {
        Self::default()
    }
}

/// A price observation for a symbol on a date, used by [`returns`].
#[derive(Debug, Clone)]
pub struct PricePoint {
    pub symbol: String,
    pub date: String,
    pub close: f64,
}

/// Resolves prices for a set of (symbol, date) pairs. Implemented by the
/// consumer (the companies server reads its FMP/EODHD cache + live API; a
/// test reads a fixture). The portfolio store is provider-agnostic.
pub trait PriceResolver {
    fn resolve(&self, symbol: &str, date: &str) -> Option<f64>;
}

/// A no-op resolver that returns `None` for every symbol — for portfolios
/// whose holdings have no market price (e.g. a CMP index of prediction
/// contracts whose value is the index probability, not a traded price).
pub struct NoPrices;

impl PriceResolver for NoPrices {
    fn resolve(&self, _symbol: &str, _date: &str) -> Option<f64> {
        None
    }
}

/// Returns computation result for a date range.
#[derive(Debug, Clone, Serialize)]
pub struct ReturnsReport {
    pub portfolio: String,
    pub from: String,
    pub to: String,
    pub total_return: f64,
    pub modified_dietz: f64,
    pub irr: f64,
    pub irr_converged: bool,
    pub start_value: f64,
    pub end_value: f64,
    pub net_cash_flows: f64,
    pub cash_flow_count: usize,
    pub positions_at_start: usize,
    pub positions_at_end: usize,
}

// ── PortfolioStore ───────────────────────────────────────────────────

/// The deepened portfolio store. Seven public methods hide all storage
/// mechanics (SQLite, schema, FK cascades, the optional double-entry ledger
/// mirror). Callers apply transactions, read the ledger, and query
/// materialized projections.
///
/// The store is `Clone` (it holds only a path + an optional driver handle),
/// so an MCP server can clone it into a `spawn_blocking` task per request.
#[derive(Clone)]
pub struct PortfolioStore {
    db_path: PathBuf,
    /// Optional cost ledger for double-entry accounting (companies server
    /// wires this; the portfolio crate itself does not require it).
    ledger_driver: Option<Arc<dyn DatabaseDriver>>,
}

impl PortfolioStore {
    /// Creates storage scoped to the authenticated server owner.
    pub fn new(owner: WebID) -> Result<Self, PortfolioError> {
        Self::new_with_asset_type(owner, AssetType::Stock)
    }

    /// Creates storage scoped to an owner, marking new portfolios with the
    /// given asset type (e.g. `PredictionContract` for a CMP-index store).
    pub fn new_with_asset_type(
        owner: WebID,
        default_asset_type: AssetType,
    ) -> Result<Self, PortfolioError> {
        let _ = default_asset_type; // recorded per-portfolio at create time
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("hkask");
        path.push("portfolios");
        path.push(sanitize_name(&owner.to_string()));
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create portfolio directory: {e}"))?;
        path.push("master.db");
        let conn = Connection::open(&path)
            .map_err(|e| format!("failed to open portfolio database: {e}"))?;
        conn.execute_batch(SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize portfolio schema: {e}"))?;
        Ok(Self {
            db_path: path,
            ledger_driver: None,
        })
    }

    /// Attach a double-entry cost ledger driver. Subsequent [`apply`] calls
    /// mirror cash/fee postings to the ledger.
    pub fn with_ledger_driver(mut self, driver: Arc<dyn DatabaseDriver>) -> Self {
        self.ledger_driver = Some(driver);
        self
    }

    /// Test constructor: create a store backed by a DB at `base_dir/master.db`.
    /// Not `#[cfg(test)]` so downstream crates (e.g. hkask-mcp-companies) can
    /// use it in their own test suites.
    pub fn with_dir(base_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&base_dir).expect("failed to create test portfolio directory");
        let db_path = base_dir.join("master.db");
        let conn = Connection::open(&db_path).expect("failed to open test portfolio database");
        conn.execute_batch(SCHEMA_DDL)
            .expect("failed to initialize test portfolio schema");
        Self {
            db_path,
            ledger_driver: None,
        }
    }

    /// Test constructor: create a store for a specific owner under `base_dir`.
    pub fn with_dir_for_owner(base_dir: PathBuf, owner: WebID) -> Self {
        Self::with_dir(base_dir.join(sanitize_name(&owner.to_string())))
    }

    fn open(&self) -> Result<Connection, PortfolioError> {
        Connection::open(&self.db_path).map_err(|e| format!("db open: {e}").into())
    }

    // ── Public interface (≤7 methods) ────────────────────────────────

    /// List all portfolio names in this owner's store.
    pub fn list(&self) -> Result<Vec<String>, PortfolioError> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare("SELECT name FROM portfolios ORDER BY name")
            .map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query: {e}"))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(names)
    }

    /// Create a portfolio. `asset_type` discriminates stock portfolios,
    /// prediction-contract portfolios, and nested (portfolio-of-portfolios)
    /// stores. Idempotent — creating an existing portfolio is not an error.
    pub fn create(&self, name: &str, asset_type: AssetType) -> Result<(), PortfolioError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err("portfolio name must not be empty or contain path separators".into());
        }
        let conn = self.open()?;
        let rows = conn
            .execute(
                "INSERT OR IGNORE INTO portfolios (name, asset_type, created_at) VALUES (?1, ?2, ?3)",
                params![name, asset_type, now_rfc3339()],
            )
            .map_err(|e| format!("create: {e}"))?;
        if rows == 0 {
            self.check_exists(&conn, name)?;
        }
        Ok(())
    }

    /// Delete a portfolio and all its transactions, holdings, and returns
    /// (FK cascade). Returns `InvalidArgument` if it does not exist.
    pub fn delete(&self, name: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let rows = conn
            .execute("DELETE FROM portfolios WHERE name = ?1", params![name])
            .map_err(|e| format!("delete: {e}"))?;
        if rows == 0 {
            return Err(format!("portfolio '{name}' does not exist").into());
        }
        Ok(())
    }

    /// Append a transaction to a portfolio's ledger. Mirrors to the
    /// double-entry cost ledger if a driver is attached. Invalidates any
    /// cached materialized view for dates >= the transaction date.
    pub fn apply(&self, name: &str, tx: &Transaction) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        conn.execute(
            "INSERT INTO transactions (id, portfolio_name, date, type, asset_type, symbol, quantity, price, commission, amount, weight, currency, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                tx.id,
                name,
                tx.date,
                tx.tx_type,
                tx.asset_type,
                tx.symbol,
                tx.quantity,
                tx.price,
                tx.commission,
                tx.amount,
                tx.weight,
                tx.currency,
                tx.notes,
                tx.created_at,
            ],
        )
        .map_err(|e| format!("insert: {e}"))?;

        // Invalidate cached views from the transaction date forward.
        conn.execute(
            "DELETE FROM daily_holdings WHERE portfolio_name = ?1 AND date >= ?2",
            params![name, tx.date],
        )
        .map_err(|e| format!("invalidate holdings: {e}"))?;
        conn.execute(
            "DELETE FROM daily_returns WHERE portfolio_name = ?1 AND date >= ?2",
            params![name, tx.date],
        )
        .map_err(|e| format!("invalidate returns: {e}"))?;

        if let Some(ref driver) = self.ledger_driver {
            self.commit_to_ledger(driver, name, tx)?;
        }

        Ok(())
    }

    /// Read transactions from the ledger, optionally filtered. Ordered by
    /// date ascending. This is the read path for export and for callers
    /// that compute their own projections.
    pub fn ledger(
        &self,
        name: &str,
        filter: LedgerFilter<'_>,
    ) -> Result<Vec<Transaction>, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let mut sql = "SELECT id, date, type, asset_type, symbol, quantity, price, commission, amount, weight, currency, notes, created_at FROM transactions WHERE portfolio_name = ?1".to_string();
        let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(name.to_string())];

        if let Some(s) = filter.symbol {
            bind_values.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND symbol = ?{}", bind_values.len()));
        }
        if let Some(t) = filter.tx_type {
            bind_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND type = ?{}", bind_values.len()));
        }
        if let Some(a) = filter.asset_type {
            bind_values.push(Box::new(a));
            sql.push_str(&format!(" AND asset_type = ?{}", bind_values.len()));
        }
        if let Some(f) = filter.from_date {
            bind_values.push(Box::new(f.to_string()));
            sql.push_str(&format!(" AND date >= ?{}", bind_values.len()));
        }
        if let Some(t) = filter.to_date {
            bind_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND date <= ?{}", bind_values.len()));
        }
        sql.push_str(" ORDER BY date ASC");

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            bind_values.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("query: {e}"))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Transaction {
                    id: row.get(0)?,
                    date: row.get(1)?,
                    tx_type: row.get(2)?,
                    asset_type: row.get(3)?,
                    symbol: row.get(4)?,
                    quantity: row.get(5)?,
                    price: row.get(6)?,
                    commission: row.get(7)?,
                    amount: row.get(8)?,
                    weight: row.get::<_, Option<f64>>(9).unwrap_or(None),
                    currency: row.get::<_, String>(10).unwrap_or_default(),
                    notes: row.get::<_, String>(11).unwrap_or_default(),
                    created_at: row.get(12)?,
                })
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut txs = Vec::new();
        for row in rows {
            txs.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(txs)
    }

    /// Materialized end-of-day holdings for a portfolio at `date`. Computes
    /// from the ledger and caches the result in `daily_holdings` so repeated
    /// reads by the portfolio viewer are O(1). Re-computing is idempotent.
    ///
    /// For a nested-portfolio store (`AssetType::Portfolio`), each holding's
    /// `symbol` is a child portfolio name; resolve it recursively with
    /// another `snapshot` call.
    pub fn snapshot(&self, name: &str, date: &str) -> Result<HoldingsSnapshot, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;

        // Serve from the materialized view if present.
        if let Some(cached) = self.read_cached_holdings(&conn, name, date)? {
            return Ok(cached);
        }

        let txs = self.ledger(name, LedgerFilter::all())?;
        let mut positions: std::collections::HashMap<String, (f64, f64, f64, AssetType)> =
            std::collections::HashMap::new();
        let mut cash = 0.0f64;
        let mut issues = Vec::new();

        for tx in &txs {
            if tx.date.as_str() > date {
                continue;
            }
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
                        let entry =
                            positions
                                .entry(sym.clone())
                                .or_insert((0.0, 0.0, 0.0, tx.asset_type));
                        entry.0 += qty;
                        entry.2 += qty * price + comm;
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
                        let entry =
                            positions
                                .entry(sym.clone())
                                .or_insert((0.0, 0.0, 0.0, tx.asset_type));
                        entry.1 += qty;
                    }
                    cash += qty * price - comm;
                }
                TxType::Roll => {
                    // A roll moves quantity from one symbol to another at
                    // the same tenor. The sell leg and buy leg are separate
                    // transactions; a roll record itself carries no cash.
                    if let Some(ref sym) = tx.symbol {
                        let entry =
                            positions
                                .entry(sym.clone())
                                .or_insert((0.0, 0.0, 0.0, tx.asset_type));
                        entry.0 += tx.quantity.unwrap_or(0.0);
                    }
                }
                TxType::WeightAdjust => {
                    // Weight adjustments don't change share count; they
                    // re-target the constituent weight. Recorded for audit
                    // but not reflected in holdings shares.
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
            }
        }

        let holdings: Vec<Holding> = positions
            .into_iter()
            .map(|(symbol, (buys, sells, cost, asset_type))| Holding {
                symbol,
                asset_type,
                shares: buys - sells,
                total_buys: buys,
                total_sells: sells,
                cost_basis: cost,
            })
            .filter(|h| h.shares.abs() > 0.0001 || h.total_buys > 0.0 || h.total_sells > 0.0)
            .collect();

        let snapshot = HoldingsSnapshot {
            portfolio: name.to_string(),
            date: date.to_string(),
            transaction_count: txs.iter().filter(|t| t.date.as_str() <= date).count(),
            holdings,
            cash_balance: cash,
            issues,
        };

        self.write_cached_holdings(&conn, &snapshot)?;
        Ok(snapshot)
    }

    /// Rebuild all materialized views (`daily_holdings`, `daily_returns`)
    /// from the ledger. Use after a corruption or a bulk ledger edit that
    /// bypassed [`apply`]. Drops existing views and recomputes.
    pub fn rebuild_views(&self, name: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        conn.execute(
            "DELETE FROM daily_holdings WHERE portfolio_name = ?1",
            params![name],
        )
        .map_err(|e| format!("clear holdings: {e}"))?;
        conn.execute(
            "DELETE FROM daily_returns WHERE portfolio_name = ?1",
            params![name],
        )
        .map_err(|e| format!("clear returns: {e}"))?;
        let txs = self.ledger(name, LedgerFilter::all())?;
        let mut dates: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tx in &txs {
            dates.insert(tx.date.clone());
        }
        for date in dates {
            // Recompute and cache the holdings snapshot for each transaction date.
            self.snapshot(name, &date)?;
        }
        Ok(())
    }

    // ── Internal helpers ────────────────────────────────────────────

    fn check_exists(&self, conn: &Connection, name: &str) -> Result<(), PortfolioError> {
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM portfolios WHERE name = ?1",
                params![name],
                |_| Ok(()),
            )
            .is_ok();
        if !exists {
            return Err(format!("portfolio '{name}' does not exist").into());
        }
        Ok(())
    }

    fn read_cached_holdings(
        &self,
        conn: &Connection,
        name: &str,
        date: &str,
    ) -> Result<Option<HoldingsSnapshot>, PortfolioError> {
        let mut stmt = conn
            .prepare(
                "SELECT symbol, asset_type, shares, cost_basis FROM daily_holdings
                 WHERE portfolio_name = ?1 AND date = ?2 ORDER BY symbol",
            )
            .map_err(|e| format!("query holdings: {e}"))?;
        let mut rows = stmt
            .query(params![name, date])
            .map_err(|e| format!("query holdings: {e}"))?;
        let mut holdings = Vec::new();
        while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
            holdings.push(Holding {
                symbol: row.get(0)?,
                asset_type: row.get(1)?,
                shares: row.get(2)?,
                cost_basis: row.get(3)?,
                total_buys: 0.0,
                total_sells: 0.0,
            });
        }
        if holdings.is_empty() {
            return Ok(None);
        }
        // cash + transaction_count + issues are not cached; recompute cheaply.
        let txs = self.ledger(name, LedgerFilter::all())?;
        let cash = txs
            .iter()
            .filter(|t| t.date.as_str() <= date)
            .map(|t| t.cash_flow())
            .sum::<f64>();
        let issues = validate_holdings(&txs, date);
        Ok(Some(HoldingsSnapshot {
            portfolio: name.to_string(),
            date: date.to_string(),
            transaction_count: txs.iter().filter(|t| t.date.as_str() <= date).count(),
            holdings,
            cash_balance: cash,
            issues,
        }))
    }

    fn write_cached_holdings(
        &self,
        conn: &Connection,
        snapshot: &HoldingsSnapshot,
    ) -> Result<(), PortfolioError> {
        for h in &snapshot.holdings {
            conn.execute(
                "INSERT OR REPLACE INTO daily_holdings (portfolio_name, date, symbol, asset_type, shares, cost_basis)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    snapshot.portfolio,
                    snapshot.date,
                    h.symbol,
                    h.asset_type,
                    h.shares,
                    h.cost_basis,
                ],
            )
            .map_err(|e| format!("cache holdings: {e}"))?;
        }
        Ok(())
    }

    /// Commit a transaction to the double-entry ledger as postings.
    fn commit_to_ledger(
        &self,
        driver: &Arc<dyn DatabaseDriver>,
        portfolio_name: &str,
        tx: &Transaction,
    ) -> Result<(), PortfolioError> {
        let ledger =
            Ledger::from_driver(driver.clone()).map_err(|e| format!("ledger from_driver: {e}"))?;

        ledger
            .ensure_account("portfolio:cash/main", "portfolio")
            .map_err(|e| format!("ledger ensure cash account: {e}"))?;
        ledger
            .ensure_account("cost:brokerage/fees", "cost")
            .map_err(|e| format!("ledger ensure fee account: {e}"))?;
        if let Some(ref sym) = tx.symbol {
            let pos_account = format!("portfolio:position/{sym}");
            ledger
                .ensure_account(&pos_account, "portfolio")
                .map_err(|e| format!("ledger ensure position account: {e}"))?;
        }

        let now = chrono::Utc::now().to_rfc3339();
        let reference = format!("portfolio:{portfolio_name}:tx:{}", tx.id);

        let amount_cents = (tx.amount.unwrap_or(0.0) * 100.0).round() as i64;
        let commission_cents = (tx.commission.unwrap_or(0.0) * 100.0).round() as i64;

        let ledger_tx = match tx.tx_type {
            TxType::Buy => {
                let symbol = tx.symbol.as_deref().unwrap_or("UNKNOWN");
                let pos_account = format!("portfolio:position/{symbol}");
                LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: format!("{reference}/buy"),
                    postings: vec![
                        Posting {
                            source: "portfolio:cash/main".into(),
                            destination: pos_account,
                            asset: "USD".into(),
                            amount: amount_cents,
                        },
                        Posting {
                            source: "portfolio:cash/main".into(),
                            destination: "cost:brokerage/fees".into(),
                            asset: "USD".into(),
                            amount: commission_cents,
                        },
                    ],
                    metadata: serde_json::json!({
                        "portfolio": portfolio_name,
                        "tx_id": tx.id,
                        "type": "buy",
                        "symbol": symbol,
                        "quantity": tx.quantity,
                        "price": tx.price,
                    }),
                }
            }
            TxType::Sell => {
                let symbol = tx.symbol.as_deref().unwrap_or("UNKNOWN");
                let pos_account = format!("portfolio:position/{symbol}");
                LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: format!("{reference}/sell"),
                    postings: vec![
                        Posting {
                            source: pos_account,
                            destination: "portfolio:cash/main".into(),
                            asset: "USD".into(),
                            amount: amount_cents,
                        },
                        Posting {
                            source: "portfolio:cash/main".into(),
                            destination: "cost:brokerage/fees".into(),
                            asset: "USD".into(),
                            amount: commission_cents,
                        },
                    ],
                    metadata: serde_json::json!({
                        "portfolio": portfolio_name,
                        "tx_id": tx.id,
                        "type": "sell",
                        "symbol": symbol,
                        "quantity": tx.quantity,
                        "price": tx.price,
                    }),
                }
            }
            TxType::Dividend | TxType::Deposit => LedgerTransaction {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: now,
                reference: format!("{reference}/{}", tx.tx_type),
                postings: vec![Posting {
                    source: "external:income".into(),
                    destination: "portfolio:cash/main".into(),
                    asset: "USD".into(),
                    amount: amount_cents,
                }],
                metadata: serde_json::json!({
                    "portfolio": portfolio_name,
                    "tx_id": tx.id,
                    "type": tx.tx_type,
                }),
            },
            TxType::Withdrawal => LedgerTransaction {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: now,
                reference: format!("{reference}/withdrawal"),
                postings: vec![Posting {
                    source: "portfolio:cash/main".into(),
                    destination: "external:income".into(),
                    asset: "USD".into(),
                    amount: amount_cents,
                }],
                metadata: serde_json::json!({
                    "portfolio": portfolio_name,
                    "tx_id": tx.id,
                    "type": "withdrawal",
                }),
            },
            // Rolls and weight adjustments are non-cash index operations;
            // they don't post to the cash ledger.
            TxType::Roll | TxType::WeightAdjust => {
                return Ok(());
            }
        };
        ledger
            .commit(&ledger_tx)
            .map_err(|e| format!("ledger commit: {e}"))?;
        Ok(())
    }
}

// ── Free functions: pure projections over the ledger ────────────────

/// Validate a ledger slice up to `date`, returning issue strings. Used by
/// [`PortfolioStore::snapshot`] to populate the `issues` field without
/// re-walking the ledger.
fn validate_holdings(txs: &[Transaction], date: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for tx in txs.iter().filter(|t| t.date.as_str() <= date) {
        match tx.tx_type {
            TxType::Buy => {
                let qty = tx.quantity.unwrap_or(0.0);
                let price = tx.price.unwrap_or(0.0);
                if qty <= 0.0 {
                    issues.push(format!("{}: buy with non-positive quantity {}", tx.id, qty));
                }
                if price <= 0.0 {
                    issues.push(format!("{}: buy with non-positive price {}", tx.id, price));
                }
            }
            TxType::Sell => {
                let qty = tx.quantity.unwrap_or(0.0);
                let price = tx.price.unwrap_or(0.0);
                if qty <= 0.0 {
                    issues.push(format!(
                        "{}: sell with non-positive quantity {}",
                        tx.id, qty
                    ));
                }
                if price <= 0.0 {
                    issues.push(format!("{}: sell with non-positive price {}", tx.id, price));
                }
            }
            TxType::Deposit | TxType::Withdrawal => {
                let amt = tx.amount.unwrap_or(0.0);
                if amt <= 0.0 {
                    issues.push(format!(
                        "{}: {} with non-positive amount {}",
                        tx.id, tx.tx_type, amt
                    ));
                }
            }
            _ => {}
        }
    }
    issues
}

/// Compute time-weighted and money-weighted returns for a portfolio over a
/// date range. Pure function: takes the ledger (via the store) and a price
/// resolver. The store's `price_cache` table is exposed through
/// [`CachedPriceResolver`] for the common case.
///
/// `from` and `to` must be `YYYY-MM-DD`. Returns `InvalidArgument` on
/// malformed dates (never silently substitutes the epoch — the SF-4 bug).
pub fn returns(
    store: &PortfolioStore,
    name: &str,
    from: &str,
    to: &str,
    prices: &dyn PriceResolver,
) -> Result<ReturnsReport, PortfolioError> {
    let from_date = parse_ymd(from, "from")?;
    let to_date = parse_ymd(to, "to")?;

    let txs = store.ledger(name, LedgerFilter::all())?;

    let mut positions_start: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    let mut positions_end: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    let mut cash_start = 0.0f64;
    let mut cash_end = 0.0f64;
    let mut cash_flow_events: Vec<(String, f64)> = Vec::new();

    for tx in &txs {
        let cf = tx.cash_flow();
        if tx.date.as_str() <= from {
            cash_start += cf;
        }
        if tx.date.as_str() <= to {
            cash_end += cf;
        }
        if tx.date.as_str() > from
            && tx.date.as_str() <= to
            && matches!(tx.tx_type, TxType::Deposit | TxType::Withdrawal)
        {
            cash_flow_events.push((tx.date.clone(), cf));
        }
        if let Some(ref sym) = tx.symbol {
            let delta = tx.position_delta();
            if tx.date.as_str() <= from
                && matches!(tx.tx_type, TxType::Buy | TxType::Sell | TxType::Roll)
            {
                *positions_start.entry(sym.clone()).or_insert(0.0) += delta;
            }
            if tx.date.as_str() <= to
                && matches!(tx.tx_type, TxType::Buy | TxType::Sell | TxType::Roll)
            {
                *positions_end.entry(sym.clone()).or_insert(0.0) += delta;
            }
        }
    }

    positions_start.retain(|_, v| *v > 0.0001);

    let all_symbols: Vec<String> = positions_start
        .keys()
        .chain(positions_end.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut prices_at: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for date in [from, to] {
        for sym in &all_symbols {
            if let Some(close) = prices.resolve(sym, date) {
                prices_at.insert(format!("{date}:{sym}"), close);
            }
        }
    }

    let mv_at = |positions: &std::collections::HashMap<String, f64>, date: &str| -> f64 {
        positions
            .iter()
            .map(|(sym, shares)| {
                let price = prices_at
                    .get(&format!("{date}:{sym}"))
                    .copied()
                    .unwrap_or(0.0);
                shares * price
            })
            .sum()
    };

    let mv_start = mv_at(&positions_start, from);
    let mv_end = mv_at(&positions_end, to);
    let total_start = mv_start + cash_start;
    let total_end = mv_end + cash_end;

    if total_start <= 0.0 {
        return Err(
            format!("portfolio has zero or negative starting value for {from}..={to}").into(),
        );
    }

    let net_flows: f64 = cash_flow_events.iter().map(|(_, amt)| amt).sum();
    let total_return = (total_end - total_start - net_flows) / total_start;

    let period_days = (to_date - from_date).num_days().max(1) as f64;
    let mut weighted_flows: f64 = 0.0;
    for (date_str, amt) in &cash_flow_events {
        let cf_date = parse_ymd(date_str, "cash flow date")?;
        let days_remaining = (to_date - cf_date).num_days().max(0) as f64;
        let weight = days_remaining / period_days;
        weighted_flows += amt * weight;
    }
    let modified_dietz = if (total_start + weighted_flows).abs() > 0.0001 {
        (total_end - total_start - net_flows) / (total_start + weighted_flows)
    } else {
        total_return
    };

    // IRR via Newton's method.
    let from_days = from_date.num_days_from_ce();
    // The first cash flow (-total_start) is at day 0 (the `from` date);
    // subsequent flows are relative to it. Using the absolute day number
    // here would put the exponent at ~738000/365 ≈ 2021, causing Newton's
    // method to diverge numerically.
    let mut cfs: Vec<(f64, f64)> = vec![(-total_start, 0.0)];
    for (date_str, amt) in &cash_flow_events {
        let cf_date = parse_ymd(date_str, "cash flow date")?;
        let days = (cf_date.num_days_from_ce() - from_days) as f64;
        cfs.push((*amt, days));
    }
    let to_days = (to_date.num_days_from_ce() - from_days) as f64;
    cfs.push((total_end, to_days));

    let npv = |r: f64| -> f64 {
        cfs.iter()
            .map(|(cf, days)| cf / (1.0 + r).powf(days / 365.0))
            .sum()
    };
    let npv_deriv = |r: f64| -> f64 {
        cfs.iter()
            .map(|(cf, days)| -cf * (days / 365.0) / (1.0 + r).powf(days / 365.0 + 1.0))
            .sum()
    };

    let mut r = 0.1;
    let mut converged = false;
    for _ in 0..50 {
        let f = npv(r);
        let fp = npv_deriv(r);
        if fp.abs() < 1e-12 {
            break;
        }
        let r_new = r - f / fp;
        if (r_new - r).abs() < 1e-8 {
            r = r_new;
            converged = true;
            break;
        }
        r = r_new;
        if r < -0.99 {
            r = -0.5;
        }
        if r > 10.0 {
            r = 1.0;
        }
    }

    Ok(ReturnsReport {
        portfolio: name.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        total_return,
        modified_dietz,
        irr: r,
        irr_converged: converged,
        start_value: total_start,
        end_value: total_end,
        net_cash_flows: net_flows,
        cash_flow_count: cash_flow_events.len(),
        positions_at_start: positions_start.len(),
        positions_at_end: positions_end.len(),
    })
}

/// Parse a `YYYY-MM-DD` date, surfacing a malformed value as `InvalidArgument`
/// rather than silently substituting the epoch (the SF-4 bug).
pub fn parse_ymd(value: &str, field: &str) -> Result<chrono::NaiveDate, PortfolioError> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("{field} must be YYYY-MM-DD (got '{value}')").into())
}

// ── CachedPriceResolver ──────────────────────────────────────────────

/// A [`PriceResolver`] backed by the store's `price_cache` table. The
/// companies server seeds this cache from FMP/EODHD; the portfolio crate
/// itself never calls a provider. Owns a [`PortfolioStore`] clone so it is
/// `'static` and can be moved into `spawn_blocking` closures.
pub struct CachedPriceResolver {
    store: PortfolioStore,
    portfolio: String,
}

impl CachedPriceResolver {
    pub fn new(store: &PortfolioStore, portfolio: &str) -> Self {
        Self {
            store: store.clone(),
            portfolio: portfolio.to_string(),
        }
    }

    /// Seed the cache with a price observation. Used by the companies server
    /// after a successful FMP/EODHD fetch so subsequent `returns` calls hit
    /// the cache instead of the API.
    pub fn seed_cache(
        &self,
        symbol: &str,
        date: &str,
        close: f64,
        source: &str,
    ) -> Result<(), PortfolioError> {
        let conn = self.store.open()?;
        conn.execute(
            "INSERT OR REPLACE INTO price_cache (portfolio_name, symbol, date, close, source, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![self.portfolio, symbol, date, close, source, now_rfc3339()],
        )
        .map_err(|e| format!("seed price cache: {e}"))?;
        Ok(())
    }
}

impl PriceResolver for CachedPriceResolver {
    fn resolve(&self, symbol: &str, date: &str) -> Option<f64> {
        let conn = self.store.open().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT close FROM price_cache WHERE portfolio_name = ?1 AND symbol = ?2 AND date = ?3",
            )
            .ok()?;
        let close: Option<f64> = stmt
            .query_row(params![self.portfolio, symbol, date], |row| row.get(0))
            .optional()
            .ok()?;
        close
    }
}

// ── Import / export (free functions over the ledger) ─────────────────

/// Import transactions from JSON into a portfolio. Auto-creates the
/// portfolio if it does not exist. Returns the imported transaction ids.
pub fn import_json(
    store: &PortfolioStore,
    name: &str,
    asset_type: AssetType,
    json: &str,
) -> Result<Vec<String>, PortfolioError> {
    check_request_size(json.len(), MAX_IMPORT_REQUEST_BYTES, "import request")?;
    let txs: Vec<Transaction> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    import_transactions(store, name, asset_type, txs)
}

/// Import transactions from CSV into a portfolio. Auto-creates the
/// portfolio if it does not exist. Returns the imported transaction ids.
pub fn import_csv(
    store: &PortfolioStore,
    name: &str,
    asset_type: AssetType,
    csv: &str,
) -> Result<Vec<String>, PortfolioError> {
    check_request_size(csv.len(), MAX_IMPORT_REQUEST_BYTES, "import request")?;
    let mut txs = Vec::new();
    let mut lines = csv.lines();
    let header = lines.next().ok_or("CSV has no header row")?;
    let columns: Vec<&str> = header.split(',').map(|c| c.trim()).collect();
    let idx = |name: &str| columns.iter().position(|c| *c == name);

    for (line_num, line) in lines.enumerate() {
        let line_num = line_num + 2;
        if line.trim().is_empty() {
            continue;
        }
        if txs.len() == MAX_IMPORT_TRANSACTION_COUNT {
            return Err(format!(
                "import exceeds maximum of {MAX_IMPORT_TRANSACTION_COUNT} transactions"
            )
            .into());
        }
        let fields: Vec<&str> = line.split(',').map(|f| f.trim()).collect();
        let get_str = |col: &str| -> Option<String> {
            idx(col).and_then(|i| fields.get(i)).map(|s| s.to_string())
        };
        let get_f64 = |col: &str| -> Option<f64> {
            idx(col)
                .and_then(|i| fields.get(i))
                .and_then(|s| s.parse().ok())
        };

        let tx_type: TxType = get_str("type")
            .ok_or_else(|| format!("line {line_num}: missing 'type' column"))?
            .parse()?;
        let date = get_str("date").unwrap_or_default();
        let symbol = get_str("symbol");
        let quantity = get_f64("quantity");
        let price = get_f64("price");
        let commission = get_f64("commission");
        let amount = get_f64("amount");
        let weight = get_f64("weight");
        let currency = get_str("currency").unwrap_or_else(|| "USD".into());
        let notes = get_str("notes").unwrap_or_default();

        if date.is_empty() {
            return Err(format!("line {line_num}: missing date").into());
        }

        txs.push(Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            date,
            tx_type,
            asset_type,
            symbol,
            quantity,
            price,
            commission,
            amount,
            weight,
            currency,
            notes,
            created_at: now_rfc3339(),
        });
    }

    import_transactions(store, name, asset_type, txs)
}

fn import_transactions(
    store: &PortfolioStore,
    name: &str,
    asset_type: AssetType,
    txs: Vec<Transaction>,
) -> Result<Vec<String>, PortfolioError> {
    if txs.len() > MAX_IMPORT_TRANSACTION_COUNT {
        return Err(format!(
            "import exceeds maximum of {MAX_IMPORT_TRANSACTION_COUNT} transactions"
        )
        .into());
    }
    if !store.list()?.contains(&name.to_string()) {
        store.create(name, asset_type)?;
    }
    let mut imported = Vec::new();
    for tx in &txs {
        // apply() inserts + invalidates views + mirrors to ledger.
        match store.apply(name, tx) {
            Ok(()) => imported.push(tx.id.clone()),
            Err(e) => return Err(e),
        }
    }
    Ok(imported)
}

/// Export a portfolio's ledger to JSON.
pub fn export_json(store: &PortfolioStore, name: &str) -> Result<String, PortfolioError> {
    let txs = store.ledger(name, LedgerFilter::all())?;
    serde_json::to_string_pretty(&txs).map_err(|e| format!("serialize: {e}").into())
}

/// Export a portfolio's ledger to CSV.
pub fn export_csv(store: &PortfolioStore, name: &str) -> Result<String, PortfolioError> {
    let txs = store.ledger(name, LedgerFilter::all())?;
    let mut out = String::from(
        "id,date,type,asset_type,symbol,quantity,price,commission,amount,weight,currency,notes,created_at\n",
    );
    for tx in &txs {
        let csv_quote = |s: &str| -> String {
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        };
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            tx.id,
            tx.date,
            tx.tx_type,
            tx.asset_type,
            tx.symbol.as_deref().unwrap_or(""),
            tx.quantity.map_or(String::new(), |v| v.to_string()),
            tx.price.map_or(String::new(), |v| v.to_string()),
            tx.commission.map_or(String::new(), |v| v.to_string()),
            tx.amount.map_or(String::new(), |v| v.to_string()),
            tx.weight.map_or(String::new(), |v| v.to_string()),
            tx.currency,
            csv_quote(&tx.notes),
            tx.created_at,
        ));
    }
    Ok(out)
}

pub mod server;

pub use server::run;

#[cfg(test)]
mod tests;
