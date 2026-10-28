#![forbid(unsafe_code)]
//! Prediction-markets data-service MCP server.
//!
//! Read-only annotated feed of market-implied probabilities from Polymarket
//! (Gamma/CLOB) and Kalshi (Predictions REST). Every probability is paired
//! with reliability covariates, calibration metadata, volatility annotation,
//! and a dual-axis ontology mapping (PKO process axis + Dublin Core state
//! axis) so forecasting consumers never receive a bare probability.
//! See `kask/docs/reference/mcp-servers/prediction-markets.md`.

use std::collections::HashSet;

use hkask_mcp_server::server::{CredentialRequirement, McpToolError, execute_tool};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};

pub mod cache;
pub mod calibration;
pub mod economic_data;
mod economic_data_tools;
pub mod economic_object;
pub mod eqm;
pub mod matcher;
pub mod ontology;
#[cfg(test)]
mod property_tests;
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
    MarketCalibrationRequest, MarketCheckResolutionsRequest, MarketHistoryRequest,
    MarketLadderRequest, MarketLookupRequest, MarketMatchRequest, MarketOntologyMapRequest,
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
        pub called_tools: std::sync::Mutex<HashSet<String>>,
        /// Optional FRED API key for the FRED economic-data tools
        /// (`fred_search_series` and siblings). Required for live FRED series
        /// fetches.
        pub fred_api_key: Option<String>,
        /// Inference port for LLM-based EQM (Explanation Quality Marker)
        /// scoring of forecast rationales. Resolved once in `run()` before
        /// the sync server-construction closure.
        pub inference_port: std::sync::Arc<dyn hkask_types::InferencePort>,
    } = crate::ontology::annotate;
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
        description = "Look up prediction markets across Polymarket and Kalshi by free-text query. Returns annotated MarketRecords: every probability is paired with spread/volume/calibration/volatility/reliability_tier and a dual-axis (PKO + Dublin Core) ontology mapping. Never returns a bare probability. The response carries records, gathered_count, and — whenever the result set is empty — a note distinguishing a provider state (no markets returned by either provider) from a no-match query result."
    )]
    pub async fn market_lookup(
        &self,
        Parameters(req): Parameters<MarketLookupRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_lookup", async {
            self.record_call("market_lookup");
            let mut records = self.gather_candidates().await?;
            let gathered_count = records.len();
            Self::substring_filter(&mut records, &req.query);
            if let Some(category) = &req.category {
                let cat = category.to_lowercase();
                records.retain(|r| r.category.to_lowercase().contains(&cat));
            }
            records.truncate(req.limit.unwrap_or(10).min(50) as usize);
            // PM-03 (mcp-tool-review): an empty result is never silent — a
            // provider state (nothing gathered) is distinguished from a
            // no-match query result, so a transient outage can't read as
            // "no markets exist".
            let note = if gathered_count == 0 {
                Some("no candidate markets returned by either provider — this is a provider state, not a query result; retry before treating it as 'no markets exist'".to_string())
            } else if records.is_empty() {
                Some(format!("no records matched the query among {gathered_count} gathered candidates"))
            } else {
                None
            };
            serde_json::to_value(serde_json::json!({
                "records": records,
                "gathered_count": gathered_count,
                "note": note,
            }))
            .map_err(|e| McpToolError::internal(format!("record serialization failed: {e}")))
        })
        .await
    }

    /// Resolve a scenario/forecast question to candidate markets about the
    /// same underlying event.
    #[tool(
        description = "Resolve a scenario or forecasting question to candidate prediction markets about the same underlying event. Refuses low-confidence candidates rather than anchoring on a wrong-event market — the refusal is surfaced, never silent: the response carries matches (Medium confidence and above, with deterministic match basis: token overlap + deadline alignment), refused_low_confidence with a refused_sample of what was rejected, gathered_count, and a note whenever the result set is empty (provider state vs all-refused)."
    )]
    pub async fn market_match(
        &self,
        Parameters(req): Parameters<MarketMatchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "market_match", async {
            self.record_call("market_match");
            let records = self.gather_candidates().await?;
            let ranked = matcher::rank_matches(&req.question, &records);
            // PM-02 (mcp-tool-review): the advertised refusal, implemented —
            // low-confidence candidates are refused, and the refusal is
            // surfaced (count + sample), never a silent drop.
            let (kept, refused) = matcher::refuse_low_confidence(ranked);
            let limit = req.limit.unwrap_or(5).min(20) as usize;
            let matches: Vec<_> = kept.into_iter().take(limit).collect();
            let refused_sample: Vec<String> = refused
                .iter()
                .take(3)
                .map(|c| c.market.question.clone())
                .collect();
            // PM-03: an empty result is never silent — a provider state is
            // distinguished from an all-refused query result.
            let note = if records.is_empty() {
                Some("no candidate markets returned by either provider — this is a provider state, not a query result; retry before treating it as 'no markets exist'".to_string())
            } else if matches.is_empty() {
                Some(format!(
                    "all {} gathered candidates refused as low-confidence — no wrong-event anchoring; refused_sample shows what was rejected",
                    records.len()
                ))
            } else {
                None
            };
            serde_json::to_value(serde_json::json!({
                "matches": matches,
                "refused_low_confidence": refused.len(),
                "refused_sample": refused_sample,
                "gathered_count": records.len(),
                "note": note,
            }))
            .map_err(|e| McpToolError::internal(format!("match serialization failed: {e}")))
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
                McpToolError::internal(format!("calibration serialization failed: {e}"))
            })
        })
        .await
    }

    /// Record a resolved market outcome into the calibration store.
    #[tool(
        description = "Record a resolved market outcome (bucket, probability-at-observation, outcome) into the calibration store. These caller-reported observations are descriptive history, not prospective source-authenticated forecasts or calibration certificates. Raw loss never changes liquidity/spread tiers."
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
                            // The manual tool carries no market id — the
                            // observation records without identity and is
                            // never deduplicated (the scanner's rescan
                            // guard only matches identity-bearing rows).
                            market_key: None,
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
                    McpToolError::internal(format!("reading serialization failed: {e}"))
                })
            },
        )
        .await
    }

    /// Subscribe to Polymarket resolution events (notifications only — the
    /// caller feeds the calibration store).
    #[tool(
        description = "Subscribe to Polymarket's public market channel for resolution events on the given CLOB asset IDs. Resolution events are logged as notifications — they do NOT write calibration observations (the wire carries no pre-resolution probability, and fabricating one would corrupt the Brier loop). Pair a notification with market_record_resolution (which takes the pre-resolution probability) to feed the loop. The subscription is doubly bounded: it returns after max_resolutions resolution events (default 1) or after 300 seconds, whichever comes first — the response's timed_out field names which bound ended the call, so a quiet channel never holds the call open."
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

                // PM-08 (mcp-tool-review): the count bound terminates the
                // call when events flow; the time bound terminates it when
                // they do not. Both bounds surface in the response
                // (resolutions_ingested, timed_out) — the call never hangs.
                const SUBSCRIBE_TIME_BOUND: std::time::Duration =
                    std::time::Duration::from_secs(300);

                let subscribe = streaming::subscribe_market(&req.asset_ids, {
                    let ingested = std::sync::Arc::clone(&ingested);
                    move |event| {
                        let ingested = std::sync::Arc::clone(&ingested);
                        async move {
                            let mut stop = false;
                            if let streaming::MarketEvent::MarketResolved {
                                winning_outcome,
                                ..
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
                                    "market resolved: outcome={} — call market_record_resolution with the pre-resolution probability to feed the calibration loop",
                                    if outcome { "yes" } else { "no" }
                                );
                                let count =
                                    ingested.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                                        + 1;
                                stop = count >= max;
                            }
                            stop
                        }
                    }
                });

                let timed_out =
                    match tokio::time::timeout(SUBSCRIBE_TIME_BOUND, subscribe).await {
                        Ok(result) => {
                            result?;
                            false
                        }
                        Err(_elapsed) => true,
                    };

                let count = ingested.load(std::sync::atomic::Ordering::SeqCst);
                Ok(serde_json::json!({
                    "resolutions_ingested": count,
                    "bucket": req.bucket,
                    "max_resolutions": max,
                    "timed_out": timed_out,
                }))
            },
        )
        .await
    }

    /// Read the calibration block for `bucket` — the per-market step the
    /// rung scanner and both lookup gatherers share. The store reading is
    /// snapshotted without holding the guard across an await (holding the
    /// MutexGuard across one makes the tool future non-Send).
    fn calibration_block_for(&self, bucket: &str) -> types::Calibration {
        let guard = self
            .calibration_store
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let reading = calibration::read_calibration(&guard, bucket);
        types::calibration_for(Some(&reading))
    }

    /// Build the Polymarket market record with its calibration block — the
    /// per-market step both the rung scanner and the lookup gatherer
    /// repeat. Same family-first bucket derivation as the snapshot writers:
    /// the calibration block a record carries must read the bucket the
    /// scanner writes (§7 followup root cause).
    fn polymarket_record_with_calibration(
        &self,
        market: &provider_polymarket::GammaMarket,
        event: &provider_polymarket::GammaEvent,
        event_tags: &[String],
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Option<types::MarketRecord> {
        let bucket = semantic_mapping::calibration_bucket_for_gamma(&market.question, &market.slug);
        let calibration_block = self.calibration_block_for(&bucket);
        types::MarketRecord::from_polymarket(
            market,
            &event.id,
            &event.slug,
            event.volume,
            event.liquidity,
            event_tags,
            calibration_block,
            now,
        )
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
                            types::calibration_for(None),
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
                        for market in &event.markets {
                            if let Some(record) = self.polymarket_record_with_calibration(
                                market,
                                event,
                                &event_tags,
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
            .map_err(|e| McpToolError::internal(format!("ladder serialization failed: {e}")))
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
            serde_json::to_value(&analysis)
                .map_err(|e| McpToolError::internal(format!("residual serialization failed: {e}")))
        })
        .await
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
        description = "Scan Polymarket and Kalshi for open and newly resolved markets, feeding the calibration store in two phases: (1) every open market's current price is snapshotted as the pre-resolution probability-at-observation (the earliest snapshot per market is kept), and (2) newly resolved markets consume their snapshot — the Brier loop scores the price the scanner first saw, never the post-resolution price. A market that resolves before its first scan is counted in resolved_without_snapshot and skipped, never fabricated. Ambiguous 50-50 resolutions are skipped. Idempotent — re-scanning is safe. Series semantics: the optional series filter scopes the KALSHI phases only (Kalshi series ticker, e.g. KXFEDDECISION); Polymarket Gamma has no series parameter and is scanned unscoped — the response's series_scope surfaces this. The response carries per-provider dispositions (markets seen, new-snapshot identities, already-snapshotted and skipped counts) and a zero_scan_reason whenever the scan recorded nothing new — a silent zero is a broken feedback loop. Run periodically so snapshots accumulate before resolutions."
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
            let mut resolved_without_snapshot = 0u32;
            let mut warnings: Vec<String> = Vec::new();

            // §7 followup: the series filter applies to the Kalshi phases
            // only — Gamma has no series parameter. The scope is surfaced in
            // the response, never assumed identical across providers.
            let series_scope = serde_json::json!({
                "series": req.series,
                "kalshi": if req.series.is_some() {
                    "series-scoped (Kalshi series_ticker)"
                } else {
                    "unscoped — scans open/settled markets up to the limit"
                },
                "polymarket": "unscoped — Gamma has no series filter; recent markets are scanned regardless of the series parameter",
            });

            let mut kalshi_open = calibration::ScanSnapshotOutcome::default();
            let mut kalshi_open_seen = 0u32;
            let mut kalshi_settled_seen = 0u32;
            let mut kalshi_resolved_with_snapshot = 0u32;
            let mut poly_open = calibration::ScanSnapshotOutcome::default();
            let mut poly_open_seen = 0u32;
            let mut poly_settled_seen = 0u32;
            let mut poly_resolved_with_snapshot = 0u32;

            // Phase 1 — snapshot open markets: the honest probability-at-
            // observation. Pre-fix behavior scored the post-resolution price
            // (Kalshi last_price_dollars of a settled market; a resolved
            // Polymarket market's terminal price with the outcome derived
            // from that same price), which guaranteed Brier ≈ 0 for every
            // scan observation. Earliest open-market snapshots preserve the forecast instead.
            match provider_kalshi::fetch_markets_by_status(
                &self.http,
                req.series.as_deref(),
                "open",
                limit,
            )
            .await
            {
                Ok(markets) => {
                    kalshi_open_seen = markets.len() as u32;
                    let mut store = self
                        .calibration_store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    kalshi_open = provider_kalshi::snapshot_open_markets(&markets, &mut store);
                }
                Err(e) => warnings.push(format!("kalshi open scan failed: {e}")),
            }

            match provider_polymarket::fetch_markets(&self.http, limit, false).await {
                Ok(markets) => {
                    poly_open_seen = markets.len() as u32;
                    let mut store = self
                        .calibration_store
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    poly_open = provider_polymarket::snapshot_open_markets(&markets, &mut store);
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
                    kalshi_settled_seen = markets.len() as u32;
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
                    kalshi_resolved_with_snapshot = observations.len() as u32;
                    self.scan_and_record_provider(observations, &mut recorded, &mut already_known)?;
                }
                Err(e) => warnings.push(format!("kalshi scan failed: {e}")),
            }

            match provider_polymarket::fetch_markets(&self.http, limit, true).await {
                Ok(markets) => {
                    poly_settled_seen = markets.len() as u32;
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
                    poly_resolved_with_snapshot = observations.len() as u32;
                    self.scan_and_record_provider(observations, &mut recorded, &mut already_known)?;
                }
                Err(e) => warnings.push(format!("polymarket scan failed: {e}")),
            }

            let snapshotted = (kalshi_open.new.len() + poly_open.new.len()) as u32;
            let mut journal_saved = false;
            if (recorded > 0 || snapshotted > 0)
                && let Some(path) = &self.calibration_path
            {
                let store = self
                    .calibration_store
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                match store.save(std::path::Path::new(path)) {
                    Ok(()) => journal_saved = true,
                    Err(e) => warnings.push(format!("journal save failed: {e}")),
                }
            }

            // §7 followup: a scan that records nothing new must say why —
            // the disposition counts attribute the zero, never leave it
            // silent.
            let zero_scan_reason = if recorded == 0
                && snapshotted == 0
                && warnings.is_empty()
            {
                Some(format!(
                    "no new state: kalshi open {kalshi_open_seen} seen ({} new, {} already snapshotted, {} no price), \
                     kalshi settled {kalshi_settled_seen} seen ({kalshi_resolved_with_snapshot} with snapshot, {resolved_without_snapshot} without); \
                     polymarket open {poly_open_seen} seen ({} new, {} already snapshotted, {} not open, {} no price), \
                     polymarket settled {poly_settled_seen} seen ({poly_resolved_with_snapshot} with snapshot, {skipped_ambiguous} ambiguous)",
                    kalshi_open.new.len(),
                    kalshi_open.already_snapshotted,
                    kalshi_open.skipped_no_price,
                    poly_open.new.len(),
                    poly_open.already_snapshotted,
                    poly_open.skipped_not_open,
                    poly_open.skipped_no_price,
                ))
            } else {
                None
            };

            serde_json::to_value(serde_json::json!({
                "recorded": recorded,
                "already_known": already_known,
                "snapshotted": snapshotted,
                "resolved_without_snapshot": resolved_without_snapshot,
                "skipped_ambiguous": skipped_ambiguous,
                "series_scope": series_scope,
                "kalshi": {
                    "open_seen": kalshi_open_seen,
                    "new_snapshots": kalshi_open.new.len(),
                    "already_snapshotted": kalshi_open.already_snapshotted,
                    "skipped_no_price": kalshi_open.skipped_no_price,
                    "settled_seen": kalshi_settled_seen,
                    "resolved_with_snapshot": kalshi_resolved_with_snapshot,
                    "snapshot_markets": kalshi_open.new,
                },
                "polymarket": {
                    "open_seen": poly_open_seen,
                    "new_snapshots": poly_open.new.len(),
                    "already_snapshotted": poly_open.already_snapshotted,
                    "skipped_not_open": poly_open.skipped_not_open,
                    "skipped_no_price": poly_open.skipped_no_price,
                    "settled_seen": poly_settled_seen,
                    "resolved_with_snapshot": poly_resolved_with_snapshot,
                    "snapshot_markets": poly_open.new,
                },
                "zero_scan_reason": zero_scan_reason,
                "journal_saved": journal_saved,
                "warnings": warnings,
            }))
            .map_err(|e| {
                McpToolError::internal(format!("scan serialization failed: {e}"))
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
            .map_err(|e| McpToolError::internal(format!("history serialization failed: {e}")))
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
                    McpToolError::internal(format!("volatility serialization failed: {e}"))
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

        let gamma_events = provider_polymarket::fetch_events(&self.http, 100).await?;
        for event in &gamma_events {
            let event_tags: Vec<String> = event.tags.iter().map(|t| t.label.clone()).collect();
            for market in &event.markets {
                if let Some(record) =
                    self.polymarket_record_with_calibration(market, event, &event_tags, &now)
                {
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
            // Same family-first bucket derivation as the snapshot writers —
            // the calibration block a lookup record carries must read the
            // bucket the scanner writes, or the descriptive history is fragmented (§7 followup root cause).
            let bucket = semantic_mapping::calibration_bucket_for_kalshi(
                &market.event_ticker,
                &market.title,
            );
            let calibration_block = self.calibration_block_for(&bucket);
            if let Some(record) =
                types::MarketRecord::from_kalshi(market, event, calibration_block, &now)
            {
                records.push(record);
            }
        }
        // PM-03 (mcp-tool-review): never cache an empty pool — a transient
        // double-provider-empty would poison every lookup/match call for the
        // TTL as a silent []. An empty gather re-fetches on the next call
        // instead of serving a cached nothing.
        if !records.is_empty() {
            if let Ok(value) = serde_json::to_value(&records) {
                self.response_cache.put(cache_key, value);
            }
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

    hkask_mcp_server::run_server(
        "hkask-mcp-prediction-markets",
        SERVER_VERSION,
        |ctx| {
            let fred_api_key = ctx.credentials.get("HKASK_FRED_API_KEY").cloned();
            // HTTP client with explicit connect+request timeouts.
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
                std::sync::Mutex::new(HashSet::new()),
                fred_api_key,
                inference_port.clone(),
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_FRED_API_KEY",
            "FRED API key for the FRED economic-data tools",
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
    use crate::economic_data::fred::FredGetSeriesInfoRequest;
    use hkask_types::McpErrorKind;
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

    /// Construct a server with minimal/test backends: a no-op inference port,
    /// an empty calibration store, and no FRED key — no real data dir is
    /// touched.
    fn make_server() -> PredictionMarketsServer {
        let cache_ttl_secs: u64 = DEFAULT_CACHE_TTL_SECS;
        let inference_port: Arc<dyn InferencePort> = Arc::new(NoopInferencePort);
        PredictionMarketsServer::new(
            WebID::new(),
            reqwest::Client::new(),
            cache_ttl_secs,
            Arc::new(Mutex::new(calibration::CalibrationStore::new())),
            cache::TtlCache::new(cache_ttl_secs),
            None,
            Mutex::new(HashSet::new()),
            None,
            inference_port,
        )
    }

    /// Extract the MCP tool-result envelope `{"content": <value>}` via
    /// the canonical parser; a tool output that fails to parse is a test
    /// failure.
    fn unwrap_content(output: &str) -> serde_json::Value {
        hkask_types::tool_response::parse_tool_response(output)
            .expect("tool output must be valid JSON")
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

    /// Pin: the FRED tools read their key from ctx credentials
    /// (`HKASK_FRED_API_KEY`); with no key configured the tool surfaces
    /// `PermissionDenied` naming the env var (`economic_data.rs` maps
    /// `MissingApiKey`) — a missing credential is an authorization
    /// failure, never a silent fallback and never an `unavailable`
    /// transport error (.rules missing-credential pattern).
    #[tokio::test]
    async fn fred_tool_without_key_is_permission_denied_naming_the_env_var() {
        let server = make_server(); // no FRED key by construction
        let error = server
            .fred_get_series_info(Parameters(FredGetSeriesInfoRequest {
                series_id: "CPIAUCSL".to_string(),
            }))
            .await
            .expect_err("missing credential must error");
        assert_eq!(
            error.kind,
            McpErrorKind::PermissionDenied,
            "missing FRED key must be permission-denied, got: {:?}",
            error.kind
        );
        assert!(
            error.message.contains("HKASK_FRED_API_KEY"),
            "error must name HKASK_FRED_API_KEY, got: {}",
            error.message
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

    /// The manual calibration sense arm through the tool seam:
    /// `market_record_resolution` writes an observation and
    /// `market_calibration` reads it back non-stale with the right sample
    /// size; raw loss never changes liquidity/spread tiers. A bucket
    /// with no observations must read stale, never a synthetic Brier 0.
    #[tokio::test]
    async fn record_resolution_feeds_calibration_read() {
        use crate::requests::{MarketCalibrationRequest, MarketRecordResolutionRequest};

        let server = make_server();

        // An unobserved bucket reads stale — the honest "no signal".
        let stale = server
            .market_calibration(Parameters(MarketCalibrationRequest {
                bucket: "politics".to_string(),
            }))
            .await
            .expect("calibration ok");
        let stale_content = unwrap_content(&stale);
        assert_eq!(
            stale_content["stale"], true,
            "an empty bucket must read stale, got: {stale_content}"
        );
        assert!(
            stale_content["brier"].is_null(),
            "an empty bucket must carry brier: null, got: {stale_content}"
        );

        // Record two observations: a confident correct call and a coin flip.
        for (probability, outcome) in [(0.9, true), (0.5, false)] {
            server
                .market_record_resolution(Parameters(MarketRecordResolutionRequest {
                    bucket: "politics".to_string(),
                    probability,
                    outcome,
                }))
                .await
                .expect("record ok");
        }

        let reading = server
            .market_calibration(Parameters(MarketCalibrationRequest {
                bucket: "politics".to_string(),
            }))
            .await
            .expect("calibration ok");
        let content = unwrap_content(&reading);
        assert_eq!(
            content["stale"], false,
            "two recorded observations must read non-stale, got: {content}"
        );
        assert_eq!(
            content["sample_size"].as_u64(),
            Some(2),
            "the sample size must count both observations, got: {content}"
        );
        let brier = content["brier"]
            .as_f64()
            .expect("a non-stale bucket must carry a measured Brier");
        // Brier per observation: (0.9-1)^2 = 0.01 and (0.5-0)^2 = 0.25 → mean 0.13.
        assert!(
            (brier - 0.13).abs() < 1e-9,
            "0.9→true and 0.5→false must average Brier 0.13, got {brier}"
        );
    }
}

// Pins the registered tool-surface count end-to-end: `combined_router()`
// merges the core tools with the `economic_data_tools.rs` sub-router — a
// missing merge arm silently drops those tools from the MCP tool list (the
// seam the merge comment at the router documents). Adding or removing a
// prediction-markets tool is an intentional surface change; this pin
// catches accidental drift instead of shipping as an undocumented surface
// change. Mirrors the shared `tool_surface_pin!` macro every server carries.
#[cfg(test)]
mod tool_surface_tests {
    use super::PredictionMarketsServer;

    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
    hkask_mcp_server::tool_surface_pin!(
        PredictionMarketsServer::combined_router(),
        "combined_router",
        27,
        tool_surface_is_exactly_27_registered_tools,
    );
}
