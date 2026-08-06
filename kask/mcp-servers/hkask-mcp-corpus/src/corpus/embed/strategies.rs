//! Chunking for the corpus embedding pipeline.
//!
//! `WordCountChunker` splits text at sentence boundaries within min/max word
//! count constraints. It is used by `EmbedService` (persona/style pipeline).
//! The `docproc_*` tools (QA training pipeline) call `crate::text::chunk_text`
//! directly — both paths share the same underlying chunker, just reached
//! differently.

/// Word-count-based chunking — used by `EmbedService` for persona/style
/// corpus embedding. Splits at sentence boundaries within min/max word
/// count constraints.
pub struct WordCountChunker {
    pub min_words: usize,
    pub max_words: usize,
    pub sentence_boundary: String,
}

impl WordCountChunker {
    /// Chunk `text` into passages, returning `(entity_ref, text)` pairs.
    pub fn chunk(&self, text: &str, entity_ref_prefix: &str) -> Vec<(String, String)> {
        crate::text::chunk_text(
            text,
            entity_ref_prefix,
            self.min_words,
            self.max_words,
            &self.sentence_boundary,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
