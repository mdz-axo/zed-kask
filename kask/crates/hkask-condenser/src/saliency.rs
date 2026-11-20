//! Saliency scoring — word-frequency computation shared with `WordRankAlgorithm`.
//!
//! `word_frequencies` is the canonical word-frequency computation that
//! `WordRankAlgorithm` delegates to instead of maintaining a copy.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
