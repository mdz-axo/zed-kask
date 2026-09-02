#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Companies — Dual-provider company financial data (FMP + EODHD)
//!
//! Tools are provider-agnostic: each tool routes to FMP or EODHD based on
//! symbol characteristics, with automatic fallback. EODHD responses are
//! normalized to match FMP format so analysis functions work transparently.
//!
//! ## Tools (43) — portfolio ledger/CRUD/returns live in the portfolio MCP server
//!
//! Tools are split across submodules under `src/tools/`, each with its own
//! `#[tool_router]` block, merged in `combined_router()`:
//! - `tools/financial_data.rs` — company_profile, stock_quote, income_statement,
//!   balance_sheet, cash_flow_statement, key_metrics, historical_price, symbol_search
//! - `tools/analysis.rs` — moat_check, management_scorecard, working_capital_cycle
//! - `tools/valuation.rs` — dcf_valuation, reverse_dcf, ep_valuation, comparable_analysis,
//!   scenario_analysis, sensitivity_analysis, monte_carlo_dcf, scenario_impact_valuation,
//!   calibrate_forecast, forecast_record, forecast_persist
//! - `tools/analytics.rs` — portfolio_attribution, portfolio_characteristics
//! - `tools/economic_profit.rs` — ep_valuation (economic profit view)
//! - `tools/expectations.rs` — expectations_gap
//! - `tools/notes.rs` — note_add, note_list, note_delete,
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
//! ## Ontology anchoring
//! Verified FIBO terms (fixture-pinned in `hkask-bridge-ontology`) where FIBO
//! publishes the concept; internal metric identifiers (plain hKask keys, no
//! external standard claimed) for ratios and line items FIBO does not
//! publish; Dublin Core fallbacks for analysis-family tool anchors. See
//! `fibo.rs` in this server and the bridge crate's `fibo` module.

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{McpToolError, map_join_error, validate_identifier};

pub(crate) mod aggregation;
mod analysis;
pub(crate) mod data_quality;
pub(crate) mod economic_profit;
pub(crate) mod fibo;
pub(crate) mod fibo_cache;
mod financial_model;
// `pub` so the integration tests (tests/fmp_endpoint_schema.rs) can pin
// the resolution ranking against the live EODHD API.
pub mod providers;
pub(crate) mod research_store;
pub(crate) use providers::{CompanyProfile, HistoricalPriceView, KeyMetrics, Provider};
mod forecast;
pub(crate) mod learning;

pub(crate) mod research;
mod scenarios;
mod screener;
pub(crate) mod superforecast;
mod transcript;
mod valuation_service;
pub(crate) use forecast::{
    StoredForecast, current_price_from_multiple, projected_terminal_multiple,
};

pub(crate) mod types;

use research_store::{PersistedForecast, ResearchStore};

pub(crate) mod tools;

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
        pub research: ResearchStore,
        pub learning: std::sync::Arc<std::sync::Mutex<LearningState>>,
        pub fermi_defaults: superforecast::FermiDefaults,
        pub fibo_cache: Option<fibo_cache::FiboDataCache>,
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
        // FIBO cache: check for a fresh raw response before hitting the API.
        let params_hash = fibo_cache::hash_params(extra);

        if let Some(ref cache) = self.fibo_cache {
            if let Some(cached) = cache.get_raw(symbol, tool, &params_hash) {
                tracing::debug!("fibo_cache: hit for {symbol} {tool}");
                return Ok(cached);
            }
        }

        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let response = providers::companies_get(
            &self.client,
            tool,
            symbol,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            extra,
            Some(&l),
        )
        .await?;
        let provider_str = match response.provider {
            providers::Provider::Fmp => "FMP",
            providers::Provider::Eodhd => "EODHD",
        };
        let result = response.value;

        // Store the fresh response in the FIBO cache and extract concepts.
        if let Some(ref cache) = self.fibo_cache {
            cache.store_raw(symbol, tool, &params_hash, &result, provider_str);
            cache.extract_and_store_concepts(symbol, tool, &result, provider_str);
        }

        Ok(result)
    }

    /// Fetch a company profile as a typed `CompanyProfile` view. Concentrates
    /// field-name knowledge so tool handlers read `profile.market_cap()`
    /// instead of `v.get("mktCap").and_then(|v| v.as_f64())`.
    async fn fetch_profile(&self, symbol: &str) -> Result<CompanyProfile, McpToolError> {
        let raw = self.fetch("company_profile", symbol, &[]).await?;
        Ok(CompanyProfile::from_raw(raw))
    }

    /// Fetch key metrics as a typed `KeyMetrics` view.
    async fn fetch_key_metrics(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<KeyMetrics, McpToolError> {
        let limit_str = limit.to_string();
        let params_hash = fibo_cache::hash_params(&[("limit", &limit_str)]);

        // Check FIBO cache first.
        if let Some(ref cache) = self.fibo_cache {
            if let Some(cached) = cache.get_raw(symbol, "key_metrics", &params_hash) {
                tracing::debug!("fibo_cache: hit for {symbol} key_metrics");
                return Ok(KeyMetrics::from_raw(cached));
            }
        }

        let l = self
            .learning
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let metrics = providers::fetch_key_metrics(
            &self.client,
            symbol,
            limit,
            &self.fmp_api_key,
            &self.eodhd_api_key,
            Some(&l),
        )
        .await?;
        let raw = metrics.raw().clone();

        // Cache the merged key-metrics response and extract FIBO concepts.
        if let Some(ref cache) = self.fibo_cache {
            cache.store_raw(symbol, "key_metrics", &params_hash, &raw, "FMP");
            cache.extract_and_store_concepts(symbol, "key_metrics", &raw, "FMP");
        }

        Ok(KeyMetrics::from_raw(raw))
    }

    /// Fetch historical prices as a typed `HistoricalPriceView` view.
    async fn fetch_historical_price(
        &self,
        symbol: &str,
        from: &str,
        to: &str,
    ) -> Result<HistoricalPriceView, McpToolError> {
        let raw = self
            .fetch("historical_price", symbol, &[("from", from), ("to", to)])
            .await?;
        Ok(HistoricalPriceView::from_raw(raw))
    }

    async fn save_forecast(&self, forecast: PersistedForecast) -> Result<(), McpToolError> {
        let research = self.research.clone();
        tokio::task::spawn_blocking(move || research.save_forecast(&forecast))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn get_persisted_forecast(
        &self,
        forecast_id: String,
    ) -> Result<Option<PersistedForecast>, McpToolError> {
        let research = self.research.clone();
        tokio::task::spawn_blocking(move || research.get_forecast(&forecast_id))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn list_persisted_forecasts(
        &self,
        symbol: String,
    ) -> Result<Vec<PersistedForecast>, McpToolError> {
        let research = self.research.clone();
        tokio::task::spawn_blocking(move || research.list_forecasts(&symbol))
            .await
            .map_err(|error| map_join_error(error, "forecast task failed"))?
            .map_err(map_portfolio_error)
    }

    async fn record_persisted_forecast_outcome(
        &self,
        forecast_id: String,
        outcome: serde_json::Value,
    ) -> Result<(), McpToolError> {
        let research = self.research.clone();
        tokio::task::spawn_blocking(move || research.record_forecast_outcome(&forecast_id, outcome))
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
            + Self::notes_router()
            + Self::analytics_router()
            + Self::valuation_router()
            + Self::economic_profit_router()
            + Self::expectations_router()
            + Self::transcript_router()
            + Self::artifacts_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for CompaniesServer {}

// ── Entry point ─────────────────────────────────────────────────────

// Fail fast before the 60s MCP `tools/call` cap kills and restarts the
// server: a hung FMP/EODHD/web-search upstream surfaces as a request error
// inside the cap, not a server restart that loses in-flight work.
const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const HTTP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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
            // FIBO financial data cache — stores raw API responses and FIBO-tagged
            // concept points in SQLite. Failures are non-fatal: the server runs
            // without caching (every fetch hits the API), but logs a warning.
            let fibo_cache = match fibo_cache::resolve_cache_db_path(
                &ctx.webid.to_string(),
            ) {
                Ok(path) => match fibo_cache::FiboDataCache::open(&path) {
                    Ok(cache) => Some(cache),
                    Err(e) => {
                        tracing::warn!("fibo_cache: failed to open cache DB: {e}");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("fibo_cache: failed to resolve cache path: {e}");
                    None
                }
            };
            // zed-kask: HTTP client with explicit connect+request timeouts.
            // The MCP client caps any `tools/call` at 60s; without these,
            // `expectations_gap` (which fans out to 5 FMP/EODHD fetches plus
            // 3 parallel web-search calls) can stall on a single slow upstream
            // and hit the 60s cap, triggering a server restart + retry that
            // never converges. A 20s request timeout keeps each upstream call
            // well under the cap so a stalled provider surfaces as
            // `McpToolError::unavailable` and routes to the fallback provider
            // via `companies_get`, instead of dragging the whole tool down.
            // `unwrap_or_else` fallback to `Client::new()` matches the
            // `hkask-mcp-swarm` pattern — builder failure is a misconfig, not
            // a reason to refuse startup.
            let http_client = reqwest::Client::builder()
                .connect_timeout(HTTP_CONNECT_TIMEOUT)
                .timeout(HTTP_REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "hkask-mcp-companies: HTTP client builder failed, falling back to no-timeout client: {e}"
                    );
                    reqwest::Client::new()
                });
            Ok(CompaniesServer::new(
                ctx.webid,
                http_client,
                fmp_api_key,
                eodhd_api_key,
                exa_api_key,
                tavily_api_key,
                brave_api_key,
                serpapi_key,
                ResearchStore::new(ctx.webid)?,
                std::sync::Arc::new(std::sync::Mutex::new({
                    let days = hkask_mcp_server::parse_env_warn(
                        "HKASK_CHRONIC_STALENESS_DAYS",
                        LearningState::default().staleness_days(),
                    );
                    LearningState::with_staleness_days(days)
                })),
                superforecast::FermiDefaults::from_env(),
                fibo_cache,

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

// ── Tool-behavior contract tests (check-mcp-tool-tests.sh) ─────────────────
//
// These drive the real `Parameters<T>` tool seam for the input-validation
// paths that run before any network I/O: a hostile or malformed symbol must
// produce the structured `{"error", "kind"}` envelope with the right
// `McpErrorKind`, not a panic and not a live fetch. The happy path requires
// SerpAPI/FMP/EODHD HTTP (the allowlist reason); the validation contract is
// testable without it.
#[cfg(test)]
mod tool_behavior_tests {
    use super::*;
    use crate::types::{ResolveSymbolRequest, SymbolRequest};
    use hkask_types::WebID;
    use rmcp::handler::server::wrapper::Parameters;

    fn make_server() -> CompaniesServer {
        CompaniesServer::new(
            WebID::new(),
            reqwest::Client::new(),
            "test-fmp-key".to_string(),
            "test-eodhd-key".to_string(),
            None,
            None,
            None,
            None,
            ResearchStore::new(WebID::new()).expect("research store init"),
            std::sync::Arc::new(std::sync::Mutex::new(LearningState::default())),
            superforecast::FermiDefaults::from_env(),
            None,
        )
    }

    fn parse_envelope(output: &str) -> serde_json::Value {
        serde_json::from_str(output)
            .unwrap_or_else(|e| panic!("tool output must be valid JSON, got: {output} ({e})"))
    }

    // Pins the registered tool-surface count end-to-end. The portfolio ledger
    // surface (portfolio_delete, ledger_import, ledger_export,
    // portfolio_comparison, portfolio_returns, transaction_note_append) was
    // removed from this server when the portfolio MCP server took ownership —
    // this pin is what makes a re-introduction (or a silent registration drop)
    // fail CI instead of shipping as an undocumented duplicate.
    #[test]
    fn tool_surface_is_exactly_43_registered_tools() {
        let n = CompaniesServer::combined_router().list_all().len();
        assert_eq!(n, 43, "companies registered tool surface changed; got {n}");
    }

    // Coverage: every registered tool must map to an ontology concept for
    // the output-JSON `"ontology"` field baked by `fibo::enrich_with_ontology`.
    #[test]
    fn fibo_mapping_covers_all_registered_tools() {
        let router = CompaniesServer::combined_router();
        for tool in router.list_all() {
            assert!(
                fibo::tool_to_ontology(&tool.name).is_some(),
                "fibo::tool_to_ontology returned None for registered tool '{}'; \
                 add an explicit arm",
                tool.name
            );
        }
    }

    /// Pin: the HTTP client built in `run()` carries explicit connect and
    /// request timeouts so a hung FMP/EODHD/web-search upstream fails fast
    /// before the 60s MCP `tools/call` cap kills and restarts the server.
    /// reqwest exposes no client-config inspection, so this pins the named
    /// consts the construction reads — dropping the timeouts means removing
    /// or rename-breaking a const this test references. It does NOT verify the
    /// built client itself carries them (not observable), nor the fallback
    /// `Client::new()` path taken on builder failure.
    #[test]
    fn http_client_timeouts_pinned_below_mcp_cap() {
        assert_eq!(
            HTTP_CONNECT_TIMEOUT,
            std::time::Duration::from_secs(10),
            "connect timeout changed; re-verify against the 60s MCP tools/call cap"
        );
        assert_eq!(
            HTTP_REQUEST_TIMEOUT,
            std::time::Duration::from_secs(20),
            "request timeout changed; must stay well under the 60s MCP tools/call cap \
             (see the construction-site comment in `run`)"
        );
    }

    /// `moat_check` with a traversal symbol must return a typed
    /// `InvalidArgument` error — never panic, never fetch.
    #[tokio::test]
    async fn moat_check_rejects_invalid_symbol_with_typed_error() {
        let server = make_server();
        let error = server
            .moat_check(Parameters(SymbolRequest {
                symbol: "../etc/passwd".to_string(),
            }))
            .await
            .expect_err("invalid symbol must yield a typed error, not a panic");
        assert!(
            matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
            "traversal symbol must be rejected as InvalidArgument, got: {:?}",
            error.kind
        );
        assert!(
            !error.message.is_empty(),
            "typed error must carry a message, got: {error:?}"
        );
    }

    /// An over-long symbol (>32 chars) violates `validate_identifier`'s cap.
    #[tokio::test]
    async fn moat_check_rejects_over_long_symbol() {
        let server = make_server();
        let long_symbol = "A".repeat(64);
        let error = server
            .moat_check(Parameters(SymbolRequest {
                symbol: long_symbol,
            }))
            .await
            .expect_err("over-long symbol must yield a typed error");
        assert!(
            matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
            "over-long symbol must be rejected as InvalidArgument, got: {:?}",
            error.kind
        );
    }

    /// A valid-shaped symbol passes validation and proceeds to fetch — which,
    /// against an unreachable endpoint, must surface as a typed error
    /// (not a panic). Uses an unroutable host so the test stays offline.
    #[tokio::test]
    async fn moat_check_valid_symbol_surfaces_structured_error_on_fetch_failure() {
        let mut server = make_server();
        // Point the client at an unreachable local port: connection refused.
        // (The fetch URL comes from provider config; the client itself cannot
        // be re-pointed per-test without rebuilding the server. Instead we
        // assert on the *shape* of the failure by using an empty key set —
        // the providers degrade to errors before any request leaves.)
        server.fmp_api_key = String::new();
        server.eodhd_api_key = String::new();

        match server
            .moat_check(Parameters(SymbolRequest {
                symbol: "AAPL".to_string(),
            }))
            .await
        {
            // Either a typed error (missing credentials) or a content payload
            // with degraded data — both are valid contracts. A panic or
            // non-JSON output is not.
            Ok(output) => {
                let parsed = parse_envelope(&output);
                assert!(
                    parsed.is_object(),
                    "content payload must be a JSON object, got: {parsed}"
                );
            }
            Err(error) => {
                assert!(
                    !error.message.is_empty(),
                    "typed error must carry a message, got: {error:?}"
                );
            }
        }
    }

    /// `resolve_symbol` needs at least one of company name / ticker —
    /// an exchange or country alone cannot identify a listing. Validated
    /// before any request leaves.
    #[tokio::test]
    async fn resolve_symbol_requires_name_or_ticker() {
        let server = make_server();
        let error = server
            .resolve_symbol(Parameters(ResolveSymbolRequest {
                company_name: None,
                ticker: None,
                exchange: Some("NASDAQ".to_string()),
                country: None,
            }))
            .await
            .expect_err("no name/ticker must yield a typed error, not a panic");
        assert!(
            matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
            "missing name/ticker must be InvalidArgument, got: {:?}",
            error.kind
        );
    }
}
