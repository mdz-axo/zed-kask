//! HTML tag stripping utility.
//
// S2 unification: this module previously carried a divergent `strip_html_tags`
// implementation (entity-decoding but no block-tag handling, no script/style
// removal). Documents processed via `corpus_convert` used `convert::strip_html`
// (block-aware, script/style removal, entity decoding) while documents
// processed via `EmbedService::embed_corpus` → `fetch_text` used
// `strip_html_tags` (entity decoding only). The embed pipeline silently lost
// block structure and leaked `<script>`/`<style>` text.
//
// The two are now unified on `convert::strip_html`, which is the richer,
// block-aware, script/style-stripping, entity-decoding implementation.
// `strip_html_tags` is retained as a thin wrapper so existing call sites
// (`corpus/fetch.rs`, the `corpus` re-export) are unchanged.

/// Strip HTML tags from text, decoding common entities and preserving
/// block-level word boundaries.
///
/// Delegates to [`crate::convert::strip_html`], which removes `<script>`/
/// `<style>` elements entirely, inserts a space at block-level tag
/// boundaries (`<p>`, `<div>`, `<br>`, headings, `<li>`, `<table>`, etc.)
/// so words don't concatenate across block boundaries, collapses
/// whitespace, and decodes named + numeric HTML entities.
///
/// Both this function and `convert::strip_html` produce single-line,
/// space-joined output (no newlines), so the embed chunker
/// (`hkask_memory::chunk_text` → `split_structural`) sees the same text
/// shape it did before — only with better word boundaries and no leaked
/// script/style content.
#[must_use]
pub fn strip_html_tags(html: &str) -> String {
    crate::convert::strip_html(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S2 unification pin: `strip_html_tags` now delegates to
    /// `convert::strip_html`, so it must inherit block-tag spacing AND
    /// entity decoding AND script/style removal in one call. Before
    /// unification, the embed path got entity decoding but lost block
    /// spacing and leaked `<script>` text; the convert path got block
    /// spacing and script removal but the two implementations drifted.
    #[test]
    fn strip_html_tags_unified_decodes_entities_and_blocks_and_strips_script() {
        // Entity decoding (previously the only thing this fn did).
        assert_eq!(strip_html_tags("a &amp; b"), "a & b");
        assert_eq!(strip_html_tags("a &lt; b &gt; c"), "a < b > c");
        assert_eq!(strip_html_tags("&#8217;quote&#8217;"), "’quote’");

        // Block-tag spacing (inherited from convert::strip_html). Without
        // it, `<p>a</p><p>b</p>` would render as "ab".
        assert_eq!(strip_html_tags("<p>a</p><p>b</p>"), "a b");

        // Combined: block spacing + entity decoding in the same document.
        assert_eq!(strip_html_tags("<p>a &amp; b</p><p>c</p>"), "a & b c");

        // script/style removal (inherited). Before unification the embed
        // path leaked script text into the corpus.
        assert_eq!(
            strip_html_tags("<p>visible</p><script>var x = 1;</script><p>more</p>"),
            "visible more"
        );
        assert_eq!(
            strip_html_tags("<p>visible</p><style>.x{color:red}</style><p>more</p>"),
            "visible more"
        );
    }
}
