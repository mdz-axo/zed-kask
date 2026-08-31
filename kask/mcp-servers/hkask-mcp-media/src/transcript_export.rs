//! Educt transcript exports — deterministic projections of the stored
//! transcript to shareable and ingestable formats (slice 7):
//! - **SRT captions** from `TimedWord` — the immutable ground truth
//!   (segments stay a derived view, design doc §1.3).
//! - **A CSV of every stored highlight** — with time ranges via the
//!   selection algebra (the only index→time mapping).
//! - **The rendered transcript text** for corpus ingestion —
//!   repository-wide semantic search by composition (decision 8: media
//!   owns the artifacts, corpus owns the index). A corpus hit on the
//!   rendered text maps back to word ranges via `text_to_word_ranges`,
//!   closing the cross-recording loop: search finds the passage, the
//!   selection algebra turns it into a media range.

use crate::transcript::TimedWord;
use crate::transcript_layers::TranscriptLayer;
use crate::transcript_select::{SelectionError, WordRange, word_range_to_time_range};
use crate::transcript_store::LayerRecord;

/// Maximum words per SRT cue — sentence punctuation splits first; the cap
/// bounds unpunctuated runs.
const MAX_CUE_WORDS: usize = 15;

/// Build SRT captions from the word timings: cues split at
/// sentence-ending punctuation, capped at `MAX_CUE_WORDS` words per cue.
/// A transcript without word timings is a named degradation — captions
/// cannot anchor.
pub fn srt_from_words(words: &[TimedWord]) -> Result<String, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let mut srt = String::new();
    let mut cue_index: usize = 1;
    let mut cue_words: Vec<&TimedWord> = Vec::new();
    for word in words {
        cue_words.push(word);
        let ends_sentence =
            word.word.ends_with('.') || word.word.ends_with('!') || word.word.ends_with('?');
        if ends_sentence || cue_words.len() >= MAX_CUE_WORDS {
            append_cue(&mut srt, cue_index, &cue_words);
            cue_index += 1;
            cue_words.clear();
        }
    }
    if !cue_words.is_empty() {
        append_cue(&mut srt, cue_index, &cue_words);
    }
    Ok(srt)
}

/// Append one numbered SRT cue block (index, timestamp line, text, blank
/// line). Every cue ends with exactly one blank line, so the cue count
/// equals the `"\n\n"` count in the result.
fn append_cue(srt: &mut String, index: usize, cue_words: &[&TimedWord]) {
    let (Some(first), Some(last)) = (cue_words.first(), cue_words.last()) else {
        return;
    };
    let text = cue_words
        .iter()
        .map(|word| word.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    srt.push_str(&index.to_string());
    srt.push('\n');
    srt.push_str(&format!(
        "{} --> {}\n",
        srt_timestamp(first.start_ms),
        srt_timestamp(last.end_ms)
    ));
    srt.push_str(&text);
    srt.push_str("\n\n");
}

/// `HH:MM:SS,mmm` — the SRT timestamp form.
fn srt_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// Build a CSV of every highlight across the given highlight layers:
/// `layer_id, model, start_word, end_word, start_ms, end_ms, label, note`.
/// Time ranges come from the selection algebra; a validated layer can
/// never fail it, but the error is named if the impossible happens.
/// Non-highlight records are skipped defensively (the caller filters).
pub fn highlights_csv(
    words: &[TimedWord],
    records: &[LayerRecord],
) -> Result<String, SelectionError> {
    let mut csv = String::from("layer_id,model,start_word,end_word,start_ms,end_ms,label,note\n");
    for record in records {
        let TranscriptLayer::Highlight(highlight) = &record.layer else {
            continue;
        };
        for entry in &highlight.highlights {
            let (start_ms, end_ms) =
                word_range_to_time_range(words, WordRange::new(entry.start_word, entry.end_word))?;
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                csv_escape(&record.id),
                csv_escape(&highlight.provenance.model),
                entry.start_word,
                entry.end_word,
                start_ms,
                end_ms,
                csv_escape(&entry.label),
                csv_escape(&entry.note),
            ));
        }
    }
    Ok(csv)
}

/// RFC 4180 field escaping: quote when the field contains a comma, quote,
/// or newline; double embedded quotes.
fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_layers::{HighlightEntry, HighlightLayer, LayerProvenance};

    fn timed_words(texts: &[&str]) -> Vec<TimedWord> {
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

    #[test]
    fn srt_splits_cues_at_sentence_punctuation() {
        let words = timed_words(&["Hello", "world.", "Next", "one."]);
        let srt = srt_from_words(&words).expect("srt builds");
        // Two cues: "Hello world." and "Next one."
        assert_eq!(srt.matches("\n\n").count(), 2);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:01,500\nHello world.\n"));
        assert!(srt.contains("2\n00:00:02,000 --> 00:00:03,500\nNext one.\n"));
    }

    #[test]
    fn srt_caps_unpunctuated_runs() {
        let words = timed_words(&[
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
        ]);
        let srt = srt_from_words(&words).expect("srt builds");
        // 16 words, cap 15 → two cues (15 + 1).
        assert_eq!(srt.matches("\n\n").count(), 2);
    }

    #[test]
    fn srt_timestamp_format() {
        assert_eq!(srt_timestamp(0), "00:00:00,000");
        assert_eq!(srt_timestamp(2_500), "00:00:02,500");
        assert_eq!(srt_timestamp(3_723_450), "01:02:03,450");
    }

    #[test]
    fn srt_without_word_timings_is_a_named_degradation() {
        assert_eq!(srt_from_words(&[]), Err(SelectionError::NoWordTimings));
    }

    #[test]
    fn highlights_csv_rows_carry_time_ranges_and_provenance() {
        let words = timed_words(&["alpha", "beta"]);
        let record = LayerRecord {
            id: "layer-1".to_string(),
            transcript_id: "t-1".to_string(),
            layer: TranscriptLayer::Highlight(HighlightLayer {
                provenance: LayerProvenance {
                    model: "test-model".to_string(),
                    prompt_template: "test".to_string(),
                    created_at: "2026-08-31T00:00:00Z".to_string(),
                },
                highlights: vec![HighlightEntry {
                    start_word: 0,
                    end_word: 1,
                    label: "key, \"argument\"".to_string(),
                    note: "the curve".to_string(),
                }],
            }),
            created_at: "2026-08-31T00:00:00Z".to_string(),
        };
        let csv = highlights_csv(&words, &[record]).expect("csv builds");
        assert!(csv.starts_with("layer_id,model,start_word,end_word,start_ms,end_ms,label,note\n"));
        // The label's comma and quotes are RFC 4180-escaped.
        assert!(csv.contains("\"key, \"\"argument\"\"\""));
        // The time range is the algebra's mapping of words [0,1].
        assert!(csv.contains("layer-1,test-model,0,1,0,1500"));
    }

    #[test]
    fn highlights_csv_skips_non_highlight_records_defensively() {
        let words = timed_words(&["alpha"]);
        let record = LayerRecord {
            id: "layer-1".to_string(),
            transcript_id: "t-1".to_string(),
            layer: TranscriptLayer::Paragraph(crate::transcript_layers::ParagraphLayer {
                provenance: LayerProvenance {
                    model: "m".to_string(),
                    prompt_template: "t".to_string(),
                    created_at: "2026-08-31T00:00:00Z".to_string(),
                },
                breaks_after: vec![0],
            }),
            created_at: "2026-08-31T00:00:00Z".to_string(),
        };
        let csv = highlights_csv(&words, &[record]).expect("csv builds");
        assert_eq!(csv.lines().count(), 1, "header only");
    }
}
