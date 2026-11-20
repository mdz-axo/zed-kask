//! Polymarket Gamma API provider (read-only, no auth).
//!
//! Gamma embeds several collections as JSON-encoded
//! strings inside JSON (`outcomes`, `outcomePrices`, `clobTokenIds`) — the
//! parser double-decodes them.

use hkask_mcp_server::server::McpToolError;
use serde::Deserialize;

use crate::http_helpers::classify_http_error;

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

    /// Locate the explicit `(yes_idx, no_idx)` indices for a binary Yes/No
    /// Gamma market. Non-binary or non-Yes/No markets return `None` so
    /// candidate-name or multi-outcome markets never misreport index 0 as Yes
    /// (S076-F02).
    fn binary_yes_no_indices(&self) -> Option<(usize, usize)> {
        let outcomes: Vec<String> = serde_json::from_str(&self.outcomes).ok()?;
        if outcomes.len() != 2 {
            return None;
        }
        let o0 = outcomes[0].trim();
        let o1 = outcomes[1].trim();
        if o0.eq_ignore_ascii_case("yes") && o1.eq_ignore_ascii_case("no") {
            Some((0, 1))
        } else if o0.eq_ignore_ascii_case("no") && o1.eq_ignore_ascii_case("yes") {
            Some((1, 0))
        } else {
            None
        }
    }

    /// CLOB asset (token) IDs, decoded and ordered with the Yes leg first when
    /// `outcomes` declares `["No", "Yes"]`.
    pub fn clob_token_ids(&self) -> Vec<String> {
        let tokens = Self::decode_string_array(&self.clob_token_ids);
        if tokens.len() == 2
            && let Some((yes_idx, no_idx)) = self.binary_yes_no_indices()
            && yes_idx == 1
        {
            return vec![tokens[yes_idx].clone(), tokens[no_idx].clone()];
        }
        tokens
    }

    /// Outcome prices decoded and parsed per slot (`None` for unparsable
    /// entries so index alignment with `outcomes` is strictly preserved).
    pub fn outcome_slot_prices(&self) -> Vec<Option<f64>> {
        Self::decode_string_array(&self.outcome_prices)
            .into_iter()
            .map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
            .collect()
    }

    /// Yes-leg implied probability: locates the explicit `"Yes"` outcome index
    /// in a binary `["Yes", "No"]` / `["No", "Yes"]` market and parses the
    /// index-aligned `outcomePrices[yes_idx]` without dropping unparsable
    /// prefixes (S076-F02). Values outside `[0, 1]`, mismatched array lengths,
    /// or non-Yes/No markets return `None`.
    pub fn yes_probability(&self) -> Option<f64> {
        let (yes_idx, _no_idx) = self.binary_yes_no_indices()?;
        let raw_prices: Vec<String> = serde_json::from_str(&self.outcome_prices).ok()?;
        if raw_prices.len() != 2 {
            return None;
        }
        raw_prices
            .get(yes_idx)?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
    }
}

// ── Calibration scan decision cores (pure, HTTP-free) ─────────────────

/// Snapshot open (unresolved) markets into the calibration store — the
/// honest probability-at-observation for the future Brier score. Re-scanning
/// keeps the EARLIEST snapshot per market; the outcome accounts for every
/// market seen (new, already pending, skipped with a named reason) — a
/// rescan that records nothing reports why, never a silent zero.
pub(crate) fn snapshot_open_markets(
    markets: &[GammaMarket],
    store: &mut crate::calibration::CalibrationStore,
) -> crate::calibration::ScanSnapshotOutcome {
    let mut outcome = crate::calibration::ScanSnapshotOutcome::default();
    for market in markets {
        if market.closed || market.uma_resolution_status == "resolved" {
            outcome.skipped_not_open += 1;
            continue;
        }
        let Some(probability) = market.yes_probability() else {
            outcome.skipped_no_price += 1;
            continue;
        };
        // Family-first bucket: base events accrue under the semantic family
        // label readers query, not the per-market slug key.
        let bucket =
            crate::semantic_mapping::calibration_bucket_for_gamma(&market.question, &market.slug);
        if store.record_pending(
            &market.id,
            crate::calibration::PendingSnapshot {
                bucket: bucket.clone(),
                probability,
            },
        ) {
            outcome.new.push(crate::calibration::SnapshotIdentity {
                market: market.id.clone(),
                bucket,
                probability,
            });
        } else {
            outcome.already_snapshotted += 1;
        }
    }
    outcome
}

/// Consume pre-resolution snapshots for resolved markets. The outcome is
/// derived from the terminal price (>=0.99 yes / <=0.01 no — for a resolved
/// market the terminal price IS the resolution declaration); the scored
/// probability comes from the pre-resolution snapshot. A resolved market
/// with no parseable in-range terminal price counts in `skipped_ambiguous`
/// — an unparseable or out-of-range price is ambiguous data, never a
/// silent drop and never a fabricated outcome. A resolved market
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
            // No parseable in-range terminal price: ambiguous, not a drop.
            *skipped_ambiguous += 1;
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
                    market_key: Some(market.id.clone()),
                },
            )),
            None => *resolved_without_snapshot += 1,
        }
    }
    observations
}

/// The shared Gamma/CLOB request chain: send, read the body, classify HTTP
/// failures, and parse the body as `T`. `service` names the endpoint family
/// in error messages ("Gamma", "CLOB"); `what` names the payload in the
/// parse-failure message.
async fn fetch_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, String)],
    service: &str,
    what: &str,
) -> Result<T, McpToolError> {
    let response = client
        .get(url)
        .query(query)
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("{service} request failed: {e}")))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("{service} body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error(
            &format!("Polymarket {service}"),
            status,
            &body,
        ));
    }
    // A body our structs cannot read repeats on every retry — `internal`,
    // never the transient `unavailable` (the Kalshi decode's rule).
    serde_json::from_str(&body)
        .map_err(|e| McpToolError::internal(format!("{service} {what} parse failed: {e}")))
}

/// Fetch markets directly (not via events). `closed=true` returns
/// resolved/closed markets — the resolution-check feed.
pub async fn fetch_markets(
    client: &reqwest::Client,
    limit: u32,
    closed: bool,
) -> Result<Vec<GammaMarket>, McpToolError> {
    fetch_json(
        client,
        &format!("{GAMMA_BASE}/markets"),
        &[("limit", limit.to_string()), ("closed", closed.to_string())],
        "Gamma",
        "markets",
    )
    .await
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
    #[derive(serde::Deserialize)]
    struct History {
        history: Vec<ClobPricePoint>,
    }
    let parsed: History = fetch_json(
        client,
        "https://clob.polymarket.com/prices-history",
        &[
            ("market", token_id.to_string()),
            ("interval", "1d".to_string()),
            ("fidelity", "60".to_string()),
        ],
        "CLOB",
        "prices-history",
    )
    .await?;
    Ok(parsed.history)
}

/// Fetch the open event with this slug (empty when Gamma has none). Gamma
/// filters by slug server-side, so the lookup does not depend on the event
/// being in the first page of open events.
pub async fn fetch_events_by_slug(
    client: &reqwest::Client,
    slug: &str,
) -> Result<Vec<GammaEvent>, McpToolError> {
    fetch_json(
        client,
        &format!("{GAMMA_BASE}/events"),
        &[("slug", slug.to_string()), ("closed", "false".to_string())],
        "Gamma",
        "events",
    )
    .await
}

/// Fetch active, open events from Gamma.
pub async fn fetch_events(
    client: &reqwest::Client,
    limit: u32,
) -> Result<Vec<GammaEvent>, McpToolError> {
    fetch_json(
        client,
        &format!("{GAMMA_BASE}/events"),
        &[
            ("limit", limit.to_string()),
            ("active", "true".to_string()),
            ("closed", "false".to_string()),
        ],
        "Gamma",
        "events",
    )
    .await
}

#[cfg(test)]
mod calibration_scan_tests {
    use super::*;
    use crate::calibration::CalibrationStore;

    fn gamma(id: &str, slug: &str, prices: &str, resolved: bool, closed: bool) -> GammaMarket {
        GammaMarket {
            id: id.to_string(),
            slug: slug.to_string(),
            outcomes: "[\"Yes\", \"No\"]".to_string(),
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
        let open = vec![gamma(
            "pm-1",
            "will-x-happen",
            "[\"0.35\", \"0.65\"]",
            false,
            false,
        )];
        assert_eq!(snapshot_open_markets(&open, &mut store).new.len(), 1);
        // Drift toward resolution keeps the earliest snapshot — and the
        // rescan attributes the market instead of a silent zero.
        let drifted = vec![gamma(
            "pm-1",
            "will-x-happen",
            "[\"0.90\", \"0.10\"]",
            false,
            false,
        )];
        let rescan = snapshot_open_markets(&drifted, &mut store);
        assert!(rescan.new.is_empty());
        assert_eq!(rescan.already_snapshotted, 1);
        let resolved = vec![gamma(
            "pm-1",
            "will-x-happen",
            "[\"1.0\", \"0.0\"]",
            true,
            true,
        )];
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
        let open = vec![gamma(
            "pm-2",
            "will-y-happen",
            "[\"0.55\", \"0.45\"]",
            false,
            false,
        )];
        assert_eq!(snapshot_open_markets(&open, &mut store).new.len(), 1);
        let ambiguous = vec![gamma(
            "pm-2",
            "will-y-happen",
            "[\"0.55\", \"0.45\"]",
            true,
            true,
        )];
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

    #[test]
    fn open_scan_accounts_closed_and_unpriced_markets() {
        // Every market seen gets a disposition; a silent zero is unattributable.
        let mut store = CalibrationStore::new();
        let markets = vec![
            gamma(
                "pm-3",
                "will-z-happen",
                "[\"0.60\", \"0.40\"]",
                false,
                false,
            ),
            gamma("pm-4", "already-closed", "[\"1\", \"0\"]", true, true),
            gamma("pm-5", "no-price", "not-json", false, false),
        ];
        let outcome = snapshot_open_markets(&markets, &mut store);
        assert_eq!(outcome.new.len(), 1);
        assert_eq!(outcome.skipped_not_open, 1, "closed market accounted");
        assert_eq!(outcome.skipped_no_price, 1, "unparseable price accounted");
        assert_eq!(outcome.already_snapshotted, 0);
    }

    #[test]
    fn fed_question_snapshots_under_the_family_bucket() {
        // A rates question accrues under the family bucket readers query —
        // not the per-market slug key.
        let mut store = CalibrationStore::new();
        let mut fed = gamma("pm-6", "fed-decision", "[\"0.55\", \"0.45\"]", false, false);
        fed.question = "Will the Fed raise rates at the next FOMC meeting?".to_string();
        let outcome = snapshot_open_markets(&[fed], &mut store);
        assert_eq!(outcome.new.len(), 1);
        assert_eq!(
            outcome.new[0].bucket, "policy_interest_rate",
            "family-first bucket, not the slug key"
        );
    }

    #[test]
    fn clob_token_ids_decode_from_the_json_string_field() {
        let mut market = gamma("pm-7", "clob-test", "[]", false, false);
        market.clob_token_ids = "[\"111\", \"222\"]".to_string();
        assert_eq!(
            market.clob_token_ids(),
            vec!["111".to_string(), "222".to_string()]
        );
        // Unparsable field decodes empty — never fabricated.
        let mut broken = gamma("pm-8", "clob-broken", "[]", false, false);
        broken.clob_token_ids = "not-json".to_string();
        assert!(broken.clob_token_ids().is_empty());
    }

    /// S076-F02: unparsable Yes-leg price must never shift the No-leg price into
    /// `yes_probability()`, reversed `["No", "Yes"]` outcome order selects the
    /// `"Yes"` index, and non-Yes/No multi-outcome markets return `None`.
    #[test]
    fn yes_probability_selects_explicit_yes_leg_without_index_shift() {
        let unparsable_yes = GammaMarket {
            id: "pm-shift".into(),
            outcomes: "[\"Yes\", \"No\"]".into(),
            outcome_prices: "[\"N/A\", \"0.25\"]".into(),
            ..Default::default()
        };
        assert_eq!(
            unparsable_yes.yes_probability(),
            None,
            "unparsable Yes-leg must never shift No-leg 0.25 into yes_probability"
        );

        let no_won_missing_yes = GammaMarket {
            id: "pm-nowin".into(),
            outcomes: "[\"Yes\", \"No\"]".into(),
            outcome_prices: "[\"\", \"1.0\"]".into(),
            uma_resolution_status: "resolved".into(),
            closed: true,
            ..Default::default()
        };
        assert_eq!(no_won_missing_yes.yes_probability(), None);

        let reversed = GammaMarket {
            id: "pm-rev".into(),
            outcomes: "[\"No\", \"Yes\"]".into(),
            outcome_prices: "[\"0.25\", \"0.75\"]".into(),
            clob_token_ids: "[\"tok-no\", \"tok-yes\"]".into(),
            ..Default::default()
        };
        assert_eq!(reversed.yes_probability(), Some(0.75));
        assert_eq!(
            reversed.clob_token_ids(),
            vec!["tok-yes".to_string(), "tok-no".to_string()],
            "Yes leg token must come first when outcomes are [No, Yes]"
        );

        let non_binary = GammaMarket {
            id: "pm-teams".into(),
            outcomes: "[\"Lakers\", \"Celtics\"]".into(),
            outcome_prices: "[\"0.60\", \"0.40\"]".into(),
            ..Default::default()
        };
        assert_eq!(non_binary.yes_probability(), None);
    }
}
