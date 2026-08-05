//! Fixture tests for the Kalshi REST provider.
//!
//! Fixtures are captured live responses (T0, 2026-08-05).

use hkask_mcp_prediction_markets::provider_kalshi::{
    KalshiEventsResponse, KalshiMarketsResponse, parse_fp,
};

const MARKETS_FIXTURE: &str = include_str!("fixtures/kalshi_markets.json");
const EVENTS_FIXTURE: &str = include_str!("fixtures/kalshi_events.json");

#[test]
fn markets_fixture_parses_with_string_numerics() {
    let response: KalshiMarketsResponse =
        serde_json::from_str(MARKETS_FIXTURE).expect("fixture parses");
    assert!(!response.markets.is_empty());
    let market = &response.markets[0];
    assert!(!market.ticker.is_empty());
    assert!(!market.event_ticker.is_empty());
    // All numerics arrive as strings — the fixture would fail bare-f64 serde.
    assert!(!market.yes_bid_dollars.is_empty());
    assert!(!market.open_interest_fp.is_empty());
}

#[test]
fn yes_midpoint_and_spread_from_two_sided_quote() {
    let response: KalshiMarketsResponse =
        serde_json::from_str(MARKETS_FIXTURE).expect("fixture parses");
    let market = &response.markets[0];
    let bid = parse_fp(&market.yes_bid_dollars).expect("bid parses");
    let ask = parse_fp(&market.yes_ask_dollars).expect("ask parses");
    let mid = market.yes_midpoint().expect("midpoint computable");
    let spread = market.spread().expect("spread computable");
    assert!((mid - (bid + ask) / 2.0).abs() < 1e-12);
    assert!((spread - (ask - bid)).abs() < 1e-12);
    assert!(spread >= 0.0);
}

#[test]
fn events_fixture_carries_settlement_sources_and_category() {
    let response: KalshiEventsResponse =
        serde_json::from_str(EVENTS_FIXTURE).expect("fixture parses");
    assert!(!response.events.is_empty());
    let event = &response.events[0];
    assert!(!event.event_ticker.is_empty());
    assert!(!event.series_ticker.is_empty());
    assert!(!event.category.is_empty());
    // Settlement sources feed dcterms:provenance in the T4 contract.
    assert!(!event.settlement_sources.is_empty());
    assert!(!event.settlement_sources[0].name.is_empty());
}

#[test]
fn parse_fp_handles_empty_and_invalid() {
    assert_eq!(parse_fp(""), None);
    assert_eq!(parse_fp("not-a-number"), None);
    assert_eq!(parse_fp("0.5700"), Some(0.57));
}

#[test]
fn http_error_classification_is_per_variant() {
    use hkask_mcp_server::server::classify_http_error;
    let rate = classify_http_error("Kalshi", reqwest::StatusCode::TOO_MANY_REQUESTS, "slow");
    assert!(rate.to_string().contains("rate_limited"));
    let not_found = classify_http_error("Kalshi", reqwest::StatusCode::NOT_FOUND, "nope");
    assert!(not_found.to_string().contains("not_found"));
}

#[test]
fn market_parses_when_optional_fields_absent() {
    // Live regression (2026-08-05): production Kalshi markets omit `subtitle`
    // and other optional fields; the struct must tolerate absence.
    let raw = r#"{"ticker":"KXFED-27APR-T4.25","event_ticker":"KXFED-27APR","title":"Fed above 4.25?","status":"active","yes_bid_dollars":"0.3000","yes_ask_dollars":"0.3400","volume_fp":"10197.97","close_time":"2027-04-28T17:55:00Z"}"#;
    let market: hkask_mcp_prediction_markets::provider_kalshi::KalshiMarket =
        serde_json::from_str(raw).expect("parses with absent optional fields");
    assert_eq!(market.ticker, "KXFED-27APR-T4.25");
    assert!(market.subtitle.is_empty());
    let mid = market.yes_midpoint().expect("midpoint");
    assert!((mid - 0.32).abs() < 1e-9);
}
