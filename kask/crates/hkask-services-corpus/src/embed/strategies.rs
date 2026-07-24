//! Strategy traits for the corpus embedding pipeline.
//!
//! These traits make the relationship between `EmbedService` (persona/style
//! pipeline) and the `docproc_*` tools (QA training pipeline) visible in the
//! semantic graph. Both pipelines perform chunking, embedding, and triple
//! extraction, but with different implementations:
//!
//! - **EmbedService** uses word-count chunking, rule-based entity tagging,
//!   plain embedding, and `hkask_services_runtime` triple extraction.
//! - **docproc tools** use token-count chunking, LLM-based ontology tagging,
//!   INSTRUCTOR-method ontology-anchored embedding, and hallucination-guarded
//!   triple extraction.
//!
//! The traits are **design documentation** — they declare the shared
//! operations without requiring behavioral convergence. Each pipeline picks
//! the strategy appropriate for its output branch.

use std::path::Path;

/// A chunking strategy — splits text into passages.
///
/// Implementations:
/// - `WordCountChunker` — used by `EmbedService` (persona pipeline)
/// - `TokenCountChunker` — used by `docproc_chunk` (QA training pipeline)
#[allow(dead_code)]
pub trait ChunkingStrategy: Send + Sync {
    /// Chunk `text` into passages, returning `(entity_ref, text)` pairs.
    fn chunk(&self, text: &str, entity_ref_prefix: &str) -> Vec<(String, String)>;

    /// Human-readable name for diagnostics (e.g., "word-count", "token-count").
    fn name(&self) -> &'static str;
}

/// An embedding strategy — converts text to vectors.
///
/// Implementations:
/// - `PlainEmbedder` — used by `EmbedService` (no annotation prefix)
/// - `InstructorEmbedder` — used by `docproc_embed` (ontology tags prepended)
#[allow(dead_code)] // Forward-declared for docproc integration
pub trait EmbeddingStrategy: Send + Sync {
    /// Embed a batch of texts, returning vectors.
    ///
    /// `annotations` is an optional parallel slice of ontology tag prefixes
    /// (INSTRUCTOR method). When `None` or empty, plain embedding is used.
    fn embed_batch(
        &self,
        model: &str,
        texts: &[&str],
        annotations: Option<&[String]>,
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>>;

    /// Human-readable name for diagnostics (e.g., "plain", "instructor").
    fn name(&self) -> &'static str;
}

/// A triple extraction strategy — extracts RDF triples from text.
///
/// Implementations:
/// - `RuntimeTripleExtractor` — used by `EmbedService` (via `hkask_services_runtime`)
/// - `DocprocTripleExtractor` — used by `docproc_extract_triples` (Jinja2 template + hallucination guard)
#[allow(dead_code)] // Forward-declared for docproc integration
pub trait TripleExtractionStrategy: Send + Sync {
    /// Extract triples from a batch of texts.
    ///
    /// Returns a vector of extraction results (one per input text).
    fn extract_batch(
        &self,
        texts: &[&str],
        config_path: Option<&Path>,
    ) -> Result<Vec<TripleExtraction>, Box<dyn std::error::Error + Send + Sync>>;

    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;
}

/// Result of triple extraction for a single passage.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Forward-declared for docproc integration
pub struct TripleExtraction {
    pub topic: String,
    pub concepts: Vec<String>,
    pub triples: Vec<(String, String, String)>,
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
