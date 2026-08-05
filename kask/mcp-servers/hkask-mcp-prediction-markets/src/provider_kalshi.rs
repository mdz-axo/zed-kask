//! Kalshi Predictions REST provider (read-only public market data, no auth).
//!
//! Field shapes pinned against live responses in T0
//! (docs/reports/prediction-markets/00-api-shape-spike.md §2).
//! Conventions handled here: all numerics are fixed-point strings
//! (`*_dollars`, `*_fp`) — never bare f64 serde. The documented
//! forecast-percentile-history endpoint 404'd live at T0; candlesticks are
//! the history source instead.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use serde::Deserialize;

const KALSHI_BASE: &str = "https://external-api.kalshi.com/trade-api/v2";

/// Parse a fixed-point numeric string; absent/empty/invalid → None.
/// Kalshi serves all numerics as strings; a bare `f64` serde field would
/// fail to deserialize the whole response on the first market.
pub fn parse_fp(raw: &str) -> Option<f64> {
    if raw.is_empty() {
        return None;
    }
    raw.parse::<f64>().ok()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KalshiMarket {
    pub ticker: String,
    pub event_ticker: String,
    pub title: String,
    pub subtitle: String,
    pub status: String,
    pub yes_bid_dollars: String,
    pub yes_ask_dollars: String,
    pub no_bid_dollars: String,
    pub no_ask_dollars: String,
    pub last_price_dollars: String,
    pub volume_fp: String,
    pub volume_24h_fp: String,
    pub open_interest_fp: String,
    pub liquidity_dollars: String,
    pub close_time: String,
    pub expiration_time: String,
    pub result: String,
    pub rules_primary: String,
    pub updated_time: String,
}

impl KalshiMarket {
    /// Yes-leg probability from the two-sided quote midpoint.
    /// The market object carries both sides (the bids-only shape is specific
    /// to the `/orderbook` endpoint — T0 §4 narrowed R13 accordingly).
    pub fn yes_midpoint(&self) -> Option<f64> {
        match (parse_fp(&self.yes_bid_dollars), parse_fp(&self.yes_ask_dollars)) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            (bid, ask) => bid.or(ask),
        }
    }

    /// Quoted spread on the yes leg.
    pub fn spread(&self) -> Option<f64> {
        match (parse_fp(&self.yes_bid_dollars), parse_fp(&self.yes_ask_dollars)) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KalshiMarketsResponse {
    pub cursor: String,
    pub markets: Vec<KalshiMarket>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KalshiSettlementSource {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KalshiEvent {
    pub event_ticker: String,
    pub series_ticker: String,
    pub title: String,
    pub sub_title: String,
    pub category: String,
    pub mutually_exclusive: bool,
    pub settlement_sources: Vec<KalshiSettlementSource>,
    pub last_updated_ts: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KalshiEventsResponse {
    pub cursor: String,
    pub events: Vec<KalshiEvent>,
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, String)],
) -> Result<T, McpToolError> {
    let response = client
        .get(url)
        .query(query)
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Kalshi request failed: {e}")))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("Kalshi body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("Kalshi", status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|e| McpToolError::internal(format!("Kalshi parse failed: {e}")))
}

/// Fetch open markets, optionally scoped to a series (e.g. "KXFEDDECISION").
pub async fn fetch_markets(
    client: &reqwest::Client,
    series_ticker: Option<&str>,
    limit: u32,
) -> Result<Vec<KalshiMarket>, McpToolError> {
    let mut query = vec![
        ("limit", limit.to_string()),
        ("status", "open".to_string()),
    ];
    if let Some(series) = series_ticker {
        query.push(("series_ticker", series.to_string()));
    }
    let response: KalshiMarketsResponse =
        get_json(client, &format!("{KALSHI_BASE}/markets"), &query).await?;
    Ok(response.markets)
}

/// Fetch open events.
pub async fn fetch_events(
    client: &reqwest::Client,
    limit: u32,
) -> Result<Vec<KalshiEvent>, McpToolError> {
    let query = vec![
        ("limit", limit.to_string()),
        ("status", "open".to_string()),
    ];
    let response: KalshiEventsResponse =
        get_json(client, &format!("{KALSHI_BASE}/events"), &query).await?;
    Ok(response.events)
}
