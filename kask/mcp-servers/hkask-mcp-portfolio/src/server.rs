//! hKask MCP Portfolio — MCP server entry point.
//!
//! Exposes the general-purpose [`PortfolioStore`] over MCP. Provider-agnostic:
//! no stock-price or contract-feed credentials. Consumers that need live
//! prices (the companies server) seed the price cache via
//! [`crate::CachedPriceResolver::seed_cache`] before calling `portfolio_returns`.

use crate::{
    AssetType, CachedPriceResolver, HoldingsSnapshot, LedgerFilter, PortfolioError, PortfolioStore,
    ReturnsReport, Transaction, export_csv, export_json, import_csv, import_json, parse_ymd,
    returns,
};
use hkask_mcp_server::server::{McpToolError, execute_tool, map_join_error};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

hkask_mcp_server::mcp_server!(
    pub struct PortfolioServer {
        pub store: PortfolioStore,
    }
);

/// Classify PortfolioError for MCP dispatch: user errors → invalid_argument,
/// system errors → internal.
fn map_portfolio_error(e: PortfolioError) -> McpToolError {
    match &e {
        PortfolioError::InvalidArgument(_) => McpToolError::invalid_argument(e.to_string()),
        _ => McpToolError::internal(e.to_string()),
    }
}

/// Run a blocking portfolio operation on the spawn-blocking pool.
async fn run_store<T>(
    store: PortfolioStore,
    operation: impl FnOnce(PortfolioStore) -> Result<T, PortfolioError> + Send + 'static,
) -> Result<T, McpToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || operation(store))
        .await
        .map_err(|error| map_join_error(error, "portfolio task failed"))?
        .map_err(map_portfolio_error)
}

// ── Request types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioCreateRequest {
    pub name: String,
    #[serde(default)]
    pub asset_type: AssetType,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioNameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioSnapshotRequest {
    pub portfolio: String,
    pub date: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioReturnsRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerApplyRequest {
    pub portfolio: String,
    pub transaction: Transaction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerReadRequest {
    pub portfolio: String,
    pub symbol: Option<String>,
    pub tx_type: Option<String>,
    pub asset_type: Option<AssetType>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerImportRequest {
    pub portfolio: String,
    pub asset_type: AssetType,
    pub format: ImportFormat,
    pub data: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerExportRequest {
    pub portfolio: String,
    pub format: ImportFormat,
}

/// Ledger import/export format.
#[derive(Debug, Clone, Deserialize, JsonSchema, serde::Serialize)]
pub enum ImportFormat {
    Csv,
    Json,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PriceSeedRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date: String,
    pub close: f64,
    pub source: String,
}

/// Request for portfolio_roll: roll a constituent from one contract to its
/// successor at the same tenor (CMP index maintenance). Emits a `roll`
/// transaction recording the move; the caller is responsible for the
/// corresponding sell/buy legs.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioRollRequest {
    pub portfolio: String,
    pub from_symbol: String,
    pub to_symbol: String,
    pub date: String,
    pub quantity: f64,
    pub price: Option<f64>,
}

// ── Tool router ─────────────────────────────────────────────────────

#[tool_router(router = portfolio_router, vis = "pub")]
impl PortfolioServer {
    #[tool(
        description = "Create a portfolio (stock, prediction-contract, or nested portfolio-of-portfolios). Idempotent."
    )]
    pub async fn portfolio_create(
        &self,
        Parameters(PortfolioCreateRequest { name, asset_type }): Parameters<PortfolioCreateRequest>,
    ) -> String {
        execute_tool(self, "portfolio_create", async {
            let response_name = name.clone();
            run_store(self.store.clone(), move |store| store.create(&name, asset_type)).await?;
            Ok(serde_json::json!({"status": "created", "name": response_name, "asset_type": asset_type.to_string()}))
        })
        .await
    }

    #[tool(description = "Delete a portfolio and all its transactions, holdings, and returns.")]
    pub async fn portfolio_delete(
        &self,
        Parameters(PortfolioNameRequest { name }): Parameters<PortfolioNameRequest>,
    ) -> String {
        execute_tool(self, "portfolio_delete", async {
            let response_name = name.clone();
            run_store(self.store.clone(), move |store| store.delete(&name)).await?;
            Ok(serde_json::json!({"status": "deleted", "name": response_name}))
        })
        .await
    }

    #[tool(description = "List all portfolios in this owner's store.")]
    pub async fn portfolio_list(&self) -> String {
        execute_tool(self, "portfolio_list", async {
            let names = run_store(self.store.clone(), |store| store.list()).await?;
            Ok(serde_json::json!({"portfolios": names}))
        })
        .await
    }

    #[tool(
        description = "Append a transaction to a portfolio's ledger (buy, sell, roll, weight_adjust, deposit, withdrawal, dividend)."
    )]
    pub async fn ledger_apply(
        &self,
        Parameters(LedgerApplyRequest {
            portfolio,
            transaction,
        }): Parameters<LedgerApplyRequest>,
    ) -> String {
        execute_tool(self, "ledger_apply", async {
            let tx_id = transaction.id.clone();
            let response_portfolio = portfolio.clone();
            run_store(self.store.clone(), move |store| {
                store.apply(&portfolio, &transaction)
            })
            .await?;
            Ok(serde_json::json!({"status": "applied", "tx_id": tx_id, "portfolio": response_portfolio}))
        })
        .await
    }

    #[tool(
        description = "Read transactions from a portfolio's ledger, optionally filtered by symbol, type, asset type, or date range."
    )]
    pub async fn ledger_read(
        &self,
        Parameters(LedgerReadRequest {
            portfolio,
            symbol,
            tx_type,
            asset_type,
            from_date,
            to_date,
        }): Parameters<LedgerReadRequest>,
    ) -> String {
        execute_tool(self, "ledger_read", async {
            let txs = run_store(self.store.clone(), move |store| {
                store.ledger(
                    &portfolio,
                    LedgerFilter {
                        symbol: symbol.as_deref(),
                        tx_type: tx_type.as_deref(),
                        asset_type,
                        from_date: from_date.as_deref(),
                        to_date: to_date.as_deref(),
                    },
                )
            })
            .await?;
            Ok(serde_json::json!({"transactions": txs, "count": txs.len()}))
        })
        .await
    }

    #[tool(
        description = "Materialized end-of-day holdings for a portfolio (the portfolio's positions at the close of `date`). Cached for fast retrieval by the portfolio viewer."
    )]
    pub async fn portfolio_snapshot(
        &self,
        Parameters(PortfolioSnapshotRequest { portfolio, date }): Parameters<
            PortfolioSnapshotRequest,
        >,
    ) -> String {
        execute_tool(self, "portfolio_snapshot", async {
            // Validate the date up front — never silently epoch-substitute
            // (the SF-4 bug: a malformed date produced garbage projections
            // while callers reported success).
            parse_ymd(&date, "date").map_err(map_portfolio_error)?;
            let snapshot: HoldingsSnapshot = run_store(self.store.clone(), move |store| {
                store.snapshot(&portfolio, &date)
            })
            .await?;
            serde_json::to_value(snapshot)
                .map_err(|e| McpToolError::internal(format!("serialize snapshot: {e}")))
        })
        .await
    }

    #[tool(
        description = "Time-weighted and money-weighted returns for a date range. Reads prices from the portfolio's price cache (seed it with portfolio_seed_price first)."
    )]
    pub async fn portfolio_returns(
        &self,
        Parameters(PortfolioReturnsRequest {
            portfolio,
            from,
            to,
        }): Parameters<PortfolioReturnsRequest>,
    ) -> String {
        execute_tool(self, "portfolio_returns", async {
            // Validate dates up front (SF-4).
            parse_ymd(&from, "from").map_err(map_portfolio_error)?;
            parse_ymd(&to, "to").map_err(map_portfolio_error)?;

            let provenance_portfolio = portfolio.clone();
            let provenance_from = from.clone();
            let provenance_to = to.clone();
            let resolver = CachedPriceResolver::new(&self.store, &portfolio);
            let report: ReturnsReport = run_store(self.store.clone(), move |s| {
                returns(&s, &portfolio, &from, &to, &resolver)
            })
            .await?;

            // Server-authoritative provenance: the widget carries this so it
            // can re-issue `portfolio_returns` with a scrubbed date range.
            let provenance_args = serde_json::json!({
                "portfolio": provenance_portfolio,
                "from": provenance_from,
                "to": provenance_to,
            });
            Ok(serde_json::json!({
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
                "provenance": {
                    "tool": "portfolio_returns",
                    "server": "hkask-mcp-portfolio",
                    "args": provenance_args,
                    "span_id": serde_json::Value::Null,
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Import transactions from CSV or JSON into a portfolio ledger (auto-creates the portfolio)."
    )]
    pub async fn ledger_import(
        &self,
        Parameters(LedgerImportRequest {
            portfolio,
            asset_type,
            format,
            data,
        }): Parameters<LedgerImportRequest>,
    ) -> String {
        execute_tool(self, "ledger_import", async {
            let ids = run_store(self.store.clone(), move |store| match format {
                ImportFormat::Csv => import_csv(&store, &portfolio, asset_type, &data),
                ImportFormat::Json => import_json(&store, &portfolio, asset_type, &data),
            })
            .await?;
            Ok(serde_json::json!({"status": "imported", "count": ids.len(), "ids": ids}))
        })
        .await
    }

    #[tool(description = "Export a portfolio's ledger to CSV or JSON.")]
    pub async fn ledger_export(
        &self,
        Parameters(LedgerExportRequest { portfolio, format }): Parameters<LedgerExportRequest>,
    ) -> String {
        execute_tool(self, "ledger_export", async {
            let output_format = format.clone();
            let data = run_store(self.store.clone(), move |store| match format {
                ImportFormat::Csv => export_csv(&store, &portfolio),
                ImportFormat::Json => export_json(&store, &portfolio),
            })
            .await?;
            Ok(serde_json::json!({"format": output_format, "data": data}))
        })
        .await
    }

    #[tool(
        description = "Seed the price cache for a (portfolio, symbol, date) triple. Call before portfolio_returns for portfolios whose holdings have market prices."
    )]
    pub async fn portfolio_seed_price(
        &self,
        Parameters(PriceSeedRequest {
            portfolio,
            symbol,
            date,
            close,
            source,
        }): Parameters<PriceSeedRequest>,
    ) -> String {
        execute_tool(self, "portfolio_seed_price", async {
            let resolver = CachedPriceResolver::new(&self.store, &portfolio);
            resolver
                .seed_cache(&symbol, &date, close, &source)
                .map_err(map_portfolio_error)?;
            Ok(serde_json::json!({
                "status": "seeded",
                "portfolio": portfolio,
                "symbol": symbol,
                "date": date,
                "close": close,
            }))
        })
        .await
    }

    #[tool(
        description = "Roll a constituent from one contract to its successor at the same tenor (CMP index maintenance). Emits a roll transaction recording the move."
    )]
    pub async fn portfolio_roll(
        &self,
        Parameters(PortfolioRollRequest {
            portfolio,
            from_symbol,
            to_symbol,
            date,
            quantity,
            price,
        }): Parameters<PortfolioRollRequest>,
    ) -> String {
        execute_tool(self, "portfolio_roll", async {
            parse_ymd(&date, "date").map_err(map_portfolio_error)?;
            let response_portfolio = portfolio.clone();
            let response_from = from_symbol.clone();
            let response_to = to_symbol.clone();
            let tx = crate::Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                date: date.clone(),
                tx_type: crate::TxType::Roll,
                asset_type: crate::AssetType::PredictionContract,
                symbol: Some(to_symbol.clone()),
                quantity: Some(quantity),
                price,
                commission: Some(0.0),
                amount: None,
                weight: None,
                currency: "USD".to_string(),
                notes: format!("Roll from {from_symbol} to {to_symbol}"),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            run_store(self.store.clone(), move |store| {
                store.apply(&portfolio, &tx)
            })
            .await?;
            Ok(serde_json::json!({
                "status": "rolled",
                "portfolio": response_portfolio,
                "from_symbol": response_from,
                "to_symbol": response_to,
                "date": date,
                "quantity": quantity,
            }))
        })
        .await
    }

    #[tool(
        description = "Rebuild all materialized views (daily holdings, daily returns) from the ledger. Use after a corruption or a bulk ledger edit."
    )]
    pub async fn portfolio_rebuild_views(
        &self,
        Parameters(PortfolioNameRequest { name }): Parameters<PortfolioNameRequest>,
    ) -> String {
        execute_tool(self, "portfolio_rebuild_views", async {
            let response_name = name.clone();
            run_store(self.store.clone(), move |store| store.rebuild_views(&name)).await?;
            Ok(serde_json::json!({"status": "rebuilt", "portfolio": response_name}))
        })
        .await
    }

    #[tool(
        description = "Materialize the daily returns view for a portfolio over a date range. Reads prices from the portfolio's price cache. Call portfolio_seed_price first."
    )]
    pub async fn portfolio_materialize_returns(
        &self,
        Parameters(PortfolioReturnsRequest {
            portfolio,
            from,
            to,
        }): Parameters<PortfolioReturnsRequest>,
    ) -> String {
        execute_tool(self, "portfolio_materialize_returns", async {
            parse_ymd(&from, "from").map_err(map_portfolio_error)?;
            parse_ymd(&to, "to").map_err(map_portfolio_error)?;
            let response_portfolio = portfolio.clone();
            let response_from = from.clone();
            let response_to = to.clone();
            let resolver = CachedPriceResolver::new(&self.store, &portfolio);
            run_store(self.store.clone(), move |store| {
                store.materialize_returns(&portfolio, &from, &to, &resolver)
            })
            .await?;
            Ok(serde_json::json!({
                "status": "materialized",
                "portfolio": response_portfolio,
                "from": response_from,
                "to": response_to,
            }))
        })
        .await
    }

    #[tool(
        description = "Read the materialized daily returns for a portfolio over a date range. Returns one row per day with market value, cash, total, and daily return. Empty until portfolio_materialize_returns or portfolio_rebuild_views is called."
    )]
    pub async fn portfolio_daily_returns(
        &self,
        Parameters(PortfolioReturnsRequest {
            portfolio,
            from,
            to,
        }): Parameters<PortfolioReturnsRequest>,
    ) -> String {
        execute_tool(self, "portfolio_daily_returns", async {
            parse_ymd(&from, "from").map_err(map_portfolio_error)?;
            parse_ymd(&to, "to").map_err(map_portfolio_error)?;
            let response_portfolio = portfolio.clone();
            let response_from = from.clone();
            let response_to = to.clone();
            let rows = run_store(self.store.clone(), move |store| {
                store.daily_returns(&portfolio, &from, &to)
            })
            .await?;
            Ok(serde_json::json!({
                "portfolio": response_portfolio,
                "from": response_from,
                "to": response_to,
                "rows": rows,
                "count": rows.len(),
            }))
        })
        .await
    }
}

#[tool_handler(router = Self::portfolio_router())]
impl rmcp::ServerHandler for PortfolioServer {}

// ── Entry point ─────────────────────────────────────────────────────

/// Run the portfolio MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-portfolio",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            Ok(PortfolioServer::new(
                ctx.webid,
                PortfolioStore::new(ctx.webid)?,
            ))
        },
        vec![],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::WebID;

    fn test_server() -> PortfolioServer {
        // Leak the tempdir so the store's db_path remains valid for the
        // lifetime of the server (the async run_store closure needs 'static).
        // Test processes are short-lived; the OS reclaims the space on exit.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        let store = PortfolioStore::with_dir(path);
        PortfolioServer::new(WebID::from_persona(b"anonymous"), store)
    }

    #[test]
    fn portfolio_returns_request_schema_has_no_boolean_property_values() {
        use hkask_mcp_server::find_boolean_schema_positions;
        use schemars::schema_for;
        let schema = schema_for!(PortfolioReturnsRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let violations = find_boolean_schema_positions(&value);
        assert!(
            violations.is_empty(),
            "PortfolioReturnsRequest schema has bare-boolean property values \
             (Ollama/Gemini would reject): {violations:?}"
        );
    }

    #[test]
    fn ledger_apply_request_schema_has_no_boolean_property_values() {
        use hkask_mcp_server::find_boolean_schema_positions;
        use schemars::schema_for;
        let schema = schema_for!(LedgerApplyRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let violations = find_boolean_schema_positions(&value);
        assert!(
            violations.is_empty(),
            "LedgerApplyRequest schema has bare-boolean property values: {violations:?}"
        );
    }

    #[test]
    fn all_request_schemas_are_boolean_clean() {
        use hkask_mcp_server::find_boolean_schema_positions;
        use schemars::schema_for;
        for (name, schema) in [
            (
                "PortfolioCreateRequest",
                schema_for!(PortfolioCreateRequest),
            ),
            ("PortfolioNameRequest", schema_for!(PortfolioNameRequest)),
            (
                "PortfolioSnapshotRequest",
                schema_for!(PortfolioSnapshotRequest),
            ),
            (
                "PortfolioReturnsRequest",
                schema_for!(PortfolioReturnsRequest),
            ),
            ("LedgerApplyRequest", schema_for!(LedgerApplyRequest)),
            ("LedgerReadRequest", schema_for!(LedgerReadRequest)),
            ("LedgerImportRequest", schema_for!(LedgerImportRequest)),
            ("LedgerExportRequest", schema_for!(LedgerExportRequest)),
            ("PriceSeedRequest", schema_for!(PriceSeedRequest)),
            ("PortfolioRollRequest", schema_for!(PortfolioRollRequest)),
        ] {
            let value = serde_json::to_value(&schema).expect("schema serializes");
            let violations = find_boolean_schema_positions(&value);
            assert!(
                violations.is_empty(),
                "{name} schema has bare-boolean property values: {violations:?}"
            );
        }
    }

    #[test]
    fn map_portfolio_error_classifies_invalid_argument() {
        let e = map_portfolio_error(PortfolioError::InvalidArgument("bad".into()));
        let msg = e.to_json_string();
        assert!(msg.contains("bad") || msg.contains("invalid"), "got: {msg}");

        let e2 = map_portfolio_error(PortfolioError::Database("boom".into()));
        let msg2 = e2.to_json_string();
        assert!(
            msg2.contains("boom") || msg2.contains("internal"),
            "got: {msg2}"
        );
    }

    #[test]
    fn test_server_constructs() {
        let _server = test_server();
    }

    #[tokio::test]
    async fn run_store_propagates_invalid_argument() {
        let server = test_server();
        // Deleting a missing portfolio is an InvalidArgument.
        let result: Result<Vec<String>, McpToolError> =
            run_store(server.store.clone(), move |store| store.list()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_store_propagates_database_errors() {
        let server = test_server();
        let result: Result<(), McpToolError> = run_store(server.store.clone(), move |store| {
            store.delete("does-not-exist")
        })
        .await;
        assert!(result.is_err());
    }
}
