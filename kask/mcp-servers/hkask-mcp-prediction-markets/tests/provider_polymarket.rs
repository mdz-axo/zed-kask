//! Fixture tests for the Polymarket Gamma provider.
//!
//! The fixture is a captured live response (T0, 2026-08-05). If Gamma
//! changes its shape, refresh the fixture and update the provider together.

use hkask_mcp_prediction_markets::provider_polymarket::GammaEvent;

const FIXTURE: &str = include_str!("fixtures/polymarket_events.json");

#[test]
fn fixture_parses_events_with_embedded_markets() {
    let events: Vec<GammaEvent> = serde_json::from_str(FIXTURE).expect("fixture parses");
    assert!(!events.is_empty(), "fixture has events");
    let event = &events[0];
    assert!(!event.id.is_empty());
    assert!(!event.slug.is_empty());
    assert!(!event.markets.is_empty(), "event has embedded markets");
}

#[test]
fn market_double_decodes_json_string_fields() {
    let events: Vec<GammaEvent> = serde_json::from_str(FIXTURE).expect("fixture parses");
    let market = &events[0].markets[0];
    // Gamma quirk: outcomes/outcomePrices/clobTokenIds are JSON strings in JSON.
    let names = market.outcome_names();
    let prices = market.prices();
    let tokens = market.token_ids();
    assert_eq!(names.len(), 2, "binary market has two outcomes");
    assert_eq!(prices.len(), 2);
    assert_eq!(tokens.len(), 2);
    assert!(prices.iter().all(|p| (0.0..=1.0).contains(p)));
}

#[test]
fn yes_probability_is_first_outcome_price() {
    let events: Vec<GammaEvent> = serde_json::from_str(FIXTURE).expect("fixture parses");
    let market = &events[0].markets[0];
    let yes = market.yes_probability().expect("fixture has a yes price");
    assert!((0.0..=1.0).contains(&yes));
    assert_eq!(Some(yes), market.prices().first().copied());
}

#[test]
fn resolution_fields_present() {
    let events: Vec<GammaEvent> = serde_json::from_str(FIXTURE).expect("fixture parses");
    let statuses: Vec<&str> = events
        .iter()
        .flat_map(|e| e.markets.iter())
        .map(|m| m.uma_resolution_status.as_str())
        .collect();
    // T0 observed "resolved"; pin that the field is populated, whatever value.
    assert!(statuses.iter().any(|s| !s.is_empty()));
}

#[test]
fn http_error_classification_is_per_variant() {
    use hkask_mcp_server::server::classify_http_error;
    let not_found = classify_http_error("Gamma", reqwest::StatusCode::NOT_FOUND, "nope");
    let rate = classify_http_error("Gamma", reqwest::StatusCode::TOO_MANY_REQUESTS, "slow");
    let unavailable =
        classify_http_error("Gamma", reqwest::StatusCode::SERVICE_UNAVAILABLE, "down");
    // Wire format carries the variant kind; blanket `internal` would fail this.
    for (err, expected) in [(not_found, "not_found"), (rate, "rate_limited"), (unavailable, "unavailable")] {
        let wire = err.to_string();
        assert!(
            wire.contains(expected),
            "expected kind {expected} in wire format, got: {wire}"
        );
    }
}
