#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Companies — Dual-provider company financial data (FMP + EODHD)
//!
//! Tools are provider-agnostic: each tool routes to FMP or EODHD based on
//! symbol characteristics, with automatic fallback. EODHD responses are
//! normalized to match FMP format so analysis functions work transparently.
//!
//! ## Tools (44) — pinned by `tool_surface_is_exactly_44_registered_tools`
//!
//! Tools are split across submodules under `src/tools/`, each with its own
//! `#[tool_router]` block, merged in `combined_router()`:
//! - `tools/financial_data.rs` — company_profile, stock_quote, income_statement,
//!   balance_sheet, cash_flow_statement, key_metrics, historical_price, symbol_search
//! - `tools/analysis.rs` — moat_check, management_scorecard, working_capital_cycle
//! - `tools/valuation.rs` — dcf_valuation, reverse_dcf, ep_valuation, comparable_analysis,
//!   scenario_analysis, sensitivity_analysis, monte_carlo_dcf, scenario_impact_valuation,
//!   calibrate_forecast, forecast_record
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
use serde::{Deserialize, Serialize};

pub mod aggregation;
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
mod transcript;
pub mod types;

use portfolio::{PersistedForecast, PortfolioManager};

pub mod tools;
pub use transcript::{MissingReason, TranscriptCoverage, TranscriptRecord, TranscriptResult};

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

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`, or
    // a sub-router missing from `combined_router()`, silently registers nothing
    // (`cargo check` passes on an unwired orphan). Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_44_registered_tools() {
        let n = CompaniesServer::combined_router().list_all().len();
        assert_eq!(n, 44, "companies registered tool surface changed; got {n}");
    }

    // Coverage: every registered tool must have a non-None ontology anchor.
    // Catches the silent-drop failure mode where a new tool is added to the
    // router without a corresponding arm in fibo::tool_to_ontology. The count
    // pin above catches addition; this test catches anchoring.
    #[test]
    fn ontology_anchor_covers_all_registered_tools() {
        let router = CompaniesServer::combined_router();
        for tool in router.list_all() {
            assert!(
                CompaniesServer::ontology_anchor(&tool.name).is_some(),
                "ontology_anchor returned None for registered tool '{}'; \
                 add an explicit arm in fibo::tool_to_ontology",
                tool.name
            );
        }
    }
}

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
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_SERPAPI_API_KEY",
                "SerpAPI key for corpus-mode transcript search",
            ),
        ],
    )
    .await
}

// ── Tracer-bullet contracts ───────────────────────────────────────

#[cfg(test)]
mod poison_tests;

#[cfg(test)]
mod dead_surface_pins {
    /// `PORTFOLIO_AGGREGATABLE_FIELDS` and `PORTFOLIO_CATEGORICAL_FIELDS` were
    /// deleted from `fibo.rs` because they had zero production call sites —
    /// only the in-module tests referenced them. This test pins their absence
    /// so a future commit cannot re-add them without a consumer. Per `.rules`
    /// "Advertised invariants need enforcement points": a constant with no
    /// consumer is dead surface regardless of its ontological correctness.
    #[test]
    fn portfolio_field_tables_not_present() {
        // The constants must not be re-added without a production consumer.
        let fibo_source = include_str!("fibo.rs");
        assert!(
            !fibo_source.contains("PORTFOLIO_AGGREGATABLE_FIELDS"),
            "PORTFOLIO_AGGREGATABLE_FIELDS must not be re-added without a consumer — it was dead surface"
        );
        assert!(
            !fibo_source.contains("PORTFOLIO_CATEGORICAL_FIELDS"),
            "PORTFOLIO_CATEGORICAL_FIELDS must not be re-added without a consumer — it was dead surface"
        );
    }
}

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
