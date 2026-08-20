//! Saliency scoring — how relevant is text to an agent's persona or memory?
//!
//! Extracted from `WordRankAlgorithm` so saliency is cleanly callable without
//! running the full compression pipeline. Three public functions:
//!
//! - `score_against_persona(text, persona_keywords)` → word-overlap score 0.0–1.0
//! - `extract_query_words(text)` → words to query memory stores with
//! - `score_memory_results(total_results)` → 0.0–1.0 from memory query hit count
//!
//! `word_frequencies` is the canonical word-frequency computation shared with
//! `WordRankAlgorithm` (which delegates here instead of duplicating).
//!
//! Memory saliency is split: the domain crate owns the scoring formula and
//! query-word extraction (pure, testable), the MCP server owns the I/O
//! (querying episodic/semantic stores). This keeps the domain crate pure
//! while making the scoring logic reusable and testable.

use std::collections::HashMap;

/// Compute normalized word frequencies for words with length > 2.
///
/// Returns a map of lowercase word → frequency (0.0–1.0). Empty map if no
/// qualifying words. This is the canonical implementation — `WordRankAlgorithm`
/// delegates here instead of maintaining a copy.
pub(crate) fn word_frequencies(words: &[&str]) -> HashMap<String, f64> {
    let mut freq: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;
    for word in words {
        let w = word.to_lowercase();
        if w.len() > 2 {
            *freq.entry(w).or_insert(0) += 1;
            total += 1;
        }
    }
    if total == 0 {
        return HashMap::new();
    }
    freq.into_iter()
        .map(|(k, v)| (k, v as f64 / total as f64))
        .collect()
}

/// Score how salient `text` is against a persona's keyword set.
///
/// Computes TF-IDF-like word overlap: what fraction of the persona's
/// keywords appear in the text, weighted by their frequency in the text.
/// Returns 0.0 (no overlap) to 1.0 (all keywords present with high weight).
/// Returns 0.5 (neutral) if keywords or text is empty.
///
/// `persona_keywords` should include the agent's charter description terms,
/// capability names, responsibility phrases, and invariant traits.
pub fn score_against_persona(text: &str, persona_keywords: &[&str]) -> f64 {
    if persona_keywords.is_empty() || text.is_empty() {
        return 0.5;
    }
    let text_lower = text.to_lowercase();
    let words: Vec<&str> = text_lower.split_whitespace().collect();
    let freq = word_frequencies(&words);

    let mut total_weight: f64 = 0.0;
    let mut hits: usize = 0;

    for keyword in persona_keywords {
        let kw = keyword.to_lowercase();
        if text_lower.contains(&kw) {
            hits += 1;
            // Weight by how prominently the keyword appears (TF component)
            let weight = freq.get(kw.as_str()).copied().unwrap_or(0.1);
            total_weight += weight;
        }
    }

    if hits == 0 {
        return 0.0;
    }

    // Normalize: hits/total filtered through the average frequency weight
    let coverage = hits as f64 / persona_keywords.len() as f64;
    let avg_weight = total_weight / hits as f64;

    // Blend coverage and weight — both matter
    (coverage * 0.6 + avg_weight * 0.4).min(1.0)
}

/// Extract query words from text for memory saliency search.
///
/// Splits on whitespace, filters to words with length > 3 (avoids noise from
/// short tokens like "the", "a", "is"), and takes at most 5 words (limits
/// memory query cost). Returns borrowed slices — callers do not need owned
/// strings for the query.
pub fn extract_query_words(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .collect()
}

/// Score memory saliency from total query result count.
///
/// Returns 0.2 if no results (text doesn't trigger memory), or
/// `0.5 + count * 0.15` capped at 1.0 if results exist (text is salient).
/// The neutral 0.5 for "no store available" is handled by the caller —
/// this function only scores the result count.
pub fn score_memory_results(total_results: usize) -> f64 {
    if total_results > 0 {
        (0.5 + total_results as f64 * 0.15).min(1.0)
    } else {
        0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_keywords_returns_neutral() {
        assert_eq!(score_against_persona("hello world", &[]), 0.5);
    }

    #[test]
    fn empty_text_returns_neutral() {
        assert_eq!(score_against_persona("", &["test"]), 0.5);
    }

    #[test]
    fn high_overlap_scores_high() {
        let keywords = &["monitor", "alert", "escalation", "curator"];
        let text =
            "The curator monitors alert channels and handles escalation of critical findings";
        let score = score_against_persona(text, keywords);
        assert!(score > 0.5, "expected high score, got {score}");
    }

    #[test]
    fn no_overlap_scores_zero() {
        let keywords = &["monitor", "alert", "escalation"];
        let text = "The weather is nice today and I had a good lunch";
        let score = score_against_persona(text, keywords);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn partial_overlap_scores_between() {
        let keywords = &["monitor", "alert", "escalation", "deploy"];
        let text = "The monitor detected an alert in the system";
        let score = score_against_persona(text, keywords);
        assert!(
            score > 0.0 && score < 1.0,
            "expected partial score, got {score}"
        );
    }

    #[test]
    fn word_frequencies_filters_short_words() {
        let freq = word_frequencies(&["hello", "world", "ok", "a", "test"]);
        // "ok" (len 2) and "a" (len 1) should be filtered out
        assert_eq!(freq.len(), 3);
        assert!(freq.contains_key("hello"));
        assert!(freq.contains_key("world"));
        assert!(freq.contains_key("test"));
        assert!(!freq.contains_key("ok"));
    }

    #[test]
    fn word_frequencies_normalizes() {
        let freq = word_frequencies(&["hello", "hello", "world"]);
        assert_eq!(freq["hello"], 2.0 / 3.0);
        assert_eq!(freq["world"], 1.0 / 3.0);
    }

    #[test]
    fn word_frequencies_empty_returns_empty() {
        let freq = word_frequencies(&["a", "b", "ok"]);
        assert!(freq.is_empty());
    }

    #[test]
    fn extract_query_words_filters_short() {
        let words = extract_query_words("the quick brown fox jumps over lazy dogs");
        // "the" (3), "fox" (3), "over" (4) → "the" and "fox" filtered, but "over" kept
        // Actually len > 3 means > 3, so "the" (3) is filtered, "fox" (3) filtered,
        // "over" (4) kept, "lazy" (4) kept, "dogs" (4) kept, "quick" (5) kept, "brown" (5) kept
        // take(5) → first 5: quick, brown, over, lazy, dogs
        assert_eq!(words.len(), 5);
        assert!(words.iter().all(|w| w.len() > 3));
    }

    #[test]
    fn extract_query_words_empty_text() {
        let words = extract_query_words("");
        assert!(words.is_empty());
    }

    #[test]
    fn score_memory_results_zero_returns_low() {
        assert_eq!(score_memory_results(0), 0.2);
    }

    #[test]
    fn score_memory_results_positive_returns_above_neutral() {
        assert_eq!(score_memory_results(1), 0.65);
        assert_eq!(score_memory_results(3), 0.95);
    }

    #[test]
    fn score_memory_results_caps_at_one() {
        // 4 results: 0.5 + 4*0.15 = 1.1 → capped at 1.0
        assert_eq!(score_memory_results(4), 1.0);
        assert_eq!(score_memory_results(100), 1.0);
    }
}
