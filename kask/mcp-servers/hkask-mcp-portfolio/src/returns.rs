use chrono::Datelike;
use hkask_types::time::now_rfc3339;
use rusqlite::{OptionalExtension, params};

use crate::store::PortfolioStore;
use crate::types::{
    AssetType, LedgerFilter, MAX_IMPORT_REQUEST_BYTES, MAX_IMPORT_TRANSACTION_COUNT,
    PortfolioError, PriceResolver, ReturnsReport, Transaction, TxType, check_request_size,
};

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

    let from_days = from_date.num_days_from_ce();
    let to_days = (to_date.num_days_from_ce() - from_days) as f64;
    let (irr, converged) = compute_irr(
        total_start,
        total_end,
        &cash_flow_events,
        from_days,
        to_days,
    )?;

    Ok(ReturnsReport {
        portfolio: name.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        total_return,
        modified_dietz,
        irr,
        irr_converged: converged,
        start_value: total_start,
        end_value: total_end,
        net_cash_flows: net_flows,
        cash_flow_count: cash_flow_events.len(),
        positions_at_start: positions_start.len(),
        positions_at_end: positions_end.len(),
    })
}

/// Compute the internal rate of return via Newton's method.
///
/// `cash_flow_events` are `(date_string, amount)` pairs occurring strictly
/// between `from` and `to`. `from_days` is the `num_days_from_ce()` of the
/// period start date; `to_days` is the day offset from `from_days` to the
/// period end date. Returns `Ok((irr_rate, converged))` or propagates a
/// `PortfolioError` if a cash-flow date is malformed.
pub fn compute_irr(
    total_start: f64,
    total_end: f64,
    cash_flow_events: &[(String, f64)],
    from_days: i32,
    to_days: f64,
) -> Result<(f64, bool), PortfolioError> {
    // The first cash flow (-total_start) is at day 0 (the `from` date);
    // subsequent flows are relative to it. Using the absolute day number
    // here would put the exponent at ~738000/365 ≈ 2021, causing Newton's
    // method to diverge numerically.
    let mut cfs: Vec<(f64, f64)> = vec![(-total_start, 0.0)];
    for (date_str, amt) in cash_flow_events {
        let cf_date = parse_ymd(date_str, "cash flow date")?;
        let days = (cf_date.num_days_from_ce() - from_days) as f64;
        cfs.push((*amt, days));
    }
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

    Ok((r, converged))
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
