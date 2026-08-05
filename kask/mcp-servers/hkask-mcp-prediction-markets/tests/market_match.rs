//! Tests for the event ↔ market matcher (T4c).

use hkask_mcp_prediction_markets::matcher::{
    self, MatchConfidence, extract_deadline, rank_matches, score_match, token_overlap,
};
use hkask_mcp_prediction_markets::provider_kalshi::KalshiMarketsResponse;
use hkask_mcp_prediction_markets::types::MarketRecord;

const KALSHI_FIXTURE: &str = include_str!("fixtures/kalshi_markets.json");

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
        .expect("valid")
        .with_timezone(&chrono::Utc)
}

fn kalshi_records() -> Vec<MarketRecord> {
    let markets: KalshiMarketsResponse =
        serde_json::from_str(KALSHI_FIXTURE).expect("parses");
    markets
        .markets
        .iter()
        .filter_map(|m| MarketRecord::from_kalshi(m, None, &now()))
        .collect()
}

#[test]
fn token_overlap_is_jaccard_over_significant_tokens() {
    let a = token_overlap(
        "Will the Fed cut rates in December 2027?",
        "Will the Fed cut rates at the December 2027 meeting?",
    );
    assert!(a > 0.5, "high overlap expected, got {a}");
    let b = token_overlap(
        "Will the Fed cut rates?",
        "Who wins the 2028 presidential election?",
    );
    assert!(b < 0.15, "low overlap expected, got {b}");
    assert_eq!(token_overlap("the will win", "a an the"), 0.0);
}

#[test]
fn deadline_extraction_prefers_iso_then_year() {
    assert_eq!(
        extract_deadline("What happens on 2027-12-08?"),
        chrono::NaiveDate::from_ymd_opt(2027, 12, 8)
    );
    assert_eq!(
        extract_deadline("Who wins in 2028?"),
        chrono::NaiveDate::from_ymd_opt(2028, 7, 1)
    );
    assert_eq!(extract_deadline("no date here"), None);
}

#[test]
fn own_question_matches_at_high_confidence() {
    let records = kalshi_records();
    let market = &records[0];
    // Query = the market's own question → high-confidence match.
    let candidate = score_match(&market.question, extract_deadline(&market.question), market);
    assert!(
        matches!(candidate.match_confidence, MatchConfidence::High),
        "own question should match high, got {:?} (score {:.2}, basis {:?})",
        candidate.match_confidence,
        candidate.score,
        candidate.match_basis
    );
}

#[test]
fn same_entities_different_cycle_scores_lower() {
    let records = kalshi_records();
    let market = &records[0];
    // The fixture markets are Fed meeting decisions; a query naming a
    // different year with the same entities must not match high.
    let wrong_cycle = "Will the upper bound of the federal funds rate be above 4.25% following the Fed's meeting in 2031?";
    let candidate = score_match(wrong_cycle, extract_deadline(wrong_cycle), market);
    assert!(
        !matches!(candidate.match_confidence, MatchConfidence::High),
        "mismatched cycle must not match high, got {:?} (score {:.2}, delta {:?})",
        candidate.match_confidence,
        candidate.score,
        candidate.match_basis.deadline_delta_days
    );
    // And it must score strictly worse than the aligned-cycle query.
    let aligned = score_match(&market.question, extract_deadline(&market.question), market);
    assert!(candidate.score < aligned.score);
}

#[test]
fn rank_matches_orders_by_score() {
    let records = kalshi_records();
    let query = &records[0].question.clone();
    let ranked = rank_matches(query, &records);
    assert!(!ranked.is_empty());
    assert!(
        ranked.windows(2).all(|w| w[0].score >= w[1].score),
        "ranked descending"
    );
    assert_eq!(ranked[0].market.market_id, records[0].market_id);
}

#[test]
fn confidence_tiers_are_refusable_mechanically() {
    // The downstream contract (T8): low-confidence matches are refusable.
    // Pin the thresholds so a tuning change is a deliberate, reviewed act.
    let records = kalshi_records();
    let market = &records[0];
    let high = score_match(&market.question, extract_deadline(&market.question), market);
    assert!(high.score >= 0.65);
    let noise = score_match("Completely unrelated question about chess 1997", None, market);
    assert!(matches!(noise.match_confidence, MatchConfidence::Low));
    assert!(noise.score < 0.45);
}

#[test]
fn match_request_schema_has_no_boolean_positions() {
    let schema = schemars::schema_for!(hkask_mcp_prediction_markets::MarketMatchRequest);
    let value = serde_json::to_value(&schema).expect("serializes");
    let positions = hkask_mcp_server::find_boolean_schema_positions(&value);
    assert!(positions.is_empty(), "bare booleans: {positions:?}");
}

#[test]
fn matcher_module_exposes_public_api() {
    // Guard against accidental privatization of the T8 consumer surface.
    let _f: fn(&str, &[MarketRecord]) -> Vec<matcher::MatchCandidate> = rank_matches;
}
