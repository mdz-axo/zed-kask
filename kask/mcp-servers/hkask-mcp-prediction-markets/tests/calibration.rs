//! Tests for the calibration store and reading (T5).

use hkask_mcp_prediction_markets::calibration::{
    CalibrationStore, ResolvedObservation, read_calibration,
};

#[test]
fn empty_bucket_is_stale_not_zero() {
    let store = CalibrationStore::new();
    let reading = read_calibration(&store, "politics");
    assert!(reading.stale);
    assert_eq!(reading.brier, None);
    assert_eq!(reading.sample_size, 0);
}

#[test]
fn populated_bucket_computes_brier() {
    let mut store = CalibrationStore::new();
    // Well-calibrated mini sample: high-prob correct, low-prob incorrect.
    store.record("economics", ResolvedObservation { probability: 0.9, outcome: true });
    store.record("economics", ResolvedObservation { probability: 0.8, outcome: true });
    store.record("economics", ResolvedObservation { probability: 0.1, outcome: false });
    let reading = read_calibration(&store, "economics");
    assert!(!reading.stale);
    let brier = reading.brier.expect("computed");
    assert!(brier < 0.05, "good sample should score low, got {brier}");
    assert_eq!(reading.sample_size, 3);
}

#[test]
fn brier_uses_hkask_forecast_math() {
    // Pin delegation to the shared crate: a deterministic pair (p=1, outcome
    // true) must score exactly 0 via brier_score_multi — if someone swaps in
    // a local reimplementation this catches drift.
    let mut store = CalibrationStore::new();
    store.record("x", ResolvedObservation { probability: 1.0, outcome: true });
    let reading = read_calibration(&store, "x");
    assert_eq!(reading.brier, Some(0.0));
    assert!(!reading.stale, "a real 0.0 is a measurement, not stale");
}

#[test]
fn calibration_request_schema_has_no_boolean_positions() {
    let schema =
        schemars::schema_for!(hkask_mcp_prediction_markets::MarketCalibrationRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}
