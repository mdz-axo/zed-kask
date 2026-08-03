//! Text processing utilities — pure string functions for chunking and cleaning.
//!
//! These functions have zero DB dependency but were originally trapped as static
//! methods on `SemanticMemory` (a SQLCipher-backed memory store). This module
//! localizes the text-processing dependency so callers don't reach through a
//! storage type to get string utilities.

/// Chunk text into passages with word-count targets and sentence-boundary splitting.
/// Delegates to the pure implementation on `SemanticMemory` (no DB access).
pub(crate) fn chunk_text(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
) -> Vec<(String, String)> {
    hkask_memory::SemanticMemory::chunk_text(
        text,
        entity_ref_prefix,
        min_words,
        max_words,
        sentence_boundary,
    )
}

/// Strip Project Gutenberg headers and footers from text.
/// Delegates to the pure implementation on `SemanticMemory` (no DB access).
pub(crate) fn strip_gutenberg_headers(text: &str) -> String {
    hkask_memory::SemanticMemory::strip_gutenberg_headers(text)
}
