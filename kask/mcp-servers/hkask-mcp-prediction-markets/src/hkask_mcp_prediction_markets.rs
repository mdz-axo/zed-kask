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
pub mod ontology;
pub mod provider_kalshi;
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
                let mut records = self.gather_candidates(&req.query).await?;
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
                let records = self.gather_candidates(&req.question).await?;
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
}

impl PredictionMarketsServer {
    /// Fetch and annotate candidate markets from both platforms, prefiltered
    /// by substring match on the query. Shared by market_lookup and
    /// market_match.
    async fn gather_candidates(
        &self,
        query: &str,
    ) -> Result<Vec<types::MarketRecord>, hkask_mcp_server::server::McpToolError> {
        let now = chrono::Utc::now();
        let query_lower = query.to_lowercase();
        let mut records = Vec::new();

        let gamma_events = provider_polymarket::fetch_events(&self.http, 100).await?;
        for event in &gamma_events {
            let haystack = format!(
                "{} {} {}",
                event.title.to_lowercase(),
                event.slug.to_lowercase(),
                event.description.to_lowercase()
            );
            if !haystack.contains(&query_lower) {
                continue;
            }
            for market in &event.markets {
                if let Some(record) = types::MarketRecord::from_polymarket(
                    market,
                    &event.id,
                    &event.slug,
                    event.volume,
                    event.liquidity,
                    &now,
                ) {
                    records.push(record);
                }
            }
        }

        let kalshi_markets = provider_kalshi::fetch_markets(&self.http, None, 200).await?;
        let kalshi_events = provider_kalshi::fetch_events(&self.http, 200).await?;
        for market in &kalshi_markets {
            let haystack = format!(
                "{} {} {}",
                market.title.to_lowercase(),
                market.ticker.to_lowercase(),
                market.rules_primary.to_lowercase()
            );
            if !haystack.contains(&query_lower) {
                continue;
            }
            let event = kalshi_events
                .iter()
                .find(|e| e.event_ticker == market.event_ticker);
            if let Some(record) = types::MarketRecord::from_kalshi(market, event, &now) {
                records.push(record);
            }
        }
        Ok(records)
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

    hkask_mcp_server::run_server(
        "hkask-mcp-prediction-markets",
        SERVER_VERSION,
        |ctx| {
            Ok(PredictionMarketsServer::new(
                ctx.webid,
                reqwest::Client::new(),
                cache_ttl_secs,
                std::sync::Mutex::new(HashSet::new()),
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
            "Cache TTL in seconds for market-data responses (default 60)",
        )],
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
