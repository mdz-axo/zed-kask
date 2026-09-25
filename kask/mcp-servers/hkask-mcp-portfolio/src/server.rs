//! hKask MCP Portfolio — MCP server entry point.
//!
//! Exposes the general-purpose [`PortfolioStore`] over MCP. Provider-agnostic:
//! no stock-price or contract-feed credentials. Consumers that need live
//! prices (the companies server) seed the price cache via
//! [`crate::CachedPriceResolver::seed_cache`] before calling `portfolio_returns`.

use crate::{
    AssetType, CachedPriceResolver, CharacteristicsReport, ClassificationObservation,
    HoldingsSnapshot, LedgerFilter, PortfolioError, PortfolioStore, PriceResolver, ReturnsReport,
    SecurityObservation, Transaction, attribution, characteristics, contribution, export_csv,
    export_json, historical_what_if, import_csv, import_json, parse_ymd, prospective_what_if,
    returns,
};
use hkask_mcp_server::server::{McpToolError, execute_tool, map_join_error};
use hkask_spreadsheet::{PublishOptions, SpreadsheetPublication, WorkbookService};
use hkask_types::spreadsheet::{
    AnalyticalTable, ArtifactOrigin, ColumnKind, SpreadsheetAccess, SpreadsheetError, TableColumn,
    TableValue,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

const PORTFOLIO_SERVER_ID: &str = "hkask-mcp-portfolio";

hkask_mcp_server::mcp_server!(
    pub struct PortfolioServer {
        pub store: PortfolioStore,
        /// The spreadsheet engine actor for what-if workbook publication — a
        /// per-instance dependency like the store, not a process global, so
        /// tests run against temp-dir artifact roots instead of the
        /// production tree.
        pub spreadsheet: Arc<WorkbookService>,
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
            McpToolError::internal(e.to_string())
        }
    }
}

/// Classify [`SpreadsheetError`] for MCP dispatch — the same per-variant
/// discipline as [`map_portfolio_error`] (mirrors hkask-mcp-spreadsheet's
/// mapper so the two surfaces classify identically).
pub fn map_spreadsheet_error(e: SpreadsheetError) -> McpToolError {
    match &e {
        SpreadsheetError::UnknownArtifact { .. } => McpToolError::not_found(e.to_string()),
        SpreadsheetError::Conflict { .. } => {
            McpToolError::new(hkask_types::McpErrorKind::FailedPrecondition, e.to_string())
        }
        SpreadsheetError::Engine { .. } => McpToolError::internal(e.to_string()),
        _ => McpToolError::invalid_argument(e.to_string()),
    }
}

/// The presentation choice for a what-if report (plan §6: callers
/// explicitly choose; no hidden mode switches). `DataOnly` keeps the
/// portfolio-viewer path; `WorkbookWhatIf` additionally publishes the
/// hypothetical transaction set and report deltas as an editable workbook
/// revision (an immutable ` ```spreadsheet ` block).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum WhatIfPresentation {
    #[default]
    DataOnly,
    WorkbookWhatIf,
}

/// Build the what-if staging workbook table: the hypothetical transaction
/// set followed by the before/after characteristic deltas, all values
/// computed from the portfolio-authoritative reports (the workbook consumes
/// them; it never reimplements the portfolio mathematics).
/// Build the what-if staging workbook table: the hypothetical transaction
/// set followed by the before/after characteristic deltas, all values
/// computed from the portfolio-authoritative reports (the workbook consumes
/// them; it never reimplements the portfolio mathematics). The transaction
/// summaries arrive pre-formatted from the caller (the transaction values
/// are moved into the store closure — one formatting site, no duplication).
fn what_if_workbook_table(
    portfolio: &str,
    date: &str,
    transaction_summaries: &[String],
    actual: &CharacteristicsReport,
    hypothetical: &CharacteristicsReport,
) -> AnalyticalTable {
    let delta_rows: [(String, TableValue, TableValue, TableValue); 6] = [
        (
            "Total market value".into(),
            TableValue::Number(actual.total_market_value),
            TableValue::Number(hypothetical.total_market_value),
            TableValue::Number(hypothetical.total_market_value - actual.total_market_value),
        ),
        (
            "Cash weight".into(),
            TableValue::Number(actual.cash_weight),
            TableValue::Number(hypothetical.cash_weight),
            TableValue::Number(hypothetical.cash_weight - actual.cash_weight),
        ),
        (
            "Position count".into(),
            TableValue::Number(actual.position_count as f64),
            TableValue::Number(hypothetical.position_count as f64),
            TableValue::Number(hypothetical.position_count as f64 - actual.position_count as f64),
        ),
        (
            "Top-five weight".into(),
            TableValue::Number(actual.top_five_weight),
            TableValue::Number(hypothetical.top_five_weight),
            TableValue::Number(hypothetical.top_five_weight - actual.top_five_weight),
        ),
        (
            "Concentration HHI".into(),
            TableValue::Number(actual.concentration_hhi),
            TableValue::Number(hypothetical.concentration_hhi),
            TableValue::Number(hypothetical.concentration_hhi - actual.concentration_hhi),
        ),
        (
            "Effective holdings".into(),
            TableValue::Number(actual.effective_holdings),
            TableValue::Number(hypothetical.effective_holdings),
            TableValue::Number(hypothetical.effective_holdings - actual.effective_holdings),
        ),
    ];
    let rows = transaction_summaries
        .iter()
        .map(|summary| {
            vec![
                TableValue::Text(format!("Hypothetical: {summary}")),
                TableValue::Empty,
                TableValue::Empty,
                TableValue::Empty,
            ]
        })
        .chain(delta_rows.into_iter().map(|(name, before, after, change)| {
            vec![TableValue::Text(name), before, after, change]
        }))
        .collect();
    AnalyticalTable::new(
        format!("What-if staging — {portfolio} as of {date}"),
        "What-if".into(),
        vec![
            TableColumn {
                id: "item".into(),
                label: "Item".into(),
                kind: ColumnKind::Text,
            },
            TableColumn {
                id: "before".into(),
                label: "Before".into(),
                kind: ColumnKind::Number,
            },
            TableColumn {
                id: "after".into(),
                label: "After".into(),
                kind: ColumnKind::Number,
            },
            TableColumn {
                id: "change".into(),
                label: "Change".into(),
                kind: ColumnKind::Number,
            },
        ],
        rows,
    )
    .expect("the what-if staging table is valid by construction")
}

/// Format a ```` ```spreadsheet ```` fenced display hint from a workbook
/// publication.
fn spreadsheet_hint(publication: SpreadsheetPublication) -> Result<String, McpToolError> {
    match publication {
        SpreadsheetPublication::Workbook { block, .. } => {
            let body = serde_json::to_string(&block).map_err(|error| {
                McpToolError::internal(format!("serialize spreadsheet block: {error}"))
            })?;
            Ok(format!("```spreadsheet\n{body}\n```"))
        }
        SpreadsheetPublication::Inline(_) => Err(McpToolError::internal(
            "workbook publication produced an inline table — not an editable what-if",
        )),
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

struct CombinedPriceResolver {
    primary: CachedPriceResolver,
    fallback: CachedPriceResolver,
}

impl PriceResolver for CombinedPriceResolver {
    fn resolve(&self, symbol: &str, date: &str) -> Option<f64> {
        self.primary
            .resolve(symbol, date)
            .or_else(|| self.fallback.resolve(symbol, date))
    }
}

/// Format a tool response carrying a server-authored ```` ```portfolio ````
/// report block, with optional additional fenced display hints appended to
/// the same `display_hint` string (the hint parsers scan for their own
/// fence, so one response can carry the portfolio report block AND a
/// ```` ```spreadsheet ```` workbook block).
fn report_response(
    portfolio: &str,
    report_kind: &str,
    report: serde_json::Value,
    provenance: serde_json::Value,
    extra_hints: Vec<String>,
) -> Result<serde_json::Value, McpToolError> {
    let block = serde_json::json!({
        "viz": "portfolio_report",
        "portfolio": portfolio,
        "report_kind": report_kind,
        "report": report,
        "provenance": provenance,
    });
    let body = serde_json::to_string(&block)
        .map_err(|error| McpToolError::internal(format!("serialize portfolio report: {error}")))?;
    let mut display_hint = format!("```portfolio\n{body}\n```");
    for extra in extra_hints {
        display_hint.push('\n');
        display_hint.push_str(&extra);
    }
    Ok(serde_json::json!({
        "report": block["report"],
        "display_hint": display_hint,
    }))
}

/// Map a tool name to its ontology concept URI for the `"ontology"` field
/// in the tool output JSON (read by the portfolio widget's "Explain"
/// affordance). Currently only `portfolio_snapshot` emits the field
/// (hardcoded `fibo:PORTFOLIO` at the call site); a per-tool anchor fn
/// was removed with the `reg.tool` span ontology tag (2026-08-29) — it
/// had no remaining consumer.

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
pub struct PortfolioContributionRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioCharacteristicsRequest {
    pub portfolio: String,
    pub date: String,
    #[serde(default)]
    pub observations: Vec<SecurityObservation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioAttributionRequest {
    pub portfolio: String,
    pub benchmark: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub classifications: Vec<ClassificationObservation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioWhatIfRequest {
    pub portfolio: String,
    pub date: String,
    pub hypothetical_transactions: Vec<Transaction>,
    #[serde(default)]
    pub observations: Vec<SecurityObservation>,
    /// The explicit presentation choice (§6): `DataOnly` (default) keeps the
    /// portfolio-viewer report; `WorkbookWhatIf` additionally publishes the
    /// transaction set and report deltas as an editable workbook revision.
    #[serde(default)]
    pub presentation: WhatIfPresentation,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioHistoricalWhatIfRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
    pub hypothetical_transactions: Vec<Transaction>,
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
    /// Single-seed form: one (symbol, date, close) triple. Prefer the
    /// `prices` array for multi-holding seeding — one call instead of N.
    pub symbol: Option<String>,
    pub date: Option<String>,
    pub close: Option<f64>,
    pub source: Option<String>,
    /// Batch form: seed many (symbol, date, close) observations in one
    /// call. When present, the single fields are ignored.
    pub prices: Option<Vec<PriceSeedEntry>>,
}

/// One price observation in a batch `portfolio_seed_price` call.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PriceSeedEntry {
    pub symbol: String,
    pub date: String,
    pub close: f64,
    /// Where the price came from (e.g. "fmp", "eodhd", "manual").
    pub source: Option<String>,
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_delete", async {
            let response_name = name.clone();
            run_store(self.store.clone(), move |store| store.delete(&name)).await?;
            Ok(serde_json::json!({"status": "deleted", "name": response_name}))
        })
        .await
    }

    #[tool(description = "List all portfolios in this owner's store.")]
    pub async fn portfolio_list(&self) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_snapshot", async {
            // Validate the date up front — never silently epoch-substitute
            // (the SF-4 bug: a malformed date produced garbage projections
            // while callers reported success).
            parse_ymd(&date, "date").map_err(map_portfolio_error)?;
            let snapshot: HoldingsSnapshot = run_store(self.store.clone(), move |store| {
                store.snapshot(&portfolio, &date)
            })
            .await?;
            let mut value = serde_json::to_value(&snapshot)
                .map_err(|e| McpToolError::internal(format!("serialize snapshot: {e}")))?;
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "ontology".to_string(),
                    serde_json::Value::String(hkask_bridge_ontology::fibo::PORTFOLIO.to_string()),
                );
            }
            Ok(value)
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
    ) -> Result<String, McpToolError> {
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
        description = "Decompose absolute portfolio profit into security contributions over a period. Contributions reconcile to portfolio return and include trades, commissions, and symbol-assigned dividends."
    )]
    pub async fn portfolio_contribution(
        &self,
        Parameters(PortfolioContributionRequest {
            portfolio,
            from,
            to,
        }): Parameters<PortfolioContributionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_contribution", async {
            let response_portfolio = portfolio.clone();
            let response_from = from.clone();
            let response_to = to.clone();
            let report = run_store(self.store.clone(), move |store| {
                let resolver = CachedPriceResolver::new(&store, &portfolio);
                contribution(&store, &portfolio, &from, &to, &resolver)
            })
            .await?;
            let report = serde_json::to_value(report)
                .map_err(|error| McpToolError::internal(format!("serialize contribution: {error}")))?;
            report_response(
                &response_portfolio,
                "contribution",
                report,
                serde_json::json!({
                    "tool": "portfolio_contribution",
                    "server": PORTFOLIO_SERVER_ID,
                    "args": {"portfolio": response_portfolio, "from": response_from, "to": response_to},
                }),
                Vec::new(),
            )
        })
        .await
    }

    #[tool(
        description = "Analyze current portfolio composition, concentration, classifications, and supplied company metrics. Valuation multiples use harmonic aggregation; other supported metrics use market-value-weighted arithmetic aggregation. Every metric reports data coverage."
    )]
    pub async fn portfolio_characteristics(
        &self,
        Parameters(PortfolioCharacteristicsRequest {
            portfolio,
            date,
            observations,
        }): Parameters<PortfolioCharacteristicsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_characteristics", async {
            let response_portfolio = portfolio.clone();
            let response_date = date.clone();
            let report = run_store(self.store.clone(), move |store| {
                let resolver = CachedPriceResolver::new(&store, &portfolio);
                characteristics(&store, &portfolio, &date, &resolver, &observations)
            })
            .await?;
            let report = serde_json::to_value(report).map_err(|error| {
                McpToolError::internal(format!("serialize characteristics: {error}"))
            })?;
            report_response(
                &response_portfolio,
                "characteristics",
                report,
                serde_json::json!({
                    "tool": "portfolio_characteristics",
                    "server": PORTFOLIO_SERVER_ID,
                    "args": {"portfolio": response_portfolio, "date": response_date},
                }),
                Vec::new(),
            )
        })
        .await
    }

    #[tool(
        description = "Decompose benchmark-relative active return with the Brinson-Fachler model into allocation, selection, and separately reported interaction effects. The benchmark is another portfolio in the same ledger store; missing classifications are shown as Unclassified."
    )]
    pub async fn portfolio_attribution(
        &self,
        Parameters(PortfolioAttributionRequest {
            portfolio,
            benchmark,
            from,
            to,
            classifications,
        }): Parameters<PortfolioAttributionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_attribution", async {
            let response_portfolio = portfolio.clone();
            let response_benchmark = benchmark.clone();
            let response_from = from.clone();
            let response_to = to.clone();
            let report = run_store(self.store.clone(), move |store| {
                let resolver = CombinedPriceResolver {
                    primary: CachedPriceResolver::new(&store, &portfolio),
                    fallback: CachedPriceResolver::new(&store, &benchmark),
                };
                attribution(
                    &store,
                    &portfolio,
                    &benchmark,
                    &from,
                    &to,
                    &resolver,
                    &classifications,
                )
            })
            .await?;
            let report = serde_json::to_value(report)
                .map_err(|error| McpToolError::internal(format!("serialize attribution: {error}")))?;
            report_response(
                &response_portfolio,
                "attribution",
                report,
                serde_json::json!({
                    "tool": "portfolio_attribution",
                    "server": PORTFOLIO_SERVER_ID,
                    "args": {"portfolio": response_portfolio, "benchmark": response_benchmark, "from": response_from, "to": response_to},
                }),
                Vec::new(),
            )
        })
        .await
    }

    #[tool(
        description = "Compare current portfolio characteristics with a hypothetical set of same-date stock buys and sells. The calculation uses cloned ledger state and never changes the authoritative portfolio."
    )]
    pub async fn portfolio_what_if(
        &self,
        Parameters(PortfolioWhatIfRequest {
            portfolio,
            date,
            hypothetical_transactions,
            observations,
            presentation,
        }): Parameters<PortfolioWhatIfRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_what_if", async {
            let response_portfolio = portfolio.clone();
            let response_date = date.clone();
            let transaction_summaries: Vec<String> = hypothetical_transactions
                .iter()
                .map(|transaction| {
                    match (&transaction.symbol, transaction.quantity, transaction.price) {
                        (Some(symbol), Some(quantity), Some(price)) => {
                            format!(
                                "{} {} {} @ {:.2}",
                                transaction.tx_type, quantity, symbol, price
                            )
                        }
                        (Some(symbol), Some(quantity), None) => {
                            format!("{} {} {}", transaction.tx_type, quantity, symbol)
                        }
                        _ => format!("{}", transaction.tx_type),
                    }
                })
                .collect();
            let (actual, hypothetical) = run_store(self.store.clone(), move |store| {
                let resolver = CachedPriceResolver::new(&store, &portfolio);
                prospective_what_if(
                    &store,
                    &portfolio,
                    &date,
                    &hypothetical_transactions,
                    &resolver,
                    &observations,
                )
            })
            .await?;
            let report = serde_json::json!({
                "mode": "prospective_composition",
                "date": response_date,
                "actual": actual,
                "hypothetical": hypothetical,
                "authoritative_state_changed": false,
            });
            // The explicit presentation choice (§6): publish the what-if
            // staging workbook as an immutable revision and append its
            // ```spreadsheet hint to the portfolio report hint.
            let extra_hints = if presentation == WhatIfPresentation::WorkbookWhatIf {
                let origin = ArtifactOrigin::new(
                    PORTFOLIO_SERVER_ID.to_string(),
                    "portfolio_what_if".to_string(),
                    serde_json::json!({"portfolio": response_portfolio, "date": response_date}),
                )
                .map_err(map_spreadsheet_error)?;
                let table = what_if_workbook_table(
                    &response_portfolio,
                    &response_date,
                    &transaction_summaries,
                    &actual,
                    &hypothetical,
                );
                let publication = self
                    .spreadsheet
                    .publish(
                        origin,
                        table,
                        PublishOptions {
                            access: SpreadsheetAccess::WorkbookWhatIf,
                        },
                    )
                    .await
                    .map_err(map_spreadsheet_error)?;
                vec![spreadsheet_hint(publication)?]
            } else {
                Vec::new()
            };
            report_response(
                &response_portfolio,
                "what_if",
                report,
                serde_json::json!({
                    "tool": "portfolio_what_if",
                    "server": PORTFOLIO_SERVER_ID,
                    "args": {"portfolio": response_portfolio},
                }),
                extra_hints,
            )
        })
        .await
    }

    #[tool(
        description = "Run a retrospective historical counterfactual by adding dated hypothetical stock buys and sells to cloned ledger state. Compares realized terminal value and return with the actual portfolio, never mutates the ledger, and labels the result as hindsight rather than an ex-ante forecast."
    )]
    pub async fn portfolio_historical_what_if(
        &self,
        Parameters(PortfolioHistoricalWhatIfRequest {
            portfolio,
            from,
            to,
            hypothetical_transactions,
        }): Parameters<PortfolioHistoricalWhatIfRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_historical_what_if", async {
            let response_portfolio = portfolio.clone();
            let response_from = from.clone();
            let response_to = to.clone();
            let report = run_store(self.store.clone(), move |store| {
                let resolver = CachedPriceResolver::new(&store, &portfolio);
                historical_what_if(
                    &store,
                    &portfolio,
                    &from,
                    &to,
                    &hypothetical_transactions,
                    &resolver,
                )
            })
            .await?;
            let report = serde_json::to_value(report).map_err(|error| {
                McpToolError::internal(format!("serialize historical what-if: {error}"))
            })?;
            report_response(
                &response_portfolio,
                "historical_what_if",
                report,
                serde_json::json!({
                    "tool": "portfolio_historical_what_if",
                    "server": PORTFOLIO_SERVER_ID,
                    "args": {"portfolio": response_portfolio, "from": response_from, "to": response_to},
                }),
                Vec::new(),
            )
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
    ) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
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
        description = "Seed the price cache for one (symbol, date, close) triple or a batch of them (the prices array — one call instead of N). Call before portfolio_returns for portfolios whose holdings have market prices; the resolver is as-of, so seeding the from/to dates carries each price forward. Materialized views are invalidated from each seeded date forward."
    )]
    pub async fn portfolio_seed_price(
        &self,
        Parameters(PriceSeedRequest {
            portfolio,
            symbol,
            date,
            close,
            source,
            prices,
        }): Parameters<PriceSeedRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_seed_price", async {
            let resolver = CachedPriceResolver::new(&self.store, &portfolio);
            let entries: Vec<(String, String, f64, String)> = if let Some(batch) = prices {
                batch
                    .into_iter()
                    .map(|entry| {
                        (
                            entry.symbol,
                            entry.date,
                            entry.close,
                            entry.source.unwrap_or_else(|| "batch".to_string()),
                        )
                    })
                    .collect()
            } else {
                let symbol = symbol.ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "provide either the single (symbol, date, close) fields or the prices array",
                    )
                })?;
                let date = date.ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "provide either the single (symbol, date, close) fields or the prices array",
                    )
                })?;
                let close = close.ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "provide either the single (symbol, date, close) fields or the prices array",
                    )
                })?;
                vec![(
                    symbol,
                    date,
                    close,
                    source.unwrap_or_else(|| "manual".to_string()),
                )]
            };
            if entries.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "prices array must not be empty",
                ));
            }
            let mut seeded = Vec::with_capacity(entries.len());
            for (symbol, date, close, source) in &entries {
                resolver
                    .seed_cache(symbol, date, *close, source)
                    .map_err(map_portfolio_error)?;
                seeded.push(serde_json::json!({
                    "symbol": symbol,
                    "date": date,
                    "close": close,
                    "source": source,
                }));
            }
            Ok(serde_json::json!({
                "status": "seeded",
                "portfolio": portfolio,
                "seeded_count": seeded.len(),
                "seeded": seeded,
                "note": "Materialized views are invalidated from each seeded date forward; recompute returns/materialize after seeding.",
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
    ) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
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
    ) -> Result<String, McpToolError> {
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
    // The spreadsheet engine actor for what-if workbook publication, rooted
    // at the production spreadsheet artifact tree. A per-instance dependency
    // (like the store below), moved into the server through the factory —
    // not a process global, so tests inject their own temp-dir root.
    let spreadsheet =
        WorkbookService::start_with_root(hkask_spreadsheet::artifact_store::production_root())
            .map_err(|error| {
                hkask_mcp_server::McpError::Infrastructure(hkask_types::InfrastructureError::Io(
                    error.to_string(),
                ))
            })?;
    hkask_mcp_server::run_server(
        "hkask-mcp-portfolio",
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::ServerContext| {
            Ok(PortfolioServer::new(
                ctx.webid,
                PortfolioStore::new(ctx.webid)?,
                spreadsheet,
            ))
        },
        vec![],
    )
    .await
}
