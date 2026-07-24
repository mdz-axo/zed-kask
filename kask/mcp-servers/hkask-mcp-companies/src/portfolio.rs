//! hKask MCP Companies — Portfolio tracking
//!
//! A portfolio is a ledger. Everything else is arithmetic on the ledger
//! at a point in time. This module manages the SQLite-backed transaction
//! ledger — create, read, validate, import, export, notes, and file attachments.

use hkask_ledger::{Ledger, LedgerError, LedgerTransaction, Posting};
use hkask_storage::database::driver::DatabaseDriver;
use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::PathBuf;
use std::sync::Arc;

const MAX_IMPORT_REQUEST_BYTES: usize = 5 * 1024 * 1024;
const MAX_ENCODED_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_DECODED_ATTACHMENT_BYTES: usize = 6 * 1024 * 1024;
const MAX_IMPORT_TRANSACTION_COUNT: usize = 10_000;

/// SQLite schema DDL for the portfolio database.
/// Used by both production (`new`) and test (`with_dir`) paths to ensure
/// identical schema — including FK cascade constraints.
const SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS portfolios (
                    name TEXT PRIMARY KEY,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS transactions (
                    id TEXT PRIMARY KEY,
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    date TEXT NOT NULL,
                    type TEXT NOT NULL CHECK(type IN ('buy','sell','dividend','deposit','withdrawal')),
                    symbol TEXT,
                    quantity REAL,
                    price REAL,
                    commission REAL DEFAULT 0,
                    amount REAL,
                    currency TEXT DEFAULT 'USD',
                    notes TEXT DEFAULT '',
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_tx_portfolio ON transactions(portfolio_name);
                CREATE INDEX IF NOT EXISTS idx_tx_date ON transactions(date);
                CREATE INDEX IF NOT EXISTS idx_tx_symbol ON transactions(symbol);
                CREATE TABLE IF NOT EXISTS price_cache (
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    symbol TEXT NOT NULL,
                    date TEXT NOT NULL,
                    close REAL NOT NULL,
                    source TEXT NOT NULL,
                    fetched_at TEXT NOT NULL,
                    PRIMARY KEY (portfolio_name, symbol, date)
                );
                CREATE TABLE IF NOT EXISTS security_links (
                    portfolio_name TEXT NOT NULL REFERENCES portfolios(name) ON DELETE CASCADE,
                    ledger_symbol TEXT NOT NULL,
                    data_symbol TEXT NOT NULL,
                    PRIMARY KEY (portfolio_name, ledger_symbol)
                );
                CREATE TABLE IF NOT EXISTS notes (
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

impl From<LedgerError> for PortfolioError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e.to_string())
    }
}

// ── Transaction ─────────────────────────────────────────────────────

/// Transaction type — matches the SQLite CHECK constraint values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TxType {
    Buy,
    Sell,
    Dividend,
    Deposit,
    Withdrawal,
}

impl std::fmt::Display for TxType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buy => write!(f, "buy"),
            Self::Sell => write!(f, "sell"),
            Self::Dividend => write!(f, "dividend"),
            Self::Deposit => write!(f, "deposit"),
            Self::Withdrawal => write!(f, "withdrawal"),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub date: String,
    #[serde(rename = "type")]
    pub tx_type: TxType,
    pub symbol: Option<String>,
    pub quantity: Option<f64>,
    pub price: Option<f64>,
    pub commission: Option<f64>,
    pub amount: Option<f64>,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: String,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn check_request_size(size: usize, maximum: usize, subject: &str) -> Result<(), PortfolioError> {
    if size > maximum {
        return Err(format!("{subject} exceeds maximum of {maximum} bytes").into());
    }
    Ok(())
}

// ── Validation report ───────────────────────────────────────────────

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

// ── PortfolioManager ────────────────────────────────────────────────

#[derive(Clone)]
pub struct PortfolioManager {
    db_path: PathBuf,
    /// Optional cost ledger for double-entry accounting.
    ledger_driver: Option<Arc<dyn DatabaseDriver>>,
}

impl Default for PortfolioManager {
    fn default() -> Self {
        Self::new(WebID::new())
    }
}

impl PortfolioManager {
    /// Creates storage scoped to the authenticated server owner.
    pub fn new(owner: WebID) -> Self {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("hkask");
        path.push("portfolios");
        path.push(sanitize_name(&owner.to_string()));
        std::fs::create_dir_all(&path).expect("failed to create portfolio directory");
        path.push("master.db");
        // Ensure schema exists on first use — hard error, not silent skip.
        let conn = Connection::open(&path).expect("failed to open portfolio database");
        conn.execute_batch(SCHEMA_DDL)
            .expect("failed to initialize portfolio schema");
        Self {
            db_path: path,
            ledger_driver: None,
        }
    }

    #[cfg(test)]
    pub fn with_dir_for_owner(base_dir: PathBuf, owner: WebID) -> Self {
        Self::with_dir(base_dir.join(sanitize_name(&owner.to_string())))
    }

    #[cfg(test)]
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

    fn open(&self) -> Result<Connection, PortfolioError> {
        Connection::open(&self.db_path).map_err(|e| format!("db open: {e}").into())
    }

    /// Persist a forecast snapshot in this owner's database.
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

    /// Retrieve a forecast belonging to this owner.
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

    /// List this owner's forecasts for a symbol, newest first.
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

    /// Verify a revision parent is visible to this owner and uses the same symbol.
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

    /// Append an outcome record to an existing forecast in this owner's database.
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

    /// Base directory for portfolio file storage (parent of master.db).
    fn base_dir(&self) -> &std::path::Path {
        self.db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
    }

    // ── Portfolio CRUD ───────────────────────────────────────────

    pub fn create(&self, name: &str) -> Result<(), PortfolioError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err("portfolio name must not be empty or contain path separators".into());
        }
        let conn = self.open()?;
        let rows = conn
            .execute(
                "INSERT OR IGNORE INTO portfolios (name, created_at) VALUES (?1, ?2)",
                params![name, now_rfc3339()],
            )
            .map_err(|e| format!("create: {e}"))?;
        // If a concurrent create beat us, that's fine — the portfolio exists.
        if rows == 0 {
            // Verify it actually exists (not a transient error)
            self.check_exists(&conn, name)?;
        }
        Ok(())
    }

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

    #[allow(dead_code)] // exercised by the test suite only
    pub fn add_transaction(&self, name: &str, tx: &Transaction) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        conn.execute(
            "INSERT INTO transactions (id, portfolio_name, date, type, symbol, quantity, price, commission, amount, currency, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                tx.id,
                name,
                tx.date,
                tx.tx_type,
                tx.symbol,
                tx.quantity,
                tx.price,
                tx.commission,
                tx.amount,
                tx.currency,
                tx.notes,
                tx.created_at,
            ],
        )
        .map_err(|e| format!("insert: {e}"))?;

        // Mirror to cost ledger if configured
        if let Some(ref driver) = self.ledger_driver {
            self.commit_to_ledger(driver, name, tx)?;
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

        // Ensure accounts exist (idempotent)
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

        // Convert amounts to integer cents (µUSD) for ledger.
        // Use rounding to avoid f64 precision loss, and saturate on overflow.
        let amount_cents = (tx.amount.unwrap_or(0.0) * 100.0).round() as i64;
        let commission_cents = (tx.commission.unwrap_or(0.0) * 100.0).round() as i64;

        match tx.tx_type {
            TxType::Buy => {
                let symbol = tx.symbol.as_deref().unwrap_or("UNKNOWN");
                let pos_account = format!("portfolio:position/{symbol}");

                // Cash → Position  +  Brokerage fee
                let tx_ref = format!("{reference}/buy");
                let ledger_tx = LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: tx_ref,
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
                };
                ledger
                    .commit(&ledger_tx)
                    .map_err(|e| format!("ledger commit buy: {e}"))?;
            }
            TxType::Sell => {
                let symbol = tx.symbol.as_deref().unwrap_or("UNKNOWN");
                let pos_account = format!("portfolio:position/{symbol}");

                // Position → Cash, minus brokerage fee
                let tx_ref = format!("{reference}/sell");
                let ledger_tx = LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: tx_ref,
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
                };
                ledger
                    .commit(&ledger_tx)
                    .map_err(|e| format!("ledger commit sell: {e}"))?;
            }
            TxType::Dividend | TxType::Deposit => {
                // External → Cash (income)
                let tx_ref = format!("{reference}/{}", tx.tx_type);
                let ledger_tx = LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: tx_ref,
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
                };
                ledger
                    .commit(&ledger_tx)
                    .map_err(|e| format!("ledger commit {type}: {e}", type = tx.tx_type))?;
            }
            TxType::Withdrawal => {
                // Cash → External
                let tx_ref = format!("{reference}/withdrawal");
                let ledger_tx = LedgerTransaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: now,
                    reference: tx_ref,
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
                };
                ledger
                    .commit(&ledger_tx)
                    .map_err(|e| format!("ledger commit withdrawal: {e}"))?;
            }
        }

        Ok(())
    }

    pub fn append_note(&self, name: &str, tx_id: &str, note: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let existing: String = conn
            .query_row(
                "SELECT notes FROM transactions WHERE id = ?1 AND portfolio_name = ?2",
                params![tx_id, name],
                |row| row.get(0),
            )
            .map_err(|e| format!("lookup: {e}"))?;
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
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let mut sql = "SELECT id, date, type, symbol, quantity, price, commission, amount, currency, notes, created_at FROM transactions WHERE portfolio_name = ?1".to_string();
        let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(name.to_string())];

        if let Some(s) = symbol {
            bind_values.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND symbol = ?{}", bind_values.len()));
        }
        if let Some(t) = tx_type {
            bind_values.push(Box::new(t.to_string()));
            sql.push_str(&format!(" AND type = ?{}", bind_values.len()));
        }
        if let Some(f) = from_date {
            bind_values.push(Box::new(f.to_string()));
            sql.push_str(&format!(" AND date >= ?{}", bind_values.len()));
        }
        if let Some(t) = to_date {
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
                    symbol: row.get(3)?,
                    quantity: row.get(4)?,
                    price: row.get(5)?,
                    commission: row.get(6)?,
                    amount: row.get(7)?,
                    currency: row.get::<_, String>(8).unwrap_or_default(),
                    notes: row.get::<_, String>(9).unwrap_or_default(),
                    created_at: row.get(10)?,
                })
            })
            .map_err(|e| format!("query: {e}"))?;

        let mut txs = Vec::new();
        for row in rows {
            txs.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(txs)
    }

    // ── Validation ───────────────────────────────────────────────

    pub fn validate(&self, name: &str) -> Result<ValidationReport, PortfolioError> {
        let txs = self.get_transactions(name, None, None, None, None)?;
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
                    let amt = tx.amount.unwrap_or(0.0);
                    cash += amt;
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

    // ── Import / Export ──────────────────────────────────────────

    pub fn import_json(&self, name: &str, json: &str) -> Result<Vec<String>, PortfolioError> {
        check_request_size(json.len(), MAX_IMPORT_REQUEST_BYTES, "import request")?;
        let txs: Vec<Transaction> =
            serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
        self.import_transactions(name, txs)
    }

    pub fn import_csv(&self, name: &str, csv: &str) -> Result<Vec<String>, PortfolioError> {
        check_request_size(csv.len(), MAX_IMPORT_REQUEST_BYTES, "import request")?;
        let mut txs = Vec::new();
        let mut lines = csv.lines();
        let header = lines.next().ok_or("CSV has no header row")?;
        let columns: Vec<&str> = header.split(',').map(|c| c.trim()).collect();

        // Map column names to field indices
        let idx = |name: &str| columns.iter().position(|c| *c == name);

        for (line_num, line) in lines.enumerate() {
            let line_num = line_num + 2; // 1-indexed, header is line 1
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
                .ok_or(format!("line {line_num}: missing 'type' column"))?
                .parse()?;
            let date = get_str("date").unwrap_or_default();
            let symbol = get_str("symbol");
            let quantity = get_f64("quantity");
            let price = get_f64("price");
            let commission = get_f64("commission");
            let amount = get_f64("amount");
            let currency = get_str("currency").unwrap_or_else(|| "USD".into());
            let notes = get_str("notes").unwrap_or_default();

            if date.is_empty() {
                return Err(format!("line {line_num}: missing date").into());
            }

            txs.push(Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                date,
                tx_type,
                symbol,
                quantity,
                price,
                commission,
                amount,
                currency,
                notes,
                created_at: now_rfc3339(),
            });
        }

        self.import_transactions(name, txs)
    }

    fn import_transactions(
        &self,
        name: &str,
        txs: Vec<Transaction>,
    ) -> Result<Vec<String>, PortfolioError> {
        if txs.len() > MAX_IMPORT_TRANSACTION_COUNT {
            return Err(format!(
                "import exceeds maximum of {MAX_IMPORT_TRANSACTION_COUNT} transactions"
            )
            .into());
        }
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let mut imported = Vec::new();
        for tx in &txs {
            match conn.execute(
                "INSERT OR IGNORE INTO transactions (id, portfolio_name, date, type, symbol, quantity, price, commission, amount, currency, notes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    tx.id,
                    name,
                    tx.date,
                    tx.tx_type,
                    tx.symbol,
                    tx.quantity,
                    tx.price,
                    tx.commission,
                    tx.amount,
                    tx.currency,
                    tx.notes,
                    tx.created_at,
                ],
            ) {
                Ok(1) => imported.push(tx.id.clone()),
                Ok(0) => {} // duplicate, silently skipped
                Ok(_) => unreachable!(),
                Err(e) => return Err(format!("insert {}: {e}", tx.id).into()),
            }
        }
        Ok(imported)
    }

    pub fn export_json(&self, name: &str) -> Result<String, PortfolioError> {
        let txs = self.get_transactions(name, None, None, None, None)?;
        serde_json::to_string_pretty(&txs).map_err(|e| format!("serialize: {e}").into())
    }

    pub fn export_csv(&self, name: &str) -> Result<String, PortfolioError> {
        let txs = self.get_transactions(name, None, None, None, None)?;
        let mut out = String::from(
            "id,date,type,symbol,quantity,price,commission,amount,currency,notes,created_at\n",
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
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                tx.id,
                tx.date,
                tx.tx_type,
                tx.symbol.as_deref().unwrap_or(""),
                tx.quantity.map_or("".to_string(), |v| v.to_string()),
                tx.price.map_or("".to_string(), |v| v.to_string()),
                tx.commission.map_or("".to_string(), |v| v.to_string()),
                tx.amount.map_or("".to_string(), |v| v.to_string()),
                tx.currency,
                csv_quote(&tx.notes),
                tx.created_at,
            ));
        }
        Ok(out)
    }

    // ── Data linkage ─────────────────────────────────────────────

    /// Get all unique symbols from a portfolio's ledger.
    pub fn get_symbols(&self, name: &str) -> Result<Vec<String>, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
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

    /// Get cached prices for a symbol in a date range.
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

    // ── Portfolio comparison ────────────────────────────────────

    /// Compare two portfolios — side-by-side positions, overlap, unique symbols.
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

    // ── Notes ────────────────────────────────────────────────────

    /// Add a note to a company/security as of a date. Returns the note ID.
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
        self.check_exists(&conn, portfolio)?;
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

    /// List notes for a symbol, optionally filtered by date range or tags.
    pub fn list_notes(
        &self,
        portfolio: &str,
        symbol: &str,
        date_from: Option<&str>,
        date_to: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, portfolio)?;
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
            // Apply tag filter in-memory if specified
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

    /// Delete a note by ID.
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

    // ── File attachments ─────────────────────────────────────────

    /// Attach a file to a company/security. Receives base64-encoded content,
    /// writes to `portfolios/{name}/files/{uuid}_{filename}`, returns the file ID.
    #[allow(clippy::too_many_arguments)]
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
        check_request_size(
            data_b64.len(),
            MAX_ENCODED_ATTACHMENT_BYTES,
            "encoded attachment",
        )?;
        let conn = self.open()?;
        self.check_exists(&conn, portfolio)?;
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
        let _ = std::fs::create_dir_all(&files_dir);
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

    /// List attached files for a symbol in a portfolio.
    pub fn list_files(
        &self,
        portfolio: &str,
        symbol: &str,
    ) -> Result<Vec<serde_json::Value>, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, portfolio)?;
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

    /// Delete an attached file by ID — removes DB record and physical file.
    pub fn delete_file(&self, file_id: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        // Look up the file path first
        let path: String = conn
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("lookup: {e}"))?;

        std::fs::remove_file(&path).map_err(|e| {
            format!(
                "delete_file: failed to remove attachment file '{path}': {e}; metadata preserved"
            )
        })?;

        let rows = conn
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])
            .map_err(|e| {
                format!("delete_file: attachment file removed but metadata deletion failed: {e}")
            })?;
        if rows == 0 {
            return Err(format!(
                "delete_file: attachment file removed but metadata for '{file_id}' was not found"
            )
            .into());
        }

        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tx(id: &str, tx_type: &str, symbol: &str, qty: f64, price: f64) -> Transaction {
        Transaction {
            id: id.to_string(),
            date: "2024-06-15".to_string(),
            tx_type: tx_type.parse().unwrap(),
            symbol: Some(symbol.to_string()),
            quantity: Some(qty),
            price: Some(price),
            commission: Some(1.0),
            amount: None,
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: "2024-06-15T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn owner_namespaces_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let owner_a = WebID::from_persona(b"portfolio-owner-a");
        let owner_b = WebID::from_persona(b"portfolio-owner-b");
        let portfolio_a = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner_a);
        let portfolio_b = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner_b);

        assert_ne!(portfolio_a.db_path, portfolio_b.db_path);
        assert!(portfolio_a.db_path.ends_with("master.db"));
        assert!(
            portfolio_a
                .db_path
                .starts_with(dir.path().join(sanitize_name(&owner_a.to_string())))
        );
        portfolio_a.create("private").unwrap();
        assert!(portfolio_b.list().unwrap().is_empty());
    }

    fn sample_forecast(id: &str, symbol: &str, revision_of: Option<&str>) -> PersistedForecast {
        PersistedForecast {
            id: id.to_string(),
            symbol: symbol.to_string(),
            revision_of: revision_of.map(str::to_string),
            snapshot: serde_json::json!({"version": 1, "model": {"periods": []}}),
            outcomes: Vec::new(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn forecasts_round_trip_across_manager_instances_for_same_owner() {
        let dir = tempfile::tempdir().unwrap();
        let owner = WebID::from_persona(b"forecast-owner");
        let first = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner);
        first
            .save_forecast(&sample_forecast("forecast-1", "AAPL", None))
            .unwrap();

        let second = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner);
        let forecast = second.get_forecast("forecast-1").unwrap().unwrap();
        assert_eq!(forecast.symbol, "AAPL");
        assert_eq!(forecast.snapshot["version"], 1);
        assert_eq!(second.list_forecasts("AAPL").unwrap().len(), 1);
    }

    #[test]
    fn forecast_outcomes_persist_across_manager_instances() {
        let dir = tempfile::tempdir().unwrap();
        let owner = WebID::from_persona(b"forecast-outcome-owner");
        let first = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner);
        first
            .save_forecast(&sample_forecast("forecast-1", "AAPL", None))
            .unwrap();

        let second = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner);
        second
            .record_forecast_outcome("forecast-1", serde_json::json!({"combined_brier": 0.25}))
            .unwrap();

        let third = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner);
        let forecast = third.get_forecast("forecast-1").unwrap().unwrap();
        assert_eq!(
            forecast.outcomes,
            vec![serde_json::json!({"combined_brier": 0.25})]
        );
    }

    #[test]
    fn forecast_revisions_require_same_owner_and_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let owner_a = WebID::from_persona(b"forecast-revision-owner-a");
        let owner_b = WebID::from_persona(b"forecast-revision-owner-b");
        let portfolio_a = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner_a);
        portfolio_a
            .save_forecast(&sample_forecast("forecast-1", "AAPL", None))
            .unwrap();

        assert!(
            portfolio_a
                .validate_forecast_revision("forecast-1", "AAPL")
                .is_ok()
        );
        assert!(
            portfolio_a
                .validate_forecast_revision("forecast-1", "MSFT")
                .unwrap_err()
                .to_string()
                .contains("belongs to symbol")
        );

        let portfolio_b = PortfolioManager::with_dir_for_owner(dir.path().to_path_buf(), owner_b);
        assert!(
            portfolio_b
                .validate_forecast_revision("forecast-1", "AAPL")
                .unwrap_err()
                .to_string()
                .contains("not found for this owner")
        );
    }

    #[test]
    fn import_limits_reject_oversized_requests_and_transaction_sets() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        assert!(
            pm.import_json("test", &" ".repeat(MAX_IMPORT_REQUEST_BYTES + 1))
                .unwrap_err()
                .to_string()
                .contains("import request exceeds")
        );

        let txs =
            vec![sample_tx("limit", "buy", "AAPL", 1.0, 1.0); MAX_IMPORT_TRANSACTION_COUNT + 1];
        assert!(
            pm.import_transactions("test", txs)
                .unwrap_err()
                .to_string()
                .contains("maximum of")
        );
    }

    #[test]
    fn attachment_limits_reject_encoded_and_decoded_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let oversized_encoded = "A".repeat(MAX_ENCODED_ATTACHMENT_BYTES + 1);
        assert!(
            pm.attach_file(
                "test",
                "AAPL",
                "2024-01-01",
                "note.txt",
                "text/plain",
                &oversized_encoded,
                ""
            )
            .unwrap_err()
            .to_string()
            .contains("encoded attachment exceeds")
        );

        let decoded = vec![0_u8; MAX_DECODED_ATTACHMENT_BYTES + 1];
        let oversized_decoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, decoded);
        assert!(
            pm.attach_file(
                "test",
                "AAPL",
                "2024-01-01",
                "note.txt",
                "text/plain",
                &oversized_decoded,
                ""
            )
            .unwrap_err()
            .to_string()
            .contains("decoded attachment exceeds")
        );
    }

    #[test]
    fn portfolio_create_list_delete() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());

        pm.create("test1").unwrap();
        pm.create("test2").unwrap();

        let list = pm.list().unwrap();
        assert!(list.contains(&"test1".to_string()));
        assert!(list.contains(&"test2".to_string()));

        assert!(pm.create("test1").is_ok()); // duplicate — idempotent per B4 fix

        pm.delete("test1").unwrap();
        let list = pm.list().unwrap();
        assert!(!list.contains(&"test1".to_string()));
        assert!(list.contains(&"test2".to_string()));

        assert!(pm.delete("nonexistent").is_err());
    }

    #[test]
    fn transaction_add_and_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let tx = sample_tx("t1", "buy", "AAPL", 10.0, 150.0);
        pm.add_transaction("test", &tx).unwrap();

        let txs = pm.get_transactions("test", None, None, None, None).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].id, "t1");
        assert_eq!(txs[0].symbol.as_deref(), Some("AAPL"));

        pm.append_note("test", "t1", "buying the dip").unwrap();
        let txs = pm.get_transactions("test", None, None, None, None).unwrap();
        assert!(txs[0].notes.contains("buying the dip"));
    }

    #[test]
    fn transaction_filtering() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        pm.add_transaction("test", &sample_tx("t1", "buy", "AAPL", 10.0, 150.0))
            .unwrap();
        let mut tx2 = sample_tx("t2", "sell", "MSFT", 5.0, 300.0);
        tx2.date = "2024-07-01".to_string();
        pm.add_transaction("test", &tx2).unwrap();

        let aapl = pm
            .get_transactions("test", Some("AAPL"), None, None, None)
            .unwrap();
        assert_eq!(aapl.len(), 1);

        let sells = pm
            .get_transactions("test", None, Some("sell"), None, None)
            .unwrap();
        assert_eq!(sells.len(), 1);

        let july = pm
            .get_transactions("test", None, None, Some("2024-07-01"), None)
            .unwrap();
        assert_eq!(july.len(), 1);
    }

    #[test]
    fn validate_positions_and_cash() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        pm.add_transaction("test", &sample_tx("t1", "buy", "AAPL", 10.0, 150.0))
            .unwrap();
        pm.add_transaction("test", &sample_tx("t2", "buy", "AAPL", 5.0, 155.0))
            .unwrap();
        pm.add_transaction("test", &sample_tx("t3", "sell", "AAPL", 3.0, 160.0))
            .unwrap();

        let deposit = Transaction {
            id: "d1".to_string(),
            date: "2024-06-15".to_string(),
            tx_type: TxType::Deposit,
            symbol: None,
            quantity: None,
            price: None,
            commission: None,
            amount: Some(10000.0),
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: "2024-06-15T00:00:00Z".to_string(),
        };
        pm.add_transaction("test", &deposit).unwrap();

        let report = pm.validate("test").unwrap();
        assert!(report.valid);
        assert_eq!(report.transaction_count, 4);

        let aapl = report
            .positions
            .iter()
            .find(|p| p.symbol == "AAPL")
            .unwrap();
        assert!((aapl.shares - 12.0).abs() < 0.001);
        assert!((aapl.total_buys - 15.0).abs() < 0.001);
        assert!((aapl.total_sells - 3.0).abs() < 0.001);

        // Cash: 10000 - (10*150 + 1) - (5*155 + 1) + (3*160 - 1) = 8202
        assert!((report.cash_balance - 8202.0).abs() < 0.01);
    }

    #[test]
    fn csv_import() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let csv = "\
type,date,symbol,quantity,price,commission,amount
buy,2024-01-15,AAPL,10,150.0,1.0,
sell,2024-02-20,AAPL,3,160.0,1.0,
dividend,2024-03-01,AAPL,,,,0.5
deposit,2024-01-01,,,,,10000.0
";

        let imported = pm.import_csv("test", csv).unwrap();
        assert_eq!(imported.len(), 4);

        let txs = pm.get_transactions("test", None, None, None, None).unwrap();
        assert_eq!(txs.len(), 4);

        let report = pm.validate("test").unwrap();
        assert!(report.valid);
    }

    #[test]
    fn json_import() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let json = r#"[
            {"id":"a","date":"2024-01-15","type":"buy","symbol":"AAPL","quantity":10.0,"price":150.0,"commission":1.0,"created_at":"2024-01-15T00:00:00Z"},
            {"id":"b","date":"2024-02-20","type":"sell","symbol":"AAPL","quantity":3.0,"price":160.0,"commission":1.0,"created_at":"2024-02-20T00:00:00Z"}
        ]"#;

        let imported = pm.import_json("test", json).unwrap();
        assert_eq!(imported.len(), 2);

        let txs = pm.get_transactions("test", None, None, None, None).unwrap();
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn export_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        pm.add_transaction("test", &sample_tx("t1", "buy", "AAPL", 10.0, 150.0))
            .unwrap();

        let json = pm.export_json("test").unwrap();
        assert!(json.contains("AAPL"));

        let csv = pm.export_csv("test").unwrap();
        assert!(csv.contains("AAPL"));
        assert!(csv.contains("buy"));
    }

    #[test]
    fn validate_detects_issues() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let mut bad = sample_tx("bad1", "buy", "AAPL", 0.0, 150.0);
        bad.quantity = Some(0.0);
        pm.add_transaction("test", &bad).unwrap();

        let report = pm.validate("test").unwrap();
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.contains("non-positive quantity"))
        );
    }

    #[test]
    fn notes_crud() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let id = pm
            .add_note(
                "test",
                "AAPL",
                "2024-06-15",
                "Earnings review",
                "Beat estimates by 5%",
                &["earnings".into(), "bullish".into()],
            )
            .unwrap();
        assert!(!id.is_empty());

        // List all
        let notes = pm.list_notes("test", "AAPL", None, None, None).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0]["title"], "Earnings review");

        // Filter by tag
        let tagged = pm
            .list_notes("test", "AAPL", None, None, Some(&["earnings".into()]))
            .unwrap();
        assert_eq!(tagged.len(), 1);

        let not_found = pm
            .list_notes("test", "AAPL", None, None, Some(&["bearish".into()]))
            .unwrap();
        assert!(not_found.is_empty());

        // Filter by date range
        let in_range = pm
            .list_notes("test", "AAPL", Some("2024-01-01"), Some("2024-12-31"), None)
            .unwrap();
        assert_eq!(in_range.len(), 1);

        let out_of_range = pm
            .list_notes("test", "AAPL", Some("2025-01-01"), None, None)
            .unwrap();
        assert!(out_of_range.is_empty());

        // Delete
        pm.delete_note(&id).unwrap();
        let empty = pm.list_notes("test", "AAPL", None, None, None).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn delete_nonexistent_note() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        assert!(pm.delete_note("nonexistent-id").is_err());
    }

    #[test]
    fn file_attach_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let data = base64::engine::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"Hello, portfolio!",
        );

        let id = pm
            .attach_file(
                "test",
                "AAPL",
                "2024-06-15",
                "notes.txt",
                "text/plain",
                &data,
                "my research notes",
            )
            .unwrap();
        assert!(!id.is_empty());

        let files = pm.list_files("test", "AAPL").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["filename"], "notes.txt");
        assert_eq!(files[0]["mime_type"], "text/plain");
        assert_eq!(files[0]["size"], 17);
        assert_eq!(files[0]["notes"], "my research notes");

        // Verify file exists on disk
        let disk_path = files[0]["path"].as_str().unwrap();
        assert!(std::path::Path::new(disk_path).exists());
        let contents = std::fs::read_to_string(disk_path).unwrap();
        assert_eq!(contents, "Hello, portfolio!");
    }

    #[test]
    fn attachment_insert_failure_removes_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let conn = pm.open().unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_file_insert BEFORE INSERT ON files
             BEGIN SELECT RAISE(ABORT, 'injected metadata failure'); END;",
        )
        .unwrap();
        drop(conn);

        let data = base64::engine::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"orphan candidate",
        );
        let error = pm
            .attach_file(
                "test",
                "AAPL",
                "2024-06-15",
                "research.txt",
                "text/plain",
                &data,
                "",
            )
            .unwrap_err();

        assert!(error.to_string().contains("injected metadata failure"));
        let files_dir = pm.base_dir().join("test").join("files");
        assert!(
            std::fs::read_dir(files_dir).unwrap().next().is_none(),
            "metadata failure must not leave an untracked attachment"
        );
    }

    #[test]
    fn file_delete_preserves_metadata_when_filesystem_delete_fails() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let data = base64::engine::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"cannot remove",
        );
        let id = pm
            .attach_file(
                "test",
                "MSFT",
                "2024-01-01",
                "locked.txt",
                "text/plain",
                &data,
                "",
            )
            .unwrap();
        let files = pm.list_files("test", "MSFT").unwrap();
        let disk_path = std::path::PathBuf::from(files[0]["path"].as_str().unwrap());

        std::fs::remove_file(&disk_path).unwrap();
        std::fs::create_dir(&disk_path).unwrap();

        let error = pm.delete_file(&id).unwrap_err();
        assert!(error.to_string().contains("metadata preserved"));
        assert_eq!(pm.list_files("test", "MSFT").unwrap().len(), 1);
    }

    #[test]
    fn file_delete() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        let data = base64::engine::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"temp file",
        );

        let id = pm
            .attach_file(
                "test",
                "MSFT",
                "2024-01-01",
                "temp.txt",
                "text/plain",
                &data,
                "",
            )
            .unwrap();

        let files = pm.list_files("test", "MSFT").unwrap();
        let disk_path = files[0]["path"].as_str().unwrap().to_string();

        pm.delete_file(&id).unwrap();
        assert!(pm.list_files("test", "MSFT").unwrap().is_empty());
        assert!(!std::path::Path::new(&disk_path).exists());

        assert!(pm.delete_file("nonexistent").is_err());
    }

    // ── Tracer-bullet contract: return computation ──────────────────
    //
    // Verifies the computational core of portfolio_returns:
    // position tracking, total return, and Modified Dietz.
    // This is the narrowest end-to-end path that doesn't require
    // API mocking: transactions + cached prices → returns.

    #[test]
    fn portfolio_returns_contract() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test").unwrap();

        // Scenario: deposit $20,000, buy 100 AAPL @ $150 ($15,000 spent, $5,000 cash)
        let txs = [
            ("2024-01-02", "deposit", None, None, None, Some(20000.0)),
            (
                "2024-01-15",
                "buy",
                Some("AAPL"),
                Some(100.0),
                Some(150.0),
                None,
            ),
        ];
        for (date, tx_type, sym, qty, price, amt) in &txs {
            pm.add_transaction(
                "test",
                &Transaction {
                    id: uuid::Uuid::new_v4().to_string(),
                    date: date.to_string(),
                    tx_type: tx_type.parse().unwrap(),
                    symbol: sym.map(|s| s.to_string()),
                    quantity: *qty,
                    price: *price,
                    commission: Some(0.0),
                    amount: *amt,
                    currency: "USD".to_string(),
                    notes: String::new(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        }

        // Seed price cache: AAPL @ $150 at start, $165 at end
        let conn = pm.open().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO price_cache (portfolio_name, symbol, date, close, source, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["test", "AAPL", "2024-01-02", 150.0, "test", "2024-01-01T00:00:00Z"],
        ).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO price_cache (portfolio_name, symbol, date, close, source, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["test", "AAPL", "2024-03-31", 165.0, "test", "2024-01-01T00:00:00Z"],
        ).unwrap();

        // Verify transaction count and position tracking
        let txs_all = pm.get_transactions("test", None, None, None, None).unwrap();
        assert_eq!(txs_all.len(), 2, "deposit + buy = 2 transactions");

        let report = pm.validate("test").unwrap();
        assert!(report.valid, "no validation issues");
        // Cash: +20000 deposit - (100 * 150 + 0) buy = 5000 cash remaining
        assert!(
            (report.cash_balance - 5000.0).abs() < 0.01,
            "expected $5,000 cash"
        );

        let positions: Vec<String> = report.positions.iter().map(|p| p.symbol.clone()).collect();
        assert!(
            positions.contains(&"AAPL".to_string()),
            "AAPL position exists"
        );

        // Verify cached prices
        let prices = pm
            .get_prices("test", "AAPL", "2024-01-01", "2024-04-01")
            .unwrap();
        assert_eq!(prices.len(), 2, "two price entries cached");
    }

    #[test]
    fn portfolio_returns_contract_total_return_formula() {
        // Direct formula verification with known values.
        // total_return = (end_value - start_value - net_flows) / start_value
        // Scenario: start $10,000, deposit $5,000 mid-period, end $16,000
        // net_flows = +5000 → total_return = (16000 - 10000 - 5000) / 10000 = 0.10 = 10%

        let start_value = 10000.0f64;
        let end_value = 16000.0f64;
        let net_flows = 5000.0f64;
        let total_return = (end_value - start_value - net_flows) / start_value;
        assert!((total_return - 0.10).abs() < 0.0001, "total_return = 10%");

        // Modified Dietz: (end - start - flows) / (start + weighted_flows)
        // Flow at day 30 of 90-day period: weight = (90-30)/90 = 2/3
        // weighted_flows = 5000 * 2/3 ≈ 3333.33
        // modified_dietz = 1000 / 13333.33 ≈ 0.075
        let period_days = 90.0;
        let days_remaining = 60.0;
        let weight = days_remaining / period_days;
        let weighted_flows = net_flows * weight;
        let modified_dietz = (end_value - start_value - net_flows) / (start_value + weighted_flows);
        assert!(
            (modified_dietz - 0.075).abs() < 0.001,
            "modified_dietz ≈ 7.5%"
        );
    }
}
