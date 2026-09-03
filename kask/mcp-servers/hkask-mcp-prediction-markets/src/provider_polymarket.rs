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

    /// Outcome prices decoded and parsed; unparsable entries are dropped
    /// (index alignment with names is preserved for parseable prefixes).
    pub fn prices(&self) -> Vec<f64> {
        Self::decode_string_array(&self.outcome_prices)
            .iter()
            .filter_map(|p| p.parse::<f64>().ok())
            .collect()
    }

    /// Yes-leg implied probability: `outcomePrices[0]` by Gamma convention
    /// (first outcome is "Yes" for binary markets).
    pub fn yes_probability(&self) -> Option<f64> {
        self.prices().first().copied()
    }
}

// ── Calibration scan decision cores (pure, HTTP-free) ─────────────────

/// Snapshot open (unresolved) markets into the calibration store — the
/// honest probability-at-observation for the future Brier score. Returns
/// the count of markets snapshotted for the first time; re-scanning keeps
/// the EARLIEST snapshot per market.
pub(crate) fn snapshot_open_markets(
    markets: &[GammaMarket],
    store: &mut crate::calibration::CalibrationStore,
) -> u32 {
    let mut snapshotted = 0;
    for market in markets {
        if market.closed || market.uma_resolution_status == "resolved" {
            continue;
        }
        let Some(probability) = market.yes_probability() else {
            continue;
        };
        let bucket = crate::types::canonical_bucket(&market.slug);
        if store.record_pending(
            &market.id,
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

/// Consume pre-resolution snapshots for resolved markets. The outcome is
/// derived from the terminal price (>=0.99 yes / <=0.01 no — for a resolved
/// market the terminal price IS the resolution declaration); the scored
/// probability comes from the pre-resolution snapshot. A resolved market
/// with no snapshot is counted in `resolved_without_snapshot` and NEVER
/// recorded — its terminal price is not an observation, and scoring it
/// would be self-fulfilling (Brier ≈ 0 by construction).
pub(crate) fn resolved_observations_from_snapshots(
    markets: &[GammaMarket],
    store: &mut crate::calibration::CalibrationStore,
    skipped_ambiguous: &mut u32,
    resolved_without_snapshot: &mut u32,
) -> Vec<(String, crate::calibration::ResolvedObservation)> {
    let mut observations = Vec::new();
    for market in markets {
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
            *skipped_ambiguous += 1;
            None
        };
        let Some(outcome) = outcome else { continue };
        match store.take_pending(&market.id) {
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
        .query(&[("limit", limit.to_string()), ("closed", closed.to_string())])
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
        .map_err(|e| McpToolError::unavailable(format!("Gamma markets parse failed: {e}")))
}

/// One point on a CLOB price-history series.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct ClobPricePoint {
    #[serde(rename = "t")]
    pub ts: u64,
    #[serde(rename = "p")]
    pub price: f64,
}

/// Fetch price history for a CLOB token (Yes leg) — the Polymarket history
/// source for realized-variance computation.
pub async fn fetch_prices_history(
    client: &reqwest::Client,
    token_id: &str,
) -> Result<Vec<ClobPricePoint>, McpToolError> {
    let url = "https://clob.polymarket.com/prices-history";
    let response = client
        .get(url)
        .query(&[("market", token_id), ("interval", "1d"), ("fidelity", "60")])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("CLOB request failed: {e}")))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("CLOB body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("Polymarket CLOB", status, &body));
    }
    #[derive(serde::Deserialize)]
    struct History {
        history: Vec<ClobPricePoint>,
    }
    let parsed: History = serde_json::from_str(&body)
        .map_err(|e| McpToolError::unavailable(format!("CLOB prices-history parse failed: {e}")))?;
    Ok(parsed.history)
}

/// Fetch active, open events from Gamma.
pub async fn fetch_events(
    client: &reqwest::Client,
    limit: u32,
) -> Result<Vec<GammaEvent>, McpToolError> {
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
        .map_err(|e| McpToolError::unavailable(format!("Gamma events parse failed: {e}")))
}

#[cfg(test)]
mod calibration_scan_tests {
    use super::*;
    use crate::calibration::CalibrationStore;

    fn gamma(id: &str, slug: &str, prices: &str, resolved: bool, closed: bool) -> GammaMarket {
        GammaMarket {
            id: id.to_string(),
            slug: slug.to_string(),
            outcome_prices: prices.to_string(),
            uma_resolution_status: if resolved {
                "resolved".to_string()
            } else {
                String::new()
            },
            closed,
            ..Default::default()
        }
    }

    #[test]
    fn resolved_market_without_snapshot_is_never_recorded() {
        // Pre-fix behavior scored the resolved market's terminal price (1.0
        // on yes), which is the outcome source itself — Brier ≈ 0 by
        // construction. Now it is counted and skipped.
        let mut store = CalibrationStore::new();
        let resolved = vec![gamma("pm-1", "will-x-happen", "[\"1\", \"0\"]", true, true)];
        let mut skipped_ambiguous = 0;
        let mut resolved_without_snapshot = 0;
        let observations = resolved_observations_from_snapshots(
            &resolved,
            &mut store,
            &mut skipped_ambiguous,
            &mut resolved_without_snapshot,
        );
        assert!(observations.is_empty());
        assert_eq!(resolved_without_snapshot, 1);
        assert_eq!(skipped_ambiguous, 0);
    }

    #[test]
    fn snapshot_then_resolution_scores_the_pre_resolution_price() {
        let mut store = CalibrationStore::new();
        let open = vec![gamma("pm-1", "will-x-happen", "[\"0.35\", \"0.65\"]", false, false)];
        assert_eq!(snapshot_open_markets(&open, &mut store), 1);
        // Drift toward resolution keeps the earliest snapshot.
        let drifted = vec![gamma("pm-1", "will-x-happen", "[\"0.90\", \"0.10\"]", false, false)];
        assert_eq!(snapshot_open_markets(&drifted, &mut store), 0);
        let resolved = vec![gamma("pm-1", "will-x-happen", "[\"1.0\", \"0.0\"]", true, true)];
        let mut skipped_ambiguous = 0;
        let mut resolved_without_snapshot = 0;
        let observations = resolved_observations_from_snapshots(
            &resolved,
            &mut store,
            &mut skipped_ambiguous,
            &mut resolved_without_snapshot,
        );
        assert_eq!(observations.len(), 1);
        assert!((observations[0].1.probability - 0.35).abs() < 1e-9);
        assert!(observations[0].1.outcome);
    }

    #[test]
    fn ambiguous_resolution_is_skipped_never_fabricated() {
        let mut store = CalibrationStore::new();
        let open = vec![gamma("pm-2", "will-y-happen", "[\"0.55\", \"0.45\"]", false, false)];
        assert_eq!(snapshot_open_markets(&open, &mut store), 1);
        let ambiguous = vec![gamma("pm-2", "will-y-happen", "[\"0.55\", \"0.45\"]", true, true)];
        let mut skipped_ambiguous = 0;
        let mut resolved_without_snapshot = 0;
        let observations = resolved_observations_from_snapshots(
            &ambiguous,
            &mut store,
            &mut skipped_ambiguous,
            &mut resolved_without_snapshot,
        );
        assert!(observations.is_empty());
        assert_eq!(skipped_ambiguous, 1);
        assert_eq!(resolved_without_snapshot, 0);
    }
}
