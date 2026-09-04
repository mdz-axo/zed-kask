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
#[serde(default)]
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
        match (
            parse_fp(&self.yes_bid_dollars),
            parse_fp(&self.yes_ask_dollars),
        ) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            (bid, ask) => bid.or(ask),
        }
    }

    /// Quoted spread on the yes leg.
    pub fn spread(&self) -> Option<f64> {
        match (
            parse_fp(&self.yes_bid_dollars),
            parse_fp(&self.yes_ask_dollars),
        ) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

// ── Calibration scan decision cores (pure, HTTP-free) ─────────────────

/// Snapshot open markets into the calibration store — the honest
/// probability-at-observation for the future Brier score (last trade,
/// falling back to the quote midpoint for untraded markets). Returns the
/// count of markets snapshotted for the first time; re-scanning keeps the
/// EARLIEST snapshot per market.
pub(crate) fn snapshot_open_markets(
    markets: &[KalshiMarket],
    store: &mut crate::calibration::CalibrationStore,
) -> u32 {
    let mut snapshotted = 0;
    for market in markets {
        let probability = parse_fp(&market.last_price_dollars).or_else(|| market.yes_midpoint());
        let Some(probability) = probability else {
            continue;
        };
        let bucket = crate::types::canonical_bucket(&market.event_ticker);
        if store.record_pending(
            &market.ticker,
            crate::calibration::PendingSnapshot {
                bucket,
                probability,
            },
        ) {
            snapshotted += 1;
        }
    }
    snapshotted
}

/// Consume pre-resolution snapshots for settled markets. A settled market
/// with no snapshot is counted in `resolved_without_snapshot` and NEVER
/// recorded — scoring its post-resolution `last_price_dollars` would be
/// self-fulfilling (the outcome is derived from the same terminal price,
/// so Brier ≈ 0 by construction and the reliability-tier demotion gate
/// could never fire from scan data).
pub(crate) fn resolved_observations_from_snapshots(
    markets: &[KalshiMarket],
    store: &mut crate::calibration::CalibrationStore,
    resolved_without_snapshot: &mut u32,
) -> Vec<(String, crate::calibration::ResolvedObservation)> {
    let mut observations = Vec::new();
    for market in markets {
        let outcome = match market.result.as_str() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        };
        let Some(outcome) = outcome else { continue };
        match store.take_pending(&market.ticker) {
            Some(snapshot) => observations.push((
                snapshot.bucket,
                crate::calibration::ResolvedObservation {
                    probability: snapshot.probability,
                    outcome,
                },
            )),
            None => *resolved_without_snapshot += 1,
        }
    }
    observations
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
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
        .map_err(|e| McpToolError::unavailable(format!("Kalshi parse failed: {e}")))
}

/// Fetch markets by status, optionally scoped to a series.
pub async fn fetch_markets_by_status(
    client: &reqwest::Client,
    series_ticker: Option<&str>,
    status: &str,
    limit: u32,
) -> Result<Vec<KalshiMarket>, McpToolError> {
    let mut query = vec![("limit", limit.to_string()), ("status", status.to_string())];
    if let Some(series) = series_ticker {
        query.push(("series_ticker", series.to_string()));
    }
    let response: KalshiMarketsResponse =
        get_json(client, &format!("{KALSHI_BASE}/markets"), &query).await?;
    Ok(response.markets)
}

/// Fetch open markets, optionally scoped to a series (e.g. "KXFEDDECISION").
pub async fn fetch_markets(
    client: &reqwest::Client,
    series_ticker: Option<&str>,
    limit: u32,
) -> Result<Vec<KalshiMarket>, McpToolError> {
    let mut query = vec![("limit", limit.to_string()), ("status", "open".to_string())];
    if let Some(series) = series_ticker {
        query.push(("series_ticker", series.to_string()));
    }
    let response: KalshiMarketsResponse =
        get_json(client, &format!("{KALSHI_BASE}/markets"), &query).await?;
    Ok(response.markets)
}

/// One candlestick period (T0-verified shape: bid/ask OHLC as `_dollars`
/// strings; `price` sub-object is empty for quote-driven markets).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct KalshiCandlestick {
    pub end_period_ts: u64,
    pub yes_bid: KalshiOhlc,
    pub yes_ask: KalshiOhlc,
    pub volume_fp: String,
    pub open_interest_fp: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct KalshiOhlc {
    pub close_dollars: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct KalshiCandlesticksResponse {
    pub markets: Vec<KalshiCandlestickSeries>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct KalshiCandlestickSeries {
    pub candlesticks: Vec<KalshiCandlestick>,
}

/// One point on a market's price history (yes-midpoint at period close).
#[derive(Debug, Clone, Copy)]
pub struct PricePoint {
    pub ts: u64,
    pub price: f64,
}

/// Fetch daily candlesticks for a market and return the yes-midpoint series.
pub async fn fetch_price_history(
    client: &reqwest::Client,
    market_ticker: &str,
    start_ts: u64,
    end_ts: u64,
) -> Result<Vec<PricePoint>, McpToolError> {
    let query = vec![
        ("market_tickers", market_ticker.to_string()),
        ("start_ts", start_ts.to_string()),
        ("end_ts", end_ts.to_string()),
        ("period_interval", "1440".to_string()),
    ];
    let response: KalshiCandlesticksResponse = get_json(
        client,
        &format!("{KALSHI_BASE}/markets/candlesticks"),
        &query,
    )
    .await?;
    let mut points = Vec::new();
    for series in &response.markets {
        for candle in &series.candlesticks {
            let bid = parse_fp(&candle.yes_bid.close_dollars);
            let ask = parse_fp(&candle.yes_ask.close_dollars);
            let price = match (bid, ask) {
                (Some(b), Some(a)) => Some((b + a) / 2.0),
                (b, a) => b.or(a),
            };
            if let Some(price) = price {
                points.push(PricePoint {
                    ts: candle.end_period_ts,
                    price,
                });
            }
        }
    }
    points.sort_by_key(|p| p.ts);
    Ok(points)
}

/// Fetch open events.
pub async fn fetch_events(
    client: &reqwest::Client,
    limit: u32,
) -> Result<Vec<KalshiEvent>, McpToolError> {
    let query = vec![("limit", limit.to_string()), ("status", "open".to_string())];
    let response: KalshiEventsResponse =
        get_json(client, &format!("{KALSHI_BASE}/events"), &query).await?;
    Ok(response.events)
}

#[cfg(test)]
mod calibration_scan_tests {
    use super::*;
    use crate::calibration::CalibrationStore;

    fn market(ticker: &str, event_ticker: &str, last_price: &str, result: &str) -> KalshiMarket {
        KalshiMarket {
            ticker: ticker.to_string(),
            event_ticker: event_ticker.to_string(),
            last_price_dollars: last_price.to_string(),
            result: result.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn settled_market_without_snapshot_is_never_recorded() {
        // The self-fulfilling guard: pre-fix behavior scored the settled
        // market's last_price_dollars (0.99 on a yes result), guaranteeing
        // Brier ≈ 0 for every scan observation. Now it is counted and skipped.
        let mut store = CalibrationStore::new();
        let settled = vec![market("KX-a", "KXEVENT", "0.99", "yes")];
        let mut resolved_without_snapshot = 0;
        let observations = resolved_observations_from_snapshots(
            &settled,
            &mut store,
            &mut resolved_without_snapshot,
        );
        assert!(observations.is_empty());
        assert_eq!(resolved_without_snapshot, 1);
    }

    #[test]
    fn snapshot_then_resolution_scores_the_pre_resolution_price() {
        let mut store = CalibrationStore::new();
        let open = vec![market("KX-a", "KXEVENT", "0.30", "")];
        assert_eq!(snapshot_open_markets(&open, &mut store), 1);
        // The price drifts toward resolution; the EARLIEST snapshot is kept.
        let drifted = vec![market("KX-a", "KXEVENT", "0.95", "")];
        assert_eq!(snapshot_open_markets(&drifted, &mut store), 0);
        let settled = vec![market("KX-a", "KXEVENT", "0.99", "yes")];
        let mut resolved_without_snapshot = 0;
        let observations = resolved_observations_from_snapshots(
            &settled,
            &mut store,
            &mut resolved_without_snapshot,
        );
        assert_eq!(resolved_without_snapshot, 0);
        assert_eq!(observations.len(), 1);
        assert!((observations[0].1.probability - 0.30).abs() < 1e-9);
        assert!(observations[0].1.outcome);
        // The snapshot is consumed — a second pass cannot re-record it.
        let mut second_pass = 0;
        let again = resolved_observations_from_snapshots(&settled, &mut store, &mut second_pass);
        assert!(again.is_empty());
        assert_eq!(second_pass, 1);
    }

    #[test]
    fn untraded_open_market_snapshots_the_quote_midpoint() {
        let mut store = CalibrationStore::new();
        let mut untraded = market("KX-b", "KXEVENT", "", "");
        untraded.yes_bid_dollars = "0.40".to_string();
        untraded.yes_ask_dollars = "0.50".to_string();
        assert_eq!(snapshot_open_markets(&[untraded], &mut store), 1);
        let snapshot = store.take_pending("KX-b").expect("snapshotted");
        assert!((snapshot.probability - 0.45).abs() < 1e-9);
    }
}
