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
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic, map_join_error};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

hkask_mcp_server::mcp_server!(
    pub struct PortfolioServer {
        pub store: PortfolioStore,
    }
);

/// Classify PortfolioError for MCP dispatch: each variant maps to a distinct
/// `McpToolError` kind so callers can distinguish "portfolio doesn't exist"
/// from "database is broken" from "bad input".
pub fn map_portfolio_error(e: PortfolioError) -> McpToolError {
    match e {
        PortfolioError::InvalidArgument(_) => McpToolError::invalid_argument(e.to_string()),
        PortfolioError::NotFound(_) => McpToolError::not_found(e.to_string()),
        PortfolioError::Database(_) | PortfolioError::Serialize(_) => {
            McpToolError::internal(e.to_string()) // rr0044-ok: mapper-internal-arm
        }
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

/// Map a tool name to its FIBO ontology concept URI. The concept is used both
/// as the `reg.tool.*` span ontology tag (via `execute_tool_semantic`) and as
/// the `"ontology"` field in the tool output JSON. The portfolio widget reads
/// this field to drive its "Explain" affordance (the "I" pattern).
fn ontology_anchor(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::fibo;
    match tool {
        "portfolio_snapshot" => Some(fibo::PORTFOLIO),
        "portfolio_returns" => Some(fibo::TIME_WEIGHTED_RETURN),
        "portfolio_create" | "portfolio_delete" | "portfolio_list" => Some(fibo::PORTFOLIO),
        "ledger_apply" | "ledger_read" | "ledger_import" | "ledger_export" => {
            Some(fibo::TRANSACTION_LEDGER)
        }
        "portfolio_seed_price"
        | "portfolio_roll"
        | "portfolio_rebuild_views"
        | "portfolio_materialize_returns"
        | "portfolio_daily_returns" => Some(fibo::PORTFOLIO),
        _ => None,
    }
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(self, "portfolio_create", ontology_anchor("portfolio_create"), async {
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_delete",
            ontology_anchor("portfolio_delete"),
            async {
                let response_name = name.clone();
                run_store(self.store.clone(), move |store| store.delete(&name)).await?;
                Ok(serde_json::json!({"status": "deleted", "name": response_name}))
            },
        )
        .await
    }

    #[tool(description = "List all portfolios in this owner's store.")]
    pub async fn portfolio_list(&self) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_list",
            ontology_anchor("portfolio_list"),
            async {
                let names = run_store(self.store.clone(), |store| store.list()).await?;
                Ok(serde_json::json!({"portfolios": names}))
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(self, "ledger_apply", ontology_anchor("ledger_apply"), async {
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(self, "ledger_read", ontology_anchor("ledger_read"), async {
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_snapshot",
            ontology_anchor("portfolio_snapshot"),
            async {
                // Validate the date up front — never silently epoch-substitute
                // (the SF-4 bug: a malformed date produced garbage projections
                // while callers reported success).
                parse_ymd(&date, "date").map_err(map_portfolio_error)?;
                let snapshot: HoldingsSnapshot = run_store(self.store.clone(), move |store| {
                    store.snapshot(&portfolio, &date)
                })
                .await?;
                let mut value = serde_json::to_value(&snapshot)
                    .map_err(|e| McpToolError::internal(format!("serialize snapshot: {e}")))?; // rr0044-ok: serialize-own-struct
                if let Some(obj) = value.as_object_mut() {
                    obj.insert(
                        "ontology".to_string(),
                        serde_json::Value::String(
                            hkask_bridge_ontology::fibo::PORTFOLIO.to_string(),
                        ),
                    );
                }
                Ok(value)
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_returns",
            ontology_anchor("portfolio_returns"),
            async {
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
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "ledger_import",
            ontology_anchor("ledger_import"),
            async {
                let ids = run_store(self.store.clone(), move |store| match format {
                    ImportFormat::Csv => import_csv(&store, &portfolio, asset_type, &data),
                    ImportFormat::Json => import_json(&store, &portfolio, asset_type, &data),
                })
                .await?;
                Ok(serde_json::json!({"status": "imported", "count": ids.len(), "ids": ids}))
            },
        )
        .await
    }

    #[tool(description = "Export a portfolio's ledger to CSV or JSON.")]
    pub async fn ledger_export(
        &self,
        Parameters(LedgerExportRequest { portfolio, format }): Parameters<LedgerExportRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "ledger_export",
            ontology_anchor("ledger_export"),
            async {
                let output_format = format.clone();
                let data = run_store(self.store.clone(), move |store| match format {
                    ImportFormat::Csv => export_csv(&store, &portfolio),
                    ImportFormat::Json => export_json(&store, &portfolio),
                })
                .await?;
                Ok(serde_json::json!({"format": output_format, "data": data}))
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_seed_price",
            ontology_anchor("portfolio_seed_price"),
            async {
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
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_roll",
            ontology_anchor("portfolio_roll"),
            async {
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
            },
        )
        .await
    }

    #[tool(
        description = "Rebuild all materialized views (daily holdings, daily returns) from the ledger. Use after a corruption or a bulk ledger edit."
    )]
    pub async fn portfolio_rebuild_views(
        &self,
        Parameters(PortfolioNameRequest { name }): Parameters<PortfolioNameRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_rebuild_views",
            ontology_anchor("portfolio_rebuild_views"),
            async {
                let response_name = name.clone();
                run_store(self.store.clone(), move |store| store.rebuild_views(&name)).await?;
                Ok(serde_json::json!({"status": "rebuilt", "portfolio": response_name}))
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_materialize_returns",
            ontology_anchor("portfolio_materialize_returns"),
            async {
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
            },
        )
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
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(
            self,
            "portfolio_daily_returns",
            ontology_anchor("portfolio_daily_returns"),
            async {
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
            },
        )
        .await
    }
}

#[tool_handler(router = Self::portfolio_router())]
impl rmcp::ServerHandler for PortfolioServer {}

// ── Entry point ─────────────────────────────────────────────────────

/// Run the portfolio MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Canonical storage route. Resolve the transactions directory (default
    // `{artifacts_dir}/portfolio-mcp/transactions/`, visible under
    // ~/Documents/zk-data/) and ensure it exists — users drop transaction
    // files here, and the directory must self-heal if a user accidentally
    // deletes or moves it.
    let transactions_dir = std::env::var("HKASK_TRANSACTIONS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            hkask_types::agent_paths::resolve_under_artifacts_dir(
                &hkask_types::agent_paths::mcp_artifacts_subdir("portfolio", "transactions"),
            )
            .to_string_lossy()
            .to_string()
        });
    if let Err(error) = std::fs::create_dir_all(&transactions_dir) {
        tracing::warn!(
            target: "hkask.mcp.portfolio",
            path = %transactions_dir,
            %error,
            "Failed to ensure transactions directory exists — transaction-file \
             imports will surface the failure"
        );
    }
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
