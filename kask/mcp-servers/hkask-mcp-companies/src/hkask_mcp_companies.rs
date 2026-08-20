#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Companies — Dual-provider company financial data (FMP + EODHD)
//!
//! Tools are provider-agnostic: each tool routes to FMP or EODHD based on
//! symbol characteristics, with automatic fallback. EODHD responses are
//! normalized to match FMP format so analysis functions work transparently.
//!
//! ## Tools (45) — pinned by `tool_surface_is_exactly_44_registered_tools`
//!
//! Tools are split across submodules under `src/tools/`, each with its own
//! `#[tool_router]` block, merged in `combined_router()`:
//! - `tools/financial_data.rs` — company_profile, stock_quote, income_statement,
//!   balance_sheet, cash_flow_statement, key_metrics, historical_price, symbol_search
//! - `tools/analysis.rs` — moat_check, management_scorecard, working_capital_cycle
//! - `tools/valuation.rs` — dcf_valuation, reverse_dcf, ep_valuation, comparable_analysis,
//!   scenario_analysis, sensitivity_analysis, monte_carlo_dcf, scenario_impact_valuation,
//!   calibrate_forecast, forecast_record, forecast_persist
//! - `tools/analytics.rs` — portfolio_attribution, portfolio_characteristics,
//!   portfolio_comparison, portfolio_returns
//! - `tools/economic_profit.rs` — ep_valuation (economic profit view)
//! - `tools/expectations.rs` — expectations_gap
//! - `tools/portfolio.rs` — ledger_import, ledger_export, portfolio_list,
//!   portfolio_delete, transaction_note_append, note_add, note_list, note_delete,
//!   file_attach, file_list, file_delete
//! - `tools/transcript.rs` — earnings-call transcript tools
//! - `tools/screener.rs` — stock_screener, research_search
//!
//! The pin test is the source of truth for the count; this list is a map.
//!
//! ## Data quality framework (FinGPT §3.2)
//! - Regulation `data_quality` spans on every valuation tool — staleness, CV, confidence
//! - `SignalQuality` on DCF/scenario outputs — outlier flags, cyclicality detection
//! - `LearningState` temporal coherence — RLSP-style market signal feedback
//! - `ResearchClaimClassifier` — category tagging, numeric extraction, ticker detection
//! - Treasury stock adjustment — hKask non-standard: TS treated as committed capital
//!
//! ## FIBO anchoring
//! Balance sheet items under `fibo-fbc-pas-fpas`, ratios under `fibo-fbc-fct-ra`,
//! securities under `fibo-sec-sec-ast`, indices under `fibo-ind-ind-ind`.

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{McpToolError, map_join_error, validate_identifier};

pub mod aggregation;
mod analysis;
pub mod data_quality;
pub mod economic_profit;
pub mod fibo;
mod financial_model;
pub mod portfolio;
mod providers;
pub use providers::{CompanyProfile, HistoricalPriceView, KeyMetrics, Provider};
mod forecast;
pub mod learning;
pub mod research;
mod scenarios;
mod screener;
pub mod superforecast;
mod transcript;
mod valuation_service;
pub(crate) use forecast::{
    StoredForecast, current_price_from_multiple, projected_terminal_multiple,
};
pub mod types;

use portfolio::{PersistedForecast, PortfolioManager};

pub mod tools;
pub use transcript::{MissingReason, TranscriptCoverage, TranscriptRecord, TranscriptResult};

// ── Validation ──────────────────────────────────────────────────────

fn validate_symbol(symbol: &str) -> Result<(), McpToolError> {
    // Allow exchange-qualified symbols (e.g., VOD.L, BMW.DE) for EODHD
    validate_identifier("symbol", symbol, 32)
}

/// Extract a symbol from a query string for learning state tracking.
/// Handles: "symbol=AAPL", "symbol=VOD.L", "query=..." (search queries).
fn parse_symbol_from_query(query: &str) -> Option<String> {
    if let Some(sym) = query.strip_prefix("symbol=") {
        let sym = sym.split('&').next().unwrap_or(sym);
        if !sym.is_empty() {
            return Some(sym.to_string());
        }
    }
    // For symbol_search, the query IS the search term — use it directly.
    if let Some(q) = query.strip_prefix("query=")
        && !q.is_empty()
    {
        return Some(q.to_string());
    }
    None
}

// ── Server struct ──────────────────────────────────────────────────

use learning::LearningState;

hkask_mcp_server::mcp_server!(
    pub struct CompaniesServer {
        pub client: reqwest::Client,
        pub fmp_api_key: String,
        pub eodhd_api_key: String,
        pub exa_api_key: Option<String>,
        pub tavily_api_key: Option<String>,
        pub brave_api_key: Option<String>,
        pub serpapi_key: Option<String>,
        pub portfolio: PortfolioManager,
        pub learning: std::sync::Arc<std::sync::Mutex<LearningState>>,
        pub fermi_defaults: superforecast::FermiDefaults,
    }
);

use hkask_mcp_portfolio::map_portfolio_error;

impl CompaniesServer {
    async fn fetch(
        &self,
        tool: &str,
        symbol: &str,
        extra: &[(&str, &str)],
    ) -> Result<serde_json::Value, McpToolError> {
        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        providers::companies_get(
            &self.client,
            tool,
            symbol,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            extra,
            Some(&l),
        )
        .await
    }

    /// Fetch a company profile as a typed `CompanyProfile` view. Concentrates
    /// field-name knowledge so tool handlers read `profile.market_cap()`
    /// instead of `v.get("mktCap").and_then(|v| v.as_f64())`.
    async fn fetch_profile(&self, symbol: &str) -> Result<CompanyProfile, McpToolError> {
        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        providers::fetch_company_profile(
            &self.client,
            symbol,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            Some(&l),
        )
        .await
    }

    /// Fetch key metrics as a typed `KeyMetrics` view.
    async fn fetch_key_metrics(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<KeyMetrics, McpToolError> {
        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        providers::fetch_key_metrics(
            &self.client,
            symbol,
            limit,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            Some(&l),
        )
        .await
    }

    /// Fetch historical prices as a typed `HistoricalPriceView` view.
    async fn fetch_historical_price(
        &self,
        symbol: &str,
        from: &str,
        to: &str,
    ) -> Result<HistoricalPriceView, McpToolError> {
        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        providers::fetch_historical_price(
            &self.client,
            symbol,
            from,
            to,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            Some(&l),
        )
        .await
    }

    async fn save_forecast(&self, forecast: PersistedForecast) -> Result<(), McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.save_forecast(&forecast))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn get_persisted_forecast(
        &self,
        forecast_id: String,
    ) -> Result<Option<PersistedForecast>, McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.get_forecast(&forecast_id))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn list_persisted_forecasts(
        &self,
        symbol: String,
    ) -> Result<Vec<PersistedForecast>, McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.list_forecasts(&symbol))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn record_persisted_forecast_outcome(
        &self,
        forecast_id: String,
        outcome: serde_json::Value,
    ) -> Result<(), McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || {
            portfolio.record_forecast_outcome(&forecast_id, outcome)
        })
        .await
        .map_err(|error| map_join_error(error, "forecast task failed"))?
        .map_err(map_portfolio_error)
    }
}

// ── Combined tool router ───────────────────────────────────────────

impl CompaniesServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::financial_data_router()
            + Self::analysis_router()
            + Self::portfolio_router()
            + Self::analytics_router()
            + Self::valuation_router()
            + Self::economic_profit_router()
            + Self::expectations_router()
            + Self::transcript_router()
    }

    /// Map a tool name to its ontology concept URI. The concept is used both
    /// as the `reg.tool.*` span ontology tag (via `execute_tool_semantic`)
    /// and as the `"ontology"` field in the tool output JSON (via
    /// `fibo::enrich_with_ontology`). Delegates to `fibo::tool_to_ontology` —
    /// the single source of truth for the tool → concept mapping.
    fn ontology_anchor(tool: &str) -> Option<&'static str> {
        fibo::tool_to_ontology(tool)
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for CompaniesServer {}

// ── Entry point ─────────────────────────────────────────────────────

/// Run the companies MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-companies",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            let fmp_api_key = ctx
                .credentials
                .get("HKASK_FMP_API_KEY")
                .ok_or_else(|| hkask_mcp_server::McpError::MissingCredentials {
                    missing: "HKASK_FMP_API_KEY".to_string(),
                })?
                .clone();
            let eodhd_api_key = ctx
                .credentials
                .get("HKASK_EODHD_API_KEY")
                .ok_or_else(|| hkask_mcp_server::McpError::MissingCredentials {
                    missing: "HKASK_EODHD_API_KEY".to_string(),
                })?
                .clone();
            let exa_api_key = ctx.credentials.get("HKASK_EXA_API_KEY").cloned();
            let tavily_api_key = ctx.credentials.get("HKASK_TAVILY_API_KEY").cloned();
            let brave_api_key = ctx.credentials.get("HKASK_BRAVE_API_KEY").cloned();
            // `HKASK_SERPAPI_API_KEY` — the canonical spelling used by kask/.env,
            // the credential registry (inference_providers.rs) and the research
            // server. This read used the shorter `HKASK_SERPAPI_KEY`, which
            // appeared in no allowlist and no credential registry, so the key
            // could never arrive and corpus-mode transcript search was
            // permanently unavailable (RR-0061).
            let serpapi_key = ctx.credentials.get("HKASK_SERPAPI_API_KEY").cloned();
            Ok(CompaniesServer::new(
                ctx.webid,
                reqwest::Client::new(),
                fmp_api_key,
                eodhd_api_key,
                exa_api_key,
                tavily_api_key,
                brave_api_key,
                serpapi_key,
                PortfolioManager::new(ctx.webid)?,
                std::sync::Arc::new(std::sync::Mutex::new({
                    let days = hkask_mcp_server::parse_env_warn(
                        "HKASK_CHRONIC_STALENESS_DAYS",
                        LearningState::default().staleness_days(),
                    );
                    LearningState::with_staleness_days(days)
                })),
                superforecast::FermiDefaults::from_env(),
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::required(
                "HKASK_FMP_API_KEY",
                "Financial Modeling Prep API key",
            ),
            hkask_mcp_server::CredentialRequirement::required(
                "HKASK_EODHD_API_KEY",
                "EOD Historical Data (EODHD) API key",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_EXA_API_KEY",
                "Exa API key for fundamental research search",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_TAVILY_API_KEY",
                "Tavily API key for fundamental research search",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_BRAVE_API_KEY",
                "Brave Search API key for fundamental research search",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_SERPAPI_API_KEY",
                "SerpAPI key for corpus-mode transcript search",
            ),
        ],
    )
    .await
}

// ── Tracer-bullet contracts ───────────────────────────────────────
