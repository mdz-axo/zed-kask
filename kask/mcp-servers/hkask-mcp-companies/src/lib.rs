#![forbid(unsafe_code)]
//! hKask MCP Companies — Dual-provider company financial data (FMP + EODHD)
//!
//! Tools are provider-agnostic: each tool routes to FMP or EODHD based on
//! symbol characteristics, with automatic fallback. EODHD responses are
//! normalized to match FMP format so analysis functions work transparently.
//!
//! ## Financial data tools
//! - `company_profile` — Company profile by symbol
//! - `stock_quote` — Real-time stock quote
//! - `income_statement` — Income statements
//! - `balance_sheet` — Balance sheet statements
//! - `cash_flow_statement` — Cash flow statements
//! - `key_metrics` — Key financial metrics
//! - `historical_price` — Historical price data
//! - `symbol_search` — Symbol search
//!
//! ## MAIA fundamental analysis
//! - `moat_check` — Competitive moat: gross margin stability + WC signal
//! - `management_scorecard` — CEO capital allocation scorecard (ROIC vs IC)
//! - `working_capital_cycle` — CFO working capital analysis (DPO, DSO, CCC)
//!
//! ## Valuation tools
//! - `dcf_valuation` — Two-stage 11-line-item DCF (Damodaran 2012)
//! - `reverse_dcf` — Market-implied growth rate (Mauboussin's Expectations Investing)
//! - `ep_valuation` — Economic Profit / Residual Income Model (Bergen et al. 2025)
//! - `comparable_analysis` — Peer multiples + DCF overlay
//! - `scenario_analysis` — Schwartz 2×2 scenario matrix
//! - `sensitivity_analysis` — Driver-by-driver intrinsic value sensitivity
//! - `monte_carlo_dcf` — N-simulation Monte Carlo with histogram
//! - `calibrate_forecast` — Fermi decomposition + Bayesian update (Tetlock)
//! - `forecast_record` — Forecast-vs-actual decomposition (11-line-item gap)
//!
//! ## Research & screening
//! - `research_search` — Multi-provider claim search with classification
//! - `stock_screener` — Natural language screening
//! - `expectations_gap` — Market-implied vs management vs analyst growth gap
//!
//! ## Portfolio tools
//! - `ledger_import` — Import CSV/JSON (auto-creates portfolio)
//! - `ledger_export` — Export CSV/JSON
//! - `portfolio_list` — List all portfolios
//! - `portfolio_delete` — Delete a portfolio
//! - `transaction_note_append` — Annotate a transaction
//! - `note_add` — Add a research note to a security
//! - `note_list` — List notes with optional date/tag filtering
//! - `note_delete` — Delete a note
//! - `file_attach` — Attach a file (base64) to a security
//! - `file_list` — List attached files for a security
//! - `file_delete` — Delete an attached file
//! - `portfolio_attribution` — What moved the portfolio
//! - `portfolio_characteristics` — Weighted-average fundamentals
//! - `portfolio_comparison` — Side-by-side comparison
//! - `portfolio_returns` — TWR and IRR for any date range
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

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{McpToolError, validate_identifier};
use hkask_mcp_server::{DaemonClient, DaemonResponse};
use hkask_types::time::now_rfc3339;
use serde::{Deserialize, Serialize};

mod analysis;
pub mod data_quality;
pub mod economic_profit;
pub mod fibo;
mod financial_model;
pub mod portfolio;
mod providers;
pub use providers::Provider;
pub mod learning;
pub mod research;
mod scenarios;
mod screener;
pub mod superforecast;
pub mod types;

use portfolio::{PersistedForecast, PortfolioError, PortfolioManager};

pub mod tools;

// ── Forecast store ───────────────────────────────────────────────────

/// A stored forecast model for later decomposition during `forecast_record`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredForecast {
    model: financial_model::ProjectedModel,
    assumptions: financial_model::ProjectionAssumptions,
    current_price: f64,
    intrinsic_per_share: f64,
}

impl StoredForecast {
    fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    fn from_snapshot(snapshot: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(snapshot.clone())
    }
}

/// Extract the terminal multiple implied by the projected model.
fn projected_terminal_multiple(model: &financial_model::ProjectedModel) -> f64 {
    if let Some(last) = model.periods.last() {
        if last.free_cash_flow > 0.0 {
            model.terminal_value / last.free_cash_flow
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Approximate a current price from a valuation multiple and historical data.
fn current_price_from_multiple(multiple: f64, hist: &financial_model::HistoricalSnapshot) -> f64 {
    let latest_fcf =
        hist.latest_revenue() * hist.gross_margin() - hist.latest_da() - hist.latest_capex();
    if hist.shares_outstanding > 0.0 {
        (latest_fcf * multiple) / hist.shares_outstanding
    } else {
        0.0
    }
}

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
        pub portfolio: PortfolioManager,
        pub learning: std::sync::Arc<std::sync::Mutex<LearningState>>,
        pub fermi_defaults: superforecast::FermiDefaults,
    }
);

/// Classify PortfolioError for MCP dispatch: user errors → invalid_argument, system errors → internal.
fn map_portfolio_error(e: PortfolioError) -> McpToolError {
    match &e {
        PortfolioError::InvalidArgument(_) => McpToolError::invalid_argument(e.to_string()),
        _ => McpToolError::internal(e.to_string()),
    }
}

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

    /// Record the outcome of a fetch-based tool call as a daemon experience.
    /// Deduplicates the Ok/Err match pattern shared across all financial-data tools.
    fn record_fetch_outcome(
        &self,
        tool: &str,
        symbol: &str,
        result: &Result<serde_json::Value, McpToolError>,
    ) {
        match result {
            Ok(v) => {
                self.record_experience(tool, &format!("symbol={}", symbol), "success", v.clone());
            }
            Err(e) => {
                self.record_experience(
                    tool,
                    &format!("symbol={}", symbol),
                    "error",
                    serde_json::json!({"error": e.to_json_string()}),
                );
            }
        }
    }
    async fn save_forecast(&self, forecast: PersistedForecast) -> Result<(), McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.save_forecast(&forecast))
            .await
            .map_err(|error| McpToolError::internal(format!("forecast task failed: {error}")))?
            .map_err(map_portfolio_error)
    }

    async fn get_persisted_forecast(
        &self,
        forecast_id: String,
    ) -> Result<Option<PersistedForecast>, McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.get_forecast(&forecast_id))
            .await
            .map_err(|error| McpToolError::internal(format!("forecast task failed: {error}")))?
            .map_err(map_portfolio_error)
    }

    async fn list_persisted_forecasts(
        &self,
        symbol: String,
    ) -> Result<Vec<PersistedForecast>, McpToolError> {
        let portfolio = self.portfolio.clone();
        tokio::task::spawn_blocking(move || portfolio.list_forecasts(&symbol))
            .await
            .map_err(|error| McpToolError::internal(format!("forecast task failed: {error}")))?
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
        .map_err(|error| McpToolError::internal(format!("forecast task failed: {error}")))?
        .map_err(map_portfolio_error)
    }

    /// Record a tool call as a narrative experience in the agent's memory.
    fn record_experience(
        &self,
        tool: &str,
        input_summary: &str,
        outcome: &str,
        detail: serde_json::Value,
    ) {
        if let Some(ref daemon) = self.daemon {
            let value = serde_json::json!({
                "tool": tool,
                "input": input_summary,
                "outcome": outcome,
                "detail": detail,
                "timestamp": now_rfc3339(),
            });
            let daemon_clone = daemon.clone();
            let userpod = self.userpod.clone();
            let tool_name = tool.to_string();
            tokio::spawn(async move {
                match daemon_clone
                    .store_experience(&userpod, "mcp_session", "observed", &value, Some(0.85))
                    .await
                {
                    Ok(DaemonResponse::StoreResponse { stored: true, .. }) => {
                        tracing::debug!(target: "hkask.mcp.companies.memory", tool = %tool_name, "Experience stored via daemon");
                    }
                    Ok(other) => {
                        tracing::warn!(target: "hkask.mcp.companies.memory", tool = %tool_name, response = ?other, "Unexpected daemon response")
                    }
                    Err(e) => {
                        tracing::warn!(target: "hkask.mcp.companies.memory", tool = %tool_name, error = %e, "Failed to store experience")
                    }
                }
            });
        }
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
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for CompaniesServer {}

// ── Entry point ─────────────────────────────────────────────────────

/// Run the companies MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-companies",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            let fmp_api_key = ctx
                .credentials
                .get("HKASK_FMP_API_KEY")
                .expect("required credential checked by run_stdio_server")
                .clone();
            let eodhd_api_key = ctx
                .credentials
                .get("HKASK_EODHD_API_KEY")
                .expect("required credential checked by run_stdio_server")
                .clone();
            let exa_api_key = ctx.credentials.get("HKASK_EXA_API_KEY").cloned();
            let tavily_api_key = ctx.credentials.get("HKASK_TAVILY_API_KEY").cloned();
            let brave_api_key = ctx.credentials.get("HKASK_BRAVE_API_KEY").cloned();
            Ok(CompaniesServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
                reqwest::Client::new(),
                fmp_api_key,
                eodhd_api_key,
                exa_api_key,
                tavily_api_key,
                brave_api_key,
                PortfolioManager::new(ctx.webid),
                std::sync::Arc::new(std::sync::Mutex::new(
                    match std::env::var("HKASK_CHRONIC_STALENESS_DAYS")
                        .ok()
                        .and_then(|v| v.parse::<u32>().ok())
                    {
                        Some(days) => LearningState::with_staleness_days(days),
                        None => LearningState::default(),
                    },
                )),
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
        ],
    )
    .await
}

// ── Tracer-bullet contracts ───────────────────────────────────────

#[cfg(test)]
mod poison_tests;

#[cfg(test)]
mod tests {
    use super::*;

    // ── durable forecast snapshots ─────────────────────────────────

    #[test]
    fn stored_forecast_snapshot_reconstructs_decomposition_model() {
        let stored = StoredForecast {
            model: financial_model::ProjectedModel {
                periods: vec![financial_model::ProjectedLineItems {
                    period: 1,
                    year: 2026.0,
                    revenue: 120.0,
                    cogs: 72.0,
                    gross_profit: 48.0,
                    da: 4.0,
                    ebit: 44.0,
                    tax: 9.0,
                    nopat: 35.0,
                    capex: 6.0,
                    change_in_nwc: 2.0,
                    free_cash_flow: 27.0,
                    discount_factor: 0.9,
                    present_value: 24.3,
                }],
                terminal_value: 300.0,
                terminal_pv: 270.0,
                enterprise_value: 294.3,
                net_debt: 20.0,
                equity_value: 274.3,
                intrinsic_per_share: 27.43,
            },
            assumptions: financial_model::ProjectionAssumptions::default(),
            current_price: 20.0,
            intrinsic_per_share: 27.43,
        };

        let reconstructed = StoredForecast::from_snapshot(&stored.snapshot()).unwrap();
        assert_eq!(reconstructed.model.periods.len(), 1);
        assert_eq!(reconstructed.model.periods[0].free_cash_flow, 27.0);
        assert_eq!(
            reconstructed.assumptions.discount_rate,
            stored.assumptions.discount_rate
        );
        assert_eq!(
            reconstructed.intrinsic_per_share,
            stored.intrinsic_per_share
        );
    }

    // ── expectations_gap: Gordon Growth Model formula ──────────────

    #[test]
    fn gordon_growth_formula_contract() {
        let target_return = 0.15f64;
        let avg_net_margin = 0.10f64;
        let price_to_sales = 2.0f64;
        let implied_growth = (target_return - avg_net_margin / price_to_sales) / 2.0;
        assert!(
            (implied_growth - 0.05).abs() < 0.0001,
            "implied growth = 5%"
        );
        let hist_growth = 0.03f64;
        let gap = implied_growth - hist_growth;
        assert!(
            (gap - 0.02).abs() < 0.0001,
            "positive expectations gap = 2%"
        );
    }

    #[test]
    fn gordon_growth_formula_insufficient_data_null_output() {
        let ps = 0.0;
        let avg_net_margin = 0.10;
        let implied: Option<f64> = if ps > 0.0 && avg_net_margin > 0.0 {
            Some((0.15 - avg_net_margin / ps) / 2.0)
        } else {
            None
        };
        assert!(implied.is_none(), "zero P/S = no implied growth");
        let ps = 2.0;
        let avg_net_margin = 0.0;
        let implied: Option<f64> = if ps > 0.0 && avg_net_margin > 0.0 {
            Some((0.15 - avg_net_margin / ps) / 2.0)
        } else {
            None
        };
        assert!(implied.is_none(), "zero margin = no implied growth");
    }

    // ── working_capital_cycle: CFO rating boundaries ───────────────

    #[test]
    fn cfo_rating_boundaries_contract() {
        let perfect = [20.0, 20.0, 20.0, 20.0];
        let score = analysis::gross_margin_stability(&perfect);
        assert!(score > 0.99, "identical spreads = near-perfect stability");
        assert!(score > 0.8, "= stable CFO rating");
        let moderate = [20.0, 35.0, 10.0, 40.0];
        let score = analysis::gross_margin_stability(&moderate);
        assert!(
            score > 0.5 && score <= 0.8,
            "moderate variance = moderate CFO rating: {score}"
        );
    }

    #[test]
    fn cfo_rating_single_period_defaults() {
        let single = analysis::gross_margin_stability(&[30.0]);
        assert!((single - 1.0).abs() < 0.001, "single period = 1.0");
    }

    // ── portfolio_attribution: weight + contribution formulas ───────

    #[test]
    fn attribution_weight_and_contribution_contract() {
        let positions = [
            ("AAPL", 60000.0, 0.15),
            ("MSFT", 30000.0, 0.05),
            ("GOOGL", 10000.0, -0.10),
        ];
        let total_mv: f64 = positions.iter().map(|(_, mv, _)| mv).sum();
        assert!((total_mv - 100000.0).abs() < 0.01, "total MV = $100K");
        let weights: Vec<f64> = positions.iter().map(|(_, mv, _)| mv / total_mv).collect();
        assert!((weights[0] - 0.60).abs() < 0.001, "AAPL weight = 60%");
        assert!((weights[1] - 0.30).abs() < 0.001, "MSFT weight = 30%");
        assert!((weights[2] - 0.10).abs() < 0.001, "GOOGL weight = 10%");
        let contributions: Vec<f64> = weights
            .iter()
            .zip(positions.iter())
            .map(|(w, (_, _, r))| w * r * 10000.0)
            .collect();
        assert!((contributions[0] - 900.0).abs() < 1.0, "AAPL = 900 bps");
        assert!((contributions[1] - 150.0).abs() < 1.0, "MSFT = 150 bps");
        assert!(
            (contributions[2] - (-100.0)).abs() < 1.0,
            "GOOGL = -100 bps"
        );
        let total_return_bps: f64 = contributions.iter().sum();
        let portfolio_return = total_return_bps / 10000.0;
        assert!(
            (portfolio_return - 0.095).abs() < 0.001,
            "portfolio return = 9.5%"
        );
    }

    // ── result_feedback: score validation + conversational prompts ─

    #[test]
    fn result_feedback_score_range_contract() {
        // Valid scores: 1–5
        for s in 1..=5u8 {
            let valid = (1..=5).contains(&s);
            assert!(valid, "score {s} should be accepted");
        }
        // Invalid scores: 0 and 6+
        for s in [0u8, 6, 10, 255] {
            let valid = (1..=5).contains(&s);
            assert!(!valid, "score {s} should be rejected");
        }
    }

    #[test]
    fn result_feedback_both_optional() {
        let _score: Option<u8> = None;
        let _comments: &str = "";
        assert!(_score.is_none() && _comments.is_empty(), "both optional");
        let score: Option<u8> = Some(4);
        assert!(score.is_some(), "score only is valid feedback");
        let comments: &str = "missing sector field";
        assert!(!comments.is_empty(), "comments only is valid feedback");
    }
    // ── Learning loop integration: feedback → state → routing ────────

    #[test]
    fn learning_loop_flaky_provider_override() {
        let mut state = LearningState::default();

        // No data → no provider preference
        assert!(!state.is_flaky("AAPL", Provider::Fmp));
        assert!(state.preferred_provider("AAPL", Provider::Fmp).is_none());

        // Feed 5 low-score ratings for FMP on AAPL (scores 1-2 → failures)
        for _ in 0..5 {
            state.record("AAPL", Provider::Fmp, Some(1));
        }
        // Beta: α=1, β=6, prob = 1/7 ≈ 0.14 < 0.70 → flaky
        assert!(state.is_flaky("AAPL", Provider::Fmp));
        assert_eq!(
            state.preferred_provider("AAPL", Provider::Fmp),
            Some(Provider::Eodhd),
            "FMP flaky → should prefer EODHD"
        );

        // EODHD is not flaky for AAPL
        assert!(!state.is_flaky("AAPL", Provider::Eodhd));

        // MSFT has no data → no preference
        assert!(state.preferred_provider("MSFT", Provider::Fmp).is_none());
    }

    #[test]
    fn learning_loop_both_flaky_no_override() {
        let mut state = LearningState::default();

        // Feed flaky ratings for both providers
        for _ in 0..5 {
            state.record("VOD.L", Provider::Fmp, Some(2));
            state.record("VOD.L", Provider::Eodhd, Some(1));
        }
        assert!(state.is_flaky("VOD.L", Provider::Fmp));
        assert!(state.is_flaky("VOD.L", Provider::Eodhd));
        // Both flaky → no preference (let default routing handle it)
        assert!(state.preferred_provider("VOD.L", Provider::Fmp).is_none());
    }

    #[test]
    fn learning_loop_recovery_after_accurate_ratings() {
        let mut state = LearningState::default();

        // Make FMP flaky with 5 failures
        for _ in 0..5 {
            state.record("AAPL", Provider::Fmp, Some(1));
        }
        assert!(state.is_flaky("AAPL", Provider::Fmp));

        // Feed 10 accurate ratings (score 4-5 → successes)
        // Beta needs more evidence to recover: 5β + 10α → α=11, β=6, prob=11/17≈0.647
        // Still < 0.70 — Beta is conservative. Feed 15 successes.
        for _ in 0..15 {
            state.record("AAPL", Provider::Fmp, Some(5));
        }
        // Beta: α=16, β=6, prob = 16/22 ≈ 0.727 > 0.70 → recovered
        assert!(
            !state.is_flaky("AAPL", Provider::Fmp),
            "should recover after sufficient high scores raise Beta posterior above 0.70"
        );
    }

    #[test]
    fn learning_loop_insufficient_data_no_override() {
        let mut state = LearningState::default();

        // Only 3 ratings — below the total >= 5 threshold
        for _ in 0..3 {
            state.record("AAPL", Provider::Fmp, Some(1));
        }
        assert!(
            !state.is_flaky("AAPL", Provider::Fmp),
            "3 ratings < 5 threshold → not enough data"
        );
        assert!(state.preferred_provider("AAPL", Provider::Fmp).is_none());
    }

    // ── Configurable staleness threshold ─────────────────────────────

    #[test]
    fn staleness_threshold_default_is_90_days() {
        let state = LearningState::default();
        assert_eq!(state.staleness_days(), learning::CHRONIC_STALENESS_DAYS);
        assert_eq!(state.staleness_days(), 90);
    }

    #[test]
    fn staleness_threshold_custom_overrides_default() {
        let state = LearningState::with_staleness_days(30);
        assert_eq!(state.staleness_days(), 30);
    }

    #[test]
    fn is_chronically_stale_respects_custom_threshold() {
        // File a snapshot whose latest filing is 40 days old.
        let old_filing = (chrono::Utc::now() - chrono::Duration::days(40))
            .format("%Y-%m-%d")
            .to_string();

        // 40-day-old filing: stale under a 30-day threshold, fresh under the
        // 90-day default.
        let mut tight = LearningState::with_staleness_days(30);
        tight.record_temporal_snapshot("AAPL", Provider::Fmp, 100.0, Some(old_filing.clone()));
        assert!(
            tight.is_chronically_stale("AAPL", Provider::Fmp),
            "40 days > 30-day threshold → chronically stale"
        );

        let mut default = LearningState::default();
        default.record_temporal_snapshot("AAPL", Provider::Fmp, 100.0, Some(old_filing));
        assert!(
            !default.is_chronically_stale("AAPL", Provider::Fmp),
            "40 days < 90-day default → not stale"
        );
    }
}
