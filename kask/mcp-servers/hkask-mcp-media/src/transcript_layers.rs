//! Educt transcript layers — typed, provenance-carrying annotation records
//! over the immutable `words` array.
//!
//! Every layer anchors to word indices, never timestamps (the
//! word-index-anchoring thesis, `tasks/transcript-store-design.md` §2), and
//! carries `LayerProvenance` (model, prompt template, created_at) — Magna
//! Carta: system types are provenance-aware. Validation is deterministic
//! and total: a layer that fails is rejected with the named failing
//! invariant and is never partially applied.
//!
//! Layer semantics (what each kind asserts over `words`):
//! - `SpeakerLayer`: who spoke which word range — spans must tile disjointly.
//! - `ParagraphLayer`: discourse structure — word indices to break after.
//! - `CorrectionLayer`: proposed text replacements over word ranges —
//!   edits must be disjoint; `words` stays immutable, corrected text is a
//!   derived view.
//! - `HighlightLayer`: labeled selections (Reduct's highlights) — overlap
//!   allowed, they are independent annotations, not a partition.
//! - `EdlLayer`: an edit-decision list — validation delegates to the
//!   slice-1 selection algebra (Keep ops disjoint; Cut ops union).

use crate::transcript_select::{Edl, SelectionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Provenance envelope carried by every layer — which model, which prompt
/// template, when. Layers are additive and versioned; provenance is how
/// a stored layer stays auditable and replayable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerProvenance {
    /// The model that produced the layer (e.g. an inference-catalog label).
    pub model: String,
    /// The prompt template that produced it (template name or inline hash).
    pub prompt_template: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// One speaker span: `speaker` said the words `[start_word, end_word]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeakerSpan {
    pub start_word: usize,
    pub end_word: usize,
    pub speaker: String,
    /// Confidence in the attribution, 0.0–1.0.
    pub confidence: f64,
}

/// Speaker attribution over a transcript (diarization-lite when produced by
/// a text-cue pass; exact when produced by an audio-capable model — the
/// provenance records which).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeakerLayer {
    pub provenance: LayerProvenance,
    pub spans: Vec<SpeakerSpan>,
}

/// Paragraph breaks: word indices after which a break occurs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParagraphLayer {
    pub provenance: LayerProvenance,
    pub breaks_after: Vec<usize>,
}

/// One proposed correction: replace the text of words
/// `[start_word, end_word]` with `replacement`. Timings are never touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CorrectionEdit {
    pub start_word: usize,
    pub end_word: usize,
    pub replacement: String,
    pub reason: String,
}

/// Transcript corrections — proposals over word ranges; applying them
/// produces a derived `full_text` view while `words` stays immutable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CorrectionLayer {
    pub provenance: LayerProvenance,
    pub edits: Vec<CorrectionEdit>,
}

/// One highlight: a labeled selection of words `[start_word, end_word]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HighlightEntry {
    pub start_word: usize,
    pub end_word: usize,
    pub label: String,
    pub note: String,
}

/// Highlights with labels (Reduct's highlights + labels).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HighlightLayer {
    pub provenance: LayerProvenance,
    pub highlights: Vec<HighlightEntry>,
}

/// An edit-decision list over the transcript — the Reel. Ops reuse the
/// slice-1 types; validation delegates to the selection algebra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EdlLayer {
    pub provenance: LayerProvenance,
    pub ops: Vec<crate::transcript_select::EdlEntry>,
}

/// A transcript layer of any kind — the storage and tool-surface unit.
/// Internally tagged by `kind` so one JSON value round-trips through the
/// store and the `educt_store_layer` tool input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptLayer {
    Speaker(SpeakerLayer),
    Paragraph(ParagraphLayer),
    Correction(CorrectionLayer),
    Highlight(HighlightLayer),
    Edl(EdlLayer),
}

/// Named layer-validation failures — every variant names the broken
/// invariant (reject-with-named-reason; a failing layer is never partially
/// applied or stored).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LayerValidationError {
    /// The transcript carries no word-level timings — layers cannot anchor.
    /// A named degradation, never an empty success.
    #[error("transcript has no word-level timings; layers cannot anchor")]
    NoWordTimings,
    #[error("word index {index} out of bounds (words_count == {len})")]
    WordIndexOutOfBounds { index: usize, len: usize },
    #[error("reversed word range: start {start_word} > end {end_word}")]
    ReversedRange { start_word: usize, end_word: usize },
    /// Speaker spans and correction edits must be disjoint; overlapping
    /// ranges are ambiguous.
    #[error("overlapping ranges at word {word}: ranges must be disjoint for this layer kind")]
    OverlappingRanges { word: usize },
    #[error("empty speaker label at word {start_word}")]
    EmptySpeakerLabel { start_word: usize },
    #[error("speaker confidence {confidence} out of range [0.0, 1.0]")]
    ConfidenceOutOfRange { confidence: f64 },
    #[error("EDL validation failed: {0}")]
    EdlSelection(SelectionError),
}

impl TranscriptLayer {
    /// The storage kind tag (`speaker` | `paragraph` | `correction` |
    /// `highlight` | `edl`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Speaker(_) => "speaker",
            Self::Paragraph(_) => "paragraph",
            Self::Correction(_) => "correction",
            Self::Highlight(_) => "highlight",
            Self::Edl(_) => "edl",
        }
    }

    /// The provenance envelope of the inner layer.
    pub fn provenance(&self) -> &LayerProvenance {
        match self {
            Self::Speaker(layer) => &layer.provenance,
            Self::Paragraph(layer) => &layer.provenance,
            Self::Correction(layer) => &layer.provenance,
            Self::Highlight(layer) => &layer.provenance,
            Self::Edl(layer) => &layer.provenance,
        }
    }

    /// Deterministic validation against the transcript's word count.
    ///
    /// Reject-with-named-invariant; nothing is partially applied. An empty
    /// transcript (`words_count == 0`) rejects every layer — no layer pass
    /// silently succeeds on a transcript that cannot anchor.
    pub fn validate(&self, words_count: usize) -> Result<(), LayerValidationError> {
        if words_count == 0 {
            return Err(LayerValidationError::NoWordTimings);
        }
        match self {
            Self::Speaker(layer) => {
                for span in &layer.spans {
                    check_range(span.start_word, span.end_word, words_count)?;
                    if span.speaker.trim().is_empty() {
                        return Err(LayerValidationError::EmptySpeakerLabel {
                            start_word: span.start_word,
                        });
                    }
                    if !(0.0..=1.0).contains(&span.confidence) {
                        return Err(LayerValidationError::ConfidenceOutOfRange {
                            confidence: span.confidence,
                        });
                    }
                }
                check_disjoint(
                    &layer
                        .spans
                        .iter()
                        .map(|span| (span.start_word, span.end_word))
                        .collect::<Vec<_>>(),
                )?;
            }
            Self::Paragraph(layer) => {
                // A break must reference an existing word; duplicates are
                // tolerated (set semantics — a repeated break is a no-op,
                // not an ambiguity).
                for index in &layer.breaks_after {
                    if *index >= words_count {
                        return Err(LayerValidationError::WordIndexOutOfBounds {
                            index: *index,
                            len: words_count,
                        });
                    }
                }
            }
            Self::Correction(layer) => {
                for edit in &layer.edits {
                    check_range(edit.start_word, edit.end_word, words_count)?;
                }
                check_disjoint(
                    &layer
                        .edits
                        .iter()
                        .map(|edit| (edit.start_word, edit.end_word))
                        .collect::<Vec<_>>(),
                )?;
            }
            Self::Highlight(layer) => {
                // Overlap allowed: highlights are independent annotations,
                // not a partition of the transcript.
                for highlight in &layer.highlights {
                    check_range(highlight.start_word, highlight.end_word, words_count)?;
                }
            }
            Self::Edl(layer) => {
                // Delegate to the slice-1 algebra: bounds, non-reversed,
                // Keep ops disjoint (Cut ops may overlap — union).
                let edl = Edl {
                    ops: layer.ops.clone(),
                };
                if let Err(error) = crate::transcript_select::edl_to_keep_ranges(words_count, &edl)
                {
                    return Err(LayerValidationError::EdlSelection(error));
                }
            }
        }
        Ok(())
    }
}

/// Range preconditions shared by every anchored layer kind.
fn check_range(
    start_word: usize,
    end_word: usize,
    words_count: usize,
) -> Result<(), LayerValidationError> {
    if start_word > end_word {
        return Err(LayerValidationError::ReversedRange {
            start_word,
            end_word,
        });
    }
    if end_word >= words_count {
        return Err(LayerValidationError::WordIndexOutOfBounds {
            index: end_word,
            len: words_count,
        });
    }
    Ok(())
}

/// Disjointness over (start, end) pairs — sorted-copy check so caller order
/// is preserved (the output of validation never reorders anything).
fn check_disjoint(ranges: &[(usize, usize)]) -> Result<(), LayerValidationError> {
    let mut sorted: Vec<(usize, usize)> = ranges.to_vec();
    sorted.sort();
    for pair in sorted.windows(2) {
        if pair[1].0 <= pair[0].1 {
            return Err(LayerValidationError::OverlappingRanges { word: pair[1].0 });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_select::{EdlEntry, EdlOp, WordRange};

    fn provenance() -> LayerProvenance {
        LayerProvenance {
            model: "test-model".to_string(),
            prompt_template: "test-template".to_string(),
            created_at: "2026-08-30T00:00:00Z".to_string(),
        }
    }

    fn speaker_span(start: usize, end: usize) -> SpeakerSpan {
        SpeakerSpan {
            start_word: start,
            end_word: end,
            speaker: "speaker-1".to_string(),
            confidence: 0.9,
        }
    }

    // ── SpeakerLayer ──────────────────────────────────────────────────────

    #[test]
    fn speaker_layer_validates_disjoint_spans() {
        let layer = TranscriptLayer::Speaker(SpeakerLayer {
            provenance: provenance(),
            spans: vec![speaker_span(0, 2), speaker_span(3, 4)],
        });
        assert_eq!(layer.validate(5), Ok(()));
    }

    #[test]
    fn speaker_layer_rejects_overlapping_spans() {
        let layer = TranscriptLayer::Speaker(SpeakerLayer {
            provenance: provenance(),
            spans: vec![speaker_span(0, 3), speaker_span(2, 4)],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::OverlappingRanges { word: 2 })
        );
    }

    #[test]
    fn speaker_layer_rejects_empty_label() {
        let layer = TranscriptLayer::Speaker(SpeakerLayer {
            provenance: provenance(),
            spans: vec![SpeakerSpan {
                start_word: 0,
                end_word: 1,
                speaker: "   ".to_string(),
                confidence: 0.9,
            }],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::EmptySpeakerLabel { start_word: 0 })
        );
    }

    #[test]
    fn speaker_layer_rejects_out_of_range_confidence() {
        let layer = TranscriptLayer::Speaker(SpeakerLayer {
            provenance: provenance(),
            spans: vec![SpeakerSpan {
                start_word: 0,
                end_word: 1,
                speaker: "speaker-1".to_string(),
                confidence: 1.5,
            }],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::ConfidenceOutOfRange { confidence: 1.5 })
        );
    }

    // ── ParagraphLayer ────────────────────────────────────────────────────

    #[test]
    fn paragraph_layer_validates_in_bounds_breaks() {
        let layer = TranscriptLayer::Paragraph(ParagraphLayer {
            provenance: provenance(),
            breaks_after: vec![2, 4],
        });
        assert_eq!(layer.validate(5), Ok(()));
    }

    #[test]
    fn paragraph_layer_rejects_break_past_the_last_word() {
        let layer = TranscriptLayer::Paragraph(ParagraphLayer {
            provenance: provenance(),
            breaks_after: vec![5],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::WordIndexOutOfBounds { index: 5, len: 5 })
        );
    }

    // ── CorrectionLayer ────────────────────────────────────────────────────

    #[test]
    fn correction_layer_validates_disjoint_edits() {
        let layer = TranscriptLayer::Correction(CorrectionLayer {
            provenance: provenance(),
            edits: vec![
                CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "Hello".to_string(),
                    reason: "misheard".to_string(),
                },
                CorrectionEdit {
                    start_word: 2,
                    end_word: 3,
                    replacement: "world".to_string(),
                    reason: "misheard".to_string(),
                },
            ],
        });
        assert_eq!(layer.validate(5), Ok(()));
    }

    #[test]
    fn correction_layer_rejects_overlapping_edits() {
        let layer = TranscriptLayer::Correction(CorrectionLayer {
            provenance: provenance(),
            edits: vec![
                CorrectionEdit {
                    start_word: 0,
                    end_word: 2,
                    replacement: "a".to_string(),
                    reason: "r".to_string(),
                },
                CorrectionEdit {
                    start_word: 2,
                    end_word: 3,
                    replacement: "b".to_string(),
                    reason: "r".to_string(),
                },
            ],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::OverlappingRanges { word: 2 })
        );
    }

    // ── HighlightLayer ─────────────────────────────────────────────────────

    #[test]
    fn highlight_layer_allows_overlap() {
        // Highlights are independent annotations — overlap is not ambiguity.
        let layer = TranscriptLayer::Highlight(HighlightLayer {
            provenance: provenance(),
            highlights: vec![
                HighlightEntry {
                    start_word: 0,
                    end_word: 3,
                    label: "theme-a".to_string(),
                    note: String::new(),
                },
                HighlightEntry {
                    start_word: 2,
                    end_word: 4,
                    label: "theme-b".to_string(),
                    note: String::new(),
                },
            ],
        });
        assert_eq!(layer.validate(5), Ok(()));
    }

    #[test]
    fn highlight_layer_rejects_out_of_bounds() {
        let layer = TranscriptLayer::Highlight(HighlightLayer {
            provenance: provenance(),
            highlights: vec![HighlightEntry {
                start_word: 0,
                end_word: 5,
                label: "theme".to_string(),
                note: String::new(),
            }],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::WordIndexOutOfBounds { index: 5, len: 5 })
        );
    }

    // ── EdlLayer ───────────────────────────────────────────────────────────

    #[test]
    fn edl_layer_delegates_to_the_selection_algebra() {
        let layer = TranscriptLayer::Edl(EdlLayer {
            provenance: provenance(),
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 1),
                    op: EdlOp::Keep,
                },
                EdlEntry {
                    range: WordRange::new(3, 4),
                    op: EdlOp::Keep,
                },
            ],
        });
        assert_eq!(layer.validate(5), Ok(()));
    }

    #[test]
    fn edl_layer_rejects_overlapping_keeps_via_the_algebra() {
        let layer = TranscriptLayer::Edl(EdlLayer {
            provenance: provenance(),
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 3),
                    op: EdlOp::Keep,
                },
                EdlEntry {
                    range: WordRange::new(2, 4),
                    op: EdlOp::Keep,
                },
            ],
        });
        assert_eq!(
            layer.validate(5),
            Err(LayerValidationError::EdlSelection(
                SelectionError::OverlappingKeepOps { word: 2 }
            ))
        );
    }

    // ── Shared invariants ──────────────────────────────────────────────────

    #[test]
    fn every_layer_kind_rejects_empty_transcripts() {
        let layers = vec![
            TranscriptLayer::Speaker(SpeakerLayer {
                provenance: provenance(),
                spans: vec![],
            }),
            TranscriptLayer::Paragraph(ParagraphLayer {
                provenance: provenance(),
                breaks_after: vec![],
            }),
            TranscriptLayer::Correction(CorrectionLayer {
                provenance: provenance(),
                edits: vec![],
            }),
            TranscriptLayer::Highlight(HighlightLayer {
                provenance: provenance(),
                highlights: vec![],
            }),
            TranscriptLayer::Edl(EdlLayer {
                provenance: provenance(),
                ops: vec![],
            }),
        ];
        for layer in layers {
            assert_eq!(
                layer.validate(0),
                Err(LayerValidationError::NoWordTimings),
                "kind {} must reject empty transcripts",
                layer.kind()
            );
        }
    }

    #[test]
    fn tagged_json_round_trips_every_kind() {
        let layers = vec![
            TranscriptLayer::Speaker(SpeakerLayer {
                provenance: provenance(),
                spans: vec![speaker_span(0, 1)],
            }),
            TranscriptLayer::Paragraph(ParagraphLayer {
                provenance: provenance(),
                breaks_after: vec![1],
            }),
            TranscriptLayer::Correction(CorrectionLayer {
                provenance: provenance(),
                edits: vec![CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "x".to_string(),
                    reason: "r".to_string(),
                }],
            }),
            TranscriptLayer::Highlight(HighlightLayer {
                provenance: provenance(),
                highlights: vec![HighlightEntry {
                    start_word: 0,
                    end_word: 1,
                    label: "l".to_string(),
                    note: "n".to_string(),
                }],
            }),
            TranscriptLayer::Edl(EdlLayer {
                provenance: provenance(),
                ops: vec![EdlEntry {
                    range: WordRange::new(0, 1),
                    op: EdlOp::Cut,
                }],
            }),
        ];
        for layer in layers {
            let json = serde_json::to_value(&layer).expect("layer serializes");
            assert_eq!(json["kind"], serde_json::json!(layer.kind()));
            let back: TranscriptLayer = serde_json::from_value(json).expect("layer deserializes");
            assert_eq!(back, layer);
        }
    }

    #[test]
    fn edl_op_serializes_snake_case() {
        let json = serde_json::to_value(EdlOp::Keep).expect("serializes");
        assert_eq!(json, serde_json::json!("keep"));
    }
}
