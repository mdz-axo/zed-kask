//! Request-struct cluster for the prediction-markets MCP server.
//!
//! Plain `#[derive(JsonSchema, Deserialize)]` request structs used as
//! `Parameters<T>` in `#[tool]` methods, plus the `default_one_hour` serde
//! default helper. Extracted from the crate root as a T4 deep-module step.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::volatility;

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

/// Request for market_cmp_indices: build provenance-carrying CMP indices
/// (ProvenancedCmpIndex objects) from live open markets — the producer for
/// scenario_from_cmp_indices (hkask-mcp-scenarios).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketCmpIndicesRequest {
    /// Base-event series ticker (must be registered).
    pub series: String,
    /// Venue filter: "kalshi", "polymarket", or "both" (default).
    pub venue: Option<String>,
    /// Max markets to fetch per provider (default 200, capped at 500).
    pub limit: Option<u32>,
    /// The current level of the underlying factor. When omitted, the curated
    /// default for the classified family applies.
    pub reference: Option<f64>,
    /// Trailing volatility of the underlying in the family's type units.
    /// When omitted, the curated default applies. None disables
    /// materiality-gated indices.
    pub volatility: Option<f64>,
    /// The predicted level (strike) contracts are structured around. When
    /// omitted, defaults to the reference → Stable orientation.
    pub predicted_level: Option<f64>,
    /// Whether the factor ends above (true) or below (false) the strike.
    pub direction_up: Option<bool>,
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
