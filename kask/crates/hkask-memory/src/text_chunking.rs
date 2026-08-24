//! Pure text-chunking helpers — no database access, no store handle.
//!
//! These were methods on the deleted `SemanticMemory` struct. They never
//! touched the store, so they are free functions here. `MemoryStore` exposes
//! `chunk_text` / `strip_gutenberg_headers` as associated functions that
//! delegate to these, so existing call sites keep their shape.

/// Threshold: if control characters exceed 0.5% of total characters, the
/// PDF font encoding is corrupted and the text should be re-extracted via
/// OCR rather than used as-is.
const CORRUPTED_FONT_ENCODING_THRESHOLD: f64 = 0.005;

/// Detect whether extracted text has corrupted font encoding.
///
/// PDFs with custom `ToUnicode` CMaps can cause `pdftotext` to emit C0
/// control characters (\x01-\x08, \x0e-\x1f) where normal ASCII letters
/// and digits should be. This makes the text unreadable by LLM taggers.
///
/// Returns true if control characters (excluding \t, \n, \r) exceed
/// 0.5% of total characters — a signal that the font encoding is broken
/// and OCR should be used instead.
pub fn has_corrupted_font_encoding(text: &str) -> bool {
    let total = text.len();
    if total == 0 {
        return false;
    }
    let control_count = text
        .chars()
        .filter(|ch| {
            let o = *ch as u32;
            o < 0x20 && o != 0x09 && o != 0x0a && o != 0x0d
        })
        .count();
    (control_count as f64 / total as f64) > CORRUPTED_FONT_ENCODING_THRESHOLD
}

/// Replace C0 control characters with spaces.
///
/// PDF extraction (pdftotext) maps mathematical symbols (turnstile ⊢,
/// sequent separators, etc.) to raw C0 control bytes (\x01-\x08, \x0e-\x1f)
/// when the PDF uses custom font encodings. These bytes are valid UTF-8 but
/// meaningless as text — they cause LLM taggers to see garbage and return
/// "empty passage" fallback tags.
///
/// Preserves \t (\x09), \n (\x0a), \r (\x0d) — structural whitespace.
/// Collapses runs of resulting spaces into one.
pub fn sanitize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_was_space = false;
    for ch in text.chars() {
        let o = ch as u32;
        if o < 0x20 && o != 0x09 && o != 0x0a && o != 0x0d {
            if !prev_was_space {
                out.push(' ');
                prev_was_space = true;
            }
        } else if ch == ' ' {
            if !prev_was_space {
                out.push(' ');
                prev_was_space = true;
            }
        } else {
            out.push(ch);
            prev_was_space = false;
        }
    }
    out
}

/// Chunk text into passages for embedding.
///
/// Sanitizes control characters, then splits on structural boundaries
/// (markdown headings, horizontal rules, then paragraph breaks), applies
/// min/max word count constraints, and splits long paragraphs at the nearest
/// sentence boundary. Short paragraphs are concatenated until min_words is
/// reached.
///
/// Returns (entity_ref, text) pairs with entity_ref formatted as
/// `{entity_ref_prefix}:{chunk_index}`.
///
/// expect: "I can store shared semantic h_mems for public knowledge"
/// \[P3\] Motivating: Generative Space — chunks text into passage-sized units for embedding
/// \[P5\] Constraining: Essentialism — structural/sentence boundary splitting with min/max words
/// pre:  text is non-empty, entity_ref_prefix is non-empty
/// pre:  min_words > 0, max_words >= min_words
/// post: returns Vec of (entity_ref, text) chunks
/// post: each chunk has word count between min_words and max_words (best-effort)
/// post: no chunk contains C0 control characters except \t \n \r
pub fn chunk_text(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
) -> Vec<(String, String)> {
    let text = sanitize_text(text);
    // Structural splitting: headings/rules become their own paragraph units so
    // chunks don't straddle unrelated sections (improves concept coherence, which
    // the salience graph depends on).
    let paragraphs = split_structural(&text);

    let mut passages = Vec::new();
    let mut buffer = String::new();
    let mut buffer_words = 0usize;
    let mut chunk_index = 0usize;
    let boundary_chars: Vec<char> = sentence_boundary
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    for paragraph in &paragraphs {
        let word_count = paragraph.split_whitespace().count();

        if buffer_words + word_count > max_words && buffer_words >= min_words {
            let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
            passages.push((entity_ref, buffer.trim().to_string()));
            chunk_index += 1;
            buffer.clear();
            buffer_words = 0;
        }

        if word_count > max_words {
            if !buffer.is_empty() && buffer_words >= min_words {
                let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                passages.push((entity_ref, buffer.trim().to_string()));
                chunk_index += 1;
                buffer.clear();
                buffer_words = 0;
            }
            // Split a too-long paragraph at the nearest sentence boundary at or
            // after max_words (look-ahead up to 25% of max_words), falling back
            // to the last boundary before max_words, then a hard cut.
            let words: Vec<&str> = paragraph.split_whitespace().collect();
            let mut start = 0usize;
            while start < words.len() {
                let target = (start + max_words).min(words.len());
                let look_ahead = (max_words / 4).max(1);
                let mut split_at = target;
                let mut found = false;
                for (i, w) in words.iter().enumerate().skip(target).take(look_ahead) {
                    if is_sentence_end(w, &boundary_chars) {
                        split_at = i + 1;
                        found = true;
                        break;
                    }
                }
                if !found {
                    let back_floor = start + min_words.min(words.len());
                    for i in (back_floor..target).rev() {
                        if is_sentence_end(words[i], &boundary_chars) {
                            split_at = i + 1;
                            break;
                        }
                    }
                }
                let chunk_words = &words[start..split_at];
                let text = chunk_words.join(" ");
                let cw = chunk_words.len();
                if cw >= min_words {
                    let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                    passages.push((entity_ref, text));
                    chunk_index += 1;
                    buffer.clear();
                    buffer_words = 0;
                } else if !buffer.is_empty() {
                    buffer.push(' ');
                    buffer.push_str(&text);
                    buffer_words += cw;
                } else {
                    let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                    passages.push((entity_ref, text));
                    chunk_index += 1;
                }
                start = split_at;
            }
        } else {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(paragraph);
            buffer_words += word_count;
        }
    }

    if !buffer.is_empty() {
        let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
        passages.push((entity_ref, buffer.trim().to_string()));
    }

    passages
}

/// True when `word` ends a sentence: its final non-quote char is a boundary
/// punctuation. Handles trailing quotes (`asked."`) and numeric decimals
/// (`3.14` — a digit before the period is not a sentence end).
/// Single-letter initials (`J.`) are not sentence ends.
fn is_sentence_end(word: &str, boundary_chars: &[char]) -> bool {
    let trimmed = word.trim_end_matches(['"', '\'', '\u{201d}', '\u{201c}']);
    let mut chars = trimmed.chars();
    let last = match chars.next_back() {
        Some(c) => c,
        None => return false,
    };
    if !boundary_chars.contains(&last) {
        return false;
    }
    if last == '.' && chars.next_back().is_some_and(|p| p.is_ascii_digit()) {
        return false;
    }
    let stem = trimmed.trim_end_matches(['.', '!', '?']);
    if last == '.'
        && stem.chars().count() == 1
        && stem.chars().next().is_some_and(|c| c.is_uppercase())
    {
        return false;
    }
    true
}

/// Split text into paragraphs on structural boundaries: markdown headings,
/// horizontal rules, and blank-line breaks. Headings/rules always start a
/// new paragraph so chunks don't straddle unrelated sections.
fn split_structural(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let is_heading = trimmed.starts_with('#')
            && trimmed
                .chars()
                .nth(1)
                .is_none_or(|c| c == '#' || c.is_whitespace());
        let is_rule = trimmed == "---" || trimmed == "***" || trimmed == "___";
        if (is_heading || is_rule) && !buf.is_empty() {
            let p = buf.trim().to_string();
            if !p.is_empty() {
                out.push(p);
            }
            buf.clear();
        }
        if is_heading || is_rule {
            let p = trimmed.to_string();
            if !p.is_empty() {
                out.push(p);
            }
        } else {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
    }
    let p = buf.trim().to_string();
    if !p.is_empty() {
        out.push(p);
    }
    let mut final_out = Vec::new();
    for para in out {
        for piece in para.split("\n\n") {
            let t = piece.trim();
            if !t.is_empty() {
                final_out.push(t.to_string());
            }
        }
    }
    final_out
}

/// Strip Project Gutenberg headers and footers from text.
///
/// Looks for the standard `*** START OF` / `*** END OF` markers.
///
/// expect: "I can store shared semantic h_mems for public knowledge"
/// \[P3\] Motivating: Generative Space — removes boilerplate for clean corpus ingestion
/// \[P5\] Constraining: Essentialism — marker-based trim, no regex
/// pre:  text is a valid &str
/// post: returns text between START OF and END OF markers
/// post: returns full text if markers not found
pub fn strip_gutenberg_headers(text: &str) -> String {
    let start_marker = "*** START OF";
    let end_marker = "*** END OF";

    let start = text
        .find(start_marker)
        .and_then(|i| text[i..].find('\n').map(|j| i + j + 1))
        .unwrap_or(0);

    let end = text.find(end_marker).unwrap_or(text.len());

    text[start..end].trim().to_string()
}

/// The form-feed character `pdftotext` uses to separate pages.
const FORM_FEED: char = '\u{000c}';

/// Filter out title pages, tables of contents, and index pages from
/// extracted text before chunking.
///
/// `pdftotext` separates pages with form-feed (`\x0c`). This function
/// splits on form-feed, classifies each page, and drops boilerplate pages.
/// Pages are classified by content patterns:
///
/// - **Title pages**: short text (< 100 chars), mostly whitespace, often
///   containing only the book title, author, publisher.
/// - **Table of contents**: lines matching `\d+\.\s+` followed by page
///   numbers, or repeated lines of `..... ` dot leaders.
/// - **Index pages**: lines that are single-word or short phrases followed
///   by page number lists (e.g. "concept, 42, 87, 103"), or lines with
///   many comma-separated numbers.
/// - **Copyright pages**: contain "copyright", "all rights reserved",
///   "isbn", "printed in".
/// - **Blank pages**: empty or whitespace-only.
///
/// Returns the text with boilerplate pages removed, rejoined with
/// form-feed so downstream chunking sees only content pages.
pub fn filter_boilerplate_pages(text: &str) -> String {
    let pages: Vec<&str> = text.split(FORM_FEED).collect();
    let kept: Vec<String> = pages
        .iter()
        .filter(|page| !is_boilerplate_page(page))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    kept.join("\n")
}

/// Classify a single page as boilerplate (title, TOC, index, copyright, blank)
/// or content. Returns true if the page should be dropped.
fn is_boilerplate_page(page: &str) -> bool {
    let trimmed = page.trim();
    if trimmed.is_empty() {
        return true;
    }
    let char_count = trimmed.chars().count();
    let lower = trimmed.to_lowercase();

    // Blank or near-blank pages (title pages are often mostly whitespace)
    if char_count < 50 {
        return true;
    }

    // Copyright / publisher pages
    if lower.contains("all rights reserved")
        || lower.contains("copyright")
        || lower.contains("printed in")
        || lower.contains("isbn")
    {
        return true;
    }

    // Table of contents: dot leaders (.... ) or repeated "N. Title    page" patterns
    let lines: Vec<&str> = trimmed.lines().collect();
    let dot_leader_count = lines
        .iter()
        .filter(|l| l.contains("...") || l.matches('.').count() > 5)
        .count();
    if lines.len() > 3 && dot_leader_count as f64 / lines.len() as f64 > 0.4 {
        return true;
    }

    // Table of contents: lines matching "N.  Title   page_num" pattern
    let toc_line_count = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            // "1. Introduction    3" or "1.1 Background  15"
            l.len() < 100
                && (l.starts_with(|c: char| c.is_ascii_digit()) || l.starts_with("Chapter"))
                && l.chars().filter(|c| c.is_ascii_digit()).count() > 0
                && (l.contains('.') && l.split_whitespace().count() < 12)
        })
        .count();
    if lines.len() > 5 && toc_line_count as f64 / lines.len() as f64 > 0.6 {
        return true;
    }

    // Index pages: entries are short phrases followed by comma-separated page numbers
    // e.g. "algorithm, 42, 87, 103" or "lambda calculus, 15, 22"
    let index_entry_count = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            // Short line with multiple comma-separated numbers at the end
            l.len() < 120
                && l.matches(',').count() >= 2
                && {
                    let nums: Vec<&str> = l.rsplit(',').take(3).collect();
                    nums.iter().filter(|s| s.trim().parse::<usize>().is_ok()).count() >= 2
                }
        })
        .count();
    if lines.len() > 5 && index_entry_count as f64 / lines.len() as f64 > 0.5 {
        return true;
    }

    // Explicit "Contents" or "Index" header on a short page
    if (lower.starts_with("contents") || lower.starts_with("index"))
        && char_count < 2000
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_control_chars_with_space() {
        let input = "hello\x01world\x06test\x0eend";
        assert_eq!(sanitize_text(input), "hello world test end");
    }

    #[test]
    fn sanitize_preserves_newlines_tabs() {
        let input = "line1\nline2\tindented\r\nwindows";
        assert_eq!(sanitize_text(input), "line1\nline2\tindented\r\nwindows");
    }

    #[test]
    fn sanitize_collapses_consecutive_spaces() {
        let input = "a\x01\x06\x03b";
        assert_eq!(sanitize_text(input), "a b");
    }

    #[test]
    fn sanitize_preserves_normal_text() {
        let input = "Normal text with unicode: ⊢ Γ ⊥ λμ";
        assert_eq!(sanitize_text(input), input);
    }

    #[test]
    fn sanitize_handles_all_c0_control_chars() {
        // Every C0 control char except \t (0x09), \n (0x0a), \r (0x0d)
        let bad: Vec<char> = (0u8..=0x1f)
            .filter(|&b| b != 0x09 && b != 0x0a && b != 0x0d)
            .map(|b| b as char)
            .collect();
        for ch in &bad {
            let input = format!("x{}y", ch);
            let result = sanitize_text(&input);
            assert!(
                !result.contains(*ch),
                "control char {:02x} not removed",
                *ch as u8
            );
        }
    }

    #[test]
    fn sanitize_handles_empty_string() {
        assert_eq!(sanitize_text(""), "");
    }

    #[test]
    fn sanitize_handles_only_control_chars() {
        assert_eq!(sanitize_text("\x01\x02\x03"), " ");
    }

    #[test]
    fn chunk_text_produces_no_control_chars() {
        let input = "Hello \x01 world \x06 this is \x0e a test passage with enough words to form a chunk for testing purposes here.";
        let chunks = chunk_text(input, "test", 5, 50, ".!?");
        for (_, text) in &chunks {
            for ch in text.chars() {
                let o = ch as u32;
                assert!(
                    o >= 0x20 || o == 0x09 || o == 0x0a || o == 0x0d,
                    "control char {:02x} found in chunk output",
                    o
                );
            }
        }
    }

    #[test]
    fn detects_corrupted_font_encoding() {
        // 6.3% of chunks in the John Brooks corpus had this pattern
        let corrupted = "th\x0e quick brown fox jumps over th\x0e lazy dog";
        assert!(has_corrupted_font_encoding(corrupted));
    }

    #[test]
    fn does_not_flag_clean_text_as_corrupted() {
        let clean = "The quick brown fox jumps over the lazy dog. This is a normal passage with no control characters whatsoever.";
        assert!(!has_corrupted_font_encoding(clean));
    }

    #[test]
    fn does_not_flag_math_symbols_as_corrupted() {
        // Unicode math symbols (⊢, ⊥, λ, Γ) are NOT control chars
        let math = "Given Γ ⊢ A ⊥ λ, the proof term is constructed as follows.";
        assert!(!has_corrupted_font_encoding(math));
    }

    #[test]
    fn filter_drops_blank_pages() {
        let text = "\x0c\x0cReal content here with enough words to be a real page.\x0c\x0c";
        let result = filter_boilerplate_pages(text);
        assert!(result.contains("Real content"));
        assert!(!result.is_empty());
    }

    #[test]
    fn filter_drops_copyright_pages() {
        let text = "Copyright © 2011 by Imperial College Press\nAll rights reserved.\nISBN-13 978-1-84816-456-7\n\x0cChapter 1: Introduction\n\nThis is real content that should be kept because it is the actual body text of the book and contains substantive material.";
        let result = filter_boilerplate_pages(text);
        assert!(!result.contains("Copyright"));
        assert!(!result.contains("ISBN"));
        assert!(result.contains("Chapter 1"));
        assert!(result.contains("real content"));
    }

    #[test]
    fn filter_drops_toc_pages() {
        let toc = "Contents\n\n1. Introduction          3\n2. Background             15\n3. Methods               27\n4. Results               42\n5. Discussion            58\n6. Conclusion            71";
        assert!(is_boilerplate_page(toc));
    }

    #[test]
    fn filter_drops_dot_leader_toc() {
        let toc = "1. Introduction........... 3\n2. Background.............. 15\n3. Methods................. 27\n4. Results................. 42";
        assert!(is_boilerplate_page(toc));
    }

    #[test]
    fn filter_drops_index_pages() {
        let index = "Index\n\nalgorithm, 42, 87, 103\nlambda calculus, 15, 22\ntype theory, 8, 34, 56\nsequent, 12, 45, 78\nturnstile, 3, 19";
        assert!(is_boilerplate_page(index));
    }

    #[test]
    fn filter_keeps_content_pages() {
        let content = "This is a substantial paragraph of real academic content that discusses important concepts in formal logic and their application to natural language semantics. The author argues that ludics provides a framework for understanding meaning through interaction.";
        assert!(!is_boilerplate_page(content));
    }

    #[test]
    fn filter_drops_title_pages() {
        let title = "Meaning, Logic and Ludics\n\nAlain Lecomte";
        assert!(is_boilerplate_page(title));
    }
}
