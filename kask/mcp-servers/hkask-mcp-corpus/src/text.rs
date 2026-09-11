//! Source identity and cleaning utilities with no database dependency.
//! Corpus chunking is adapted in `helpers::chunk_structure` and implemented
//! by the shared word-window engine in `hkask_memory::text_chunking`.

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
