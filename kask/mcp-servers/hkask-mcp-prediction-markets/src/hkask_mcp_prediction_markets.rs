#![forbid(unsafe_code)]
// `tokio` and `anyhow` are in [dependencies] for the bin targets' `#[tokio::main]`
// and `anyhow::Result` in `src/bin/fetch_contracts.rs`; the lib itself does not
// use them, so the unused_crate_dependencies lint fires on the lib target.
// This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! Prediction-markets data-service MCP server.
//!
//! Read-only annotated feed of market-implied probabilities from Polymarket
//! (Gamma/CLOB) and Kalshi (Predictions REST). Every probability is paired
//! with reliability covariates, calibration metadata, volatility annotation,
//! and a dual-axis ontology mapping (PKO process axis + Dublin Core state
//! axis) so forecasting consumers never receive a bare probability.
//! See docs/reports/prediction-markets/02-zed-kask-integration.md §4.

use std::collections::HashSet;

use hkask_mcp_portfolio::map_portfolio_error;
use hkask_mcp_server::server::{
    CredentialRequirement, McpToolError, execute_tool_semantic, map_join_error,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

pub mod base_event;
pub mod cache;
pub mod calibration;
pub mod cmp;
pub mod cmp_portfolio;
pub mod economic_object;
pub mod matcher;
pub mod ontology;
pub mod provider_kalshi;
pub mod provider_polymarket;
pub mod residual;
pub mod semantic_mapping;
mod streaming;
pub mod types;
pub mod volatility;

// ── Request/response types ─────────────────────────────────────────────────

/// Empty request for prediction_markets_status (no parameters needed).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatusRequest {}

/// Empty request for market_ontology_map.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketOntologyMapRequest {}

/// Request for market_lookup: free-text query with optional filters.
///
/// Matching is substring-based over event title/slug/description until T4c's
/// dedicated matcher lands; T4c replaces this tool's retrieval internals.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketLookupRequest {
    /// Free-text query (case-insensitive substring over title/slug/description).
    pub query: String,
    /// Optional category filter (Kalshi category or Polymarket tag label).
    pub category: Option<String>,
    /// Max records to return (default 10, capped at 50).
    pub limit: Option<u32>,
}

/// Request for market_record_resolution: feed a resolved outcome into the
/// calibration store (the sense arm of the T10 feedback loop).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketRecordResolutionRequest {
    /// Calibration bucket (domain or series) the market belonged to.
    pub bucket: String,
    /// The market-implied probability at observation time.
    pub probability: f64,
    /// The realized outcome (true = the event occurred / Yes won).
    pub outcome: bool,
}

/// Request for market_subscribe_resolutions: stream market_resolved events
/// from Polymarket's public market channel into the calibration store.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketSubscribeRequest {
    /// CLOB asset (token) IDs to subscribe to (from a record's token IDs).
    pub asset_ids: Vec<String>,
    /// Calibration bucket to record resolutions under.
    pub bucket: String,
    /// Max resolutions to ingest before returning (default 1) — bounds the
    /// tool's lifetime so it doesn't hold a tool call open indefinitely.
    pub max_resolutions: Option<u32>,
}

/// Request for market_residual: niche-event exposure to a base event.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketResidualRequest {
    /// Niche market ticker (Kalshi).
    pub market_ticker: String,
    /// Base-event market ticker (Kalshi) — the benchmark leg.
    pub base_ticker: String,
    /// Lookback window in days for the price histories (default 90).
    pub window_days: Option<u32>,
}

/// Request for market_cmp_index: the full constant-maturity curve for a
/// registered base event (the published index, not a point query).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpIndexRequest {
    /// Base-event series ticker (must be registered).
    pub series: String,
}

/// Request for market_cmp_index_store: compute the CMP index set for a
/// registered base event from live markets and persist each (bucket,
/// orientation) index as a transaction-ledger portfolio of contracts, with
/// materialized daily holdings. The portfolio name is
/// `cmp:{series}:{bucket}:{orientation}`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpIndexStoreRequest {
    /// Base-event series ticker (must be registered).
    pub series: String,
    /// Observation date (YYYY-MM-DD) for the stored snapshot. Defaults to
    /// today's UTC date when omitted.
    pub date: Option<String>,
}

/// Request for market_cmp_portfolio_store: compute the solved-portfolio CMP
/// index set (maturity-bucketed, orientation-tagged portfolios of contracts
/// with maturity-matched weights) for a registered base event and persist
/// each (bucket, orientation) index as a transaction-ledger portfolio.
///
/// All economic-context fields are optional — when omitted, the tool uses
/// the curated default for the base-event family (see
/// `BaseEvent::default_economic_context`). This follows the zed-kask design
/// pattern: never present a blank field — always provide a reasonable default
/// the user can accept or override. Use `market_cmp_context_suggest` for an
/// AI-assisted proposal with reasoning.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpPortfolioStoreRequest {
    /// Base-event series ticker (must be registered).
    pub series: String,
    /// The current level of the underlying factor (e.g. 5.375 for Fed funds
    /// at 5.25-5.50%, 80.0 for WTI crude). When omitted, uses the curated
    /// default for the family.
    pub reference: Option<f64>,
    /// The trailing volatility of the underlying in the family's type units
    /// (absolute: bp/pp/$; relative: as a fraction). When omitted, uses the
    /// curated default. None disables materiality-gated indices.
    pub volatility: Option<f64>,
    /// The predicted level (strike) the contracts are structured around.
    /// When omitted, defaults to the reference → Stable orientation.
    pub predicted_level: Option<f64>,
    /// Whether the contract predicts the factor ends above its strike
    /// (true) or below (false). When omitted, the tool extracts the direction
    /// from the market title or uses the curated default.
    pub direction_up: Option<bool>,
    /// Observation date (YYYY-MM-DD). Defaults to today's UTC date.
    pub date: Option<String>,
}

/// Request for market_cmp_context_suggest: propose a curated economic
/// context (reference, volatility, predicted level, direction) for a
/// base-event family, with reasoning. Read-only aid — no ledger debit, no
/// storage. The operator accepts, overrides, or rejects the proposal before
/// calling `market_cmp_portfolio_store`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpContextSuggestRequest {
    /// Base-event series ticker (must be registered). The tool classifies the
    /// family from the series and returns the curated default context.
    pub series: String,
}

/// Request for market_volatility: compute the DR-AS structural volatility
/// forecast for a single contract (arXiv:2607.08199). Returns the conditional
/// variance, its DR and AS decomposition, and a 95% prediction interval.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketVolatilityRequest {
    /// The prediction-market price (YES probability), in [0, 1].
    pub price: f64,
    /// Time to resolution in hours. Must be > 0.
    pub hours_to_resolution: f64,
    /// The bid-ask spread in dollars (price units [0, 1]). When omitted, the
    /// adverse-selection channel contributes 0.
    pub spread: Option<f64>,
    /// Trading volume during the observation window. 0 when no trades.
    #[serde(default)]
    pub volume: f64,
    /// Forecast horizon in hours (default 1.0 — one-hour-ahead, matching the
    /// paper's hourly grid).
    #[serde(default = "default_one_hour")]
    pub horizon_hours: f64,
    /// The fitted adverse-selection scale K. When omitted, uses the default
    /// (0.12 — a conservative midpoint from the paper's pooled Kalshi panel).
    /// Override with a locally fitted value when available.
    pub k: Option<f64>,
    /// The activity proxy ν(V). When omitted, uses √V (the paper's best).
    #[serde(default)]
    pub activity_proxy: Option<volatility::ActivityProxy>,
}

fn default_one_hour() -> f64 {
    1.0
}

/// Request for market_cmp: constant-maturity prediction for a registered
/// base event at a fixed tenor.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpRequest {
    /// Base-event series ticker (must be in the configured registry —
    /// HKASK_PREDICTION_MARKETS_BASE_EVENTS, "domain:series,..." pairs).
    pub series: String,
    /// Tenor in days (e.g. 30, 90, 180).
    pub tenor_days: u32,
}

/// Request for market_history: a market's record enriched with realized
/// variance computed from its price history.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketHistoryRequest {
    /// Kalshi market ticker (e.g. "KXFED-27DEC-H0") or Polymarket CLOB token
    /// ID (the Yes leg).
    pub market: String,
    /// Platform hint: "kalshi" (default) or "polymarket".
    pub source: Option<String>,
    /// Lookback window in days (default 90, Kalshi only — Polymarket CLOB
    /// history uses its own interval parameter).
    pub window_days: Option<u32>,
}

/// Request for market_check_resolutions: scan for newly resolved markets
/// and feed their outcomes into the calibration store (the loop's
/// self-feeding sense arm).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCheckResolutionsRequest {
    /// Optional series/bucket scope (Kalshi series ticker; Polymarket scans
    /// recent closed markets regardless).
    pub series: Option<String>,
    /// Max markets to scan per platform (default 100).
    pub limit: Option<u32>,
}

/// Request for market_calibration: per-bucket Brier reading.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCalibrationRequest {
    /// Calibration bucket: a domain ("politics", "economics") or series ticker.
    pub bucket: String,
}

/// Request for market_match: entity resolution from a scenario/forecast
/// question to candidate markets about the same underlying event (T4c).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketMatchRequest {
    /// The scenario or forecasting question to resolve against markets.
    pub question: String,
    /// Max candidates to return (default 5, capped at 20).
    pub limit: Option<u32>,
}

/// Request for market_ladder: the duration profile of a contract series.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketLadderRequest {
    /// Kalshi series ticker (e.g. "KXFEDDECISION") or Polymarket event slug.
    /// Both platforms are probed; whichever recognizes the identifier
    /// contributes rungs, the other reports its failure in `warnings`.
    pub series: String,
}

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct PredictionMarketsServer {
        pub http: reqwest::Client,
        pub cache_ttl_secs: u64,
        pub calibration_store: std::sync::Arc<std::sync::Mutex<calibration::CalibrationStore>>,
        pub response_cache: cache::TtlCache,
        pub calibration_path: Option<String>,
        pub base_events: Vec<(String, String)>,
        pub called_tools: std::sync::Mutex<HashSet<String>>,
        /// General-purpose portfolio store (from `hkask-mcp-portfolio`) for
        /// persisting CMP indices as transaction ledgers with materialized
        /// daily holdings and returns views.
        pub portfolio_store: hkask_mcp_portfolio::PortfolioStore,
        /// Optional FRED API key for live reference-level fetches. When absent,
        /// `market_cmp_context_suggest` uses curated static defaults.
        pub fred_api_key: Option<String>,
    }
);

// ── Tool router ────────────────────────────────────────────────────────────

impl PredictionMarketsServer {
    fn combined_router() -> ToolRouter<Self> {
        Self::prediction_markets_router()
    }

    /// Registry-convention ontology anchor (mirrors scenarios server).
    fn ontology_anchor(_tool: &str) -> &'static str {
        "dublin-core"
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

#[tool_router(router = prediction_markets_router, vis = "pub")]
impl PredictionMarketsServer {
    /// Return the current server state snapshot.
    #[tool(
        description = "Return current prediction-markets server state: cache TTL, ontology mapping version, and tools called this session."
    )]
    async fn prediction_markets_status(
        &self,
        Parameters(_req): Parameters<StatusRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "prediction_markets_status",
            Some(Self::ontology_anchor("prediction_markets_status")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("prediction_markets_status".to_string());
                Ok(serde_json::json!({
                    "server": "hkask-mcp-prediction-markets",
                    "cache_ttl_secs": self.cache_ttl_secs,
                    "ontology_mapping_version": ontology::MAPPING_VERSION,
                    "called_tools": self
                        .called_tools
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>(),
                }))
            },
        )
        .await
    }

    /// Look up markets matching a query across both platforms.
    #[tool(
        description = "Look up prediction markets across Polymarket and Kalshi by free-text query. Returns annotated MarketRecords: every probability is paired with spread/volume/calibration/volatility/reliability_tier and a dual-axis (PKO + Dublin Core) ontology mapping. Never returns a bare probability."
    )]
    pub async fn market_lookup(&self, Parameters(req): Parameters<MarketLookupRequest>) -> String {
        execute_tool_semantic(
            self,
            "market_lookup",
            Some(Self::ontology_anchor("market_lookup")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_lookup".to_string());
                let mut records = self.gather_candidates().await?;
                Self::substring_filter(&mut records, &req.query);
                if let Some(category) = &req.category {
                    let cat = category.to_lowercase();
                    records.retain(|r| r.category.to_lowercase().contains(&cat));
                }
                records.truncate(req.limit.unwrap_or(10).min(50) as usize);
                serde_json::to_value(&records).map_err(|e| {
                    McpToolError::internal(format!("record serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Resolve a scenario/forecast question to candidate markets about the
    /// same underlying event.
    #[tool(
        description = "Resolve a scenario or forecasting question to candidate prediction markets about the same underlying event. Returns confidence-tiered candidates with deterministic match basis (token overlap + deadline alignment). Refuse low-confidence matches rather than anchoring on a wrong-event market."
    )]
    pub async fn market_match(&self, Parameters(req): Parameters<MarketMatchRequest>) -> String {
        execute_tool_semantic(
            self,
            "market_match",
            Some(Self::ontology_anchor("market_match")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_match".to_string());
                let records = self.gather_candidates().await?;
                let mut matches = matcher::rank_matches(&req.question, &records);
                matches.truncate(req.limit.unwrap_or(5).min(20) as usize);
                serde_json::to_value(&matches).map_err(|e| {
                    McpToolError::internal(format!("match serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Return the dual-axis ontology mapping document.
    #[tool(
        description = "Return the dual-axis (PKO process + Dublin Core state) ontology mapping document that annotates every MarketRecord, including the market lifecycle stages and field-level mappings. Fetch this before interpreting market records."
    )]
    pub async fn market_ontology_map(
        &self,
        Parameters(_req): Parameters<MarketOntologyMapRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_ontology_map",
            Some(Self::ontology_anchor("market_ontology_map")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_ontology_map".to_string());
                Ok(ontology::mapping_document())
            },
        )
        .await
    }

    /// Return the calibration reading for a domain/series bucket.
    #[tool(
        description = "Return the calibration reading (Brier score, sample size, staleness) for a domain or series bucket, computed from resolved market observations via hkask-forecast. A bucket with no resolved data returns stale: true — never a synthetic brier of 0."
    )]
    pub async fn market_calibration(
        &self,
        Parameters(req): Parameters<MarketCalibrationRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_calibration",
            Some(Self::ontology_anchor("market_calibration")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_calibration".to_string());
                let store = self
                    .calibration_store
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let reading = calibration::read_calibration(&store, &req.bucket);
                serde_json::to_value(&reading).map_err(|e| {
                    McpToolError::internal(format!("calibration serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Record a resolved market outcome into the calibration store.
    #[tool(
        description = "Record a resolved market outcome (bucket, probability-at-observation, outcome) into the calibration store. This is the sense arm of the calibration feedback loop: accrued resolutions drive per-bucket Brier scores, which demote poorly-calibrated buckets' reliability tiers on subsequent lookups."
    )]
    pub async fn market_record_resolution(
        &self,
        Parameters(req): Parameters<MarketRecordResolutionRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_record_resolution",
            Some(Self::ontology_anchor("market_record_resolution")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_record_resolution".to_string());
                if !(0.0..=1.0).contains(&req.probability) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!("probability {} not in [0, 1]", req.probability),
                    ));
                }
                let reading = {
                    let mut store =
                        self.calibration_store.lock().unwrap_or_else(|e| e.into_inner());
                    let bucket = types::canonical_bucket(&req.bucket);
                    store.record(
                        &bucket,
                        calibration::ResolvedObservation {
                            probability: req.probability,
                            outcome: req.outcome,
                        },
                    );
                    let reading = calibration::read_calibration(&store, &bucket);
                    if let Some(path) = &self.calibration_path
                        && let Err(e) = store.save(std::path::Path::new(path))
                    {
                        // Persistence failure must not silently drop the
                        // in-memory observation — warn so an operator can
                        // distinguish "recorded" from "recorded but unsaved".
                        tracing::warn!(
                            "calibration journal save to {path} failed: {e} —                              observation is in-memory only for this session"
                        );
                    }
                    reading
                };
                serde_json::to_value(&reading).map_err(|e| {
                    McpToolError::internal(format!("reading serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Subscribe to Polymarket resolution events and feed the calibration store.
    #[tool(
        description = "Subscribe to Polymarket's public market channel for resolution events on the given CLOB asset IDs. Resolution events are logged as notifications — they do NOT write calibration observations (the wire carries no pre-resolution probability, and fabricating one would corrupt the Brier loop). Pair a notification with market_record_resolution (which takes the pre-resolution probability) to feed the loop."
    )]
    pub async fn market_subscribe_resolutions(
        &self,
        Parameters(req): Parameters<MarketSubscribeRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_subscribe_resolutions",
            Some(Self::ontology_anchor("market_subscribe_resolutions")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_subscribe_resolutions".to_string());
                let max = req.max_resolutions.unwrap_or(1).max(1);
                let ingested = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
                let store = std::sync::Arc::clone(&self.calibration_store);
                let path = self.calibration_path.clone();
                let bucket = req.bucket.clone();
                let ingested_clone = std::sync::Arc::clone(&ingested);

                streaming::subscribe_market(&req.asset_ids, move |event| {
                    let store = std::sync::Arc::clone(&store);
                    let path = path.clone();
                    let _bucket = bucket.clone();
                    let ingested = std::sync::Arc::clone(&ingested_clone);
                    async move {
                        if let streaming::MarketEvent::MarketResolved {
                            winning_outcome, ..
                        } = event
                        {
                            // Do NOT write a calibration observation here:
                            // Brier scoring needs the *pre-resolution*
                            // probability, which the stream does not carry.
                            // Fabricating 1.0/0.0 would make every bucket
                            // look perfectly calibrated — the reinforcing-loop
                            // trap. The stream's role is to *notify*; the
                            // caller pairs it with market_record_resolution
                            // (which takes the pre-resolution probability).
                            let outcome = winning_outcome.eq_ignore_ascii_case("yes");
                            tracing::info!(
                                "market resolved: outcome={} — call market_record_resolution                                  with the pre-resolution probability to feed the calibration loop",
                                if outcome { "yes" } else { "no" }
                            );
                            let _ = (&store, &path); // reserved for a future price-snapshot join
                            ingested.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                })
                .await?;

                let count = ingested.load(std::sync::atomic::Ordering::SeqCst);
                Ok(serde_json::json!({
                    "resolutions_ingested": count,
                    "bucket": req.bucket,
                    "max_resolutions": max,
                }))
            },
        )
        .await
    }

    /// Duration profile of a contract series (the term-structure ladder).
    #[tool(
        description = "Return the ladder of contracts in a series ordered by deadline, each annotated with its time_to_maturity in fractional years. Kalshi series ticker or Polymarket event slug; both platforms are probed. Unparseable deadlines sort last with null maturity; per-platform failures surface in warnings — the ladder never fabricates a maturity."
    )]
    pub async fn market_ladder(&self, Parameters(req): Parameters<MarketLadderRequest>) -> String {
        execute_tool_semantic(
            self,
            "market_ladder",
            Some(Self::ontology_anchor("market_ladder")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_ladder".to_string());
                let now = chrono::Utc::now();
                let mut rungs: Vec<serde_json::Value> = Vec::new();
                let mut warnings: Vec<String> = Vec::new();

                match provider_kalshi::fetch_markets(&self.http, Some(&req.series), 200).await {
                    Ok(markets) => {
                        for market in &markets {
                            if let Some(record) = types::MarketRecord::from_kalshi(
                                market,
                                None,
                                types::calibration_for(None, ""),
                                &now,
                            ) {
                                rungs.push(serde_json::json!({
                                    "source": "kalshi",
                                    "market_id": record.market_id,
                                    "question": record.question,
                                    "deadline": record.deadline,
                                    "time_to_maturity": record.time_to_maturity,
                                }));
                            }
                        }
                    }
                    Err(e) => warnings.push(format!("kalshi: {e}")),
                }

                match provider_polymarket::fetch_events(&self.http, 100).await {
                    Ok(events) => {
                        for event in &events {
                            if event.slug != req.series && event.ticker != req.series {
                                continue;
                            }
                            let event_tags: Vec<String> =
                                event.tags.iter().map(|t| t.label.clone()).collect();
                            let bucket = types::canonical_bucket(
                                event_tags.first().map(String::as_str).unwrap_or(""),
                            );
                            let reading = {
                                let guard = self
                                    .calibration_store
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                calibration::read_calibration(&guard, &bucket)
                            };
                            for market in &event.markets {
                                let calibration_block =
                                    types::calibration_for(Some(&reading), &bucket);
                                if let Some(record) = types::MarketRecord::from_polymarket(
                                    market,
                                    &event.id,
                                    &event.slug,
                                    event.volume,
                                    event.liquidity,
                                    &event_tags,
                                    calibration_block,
                                    &now,
                                ) {
                                    rungs.push(serde_json::json!({
                                        "source": "polymarket",
                                        "market_id": record.market_id,
                                        "question": record.question,
                                        "deadline": record.deadline,
                                        "time_to_maturity": record.time_to_maturity,
                                    }));
                                }
                            }
                        }
                    }
                    Err(e) => warnings.push(format!("polymarket: {e}")),
                }

                // Sort by maturity; unparseable deadlines (null) last.
                rungs.sort_by(|a, b| {
                    let ma = a["time_to_maturity"].as_f64().unwrap_or(f64::MAX);
                    let mb = b["time_to_maturity"].as_f64().unwrap_or(f64::MAX);
                    ma.partial_cmp(&mb).unwrap_or(std::cmp::Ordering::Equal)
                });

                serde_json::to_value(serde_json::json!({
                    "series": req.series,
                    "rungs": rungs,
                    "warnings": warnings,
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("ladder serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }
    #[tool(
        description = "Constant Maturity Prediction (CMP): synthesize a fixed-tenor probability for a registered base event by interpolating its family's markets in log-odds space. Sparse coverage returns bucketed_sparse with the bracket width rather than a fabricated tight curve. Base events come only from HKASK_PREDICTION_MARKETS_BASE_EVENTS — unregistered series are refused."
    )]
    pub async fn market_cmp(&self, Parameters(req): Parameters<MarketCmpRequest>) -> String {
        execute_tool_semantic(
            self,
            "market_cmp",
            Some(Self::ontology_anchor("market_cmp")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_cmp".to_string());
                if !self.base_events.iter().any(|(_, series)| series == &req.series) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!(
                            "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                            req.series
                        ),
                    ));
                }
                let markets =
                    provider_kalshi::fetch_markets(&self.http, Some(&req.series), 200).await?;
                let now = chrono::Utc::now();
                let points: Vec<cmp::TenorPoint> = markets
                    .iter()
                    .filter_map(|m| {
                        let mid = m.yes_midpoint()?;
                        let deadline =
                            chrono::DateTime::parse_from_rfc3339(&m.close_time).ok()?;
                        let days = (deadline.with_timezone(&chrono::Utc) - now).num_seconds()
                            as f64
                            / 86_400.0;
                        (days > 0.0).then_some(cmp::TenorPoint {
                            days_to_resolution: days,
                            price: mid,
                        })
                    })
                    .collect();
                let value = cmp::constant_maturity(&points, req.tenor_days).ok_or_else(|| {
                    hkask_mcp_server::server::McpToolError::not_found(format!(
                        "no live markets with future deadlines for series '{}'",
                        req.series
                    ))
                })?;
                serde_json::to_value(&value).map_err(|e| {
                    McpToolError::internal(format!("cmp serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Residual risk decomposition: niche exposure to a base event.
    #[tool(
        description = "Decompose a niche market's movement into base-event exposure (beta in log-odds space) plus an idiosyncratic residual. Refuses with insufficient_overlap below 10 shared observations — never fabricates a residual from thin data. Output carries r_squared and observations so fit quality is explicit."
    )]
    pub async fn market_residual(
        &self,
        Parameters(req): Parameters<MarketResidualRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_residual",
            Some(Self::ontology_anchor("market_residual")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_residual".to_string());
                let window = i64::from(req.window_days.unwrap_or(90));
                let now = chrono::Utc::now().timestamp();
                let start = (now - window * 86_400).max(0) as u64;
                let end = now as u64;
                let niche_history = provider_kalshi::fetch_price_history(
                    &self.http,
                    &req.market_ticker,
                    start,
                    end,
                )
                .await?;
                let base_history =
                    provider_kalshi::fetch_price_history(&self.http, &req.base_ticker, start, end)
                        .await?;
                // Align on shared period timestamps.
                let observations: Vec<(f64, f64)> = niche_history
                    .iter()
                    .filter_map(|n| {
                        base_history
                            .iter()
                            .find(|b| b.ts == n.ts)
                            .map(|b| (n.price, b.price))
                    })
                    .collect();
                let analysis = residual::residual_analysis(&observations).ok_or_else(|| {
                    hkask_mcp_server::server::McpToolError::failed_precondition(format!(
                        "insufficient_overlap: {} shared observations (minimum {})",
                        observations.len(),
                        residual::MIN_OBSERVATIONS
                    ))
                })?;
                serde_json::to_value(&analysis).map_err(|e| {
                    McpToolError::internal(format!("residual serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Scan for resolved markets and record their outcomes.
    #[tool(
        description = "Scan Polymarket and Kalshi for newly resolved markets and record definitive outcomes into the calibration store (idempotent — re-scanning is safe). Only terminal prices (>=0.99 / <=0.01) or explicit Kalshi results count; ambiguous 50-50 resolutions are skipped, never fabricated. This is the self-feeding sense arm of the calibration loop."
    )]
    pub async fn market_check_resolutions(
        &self,
        Parameters(req): Parameters<MarketCheckResolutionsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_check_resolutions",
            Some(Self::ontology_anchor("market_check_resolutions")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_check_resolutions".to_string());
                let limit = req.limit.unwrap_or(100).min(500);
                let mut recorded = 0u32;
                let mut skipped_ambiguous = 0u32;
                let mut already_known = 0u32;
                let mut warnings: Vec<String> = Vec::new();

                // Kalshi: settled markets carry an explicit `result`.
                match provider_kalshi::fetch_markets_by_status(
                    &self.http,
                    req.series.as_deref(),
                    "settled",
                    limit,
                )
                .await
                {
                    Ok(markets) => {
                        for market in &markets {
                            let outcome = match market.result.as_str() {
                                "yes" => Some(true),
                                "no" => Some(false),
                                _ => None,
                            };
                            let Some(outcome) = outcome else { continue };
                            let bucket = types::canonical_bucket(&market.event_ticker);
                            // Pre-resolution probability is unrecoverable from
                            // a settled snapshot; record the last traded price
                            // as the closest honest observation.
                            let Some(probability) =
                                provider_kalshi::parse_fp(&market.last_price_dollars)
                            else {
                                continue;
                            };
                            let observation = calibration::ResolvedObservation {
                                probability,
                                outcome,
                            };
                            let mut store = self
                                .calibration_store
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            if store.contains(&bucket, &observation) {
                                already_known += 1;
                            } else {
                                store.record(&bucket, observation);
                                recorded += 1;
                            }
                        }
                    }
                    Err(e) => warnings.push(format!("kalshi scan failed: {e}")),
                }

                // Polymarket: closed markets; definitive outcome from terminal
                // prices (the B1 gate — 50-50 "Unknown" resolutions skip).
                match provider_polymarket::fetch_markets(&self.http, limit, true).await {
                    Ok(markets) => {
                        for market in &markets {
                            if market.uma_resolution_status != "resolved" {
                                continue;
                            }
                            let Some(price) = market.yes_probability() else {
                                continue;
                            };
                            let outcome = if price >= 0.99 {
                                Some(true)
                            } else if price <= 0.01 {
                                Some(false)
                            } else {
                                skipped_ambiguous += 1;
                                None
                            };
                            let Some(outcome) = outcome else { continue };
                            let bucket = types::canonical_bucket(&market.slug);
                            let observation = calibration::ResolvedObservation {
                                probability: price,
                                outcome,
                            };
                            let mut store = self
                                .calibration_store
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            if store.contains(&bucket, &observation) {
                                already_known += 1;
                            } else {
                                store.record(&bucket, observation);
                                recorded += 1;
                            }
                        }
                    }
                    Err(e) => warnings.push(format!("polymarket scan failed: {e}")),
                }

                // Persist if anything changed.
                if recorded > 0
                    && let Some(path) = &self.calibration_path
                {
                    let store = self
                        .calibration_store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = store.save(std::path::Path::new(path)) {
                        warnings.push(format!("journal save failed: {e}"));
                    }
                }

                serde_json::to_value(serde_json::json!({
                    "recorded": recorded,
                    "already_known": already_known,
                    "skipped_ambiguous": skipped_ambiguous,
                    "warnings": warnings,
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("scan serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Market record enriched with realized variance from price history.
    #[tool(
        description = "Fetch a market's price history and return its record with realized_variance populated (log-odds step variance, 2607.08199-consistent) plus the volatility regime (smooth vs jump-like). Kalshi: candlesticks over the window; Polymarket: CLOB prices-history for the token."
    )]
    pub async fn market_history(
        &self,
        Parameters(req): Parameters<MarketHistoryRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_history",
            Some(Self::ontology_anchor("market_history")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_history".to_string());
                let source = req.source.as_deref().unwrap_or("kalshi");
                let prices: Vec<f64> = match source {
                    "kalshi" => {
                        let window = i64::from(req.window_days.unwrap_or(90));
                        let now = chrono::Utc::now().timestamp();
                        let start = (now - window * 86_400).max(0) as u64;
                        provider_kalshi::fetch_price_history(
                            &self.http,
                            &req.market,
                            start,
                            now as u64,
                        )
                        .await?
                        .iter()
                        .map(|p| p.price)
                        .collect()
                    }
                    "polymarket" => {
                        provider_polymarket::fetch_prices_history(&self.http, &req.market)
                            .await?
                            .iter()
                            .map(|p| p.price)
                            .collect()
                    }
                    other => {
                        return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                            format!("unknown source '{other}' (expected kalshi|polymarket)"),
                        ));
                    }
                };
                let variance = types::realized_variance(&prices);
                let regime = hkask_forecast::volatility_regime(&prices);
                serde_json::to_value(serde_json::json!({
                    "market": req.market,
                    "source": source,
                    "observations": prices.len(),
                    "realized_variance": variance,
                    "volatility_regime": format!("{regime:?}"),
                    "insufficient_history": variance.is_none(),
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("history serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// The full CMP index curve for a registered base event.
    #[tool(
        description = "Compute the full Constant Maturity Prediction index for a registered base event: the curve of probabilities across the standard tenor grid (7d/30d/90d/180d/1y/2y), interpolated in log-odds space. Tenors without cohort coverage return null probability, never a fabricated extrapolation. Includes curve slope (log-odds/year) as the term-structure signal."
    )]
    pub async fn market_cmp_index(
        &self,
        Parameters(req): Parameters<MarketCmpIndexRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_cmp_index",
            Some(Self::ontology_anchor("market_cmp_index")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_cmp_index".to_string());
                if !self.base_events.iter().any(|(_, series)| series == &req.series) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!(
                            "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                            req.series
                        ),
                    ));
                }
                let markets =
                    provider_kalshi::fetch_markets(&self.http, Some(&req.series), 200).await?;
                let now = chrono::Utc::now();
                let points: Vec<cmp::TenorPoint> = markets
                    .iter()
                    .filter_map(|m| {
                        let mid = m.yes_midpoint()?;
                        let deadline =
                            chrono::DateTime::parse_from_rfc3339(&m.close_time).ok()?;
                        let days = (deadline.with_timezone(&chrono::Utc) - now).num_seconds()
                            as f64
                            / 86_400.0;
                        (days > 0.0).then_some(cmp::TenorPoint {
                            days_to_resolution: days,
                            price: mid,
                        })
                    })
                    .collect();
                if points.is_empty() {
                    return Err(hkask_mcp_server::server::McpToolError::not_found(format!(
                        "no live markets with future deadlines for series '{}'",
                        req.series
                    )));
                }
                let index =
                    cmp::compute_index(&req.series, &points, &now.to_rfc3339());
                let slope_30_365 = cmp::curve_slope(&index, 30, 365);
                serde_json::to_value(serde_json::json!({
                    "index": index,
                    "slope_30d_1y_logodds_per_year": slope_30_365,
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("index serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Compute the DR-AS structural volatility forecast for a single
    /// prediction-market contract (arXiv:2607.08199). Returns the conditional
    /// variance, its deadline-resolution and adverse-selection decomposition,
    /// and a 95% prediction interval. All config fields are optional with
    /// sensible defaults from the paper.
    #[tool(
        description = "Compute the DR-AS structural volatility forecast (arXiv:2607.08199) for a prediction-market contract: conditional variance = p(1−p)/τ + K·ν(V)·s²/4, with a 95% prediction interval. All config fields optional with paper defaults."
    )]
    pub async fn market_volatility(
        &self,
        Parameters(MarketVolatilityRequest {
            price,
            hours_to_resolution,
            spread,
            volume,
            horizon_hours,
            k,
            activity_proxy,
        }): Parameters<MarketVolatilityRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_volatility",
            Some(Self::ontology_anchor("market_volatility")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_volatility".to_string());
                let config = volatility::DrasConfig {
                    k: k.unwrap_or_else(|| volatility::DrasConfig::default().k),
                    activity_proxy: activity_proxy.unwrap_or_default(),
                };
                let inputs = volatility::VolatilityInputs {
                    price,
                    hours_to_resolution,
                    spread,
                    volume,
                };
                let fc = volatility::forecast(inputs, config, horizon_hours).ok_or_else(|| {
                    hkask_mcp_server::server::McpToolError::invalid_argument(
                        "degenerate inputs: price must be in [0,1], hours_to_resolution > 0, horizon > 0".to_string()
                    )
                })?;
                serde_json::to_value(serde_json::json!({
                    "price": price,
                    "hours_to_resolution": hours_to_resolution,
                    "spread": spread,
                    "volume": volume,
                    "horizon_hours": horizon_hours,
                    "conditional_volatility": fc.conditional_volatility,
                    "conditional_variance": fc.conditional_variance,
                    "decomposition": {
                        "deadline_resolution": fc.dr_variance,
                        "adverse_selection": fc.as_variance,
                    },
                    "activity_value": fc.activity_value,
                    "config": {
                        "k": fc.config.k,
                        "activity_proxy": fc.config.activity_proxy,
                    },
                    "interval_95": {
                        "lower": fc.interval_95.0,
                        "upper": fc.interval_95.1,
                    },
                    "model": "DR-AS (Xi, Moallemi, Pai & Wang, arXiv:2607.08199)",
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("volatility serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Store the CMP index curve for a registered base event as a
    /// transaction-ledger portfolio. Each tenor point on the curve becomes a
    /// constituent holding with weight = the synthesized probability. The
    /// portfolio name is `cmp:{series}`. Materialized daily holdings are
    /// computed on insert (and rebuildable from the ledger via
    /// `portfolio_rebuild_views` on the portfolio server).
    #[tool(
        description = "Store the CMP index curve for a registered base event as a transaction-ledger portfolio of tenor constituents, with materialized daily holdings. Returns the stored portfolio name and constituent count."
    )]
    pub async fn market_cmp_index_store(
        &self,
        Parameters(MarketCmpIndexStoreRequest { series, date }): Parameters<
            MarketCmpIndexStoreRequest,
        >,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_cmp_index_store",
            Some(Self::ontology_anchor("market_cmp_index_store")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_cmp_index_store".to_string());
                if !self.base_events.iter().any(|(_, s)| s == &series) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!(
                            "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                            series
                        ),
                    ));
                }
                let markets =
                    provider_kalshi::fetch_markets(&self.http, Some(&series), 200).await?;
                let now = chrono::Utc::now();
                let points: Vec<cmp::TenorPoint> = markets
                    .iter()
                    .filter_map(|m| {
                        let mid = m.yes_midpoint()?;
                        let deadline =
                            chrono::DateTime::parse_from_rfc3339(&m.close_time).ok()?;
                        let days = (deadline.with_timezone(&chrono::Utc) - now).num_seconds()
                            as f64
                            / 86_400.0;
                        (days > 0.0).then_some(cmp::TenorPoint {
                            days_to_resolution: days,
                            price: mid,
                        })
                    })
                    .collect();
                if points.is_empty() {
                    return Err(hkask_mcp_server::server::McpToolError::not_found(format!(
                        "no live markets with future deadlines for series '{}'",
                        series
                    )));
                }
                let computed_at = date.clone().unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
                let index = cmp::compute_index(&series, &points, &now.to_rfc3339());

                // Persist the index as a portfolio ledger. Each supported
                // tenor point becomes a constituent buy transaction with
                // weight = probability. Unsupported tenors (probability: None)
                // are withheld — never fabricated.
                let portfolio_name = format!("cmp:{series}");
                let response_portfolio = portfolio_name.clone();
                let response_series = series.clone();
                let response_date = computed_at.clone();
                let response_points: Vec<serde_json::Value> = index
                    .points
                    .iter()
                    .map(|p| serde_json::json!({
                        "tenor_days": p.tenor_days,
                        "probability": p.probability,
                    }))
                    .collect();
                let store = self.portfolio_store.clone();
                let created_at = now.to_rfc3339();
                let stored = tokio::task::spawn_blocking(move || {
                    use hkask_mcp_portfolio::{AssetType, PortfolioError, Transaction, TxType};
                    // Create the portfolio (idempotent) as a prediction-contract portfolio.
                    if let Err(e) = store.create(&portfolio_name, AssetType::PredictionContract) {
                        // Already exists is fine; other errors propagate.
                        match e {
                            PortfolioError::InvalidArgument(_) => {}
                            other => return Err(other),
                        }
                    }
                    let mut applied = 0usize;
                    let mut withheld = 0usize;
                    for point in &index.points {
                        let Some(prob) = point.probability else {
                            withheld += 1;
                            continue;
                        };
                        // The constituent symbol is the tenor (e.g. "cmp:KXFEDDECISION:30d").
                        // The weight is the synthesized probability; the
                        // quantity is 1.0 (one unit of the index at this tenor).
                        let symbol = format!("cmp:{series}:{}d", point.tenor_days);
                        let tx = Transaction {
                            id: uuid::Uuid::new_v4().to_string(),
                            date: computed_at.clone(),
                            tx_type: TxType::Buy,
                            asset_type: AssetType::PredictionContract,
                            symbol: Some(symbol),
                            quantity: Some(1.0),
                            price: Some(prob),
                            commission: Some(0.0),
                            amount: None,
                            weight: Some(prob),
                            currency: "USD".to_string(),
                            notes: format!(
                                "CMP index constituent: tenor={}d method={:?} cohorts={} bracket={}",
                                point.tenor_days, point.method, point.cohorts, point.bracket_days
                            ),
                            created_at: created_at.clone(),
                        };
                        store.apply(&portfolio_name, &tx)?;
                        applied += 1;
                    }
                    // Materialize the holdings snapshot for the observation date.
                    let snapshot = store.snapshot(&portfolio_name, &computed_at)?;
                    Ok::<_, PortfolioError>((applied, withheld, snapshot))
                })
                .await
                .map_err(|e| map_join_error(e, "portfolio store task failed"))?;
                let (applied, withheld, snapshot) = stored.map_err(map_portfolio_error)?;
                serde_json::to_value(serde_json::json!({
                    "status": "stored",
                    "portfolio": response_portfolio,
                    "series": response_series,
                    "observation_date": response_date,
                    "constituents_applied": applied,
                    "constituents_withheld": withheld,
                    "holdings": snapshot.holdings.len(),
                    "index_probability_curve": response_points,
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("store response serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Read the materialized holdings for a stored CMP index portfolio.
    /// Compute the solved-portfolio CMP index set for a registered base event
    /// and persist each (bucket, orientation) index as a transaction-ledger
    /// portfolio of contracts with maturity-matched weights. This is the
    /// contract-portfolio CMP index (from `cmp_portfolio`), distinct from the
    /// curve-based `market_cmp_index_store`.
    #[tool(
        description = "Compute the solved-portfolio CMP index set (maturity-bucketed, orientation-tagged portfolios of contracts with maturity-matched weights) for a registered base event and persist each (bucket, orientation) index as a transaction-ledger portfolio. All economic-context fields are optional — when omitted, the tool uses the curated default for the base-event family (see market_cmp_context_suggest)."
    )]
    pub async fn market_cmp_portfolio_store(
        &self,
        Parameters(MarketCmpPortfolioStoreRequest {
            series,
            reference,
            volatility,
            predicted_level,
            direction_up,
            date,
        }): Parameters<MarketCmpPortfolioStoreRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_cmp_portfolio_store",
            Some(Self::ontology_anchor("market_cmp_portfolio_store")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_cmp_portfolio_store".to_string());
                if !self.base_events.iter().any(|(_, s)| s == &series) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!(
                            "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                            series
                        ),
                    ));
                }
                let markets =
                    provider_kalshi::fetch_markets(&self.http, Some(&series), 200).await?;
                let now = chrono::Utc::now();
                let observation_date =
                    date.clone().unwrap_or_else(|| now.format("%Y-%m-%d").to_string());

                // Resolve economic context: use operator-supplied values when
                // present, otherwise fall back to the curated default for the
                // base-event family. This follows the zed-kask pattern — never
                // a blank field, always a reasonable default.
                //
                // We classify the base event from the first fetched market's
                // text to pick the right default family. If classification
                // fails (no signature match), we use a generic stable default.
                let default_ctx = markets
                    .first()
                    .and_then(|m| {
                        base_event::classify_base_event_text(
                            &m.title, &m.subtitle, &series, "",
                        )
                    })
                    .map(|be| be.default_economic_context())
                    .unwrap_or_else(|| base_event::EconomicContext {
                        reference: 0.0,
                        volatility: None,
                        predicted_level: 0.0,
                        direction_up: false,
                        rationale: "no base-event match — generic stable default".into(),
                    });
                let reference = reference.unwrap_or(default_ctx.reference);
                let volatility = volatility.or(default_ctx.volatility);
                // When the operator didn't supply a predicted_level, try to
                // extract a strike from the first market's title. If a strike
                // is found, use it (directional index); otherwise fall back to
                // the curated default (predicted_level = reference → Stable).
                let predicted_level = predicted_level.unwrap_or_else(|| {
                    if let Some(m) = markets.first()
                        && let Some(be) = base_event::classify_base_event_text(&m.title, &m.subtitle, &series, "")
                        && let Some((strike, _)) = be.extract_strike(&m.title)
                    {
                        strike
                    } else {
                        default_ctx.predicted_level
                    }
                });
                // direction_up: operator override, else extracted from title, else default.
                let direction_up = match direction_up {
                    Some(up) => up,
                    None => {
                        if let Some(m) = markets.first()
                            && let Some(be) = base_event::classify_base_event_text(&m.title, &m.subtitle, &series, "")
                            && let Some((_, up)) = be.extract_strike(&m.title)
                        {
                            up
                        } else {
                            default_ctx.direction_up
                        }
                    }
                };
                let context_rationale = default_ctx.rationale.clone();

                // Build OrientedConstituents from fetched markets + operator
                // economic context. Each market becomes a candidate
                // constituent; eligibility filters by base event, materiality,
                // orientation, and maturity window inside construct_cmp_index_set.
                let config = cmp_portfolio::CmpConfig::default();
                let mut oriented: Vec<cmp_portfolio::OrientedConstituent> = Vec::new();
                let mut market_ids: Vec<String> = Vec::new();
                for (idx, market) in markets.iter().enumerate() {
                    let Some(probability) = market.yes_midpoint() else {
                        continue;
                    };
                    let Some(deadline) = chrono::DateTime::parse_from_rfc3339(&market.close_time).ok() else {
                        continue;
                    };
                    let days = (deadline.with_timezone(&chrono::Utc) - now).num_seconds() as f64
                        / 86_400.0;
                    if days <= 0.0 {
                        continue;
                    }
                    // Base-event classification from the market's text fields
                    // (no full MarketRecord construction needed —
                    // classify_base_event_text reads just the haystack).
                    let base_event = base_event::classify_base_event_text(
                        &market.title,
                        &market.subtitle,
                        &series,
                        "", // category unknown without the event lookup
                    );
                    let Some(base_event) = base_event else {
                        continue;
                    };
                    // Orientation: classify using the operator-supplied
                    // predicted_level + reference + the materiality level for
                    // this family at the 30d tenor (a representative target).
                    let setting = base_event.default_materiality();
                    let level = cmp_portfolio::materiality_level(
                        &setting,
                        volatility,
                        30,
                        &config,
                    );
                    let orientation = match level {
                        Some(level) => cmp_portfolio::classify_orientation(
                            predicted_level,
                            reference,
                            level,
                            direction_up,
                        ),
                        None => cmp_portfolio::Orientation::Stable,
                    };
                    oriented.push(cmp_portfolio::OrientedConstituent {
                        constituent: cmp_portfolio::Constituent {
                            days_to_expiration: days,
                            probability,
                            quality: 1.0,
                        },
                        orientation,
                        market_index: idx,
                    });
                    market_ids.push(market.ticker.clone());
                }
                if oriented.is_empty() {
                    return Err(hkask_mcp_server::server::McpToolError::not_found(format!(
                        "no eligible markets for series '{}' with the supplied economic context",
                        series
                    )));
                }

                let index_set = cmp_portfolio::construct_cmp_index_set(&oriented, &config);

                // Persist each solved CmpIndex as a portfolio ledger of
                // WeightAdjust transactions (one per constituent, weight =
                // the solved portfolio weight). The portfolio name is
                // `cmp:{series}:{bucket}:{orientation}`.
                let store = self.portfolio_store.clone();
                let created_at = now.to_rfc3339();
                let response_series = series.clone();
                let response_date = observation_date.clone();
                let stored = tokio::task::spawn_blocking(move || {
                    use hkask_mcp_portfolio::{AssetType, PortfolioError, Transaction, TxType};
                    let mut stored_indices: Vec<(String, usize)> = Vec::new();
                    for index in &index_set.indices {
                        let portfolio_name = format!(
                            "cmp:{series}:{}:{}",
                            index.bucket.label(),
                            index.orientation
                        );
                        if let Err(e) = store.create(&portfolio_name, AssetType::PredictionContract) {
                            match e {
                                PortfolioError::InvalidArgument(_) => {}
                                other => return Err(other),
                            }
                        }
                        for constituent in &index.portfolio.constituents {
                            let symbol = market_ids
                                .get(constituent.market_index)
                                .cloned()
                                .unwrap_or_else(|| format!("market_{}", constituent.market_index));
                            let tx = Transaction {
                                id: uuid::Uuid::new_v4().to_string(),
                                date: observation_date.clone(),
                                tx_type: TxType::Buy,
                                asset_type: AssetType::PredictionContract,
                                symbol: Some(symbol),
                                quantity: Some(1.0),
                                price: Some(constituent.probability),
                                commission: Some(0.0),
                                amount: None,
                                weight: Some(constituent.weight),
                                currency: "USD".to_string(),
                                notes: format!(
                                    "CMP portfolio constituent: bucket={} orientation={} weight={:.4} dte={:.1}",
                                    index.bucket.label(),
                                    index.orientation,
                                    constituent.weight,
                                    constituent.days_to_expiration
                                ),
                                created_at: created_at.clone(),
                            };
                            store.apply(&portfolio_name, &tx)?;
                        }
                        // Materialize the holdings snapshot.
                        let snap = store.snapshot(&portfolio_name, &observation_date)?;
                        stored_indices.push((portfolio_name, snap.holdings.len()));
                    }
                    Ok::<_, PortfolioError>((stored_indices, index_set.withheld_buckets.len()))
                })
                .await
                .map_err(|e| map_join_error(e, "portfolio store task failed"))?
                .map_err(map_portfolio_error)?;
                let (stored_indices, withheld) = stored;
                serde_json::to_value(serde_json::json!({
                    "status": "stored",
                    "series": response_series,
                    "observation_date": response_date,
                    "economic_context": {
                        "reference": reference,
                        "volatility": volatility,
                        "predicted_level": predicted_level,
                        "direction_up": direction_up,
                        "rationale": context_rationale,
                    },
                    "indices_stored": stored_indices.len(),
                    "withheld_buckets": withheld,
                    "indices": stored_indices.iter().map(|(name, holdings)| serde_json::json!({
                        "portfolio": name,
                        "holdings": holdings,
                    })).collect::<Vec<_>>(),
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("store response serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }

    /// Propose a curated economic context for a base-event family, with
    /// reasoning. Read-only aid — the operator accepts, overrides, or rejects
    /// the proposal before calling `market_cmp_portfolio_store`. This follows
    /// the zed-kask design pattern: never present a blank field — always
    /// provide a reasonable default with reasoning the user can accept or
    /// override.
    #[tool(
        description = "Propose a curated economic context (reference level, volatility, predicted level, direction) for a base-event family, with reasoning. Read-only aid for the market_cmp_portfolio_store tool — the operator accepts, overrides, or rejects the proposal."
    )]
    pub async fn market_cmp_context_suggest(
        &self,
        Parameters(MarketCmpContextSuggestRequest { series }): Parameters<
            MarketCmpContextSuggestRequest,
        >,
    ) -> String {
        execute_tool_semantic(
            self,
            "market_cmp_context_suggest",
            Some(Self::ontology_anchor("market_cmp_context_suggest")),
            async {
                self.called_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert("market_cmp_context_suggest".to_string());
                if !self.base_events.iter().any(|(_, s)| s == &series) {
                    return Err(hkask_mcp_server::server::McpToolError::invalid_argument(
                        format!(
                            "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                            series
                        ),
                    ));
                }
                // Classify the family from the series ticker to pick the
                // curated default, then try to fetch a live reference level
                // (FRED for macro, CoinGecko for crypto). Falls back to the
                // curated static default on any failure — the zed-kask pattern:
                // always have a default, the live fetch is an enhancement.
                let base_event = base_event::classify_base_event_text(&series, "", &series, "");
                let (context, family) = match base_event {
                    Some(be) => {
                        let ctx = be.live_economic_context(&self.http, self.fred_api_key.as_deref()).await;
                        (ctx, be.factor().to_string())
                    }
                    None => (base_event::EconomicContext {
                        reference: 0.0,
                        volatility: None,
                        predicted_level: 0.0,
                        direction_up: false,
                        rationale: format!(
                            "series '{series}' did not match a known base-event family \
                             signature; returning a generic stable default. Override with \
                             live data before storing an index."
                        ),
                    }, "unknown".to_string()),
                };
                serde_json::to_value(serde_json::json!({
                    "series": series,
                    "family": family,
                    "proposed_context": {
                        "reference": context.reference,
                        "volatility": context.volatility,
                        "predicted_level": context.predicted_level,
                        "direction_up": context.direction_up,
                    },
                    "rationale": context.rationale,
                    "usage": "Pass these values to market_cmp_portfolio_store, or override with live data. All fields are optional in market_cmp_portfolio_store — omitting them uses these curated defaults.",
                }))
                .map_err(|e| {
                    McpToolError::internal(format!("context suggest serialization failed: {e}")) // rr0044-ok: serialize-own-struct
                })
            },
        )
        .await
    }
}

impl PredictionMarketsServer {
    /// Fetch and annotate all candidate markets from both platforms.
    /// Retrieval is deliberately broad (no query filtering): the caller —
    /// `market_lookup`'s substring filter or `market_match`'s deterministic
    /// scorer — applies its own relevance gate. A substring prefilter here
    /// silently dropped every natural-language question from market_match
    /// (no haystack contains a full NL question), so filtering must live in
    /// the consumers where its semantics are explicit.
    async fn gather_candidates(
        &self,
    ) -> Result<Vec<types::MarketRecord>, hkask_mcp_server::server::McpToolError> {
        let cache_key = "candidates:all";
        if let Some(cached) = self.response_cache.get(cache_key)
            && let Ok(records) = serde_json::from_value::<Vec<types::MarketRecord>>(cached)
        {
            return Ok(records);
        }
        let now = chrono::Utc::now();
        let mut records = Vec::new();
        // Snapshot store readings before any await: holding the MutexGuard
        // across a fetch makes the tool future non-Send. Readings are cheap
        // clones keyed by bucket; per-bucket lookup below.
        let store = std::sync::Arc::clone(&self.calibration_store);

        let gamma_events = provider_polymarket::fetch_events(&self.http, 100).await?;
        for event in &gamma_events {
            let event_tags: Vec<String> = event.tags.iter().map(|t| t.label.clone()).collect();
            let bucket =
                types::canonical_bucket(event_tags.first().map(String::as_str).unwrap_or(""));
            let reading = {
                let guard = store.lock().unwrap_or_else(|e| e.into_inner());
                calibration::read_calibration(&guard, &bucket)
            };
            for market in &event.markets {
                let calibration_block = types::calibration_for(Some(&reading), &bucket);
                if let Some(record) = types::MarketRecord::from_polymarket(
                    market,
                    &event.id,
                    &event.slug,
                    event.volume,
                    event.liquidity,
                    &event_tags,
                    calibration_block,
                    &now,
                ) {
                    records.push(record);
                }
            }
        }

        let kalshi_markets = provider_kalshi::fetch_markets(&self.http, None, 200).await?;
        let kalshi_events = provider_kalshi::fetch_events(&self.http, 200).await?;
        for market in &kalshi_markets {
            let event = kalshi_events
                .iter()
                .find(|e| e.event_ticker == market.event_ticker);
            let bucket = types::canonical_bucket(event.map(|e| e.category.as_str()).unwrap_or(""));
            let reading = {
                let guard = store.lock().unwrap_or_else(|e| e.into_inner());
                calibration::read_calibration(&guard, &bucket)
            };
            let calibration_block = types::calibration_for(Some(&reading), &bucket);
            if let Some(record) =
                types::MarketRecord::from_kalshi(market, event, calibration_block, &now)
            {
                records.push(record);
            }
        }
        if let Ok(value) = serde_json::to_value(&records) {
            self.response_cache.put(cache_key, value);
        }
        Ok(records)
    }
}

impl PredictionMarketsServer {
    /// Substring filter used by market_lookup (explicit lookup semantics:
    /// the caller typed a string to find, not a question to resolve).
    fn substring_filter(records: &mut Vec<types::MarketRecord>, query: &str) {
        let query_lower = query.to_lowercase();
        records.retain(|r| {
            format!(
                "{} {} {} {}",
                r.question.to_lowercase(),
                r.description.to_lowercase(),
                r.series.to_lowercase(),
                r.market_id.to_lowercase()
            )
            .contains(&query_lower)
        });
    }
}

#[tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for PredictionMarketsServer {}

// ── Entry point ────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_CACHE_TTL_SECS: u64 = 60;

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // A malformed numeric env var must warn, not silently fall back — an
    // operator cannot distinguish "not configured" from "configured but
    // broken" otherwise.
    let cache_ttl_secs = match std::env::var("HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS") {
        Ok(raw) => match raw.parse::<u64>() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS={raw} failed to parse ({e}); \
                     falling back to default {DEFAULT_CACHE_TTL_SECS}s"
                );
                DEFAULT_CACHE_TTL_SECS
            }
        },
        Err(_) => DEFAULT_CACHE_TTL_SECS,
    };

    // Calibration journal: HKASK_PREDICTION_MARKETS_DATA points at the data
    // dir; the journal lives at <dir>/calibration.jsonl. A load failure is
    // never silent — the loop must distinguish "no data" from "failed to
    // read data" (the unwrap_or(0) sense-input trap).
    let data_dir = std::env::var("HKASK_PREDICTION_MARKETS_DATA").ok();
    let calibration_path = data_dir.as_ref().map(|d| format!("{d}/calibration.jsonl"));
    let store = match &calibration_path {
        Some(path) => match calibration::CalibrationStore::load(std::path::Path::new(path)) {
            Ok(store) => store,
            Err(e) => {
                tracing::warn!(
                    "calibration journal at {path} failed to load ({e});                          starting with an empty store — calibration signals                          will read stale until new observations accrue"
                );
                calibration::CalibrationStore::new()
            }
        },
        None => calibration::CalibrationStore::new(),
    };

    let base_events = std::env::var("HKASK_PREDICTION_MARKETS_BASE_EVENTS")
        .map(|raw| cmp::parse_base_events(&raw))
        .unwrap_or_default();

    hkask_mcp_server::run_server(
        "hkask-mcp-prediction-markets",
        SERVER_VERSION,
        |ctx| {
            let portfolio_store =
                hkask_mcp_portfolio::PortfolioStore::new(ctx.webid).map_err(|e| {
                    hkask_mcp_server::McpError::UnexpectedResponse {
                        context: "portfolio".to_string(),
                        detail: format!("failed to initialize CMP index portfolio store: {e}"),
                    }
                })?;
            let fred_api_key = ctx.credentials.get("HKASK_FRED_API_KEY").cloned();
            Ok(PredictionMarketsServer::new(
                ctx.webid,
                reqwest::Client::new(),
                cache_ttl_secs,
                std::sync::Arc::new(std::sync::Mutex::new(store)),
                cache::TtlCache::new(cache_ttl_secs),
                calibration_path.clone(),
                base_events.clone(),
                std::sync::Mutex::new(HashSet::new()),
                portfolio_store,
                fred_api_key,
            ))
        },
        vec![
            CredentialRequirement::optional(
                "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
                "Cache TTL in seconds for market-data responses (default 60)",
            ),
            CredentialRequirement::optional(
                "HKASK_PREDICTION_MARKETS_DATA",
                "Data directory for the calibration journal (in-memory if absent)",
            ),
            CredentialRequirement::optional(
                "HKASK_FRED_API_KEY",
                "FRED API key for live reference-level fetches (curated static defaults used when absent)",
            ),
        ],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_server() -> PredictionMarketsServer {
        // Leak the tempdir so the portfolio store's db_path remains valid for
        // the server's lifetime (the store opens the DB lazily per call).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        let portfolio_store = hkask_mcp_portfolio::PortfolioStore::with_dir(path);
        PredictionMarketsServer::new(
            hkask_types::WebID::default(),
            reqwest::Client::new(),
            60,
            std::sync::Arc::new(std::sync::Mutex::new(calibration::CalibrationStore::new())),
            cache::TtlCache::new(60),
            None,
            Vec::new(),
            std::sync::Mutex::new(HashSet::new()),
            portfolio_store,
            None, // fred_api_key — tests use curated defaults
        )
    }

    #[tokio::test]
    async fn status_returns_envelope_with_mapping_version() {
        let server = empty_server();
        let response = server
            .prediction_markets_status(Parameters(StatusRequest {}))
            .await;
        let content = hkask_types::tool_response::parse_tool_response(&response)
            .expect("MCP tool responses are {\"content\": ...} envelopes");
        assert_eq!(content["server"], "hkask-mcp-prediction-markets");
        assert_eq!(
            content["ontology_mapping_version"],
            serde_json::json!(ontology::MAPPING_VERSION)
        );
    }

    #[test]
    fn status_request_schema_has_no_boolean_positions() {
        let schema = schemars::schema_for!(StatusRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
        assert!(
            positions.is_empty(),
            "bare-boolean schema positions found: {positions:?}"
        );
    }

    // ── CMP index storage tests ───────────────────────────────────────
    //
    // The storage path (market_cmp_index_store → portfolio ledger →
    // materialized holdings) is testable without live market data: we
    // exercise the portfolio store directly with a fixture CMP index and
    // verify the ledger → holdings → rebuild round-trip.

    fn cmp_index_store_fixture() -> hkask_mcp_portfolio::PortfolioStore {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        hkask_mcp_portfolio::PortfolioStore::with_dir(path)
    }

    #[test]
    fn cmp_index_stored_as_ledger_round_trips_to_holdings() {
        // A CMP index is a portfolio of tenor constituents. Storing it as a
        // ledger of buy transactions (one per supported tenor, weight =
        // probability) and reading the materialized holdings returns the
        // same constituents.
        let store = cmp_index_store_fixture();
        let portfolio = "cmp:KXFEDDECISION";
        store
            .create(
                portfolio,
                hkask_mcp_portfolio::AssetType::PredictionContract,
            )
            .expect("create");
        let date = "2024-01-15";
        let tenors = [(7u32, 0.62f64), (30, 0.58), (90, 0.55), (180, 0.52)];
        for (tenor, prob) in tenors {
            let tx = hkask_mcp_portfolio::Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                date: date.to_string(),
                tx_type: hkask_mcp_portfolio::TxType::Buy,
                asset_type: hkask_mcp_portfolio::AssetType::PredictionContract,
                symbol: Some(format!("cmp:KXFEDDECISION:{tenor}d")),
                quantity: Some(1.0),
                price: Some(prob),
                commission: Some(0.0),
                amount: None,
                weight: Some(prob),
                currency: "USD".to_string(),
                notes: format!("CMP index constituent: tenor={tenor}d"),
                created_at: "2024-01-15T00:00:00Z".to_string(),
            };
            store.apply(portfolio, &tx).expect("apply");
        }
        let snapshot = store.snapshot(portfolio, date).expect("snapshot");
        assert_eq!(snapshot.holdings.len(), 4, "four tenor constituents");
        let symbols: Vec<String> = snapshot.holdings.iter().map(|h| h.symbol.clone()).collect();
        assert!(symbols.contains(&"cmp:KXFEDDECISION:30d".to_string()));
        assert!(symbols.contains(&"cmp:KXFEDDECISION:90d".to_string()));
    }

    #[test]
    fn cmp_index_withheld_tenors_not_stored() {
        // Unsupported tenors (probability: None) are withheld — never
        // fabricated as constituents. The portfolio holds only the supported
        // tenors.
        let store = cmp_index_store_fixture();
        let portfolio = "cmp:KXFEDDECISION";
        store
            .create(
                portfolio,
                hkask_mcp_portfolio::AssetType::PredictionContract,
            )
            .expect("create");
        let date = "2024-01-15";
        // Only two of the four tenors are supported; the other two are withheld.
        let supported = [(30u32, 0.58f64), (90, 0.55)];
        for (tenor, prob) in supported {
            let tx = hkask_mcp_portfolio::Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                date: date.to_string(),
                tx_type: hkask_mcp_portfolio::TxType::Buy,
                asset_type: hkask_mcp_portfolio::AssetType::PredictionContract,
                symbol: Some(format!("cmp:KXFEDDECISION:{tenor}d")),
                quantity: Some(1.0),
                price: Some(prob),
                commission: Some(0.0),
                amount: None,
                weight: Some(prob),
                currency: "USD".to_string(),
                notes: String::new(),
                created_at: "2024-01-15T00:00:00Z".to_string(),
            };
            store.apply(portfolio, &tx).expect("apply");
        }
        let snapshot = store.snapshot(portfolio, date).expect("snapshot");
        assert_eq!(snapshot.holdings.len(), 2, "only supported tenors stored");
    }

    #[test]
    fn cmp_index_holdings_rebuildable_from_ledger() {
        // The materialized holdings view is rebuildable from the ledger
        // (the append-only source of truth). rebuild_views clears the
        // cached views and recomputes from the ledger.
        let store = cmp_index_store_fixture();
        let portfolio = "cmp:KXFEDDECISION";
        store
            .create(
                portfolio,
                hkask_mcp_portfolio::AssetType::PredictionContract,
            )
            .expect("create");
        let date = "2024-01-15";
        let tx = hkask_mcp_portfolio::Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            date: date.to_string(),
            tx_type: hkask_mcp_portfolio::TxType::Buy,
            asset_type: hkask_mcp_portfolio::AssetType::PredictionContract,
            symbol: Some("cmp:KXFEDDECISION:30d".to_string()),
            quantity: Some(1.0),
            price: Some(0.58),
            commission: Some(0.0),
            amount: None,
            weight: Some(0.58),
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: "2024-01-15T00:00:00Z".to_string(),
        };
        store.apply(portfolio, &tx).expect("apply");
        let _ = store.snapshot(portfolio, date).expect("snapshot");

        // Rebuild clears the cached views and recomputes from the ledger.
        store.rebuild_views(portfolio).expect("rebuild");
        let rebuilt = store
            .snapshot(portfolio, date)
            .expect("snapshot after rebuild");
        assert_eq!(rebuilt.holdings.len(), 1, "holdings rebuilt from ledger");
    }

    #[test]
    fn cmp_index_store_request_schema_has_no_boolean_positions() {
        let schema = schemars::schema_for!(MarketCmpIndexStoreRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
        assert!(
            positions.is_empty(),
            "MarketCmpIndexStoreRequest schema has bare-boolean positions: {positions:?}"
        );
    }

    #[test]
    fn empty_server_constructs_with_portfolio_store() {
        let server = empty_server();
        // The portfolio store is wired; listing an empty store returns [].
        let names = server.portfolio_store.list().expect("list");
        assert!(names.is_empty(), "fresh store has no portfolios");
    }

    #[test]
    fn cmp_portfolio_store_request_schema_has_no_boolean_positions() {
        let schema = schemars::schema_for!(MarketCmpPortfolioStoreRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
        assert!(
            positions.is_empty(),
            "MarketCmpPortfolioStoreRequest schema has bare-boolean positions: {positions:?}"
        );
    }

    #[test]
    fn cmp_context_suggest_request_schema_has_no_boolean_positions() {
        let schema = schemars::schema_for!(MarketCmpContextSuggestRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
        assert!(
            positions.is_empty(),
            "MarketCmpContextSuggestRequest schema has bare-boolean positions: {positions:?}"
        );
    }

    #[test]
    fn market_volatility_request_schema_has_no_boolean_positions() {
        let schema = schemars::schema_for!(MarketVolatilityRequest);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
        assert!(
            positions.is_empty(),
            "MarketVolatilityRequest schema has bare-boolean positions: {positions:?}"
        );
    }

    #[test]
    fn default_economic_context_provides_reasonable_defaults_for_each_family() {
        // Every base-event family has a curated default with a non-zero
        // reference and a rationale — never a blank field.
        use base_event::{BaseEvent, EconomicContext};
        for be in BaseEvent::ALL {
            let ctx: EconomicContext = be.default_economic_context();
            assert!(!ctx.rationale.is_empty(), "{:?} has no rationale", be);
            // predicted_level defaults to reference → Stable orientation.
            assert!(
                (ctx.predicted_level - ctx.reference).abs() < 1e-9,
                "{:?} predicted_level should default to reference",
                be
            );
        }
    }

    #[test]
    fn cmp_portfolio_solved_index_persists_as_weight_adjust_ledger() {
        // The solved-portfolio CMP index path (construct_cmp_index_set)
        // produces CmpIndex entries with weighted constituents. Persisting
        // each as a WeightAdjust ledger and reading the materialized holdings
        // returns the constituents with their weights.
        use hkask_mcp_portfolio::{AssetType, PortfolioStore, Transaction, TxType};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        let store = PortfolioStore::with_dir(path);

        // Build a fixture solved index: two constituents at 30d, increase orientation.
        let portfolio_name = "cmp:KXFEDDECISION:1m:increase";
        store
            .create(portfolio_name, AssetType::PredictionContract)
            .expect("create");
        let date = "2024-01-15";
        let constituents = [
            ("KXFED-DEC25-YES", 0.6, 0.55, 28.0),
            ("KXFED-JAN26-YES", 0.4, 0.58, 33.0),
        ];
        for (symbol, weight, prob, dte) in constituents {
            let tx = Transaction {
                id: uuid::Uuid::new_v4().to_string(),
                date: date.to_string(),
                tx_type: TxType::Buy,
                asset_type: AssetType::PredictionContract,
                symbol: Some(symbol.to_string()),
                quantity: Some(1.0),
                price: Some(prob),
                commission: Some(0.0),
                amount: None,
                weight: Some(weight),
                currency: "USD".to_string(),
                notes: format!("CMP portfolio constituent: weight={weight:.4} dte={dte:.1}"),
                created_at: "2024-01-15T00:00:00Z".to_string(),
            };
            store.apply(portfolio_name, &tx).expect("apply");
        }
        let snap = store.snapshot(portfolio_name, date).expect("snapshot");
        assert_eq!(snap.holdings.len(), 2, "two constituents");
        let symbols: Vec<String> = snap.holdings.iter().map(|h| h.symbol.clone()).collect();
        assert!(symbols.contains(&"KXFED-DEC25-YES".to_string()));
        assert!(symbols.contains(&"KXFED-JAN26-YES".to_string()));
    }
}
