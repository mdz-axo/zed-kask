//! Tests for CMP construction (T14).

use hkask_mcp_prediction_markets::cmp::{
    CmpMethod, TenorPoint, constant_maturity, parse_base_events,
};
use hkask_forecast::{from_log_odds, log_odds};

#[test]
fn interpolated_cmp_lies_between_bracketing_cohorts_in_log_odds() {
    // 30d cohort at 0.70, 90d cohort at 0.50 → 60d CMP must interpolate
    // log-odds: midpoint of logits, not of probabilities.
    let points = [
        TenorPoint { days_to_resolution: 30.0, price: 0.70 },
        TenorPoint { days_to_resolution: 90.0, price: 0.50 },
    ];
    let cmp = constant_maturity(&points, 60).expect("interpolates");
    assert!(matches!(cmp.method, CmpMethod::Interpolated));
    let expected = from_log_odds((log_odds(0.70) + log_odds(0.50)) / 2.0);
    assert!(
        (cmp.probability - expected).abs() < 1e-9,
        "log-odds midpoint {expected} vs {}",
        cmp.probability
    );
    // Sanity: log-odds midpoint of 0.7/0.5 ≈ 0.608, NOT the linear 0.60.
    assert!(cmp.probability > 0.60);
}

#[test]
fn single_cohort_degrades_to_bucketed_sparse() {
    let points = [TenorPoint { days_to_resolution: 45.0, price: 0.65 }];
    let cmp = constant_maturity(&points, 90).expect("bucketed");
    assert!(matches!(cmp.method, CmpMethod::BucketedSparse));
    assert_eq!(cmp.cohorts, 1);
    // Honest uncertainty: the bracket width reports the distance.
    assert!((cmp.bracket_days - 45.0).abs() < 1e-9);
    assert!((cmp.probability - 0.65).abs() < 1e-9);
}

#[test]
fn empty_input_is_none_never_fabricated() {
    assert!(constant_maturity(&[], 30).is_none());
}

#[test]
fn extrapolation_is_flat_never_invents_slope() {
    let points = [
        TenorPoint { days_to_resolution: 30.0, price: 0.60 },
        TenorPoint { days_to_resolution: 90.0, price: 0.70 },
    ];
    let near = constant_maturity(&points, 10).expect("near extrapolation");
    let far = constant_maturity(&points, 200).expect("far extrapolation");
    assert!((near.probability - 0.60).abs() < 1e-9);
    assert!((far.probability - 0.70).abs() < 1e-9);
}

#[test]
fn same_tenor_markets_share_a_cohort() {
    let points = [
        TenorPoint { days_to_resolution: 30.0, price: 0.60 },
        TenorPoint { days_to_resolution: 30.2, price: 0.64 },
        TenorPoint { days_to_resolution: 90.0, price: 0.50 },
    ];
    let cmp = constant_maturity(&points, 60).expect("interpolates");
    assert_eq!(cmp.cohorts, 2, "near-identical tenors merge into one cohort");
}

#[test]
fn base_events_parse_config_format() {
    let parsed = parse_base_events("economics:KXFEDDECISION, politics:KXPREZ-28");
    assert_eq!(
        parsed,
        vec![
            ("economics".to_string(), "KXFEDDECISION".to_string()),
            ("politics".to_string(), "KXPREZ-28".to_string())
        ]
    );
    // Malformed entries are dropped, never guessed.
    assert!(parse_base_events("garbage,also: ").is_empty());
}

#[test]
fn cmp_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketCmpRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

#[test]
fn unregistered_series_is_refused() {
    // The tool-level gate is exercised here at the registry level: a market
    // can never auto-promote to base-event status. (The live refusal path is
    // in the tool handler; this pins the registry semantics the gate reads.)
    let registry = parse_base_events("economics:KXFEDDECISION");
    assert!(!registry.iter().any(|(_, s)| s == "KXRANDOM"));
    assert!(registry.iter().any(|(_, s)| s == "KXFEDDECISION"));
}
