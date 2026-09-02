//! Event ↔ market entity resolution (T4c).
//!
//! The load-bearing operation of the data service: given a scenario or
//! forecast question, find the market(s) about the *same underlying event*.
//! Scoring is deterministic over extracted features — no LLM judgment — so
//! consumers can refuse low-confidence matches mechanically (the same
//! epistemic posture as `reliability_tier`).

use crate::types::MarketRecord;

/// Confidence tiers for a candidate match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchConfidence {
    High,
    Medium,
    Low,
}

/// A candidate market with its match assessment.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MatchCandidate {
    pub market: MarketRecord,
    pub match_confidence: MatchConfidence,
    /// Deterministic score in [0,1] the tier was derived from.
    pub score: f64,
    /// Which features drove the score — provenance for the consumer.
    pub match_basis: MatchBasis,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MatchBasis {
    /// Jaccard similarity over normalized significant tokens.
    pub token_overlap: f64,
    /// Absolute days between query deadline and market deadline, if both known.
    pub deadline_delta_days: Option<f64>,
}

/// English stopwords plus domain-generic verbs that carry no entity signal.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "in", "on", "of", "for", "to", "by", "at", "or", "and", "is", "be", "will",
    "what", "who", "whom", "which", "that", "this", "these", "those", "it", "its", "happen",
    "occur", "take", "place", "there", "their", "they", "them", "his", "her", "before", "after",
    "during", "above", "below", "over", "under", "than", "then", "when", "how", "many", "much",
    "does", "do", "did", "any", "some", "no", "not", "yes", "win", "become", "get", "make", "made",
    "out", "up", "down", "new", "next", "end", "year",
];

/// Normalize into a set of significant lowercase tokens.
fn significant_tokens(text: &str) -> std::collections::HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Jaccard similarity over significant tokens of two questions.
pub fn token_overlap(a: &str, b: &str) -> f64 {
    let ta = significant_tokens(a);
    let tb = significant_tokens(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    intersection / union
}

/// Extract a deadline signal from the query: prefer an explicit RFC3339/ISO
/// date; otherwise a bare 4-digit year → the *midpoint* of that year (Jul 1).
/// End-of-year is wrong: "the January 2028 meeting" means Jan 2028, and a
/// Dec-31 pivot would put it ~340 days from the market's actual deadline for
/// the *same* event. Mid-year minimizes worst-case error for coarse queries.
pub fn extract_deadline(query: &str) -> Option<chrono::NaiveDate> {
    // ISO date token, e.g. 2027-04-28.
    for token in query.split(|c: char| !c.is_alphanumeric() && c != '-') {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(token, "%Y-%m-%d") {
            return Some(date);
        }
    }
    // Bare year token, e.g. "2028" → mid-year pivot.
    for token in query.split(|c: char| !c.is_alphanumeric()) {
        if token.len() == 4
            && let Ok(year) = token.parse::<i32>()
            && (2020..=2100).contains(&year)
        {
            return chrono::NaiveDate::from_ymd_opt(year, 7, 1);
        }
    }
    None
}

/// Days between the query deadline and a market deadline string (RFC3339).
fn deadline_delta_days(query_deadline: chrono::NaiveDate, market_deadline: &str) -> Option<f64> {
    let market = chrono::DateTime::parse_from_rfc3339(market_deadline).ok()?;
    Some((market.date_naive() - query_deadline).num_days().abs() as f64)
}

/// Score a candidate market against a query. Deterministic:
/// `score = token_overlap * deadline_factor`. The deadline factor only
/// discriminates between *competing* candidates when the query explicitly
/// names a date/year that differs from the market's deadline — a market's
/// own question (which embeds its date) must score a perfect match against
/// itself, and a query with no date signal is never penalized. So:
/// factor = 1.0 when no explicit query date, or the dates align (<=45 days,
/// generous because "December meeting" vs "Dec 8" are the same cycle);
/// decays to 0.1 beyond that.
pub fn score_match(
    query: &str,
    query_deadline: Option<chrono::NaiveDate>,
    market: &MarketRecord,
) -> MatchCandidate {
    let overlap = token_overlap(query, &market.question);
    let delta = query_deadline.and_then(|d| deadline_delta_days(d, &market.deadline));
    // Tolerance tracks the query's own extraction precision: an ISO-date
    // query (day precision) penalizes mismatches past 45 days; a year-only
    // query (±6-month precision) only penalizes different *cycles* (>1 year).
    let day_precision = query_deadline.is_some()
        && query
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .any(|t| chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d").is_ok());
    let tolerance = if day_precision { 45.0 } else { 366.0 };
    let deadline_factor = match delta {
        Some(d) if d <= tolerance => 1.0,
        Some(d) => (1.0 - (d - tolerance) / 300.0).max(0.1),
        None => 1.0,
    };
    let score = overlap * deadline_factor;
    let match_confidence = if score >= 0.65 {
        MatchConfidence::High
    } else if score >= 0.45 {
        MatchConfidence::Medium
    } else {
        MatchConfidence::Low
    };
    MatchCandidate {
        market: market.clone(),
        match_confidence,
        score,
        match_basis: MatchBasis {
            token_overlap: overlap,
            deadline_delta_days: delta,
        },
    }
}

/// Rank candidates by score, highest first.
pub fn rank_matches(query: &str, candidates: &[MarketRecord]) -> Vec<MatchCandidate> {
    let query_deadline = extract_deadline(query);
    let mut scored: Vec<MatchCandidate> = candidates
        .iter()
        .map(|m| score_match(query, query_deadline, m))
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid `MarketRecord` via serde — the matcher only reads
    /// `question` and `deadline`, but the record has no `Default` and the
    /// provider constructors require upstream JSON.
    fn test_market(question: &str, deadline: &str) -> MarketRecord {
        serde_json::from_value(serde_json::json!({
            "source": "kalshi",
            "event_id": "ev-test",
            "market_id": "mkt-test",
            "question": question,
            "description": "",
            "category": "politics",
            "series": "KXTEST",
            "deadline": deadline,
            "time_to_maturity": null,
            "probability": 0.5,
            "probability_method": "midpoint",
            "spread": null,
            "volume": 1000.0,
            "volume_grain": "market",
            "liquidity": null,
            "open_interest": null,
            "last_update": "2026-01-01T00:00:00Z",
            "volatility": {
                "realized_variance": null,
                "structural_flag": "None",
                "interpretation": ""
            },
            "status": "open",
            "resolved_outcome": null,
            "resolution_source": "test",
            "calibration": {
                "brier": null,
                "domain_bias": null,
                "bias_source": "test",
                "sample_size": 0,
                "stale": true
            },
            "reliability_tier": "high",
            "ontology": {
                "process": { "type": "test", "stage": "test", "probability_role": "test" },
                "state": {
                    "identifier": "test",
                    "title": "test",
                    "description": "test",
                    "temporal": "test",
                    "provenance": "test"
                },
                "mapping_version": 1
            }
        }))
        .expect("test market record deserializes")
    }

    #[test]
    fn token_overlap_identical_questions_score_one() {
        assert!((token_overlap("Will the Fed cut rates", "Will the Fed cut rates") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn token_overlap_ignores_stopwords() {
        // "will" and "in" are stopwords — adding them must not change the overlap.
        assert!(
            (token_overlap("Fed cut rates in December", "Fed cut rates December") - 1.0).abs() < 1e-9
        );
    }

    #[test]
    fn token_overlap_disjoint_questions_score_zero() {
        assert!(token_overlap("Fed cut rates", "Champions league winner") < 1e-9);
    }

    #[test]
    fn extract_deadline_prefers_iso_date() {
        let date = extract_deadline("Will X happen by 2027-04-28 or later?").expect("iso date");
        assert_eq!(date, chrono::NaiveDate::from_ymd_opt(2027, 4, 28).expect("valid date"));
    }

    #[test]
    fn extract_deadline_bare_year_uses_midyear_pivot() {
        // Mid-year, not end-of-year: "the January 2028 meeting" must not be
        // ~340 days from a Dec-31 pivot of the same year.
        let date = extract_deadline("Who wins the 2028 election?").expect("year");
        assert_eq!(date, chrono::NaiveDate::from_ymd_opt(2028, 7, 1).expect("valid date"));
    }

    #[test]
    fn extract_deadline_absent_is_none() {
        assert!(extract_deadline("Will the sun rise tomorrow?").is_none());
    }

    #[test]
    fn market_scores_perfectly_against_its_own_question() {
        let market = test_market("Will the Fed cut rates in December", "2026-12-15T00:00:00Z");
        let candidate = score_match("Will the Fed cut rates in December", None, &market);
        assert!((candidate.score - 1.0).abs() < 1e-9);
        assert_eq!(candidate.match_confidence, MatchConfidence::High);
    }

    #[test]
    fn deadline_mismatch_decays_score_when_query_names_a_date() {
        // The query names 2027-04-28 (day precision); the market resolves
        // 2027-12-31 — 247 days apart, past the 45-day tolerance, so the
        // deadline factor must reduce the score below the raw token overlap.
        let market = test_market("Will X happen", "2027-12-31T00:00:00Z");
        let query = "Will X happen by 2027-04-28";
        let query_deadline = extract_deadline(query).expect("query names a date");
        let candidate = score_match(query, Some(query_deadline), &market);
        assert!(
            candidate.score < candidate.match_basis.token_overlap,
            "deadline mismatch must reduce the score below raw overlap"
        );
        assert!(candidate.match_basis.deadline_delta_days.is_some());
    }

    #[test]
    fn rank_matches_orders_highest_first() {
        let exact = test_market("Will the Fed cut rates in December", "2026-12-15T00:00:00Z");
        let unrelated = test_market("Champions league winner", "2026-12-15T00:00:00Z");
        let ranked = rank_matches("Will the Fed cut rates in December", &[unrelated, exact]);
        assert_eq!(
            ranked[0].market.question, "Will the Fed cut rates in December",
            "the matching market must rank first"
        );
        assert!(ranked[0].score > ranked[1].score);
    }
}
