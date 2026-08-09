//! Text processing utilities — pure string functions for chunking and cleaning.
//!
//! These functions have zero DB dependency. They delegate to the pure free
//! functions in `hkask_memory::text_chunking` (re-exported at the crate root as
//! `hkask_memory::chunk_text` / `hkask_memory::strip_gutenberg_headers`). This
//! module localizes the text-processing dependency so callers don't reach
//! through a storage type to get string utilities.

/// Chunk text into passages with word-count targets and sentence-boundary splitting.
/// Delegates to the pure free function in `hkask_memory::text_chunking`.
pub(crate) fn chunk_text(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
) -> Vec<(String, String)> {
    hkask_memory::chunk_text(
        text,
        entity_ref_prefix,
        min_words,
        max_words,
        sentence_boundary,
    )
}

/// Strip Project Gutenberg headers and footers from text.
/// Delegates to the pure free function in `hkask_memory::text_chunking`.
pub(crate) fn strip_gutenberg_headers(text: &str) -> String {
    hkask_memory::strip_gutenberg_headers(text)
}
