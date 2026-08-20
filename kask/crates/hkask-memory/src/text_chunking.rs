//! Pure text-chunking helpers — no database access, no store handle.
//!
//! These were methods on the deleted `SemanticMemory` struct. They never
//! touched the store, so they are free functions here. `MemoryStore` exposes
//! `chunk_text` / `strip_gutenberg_headers` as associated functions that
//! delegate to these, so existing call sites keep their shape.

/// Chunk text into passages for embedding.
///
/// Splits on structural boundaries (markdown headings, horizontal rules,
/// then paragraph breaks), applies min/max word count constraints, and
/// splits long paragraphs at the nearest sentence boundary. Short
/// paragraphs are concatenated until min_words is reached.
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
pub fn chunk_text(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
) -> Vec<(String, String)> {
    // Structural splitting: headings/rules become their own paragraph units so
    // chunks don't straddle unrelated sections (improves concept coherence, which
    // the salience graph depends on).
    let paragraphs = split_structural(text);

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
