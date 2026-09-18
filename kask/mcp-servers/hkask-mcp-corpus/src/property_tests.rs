//! Property layer for the corpus chunking contract
//! (`kask/docs/reference/testing-protocol.md`). Batch 4 of the propagation
//! plan (`kask/docs/reference/testing-protocol.md`): Phase 0 measured 181
//! value-assert tests and zero property sites in this crate. The properties
//! pin the shared window engine's documented contract (hkask-memory
//! `text_chunking`: "positive overlap repeats exactly overlap_words from the
//! previous suffix; each passage contributes new words and contains at most
//! max_words") through the corpus seam, plus the `[P4]` budget validation.
//! A shrunk counterexample is a finding to report, never a signal to weaken
//! a property.

use crate::helpers::{chunk_structure, chunk_word_bounds, tokens_to_words};
use proptest::prelude::*;

proptest! {
    /// Hypothesis: the word budget is honored — every chunk of any generated
    /// text carries at most `max_words` whitespace-delimited words.
    #[test]
    fn chunk_word_budget_is_honored(
        words in proptest::collection::vec("[a-z]{1,12}", 1..600),
        min_words in 1usize..=8,
        max_words in 9usize..=64,
        overlap_words in 0usize..=7,
    ) {
        let text = words.join(" ");
        let chunks = chunk_structure(&text, "t", min_words, max_words, ".!?", overlap_words)
            .expect("valid budgets chunk");
        for (entity_ref, chunk) in &chunks {
            let count = chunk.split_whitespace().count();
            prop_assert!(
                count <= max_words,
                "chunk {} carries {} words over the {} budget",
                entity_ref,
                count,
                max_words
            );
        }
    }

    /// Hypothesis: positive overlap repeats exactly `overlap_words` from the
    /// previous suffix — consecutive chunks share their boundary words
    /// verbatim, no more and no less.
    #[test]
    fn chunk_overlap_repeats_the_previous_suffix_exactly(
        words in proptest::collection::vec("[a-z]{1,12}", 20..600),
        min_words in 1usize..=8,
        max_words in 9usize..=64,
        overlap_words in 1usize..=7,
    ) {
        let text = words.join(" ");
        let chunks = chunk_structure(&text, "t", min_words, max_words, ".!?", overlap_words)
            .expect("valid budgets chunk");
        for pair in chunks.windows(2) {
            let prev: Vec<&str> = pair[0].1.split_whitespace().collect();
            let next: Vec<&str> = pair[1].1.split_whitespace().collect();
            if prev.len() < overlap_words || next.len() < overlap_words {
                // Boundary-shrunk windows are budgeted by the floor rule;
                // the suffix-prefix check applies whenever both sides carry
                // the full overlap (the engine's floor guarantees this for
                // multi-chunk outputs of non-trivial inputs).
                continue;
            }
            let suffix = &prev[prev.len() - overlap_words..];
            let prefix = &next[..overlap_words];
            prop_assert_eq!(
                suffix, prefix,
                "consecutive chunks must share exactly {} boundary words",
                overlap_words
            );
        }
    }

    /// Hypothesis: no word is dropped and none invented — with zero overlap
    /// the chunks partition the source exactly; with k overlap words the
    /// emitted total is exactly source + k·(chunks − 1).
    #[test]
    fn chunking_conserves_the_source_words(
        words in proptest::collection::vec("[a-z]{1,12}", 1..600),
        max_words in 9usize..=64,
        overlap_words in 0usize..=7,
    ) {
        let text = words.join(" ");
        let chunks = chunk_structure(&text, "t", 1, max_words, ".!?", overlap_words)
            .expect("valid budgets chunk");
        let source = hkask_memory::text_chunking::sanitize_text(&text)
            .split_whitespace()
            .count();
        let emitted: usize = chunks
            .iter()
            .map(|(_, chunk)| chunk.split_whitespace().count())
            .sum();
        let repeats = overlap_words * chunks.len().saturating_sub(1);
        prop_assert_eq!(
            emitted,
            source + repeats,
            "emitted {} must be source {} plus {} overlap repeats",
            emitted,
            source,
            repeats
        );
    }

    /// Hypothesis: every overlapping passage after the first contributes
    /// strictly new words — its word count always exceeds the overlap budget.
    #[test]
    fn every_overlap_passage_contributes_new_words(
        words in proptest::collection::vec("[a-z]{1,12}", 20..600),
        min_words in 1usize..=8,
        max_words in 9usize..=64,
        overlap_words in 1usize..=7,
    ) {
        let text = words.join(" ");
        let chunks = chunk_structure(&text, "t", min_words, max_words, ".!?", overlap_words)
            .expect("valid budgets chunk");
        for (index, (_, chunk)) in chunks.iter().enumerate().skip(1) {
            let count = chunk.split_whitespace().count();
            prop_assert!(
                count > overlap_words,
                "chunk {} carries {} words but overlaps {} — no new material",
                index,
                count,
                overlap_words
            );
        }
    }

    /// Hypothesis: the `[P4]` budget validation is total and invariant —
    /// over the full token/overlap domain the validator never panics, and
    /// every accepted budget satisfies the documented post-conditions:
    /// max_words > 0, 1 ≤ min_words ≤ max_words, overlap_words < max_words.
    #[test]
    fn chunk_word_bounds_validation_is_total_and_invariant(
        max_tokens in 0usize..=4096,
        overlap_tokens in 0usize..=4096,
    ) {
        if let Ok(bounds) = chunk_word_bounds(Some(max_tokens), Some(overlap_tokens)) {
            prop_assert!(bounds.max_words > 0);
            prop_assert!(bounds.min_words >= 1);
            prop_assert!(bounds.min_words <= bounds.max_words);
            prop_assert!(bounds.overlap_words < bounds.max_words);
            prop_assert_eq!(bounds.overlap_words, tokens_to_words(overlap_tokens));
        }
        // Rejection paths are pinned by the unit tests; the property is
        // totality: any combination yields Ok or Err, never a panic.
    }

    /// Hypothesis: the token→word conversion is weakly monotone — a larger
    /// token budget never yields a smaller word budget.
    #[test]
    fn tokens_to_words_is_weakly_monotone(
        pair in (0usize..=4096).prop_flat_map(|smaller| (Just(smaller), smaller..=4096)),
    ) {
        let (smaller, larger) = pair;
        prop_assert!(tokens_to_words(smaller) <= tokens_to_words(larger));
    }
}
