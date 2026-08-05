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
    "a", "an", "the", "in", "on", "of", "for", "to", "by", "at", "or", "and", "is", "be",
    "will", "what", "who", "whom", "which", "that", "this", "these", "those", "it", "its",
    "happen", "occur", "take", "place", "there", "their", "they", "them", "his", "her",
    "before", "after", "during", "above", "below", "over", "under", "than", "then", "when",
    "how", "many", "much", "does", "do", "did", "any", "some", "no", "not", "yes", "win",
    "become", "get", "make", "made", "out", "up", "down", "new", "next", "end", "year",
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
/// date; otherwise a bare 4-digit year → end of that year (conservative —
/// a "…in 2028?" query means "by end of 2028").
pub fn extract_deadline(query: &str) -> Option<chrono::NaiveDate> {
    // ISO date token, e.g. 2027-04-28.
    for token in query.split(|c: char| !c.is_alphanumeric() && c != '-') {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(token, "%Y-%m-%d") {
            return Some(date);
        }
    }
    // Bare year token, e.g. "2028".
    for token in query.split(|c: char| !c.is_alphanumeric()) {
        if token.len() == 4
            && let Ok(year) = token.parse::<i32>()
            && (2020..=2100).contains(&year)
        {
            return chrono::NaiveDate::from_ymd_opt(year, 12, 31);
        }
    }
    None
}

/// Days between the query deadline and a market deadline string (RFC3339).
fn deadline_delta_days(
    query_deadline: chrono::NaiveDate,
    market_deadline: &str,
) -> Option<f64> {
    let market = chrono::DateTime::parse_from_rfc3339(market_deadline).ok()?;
    Some((market.date_naive() - query_deadline).num_days().abs() as f64)
}

/// Score a candidate market against a query. Deterministic:
/// `score = token_overlap * deadline_factor`, where deadline_factor is 1.0
/// when the query carries no deadline signal or the deadlines align, and
/// decays to 0.25 at ≥30 days of mismatch. Deadline only *penalizes*
/// mismatches — a market's own question (which may not name a date) must
/// score 1.0, and absence of a date signal is not evidence against a match.
pub fn score_match(
    query: &str,
    query_deadline: Option<chrono::NaiveDate>,
    market: &MarketRecord,
) -> MatchCandidate {
    let overlap = token_overlap(query, &market.question);
    let delta = query_deadline.and_then(|d| deadline_delta_days(d, &market.deadline));
    let deadline_factor = match delta {
        Some(d) if d <= 3.0 => 1.0,
        Some(d) => (1.0 - (d - 3.0) / 27.0 * 0.75).max(0.25),
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
pub fn rank_matches(
    query: &str,
    candidates: &[MarketRecord],
) -> Vec<MatchCandidate> {
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
