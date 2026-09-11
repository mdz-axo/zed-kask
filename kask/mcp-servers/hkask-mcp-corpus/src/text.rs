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

/// Explicit repeated word context through the same canonical text chunker.
pub(crate) fn chunk_text_with_overlap(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
    overlap_words: usize,
) -> Result<Vec<(String, String)>, hkask_mcp_server::server::McpToolError> {
    hkask_memory::text_chunking::chunk_text_with_overlap(
        text,
        entity_ref_prefix,
        min_words,
        max_words,
        sentence_boundary,
        overlap_words,
    )
    .map_err(|error| hkask_mcp_server::server::McpToolError::invalid_argument(error.to_string()))
}

/// Reversible source component: fixed-width hex of UTF-8 bytes. Unlike replacing
/// punctuation, this cannot alias `a.b.txt`, `a_b.txt`, or literal escape strings.
/// Keep the original source separately in provenance; this is only an ID component.
pub(crate) fn source_component(source: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::from("utf8-");
    for byte in source.bytes() {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 15)] as char);
    }
    encoded
}

/// Strip Project Gutenberg headers and footers from text.
/// Delegates to the pure free function in `hkask_memory::text_chunking`.
pub(crate) fn strip_gutenberg_headers(text: &str) -> String {
    hkask_memory::strip_gutenberg_headers(text)
}
