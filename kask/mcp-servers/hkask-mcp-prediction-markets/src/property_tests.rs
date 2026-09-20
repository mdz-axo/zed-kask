//! Property layer for the venue response parsers and calibration-scan
//! decision cores (`kask/docs/reference/testing-protocol.md` layer 2).
//!
//! The expectation contract: the venue APIs (Polymarket Gamma/CLOB, Kalshi)
//! evolve independently of this server; a drifted response must never
//! fabricate a probability into the calibration store, a market record, a
//! CMP cohort, or the realized-variance window. Deserialization either
//! errors (surfaced as a typed `unavailable` by the provider boundary) or
//! yields records whose derived probabilities are honest: `None`, or a
//! value in [0, 1]. The `.rules` degradation rule is the authority: a
//! degraded parse is skipped or counted, never silently dropped and never
//! replaced with a fabricated value.
//!
//! Oracle: JSON fixtures mirroring the T0-verified venue shapes pinned by
//! the parser structs, with one generated mutation applied at a random
//! path (field removal, null, type swap, garbage string, out-of-range
//! numeric). The mutation classes reproduce what venue drift looks like
//! on the wire: renamed or missing fields, changed types, nulls, and
//! values that parse but are not probabilities.

use proptest::prelude::*;
use serde_json::Value;

use crate::calibration::CalibrationStore;
use crate::calibration::PendingSnapshot;
use crate::provider_kalshi::KalshiCandlesticksResponse;
use crate::provider_kalshi::KalshiMarketsResponse;
use crate::provider_kalshi::candlesticks_to_points;
use crate::provider_kalshi::snapshot_open_markets;
use crate::provider_polymarket::GammaMarket;
use crate::provider_polymarket::resolved_observations_from_snapshots;
use crate::types::canonical_bucket;

// ── Fixtures (T0-verified shapes pinned by the parser structs) ──────────────

/// A Gamma `/markets` body: one open market and one resolved market, so
/// mutations hit both the snapshot path and the resolution path.
fn gamma_markets_fixture() -> Value {
    serde_json::json!([
        {
            "id": "fixture-open",
            "question": "Will the fixture event occur by June 2027?",
            "conditionId": "0xabc",
            "slug": "fixture-open-market",
            "description": "Open fixture market",
            "endDate": "2027-06-30T00:00:00Z",
            "outcomes": "[\"Yes\", \"No\"]",
            "outcomePrices": "[\"0.62\", \"0.38\"]",
            "clobTokenIds": "[\"111\", \"222\"]",
            "active": true,
            "closed": false,
            "volume": "123456",
            "volumeNum": 123456.0,
            "bestBid": 0.61,
            "bestAsk": 0.63,
            "lastTradePrice": 0.62,
            "spread": 0.02,
            "umaResolutionStatus": "",
            "resolvedBy": "",
            "updatedAt": "2026-09-18T00:00:00Z"
        },
        {
            "id": "fixture-resolved",
            "question": "Did the resolved fixture event occur?",
            "conditionId": "0xdef",
            "slug": "fixture-resolved-market",
            "description": "Resolved fixture market",
            "endDate": "2026-06-30T00:00:00Z",
            "outcomes": "[\"Yes\", \"No\"]",
            "outcomePrices": "[\"0.99\", \"0.01\"]",
            "clobTokenIds": "[\"333\", \"444\"]",
            "active": false,
            "closed": true,
            "volume": "999",
            "volumeNum": 999.0,
            "bestBid": 0.99,
            "bestAsk": 1.0,
            "lastTradePrice": 0.99,
            "spread": 0.01,
            "umaResolutionStatus": "resolved",
            "resolvedBy": "uma",
            "updatedAt": "2026-07-01T00:00:00Z"
        }
    ])
}

/// A Kalshi `/markets` body: two active markets with distinct tickers so
/// the snapshot core can be checked per key.
fn kalshi_markets_fixture() -> Value {
    let market = |ticker: &str, bid: &str, ask: &str, last: &str| {
        serde_json::json!({
            "ticker": ticker,
            "event_ticker": "FIXTURE",
            "title": "Fixture event occurs",
            "subtitle": "",
            "status": "active",
            "yes_bid_dollars": bid,
            "yes_ask_dollars": ask,
            "no_bid_dollars": "0.37",
            "no_ask_dollars": "0.39",
            "last_price_dollars": last,
            "volume_fp": "12345",
            "volume_24h_fp": "678",
            "open_interest_fp": "90",
            "liquidity_dollars": "5000.25",
            "close_time": "2027-06-26T00:00:00Z",
            "expiration_time": "2027-06-26T00:00:00Z",
            "result": "",
            "rules_primary": "Fixture rules",
            "updated_time": "2026-09-18T00:00:00Z"
        })
    };
    serde_json::json!({
        "cursor": "fixture-cursor",
        "markets": [
            market("FIXTURE-EVENT-26JUN27", "0.61", "0.63", "0.62"),
            market("FIXTURE-EVENT2-26JUN27", "0.30", "0.32", "0.31")
        ]
    })
}

/// A Kalshi `/markets/candlesticks` body: two daily candles.
fn kalshi_candles_fixture() -> Value {
    let candle = |ts: u64, bid_close: &str, ask_close: &str| {
        serde_json::json!({
            "end_period_ts": ts,
            "yes_bid": { "close_dollars": bid_close },
            "yes_ask": { "close_dollars": ask_close },
            "volume_fp": "100",
            "open_interest_fp": "10"
        })
    };
    serde_json::json!({
        "markets": [
            {
                "candlesticks": [
                    candle(1_800_000_000, "0.61", "0.63"),
                    candle(1_808_640_000, "0.29", "0.33")
                ]
            }
        ]
    })
}

// ── Mutation engine ─────────────────────────────────────────────────────────

/// One venue-drift mutation, applied at a random path of the fixture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mutation {
    /// The venue renamed or dropped the field.
    Remove,
    /// The venue returned null.
    Nullify,
    /// The field changed type (string ↔ number ↔ bool ↔ collection).
    TypeSwap,
    /// The field carries a human-readable placeholder instead of a number.
    GarbageString,
    /// The field parses but is not a probability (the fabrication vector).
    OutOfRangeString,
    /// A numeric field drifted out of range.
    ExtremeNumber,
}

/// Apply `op` at the root/scalar of `value`. `Remove` on a root scalar (no
/// parent) degrades to nullification.
fn apply_op(value: &mut Value, op: Mutation) {
    match op {
        Mutation::Remove | Mutation::Nullify => *value = Value::Null,
        Mutation::TypeSwap => {
            *value = match value {
                Value::Bool(b) => Value::from(*b),
                Value::Number(_) => Value::String("0.5".into()),
                Value::String(_) => Value::Bool(true),
                Value::Array(_) => Value::Object(serde_json::Map::new()),
                Value::Object(_) => Value::Array(Vec::new()),
                Value::Null => Value::Bool(false),
            };
        }
        Mutation::GarbageString => *value = Value::String("N/A (venue drift)".into()),
        Mutation::OutOfRangeString => *value = Value::String("1.7".into()),
        Mutation::ExtremeNumber => *value = Value::from(1.7f64),
    }
}

/// Walk `path` through objects (key by index, deterministic order) and
/// arrays (index modulo length); apply `op` at the path's end. A path
/// running deeper than the value mutates the scalar in place — the
/// venue moved or renamed the nesting level.
fn mutate_at(value: &mut Value, path: &[u8], op: Mutation) {
    let Some((&head, rest)) = path.split_first() else {
        apply_op(value, op);
        return;
    };
    match value {
        Value::Object(map) if !map.is_empty() => {
            let key = map
                .keys()
                .nth(head as usize % map.len())
                .cloned()
                .expect("nth over a non-empty map");
            if rest.is_empty() && op == Mutation::Remove {
                map.remove(&key);
            } else {
                mutate_at(map.get_mut(&key).expect("key chosen by nth"), rest, op);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            let index = head as usize % items.len();
            if rest.is_empty() && op == Mutation::Remove {
                items.remove(index);
            } else {
                mutate_at(&mut items[index], rest, op);
            }
        }
        _ => apply_op(value, op),
    }
}

/// A random path (1-6 hops) plus a random mutation class.
fn mutation_strategy() -> impl Strategy<Value = (Vec<u8>, Mutation)> {
    (
        prop::collection::vec(any::<u8>(), 1..=6),
        proptest::sample::select(vec![
            Mutation::Remove,
            Mutation::Nullify,
            Mutation::TypeSwap,
            Mutation::GarbageString,
            Mutation::OutOfRangeString,
            Mutation::ExtremeNumber,
        ]),
    )
}

/// Deserialize the mutated fixture exactly as the provider does:
/// `serde_json::from_str` over the serialized body.
fn parse<T: for<'de> serde::Deserialize<'de>>(body: &Value) -> Result<T, serde_json::Error> {
    serde_json::from_str(&body.to_string())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Hypothesis: for any single-point mutation of a Gamma `/markets`
    /// body, deserialization either errors or every parsed market's
    /// yes-leg probability is `None` or in [0, 1] — venue drift never
    /// fabricates a probability.
    #[test]
    fn gamma_market_mutations_never_fabricate_probabilities(
        (path, op) in mutation_strategy(),
    ) {
        let mut body = gamma_markets_fixture();
        mutate_at(&mut body, &path, op);
        let Ok(markets) = parse::<Vec<GammaMarket>>(&body) else {
            return Ok(());
        };
        for market in &markets {
            match market.yes_probability() {
                None => {}
                Some(p) => prop_assert!(
                    (0.0..=1.0).contains(&p),
                    "fabricated probability {p} for {}",
                    market.id
                ),
            }
        }
    }

    /// Hypothesis: for any single-point mutation of a Kalshi `/markets`
    /// body, the midpoint derivation never fabricates and the snapshot
    /// decision core never records an out-of-range probability (last
    /// trade, midpoint fallback — every path gated).
    #[test]
    fn kalshi_mutations_never_fabricate_or_snapshot_unbounded_probabilities(
        (path, op) in mutation_strategy(),
    ) {
        let mut body = kalshi_markets_fixture();
        mutate_at(&mut body, &path, op);
        let Ok(response) = parse::<KalshiMarketsResponse>(&body) else {
            return Ok(());
        };
        let mut store = CalibrationStore::new();
        snapshot_open_markets(&response.markets, &mut store);
        for market in &response.markets {
            match market.yes_midpoint() {
                None => {}
                Some(p) => prop_assert!(
                    (0.0..=1.0).contains(&p),
                    "fabricated midpoint {p} for {}",
                    market.ticker
                ),
            }
            // End-to-end through the scan core: a recorded snapshot is
            // either absent (the market was skipped) or bounded.
            if let Some(snapshot) = store.take_pending(&market.ticker) {
                prop_assert!(
                    (0.0..=1.0).contains(&snapshot.probability),
                    "snapshotted unbounded probability {} for {}",
                    snapshot.probability,
                    market.ticker
                );
            }
        }
    }

    /// Hypothesis: for any embedded `outcomePrices` JSON-string content —
    /// Gamma's string-in-JSON quirk, which body-level mutations cannot
    /// reach — the decoded yes-leg probability is `None` or in [0, 1].
    /// This is the direct test of the `yes_probability` range gate:
    /// without it, a parseable out-of-range first price (e.g. "1.7")
    /// fabricates a probability into the calibration snapshot path.
    /// Domain: 0-4 outcome price strings drawn from valid probabilities,
    /// out-of-range numerics, and garbage.
    #[test]
    fn gamma_outcome_prices_content_never_fabricates_probabilities(
        prices in prop::collection::vec(
            proptest::sample::select(vec!["0.62", "1.7", "-0.3", "N/A", "", "0.5", "banana", "2"]),
            0..=4
        ),
    ) {
        let embedded = serde_json::to_string(&prices).expect("serializable outcome strings");
        let market = GammaMarket {
            outcome_prices: embedded.clone(),
            ..Default::default()
        };
        match market.yes_probability() {
            None => {}
            Some(p) => prop_assert!(
                (0.0..=1.0).contains(&p),
                "fabricated probability {p} from outcomePrices {embedded}"
            ),
        }
    }

    /// Hypothesis: every resolved Gamma market is accounted for exactly
    /// once by the resolution core — recorded (snapshot consumed), counted
    /// in `resolved_without_snapshot`, or counted in `skipped_ambiguous`.
    /// A resolved market with no parseable in-range terminal price is
    /// counted as ambiguous, never silently dropped. Domain: the Gamma
    /// fixture with a pending snapshot seeded for every parsed market, so
    /// only a mutation can open the `resolved_without_snapshot` path.
    #[test]
    fn gamma_resolution_core_accounts_every_resolved_market(
        (path, op) in mutation_strategy(),
    ) {
        let mut body = gamma_markets_fixture();
        mutate_at(&mut body, &path, op);
        let Ok(markets) = parse::<Vec<GammaMarket>>(&body) else {
            return Ok(());
        };
        let mut store = CalibrationStore::new();
        for market in &markets {
            store.record_pending(
                &market.id,
                PendingSnapshot {
                    // Same family-first derivation the scanner's writer uses.
                    bucket: crate::semantic_mapping::calibration_bucket_for_gamma(
                        &market.question,
                        &market.slug,
                    ),
                    probability: 0.4,
                },
            );
        }
        let mut skipped_ambiguous = 0u32;
        let mut resolved_without_snapshot = 0u32;
        let observations = resolved_observations_from_snapshots(
            &markets,
            &mut store,
            &mut skipped_ambiguous,
            &mut resolved_without_snapshot,
        );
        let resolved_count = markets
            .iter()
            .filter(|m| m.uma_resolution_status == "resolved")
            .count();
        prop_assert_eq!(
            observations.len() as u32 + skipped_ambiguous + resolved_without_snapshot,
            resolved_count as u32,
            "resolved markets must all be accounted for (recorded + ambiguous + no-snapshot)"
        );
        for (_bucket, observation) in &observations {
            prop_assert!(
                (0.0..=1.0).contains(&observation.probability),
                "recorded unbounded scored probability {}",
                observation.probability
            );
        }
    }

    /// Hypothesis: candle mutations never fabricate out-of-range price
    /// points into the realized-variance window — absent, invalid, or
    /// out-of-range closes drop out, they do not enter as values.
    #[test]
    fn candle_mutations_never_fabricate_unbounded_prices(
        (path, op) in mutation_strategy(),
    ) {
        let mut body = kalshi_candles_fixture();
        mutate_at(&mut body, &path, op);
        let Ok(response) = parse::<KalshiCandlesticksResponse>(&body) else {
            return Ok(());
        };
        let points = candlesticks_to_points(&response);
        for point in &points {
            prop_assert!(
                (0.0..=1.0).contains(&point.price),
                "fabricated price point {} at ts {}",
                point.price,
                point.ts
            );
        }
    }
}
