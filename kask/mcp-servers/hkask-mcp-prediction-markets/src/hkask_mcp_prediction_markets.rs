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
use hkask_mcp_server::server::{CredentialRequirement, McpToolError, execute_tool, map_join_error};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};

pub mod base_event;
pub mod cache;
pub mod calibration;
pub mod cmp;
pub mod cmp_index_builder;
pub mod cmp_portfolio;
pub mod economic_data;
mod economic_data_tools;
pub mod economic_object;
pub mod eqm;
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

mod requests;
pub use requests::{
    MarketCalibrationRequest, MarketCheckResolutionsRequest, MarketCmpContextSuggestRequest,
    MarketCmpIndexRequest, MarketCmpIndexStoreRequest, MarketCmpIndicesRequest,
    MarketCmpPortfolioStoreRequest, MarketHistoryRequest, MarketLadderRequest,
    MarketLookupRequest, MarketMatchRequest, MarketOntologyMapRequest,
    MarketRecordResolutionRequest, MarketResidualRequest, MarketSubscribeRequest,
    MarketVolatilityRequest, StatusRequest,
};

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
        /// Inference port for LLM-based EQM (Explanation Quality Marker)
        /// scoring of forecast rationales. Resolved once in `run()` before
        /// the sync server-construction closure.
        pub inference_port: std::sync::Arc<dyn hkask_types::InferencePort>,
    }
);

// ── Tool router ────────────────────────────────────────────────────────────

impl PredictionMarketsServer {
    fn combined_router() -> ToolRouter<Self> {
        // Merge the core tools with the economic-data/EQM tools extracted into
        // `economic_data_tools.rs` (its own `#[tool_router]` block). A missing
        // merge silently drops those 15 tools from the MCP tool list.
        Self::prediction_markets_router() + Self::economic_data_tools_router()
    }

    /// Record that a tool was called on this server instance. Used by the
    /// status tool to report which tools have been invoked this session.
    fn record_call(&self, tool: &str) {
        self.called_tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(tool.to_string());
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct AnnotatedMarketRecord {
    days_to_expiration: f64,
    probability: f64,
    quality: f64,
    orientation: cmp_portfolio::Orientation,
    market_index: usize,
    ticker: String,
}

#[tool_router(router = prediction_markets_router, vis = "pub")]
impl PredictionMarketsServer {
    /// Return the current server state snapshot.
    #[tool(
        description = "Return current prediction-markets server state: cache TTL, ontology mapping version, and tools called this session."
    )]
    async fn prediction_markets_status(
        &self,
        Parameters(_req): Parameters<StatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "prediction_markets_status", async {
            self.record_call("prediction_markets_status");
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
        })
        .await
    }

    /// Look up markets matching a query across both platforms.
    #[tool(
        description = "Look up prediction markets across Polymarket and Kalshi by free-text query. Returns annotated MarketRecords: every probability is paired with spread/volume/calibration/volatility/reliability_tier and a dual-axis (PKO + Dublin Core) ontology mapping. Never returns a bare probability."
    )]
    pub async fn market_lookup(
        &self,
        Parameters(req): Parameters<MarketLookupRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_lookup", async {
            self.record_call("market_lookup");
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
        })
        .await
    }

    /// Resolve a scenario/forecast question to candidate markets about the
    /// same underlying event.
    #[tool(
        description = "Resolve a scenario or forecasting question to candidate prediction markets about the same underlying event. Returns confidence-tiered candidates with deterministic match basis (token overlap + deadline alignment). Refuse low-confidence matches rather than anchoring on a wrong-event market."
    )]
    pub async fn market_match(
        &self,
        Parameters(req): Parameters<MarketMatchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_match", async {
            self.record_call("market_match");
            let records = self.gather_candidates().await?;
            let mut matches = matcher::rank_matches(&req.question, &records);
            matches.truncate(req.limit.unwrap_or(5).min(20) as usize);
            serde_json::to_value(&matches).map_err(|e| {
                McpToolError::internal(format!("match serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        })
        .await
    }

    /// Return the dual-axis ontology mapping document.
    #[tool(
        description = "Return the dual-axis (PKO process + Dublin Core state) ontology mapping document that annotates every MarketRecord, including the market lifecycle stages and field-level mappings. Fetch this before interpreting market records."
    )]
    pub async fn market_ontology_map(
        &self,
        Parameters(_req): Parameters<MarketOntologyMapRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_ontology_map", async {
            self.record_call("market_ontology_map");
            Ok(ontology::mapping_document())
        })
        .await
    }

    /// Return the calibration reading for a domain/series bucket.
    #[tool(
        description = "Return the calibration reading (Brier score, sample size, staleness) for a domain or series bucket, computed from resolved market observations via hkask-forecast. A bucket with no resolved data returns stale: true — never a synthetic brier of 0."
    )]
    pub async fn market_calibration(
        &self,
        Parameters(req): Parameters<MarketCalibrationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_calibration", async {
            self.record_call("market_calibration");
            let store = self
                .calibration_store
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let reading = calibration::read_calibration(&store, &req.bucket);
            serde_json::to_value(&reading).map_err(|e| {
                McpToolError::internal(format!("calibration serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        })
        .await
    }

    /// Record a resolved market outcome into the calibration store.
    #[tool(
        description = "Record a resolved market outcome (bucket, probability-at-observation, outcome) into the calibration store. This is the sense arm of the calibration feedback loop: accrued resolutions drive per-bucket Brier scores, which demote poorly-calibrated buckets' reliability tiers on subsequent lookups."
    )]
    pub async fn market_record_resolution(
        &self,
        Parameters(req): Parameters<MarketRecordResolutionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "market_record_resolution",
            async {
                self.record_call("market_record_resolution");
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
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "market_subscribe_resolutions",
            async {
                self.record_call("market_subscribe_resolutions");
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
        description = "Return the ladder of contracts in a series ordered by deadline, each annotated with its time_to_maturity in fractional years. Kalshi series ticker or Polymarket event slug; both platforms are probed. Unparsable deadlines sort last with null maturity; per-platform failures surface in warnings — the ladder never fabricates a maturity."
    )]
    pub async fn market_ladder(
        &self,
        Parameters(req): Parameters<MarketLadderRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_ladder", async {
            self.record_call("market_ladder");
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

            // Sort by maturity; unparsable deadlines (null) last.
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
        })
        .await
    }
    /// Residual risk decomposition: niche exposure to a base event.
    #[tool(
        description = "Decompose a niche market's movement into base-event exposure (beta in log-odds space) plus an idiosyncratic residual. Refuses with insufficient_overlap below 10 shared observations — never fabricates a residual from thin data. Output carries r_squared and observations so fit quality is explicit."
    )]
    pub async fn market_residual(
        &self,
        Parameters(req): Parameters<MarketResidualRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_residual", async {
            self.record_call("market_residual");
            let window = i64::from(req.window_days.unwrap_or(90));
            let now = chrono::Utc::now().timestamp();
            let start = (now - window * 86_400).max(0) as u64;
            let end = now as u64;
            let niche_history =
                provider_kalshi::fetch_price_history(&self.http, &req.market_ticker, start, end)
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
        })
        .await
    }

    /// Refuse unregistered series — base events come only from
    /// HKASK_PREDICTION_MARKETS_BASE_EVENTS; an unregistered series is never
    /// silently treated as one.
    fn require_registered_base_event(&self, series: &str) -> Result<(), McpToolError> {
        if self.base_events.iter().any(|(_, s)| s == series) {
            Ok(())
        } else {
            Err(McpToolError::invalid_argument(format!(
                "series '{}' is not a registered base event (HKASK_PREDICTION_MARKETS_BASE_EVENTS)",
                series
            )))
        }
    }

    /// Extract (days-to-resolution, yes-midpoint) tenor points from open
    /// Kalshi markets — shared by the CMP curve tools (previously copy-pasted
    /// per tool). Markets without a parseable midpoint or a future deadline
    /// are dropped; an empty result is the caller's not-found case.
    fn kalshi_tenor_points(
        markets: &[provider_kalshi::KalshiMarket],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<cmp::TenorPoint> {
        markets
            .iter()
            .filter_map(|m| {
                let mid = m.yes_midpoint()?;
                let deadline = chrono::DateTime::parse_from_rfc3339(&m.close_time).ok()?;
                let days =
                    (deadline.with_timezone(&chrono::Utc) - now).num_seconds() as f64 / 86_400.0;
                (days > 0.0).then_some(cmp::TenorPoint {
                    days_to_resolution: days,
                    price: mid,
                })
            })
            .collect()
    }

    fn scan_and_record_provider(
        &self,
        observations: Vec<(String, calibration::ResolvedObservation)>,
        recorded: &mut u32,
        already_known: &mut u32,
    ) -> Result<(), McpToolError> {
        for (bucket, observation) in observations {
            let mut store = self
                .calibration_store
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if store.contains(&bucket, &observation) {
                *already_known += 1;
            } else {
                store.record(&bucket, observation);
                *recorded += 1;
            }
        }
        Ok(())
    }

    /// Scan for resolved markets and record their outcomes.
    #[tool(
        description = "Scan Polymarket and Kalshi for open and newly resolved markets, feeding the calibration store in two phases: (1) every open market's current price is snapshotted as the pre-resolution probability-at-observation (the earliest snapshot per market is kept), and (2) newly resolved markets consume their snapshot — the Brier loop scores the price the scanner first saw, never the post-resolution price. A market that resolves before its first scan is counted in resolved_without_snapshot and skipped, never fabricated. Ambiguous 50-50 resolutions are skipped. Idempotent — re-scanning is safe. This is the self-feeding sense arm of the calibration loop; run it periodically so snapshots accumulate before resolutions."
    )]
    pub async fn market_check_resolutions(
        &self,
        Parameters(req): Parameters<MarketCheckResolutionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_check_resolutions", async {
            self.record_call("market_check_resolutions");
            let limit = req.limit.unwrap_or(100).min(500);
            let mut recorded = 0u32;
            let mut skipped_ambiguous = 0u32;
            let mut already_known = 0u32;
            let mut snapshotted = 0u32;
            let mut resolved_without_snapshot = 0u32;
            let mut warnings: Vec<String> = Vec::new();

            // Phase 1 — snapshot open markets: the honest probability-at-
            // observation. Pre-fix behavior scored the post-resolution price
            // (Kalshi last_price_dollars of a settled market; a resolved
            // Polymarket market's terminal price with the outcome derived
            // from that same price), which guaranteed Brier ≈ 0 for every
            // scan observation and made the reliability-tier demotion gate
            // unreachable from scan data.
            match provider_kalshi::fetch_markets_by_status(
                &self.http,
                req.series.as_deref(),
                "open",
                limit,
            )
            .await
            {
                Ok(markets) => {
                    let mut store = self
                        .calibration_store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    snapshotted += provider_kalshi::snapshot_open_markets(&markets, &mut store);
                }
                Err(e) => warnings.push(format!("kalshi open scan failed: {e}")),
            }

            match provider_polymarket::fetch_markets(&self.http, limit, false).await {
                Ok(markets) => {
                    let mut store = self
                        .calibration_store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    snapshotted += provider_polymarket::snapshot_open_markets(&markets, &mut store);
                }
                Err(e) => warnings.push(format!("polymarket open scan failed: {e}")),
            }

            // Phase 2 — consume snapshots for newly settled markets.
            match provider_kalshi::fetch_markets_by_status(
                &self.http,
                req.series.as_deref(),
                "settled",
                limit,
            )
            .await
            {
                Ok(markets) => {
                    let observations = {
                        let mut store = self
                            .calibration_store
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        provider_kalshi::resolved_observations_from_snapshots(
                            &markets,
                            &mut store,
                            &mut resolved_without_snapshot,
                        )
                    };
                    self.scan_and_record_provider(observations, &mut recorded, &mut already_known)?;
                }
                Err(e) => warnings.push(format!("kalshi scan failed: {e}")),
            }

            match provider_polymarket::fetch_markets(&self.http, limit, true).await {
                Ok(markets) => {
                    let observations = {
                        let mut store = self
                            .calibration_store
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        provider_polymarket::resolved_observations_from_snapshots(
                            &markets,
                            &mut store,
                            &mut skipped_ambiguous,
                            &mut resolved_without_snapshot,
                        )
                    };
                    self.scan_and_record_provider(observations, &mut recorded, &mut already_known)?;
                }
                Err(e) => warnings.push(format!("polymarket scan failed: {e}")),
            }

            if (recorded > 0 || snapshotted > 0)
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
                "snapshotted": snapshotted,
                "resolved_without_snapshot": resolved_without_snapshot,
                "skipped_ambiguous": skipped_ambiguous,
                "warnings": warnings,
            }))
            .map_err(|e| {
                McpToolError::internal(format!("scan serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        })
        .await
    }

    /// Market record enriched with realized variance from price history.
    #[tool(
        description = "Fetch a market's price history and return its record with realized_variance populated (log-odds step variance, 2607.08199-consistent) plus the volatility regime (smooth vs jump-like). Kalshi: candlesticks over the window; Polymarket: CLOB prices-history for the token."
    )]
    pub async fn market_history(
        &self,
        Parameters(req): Parameters<MarketHistoryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_history", async {
            self.record_call("market_history");
            let source = req.source.as_deref().unwrap_or("kalshi");
            let prices: Vec<f64> = match source {
                "kalshi" => {
                    let window = i64::from(req.window_days.unwrap_or(90));
                    let now = chrono::Utc::now().timestamp();
                    let start = (now - window * 86_400).max(0) as u64;
                    provider_kalshi::fetch_price_history(&self.http, &req.market, start, now as u64)
                        .await?
                        .iter()
                        .map(|p| p.price)
                        .collect()
                }
                "polymarket" => provider_polymarket::fetch_prices_history(&self.http, &req.market)
                    .await?
                    .iter()
                    .map(|p| p.price)
                    .collect(),
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
        })
        .await
    }

    /// The full CMP index curve for a registered base event.
    #[tool(
        description = "Compute the full Constant Maturity Prediction index for a registered base event: the curve of probabilities across the standard tenor grid (7d/30d/90d/180d/1y/2y), interpolated in log-odds space. Tenors without cohort coverage return null probability, never a fabricated extrapolation. Includes curve slope (log-odds/year) as the term-structure signal."
    )]
    pub async fn market_cmp_index(
        &self,
        Parameters(req): Parameters<MarketCmpIndexRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_cmp_index", async {
            self.record_call("market_cmp_index");
            self.require_registered_base_event(&req.series)?;
            let markets =
                provider_kalshi::fetch_markets(&self.http, Some(&req.series), 200).await?;
            let now = chrono::Utc::now();
            let points = Self::kalshi_tenor_points(&markets, now);
            if points.is_empty() {
                return Err(hkask_mcp_server::server::McpToolError::not_found(format!(
                    "no live markets with future deadlines for series '{}'",
                    req.series
                )));
            }
            let index = cmp::compute_index(&req.series, &points, &now.to_rfc3339());
            let slope_30_365 = cmp::curve_slope(&index, 30, 365);
            serde_json::to_value(serde_json::json!({
                "index": index,
                "slope_30d_1y_logodds_per_year": slope_30_365,
            }))
            .map_err(|e| {
                McpToolError::internal(format!("index serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "market_volatility",
            async {
                self.record_call("market_volatility");
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_cmp_index_store", async {
            self.record_call("market_cmp_index_store");
            self.require_registered_base_event(&series)?;
            let markets = provider_kalshi::fetch_markets(&self.http, Some(&series), 200).await?;
            let now = chrono::Utc::now();
            let points = Self::kalshi_tenor_points(&markets, now);
            if points.is_empty() {
                return Err(hkask_mcp_server::server::McpToolError::not_found(format!(
                    "no live markets with future deadlines for series '{}'",
                    series
                )));
            }
            let computed_at = date
                .clone()
                .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
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
                .map(|p| {
                    serde_json::json!({
                        "tenor_days": p.tenor_days,
                        "probability": p.probability,
                    })
                })
                .collect();
            let store = self.portfolio_store.clone();
            let created_at = now.to_rfc3339();
            let stored = tokio::task::spawn_blocking(move || {
                use hkask_mcp_portfolio::{AssetType, PortfolioError, Transaction, TxType};
                // Create the portfolio (idempotent) as a prediction-contract portfolio.
                // `create` uses INSERT OR IGNORE — already-exists is Ok, not an error.
                // Any error here (invalid name, DB failure) must propagate.
                store.create(&portfolio_name, AssetType::PredictionContract)?;
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
        })
        .await
    }

    fn resolve_economic_context(
        &self,
        markets: &[provider_kalshi::KalshiMarket],
        series: &str,
        reference: Option<f64>,
        volatility: Option<f64>,
        predicted_level: Option<f64>,
        direction_up: Option<bool>,
    ) -> base_event::EconomicContext {
        let default_ctx = markets
            .first()
            .and_then(|m| base_event::classify_base_event_text(&m.title, &m.subtitle, &series, ""))
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
        let predicted_level = predicted_level.unwrap_or_else(|| {
            if let Some(m) = markets.first()
                && let Some(be) =
                    base_event::classify_base_event_text(&m.title, &m.subtitle, &series, "")
                && let Some((strike, _)) = be.extract_strike(&m.title)
            {
                strike
            } else {
                default_ctx.predicted_level
            }
        });
        let direction_up = match direction_up {
            Some(up) => up,
            None => {
                if let Some(m) = markets.first()
                    && let Some(be) =
                        base_event::classify_base_event_text(&m.title, &m.subtitle, &series, "")
                    && let Some((_, up)) = be.extract_strike(&m.title)
                {
                    up
                } else {
                    default_ctx.direction_up
                }
            }
        };
        base_event::EconomicContext {
            reference,
            volatility,
            predicted_level,
            direction_up,
            rationale: default_ctx.rationale,
        }
    }

    fn build_annotated_market_records(
        markets: &[provider_kalshi::KalshiMarket],
        ctx: &base_event::EconomicContext,
        _observation_date: &str,
        series: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<serde_json::Value> {
        let config = cmp_portfolio::CmpConfig::default();
        let mut records: Vec<serde_json::Value> = Vec::new();
        for (idx, market) in markets.iter().enumerate() {
            let Some(probability) = market.yes_midpoint() else {
                continue;
            };
            let Some(deadline) = chrono::DateTime::parse_from_rfc3339(&market.close_time).ok()
            else {
                continue;
            };
            let days = (deadline.with_timezone(&chrono::Utc) - now).num_seconds() as f64 / 86_400.0;
            if days <= 0.0 {
                continue;
            }
            let Some(base_event) =
                base_event::classify_base_event_text(&market.title, &market.subtitle, &series, "")
            else {
                continue;
            };
            let setting = base_event.default_materiality();
            let level = cmp_portfolio::materiality_level(&setting, ctx.volatility, 30, &config);
            let orientation = match level {
                Some(level) => cmp_portfolio::classify_orientation(
                    ctx.predicted_level,
                    ctx.reference,
                    level,
                    ctx.direction_up,
                ),
                None => cmp_portfolio::Orientation::Stable,
            };
            records.push(serde_json::json!({
                "days_to_expiration": days,
                "probability": probability,
                "quality": 1.0,
                "orientation": orientation,
                "market_index": idx,
                "ticker": market.ticker.clone(),
            }));
        }
        records
    }

    async fn persist_cmp_portfolio(
        &self,
        records: Vec<serde_json::Value>,
        series: &str,
        observation_date: &str,
        ctx: &base_event::EconomicContext,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, McpToolError> {
        let mut oriented: Vec<cmp_portfolio::OrientedConstituent> = Vec::new();
        let mut market_ids: Vec<String> = Vec::new();
        for record in records {
            let r: AnnotatedMarketRecord = serde_json::from_value(record).map_err(|e| {
                McpToolError::invalid_argument(format!("record deserialization failed: {e}"))
            })?;
            oriented.push(cmp_portfolio::OrientedConstituent {
                constituent: cmp_portfolio::Constituent {
                    days_to_expiration: r.days_to_expiration,
                    probability: r.probability,
                    quality: r.quality,
                },
                orientation: r.orientation,
                market_index: r.market_index,
            });
            market_ids.push(r.ticker);
        }
        if oriented.is_empty() {
            return Err(McpToolError::not_found(format!(
                "no eligible markets for series '{}' with the supplied economic context",
                series
            )));
        }

        let config = cmp_portfolio::CmpConfig::default();
        let index_set = cmp_portfolio::construct_cmp_index_set(&oriented, &config);

        let store = self.portfolio_store.clone();
        let created_at = now.to_rfc3339();
        let series_owned = series.to_string();
        let observation_date_owned = observation_date.to_string();
        let stored = tokio::task::spawn_blocking(move || {
            use hkask_mcp_portfolio::{AssetType, PortfolioError, Transaction, TxType};
            let mut stored_indices: Vec<(String, usize)> = Vec::new();
            for index in &index_set.indices {
                let portfolio_name = format!(
                    "cmp:{series_owned}:{}:{}",
                    index.bucket.label(),
                    index.orientation
                );
                // `create` uses INSERT OR IGNORE — already-exists is Ok, not an error.
                // Any error here (invalid name, DB failure) must propagate.
                store.create(&portfolio_name, AssetType::PredictionContract)?;
                for constituent in &index.portfolio.constituents {
                    let symbol = market_ids
                        .get(constituent.market_index)
                        .cloned()
                        .unwrap_or_else(|| format!("market_{}", constituent.market_index));
                    let tx = Transaction {
                        id: uuid::Uuid::new_v4().to_string(),
                        date: observation_date_owned.clone(),
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
                let snap = store.snapshot(&portfolio_name, &observation_date_owned)?;
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
            "series": series,
            "observation_date": observation_date,
            "economic_context": {
                "reference": ctx.reference,
                "volatility": ctx.volatility,
                "predicted_level": ctx.predicted_level,
                "direction_up": ctx.direction_up,
                "rationale": ctx.rationale,
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_cmp_portfolio_store", async {
            self.record_call("market_cmp_portfolio_store");
            self.require_registered_base_event(&series)?;
            let markets = provider_kalshi::fetch_markets(&self.http, Some(&series), 200).await?;
            let now = chrono::Utc::now();
            let observation_date = date
                .clone()
                .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
            let ctx = self.resolve_economic_context(
                &markets,
                &series,
                reference,
                volatility,
                predicted_level,
                direction_up,
            );
            let records = Self::build_annotated_market_records(
                &markets,
                &ctx,
                &observation_date,
                &series,
                now,
            );
            self.persist_cmp_portfolio(records, &series, &observation_date, &ctx, now)
                .await
        })
        .await
    }

    /// Build provenance-carrying CMP indices for the scenarios seam.
    #[tool(
        description = "Build provenance-carrying CMP indices (ProvenancedCmpIndex objects) for a registered base-event series from live open markets on Kalshi and/or Polymarket. This is the producer for scenario_from_cmp_indices (hkask-mcp-scenarios): pass the returned indices array verbatim as its cmp_indices input to compose an EventTree for coherence testing. Per-venue indices are never pooled; buckets without an eligible maturity bracket are withheld and surfaced in withheld_buckets with rejection reasons — never fabricated. All economic-context fields are optional; when omitted, the curated default for the classified family applies (see market_cmp_context_suggest)."
    )]
    pub async fn market_cmp_indices(
        &self,
        Parameters(MarketCmpIndicesRequest {
            series,
            venue,
            limit,
            reference,
            volatility,
            predicted_level,
            direction_up,
        }): Parameters<MarketCmpIndicesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_cmp_indices", async {
            self.record_call("market_cmp_indices");
            self.require_registered_base_event(&series)?;
            // Resolve the family from the series (Kalshi series-prefix
            // classification). Refuse when unclassifiable — never fabricate
            // a family or a materiality setting.
            let family = semantic_mapping::classify_base_object_from_catalog("kalshi", &series, "")
                .ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "series '{}' does not resolve to a base-event family — cannot build CMP indices",
                        series
                    ))
                })?;
            // Curated default context for the family, with caller overrides.
            // RealGdpGrowth has no BaseEvent materiality setting; the builder
            // withholds all its buckets with an explicit reason.
            let default_ctx = cmp_index_builder::base_event_for(family)
                .map(|be| be.default_economic_context())
                .unwrap_or_else(|| base_event::EconomicContext {
                    reference: 0.0,
                    volatility: None,
                    predicted_level: 0.0,
                    direction_up: false,
                    rationale: "no BaseEvent materiality setting for this family — \
                                all buckets will be withheld"
                        .into(),
                });
            let context = base_event::EconomicContext {
                reference: reference.unwrap_or(default_ctx.reference),
                volatility: volatility.or(default_ctx.volatility),
                predicted_level: predicted_level.unwrap_or(default_ctx.predicted_level),
                direction_up: direction_up.unwrap_or(default_ctx.direction_up),
                rationale: default_ctx.rationale,
            };

            let venue_filter = venue.as_deref().unwrap_or("both");
            if !matches!(venue_filter, "kalshi" | "polymarket" | "both") {
                return Err(McpToolError::invalid_argument(format!(
                    "venue must be 'kalshi', 'polymarket', or 'both', got '{venue_filter}'"
                )));
            }
            let limit = limit.unwrap_or(200).min(500);
            let now = chrono::Utc::now();
            let config = cmp_portfolio::CmpConfig::default();
            let mut indices: Vec<serde_json::Value> = Vec::new();
            let mut venue_reports: Vec<serde_json::Value> = Vec::new();
            let mut warnings: Vec<String> = Vec::new();

            if matches!(venue_filter, "kalshi" | "both") {
                match provider_kalshi::fetch_markets(&self.http, Some(&series), limit).await {
                    Ok(markets) => {
                        let lines: Vec<String> = markets
                            .iter()
                            .filter_map(|m| {
                                let record = cmp_index_builder::KalshiCatalogRecord {
                                    source: "kalshi".into(),
                                    event_ticker: m.event_ticker.clone(),
                                    base_object: String::new(),
                                    market_ticker: m.ticker.clone(),
                                    title: m.title.clone(),
                                    status: m.status.clone(),
                                    close_time: m.close_time.clone(),
                                    expiration_time: m.expiration_time.clone(),
                                    yes_bid: m.yes_bid_dollars.clone(),
                                    yes_ask: m.yes_ask_dollars.clone(),
                                    volume_fp: m.volume_fp.clone(),
                                    liquidity_dollars: m.liquidity_dollars.clone(),
                                    result: m.result.clone(),
                                    rules_primary: m.rules_primary.clone(),
                                };
                                serde_json::to_string(&record).ok()
                            })
                            .collect();
                        match cmp_index_builder::build_cmp_indices_from_lines(
                            &lines,
                            family,
                            cmp_index_builder::Venue::Kalshi,
                            &context,
                            &config,
                            &now,
                        ) {
                            Ok(set) => {
                                let n = set.indices.len();
                                for index in &set.indices {
                                    match serde_json::to_value(index) {
                                        Ok(v) => indices.push(v),
                                        Err(e) => warnings.push(format!(
                                            "kalshi index serialization failed: {e}"
                                        )),
                                    }
                                }
                                venue_reports.push(serde_json::json!({
                                    "venue": "kalshi",
                                    "indices": n,
                                    "withheld_buckets": set.withheld_buckets,
                                    "n_records_read": set.n_records_read,
                                    "n_eligible": set.n_eligible,
                                    "rejection_sample": set.rejection_sample,
                                }));
                            }
                            Err(e) => warnings.push(format!("kalshi index construction: {e}")),
                        }
                    }
                    Err(e) => warnings.push(format!("kalshi fetch failed: {e}")),
                }
            }

            if matches!(venue_filter, "polymarket" | "both") {
                // Gamma has no series-scoped fetch; the per-record semantic
                // classification inside the builder rejects non-family
                // markets with surfaced reasons (never silently dropped).
                match provider_polymarket::fetch_markets(&self.http, limit, false).await {
                    Ok(markets) => {
                        let lines: Vec<String> = markets
                            .iter()
                            .filter_map(|m| {
                                let record = cmp_index_builder::GammaCatalogRecord {
                                    source: "gamma".into(),
                                    event_id: m.id.clone(),
                                    base_object: String::new(),
                                    market_id: m.id.clone(),
                                    question: m.question.clone(),
                                    condition_id: m.condition_id.clone(),
                                    end_date: m.end_date.clone(),
                                    closed: m.closed,
                                    volume_num: m.volume_num,
                                    best_bid: m.best_bid,
                                    best_ask: m.best_ask,
                                    last_trade_price: m.last_trade_price,
                                    spread: m.spread,
                                    uma_resolution_status: m.uma_resolution_status.clone(),
                                };
                                serde_json::to_string(&record).ok()
                            })
                            .collect();
                        match cmp_index_builder::build_cmp_indices_from_lines(
                            &lines,
                            family,
                            cmp_index_builder::Venue::Polymarket,
                            &context,
                            &config,
                            &now,
                        ) {
                            Ok(set) => {
                                let n = set.indices.len();
                                for index in &set.indices {
                                    match serde_json::to_value(index) {
                                        Ok(v) => indices.push(v),
                                        Err(e) => warnings.push(format!(
                                            "polymarket index serialization failed: {e}"
                                        )),
                                    }
                                }
                                venue_reports.push(serde_json::json!({
                                    "venue": "polymarket",
                                    "indices": n,
                                    "withheld_buckets": set.withheld_buckets,
                                    "n_records_read": set.n_records_read,
                                    "n_eligible": set.n_eligible,
                                    "rejection_sample": set.rejection_sample,
                                }));
                            }
                            Err(e) => warnings.push(format!("polymarket index construction: {e}")),
                        }
                    }
                    Err(e) => warnings.push(format!("polymarket fetch failed: {e}")),
                }
            }

            serde_json::to_value(serde_json::json!({
                "series": series,
                "family": family,
                "observation_date": now.format("%Y-%m-%d").to_string(),
                "indices": indices,
                "indices_count": venue_reports.iter()
                    .filter_map(|r| r.get("indices").and_then(|n| n.as_u64()))
                    .sum::<u64>(),
                "venues": venue_reports,
                "warnings": warnings,
                "note": "Pass indices to scenario_from_cmp_indices (hkask-mcp-scenarios) as cmp_indices to compose an EventTree; optionally persist curves with market_cmp_index_store.",
            }))
            .map_err(|e| {
                McpToolError::internal(format!("cmp indices serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "market_cmp_context_suggest",
            async {
                self.record_call("market_cmp_context_suggest");
                self.require_registered_base_event(&series)?;
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

// Fail fast before the 60s MCP `tools/call` cap kills and restarts the
// server: a hung upstream (Kalshi, Polymarket, FRED) surfaces as a request
// error inside the cap, not a server restart that loses in-flight work.
const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const HTTP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Construct the inference port before entering the sync server-
    // construction closure. `resolve_inference_port` is async (it constructs
    // a `LazyInferencePort` — the bridge connection itself is deferred to
    // each `generate()` call, which re-tries `InferenceIpcClient::from_env()`);
    // the closure passed to `run_server` is sync, so the await must happen
    // here. Used by the EQM scoring tool.
    let inference_port = hkask_inference::resolve_inference_port().await;

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

    // D28 — Standardized Artifact Storage. Calibration journal lives at
    // `{kask_data_dir}/mcp/prediction-markets/calibration.jsonl`. Override
    // via `HKASK_PREDICTION_MARKETS_DATA` (points at the data dir; the
    // journal lives at <dir>/calibration.jsonl). A load failure is never
    // silent — the loop must distinguish "no data" from "failed to read
    // data" (the unwrap_or(0) sense-input trap).
    let data_dir = std::env::var("HKASK_PREDICTION_MARKETS_DATA")
        .ok()
        .unwrap_or_else(|| {
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                hkask_types::agent_paths::MCP_DIR,
            ))
            .join("prediction-markets")
            .to_string_lossy()
            .to_string()
        });
    let calibration_path = format!("{data_dir}/calibration.jsonl");
    let store = match calibration::CalibrationStore::load(std::path::Path::new(&calibration_path)) {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!(
                "calibration journal at {calibration_path} failed to load ({e});                          starting with an empty store — calibration signals                          will read stale until new observations accrue"
            );
            calibration::CalibrationStore::new()
        }
    };

    let base_events = match std::env::var("HKASK_PREDICTION_MARKETS_BASE_EVENTS") {
        Ok(raw) => {
            let parsed = cmp::parse_base_events(&raw);
            let declared = raw.split(',').filter(|s| !s.trim().is_empty()).count();
            if parsed.len() < declared {
                tracing::warn!(
                    "HKASK_PREDICTION_MARKETS_BASE_EVENTS: {} of {} entries malformed \
                     (need domain:series pairs, comma-separated) — dropped",
                    declared - parsed.len(),
                    declared
                );
            }
            parsed
        }
        Err(_) => Vec::new(),
    };

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
            // zed-kask: HTTP client with explicit connect+request timeouts.
            // Without these, a stalled upstream (Kalshi, Polymarket, FRED)
            // hangs the MCP `tools/call` past the 60s client cap, triggering
            // a server restart + retry that never converges. A 20s request
            // timeout surfaces a stalled provider as an error instead of
            // dragging the whole tool down. `unwrap_or_else` fallback logs
            // the builder failure (per .rules: opt-in features that fail must
            // log the failure classification, not collapse silently).
            let http_client = reqwest::Client::builder()
                .connect_timeout(HTTP_CONNECT_TIMEOUT)
                .timeout(HTTP_REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "hkask-mcp-prediction-markets: HTTP client builder failed, falling back to no-timeout client: {e}"
                    );
                    reqwest::Client::new()
                });
            Ok(PredictionMarketsServer::new(
                ctx.webid,
                http_client,
                cache_ttl_secs,
                std::sync::Arc::new(std::sync::Mutex::new(store)),
                cache::TtlCache::new(cache_ttl_secs),
                Some(calibration_path.clone()),
                base_events.clone(),
                std::sync::Mutex::new(HashSet::new()),
                portfolio_store,
                fred_api_key,
                inference_port.clone(),
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_FRED_API_KEY",
            "FRED API key for live reference-level fetches (curated static defaults used when absent)",
        )],
    )
    .await
}

// ── Smoke tests ─────────────────────────────────────────────────────────────
//
// Inline (not `tests/`) because some internal types are `pub(crate)`. Verifies
// the server constructs with minimal/test backends and that the simplest tools
// return the `{"content": <value>}` MCP envelope with the expected payload.
// Mirrors the `hkask-mcp-corpus` smoke-test pattern.

#[cfg(test)]
mod smoke {
    use super::*;
    use hkask_types::WebID;
    use hkask_types::ports::{InferenceError, InferencePort, InferenceResult};
    use hkask_types::template::LLMParameters;
    use rmcp::handler::server::wrapper::Parameters;
    use std::collections::HashSet;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    /// No-op inference port for smoke tests — every call returns an error.
    /// Smoke tests only exercise tools that don't call inference, so this never
    /// runs; it exists solely to satisfy the `inference_port` field.
    struct NoopInferencePort;

    impl InferencePort for NoopInferencePort {
        fn generate(
            &self,
            _: &str,
            _: &LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "noop inference port — not configured for smoke tests".into(),
                ))
            })
        }
    }

    /// Construct a server with minimal/test backends: a no-op inference port, an
    /// empty calibration store, no FRED key, and a portfolio DB isolated in a
    /// unique temp subdir so parallel runs never collide and no real data dir is
    /// touched.
    fn make_server() -> PredictionMarketsServer {
        let cache_ttl_secs: u64 = DEFAULT_CACHE_TTL_SECS;
        let inference_port: Arc<dyn InferencePort> = Arc::new(NoopInferencePort);
        let portfolio_dir =
            std::env::temp_dir().join(format!("hkask-pm-smoke-{}", uuid::Uuid::new_v4()));
        let portfolio_store = hkask_mcp_portfolio::PortfolioStore::with_dir(portfolio_dir);
        PredictionMarketsServer::new(
            WebID::new(),
            reqwest::Client::new(),
            cache_ttl_secs,
            Arc::new(Mutex::new(calibration::CalibrationStore::new())),
            cache::TtlCache::new(cache_ttl_secs),
            None,
            Vec::new(),
            Mutex::new(HashSet::new()),
            portfolio_store,
            None,
            inference_port,
        )
    }

    /// Extract the MCP tool-result envelope: `{"content": <value>}`.
    /// Panics with the raw output on any shape violation so failures are actionable.
    fn unwrap_content(output: &str) -> serde_json::Value {
        let parsed: serde_json::Value = serde_json::from_str(output)
            .unwrap_or_else(|e| panic!("tool output must be valid JSON, got: {output} ({e})"));
        parsed
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("tool output must have 'content' key, got: {parsed}"))
    }

    /// Pin: the HTTP client built in `run()` carries explicit connect and
    /// request timeouts so a hung upstream (Kalshi, Polymarket, FRED) fails
    /// fast before the 60s MCP `tools/call` cap kills and restarts the
    /// server. reqwest exposes no client-config inspection, so this pins the
    /// named consts the construction reads — dropping the timeouts means
    /// removing or rename-breaking a const this test references. It does NOT
    /// verify the built client itself carries them (not observable), nor the
    /// fallback `Client::new()` path taken on builder failure.
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

    #[tokio::test]
    async fn prediction_markets_status_returns_valid_json() {
        let server = make_server();
        let output = server
            .prediction_markets_status(Parameters(StatusRequest {}))
            .await
            .expect("tool ok");
        let content = unwrap_content(&output);
        assert_eq!(
            content["server"], "hkask-mcp-prediction-markets",
            "status must identify the server, got: {content}"
        );
        assert_eq!(
            content["cache_ttl_secs"].as_u64(),
            Some(DEFAULT_CACHE_TTL_SECS),
            "status must echo the configured cache TTL, got: {content}"
        );
        assert!(
            content.get("ontology_mapping_version").is_some(),
            "status must report the ontology mapping version, got: {content}"
        );
        // `called_tools` starts empty; status records its own invocation first,
        // so it must appear in the reported set.
        let called = content["called_tools"]
            .as_array()
            .expect("called_tools must be an array");
        assert!(
            called
                .iter()
                .any(|tool| tool == "prediction_markets_status"),
            "status must record its own invocation, got: {content}"
        );
    }

    #[tokio::test]
    async fn market_volatility_returns_valid_json() {
        // Pure-computation tool: no network, no credentials. Mid-price inputs
        // satisfy the DR-AS validity guards (price in [0,1], τ > 0, horizon > 0)
        // so the forecast succeeds rather than returning `invalid_argument`.
        let server = make_server();
        let output = server
            .market_volatility(Parameters(MarketVolatilityRequest {
                price: 0.5,
                hours_to_resolution: 24.0,
                spread: Some(0.01),
                volume: 1000.0,
                horizon_hours: 1.0,
                k: None,
                activity_proxy: None,
            }))
            .await
            .expect("tool ok");
        let content = unwrap_content(&output);
        assert_eq!(
            content["model"], "DR-AS (Xi, Moallemi, Pai & Wang, arXiv:2607.08199)",
            "volatility must label its model, got: {content}"
        );
        assert!(
            content.get("conditional_volatility").is_some()
                && content.get("conditional_variance").is_some(),
            "volatility must report variance and volatility, got: {content}"
        );
        let interval = &content["interval_95"];
        assert!(
            interval.get("lower").is_some() && interval.get("upper").is_some(),
            "volatility must report a 95% interval, got: {content}"
        );
    }
}
