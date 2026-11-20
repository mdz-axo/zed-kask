use crate::transcript::TimedWord;
use crate::transcript_layers::{EdlLayer, LayerProvenance, TranscriptLayer};
use crate::transcript_select::{EdlEntry, EdlOp, WordRange};

pub fn timed_words(texts: &[&str]) -> Vec<TimedWord> {
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

pub fn overlapping_keeps_edl(provenance: LayerProvenance) -> TranscriptLayer {
    TranscriptLayer::Edl(EdlLayer {
        provenance,
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
    })
}
