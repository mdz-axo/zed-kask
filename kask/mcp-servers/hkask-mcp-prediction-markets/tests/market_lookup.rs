//! Contract tests for the annotated MarketRecord (integration report §4).

use hkask_mcp_prediction_markets::provider_kalshi::{KalshiEvent, KalshiMarketsResponse};
use hkask_mcp_prediction_markets::provider_polymarket::GammaEvent;
use hkask_mcp_prediction_markets::types::{
    self, MarketRecord, MarketStatus, ReliabilityTier, Source, StructuralFlag,
};

const KALSHI_FIXTURE: &str = include_str!("fixtures/kalshi_markets.json");
const KALSHI_EVENTS_FIXTURE: &str = include_str!("fixtures/kalshi_events.json");
const POLY_FIXTURE: &str = include_str!("fixtures/polymarket_events.json");

fn now() -> chrono::DateTime<chrono::Utc> {
    // Fixed reference time before all fixture deadlines.
    chrono::DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
        .expect("valid")
        .with_timezone(&chrono::Utc)
}

fn stale_calibration_for(category: &str) -> types::Calibration {
    types::calibration_for(None, category)
}

fn kalshi_record() -> MarketRecord {
    let markets: KalshiMarketsResponse =
        serde_json::from_str(KALSHI_FIXTURE).expect("parses");
    let events: hkask_mcp_prediction_markets::provider_kalshi::KalshiEventsResponse =
        serde_json::from_str(KALSHI_EVENTS_FIXTURE).expect("parses");
    let market = &markets.markets[0];
    let event = events
        .events
        .iter()
        .find(|e| e.event_ticker == market.event_ticker);
    let category = event.map(|e| e.category.clone()).unwrap_or_default();
    MarketRecord::from_kalshi(market, event, stale_calibration_for(&category), &now())
        .expect("record builds")
}

#[test]
fn record_never_carries_bare_probability() {
    let record = kalshi_record();
    let value = serde_json::to_value(&record).expect("serializes");
    // Every probability is paired with the annotation fields — the contract's
    // core guardrail.
    for field in [
        "probability",
        "spread",
        "volume",
        "last_update",
        "calibration",
        "reliability_tier",
        "volatility",
        "ontology",
    ] {
        assert!(value.get(field).is_some(), "missing contract field {field}");
    }
}

#[test]
fn kalshi_probability_is_bid_ask_midpoint() {
    let markets: KalshiMarketsResponse =
        serde_json::from_str(KALSHI_FIXTURE).expect("parses");
    let market = &markets.markets[0];
    let record = kalshi_record();
    let expected = market.yes_midpoint().expect("midpoint");
    assert!((record.probability - expected).abs() < 1e-12);
    assert!(matches!(
        record.probability_method,
        types::ProbabilityMethod::Midpoint
    ));
}

#[test]
fn ontology_block_has_both_axes_and_shared_version() {
    let record = kalshi_record();
    assert_eq!(record.ontology.process.r#type, "pko:ProcedureExecution");
    assert_eq!(record.ontology.process.probability_role, "pko:StepExecution.output");
    assert!(record.ontology.state.identifier.starts_with("kalshi:"));
    assert_eq!(record.ontology.state.provenance, "kalshi_exchange");
    assert_eq!(
        record.ontology.mapping_version,
        hkask_mcp_prediction_markets::ontology::MAPPING_VERSION
    );
}

#[test]
fn politics_bias_guardrail() {
    // 2602.19520: politics/elections categories must carry the bias flag.
    assert_eq!(types::domain_bias_for("Elections"), Some("underconfident"));
    assert_eq!(types::domain_bias_for("politics"), Some("underconfident"));
    assert_eq!(types::domain_bias_for("Sports"), None);
}

#[test]
fn near_deadline_coinflip_flags() {
    assert_eq!(
        types::structural_flag(0.52, Some(3.0)),
        StructuralFlag::NearDeadlineAndCoinflip
    );
    assert_eq!(
        types::structural_flag(0.85, Some(30.0)),
        StructuralFlag::None
    );
    assert_eq!(
        types::structural_flag(0.50, Some(60.0)),
        StructuralFlag::NearCoinflip
    );
    assert_eq!(
        types::structural_flag(0.90, Some(2.0)),
        StructuralFlag::NearDeadline
    );
}

#[test]
fn thin_volume_is_low_reliability() {
    assert_eq!(
        types::reliability_tier(500.0, Some(0.02)),
        ReliabilityTier::Low
    );
    assert_eq!(
        types::reliability_tier(2_000_000.0, Some(0.01)),
        ReliabilityTier::High
    );
    assert_eq!(
        types::reliability_tier(100_000.0, Some(0.20)),
        ReliabilityTier::Low
    );
}

#[test]
fn calibration_stale_is_not_zero_brier() {
    // Cybernetic invariant: an unmeasured calibration is stale, never brier 0.
    let record = kalshi_record();
    assert!(record.calibration.stale);
    assert!(record.calibration.brier.is_none());
}

#[test]
fn polymarket_record_builds_with_uma_provenance() {
    let events: Vec<GammaEvent> = serde_json::from_str(POLY_FIXTURE).expect("parses");
    let event = &events[0];
    let market = &event.markets[0];
    let event_tags: Vec<String> = event.tags.iter().map(|t| t.label.clone()).collect();
    let bucket = event_tags.first().cloned().unwrap_or_default();
    let record = MarketRecord::from_polymarket(
        market,
        &event.id,
        &event.slug,
        event.volume,
        event.liquidity,
        &event_tags,
        stale_calibration_for(&bucket),
        &now(),
    )
    .expect("record builds");
    assert!(matches!(record.source, Source::Polymarket));
    assert_eq!(record.resolution_source, "uma_oracle");
    assert!(record.ontology.state.identifier.starts_with("polymarket:"));
    // Fixture market is resolved (T0).
    if matches!(record.status, MarketStatus::Resolved) {
        assert!(record.resolved_outcome.is_some());
        assert_eq!(record.ontology.process.stage, "settlement");
    }
}

#[test]
fn market_lookup_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketLookupRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

#[test]
fn kalshi_event_type_used() {
    // Keep the event pairing path honest: records with a matched event carry
    // its category; unmatched carry empty.
    let markets: KalshiMarketsResponse =
        serde_json::from_str(KALSHI_FIXTURE).expect("parses");
    let market = &markets.markets[0];
    let no_event: Option<&KalshiEvent> = None;
    let record =
        MarketRecord::from_kalshi(market, no_event, stale_calibration_for(""), &now())
            .expect("builds");
    assert!(record.category.is_empty());
}

// ── T4b: market_ontology_map ───────────────────────────────────────────────

#[test]
fn ontology_map_document_matches_per_record_block() {
    // Anti-drift invariant: the tool document and the per-record ontology
    // block are generated from the same constants — change one, both change.
    let doc = hkask_mcp_prediction_markets::ontology::mapping_document();
    let record = kalshi_record();
    assert_eq!(
        doc["mapping_version"],
        serde_json::json!(record.ontology.mapping_version)
    );
    assert_eq!(
        doc["process_axis"]["record_type"],
        serde_json::json!(record.ontology.process.r#type)
    );
    assert_eq!(
        doc["process_axis"]["probability_role"],
        serde_json::json!(record.ontology.process.probability_role)
    );
    // The record's stage must come from the documented lifecycle stages.
    let stages = doc["process_axis"]["lifecycle_stages"]
        .as_array()
        .expect("stages array");
    assert!(
        stages
            .iter()
            .any(|s| s.as_str() == Some(record.ontology.process.stage.as_ref())),
        "record stage {} not in documented lifecycle stages",
        record.ontology.process.stage
    );
}

#[test]
fn ontology_map_request_schema_has_no_boolean_positions() {
    let schema =
        schemars::schema_for!(hkask_mcp_prediction_markets::MarketOntologyMapRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

// ── Bug-fix regression tests (review 2026-08-05) ──────────────────────────

#[test]
fn b1_fifty_fifty_resolution_never_fabricates_outcome() {
    // arXiv:2604.20421 "Unknown/50-50" resolutions settle both legs at 0.50;
    // the old >= 0.5 threshold reported a fabricated outcome.
    let mut market = polymarket_fixture_market();
    market.uma_resolution_status = "resolved".to_string();
    market.closed = true;
    market.outcome_prices = "[\"0.5\", \"0.5\"]".to_string();
    let record = MarketRecord::from_polymarket(
        &market, "e", "s", 1e6, 1e5, &[],
        stale_calibration_for(""), &now(),
    )
    .expect("builds");
    assert!(matches!(record.status, MarketStatus::Resolved));
    assert_eq!(record.resolved_outcome, None, "50-50 must not fabricate");
}

#[test]
fn b1_definitive_resolution_records_outcome() {
    let mut market = polymarket_fixture_market();
    market.uma_resolution_status = "resolved".to_string();
    market.closed = true;
    market.outcome_prices = "[\"1\", \"0\"]".to_string();
    let record = MarketRecord::from_polymarket(
        &market, "e", "s", 1e6, 1e5, &[],
        stale_calibration_for(""), &now(),
    )
    .expect("builds");
    assert_eq!(record.resolved_outcome, Some(true));
}

fn polymarket_fixture_market() -> hkask_mcp_prediction_markets::provider_polymarket::GammaMarket {
    let events: Vec<GammaEvent> = serde_json::from_str(POLY_FIXTURE).expect("parses");
    events[0].markets[0].clone()
}

#[test]
fn b4_polymarket_category_derived_from_tags_fires_bias_guardrail() {
    let events: Vec<GammaEvent> = serde_json::from_str(POLY_FIXTURE).expect("parses");
    let event = &events[0];
    let market = polymarket_fixture_market();
    let tags = vec!["Politics".to_string(), "Elections".to_string()];
    let record = MarketRecord::from_polymarket(
        &market, &event.id, &event.slug, event.volume, event.liquidity, &tags,
        stale_calibration_for("Politics"), &now(),
    )
    .expect("builds");
    assert_eq!(record.category, "Politics");
    assert_eq!(
        record.calibration.domain_bias.as_deref(),
        Some("underconfident"),
        "politics guardrail must fire for Polymarket records"
    );
}

#[test]
fn b3_volume_grain_is_explicit() {
    let kalshi = kalshi_record();
    assert!(matches!(kalshi.volume_grain, types::VolumeGrain::Market));
    let record = MarketRecord::from_polymarket(
        &polymarket_fixture_market(), "e", "s", 1e6, 1e5, &[],
        stale_calibration_for(""), &now(),
    )
    .expect("builds");
    assert!(matches!(record.volume_grain, types::VolumeGrain::Event));
}

#[test]
fn b2_calibration_reading_reaches_record() {
    // The loop-closure invariant: a populated store's reading must appear on
    // the record (not the hardcoded stale block).
    use hkask_mcp_prediction_markets::calibration::{
        CalibrationStore, ResolvedObservation, read_calibration,
    };
    let mut store = CalibrationStore::new();
    store.record("Elections", ResolvedObservation { probability: 0.9, outcome: true });
    let reading = read_calibration(&store, "Elections");
    let block = types::calibration_for(Some(&reading), "Elections");
    assert!(!block.stale, "measured bucket must not be stale");
    assert!(block.brier.is_some());
    assert_eq!(block.sample_size, 1);
    // And the empty-bucket path stays honest.
    let stale_block = types::calibration_for(None, "Elections");
    assert!(stale_block.stale);
    assert_eq!(stale_block.brier, None);
}
