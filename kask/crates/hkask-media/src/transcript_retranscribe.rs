//! Section re-transcription (plan §7 Phase 1c) — the pure layer-building
//! half of `educt_retranscribe_section`: turning a re-transcribed span
//! into a candidate correction edit, and merging it over the latest stored
//! correction layer with the supersession surfaced, never a silent merge.
//! The source words are never touched — the edit is the derived view's
//! input, exactly like the correction pass. The newest correction layer
//! fully defines the working view, so disjoint prior edits are copied
//! verbatim into the new layer (dropping them would silently revert
//! earlier corrections); only edits overlapping the re-transcribed range
//! are superseded, and they ride the response for the operator to see.

use crate::transcript::TimedWord;
use crate::transcript_layers::CorrectionEdit;

/// The rendered replacement text for a re-transcribed span: the STT
/// bundle's trimmed words joined with single spaces (the rendered-form
/// contract — separator-prefixed tokens are already trimmed by
/// `transcript_bundle_from_raw`). An empty span (silence) yields an empty
/// replacement: a legal contraction edit, visible in the response's
/// `words_count`.
pub fn replacement_text(words: &[TimedWord]) -> String {
    words
        .iter()
        .map(|word| word.word.trim())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Merge the re-transcription edit over the latest stored correction
/// layer: prior edits disjoint from the range are copied verbatim (the
/// correction survives), prior edits overlapping the range are superseded
/// by the new STT text and returned for the response to surface — never a
/// silent drop. The returned edit list is disjoint by construction (kept
/// edits are disjoint among themselves; the new edit is disjoint from
/// every kept one).
pub fn merge_over_prior(
    prior: &[CorrectionEdit],
    new_edit: CorrectionEdit,
) -> (Vec<CorrectionEdit>, Vec<CorrectionEdit>) {
    let mut edits = Vec::with_capacity(prior.len() + 1);
    let mut superseded = Vec::new();
    for edit in prior {
        if edit.start_word <= new_edit.end_word && edit.end_word >= new_edit.start_word {
            superseded.push(edit.clone());
        } else {
            edits.push(edit.clone());
        }
    }
    edits.push(new_edit);
    (edits, superseded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start_word: usize, end_word: usize, replacement: &str) -> CorrectionEdit {
        CorrectionEdit {
            start_word,
            end_word,
            replacement: replacement.to_string(),
            reason: "prior".to_string(),
        }
    }

    fn timed_word(word: &str) -> TimedWord {
        TimedWord {
            word: word.to_string(),
            start_ms: 0,
            end_ms: 100,
            confidence: None,
        }
    }

    #[test]
    fn replacement_text_joins_trimmed_words() {
        let words = [
            timed_word(" gamma"),
            timed_word("delta "),
            timed_word("epsilon"),
        ];
        assert_eq!(replacement_text(&words), "gamma delta epsilon");
    }

    #[test]
    fn replacement_text_of_an_empty_span_is_empty() {
        assert_eq!(replacement_text(&[]), "");
        // A whitespace-only STT result trims to the same legal empty edit.
        assert_eq!(replacement_text(&[timed_word("   ")]), "");
    }

    #[test]
    fn merge_keeps_disjoint_edits_verbatim_and_supersedes_overlaps() {
        let prior = vec![
            edit(0, 0, "alpa-fixed"),
            edit(1, 3, "old-mid"),
            edit(6, 7, "tail"),
        ];
        let (edits, superseded) = merge_over_prior(&prior, edit(1, 2, "gamma delta"));
        // The disjoint edits survive verbatim, in prior order; the
        // overlapping one is superseded and listed.
        assert_eq!(
            edits,
            vec![
                edit(0, 0, "alpa-fixed"),
                edit(6, 7, "tail"),
                edit(1, 2, "gamma delta")
            ]
        );
        assert_eq!(superseded, vec![edit(1, 3, "old-mid")]);
    }

    #[test]
    fn merge_supersedes_edits_touching_the_range_from_either_side() {
        // A prior edit starting inside the range.
        let (edits, superseded) = merge_over_prior(&[edit(2, 5, "a")], edit(1, 3, "new"));
        assert_eq!(superseded, vec![edit(2, 5, "a")]);
        assert_eq!(edits, vec![edit(1, 3, "new")]);
        // A prior edit ending inside the range.
        let (_, superseded) = merge_over_prior(&[edit(0, 1, "b")], edit(1, 3, "new"));
        assert_eq!(superseded, vec![edit(0, 1, "b")]);
    }

    #[test]
    fn merge_treats_adjacency_as_disjoint() {
        // Prior [0..=0] and new [1..=2] touch but do not overlap — both
        // survive.
        let (edits, superseded) = merge_over_prior(&[edit(0, 0, "head")], edit(1, 2, "new"));
        assert!(superseded.is_empty());
        assert_eq!(edits, vec![edit(0, 0, "head"), edit(1, 2, "new")]);
    }

    #[test]
    fn merge_without_prior_is_just_the_new_edit() {
        let (edits, superseded) = merge_over_prior(&[], edit(1, 2, "new"));
        assert!(superseded.is_empty());
        assert_eq!(edits, vec![edit(1, 2, "new")]);
    }

    #[test]
    fn merge_where_every_prior_edit_overlaps_supersedes_all() {
        let prior = vec![edit(0, 1, "a"), edit(2, 4, "b")];
        let (edits, superseded) = merge_over_prior(&prior, edit(0, 4, "whole"));
        assert_eq!(superseded.len(), 2);
        assert_eq!(edits, vec![edit(0, 4, "whole")]);
    }
}
