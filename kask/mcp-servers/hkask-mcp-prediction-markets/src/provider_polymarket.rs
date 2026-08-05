//! Polymarket Gamma API provider (read-only, no auth).
//!
//! Field shapes pinned against live responses in T0
//! (docs/reports/prediction-markets/00-api-shape-spike.md §1).
//! Quirk handled here: Gamma embeds several collections as JSON-encoded
//! strings inside JSON (`outcomes`, `outcomePrices`, `clobTokenIds`) — the
//! parser double-decodes them.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use serde::Deserialize;

const GAMMA_BASE: &str = "https://gamma-api.polymarket.com";

/// A market as embedded in a Gamma event (raw provider-local shape).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GammaMarket {
    pub id: String,
    pub question: String,
    pub condition_id: String,
    pub slug: String,
    pub description: String,
    pub end_date: String,
    /// JSON-string array of outcome names, e.g. `"[\"Yes\", \"No\"]"`.
    pub outcomes: String,
    /// JSON-string array of decimal price strings aligned with `outcomes`.
    pub outcome_prices: String,
    /// JSON-string array of ERC1155 token IDs aligned with `outcomes`.
    pub clob_token_ids: String,
    pub active: bool,
    pub closed: bool,
    pub volume: String,
    pub volume_num: f64,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub last_trade_price: Option<f64>,
    pub spread: Option<f64>,
    pub uma_resolution_status: String,
    pub resolved_by: String,
    pub updated_at: String,
}

impl Default for GammaMarket {
    fn default() -> Self {
        Self {
            id: String::new(),
            question: String::new(),
            condition_id: String::new(),
            slug: String::new(),
            description: String::new(),
            end_date: String::new(),
            outcomes: String::new(),
            outcome_prices: String::new(),
            clob_token_ids: String::new(),
            active: false,
            closed: false,
            volume: String::new(),
            volume_num: 0.0,
            best_bid: None,
            best_ask: None,
            last_trade_price: None,
            spread: None,
            uma_resolution_status: String::new(),
            resolved_by: String::new(),
            updated_at: String::new(),
        }
    }
}

/// A Gamma event with its embedded markets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GammaEvent {
    pub id: String,
    pub ticker: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub start_date: String,
    pub end_date: String,
    pub active: bool,
    pub closed: bool,
    pub liquidity: f64,
    pub volume: f64,
    pub open_interest: f64,
    pub volume24hr: f64,
    pub updated_at: String,
    pub markets: Vec<GammaMarket>,
    pub tags: Vec<GammaTag>,
}

impl Default for GammaEvent {
    fn default() -> Self {
        Self {
            id: String::new(),
            ticker: String::new(),
            slug: String::new(),
            title: String::new(),
            description: String::new(),
            start_date: String::new(),
            end_date: String::new(),
            active: false,
            closed: false,
            liquidity: 0.0,
            volume: 0.0,
            open_interest: 0.0,
            volume24hr: 0.0,
            updated_at: String::new(),
            markets: Vec::new(),
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GammaTag {
    pub label: String,
    pub slug: String,
}

impl GammaMarket {
    /// Double-decode a JSON-string array field (Gamma's string-in-JSON quirk).
    fn decode_string_array(field: &str) -> Vec<String> {
        serde_json::from_str(field).unwrap_or_default()
    }

    /// Outcome names decoded from the embedded JSON string.
    pub fn outcome_names(&self) -> Vec<String> {
        Self::decode_string_array(&self.outcomes)
    }

    /// Outcome prices decoded and parsed; unparseable entries are dropped
    /// (index alignment with names is preserved for parseable prefixes).
    pub fn prices(&self) -> Vec<f64> {
        Self::decode_string_array(&self.outcome_prices)
            .iter()
            .filter_map(|p| p.parse::<f64>().ok())
            .collect()
    }

    /// CLOB token IDs decoded from the embedded JSON string.
    pub fn token_ids(&self) -> Vec<String> {
        Self::decode_string_array(&self.clob_token_ids)
    }

    /// Yes-leg implied probability: `outcomePrices[0]` by Gamma convention
    /// (first outcome is "Yes" for binary markets).
    pub fn yes_probability(&self) -> Option<f64> {
        self.prices().first().copied()
    }
}

/// Fetch markets directly (not via events). `closed=true` returns
/// resolved/closed markets — the resolution-check feed.
pub async fn fetch_markets(
    client: &reqwest::Client,
    limit: u32,
    closed: bool,
) -> Result<Vec<GammaMarket>, McpToolError> {
    let url = format!("{GAMMA_BASE}/markets");
    let response = client
        .get(&url)
        .query(&[
            ("limit", limit.to_string()),
            ("closed", closed.to_string()),
        ])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Gamma request failed: {e}")))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Gamma body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("Polymarket Gamma", status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|e| McpToolError::internal(format!("Gamma markets parse failed: {e}")))
}

/// Fetch active, open events from Gamma.
pub async fn fetch_events(client: &reqwest::Client, limit: u32) -> Result<Vec<GammaEvent>, McpToolError> {
    let url = format!("{GAMMA_BASE}/events");
    let response = client
        .get(&url)
        .query(&[
            ("limit", limit.to_string()),
            ("active", "true".to_string()),
            ("closed", "false".to_string()),
        ])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Gamma request failed: {e}")))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Gamma body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("Polymarket Gamma", status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|e| McpToolError::internal(format!("Gamma events parse failed: {e}")))
}
