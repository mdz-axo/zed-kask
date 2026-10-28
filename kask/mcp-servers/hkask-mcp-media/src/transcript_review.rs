//! Review-flag derivation over stored transcript layers (plan §7 Phase 1a,
//! `educt-speaker-identity-plan-2026-10-27.md`).
//!
//! Deterministic and LLM-free: paragraph blocks from the latest paragraph
//! layer, scored by the latest speaker layer's stored attribution
//! confidences (the only pass that stores confidence today). Every
//! unscoreable state is named — a missing layer or an uncovered block is
//! `no-confidence-recorded`, never a fabricated green.

use crate::transcript_layers::TranscriptLayer;
use crate::transcript_store::LayerRecord;

/// Speaker-attribution confidence at or above which a paragraph block is
/// NOT flagged. A starting point from the task card; the 1a reversal
/// condition (recalibrate, or gate shipping on measured flag quality if
/// pass confidences prove uninformative) applies.
pub const REVIEW_CONFIDENCE_THRESHOLD: f64 = 0.75;

/// Review status of one paragraph block. Serialized kebab-case so the wire
/// string matches the CSV column and the pane label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewStatus {
    /// Minimum overlapping speaker-span confidence is below the threshold.
    Flagged,
    /// Every overlapping span scored at or above the threshold.
    Ok,
    /// No speaker span covers this block — no confidence recorded.
    NoConfidenceRecorded,
}

impl ReviewStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flagged => "flagged",
            Self::Ok => "ok",
            Self::NoConfidenceRecorded => "no-confidence-recorded",
        }
    }
}

/// One paragraph block's derived review flag.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReviewBlockFlag {
    pub start_word: usize,
    pub end_word: usize,
    /// Minimum confidence among the speaker spans overlapping the block;
    /// `None` iff no span covers it (status `no-confidence-recorded`).
    pub min_confidence: Option<f64>,
    pub status: ReviewStatus,
}

/// The derived review view over one transcript's stored layers.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReviewFlags {
    pub threshold: f64,
    /// The paragraph layer the blocks came from; `None` when none exists.
    pub paragraph_layer_id: Option<String>,
    /// The speaker layer the confidences came from; `None` when none exists.
    pub speaker_layer_id: Option<String>,
    pub blocks: Vec<ReviewBlockFlag>,
    /// Named degradation when blocks or scores are unavailable.
    pub note: Option<String>,
}

/// Derive review flags from a transcript's stored layers: blocks from the
/// latest paragraph layer, scores from the latest speaker layer ("latest"
/// = last by `created_at`, matching `list_layers` ordering). Both anchor
/// source word indices; block ranges are inclusive.
pub fn derive_review_flags(words_count: usize, records: &[LayerRecord]) -> ReviewFlags {
    let paragraph = records.iter().rev().find_map(|record| match &record.layer {
        TranscriptLayer::Paragraph(layer) => Some((record.id.as_str(), layer)),
        _ => None,
    });
    let speaker = records.iter().rev().find_map(|record| match &record.layer {
        TranscriptLayer::Speaker(layer) => Some((record.id.as_str(), layer)),
        _ => None,
    });
    let Some((paragraph_layer_id, paragraph_layer)) = paragraph else {
        return ReviewFlags {
            threshold: REVIEW_CONFIDENCE_THRESHOLD,
            paragraph_layer_id: None,
            speaker_layer_id: speaker.map(|(id, _)| id.to_string()),
            blocks: Vec::new(),
            note: Some(
                "no paragraph layer recorded — review flags attach to \
                 paragraph blocks; run educt_paragraph_pass"
                    .to_string(),
            ),
        };
    };
    let blocks: Vec<ReviewBlockFlag> = paragraph_bounds(words_count, &paragraph_layer.breaks_after)
        .into_iter()
        .map(|(start_word, end_word)| {
            let min_confidence = speaker.and_then(|(_, layer)| {
                layer
                    .spans
                    .iter()
                    .filter(|span| span.start_word <= end_word && span.end_word >= start_word)
                    .map(|span| span.confidence)
                    .reduce(f64::min)
            });
            let status = match min_confidence {
                Some(confidence) if confidence < REVIEW_CONFIDENCE_THRESHOLD => {
                    ReviewStatus::Flagged
                }
                Some(_) => ReviewStatus::Ok,
                None => ReviewStatus::NoConfidenceRecorded,
            };
            ReviewBlockFlag {
                start_word,
                end_word,
                min_confidence,
                status,
            }
        })
        .collect();
    ReviewFlags {
        threshold: REVIEW_CONFIDENCE_THRESHOLD,
        paragraph_layer_id: Some(paragraph_layer_id.to_string()),
        speaker_layer_id: speaker.map(|(id, _)| id.to_string()),
        note: if speaker.is_none() {
            Some(
                "no speaker layer recorded — the speaker pass stores the \
                 attribution confidences; run educt_speaker_pass"
                    .to_string(),
            )
        } else {
            None
        },
        blocks,
    }
}

/// Inclusive paragraph-block bounds over source word indices from the
/// paragraph layer's `breaks_after` points (sorted and deduplicated
/// defensively; validation already bounds each index below `words_count`,
/// so the out-of-range break is unreachable insurance against a mismatched
/// caller-supplied `words_count`).
fn paragraph_bounds(words_count: usize, breaks_after: &[usize]) -> Vec<(usize, usize)> {
    let mut breaks = breaks_after.to_vec();
    breaks.sort_unstable();
    breaks.dedup();
    let mut bounds = Vec::new();
    let mut start = 0usize;
    for break_after in breaks {
        if break_after >= words_count {
            break;
        }
        bounds.push((start, break_after));
        start = break_after + 1;
    }
    if start < words_count {
        bounds.push((start, words_count - 1));
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_layers::{LayerProvenance, ParagraphLayer, SpeakerLayer, SpeakerSpan};

    fn provenance(created_at: &str) -> LayerProvenance {
        LayerProvenance {
            model: "test-model".to_string(),
            prompt_template: "test".to_string(),
            created_at: created_at.to_string(),
        }
    }

    fn record(id: &str, created_at: &str, layer: TranscriptLayer) -> LayerRecord {
        LayerRecord {
            id: id.to_string(),
            transcript_id: "t-1".to_string(),
            layer,
            created_at: created_at.to_string(),
        }
    }

    fn paragraph(id: &str, created_at: &str, breaks_after: Vec<usize>) -> LayerRecord {
        record(
            id,
            created_at,
            TranscriptLayer::Paragraph(ParagraphLayer {
                provenance: provenance(created_at),
                breaks_after,
            }),
        )
    }

    fn speaker(id: &str, created_at: &str, spans: Vec<SpeakerSpan>) -> LayerRecord {
        record(
            id,
            created_at,
            TranscriptLayer::Speaker(SpeakerLayer {
                provenance: provenance(created_at),
                spans,
            }),
        )
    }

    fn span(start_word: usize, end_word: usize, confidence: f64) -> SpeakerSpan {
        SpeakerSpan {
            start_word,
            end_word,
            speaker: "speaker-1".to_string(),
            confidence,
        }
    }

    #[test]
    fn flags_below_threshold_and_blocks_from_breaks() {
        let records = [
            paragraph("p-1", "2026-10-27T00:00:01Z", vec![1]),
            speaker(
                "s-1",
                "2026-10-27T00:00:02Z",
                vec![span(0, 1, 0.9), span(2, 3, 0.5)],
            ),
        ];
        let review = derive_review_flags(4, &records);
        assert_eq!(review.threshold, REVIEW_CONFIDENCE_THRESHOLD);
        assert_eq!(review.note, None);
        assert_eq!(review.paragraph_layer_id.as_deref(), Some("p-1"));
        assert_eq!(review.speaker_layer_id.as_deref(), Some("s-1"));
        assert_eq!(
            review.blocks,
            vec![
                ReviewBlockFlag {
                    start_word: 0,
                    end_word: 1,
                    min_confidence: Some(0.9),
                    status: ReviewStatus::Ok,
                },
                ReviewBlockFlag {
                    start_word: 2,
                    end_word: 3,
                    min_confidence: Some(0.5),
                    status: ReviewStatus::Flagged,
                },
            ]
        );
    }

    #[test]
    fn exact_threshold_is_ok_not_flagged() {
        let records = [
            paragraph("p-1", "2026-10-27T00:00:01Z", Vec::new()),
            speaker("s-1", "2026-10-27T00:00:02Z", vec![span(0, 1, 0.75)]),
        ];
        let review = derive_review_flags(2, &records);
        assert_eq!(review.blocks[0].status, ReviewStatus::Ok);
        assert_eq!(review.blocks[0].min_confidence, Some(0.75));
    }

    #[test]
    fn min_across_overlapping_spans_wins() {
        let records = [
            paragraph("p-1", "2026-10-27T00:00:01Z", Vec::new()),
            speaker(
                "s-1",
                "2026-10-27T00:00:02Z",
                vec![span(0, 3, 0.8), span(1, 2, 0.4)],
            ),
        ];
        let review = derive_review_flags(4, &records);
        assert_eq!(review.blocks[0].min_confidence, Some(0.4));
        assert_eq!(review.blocks[0].status, ReviewStatus::Flagged);
    }

    #[test]
    fn uncovered_block_is_no_confidence_recorded() {
        let records = [
            paragraph("p-1", "2026-10-27T00:00:01Z", vec![1]),
            speaker("s-1", "2026-10-27T00:00:02Z", vec![span(0, 1, 0.9)]),
        ];
        let review = derive_review_flags(4, &records);
        assert_eq!(review.blocks[1].min_confidence, None);
        assert_eq!(review.blocks[1].status, ReviewStatus::NoConfidenceRecorded);
        // The speaker layer exists, so no whole-transcript note — the gap
        // is per-block, never a fabricated green.
        assert_eq!(review.note, None);
    }

    #[test]
    fn latest_layers_of_each_kind_win() {
        let records = [
            paragraph("p-old", "2026-10-27T00:00:01Z", vec![0]),
            speaker("s-old", "2026-10-27T00:00:02Z", vec![span(0, 1, 0.1)]),
            paragraph("p-new", "2026-10-27T00:00:03Z", vec![2]),
            speaker("s-new", "2026-10-27T00:00:04Z", vec![span(0, 3, 0.9)]),
        ];
        let review = derive_review_flags(4, &records);
        assert_eq!(review.paragraph_layer_id.as_deref(), Some("p-new"));
        assert_eq!(review.speaker_layer_id.as_deref(), Some("s-new"));
        // Blocks follow p-new's break, scores follow s-new's span.
        assert_eq!(
            review
                .blocks
                .iter()
                .map(|block| (block.start_word, block.end_word))
                .collect::<Vec<_>>(),
            vec![(0, 2), (3, 3)]
        );
        assert_eq!(review.blocks[0].min_confidence, Some(0.9));
    }

    #[test]
    fn no_paragraph_layer_is_a_named_note() {
        let records = [speaker(
            "s-1",
            "2026-10-27T00:00:01Z",
            vec![span(0, 1, 0.9)],
        )];
        let review = derive_review_flags(2, &records);
        assert!(review.blocks.is_empty());
        assert_eq!(review.paragraph_layer_id, None);
        assert_eq!(review.speaker_layer_id.as_deref(), Some("s-1"));
        let note = review.note.expect("missing-paragraph state is named");
        assert!(note.contains("educt_paragraph_pass"), "note: {note}");
    }

    #[test]
    fn no_speaker_layer_scores_nothing() {
        let records = [paragraph("p-1", "2026-10-27T00:00:01Z", vec![1])];
        let review = derive_review_flags(4, &records);
        assert_eq!(review.blocks.len(), 2);
        assert!(review.blocks.iter().all(|block| {
            block.status == ReviewStatus::NoConfidenceRecorded && block.min_confidence.is_none()
        }));
        let note = review.note.expect("missing-speaker state is named");
        assert!(note.contains("educt_speaker_pass"), "note: {note}");
    }

    #[test]
    fn break_at_last_word_ends_the_transcript() {
        let records = [paragraph("p-1", "2026-10-27T00:00:01Z", vec![3])];
        let review = derive_review_flags(4, &records);
        assert_eq!(
            review
                .blocks
                .iter()
                .map(|block| (block.start_word, block.end_word))
                .collect::<Vec<_>>(),
            vec![(0, 3)]
        );
    }

    #[test]
    fn empty_words_yield_no_blocks() {
        let records = [paragraph("p-1", "2026-10-27T00:00:01Z", Vec::new())];
        let review = derive_review_flags(0, &records);
        assert!(review.blocks.is_empty());
    }

    #[test]
    fn status_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ReviewStatus::NoConfidenceRecorded).expect("serializes"),
            "\"no-confidence-recorded\""
        );
        assert_eq!(
            serde_json::to_string(&ReviewStatus::Flagged).expect("serializes"),
            "\"flagged\""
        );
        assert_eq!(
            serde_json::to_string(&ReviewStatus::Ok).expect("serializes"),
            "\"ok\""
        );
    }
}
