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

// ── realized variance (T4 follow-up) ───────────────────────────────────────

#[test]
fn realized_variance_uses_log_odds_steps() {
    use hkask_mcp_prediction_markets::types::realized_variance;
    // Constant series → zero variance.
    assert_eq!(realized_variance(&[0.5, 0.5, 0.5]), Some(0.0));
    // Moving series → positive variance.
    let v = realized_variance(&[0.5, 0.6, 0.55, 0.65]).expect("computes");
    assert!(v > 0.0);
    // <2 moves → None, never a fabricated 0.
    assert_eq!(realized_variance(&[0.5, 0.6]), None);
    assert_eq!(realized_variance(&[]), None);
}

#[test]
fn history_request_schema_has_no_boolean_positions() {
    let schema =
        schemars::schema_for!(hkask_mcp_prediction_markets::MarketHistoryRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

// ── CMP index ──────────────────────────────────────────────────────────────

#[test]
fn index_spans_the_standard_grid() {
    use hkask_mcp_prediction_markets::cmp::{INDEX_TENORS_DAYS, compute_index};
    let points = [
        TenorPoint { days_to_resolution: 30.0, price: 0.60 },
        TenorPoint { days_to_resolution: 90.0, price: 0.65 },
        TenorPoint { days_to_resolution: 180.0, price: 0.70 },
        TenorPoint { days_to_resolution: 400.0, price: 0.75 },
    ];
    let index = compute_index("TEST", &points, "2026-08-05T00:00:00Z");
    assert_eq!(index.points.len(), INDEX_TENORS_DAYS.len());
    // Interior tenors interpolate; the 7d tenor extrapolates flat from 30d.
    let p30 = index.points.iter().find(|p| p.tenor_days == 30).expect("30d");
    assert!((p30.probability.expect("p") - 0.60).abs() < 1e-9);
    let p90 = index.points.iter().find(|p| p.tenor_days == 90).expect("90d");
    assert!((p90.probability.expect("p") - 0.65).abs() < 1e-9);
}

#[test]
fn index_never_fabricates_on_empty_input() {
    use hkask_mcp_prediction_markets::cmp::compute_index;
    let index = compute_index("TEST", &[], "2026-08-05T00:00:00Z");
    assert!(index.points.iter().all(|p| p.probability.is_none()));
}

#[test]
fn curve_slope_sign_tracks_term_structure() {
    use hkask_mcp_prediction_markets::cmp::{compute_index, curve_slope};
    // Rising curve: longer tenors higher probability.
    let rising = [
        TenorPoint { days_to_resolution: 30.0, price: 0.50 },
        TenorPoint { days_to_resolution: 400.0, price: 0.70 },
    ];
    let index = compute_index("TEST", &rising, "2026-08-05T00:00:00Z");
    let slope = curve_slope(&index, 30, 365).expect("slope");
    assert!(slope > 0.0, "rising curve ⇒ positive slope, got {slope}");
    // Inverted curve.
    let inverted = [
        TenorPoint { days_to_resolution: 30.0, price: 0.70 },
        TenorPoint { days_to_resolution: 400.0, price: 0.50 },
    ];
    let index = compute_index("TEST", &inverted, "2026-08-05T00:00:00Z");
    let slope = curve_slope(&index, 30, 365).expect("slope");
    assert!(slope < 0.0, "inverted curve ⇒ negative slope, got {slope}");
}

#[test]
fn cmp_index_request_schema_has_no_boolean_positions() {
    let schema =
        schemars::schema_for!(hkask_mcp_prediction_markets::MarketCmpIndexRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}
