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
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic, map_join_error};
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
        execute_tool_semantic(
            self,
            "portfolio_delete",
            Self::ontology_anchor("portfolio_delete"),
            async {
                let response_name = name.clone();
                run_portfolio(self.portfolio.clone(), move |portfolio| {
                    portfolio.delete(&name)
                })
                .await?;
                Ok(serde_json::json!({"status": "deleted", "name": response_name}))
            },
        )
        .await
    }

    #[tool(description = "List all portfolios")]
    pub async fn portfolio_list(&self) -> String {
        execute_tool_semantic(self, "portfolio_list", Self::ontology_anchor("portfolio_list"), async {
            let names = run_portfolio(self.portfolio.clone(), |portfolio| portfolio.list()).await?;
            Ok(fibo::enrich_with_ontology(
                serde_json::json!({"portfolios": names, "fibo": {"portfolio": fibo::PORTFOLIO}}),
                "portfolio_list",
            ))
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
        execute_tool_semantic(
            self,
            "ledger_import",
            Self::ontology_anchor("ledger_import"),
            async {
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
            },
        )
        .await
    }

    #[tool(description = "Export portfolio ledger to CSV or JSON")]
    pub async fn ledger_export(
        &self,
        Parameters(LedgerExportRequest { portfolio, format }): Parameters<LedgerExportRequest>,
    ) -> String {
        execute_tool_semantic(self, "ledger_export", Self::ontology_anchor("ledger_export"), async {
            let output_format = format.clone();
            let data = run_portfolio(self.portfolio.clone(), move |manager| match format {
                types::ImportFormat::Csv => manager.export_csv(&portfolio),
                types::ImportFormat::Json => manager.export_json(&portfolio),
            })
            .await?;
            Ok(fibo::enrich_with_ontology(
                serde_json::json!({"format": output_format, "data": data, "fibo": {"transaction_ledger": fibo::TRANSACTION_LEDGER}}),
                "ledger_export",
            ))
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
        execute_tool_semantic(
            self,
            "transaction_note_append",
            Self::ontology_anchor("transaction_note_append"),
            async {
                let response_tx_id = tx_id.clone();
                run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.append_note(&portfolio, &tx_id, &note)
                })
                .await?;
                Ok(serde_json::json!({"status": "note appended", "tx_id": response_tx_id}))
            },
        )
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
        execute_tool_semantic(
            self,
            "portfolio_comparison",
            Self::ontology_anchor("portfolio_comparison"),
            async {
                run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.compare(&portfolio_a, &portfolio_b)
                })
                .await
            },
        )
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
        execute_tool_semantic(
            self,
            "portfolio_returns",
            Self::ontology_anchor("portfolio_returns"),
            async {
                // SF-4: validate from/to up front — never silently epoch-substitute
                // a malformed date (the prior `unwrap_or_default()` produced
                // garbage IRR while `irr_converged` reported true).
                let _from_date = parse_date_arg(&from, "from")?;
                let _to_date = parse_date_arg(&to, "to")?;

                // Gather the symbols we need prices for, by reading the ledger
                // and computing the positions at `from` and `to`. The returns
                // computation itself is delegated to the portfolio crate
                // (`hkask_mcp_portfolio::returns`), which is provider-agnostic.
                // This tool's job is to seed the portfolio store's price cache
                // from FMP/EODHD so the delegated computation has prices to read.
                let transaction_portfolio = portfolio.clone();
                let txs = run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.get_transactions(&transaction_portfolio, None, None, None, None)
                })
                .await?;

                // Compute positions at `from` and `to` to know which symbols
                // need prices. This mirrors the portfolio crate's returns logic
                // but only to enumerate symbols — the actual returns math runs
                // in the portfolio crate.
                let mut positions_start: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                let mut positions_end: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                for tx in &txs {
                    if let Some(ref sym) = tx.symbol {
                        let qty = tx.quantity.unwrap_or(0.0);
                        if tx.date.as_str() <= from.as_str() {
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
                        if tx.date.as_str() <= to.as_str() {
                            match tx.tx_type {
                                TxType::Buy => {
                                    *positions_end.entry(sym.clone()).or_insert(0.0) += qty
                                }
                                TxType::Sell => {
                                    *positions_end.entry(sym.clone()).or_insert(0.0) -= qty
                                }
                                _ => {}
                            }
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

                // Seed the portfolio store's price cache: try the cache first,
                // then fall back to FMP/EODHD and seed the cache for next time.
                for date in [&from, &to] {
                    for sym in &all_symbols {
                        let cached = run_portfolio(self.portfolio.clone(), {
                            let portfolio = portfolio.clone();
                            let symbol = sym.clone();
                            let date = (*date).to_string();
                            move |manager| manager.get_prices(&portfolio, &symbol, &date, &date)
                        })
                        .await;
                        let already_cached = matches!(cached, Ok(ref v) if !v.is_empty());
                        if already_cached {
                            continue;
                        }
                        // Fall back to the provider API, then seed the cache.
                        if let Ok(view) = self.fetch_historical_price(sym, date, date).await
                            && let Some(close) = view.latest_close()
                        {
                            let seed_portfolio = portfolio.clone();
                            let seed_symbol = sym.clone();
                            let seed_date = (*date).to_string();
                            run_portfolio(self.portfolio.clone(), move |manager| {
                                manager.seed_price_cache(
                                    &seed_portfolio,
                                    &seed_symbol,
                                    &seed_date,
                                    close,
                                    "fmp-eodhd",
                                )
                            })
                            .await?;
                        }
                    }
                }

                // Delegate the returns computation to the portfolio crate. It
                // reads prices from the cache we just seeded.
                let returns_portfolio = portfolio.clone();
                let returns_from = from.clone();
                let returns_to = to.clone();
                let report = run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.compute_returns(&returns_portfolio, &returns_from, &returns_to)
                })
                .await?;

                // Server-authoritative provenance (T3): the widget carries this so it
                // can re-issue `portfolio_returns` with a scrubbed date range (T5).
                let provenance_args = serde_json::json!({
                    "portfolio": portfolio.clone(),
                    "from": from.clone(),
                    "to": to.clone(),
                });
                let provenance_span_id = serde_json::Value::Null;

                Ok(fibo::enrich_with_ontology(
                    serde_json::json!({
                        "portfolio": report.portfolio,
                        "from": report.from,
                        "to": report.to,
                        "total_return": report.total_return,
                        "modified_dietz": report.modified_dietz,
                        "irr": report.irr,
                        "irr_converged": report.irr_converged,
                        "start_value": report.start_value,
                        "end_value": report.end_value,
                        "net_cash_flows": report.net_cash_flows,
                        "cash_flow_count": report.cash_flow_count,
                        "positions_at_start": report.positions_at_start,
                        "positions_at_end": report.positions_at_end,
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
                    }),
                    "portfolio_returns",
                ))
            },
        )
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
        execute_tool_semantic(self, "note_add", Self::ontology_anchor("note_add"), async {
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
        execute_tool_semantic(
            self,
            "note_list",
            Self::ontology_anchor("note_list"),
            async {
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
            },
        )
        .await
    }

    #[tool(description = "Delete a note by ID")]
    pub async fn note_delete(
        &self,
        Parameters(NoteDeleteRequest { note_id }): Parameters<NoteDeleteRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "note_delete",
            Self::ontology_anchor("note_delete"),
            async {
                let response_note_id = note_id.clone();
                run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.delete_note(&note_id)
                })
                .await?;
                Ok(serde_json::json!({"status": "deleted", "id": response_note_id}))
            },
        )
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
        execute_tool_semantic(
            self,
            "file_attach",
            Self::ontology_anchor("file_attach"),
            async {
                let id = run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.attach_file(
                        &portfolio, &symbol, &date, &filename, &mime_type, &data, &notes,
                    )
                })
                .await?;
                Ok(serde_json::json!({"status": "attached", "id": id}))
            },
        )
        .await
    }

    #[tool(description = "List attached files for a symbol in a portfolio")]
    pub async fn file_list(
        &self,
        Parameters(FileListRequest { portfolio, symbol }): Parameters<FileListRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "file_list",
            Self::ontology_anchor("file_list"),
            async {
                let files = run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.list_files(&portfolio, &symbol)
                })
                .await?;
                Ok(serde_json::json!({"files": files}))
            },
        )
        .await
    }

    #[tool(description = "Delete an attached file by ID — removes record and file from disk")]
    pub async fn file_delete(
        &self,
        Parameters(FileDeleteRequest { file_id }): Parameters<FileDeleteRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "file_delete",
            Self::ontology_anchor("file_delete"),
            async {
                let response_file_id = file_id.clone();
                run_portfolio(self.portfolio.clone(), move |manager| {
                    manager.delete_file(&file_id)
                })
                .await?;
                Ok(serde_json::json!({"status": "deleted", "id": response_file_id}))
            },
        )
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
            std::sync::Arc::new(hkask_verification::VerificationStore::in_memory()),
            reqwest::Client::new(),
            String::new(),
            String::new(),
            None,
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
            asset_type: hkask_mcp_portfolio::AssetType::Stock,
            symbol: None,
            quantity: None,
            price: None,
            commission: None,
            amount: Some(amount),
            weight: None,
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
            std::sync::Arc::new(hkask_verification::VerificationStore::in_memory()),
            reqwest::Client::new(),
            String::new(),
            String::new(),
            None,
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
