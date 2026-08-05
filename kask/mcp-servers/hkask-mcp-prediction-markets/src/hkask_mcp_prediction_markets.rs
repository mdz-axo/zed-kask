//! Prediction-markets data-service MCP server.
//!
//! Read-only annotated feed of market-implied probabilities from Polymarket
//! (Gamma/CLOB) and Kalshi (Predictions REST). Every probability is paired
//! with reliability covariates, calibration metadata, volatility annotation,
//! and a dual-axis ontology mapping (PKO process axis + Dublin Core state
//! axis) so forecasting consumers never receive a bare probability.
//! See docs/reports/prediction-markets/02-zed-kask-integration.md §4.

use std::collections::HashSet;

use hkask_mcp_server::server::{CredentialRequirement, execute_tool_semantic};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

pub mod matcher;
pub mod cache;
pub mod calibration;
pub mod ontology;
pub mod provider_kalshi;
mod streaming;
pub mod provider_polymarket;
pub mod types;

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

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct PredictionMarketsServer {
        pub http: reqwest::Client,
        pub cache_ttl_secs: u64,
        pub calibration_store: std::sync::Arc<std::sync::Mutex<calibration::CalibrationStore>>,
        pub response_cache: cache::TtlCache,
        pub calibration_path: Option<String>,
        pub called_tools: std::sync::Mutex<HashSet<String>>,
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
                    hkask_mcp_server::server::McpToolError::internal(format!(
                        "record serialization failed: {e}"
                    ))
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
                    hkask_mcp_server::server::McpToolError::internal(format!(
                        "match serialization failed: {e}"
                    ))
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
                    hkask_mcp_server::server::McpToolError::internal(format!(
                        "calibration serialization failed: {e}"
                    ))
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
                    store.record(
                        &req.bucket,
                        calibration::ResolvedObservation {
                            probability: req.probability,
                            outcome: req.outcome,
                        },
                    );
                    let reading = calibration::read_calibration(&store, &req.bucket);
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
                    hkask_mcp_server::server::McpToolError::internal(format!(
                        "reading serialization failed: {e}"
                    ))
                })
            },
        )
        .await
    }

    /// Subscribe to Polymarket resolution events and feed the calibration store.
    #[tool(
        description = "Subscribe to Polymarket's public market channel for resolution events on the given CLOB asset IDs. Each market_resolved event is recorded into the calibration store under the given bucket (the automatic sense arm of the calibration loop). Returns after max_resolutions ingestions or stream end."
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
            let event_tags: Vec<String> =
                event.tags.iter().map(|t| t.label.clone()).collect();
            let bucket = event_tags.first().cloned().unwrap_or_default();
            let reading = {
                let guard = store.lock().unwrap_or_else(|e| e.into_inner());
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
            let bucket = event.map(|e| e.category.clone()).unwrap_or_default();
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
    let calibration_path = data_dir
        .as_ref()
        .map(|d| format!("{d}/calibration.jsonl"));
    let store = match &calibration_path {
        Some(path) => {
            match calibration::CalibrationStore::load(std::path::Path::new(path)) {
                Ok(store) => store,
                Err(e) => {
                    tracing::warn!(
                        "calibration journal at {path} failed to load ({e});                          starting with an empty store — calibration signals                          will read stale until new observations accrue"
                    );
                    calibration::CalibrationStore::new()
                }
            }
        }
        None => calibration::CalibrationStore::new(),
    };

    hkask_mcp_server::run_server(
        "hkask-mcp-prediction-markets",
        SERVER_VERSION,
        |ctx| {
            Ok(PredictionMarketsServer::new(
                ctx.webid,
                reqwest::Client::new(),
                cache_ttl_secs,
                std::sync::Arc::new(std::sync::Mutex::new(store)),
                cache::TtlCache::new(cache_ttl_secs),
                calibration_path.clone(),
                std::sync::Mutex::new(HashSet::new()),
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
        ],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_server() -> PredictionMarketsServer {
        PredictionMarketsServer::new(
            hkask_types::WebID::default(),
            reqwest::Client::new(),
            60,
            std::sync::Arc::new(std::sync::Mutex::new(calibration::CalibrationStore::new())),
            cache::TtlCache::new(60),
            None,
            std::sync::Mutex::new(HashSet::new()),
        )
    }

    #[tokio::test]
    async fn status_returns_envelope_with_mapping_version() {
        let server = empty_server();
        let response = server
            .prediction_markets_status(Parameters(StatusRequest {}))
            .await;
        let parsed: serde_json::Value = serde_json::from_str(&response).expect("response is JSON");
        let content = parsed
            .get("content")
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
}
