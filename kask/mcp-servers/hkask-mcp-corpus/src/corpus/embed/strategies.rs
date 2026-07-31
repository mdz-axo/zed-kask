//! Chunking strategy for the corpus embedding pipeline.
//!
//! `ChunkingStrategy` makes the relationship between `EmbedService`
//! (persona/style pipeline) and the `docproc_*` tools (QA training pipeline)
//! visible in the semantic graph. Both pipelines chunk text, but with
//! different implementations: word-count chunking for `EmbedService` and
//! token-count chunking for `corpus_chunk`.

/// A chunking strategy — splits text into passages.
///
/// Implementations:
/// - `WordCountChunker` — used by `EmbedService` (persona pipeline)
/// - `TokenCountChunker` — used by `corpus_chunk` (QA training pipeline)
pub trait ChunkingStrategy: Send + Sync {
    /// Chunk `text` into passages, returning `(entity_ref, text)` pairs.
    fn chunk(&self, text: &str, entity_ref_prefix: &str) -> Vec<(String, String)>;

    /// Human-readable name for diagnostics (e.g., "word-count", "token-count").
    fn name(&self) -> &'static str;
}

// ── Concrete strategy implementations ──────────────────────────────────────

/// Word-count-based chunking — used by `EmbedService` for persona/style
/// corpus embedding. Splits at sentence boundaries within min/max word
/// count constraints.
pub struct WordCountChunker {
    pub min_words: usize,
    pub max_words: usize,
    pub sentence_boundary: String,
}

impl ChunkingStrategy for WordCountChunker {
    fn chunk(&self, text: &str, entity_ref_prefix: &str) -> Vec<(String, String)> {
        hkask_memory::SemanticMemory::chunk_text(
            text,
            entity_ref_prefix,
            self.min_words,
            self.max_words,
            &self.sentence_boundary,
        )
    }

    fn name(&self) -> &'static str {
        "word-count"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_chunker_name() {
        let chunker = WordCountChunker {
            min_words: 40,
            max_words: 150,
            sentence_boundary: ".!? ".to_string(),
        };
        assert_eq!(chunker.name(), "word-count");
    }

    #[test]
    fn word_count_chunker_produces_passages() {
        let chunker = WordCountChunker {
            min_words: 1,
            max_words: 10,
            sentence_boundary: ". ".to_string(),
        };
        let text = "First sentence. Second sentence. Third sentence here.";
        let passages = chunker.chunk(text, "test");
        assert!(!passages.is_empty(), "should produce at least one passage");
        assert!(
            passages.iter().all(|(r, _)| r.starts_with("test:")),
            "entity refs should use prefix"
        );
    }
}
