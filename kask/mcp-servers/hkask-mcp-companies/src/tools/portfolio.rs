//! Portfolio management tools.
use crate::{
    CompaniesServer, fibo, map_portfolio_error,
    portfolio::{self, PortfolioError, PortfolioManager, TxType},
    types::{
        self, FileAttachRequest, FileDeleteRequest, FileListRequest, LedgerExportRequest,
        LedgerImportRequest, NoteAddRequest, NoteDeleteRequest, NoteListRequest,
        PortfolioCompareRequest, PortfolioNameRequest, PortfolioReturnsRequest,
        TransactionNoteRequest,
    },
};
use chrono::Datelike;
use hkask_mcp_server::server::{McpToolError, execute_tool, map_join_error};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

pub(crate) async fn run_portfolio<T>(
    portfolio: PortfolioManager,
    operation: impl FnOnce(PortfolioManager) -> Result<T, PortfolioError> + Send + 'static,
) -> Result<T, McpToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || operation(portfolio))
        .await
        .map_err(|error| map_join_error(error, "portfolio task failed"))?
        .map_err(map_portfolio_error)
}

/// Parse a `YYYY-MM-DD` date argument, surfacing a malformed value as an
/// `invalid_argument` error rather than silently substituting the epoch
/// (the SF-4 bug: `unwrap_or_default()` produced 1970-01-01 and garbage IRR
/// while `irr_converged` reported true).
fn parse_date_arg(value: &str, field: &str) -> Result<chrono::NaiveDate, McpToolError> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        McpToolError::invalid_argument(format!("{field} must be YYYY-MM-DD (got '{value}')"))
    })
}

#[tool_router(router = portfolio_router, vis = "pub")]
impl CompaniesServer {
    #[tool(description = "Delete a portfolio and all its data")]
    pub async fn portfolio_delete(
        &self,
        Parameters(PortfolioNameRequest { name }): Parameters<PortfolioNameRequest>,
    ) -> String {
        execute_tool(self, "portfolio_delete", async {
            let response_name = name.clone();
            run_portfolio(self.portfolio.clone(), move |portfolio| {
                portfolio.delete(&name)
            })
            .await?;
            Ok(serde_json::json!({"status": "deleted", "name": response_name}))
        })
        .await
    }

    #[tool(description = "List all portfolios")]
    pub async fn portfolio_list(&self) -> String {
        execute_tool(self, "portfolio_list", async {
            let names = run_portfolio(self.portfolio.clone(), |portfolio| portfolio.list()).await?;
            Ok(serde_json::json!({"portfolios": names, "fibo": {"portfolio": fibo::PORTFOLIO}}))
        })
        .await
    }

    #[tool(description = "Import transactions from CSV or JSON into a portfolio ledger")]
    pub async fn ledger_import(
        &self,
        Parameters(LedgerImportRequest {
            portfolio,
            format,
            data,
        }): Parameters<LedgerImportRequest>,
    ) -> String {
        execute_tool(self, "ledger_import", async {
            let (ids, validation) = run_portfolio(self.portfolio.clone(), move |manager| {
                if !manager.list()?.contains(&portfolio) {
                    manager
                        .create(&portfolio)
                        .map_err(|error| format!("auto-create failed: {error}"))?;
                }
                let ids = match format {
                    types::ImportFormat::Csv => manager.import_csv(&portfolio, &data),
                    types::ImportFormat::Json => manager.import_json(&portfolio, &data),
                }?;
                let validation = manager.validate(&portfolio).unwrap_or_else(|error| {
                    portfolio::ValidationReport {
                        valid: false,
                        transaction_count: ids.len(),
                        positions: vec![],
                        cash_balance: 0.0,
                        issues: vec![error.to_string()],
                    }
                });
                Ok((ids, validation))
            })
            .await?;
            Ok(serde_json::json!({
                "status": "imported",
                "count": ids.len(),
                "validation": {
                    "valid": validation.valid,
                    "positions": validation.positions.len(),
                    "cash": validation.cash_balance,
                    "issues": validation.issues,
                }
            }))
        })
        .await
    }

    #[tool(description = "Export portfolio ledger to CSV or JSON")]
    pub async fn ledger_export(
        &self,
        Parameters(LedgerExportRequest { portfolio, format }): Parameters<LedgerExportRequest>,
    ) -> String {
        execute_tool(self, "ledger_export", async {
            let output_format = format.clone();
            let data = run_portfolio(self.portfolio.clone(), move |manager| match format {
                types::ImportFormat::Csv => manager.export_csv(&portfolio),
                types::ImportFormat::Json => manager.export_json(&portfolio),
            })
            .await?;
            Ok(serde_json::json!({"format": output_format, "data": data, "fibo": {"transaction_ledger": fibo::TRANSACTION_LEDGER}}))
        })
        .await
    }

    #[tool(description = "Append a note to an existing transaction")]
    pub async fn transaction_note_append(
        &self,
        Parameters(TransactionNoteRequest {
            portfolio,
            tx_id,
            note,
        }): Parameters<TransactionNoteRequest>,
    ) -> String {
        execute_tool(self, "transaction_note_append", async {
            let response_tx_id = tx_id.clone();
            run_portfolio(self.portfolio.clone(), move |manager| {
                manager.append_note(&portfolio, &tx_id, &note)
            })
            .await?;
            Ok(serde_json::json!({"status": "note appended", "tx_id": response_tx_id}))
        })
        .await
    }

    #[tool(
        description = "Compare two portfolios side by side — positions, overlap, unique symbols"
    )]
    pub async fn portfolio_comparison(
        &self,
        Parameters(PortfolioCompareRequest {
            portfolio_a,
            portfolio_b,
        }): Parameters<PortfolioCompareRequest>,
    ) -> String {
        execute_tool(self, "portfolio_comparison", async {
            run_portfolio(self.portfolio.clone(), move |manager| {
                manager.compare(&portfolio_a, &portfolio_b)
            })
            .await
        })
        .await
    }

    #[tool(description = "Time-weighted and money-weighted returns for a date range")]
    pub async fn portfolio_returns(
        &self,
        Parameters(PortfolioReturnsRequest {
            portfolio,
            from,
            to,
        }): Parameters<PortfolioReturnsRequest>,
    ) -> String {
        execute_tool(self, "portfolio_returns", async {
            // SF-4: validate from/to up front — never silently epoch-substitute
            // a malformed date (the prior `unwrap_or_default()` produced
            // garbage IRR while `irr_converged` reported true).
            let from_date = parse_date_arg(&from, "from")?;
            let to_date = parse_date_arg(&to, "to")?;

            let transaction_portfolio = portfolio.clone();
            let txs = run_portfolio(self.portfolio.clone(), move |manager| {
                manager.get_transactions(&transaction_portfolio, None, None, None, None)
            })
            .await?;

            // ── Compute positions at from and to ─────────────────────
            let mut positions_start: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            let mut positions_end: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            let mut cash_start = 0.0f64;
            let mut cash_end = 0.0f64;

            // Collect cash flow dates for TWR sub-periods
            let mut cash_flow_events: Vec<(String, f64)> = Vec::new();

            for tx in &txs {
                // Cash accounting
                let cf_amount = match tx.tx_type {
                    TxType::Deposit => tx.amount.unwrap_or(0.0),
                    TxType::Withdrawal => -tx.amount.unwrap_or(0.0),
                    TxType::Buy => {
                        let qty = tx.quantity.unwrap_or(0.0);
                        let price = tx.price.unwrap_or(0.0);
                        let comm = tx.commission.unwrap_or(0.0);
                        -(qty * price + comm)
                    }
                    TxType::Sell => {
                        let qty = tx.quantity.unwrap_or(0.0);
                        let price = tx.price.unwrap_or(0.0);
                        let comm = tx.commission.unwrap_or(0.0);
                        qty * price - comm
                    }
                    TxType::Dividend => tx.amount.unwrap_or(0.0),
                };

                if tx.date <= from {
                    cash_start += cf_amount;
                }
                if tx.date <= to {
                    cash_end += cf_amount;
                }

                // Collect deposit/withdrawal events in (from, to] for TWR sub-periods
                if tx.date > from
                    && tx.date <= to
                    && (tx.tx_type == TxType::Deposit || tx.tx_type == TxType::Withdrawal)
                {
                    let amt = match tx.tx_type {
                        TxType::Deposit => tx.amount.unwrap_or(0.0),
                        TxType::Withdrawal => -tx.amount.unwrap_or(0.0),
                        _ => 0.0,
                    };
                    cash_flow_events.push((tx.date.clone(), amt));
                }

                // Position accounting
                if let Some(ref sym) = tx.symbol {
                    let qty = tx.quantity.unwrap_or(0.0);
                    if tx.date <= from {
                        match tx.tx_type {
                            TxType::Buy => {
                                *positions_start.entry(sym.clone()).or_insert(0.0) += qty
                            }
                            TxType::Sell => {
                                *positions_start.entry(sym.clone()).or_insert(0.0) -= qty
                            }
                            _ => {}
                        }
                    }
                    if tx.date <= to {
                        match tx.tx_type {
                            TxType::Buy => *positions_end.entry(sym.clone()).or_insert(0.0) += qty,
                            TxType::Sell => *positions_end.entry(sym.clone()).or_insert(0.0) -= qty,
                            _ => {}
                        }
                    }
                }
            }

            // Retain only positive positions at start
            positions_start.retain(|_, v| *v > 0.0001);

            // Fetch prices for all symbols at from and to
            let all_symbols: Vec<String> = positions_start
                .keys()
                .chain(positions_end.keys())
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();

            let mut prices_at: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();

            // Try price_cache first, then API
            for date in [&from, &to] {
                let key_prefix = format!("{date}:");
                for sym in &all_symbols {
                    // Check cache
                    let cached = run_portfolio(self.portfolio.clone(), {
                        let portfolio = portfolio.clone();
                        let symbol = sym.clone();
                        let date = (*date).to_string();
                        move |manager| manager.get_prices(&portfolio, &symbol, &date, &date)
                    })
                    .await;
                    if let Ok(cached) = cached
                        && let Some((_, close, _)) = cached.first()
                    {
                        prices_at.insert(format!("{key_prefix}{sym}"), *close);
                        continue;
                    }
                    // Fall back to API
                    if let Ok(value) = self
                        .fetch("historical_price", sym, &[("from", date), ("to", date)])
                        .await
                        && let Some(days) = value.get("historical").and_then(|h| h.as_array())
                        && let Some(day) = days.first()
                        && let Some(close) = day
                            .get("close")
                            .or_else(|| day.get("adjClose"))
                            .and_then(|v| v.as_f64())
                    {
                        prices_at.insert(format!("{key_prefix}{sym}"), close);
                    }
                }
            }

            // ── Compute market values ─────────────────────────────────
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

            let mv_start = mv_at(&positions_start, &from);
            let mv_end = mv_at(&positions_end, &to);
            let total_start = mv_start + cash_start;
            let total_end = mv_end + cash_end;

            if total_start <= 0.0 {
                return Ok(serde_json::json!({
                    "error": "portfolio has zero or negative starting value",
                    "from": from,
                    "to": to,
                }));
            }

            let net_flows: f64 = cash_flow_events.iter().map(|(_, amt)| amt).sum();

            // ── Total return ──────────────────────────────────────────
            let total_return = (total_end - total_start - net_flows) / total_start;

            // ── Modified Dietz (approximate TWR) ──────────────────────
            // `from_date`/`to_date` validated up front (SF-4). Cash-flow dates
            // parse with error propagation — a malformed stored date surfaces
            // as invalid_argument rather than a 1970-epoch substitute.
            let period_days = (to_date - from_date).num_days().max(1) as f64;

            let mut weighted_flows: f64 = 0.0;
            for (date_str, amt) in &cash_flow_events {
                let cf_date = parse_date_arg(date_str, "cash flow date")?;
                let days_remaining = (to_date - cf_date).num_days().max(0) as f64;
                let weight = days_remaining / period_days;
                weighted_flows += amt * weight;
            }

            let modified_dietz = if (total_start + weighted_flows).abs() > 0.0001 {
                (total_end - total_start - net_flows) / (total_start + weighted_flows)
            } else {
                total_return
            };

            // ── IRR via Newton's method ───────────────────────────────
            // Treat this as solving NPV(r) = 0 where:
            // cash flows = [-total_start at from, each external CF, +total_end at to]
            let irr = {
                let from_days = from_date.num_days_from_ce();
                let mut cfs: Vec<(f64, f64)> = vec![(-total_start, from_days as f64)];
                for (date_str, amt) in &cash_flow_events {
                    let cf_date = parse_date_arg(date_str, "cash flow date")?;
                    let days = (cf_date.num_days_from_ce() - from_days) as f64;
                    cfs.push((*amt, days));
                }
                let to_days = (to_date.num_days_from_ce() - from_days) as f64;
                cfs.push((total_end, to_days));

                // Newton's method: r_{n+1} = r_n - NPV(r_n) / NPV'(r_n)
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

                let mut r = 0.1; // initial guess: 10%
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
                        r = -0.5; // reset if diving below -100%
                    }
                    if r > 10.0 {
                        r = 1.0; // cap at 100% and continue
                    }
                }
                (r, converged)
            };

            let (irr, irr_converged) = irr;

            // Server-authoritative provenance (T3): the widget carries this so it
            // can re-issue `portfolio_returns` with a scrubbed date range (T5).
            // `args` is the request the tool was invoked with; `span_id` is null
            // here — `execute_tool` emits the reg.tool span via tracing but does
            // not surface the id to the tool body.
            let provenance_args = serde_json::json!({
                "portfolio": portfolio.clone(),
                "from": from.clone(),
                "to": to.clone(),
            });
            let provenance_span_id = serde_json::Value::Null;

            Ok(serde_json::json!({
                "portfolio": portfolio,
                "from": from,
                "to": to,
                "total_return": total_return,
                "modified_dietz": modified_dietz,
                "irr": irr,
                "irr_converged": irr_converged,
                "start_value": total_start,
                "end_value": total_end,
                "net_cash_flows": net_flows,
                "cash_flow_count": cash_flow_events.len(),
                "positions_at_start": positions_start.len(),
                "positions_at_end": positions_end.len(),
                "fibo": {
                    "time_weighted_return": fibo::TIME_WEIGHTED_RETURN,
                    "internal_rate_of_return": fibo::INTERNAL_RATE_OF_RETURN,
                },
                "provenance": {
                    "tool": "portfolio_returns",
                    "server": "hkask-mcp-companies",
                    "args": provenance_args,
                    "span_id": provenance_span_id,
                },
            }))
        })
        .await
    }

    // ── Notes & Files tools ─────────────────────────────────────

    #[tool(description = "Add a note to a company/security as of a date")]
    pub async fn note_add(
        &self,
        Parameters(NoteAddRequest {
            portfolio,
            symbol,
            date,
            title,
            body,
            tags,
        }): Parameters<NoteAddRequest>,
    ) -> String {
        execute_tool(self, "note_add", async {
            let id = run_portfolio(self.portfolio.clone(), move |manager| {
                manager.add_note(&portfolio, &symbol, &date, &title, &body, &tags)
            })
            .await?;
            Ok(serde_json::json!({"status": "created", "id": id}))
        })
        .await
    }

    #[tool(description = "List notes for a symbol, optionally filtered by date range or tags")]
    pub async fn note_list(
        &self,
        Parameters(NoteListRequest {
            portfolio,
            symbol,
            date_from,
            date_to,
            tags,
        }): Parameters<NoteListRequest>,
    ) -> String {
        execute_tool(self, "note_list", async {
            let notes = run_portfolio(self.portfolio.clone(), move |manager| {
                manager.list_notes(
                    &portfolio,
                    &symbol,
                    date_from.as_deref(),
                    date_to.as_deref(),
                    tags.as_deref(),
                )
            })
            .await?;
            Ok(serde_json::json!({"notes": notes}))
        })
        .await
    }

    #[tool(description = "Delete a note by ID")]
    pub async fn note_delete(
        &self,
        Parameters(NoteDeleteRequest { note_id }): Parameters<NoteDeleteRequest>,
    ) -> String {
        execute_tool(self, "note_delete", async {
            let response_note_id = note_id.clone();
            run_portfolio(self.portfolio.clone(), move |manager| {
                manager.delete_note(&note_id)
            })
            .await?;
            Ok(serde_json::json!({"status": "deleted", "id": response_note_id}))
        })
        .await
    }

    #[tool(description = "Attach a file (base64-encoded) to a company/security")]
    pub async fn file_attach(
        &self,
        Parameters(FileAttachRequest {
            portfolio,
            symbol,
            date,
            filename,
            mime_type,
            data,
            notes,
        }): Parameters<FileAttachRequest>,
    ) -> String {
        execute_tool(self, "file_attach", async {
            let id = run_portfolio(self.portfolio.clone(), move |manager| {
                manager.attach_file(
                    &portfolio, &symbol, &date, &filename, &mime_type, &data, &notes,
                )
            })
            .await?;
            Ok(serde_json::json!({"status": "attached", "id": id}))
        })
        .await
    }

    #[tool(description = "List attached files for a symbol in a portfolio")]
    pub async fn file_list(
        &self,
        Parameters(FileListRequest { portfolio, symbol }): Parameters<FileListRequest>,
    ) -> String {
        execute_tool(self, "file_list", async {
            let files = run_portfolio(self.portfolio.clone(), move |manager| {
                manager.list_files(&portfolio, &symbol)
            })
            .await?;
            Ok(serde_json::json!({"files": files}))
        })
        .await
    }

    #[tool(description = "Delete an attached file by ID — removes record and file from disk")]
    pub async fn file_delete(
        &self,
        Parameters(FileDeleteRequest { file_id }): Parameters<FileDeleteRequest>,
    ) -> String {
        execute_tool(self, "file_delete", async {
            let response_file_id = file_id.clone();
            run_portfolio(self.portfolio.clone(), move |manager| {
                manager.delete_file(&file_id)
            })
            .await?;
            Ok(serde_json::json!({"status": "deleted", "id": response_file_id}))
        })
        .await
    }

    // ── Analysis tools ───────────────────────────────────────
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CompaniesServer;
    use crate::learning::LearningState;
    use crate::portfolio::{PortfolioManager, Transaction, TxType};
    use crate::superforecast::FermiDefaults;
    use hkask_types::WebID;
    use rmcp::handler::server::wrapper::Parameters;

    fn test_server(dir: &tempfile::TempDir) -> CompaniesServer {
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        CompaniesServer::new(
            WebID::default(),
            reqwest::Client::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            pm,
            std::sync::Arc::new(std::sync::Mutex::new(LearningState::default())),
            FermiDefaults::default(),
        )
    }

    fn deposit_tx(id: &str, date: &str, amount: f64) -> Transaction {
        Transaction {
            id: id.to_string(),
            date: date.to_string(),
            tx_type: TxType::Deposit,
            symbol: None,
            quantity: None,
            price: None,
            commission: None,
            amount: Some(amount),
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    // SF-4: a malformed `from`/`to` must surface as invalid_argument, NOT a
    // 1970-epoch result with garbage IRR. Pins the regression.
    #[tokio::test]
    async fn portfolio_returns_rejects_malformed_from_date()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let server = test_server(&dir);
        let response = server
            .portfolio_returns(Parameters(PortfolioReturnsRequest {
                portfolio: "test".to_string(),
                from: "not-a-date".to_string(),
                to: "2024-12-01".to_string(),
            }))
            .await;
        // Error wire format: {"error": "...", "kind": "invalid_argument"}.
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(
            parsed["kind"].as_str(),
            Some("invalid_argument"),
            "malformed `from` must surface as invalid_argument, not a 1970-epoch result (SF-4)"
        );
        assert!(
            parsed["error"]
                .as_str()
                .unwrap_or_default()
                .contains("from"),
            "error message names the offending field"
        );
        // Regression guard: the success-path keys must NOT be present.
        assert!(
            parsed.get("total_return").is_none(),
            "no returns body on error"
        );
        assert!(parsed.get("irr").is_none(), "no IRR body on error");
        assert!(
            parsed.get("provenance").is_none(),
            "no provenance body on error"
        );
        Ok(())
    }

    #[tokio::test]
    async fn portfolio_returns_rejects_malformed_to_date() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let server = test_server(&dir);
        let response = server
            .portfolio_returns(Parameters(PortfolioReturnsRequest {
                portfolio: "test".to_string(),
                from: "2024-01-01".to_string(),
                to: "01/02/2024".to_string(),
            }))
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&response)?;
        assert_eq!(parsed["kind"].as_str(), Some("invalid_argument"));
        assert!(
            parsed["error"].as_str().unwrap_or_default().contains("to"),
            "error message names the offending field"
        );
        Ok(())
    }

    // The emitted portfolio block body carries a non-empty provenance.tool.
    #[tokio::test]
    async fn portfolio_returns_emits_dispatchable_provenance()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let pm = PortfolioManager::with_dir(dir.path().to_path_buf());
        pm.create("test")?;
        // A deposit before `from` so total_start > 0 (no positions/prices needed).
        pm.add_transaction("test", &deposit_tx("d1", "2024-01-02", 20000.0))?;
        let server = CompaniesServer::new(
            WebID::default(),
            reqwest::Client::new(),
            String::new(),
            String::new(),
            None,
            None,
            None,
            pm,
            std::sync::Arc::new(std::sync::Mutex::new(LearningState::default())),
            FermiDefaults::default(),
        );
        let response = server
            .portfolio_returns(Parameters(PortfolioReturnsRequest {
                portfolio: "test".to_string(),
                from: "2024-01-03".to_string(),
                to: "2024-12-31".to_string(),
            }))
            .await;
        // Success output is wrapped in the {"content": ...} envelope.
        let content =
            hkask_types::tool_response::parse_tool_response(&response).ok_or_else(|| {
                "portfolio_returns should return a content envelope on success".to_string()
            })?;
        let provenance = content
            .get("provenance")
            .ok_or_else(|| "portfolio_returns emits a provenance block".to_string())?;
        let tool = provenance
            .get("tool")
            .and_then(|t| t.as_str())
            .ok_or_else(|| "provenance.tool is a non-empty string".to_string())?;
        assert!(!tool.is_empty(), "provenance.tool must be non-empty");
        assert_eq!(tool, "portfolio_returns");
        assert_eq!(
            provenance.get("server").and_then(|s| s.as_str()),
            Some("hkask-mcp-companies")
        );
        // args carries the request the tool was invoked with.
        assert_eq!(provenance["args"]["from"].as_str(), Some("2024-01-03"));
        assert_eq!(provenance["args"]["to"].as_str(), Some("2024-12-31"));
        assert_eq!(provenance["args"]["portfolio"].as_str(), Some("test"));
        assert!(
            provenance.get("span_id").is_some(),
            "provenance.span_id present"
        );
        Ok(())
    }
}
