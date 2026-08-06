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
    store.record(
        "economics",
        ResolvedObservation {
            probability: 0.9,
            outcome: true,
        },
    );
    store.record(
        "economics",
        ResolvedObservation {
            probability: 0.8,
            outcome: true,
        },
    );
    store.record(
        "economics",
        ResolvedObservation {
            probability: 0.1,
            outcome: false,
        },
    );
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
    store.record(
        "x",
        ResolvedObservation {
            probability: 1.0,
            outcome: true,
        },
    );
    let reading = read_calibration(&store, "x");
    assert_eq!(reading.brier, Some(0.0));
    assert!(!reading.stale, "a real 0.0 is a measurement, not stale");
}

#[test]
fn calibration_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketCalibrationRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

// ── T10: loop closure ──────────────────────────────────────────────────────

#[test]
fn loop_closure_persistence_round_trip() {
    // Observations survive a save/load cycle (the journal is the loop's
    // memory across restarts).
    let dir = std::env::temp_dir().join(format!("pm-cal-{}", std::process::id()));
    let path = dir.join("calibration.jsonl");
    let mut store = CalibrationStore::new();
    for i in 0..6 {
        store.record(
            "politics",
            ResolvedObservation {
                probability: 0.55,
                outcome: i % 2 == 0,
            },
        );
    }
    store.save(&path).expect("saves");
    let loaded = CalibrationStore::load(&path).expect("loads");
    let reading = read_calibration(&loaded, "politics");
    assert_eq!(reading.sample_size, 6);
    assert!(!reading.stale);
    let brier = reading.brier.expect("computed");
    assert!(brier > 0.2, "coin-flip guesses score poorly: {brier}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn loop_closure_malformed_journal_line_degrades_to_stale_not_panic() {
    let dir = std::env::temp_dir().join(format!("pm-cal-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("calibration.jsonl");
    std::fs::write(
        &path,
        "{\"bucket\":\"x\",\"probability\":0.9,\"outcome\":true}\nNOT-JSON\n",
    )
    .expect("write");
    let loaded = CalibrationStore::load(&path).expect("loads despite bad line");
    assert_eq!(loaded.sample_size("x"), 1, "good lines survive");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn loop_closure_is_negative_via_tier_demotion() {
    // End-to-end at the library level: 6 resolved markets with terrible
    // calibration (confidently wrong) in a bucket ⇒ that bucket's records
    // demote from High to Medium on the next lookup. This is the corrective
    // polarity assertion from the plan.
    use hkask_mcp_prediction_markets::types;
    let mut store = CalibrationStore::new();
    for _ in 0..6 {
        store.record(
            "Elections",
            ResolvedObservation {
                probability: 0.9,
                outcome: false, // confidently wrong, repeatedly
            },
        );
    }
    let reading = read_calibration(&store, "Elections");
    let block = types::calibration_for(Some(&reading), "Elections");
    let tier = types::reliability_tier(2_000_000.0, Some(0.01), &block);
    assert_eq!(
        tier,
        types::ReliabilityTier::Medium,
        "repeated confidently-wrong resolutions must demote the bucket"
    );
}

#[test]
fn record_resolution_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketRecordResolutionRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

// ── C1: bucket normalization ───────────────────────────────────────────────

#[test]
fn canonical_bucket_unifies_platform_dialects() {
    use hkask_mcp_prediction_markets::types::canonical_bucket;
    // The loop-closure invariant: Kalshi's "Elections" and Polymarket's
    // "Politics" tag accrue calibration under ONE bucket.
    assert_eq!(canonical_bucket("Elections"), canonical_bucket("Politics"));
    assert_eq!(canonical_bucket("Economics"), canonical_bucket("finance"));
    assert_eq!(canonical_bucket("Sports"), "sports");
    // Unknown categories pass through lowercased — coherent, not fabricated.
    assert_eq!(canonical_bucket("Esports"), "esports");
}

#[test]
fn loop_closes_across_platform_dialects() {
    // The C1 scenario end-to-end: observations recorded under the Kalshi
    // dialect inform records bucketed under the Polymarket dialect.
    use hkask_mcp_prediction_markets::types::{calibration_for, canonical_bucket};
    let mut store = CalibrationStore::new();
    for _ in 0..6 {
        store.record(
            &canonical_bucket("Elections"), // Kalshi dialect
            ResolvedObservation {
                probability: 0.9,
                outcome: false,
            },
        );
    }
    let reading = read_calibration(&store, &canonical_bucket("Politics")); // Polymarket dialect
    assert!(!reading.stale, "cross-dialect bucket must close the loop");
    assert_eq!(reading.sample_size, 6);
    let block = calibration_for(Some(&reading), "Politics");
    assert!(!block.stale);
}

#[test]
fn bias_source_is_serialized_provenance() {
    use hkask_mcp_prediction_markets::types::calibration_for;
    let block = calibration_for(None, "Elections");
    assert_eq!(block.bias_source.as_ref(), "static_2602_19520");
    let block = calibration_for(None, "Sports");
    assert_eq!(block.bias_source.as_ref(), "none");
}

#[test]
fn contains_guards_idempotent_ingest() {
    // The resolution scanner re-scans the same settled markets; the store
    // must not double-count.
    let mut store = CalibrationStore::new();
    let observation = ResolvedObservation {
        probability: 0.9,
        outcome: true,
    };
    store.record("politics", observation);
    assert!(store.contains("politics", &observation));
    assert!(!store.contains(
        "politics",
        &ResolvedObservation {
            probability: 0.9,
            outcome: false
        }
    ));
    assert!(!store.contains("economics", &observation));
    // Second identical record would be skipped by the caller's contains check.
    assert_eq!(store.sample_size("politics"), 1);
}

#[test]
fn check_resolutions_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketCheckResolutionsRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}
