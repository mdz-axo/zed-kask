//! Behavioral seam tests for shared market-contract domain types, stopword-filtered
//! Jaccard `token_overlap`, and DR-AS (`arXiv:2607.08199`) structural volatility math
//! in `hkask_forecast`.

use std::borrow::Cow;

use hkask_forecast::market_contract::{
    Calibration, MarketRecord, MarketStatus, OntologyBlock, ProbabilityMethod, ProcessAxis,
    ReliabilityTier, Source, StateAxis, StructuralFlag, Volatility, VolumeGrain, canonical_bucket,
    realized_variance, reliability_tier, structural_flag, token_overlap,
};
use hkask_forecast::volatility::{
    ActivityProxy, DrasConfig, VolatilityInputs, forecast as dras_forecast,
};

#[test]
fn market_record_serde_wire_compatibility_and_reliability_covariates() {
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.02),
        volume: 60_000.0,
    };
    let dras = dras_forecast(inputs, DrasConfig::default(), 1.0).expect("valid DR-AS forecast");
    assert!(dras.conditional_variance > dras.dr_variance);
    assert!(dras.interval_95.0 >= 0.0 && dras.interval_95.1 <= 1.0);

    let tier = reliability_tier(60_000.0, Some(0.02));
    assert_eq!(tier, ReliabilityTier::High);
    assert_eq!(reliability_tier(100.0, Some(0.02)), ReliabilityTier::Low);

    let flag = structural_flag(0.52, Some(3.0));
    assert_eq!(flag, StructuralFlag::NearDeadlineAndCoinflip);

    let record = MarketRecord {
        source: Source::Kalshi,
        event_id: "KXFED-27DEC".to_string(),
        market_id: "KXFED-27DEC-H0".to_string(),
        question: "Will the Federal Reserve hold rates in December 2027?".to_string(),
        description: "FOMC target rate decision.".to_string(),
        category: canonical_bucket("Economics"),
        series: "KXFEDDECISION".to_string(),
        deadline: "2027-12-15T19:00:00Z".to_string(),
        time_to_maturity: Some(0.25),
        probability: 0.52,
        probability_method: ProbabilityMethod::Midpoint,
        spread: Some(0.02),
        volume: 60_000.0,
        volume_grain: VolumeGrain::Market,
        liquidity: Some(120_000.0),
        open_interest: Some(45_000.0),
        last_update: "2027-09-15T12:00:00Z".to_string(),
        clob_asset_ids: None,
        volatility: Volatility {
            realized_variance: realized_variance(&[0.48, 0.50, 0.52]),
            structural_flag: flag,
            interpretation: Cow::Borrowed("high"),
            dras_forecast: None,
        },
        status: MarketStatus::Open,
        resolved_outcome: None,
        resolution_source: Cow::Borrowed("kalshi_exchange"),
        calibration: Calibration {
            brier: Some(0.11),
            sample_size: 42,
            stale: false,
        },
        reliability_tier: tier,
        ontology: OntologyBlock {
            process: ProcessAxis {
                r#type: Cow::Borrowed("test-process-type"),
                stage: Cow::Borrowed("trading"),
                probability_role: Cow::Borrowed("test-probability-role"),
            },
            state: StateAxis {
                identifier: "kalshi:KXFED-27DEC-H0".to_string(),
                title: "Will the Federal Reserve hold rates in December 2027?".to_string(),
                description: "FOMC target rate decision.".to_string(),
                temporal: "2027-12-15T19:00:00Z".to_string(),
                provenance: Cow::Borrowed("kalshi_exchange"),
            },
            mapping_version: 1,
        },
    };

    // Wire contract: None dras_forecast is omitted; missing clob_asset_ids deserializes as None.
    let mut encoded = serde_json::to_value(&record).expect("serialize MarketRecord");
    assert_eq!(encoded["source"], "kalshi");
    assert_eq!(encoded["probability_method"], "midpoint");
    assert_eq!(encoded["status"], "open");
    assert_eq!(encoded["reliability_tier"], "high");
    assert!(encoded["volatility"].get("dras_forecast").is_none());
    encoded
        .as_object_mut()
        .expect("obj")
        .remove("clob_asset_ids");

    let decoded: MarketRecord = serde_json::from_value(encoded).expect("deserialize MarketRecord");
    assert_eq!(decoded.market_id, "KXFED-27DEC-H0");
    assert!(decoded.clob_asset_ids.is_none());
    assert!(decoded.volatility.realized_variance.is_some());
}

#[test]
fn token_overlap_filters_stopwords_and_detects_duplicate_questions() {
    let q1 = "Will the Federal Reserve cut interest rates before 2027?";
    let q2 = "Does the Federal Reserve cut interest rates in 2027?";
    let unrelated = "Will SpaceX land Starship on Mars?";

    let high = token_overlap(q1, q2);
    let zero = token_overlap(q1, unrelated);
    assert!(
        high >= 0.80,
        "expected high overlap on same event, got {high}"
    );
    assert_eq!(zero, 0.0);
    assert_eq!(token_overlap("will the is be", "in on of for"), 0.0);
}

#[test]
fn dras_volatility_rejects_degenerate_and_negative_parameters() {
    let valid = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 24.0,
        spread: Some(0.04),
        volume: 1000.0,
    };
    assert_eq!(ActivityProxy::default(), ActivityProxy::SqrtVolume);
    assert!(
        dras_forecast(
            valid,
            DrasConfig {
                k: -1.0,
                ..DrasConfig::default()
            },
            1.0
        )
        .is_none()
    );
    assert!(dras_forecast(valid, DrasConfig::default(), 0.0).is_none());
}
