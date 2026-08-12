use serde::{Deserialize, Serialize};

pub(crate) const MAX_IMPORT_REQUEST_BYTES: usize = 5 * 1024 * 1024;
pub(crate) const MAX_IMPORT_TRANSACTION_COUNT: usize = 10_000;

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

// ── Asset type ───────────────────────────────────────────────────────

/// The kind of asset a portfolio holds. Discriminates the polymorphic ledger:
/// a stock portfolio holds tickers; a CMP-index portfolio holds nested
/// portfolio references (each itself a portfolio of contracts); a
/// prediction-event portfolio holds contract identifiers.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    /// A stock ticker (e.g. AAPL, VOD.L).
    #[default]
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

pub(crate) fn check_request_size(
    size: usize,
    maximum: usize,
    subject: &str,
) -> Result<(), PortfolioError> {
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

/// One row of the materialized daily returns view. `daily_return` is the
/// day-over-day total return: `(total_D - total_{D-1} - cash_flow_D) /
/// total_{D-1}`. Zero for the first day in the range (no prior total).
#[derive(Debug, Clone, Serialize)]
pub struct DailyReturnRow {
    pub date: String,
    pub market_value: f64,
    pub cash: f64,
    pub total: f64,
    pub daily_return: f64,
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
