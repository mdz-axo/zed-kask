use hkask_types::{WebID, agent_paths::sanitize_name, time::now_rfc3339};
use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::returns::{CachedPriceResolver, parse_ymd};
use crate::types::{
    AssetType, DailyReturnRow, Holding, HoldingsSnapshot, LedgerFilter, NoPrices, PortfolioError,
    PriceResolver, Transaction, TxType,
};

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

/// The deepened portfolio store. Seven public methods hide all storage
/// mechanics (SQLite, schema, FK cascades). Callers apply transactions,
/// read the ledger, and query materialized projections.
///
/// The store is `Clone` (it holds only a path), so an MCP server can clone
/// it into a `spawn_blocking` task per request.
#[derive(Clone)]
pub struct PortfolioStore {
    db_path: PathBuf,
}

/// Open the portfolio DB at `path` and apply [`SCHEMA_DDL`].
///
/// If schema initialization fails on an existing DB created by an older
/// schema (e.g. a missing `asset_type` column that the index DDL references),
/// the stale file is deleted and recreated from scratch. No migration path is
/// maintained — portfolio data is treated as disposable across schema changes.
fn open_with_schema_recovery(path: &std::path::Path) -> Result<PathBuf, PortfolioError> {
    let conn =
        Connection::open(path).map_err(|e| format!("failed to open portfolio database: {e}"))?;
    if let Err(e) = conn.execute_batch(SCHEMA_DDL) {
        eprintln!(
            "portfolio schema initialization failed on existing DB at {} — \
             discarding stale file and recreating from scratch: {e}",
            path.display()
        );
        drop(conn);
        std::fs::remove_file(path)
            .map_err(|e| format!("failed to remove stale portfolio database: {e}"))?;
        let conn = Connection::open(path)
            .map_err(|e| format!("failed to reopen portfolio database: {e}"))?;
        conn.execute_batch(SCHEMA_DDL)
            .map_err(|e| format!("failed to initialize portfolio schema: {e}"))?;
    }
    Ok(path.to_path_buf())
}

impl PortfolioStore {
    /// Creates storage scoped to the authenticated server owner.
    ///
    /// If the existing database was created by an older schema that the current
    /// DDL cannot reconcile (e.g. a missing `asset_type` column on `transactions`
    /// that the index DDL references), the stale file is deleted and recreated
    /// from scratch. No backward-compatibility/migration path is maintained —
    /// portfolio data is treated as disposable across schema changes.
    pub fn new(owner: WebID) -> Result<Self, PortfolioError> {
        // Databases live in the internal data dir (the ONLY thing that lives
        // there — artifact files like transactions go to the visible
        // artifacts dir under {server}-mcp/{artifact-type}/). Portfolio DB
        // lives at `{kask_data_dir}/mcp/portfolio/{owner}/master.db`,
        // resolved via `resolve_under_data_dir`.
        let mut path = hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
            hkask_types::agent_paths::MCP_DIR,
        ))
        .join("portfolio");
        path.push(sanitize_name(&owner.to_string()));
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create portfolio directory: {e}"))?;
        path.push("master.db");
        let db_path = open_with_schema_recovery(&path)?;
        Ok(Self { db_path })
    }

    /// Test constructor: create a store backed by a DB at `base_dir/master.db`.
    /// Not `#[cfg(test)]` so downstream crates (e.g. hkask-mcp-companies) can
    /// use it in their own test suites.
    pub fn with_dir(base_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&base_dir).expect("failed to create test portfolio directory");
        let db_path = base_dir.join("master.db");
        let db_path = open_with_schema_recovery(&db_path)
            .expect("failed to initialize test portfolio schema");
        Self { db_path }
    }

    /// Test constructor: create a store for a specific owner under `base_dir`.
    pub fn with_dir_for_owner(base_dir: PathBuf, owner: WebID) -> Self {
        Self::with_dir(base_dir.join(sanitize_name(&owner.to_string())))
    }

    pub(crate) fn open(&self) -> Result<Connection, PortfolioError> {
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

    /// The portfolio's asset type (from the `portfolios` table).
    /// Determines the price semantics [`rebuild_views`](Self::rebuild_views)
    /// uses: prediction-contract portfolios value holdings at the index
    /// probability (the documented NoPrices semantics), stock portfolios
    /// require seeded prices.
    pub fn asset_type(&self, name: &str) -> Result<AssetType, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let asset_type: String = conn
            .query_row(
                "SELECT asset_type FROM portfolios WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .map_err(|e| format!("query asset_type: {e}"))?;
        match asset_type.as_str() {
            "stock" => Ok(AssetType::Stock),
            "prediction_contract" => Ok(AssetType::PredictionContract),
            "portfolio" => Ok(AssetType::Portfolio),
            other => Err(format!("unknown asset_type '{other}' for portfolio {name}").into()),
        }
    }

    /// Delete a portfolio and all its transactions, holdings, and returns
    /// (FK cascade). Returns `NotFound` if it does not exist.
    pub fn delete(&self, name: &str) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        let rows = conn
            .execute("DELETE FROM portfolios WHERE name = ?1", params![name])
            .map_err(|e| format!("delete: {e}"))?;
        if rows == 0 {
            return Err(PortfolioError::NotFound(format!(
                "portfolio '{name}' does not exist"
            )));
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
        for date in &dates {
            // Recompute and cache the holdings snapshot for each transaction date.
            self.snapshot(name, date)?;
        }
        // Materialize daily returns across the full date span. Uses the
        // price cache (seeded via portfolio_seed_price) for stock
        // portfolios; prediction-contract portfolios (CMP indices) value
        // holdings at the index probability, not a traded price, so they
        // use the NoPrices semantics. A held stock position with no price
        // on or before a day is a data gap and fails the rebuild — it is
        // never silently zero-valued.
        let asset_type = self.asset_type(name)?;
        if let Some(first) = dates.iter().next()
            && let Some(last) = dates.iter().next_back()
        {
            match asset_type {
                AssetType::PredictionContract => {
                    self.materialize_returns(name, first, last, &NoPrices)?;
                }
                AssetType::Stock | AssetType::Portfolio => {
                    let resolver = CachedPriceResolver::new(self, name);
                    self.materialize_returns(name, first, last, &resolver)?;
                }
            }
        }
        Ok(())
    }

    /// Materialize the daily returns view for a date range. For each day
    /// in `[from, to]`, computes the portfolio's total value (market value
    /// of holdings at that day's prices + cash) and the day-over-day return,
    /// then writes a row to `daily_returns`. Reads prices via the supplied
    /// resolver (typically [`CachedPriceResolver`] over the `price_cache`
    /// table, which resolves as-of the last price on or before each day).
    ///
    /// A held position with no resolvable price is a data gap, not a zero
    /// valuation: the call fails naming the gaps and writes nothing (rows
    /// are buffered and committed only when the range is gap-free). The gap
    /// gate is skipped for resolvers that do not expect prices (NoPrices
    /// portfolios value holdings at zero by design).
    ///
    /// This is the realization of the `daily_returns` materialized view
    /// declared in the schema. Idempotent: re-running over the same range
    /// replaces existing rows.
    pub fn materialize_returns(
        &self,
        name: &str,
        from: &str,
        to: &str,
        prices: &dyn PriceResolver,
    ) -> Result<(), PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let from_date = parse_ymd(from, "from")?;
        let to_date = parse_ymd(to, "to")?;
        if to_date < from_date {
            return Err(format!("from ({from}) is after to ({to})").into());
        }

        // Read the ledger once and walk it incrementally. This is O(N + D)
        // where N = transactions, D = days — not O(N × D) from re-scanning
        // the ledger per day via snapshot().
        let txs = self.ledger(name, LedgerFilter::all())?;

        // Group transactions by date for the cash-flow computation.
        let mut txs_by_date: std::collections::BTreeMap<String, Vec<&Transaction>> =
            std::collections::BTreeMap::new();
        for tx in &txs {
            txs_by_date.entry(tx.date.clone()).or_default().push(tx);
        }

        // Running state: positions (symbol → shares) and cash balance.
        let mut positions: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut cash: f64 = 0.0;

        // Apply all transactions before `from` to initialize the running state.
        for tx in &txs {
            if tx.date.as_str() >= from {
                break;
            }
            apply_tx_to_running_state(tx, &mut positions, &mut cash);
        }

        let mut prev_total: Option<f64> = None;
        let mut current_date = from_date;
        // Buffered rows — committed only when the range is gap-free, so a
        // failing materialization never leaves a partial view.
        let mut rows: Vec<(String, f64, f64, f64, f64)> = Vec::new();
        let mut missing_prices: Vec<String> = Vec::new();
        while current_date <= to_date {
            let date_str = current_date.format("%Y-%m-%d").to_string();

            // Apply transactions on this date to the running state.
            if let Some(day_txs) = txs_by_date.get(&date_str) {
                for tx in day_txs {
                    apply_tx_to_running_state(tx, &mut positions, &mut cash);
                }
            }

            // Market value: sum(shares * price) using the resolver. A held
            // position with no resolvable price is a data gap — collected
            // and reported, never silently zero-valued (the pre-fix behavior
            // fabricated a one-day total-loss crash in the return series).
            let mut market_value: f64 = 0.0;
            for (symbol, shares) in positions.iter() {
                if shares.abs() <= 0.0001 {
                    continue;
                }
                match prices.resolve(symbol, &date_str) {
                    Some(price) => market_value += shares * price,
                    None if prices.expects_prices() => {
                        missing_prices.push(format!("{date_str}:{symbol}"));
                    }
                    None => {}
                }
            }
            let total = market_value + cash;

            // Cash flow on this date: sum of deposit/withdrawal amounts.
            let cash_flow: f64 = txs_by_date
                .get(&date_str)
                .map(|day_txs| {
                    day_txs
                        .iter()
                        .map(|t| match t.tx_type {
                            TxType::Deposit => t.amount.unwrap_or(0.0),
                            TxType::Withdrawal => -t.amount.unwrap_or(0.0),
                            _ => 0.0,
                        })
                        .sum()
                })
                .unwrap_or(0.0);

            let daily_return = match prev_total {
                Some(prev) if prev.abs() > 1e-9 => (total - prev - cash_flow) / prev,
                _ => 0.0,
            };

            rows.push((date_str, market_value, cash, total, daily_return));

            prev_total = Some(total);
            match current_date.succ_opt() {
                Some(next) => current_date = next,
                None => break,
            }
        }

        if !missing_prices.is_empty() {
            return Err(format!(
                "missing cached prices for {} held position(s) across {from}..={to}: {}. \
                 A missing price is a data gap, not a zero valuation — seed each \
                 (symbol, date) with portfolio_seed_price and retry",
                missing_prices.len(),
                missing_prices.join(", ")
            )
            .into());
        }

        for (date_str, market_value, cash, total, daily_return) in rows {
            conn.execute(
                "INSERT OR REPLACE INTO daily_returns (portfolio_name, date, market_value, cash, total, daily_return)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![name, date_str, market_value, cash, total, daily_return],
            )
            .map_err(|e| format!("materialize daily_returns: {e}"))?;
        }
        Ok(())
    }

    /// Read the materialized daily returns for a portfolio over a date range.
    /// Returns `(date, market_value, cash, total, daily_return)` rows ordered
    /// by date ascending. Empty when the view has not been materialized
    /// (call [`materialize_returns`] or [`rebuild_views`] first).
    pub fn daily_returns(
        &self,
        name: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<DailyReturnRow>, PortfolioError> {
        let conn = self.open()?;
        self.check_exists(&conn, name)?;
        let mut stmt = conn
            .prepare(
                "SELECT date, market_value, cash, total, daily_return FROM daily_returns
                 WHERE portfolio_name = ?1 AND date >= ?2 AND date <= ?3 ORDER BY date ASC",
            )
            .map_err(|e| format!("query daily_returns: {e}"))?;
        let rows = stmt
            .query_map(params![name, from, to], |row| {
                Ok(DailyReturnRow {
                    date: row.get(0)?,
                    market_value: row.get(1)?,
                    cash: row.get(2)?,
                    total: row.get(3)?,
                    daily_return: row.get(4)?,
                })
            })
            .map_err(|e| format!("query daily_returns: {e}"))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("row: {e}"))?);
        }
        Ok(out)
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
            return Err(PortfolioError::NotFound(format!(
                "portfolio '{name}' does not exist"
            )));
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
}

// ── Free functions: pure projections over the ledger ────────────────

/// Apply a transaction to the running positions + cash state (used by
/// `materialize_returns` for incremental computation).
fn apply_tx_to_running_state(
    tx: &Transaction,
    positions: &mut std::collections::HashMap<String, f64>,
    cash: &mut f64,
) {
    match tx.tx_type {
        TxType::Buy => {
            let qty = tx.quantity.unwrap_or(0.0);
            let price = tx.price.unwrap_or(0.0);
            let comm = tx.commission.unwrap_or(0.0);
            if let Some(ref sym) = tx.symbol {
                *positions.entry(sym.clone()).or_insert(0.0) += qty;
            }
            *cash -= qty * price + comm;
        }
        TxType::Sell => {
            let qty = tx.quantity.unwrap_or(0.0);
            let price = tx.price.unwrap_or(0.0);
            let comm = tx.commission.unwrap_or(0.0);
            if let Some(ref sym) = tx.symbol {
                *positions.entry(sym.clone()).or_insert(0.0) -= qty;
            }
            *cash += qty * price - comm;
        }
        TxType::Roll => {
            if let Some(ref sym) = tx.symbol {
                *positions.entry(sym.clone()).or_insert(0.0) += tx.quantity.unwrap_or(0.0);
            }
        }
        TxType::WeightAdjust | TxType::Dividend => {}
        TxType::Deposit => {
            *cash += tx.amount.unwrap_or(0.0);
        }
        TxType::Withdrawal => {
            *cash -= tx.amount.unwrap_or(0.0);
        }
    }
}

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
