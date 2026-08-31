//! Educt selection algebra — the deterministic word-index ↔ time layer.
//!
//! The trust boundary of the transcript-store design: LLM passes emit word
//! indices, never timestamps; this module owns the only index→time mapping
//! (`tasks/transcript-store-design.md` §2). Everything here is pure over
//! `&[TimedWord]` — no I/O, no storage, no inference — so every guarantee
//! is checkable by test.
//!
//! Uniform EDL semantics: `keep = (Keep ops in EDL order, or the full
//! transcript when no Keep op exists) minus (Cut ops)`. This one rule makes
//! every EDL shape deterministic — no ops keeps everything (no edits),
//! all-Cut yields the complement (strikethrough / subtractive editing),
//! all-Keep yields the reel (additive, EDL order preserved — the
//! reorderable EDL), and mixed applies strikethroughs inside the reel.

use crate::transcript::TimedWord;
use serde::{Deserialize, Serialize};

/// Inclusive word-index range `[start_word, end_word]` into the immutable
/// `words` array. A valid range always spans at least one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordRange {
    pub start_word: usize,
    pub end_word: usize,
}

impl WordRange {
    /// Construct a range. Ordering (`start_word <= end_word`) is validated
    /// by the algebra functions so an invalid range reaches the
    /// named-error path instead of being silently normalized.
    pub fn new(start_word: usize, end_word: usize) -> Self {
        Self {
            start_word,
            end_word,
        }
    }

    /// Number of words spanned (always >= 1 for a valid range).
    pub fn word_count(&self) -> usize {
        self.end_word - self.start_word + 1
    }
}

/// One EDL operation over a word range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdlOp {
    /// Include the range in the output; reel order = EDL order.
    Keep,
    /// Exclude the range from the output (strikethrough).
    Cut,
}

/// One EDL entry: an operation over a word range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdlEntry {
    pub range: WordRange,
    pub op: EdlOp,
}

/// An edit-decision list over a transcript's `words` array.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edl {
    pub ops: Vec<EdlEntry>,
}

/// Named selection failures — every variant names the broken invariant
/// (reject-with-named-reason; a failing EDL is never partially applied).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectionError {
    /// The transcript carries no word-level timings, so nothing can anchor.
    /// A named degradation — never an empty success.
    #[error("transcript has no word-level timings; selection cannot anchor")]
    NoWordTimings,
    /// A word index is outside the `words` array.
    #[error("word index {index} out of bounds (words.len() == {len})")]
    WordIndexOutOfBounds { index: usize, len: usize },
    /// A range with `start_word > end_word`.
    #[error("reversed word range: start {start_word} > end {end_word}")]
    ReversedRange { start_word: usize, end_word: usize },
    /// Keep ops overlap — the reel would duplicate words.
    #[error("overlapping Keep ops at word {word}: Keep ranges must be disjoint")]
    OverlappingKeepOps { word: usize },
}

/// Map a word range to its media time range `(start_ms, end_ms)`.
///
/// The ONLY index→time mapping in the design — LLM layers never see time.
pub fn word_range_to_time_range(
    words: &[TimedWord],
    range: WordRange,
) -> Result<(u64, u64), SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    if range.start_word > range.end_word {
        return Err(SelectionError::ReversedRange {
            start_word: range.start_word,
            end_word: range.end_word,
        });
    }
    let start = words
        .get(range.start_word)
        .ok_or(SelectionError::WordIndexOutOfBounds {
            index: range.start_word,
            len: words.len(),
        })?;
    let end = words
        .get(range.end_word)
        .ok_or(SelectionError::WordIndexOutOfBounds {
            index: range.end_word,
            len: words.len(),
        })?;
    Ok((start.start_ms, end.end_ms))
}

/// Resolve a transcript passage to every matching word range.
///
/// Exact match over the rendered transcript text (words joined by single
/// spaces), aligned to word boundaries. Ambiguity is surfaced as ALL
/// candidate ranges — never a guess; an empty result means no match (the
/// caller surfaces that as a named status). Query whitespace is normalized
/// to the rendered form (split on whitespace, rejoin with single spaces);
/// queries should quote the rendered form, punctuation included ("world."
/// not "world").
pub fn text_to_word_ranges(words: &[TimedWord], text: &str) -> Vec<WordRange> {
    if words.is_empty() {
        return Vec::new();
    }
    let normalized_query = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized_query.is_empty() {
        return Vec::new();
    }

    // Rendered text with per-word character spans (byte offsets; word
    // texts contain no multi-byte-splitting risks because spans are computed
    // from String::len of whole pushes).
    let mut rendered = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(words.len());
    for word in words {
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        let start = rendered.len();
        rendered.push_str(&word.word);
        spans.push((start, rendered.len()));
    }

    // All occurrences — advance one character past each hit so overlapping
    // occurrences surface as separate candidates — kept only when aligned
    // to word boundaries on both ends.
    let advance = normalized_query.chars().next().map_or(1, char::len_utf8);
    let mut candidates = Vec::new();
    let mut search_from = 0;
    while let Some(relative) = rendered[search_from..].find(&normalized_query) {
        let absolute = search_from + relative;
        let end_char = absolute + normalized_query.len();
        let aligned = spans.iter().any(|span| span.0 == absolute)
            && spans.iter().any(|span| span.1 == end_char);
        if aligned {
            let first = spans.iter().position(|span| span.0 == absolute);
            let last = spans.iter().rposition(|span| span.1 == end_char);
            if let (Some(first), Some(last)) = (first, last) {
                candidates.push(WordRange::new(first, last));
            }
        }
        search_from = absolute + advance;
    }
    candidates
}

/// Compute the keep-ranges of an EDL over a transcript of `words_len` words.
///
/// Validation (reject-with-named-reason, nothing partially applied):
/// - every range in bounds and non-reversed;
/// - Keep ops pairwise disjoint (Cut ops may overlap — union semantics;
///   a Cut outside every Keep range is a harmless no-op).
///
/// An empty transcript is a named degradation (`NoWordTimings`) regardless
/// of ops — no EDL silently succeeds on a transcript that cannot anchor.
pub fn edl_to_keep_ranges(words_len: usize, edl: &Edl) -> Result<Vec<WordRange>, SelectionError> {
    if words_len == 0 {
        return Err(SelectionError::NoWordTimings);
    }
    for entry in &edl.ops {
        let range = entry.range;
        if range.start_word > range.end_word {
            return Err(SelectionError::ReversedRange {
                start_word: range.start_word,
                end_word: range.end_word,
            });
        }
        if range.end_word >= words_len {
            return Err(SelectionError::WordIndexOutOfBounds {
                index: range.end_word,
                len: words_len,
            });
        }
    }

    // Keep ops must be pairwise disjoint. Checked on a sorted copy; the
    // output preserves EDL order (reel order is the reorderable-EDL feature).
    let mut keeps_sorted: Vec<WordRange> = edl
        .ops
        .iter()
        .filter(|entry| entry.op == EdlOp::Keep)
        .map(|entry| entry.range)
        .collect();
    keeps_sorted.sort_by_key(|range| range.start_word);
    for pair in keeps_sorted.windows(2) {
        if pair[1].start_word <= pair[0].end_word {
            return Err(SelectionError::OverlappingKeepOps {
                word: pair[1].start_word,
            });
        }
    }

    let cuts = merged_cuts(
        &edl.ops
            .iter()
            .filter(|entry| entry.op == EdlOp::Cut)
            .map(|entry| entry.range)
            .collect::<Vec<_>>(),
    );

    // Base: the reel (EDL order) when Keep ops exist, else the full
    // transcript (subtractive mode).
    let base: Vec<WordRange> = if keeps_sorted.is_empty() {
        vec![WordRange::new(0, words_len - 1)]
    } else {
        edl.ops
            .iter()
            .filter(|entry| entry.op == EdlOp::Keep)
            .map(|entry| entry.range)
            .collect()
    };

    let mut keep_ranges = Vec::new();
    for range in base {
        keep_ranges.extend(subtract_cuts_from_range(range, &cuts));
    }
    Ok(keep_ranges)
}

/// Map keep-ranges to a render plan: `(start_ms, end_ms)` pairs for
/// `video_clip`/`video_concat`.
///
/// Ranges that are list-adjacent AND word-adjacent (`next.start ==
/// prev.end + 1`) merge into one clip — contiguous media is one clip. A
/// reordered reel (Keep ops out of transcript order) keeps its EDL order:
/// clips emit in list order. Input ranges are expected validated (the EDL
/// path enforces disjointness); overlapping direct input yields overlapping
/// clips, visible in the plan rather than silently merged.
pub fn keep_ranges_to_clip_plan(
    words: &[TimedWord],
    keep_ranges: &[WordRange],
) -> Result<Vec<(u64, u64)>, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let merged = merge_adjacent(keep_ranges);
    let mut plan = Vec::with_capacity(merged.len());
    for range in merged {
        plan.push(word_range_to_time_range(words, range)?);
    }
    Ok(plan)
}

/// EDL → clip plan in one step (the render path: selection → EDL → render).
pub fn edl_to_clip_plan(words: &[TimedWord], edl: &Edl) -> Result<Vec<(u64, u64)>, SelectionError> {
    let keep_ranges = edl_to_keep_ranges(words.len(), edl)?;
    keep_ranges_to_clip_plan(words, &keep_ranges)
}

/// Merge cut ranges into a sorted, disjoint union. Cuts may overlap (union
/// semantics); adjacent cuts merge because the gap between them is empty.
fn merged_cuts(cuts: &[WordRange]) -> Vec<WordRange> {
    let mut sorted: Vec<WordRange> = cuts.to_vec();
    sorted.sort_by_key(|range| range.start_word);
    let mut merged: Vec<WordRange> = Vec::with_capacity(sorted.len());
    for range in sorted {
        match merged.last_mut() {
            Some(last) if range.start_word <= last.end_word.saturating_add(1) => {
                last.end_word = last.end_word.max(range.end_word);
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// Subtract sorted, disjoint cuts from one base range, preserving order.
/// A cut may split the base range into multiple pieces.
fn subtract_cuts_from_range(base: WordRange, cuts: &[WordRange]) -> Vec<WordRange> {
    let mut pieces = Vec::new();
    let mut cursor = base.start_word;
    for cut in cuts {
        if cut.end_word < cursor {
            continue;
        }
        if cut.start_word > base.end_word {
            break;
        }
        let cut_start = cut.start_word.max(cursor);
        if cut_start > cursor {
            pieces.push(WordRange::new(cursor, cut_start - 1));
        }
        cursor = cut.end_word.saturating_add(1);
        if cursor > base.end_word {
            return pieces;
        }
    }
    if cursor <= base.end_word {
        pieces.push(WordRange::new(cursor, base.end_word));
    }
    pieces
}

/// Merge ranges that are list-adjacent and word-adjacent. Reordered input
/// (a reel out of transcript order) does not falsely merge: the adjacency
/// test is positional in the list AND sequential in word index.
fn merge_adjacent(ranges: &[WordRange]) -> Vec<WordRange> {
    let mut merged: Vec<WordRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start_word == last.end_word.saturating_add(1) => {
                last.end_word = range.end_word;
            }
            _ => merged.push(*range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words at 1-second boundaries: word i spans [i*1000, i*1000+500] ms.
    fn words_from_texts(texts: &[&str]) -> Vec<TimedWord> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| TimedWord {
                word: text.to_string(),
                start_ms: index as u64 * 1000,
                end_ms: index as u64 * 1000 + 500,
                confidence: None,
            })
            .collect()
    }

    fn entry(start: usize, end: usize, op: EdlOp) -> EdlEntry {
        EdlEntry {
            range: WordRange::new(start, end),
            op,
        }
    }

    // ── word_range_to_time_range ──────────────────────────────────────────

    #[test]
    fn time_range_maps_exact_boundaries() {
        let words = words_from_texts(&["a", "b", "c", "d"]);
        assert_eq!(
            word_range_to_time_range(&words, WordRange::new(0, 0)),
            Ok((0, 500))
        );
        assert_eq!(
            word_range_to_time_range(&words, WordRange::new(3, 3)),
            Ok((3000, 3500))
        );
        assert_eq!(
            word_range_to_time_range(&words, WordRange::new(1, 2)),
            Ok((1000, 2500))
        );
    }

    #[test]
    fn time_range_rejects_out_of_bounds_with_named_error() {
        let words = words_from_texts(&["a", "b"]);
        assert_eq!(
            word_range_to_time_range(&words, WordRange::new(0, 2)),
            Err(SelectionError::WordIndexOutOfBounds { index: 2, len: 2 })
        );
    }

    #[test]
    fn time_range_rejects_reversed_range() {
        let words = words_from_texts(&["a", "b"]);
        assert_eq!(
            word_range_to_time_range(&words, WordRange::new(1, 0)),
            Err(SelectionError::ReversedRange {
                start_word: 1,
                end_word: 0
            })
        );
    }

    #[test]
    fn time_range_rejects_empty_words_as_named_degradation() {
        assert_eq!(
            word_range_to_time_range(&[], WordRange::new(0, 0)),
            Err(SelectionError::NoWordTimings)
        );
    }

    // ── text_to_word_ranges ───────────────────────────────────────────────

    #[test]
    fn text_selection_finds_single_match() {
        let words = words_from_texts(&["Hello", "world", "said", "the", "agent"]);
        assert_eq!(
            text_to_word_ranges(&words, "said the"),
            vec![WordRange::new(2, 3)]
        );
    }

    #[test]
    fn text_selection_surfaces_all_candidates() {
        let words = words_from_texts(&["the", "plan", "the", "plan", "again"]);
        assert_eq!(
            text_to_word_ranges(&words, "the plan"),
            vec![WordRange::new(0, 1), WordRange::new(2, 3)]
        );
    }

    #[test]
    fn text_selection_no_match_returns_empty() {
        let words = words_from_texts(&["a", "b"]);
        assert!(text_to_word_ranges(&words, "missing passage").is_empty());
    }

    #[test]
    fn text_selection_matches_rendered_punctuation() {
        let words = words_from_texts(&["Hello,", "world."]);
        assert_eq!(
            text_to_word_ranges(&words, "Hello, world."),
            vec![WordRange::new(0, 1)]
        );
        // The unpunctuated form is NOT the rendered form — no match.
        assert!(text_to_word_ranges(&words, "Hello world").is_empty());
    }

    #[test]
    fn text_selection_requires_word_boundaries() {
        let words = words_from_texts(&["Hello", "world"]);
        // Interior substring, not word-aligned — no match.
        assert!(text_to_word_ranges(&words, "lo wor").is_empty());
        assert!(text_to_word_ranges(&words, "Hello wor").is_empty());
    }

    #[test]
    fn text_selection_normalizes_query_whitespace() {
        let words = words_from_texts(&["a", "b", "c"]);
        assert_eq!(
            text_to_word_ranges(&words, "  a   b  "),
            vec![WordRange::new(0, 1)]
        );
    }

    #[test]
    fn text_selection_empty_words_returns_empty() {
        assert!(text_to_word_ranges(&[], "anything").is_empty());
    }

    // ── edl_to_keep_ranges ────────────────────────────────────────────────

    #[test]
    fn empty_edl_keeps_full_transcript() {
        let keeps = edl_to_keep_ranges(5, &Edl::default()).unwrap();
        assert_eq!(keeps, vec![WordRange::new(0, 4)]);
    }

    #[test]
    fn all_cut_edl_yields_complement() {
        let edl = Edl {
            ops: vec![entry(1, 2, EdlOp::Cut)],
        };
        let keeps = edl_to_keep_ranges(5, &edl).unwrap();
        assert_eq!(keeps, vec![WordRange::new(0, 0), WordRange::new(3, 4)]);
    }

    #[test]
    fn complement_tiles_the_transcript() {
        // Property: keep ∪ cut = the full transcript, disjoint.
        let edl = Edl {
            ops: vec![entry(1, 1, EdlOp::Cut), entry(3, 5, EdlOp::Cut)],
        };
        let keeps = edl_to_keep_ranges(8, &edl).unwrap();
        let mut covered: Vec<usize> = Vec::new();
        for range in &keeps {
            for word in range.start_word..=range.end_word {
                covered.push(word);
            }
        }
        for word in [1usize, 3, 4, 5] {
            assert!(!covered.contains(&word), "cut word {word} was kept");
        }
        let mut expected: Vec<usize> = (0..8).collect();
        for word in [1usize, 3, 4, 5] {
            expected.retain(|w| *w != word);
        }
        assert_eq!(covered, expected);
    }

    #[test]
    fn all_keep_edl_preserves_edl_order() {
        // Reordered reel: Keep [3,4] before Keep [0,1] — reel order wins.
        let edl = Edl {
            ops: vec![entry(3, 4, EdlOp::Keep), entry(0, 1, EdlOp::Keep)],
        };
        let keeps = edl_to_keep_ranges(5, &edl).unwrap();
        assert_eq!(keeps, vec![WordRange::new(3, 4), WordRange::new(0, 1)]);
    }

    #[test]
    fn mixed_edl_cuts_within_keeps() {
        // Reel [0,4] with a strikethrough at word 2.
        let edl = Edl {
            ops: vec![entry(0, 4, EdlOp::Keep), entry(2, 2, EdlOp::Cut)],
        };
        let keeps = edl_to_keep_ranges(5, &edl).unwrap();
        assert_eq!(keeps, vec![WordRange::new(0, 1), WordRange::new(3, 4)]);
    }

    #[test]
    fn cut_outside_keeps_is_noop() {
        let edl = Edl {
            ops: vec![entry(0, 1, EdlOp::Keep), entry(3, 4, EdlOp::Cut)],
        };
        let keeps = edl_to_keep_ranges(5, &edl).unwrap();
        assert_eq!(keeps, vec![WordRange::new(0, 1)]);
    }

    #[test]
    fn overlapping_cuts_merge_by_union() {
        let edl = Edl {
            ops: vec![entry(0, 2, EdlOp::Cut), entry(1, 3, EdlOp::Cut)],
        };
        let keeps = edl_to_keep_ranges(5, &edl).unwrap();
        assert_eq!(keeps, vec![WordRange::new(4, 4)]);
    }

    #[test]
    fn overlapping_keep_ops_rejected_with_named_error() {
        let edl = Edl {
            ops: vec![entry(0, 3, EdlOp::Keep), entry(2, 4, EdlOp::Keep)],
        };
        assert_eq!(
            edl_to_keep_ranges(5, &edl),
            Err(SelectionError::OverlappingKeepOps { word: 2 })
        );
    }

    #[test]
    fn out_of_bounds_op_rejected() {
        let edl = Edl {
            ops: vec![entry(0, 5, EdlOp::Cut)],
        };
        assert_eq!(
            edl_to_keep_ranges(5, &edl),
            Err(SelectionError::WordIndexOutOfBounds { index: 5, len: 5 })
        );
    }

    #[test]
    fn reversed_op_rejected() {
        let edl = Edl {
            ops: vec![entry(3, 1, EdlOp::Cut)],
        };
        assert_eq!(
            edl_to_keep_ranges(5, &edl),
            Err(SelectionError::ReversedRange {
                start_word: 3,
                end_word: 1
            })
        );
    }

    #[test]
    fn empty_transcript_is_named_degradation_even_without_ops() {
        assert_eq!(
            edl_to_keep_ranges(0, &Edl::default()),
            Err(SelectionError::NoWordTimings)
        );
    }

    // ── keep_ranges_to_clip_plan / edl_to_clip_plan ───────────────────────

    #[test]
    fn clip_plan_merges_adjacent_ranges() {
        let words = words_from_texts(&["a", "b", "c", "d"]);
        let plan = keep_ranges_to_clip_plan(&words, &[WordRange::new(0, 1), WordRange::new(2, 3)])
            .unwrap();
        assert_eq!(plan, vec![(0, 3500)]);
    }

    #[test]
    fn clip_plan_keeps_nonadjacent_ranges_separate() {
        let words = words_from_texts(&["a", "b", "c", "d"]);
        let plan = keep_ranges_to_clip_plan(&words, &[WordRange::new(0, 0), WordRange::new(2, 3)])
            .unwrap();
        assert_eq!(plan, vec![(0, 500), (2000, 3500)]);
    }

    #[test]
    fn clip_plan_preserves_reel_order() {
        // Reordered reel: [3,3] then [0,0] — clips emit in list order.
        let words = words_from_texts(&["a", "b", "c", "d"]);
        let plan = keep_ranges_to_clip_plan(&words, &[WordRange::new(3, 3), WordRange::new(0, 0)])
            .unwrap();
        assert_eq!(plan, vec![(3000, 3500), (0, 500)]);
    }

    #[test]
    fn clip_plan_empty_ranges_yield_empty_plan() {
        let words = words_from_texts(&["a"]);
        assert_eq!(
            keep_ranges_to_clip_plan(&words, &[]).unwrap(),
            Vec::<(u64, u64)>::new()
        );
    }

    #[test]
    fn clip_plan_empty_words_is_named_degradation() {
        assert_eq!(
            keep_ranges_to_clip_plan(&[], &[WordRange::new(0, 0)]),
            Err(SelectionError::NoWordTimings)
        );
    }

    #[test]
    fn edl_to_clip_plan_composes_both_stages() {
        let words = words_from_texts(&["a", "b", "c", "d", "e"]);
        // Keep [0,4], cut [1,1] → keep [0,0] + [2,4] → two clips: word 1
        // is cut between them, so the ranges are not adjacent and do not
        // merge.
        let edl = Edl {
            ops: vec![entry(0, 4, EdlOp::Keep), entry(1, 1, EdlOp::Cut)],
        };
        assert_eq!(
            edl_to_clip_plan(&words, &edl).unwrap(),
            vec![(0, 500), (2000, 4500)]
        );
    }

    #[test]
    fn subtractive_edl_round_trips_to_complement_clip_plan() {
        let words = words_from_texts(&["a", "b", "c", "d", "e"]);
        // Cut the middle word (index 2): keep [0,1] + [3,4] → clips
        // (0,1500),(3000,4500).
        let edl = Edl {
            ops: vec![entry(2, 2, EdlOp::Cut)],
        };
        assert_eq!(
            edl_to_clip_plan(&words, &edl).unwrap(),
            vec![(0, 1500), (3000, 4500)]
        );
    }
}
