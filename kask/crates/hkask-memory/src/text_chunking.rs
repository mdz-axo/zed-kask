//! Pure text-chunking helpers — no database access, no store handle.
//!
//! Turn-memory ingestion uses the no-overlap `chunk_text` entry point;
//! corpus processing supplies explicit overlap to the same window engine.

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
/// No-overlap entry point used by turn-memory ingestion. The shared window
/// engine sanitizes control characters and prefers structural/sentence ends
/// within the word budget. Short structural fragments are carried forward
/// until min_words is reached; the final remainder is always retained.
///
/// # Panics
/// Panics if max_words is zero (a programmer error in this infallible API).
/// Caller-controlled budgets should use `chunk_text_with_overlap`.
///
/// Returns (entity_ref, text) pairs with entity_ref formatted as
/// `{entity_ref_prefix}:{chunk_index}`.
///
/// expect: "I can store shared h_mems for public knowledge"
/// \[P3\] Motivating: Generative Space — chunks text into passage-sized units for embedding
/// \[P5\] Constraining: Essentialism — structural/sentence boundary splitting with min/max words
/// pre:  text is non-empty, entity_ref_prefix is non-empty
/// pre:  min_words > 0, max_words >= min_words
/// post: returns Vec of (entity_ref, text) chunks
/// post: every chunk has at most max_words; only the final chunk may be below min_words
/// post: no chunk contains C0 control characters except \t \n \r
pub fn chunk_text(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
) -> Vec<(String, String)> {
    assert!(max_words > 0, "max_words must be positive");
    chunk_windows(
        text,
        entity_ref_prefix,
        min_words,
        max_words,
        sentence_boundary,
        0,
    )
}

/// Chunk with explicit repeated context, measured in whitespace-delimited words.
///
/// expect: "Every overlapping passage repeats context and contributes new source words."
/// [P3] Motivating: Generative Space — retain source content for retrieval.
/// [P4] Constraining: bounded, advancing windows; no model-token guarantee.
/// pre: max_words > 0, overlap_words < max_words
/// post: positive overlap repeats exactly overlap_words from the previous suffix;
///       each passage contributes new words and contains at most max_words.
/// post: zero overlap uses the same window rule, without repeated words.
///
/// Uses the canonical sanitizer, structural splitter and sentence-end detector.
/// Structural/sentence ends are preferred within the word
/// budget, but never at the expense of forward progress. Whitespace is normalized.
/// This is not a tokenizer: words (especially non-English text or long identifiers)
/// may consume many model tokens.
pub fn chunk_text_with_overlap(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
    overlap_words: usize,
) -> anyhow::Result<Vec<(String, String)>> {
    anyhow::ensure!(max_words > 0, "max_words must be positive");
    anyhow::ensure!(
        overlap_words < max_words,
        "overlap_words must be less than max_words"
    );
    Ok(chunk_windows(
        text,
        entity_ref_prefix,
        min_words,
        max_words,
        sentence_boundary,
        overlap_words,
    ))
}

fn chunk_windows(
    text: &str,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    sentence_boundary: &str,
    overlap_words: usize,
) -> Vec<(String, String)> {
    let sanitized = sanitize_text(text);
    let paragraphs = split_structural(&sanitized);
    let mut words = Vec::new();
    let mut structural_ends = Vec::new();
    for paragraph in &paragraphs {
        words.extend(paragraph.split_whitespace());
        structural_ends.push(words.len());
    }
    let boundary_chars: Vec<_> = sentence_boundary
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut passages = Vec::new();
    let mut start = 0usize;
    while start < words.len() {
        let target = start.saturating_add(max_words).min(words.len());
        let floor = start.saturating_add(min_words.max(overlap_words + 1).min(max_words));
        let end = if target == words.len() {
            target
        } else {
            // partition_point avoids rescanning all prior sections for every window.
            let upper = structural_ends.partition_point(|&end| end <= target);
            upper
                .checked_sub(1)
                .and_then(|index| structural_ends.get(index))
                .copied()
                .filter(|&end| end >= floor)
                .or_else(|| {
                    (floor..=target)
                        .rev()
                        .find(|&end| is_sentence_end(words[end - 1], &boundary_chars))
                })
                .unwrap_or(target)
        };
        passages.push((
            format!("{entity_ref_prefix}:{}", passages.len()),
            words[start..end].join(" "),
        ));
        if end == words.len() {
            break;
        }
        start = end - overlap_words;
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
/// expect: "I can store shared h_mems for public knowledge"
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoilerplateExclusion {
    pub reason: &'static str,
    pub boundary_unit: &'static str,
    pub start: usize,
    pub end: usize,
    pub removed_words: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoilerplateFilterResult {
    pub text: String,
    pub input_words: usize,
    pub retained_words: usize,
    pub exclusions: Vec<BoilerplateExclusion>,
}

/// expect: I can remove bounded book furniture while retaining substantive content and reviewing every exclusion.
/// [P3] Motivating: Generative Space — downstream corpus stages receive content rather than front/back matter.
/// [P1] Constraining: Human Agency — every removal carries a reason and source boundary.
/// [P2] Constraining: Cognitive Sovereignty — prose mentions never act as deletion commands.
/// pre: text is valid UTF-8 and may be page-delimited or a form-feed-free extraction
/// post: returns retained text plus complete word-count and exclusion-range accounting
pub fn filter_boilerplate_pages_with_report(text: &str) -> BoilerplateFilterResult {
    let input_words = text.split_whitespace().count();
    let (filtered, exclusions) = if text.contains(FORM_FEED) {
        filter_page_delimited_boilerplate(text)
    } else if let Some(reason) = boilerplate_page_reason(text) {
        let removed_words = text.split_whitespace().count();
        (
            String::new(),
            vec![BoilerplateExclusion {
                reason,
                boundary_unit: "document",
                start: 0,
                end: 1,
                removed_words,
            }],
        )
    } else {
        filter_unpaged_boilerplate(text)
    };
    let retained_words = filtered.split_whitespace().count();

    BoilerplateFilterResult {
        text: filtered,
        input_words,
        retained_words,
        exclusions,
    }
}

pub fn filter_boilerplate_pages(text: &str) -> String {
    filter_boilerplate_pages_with_report(text).text
}

fn filter_page_delimited_boilerplate(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let mut kept = Vec::new();
    let mut exclusions = Vec::new();
    for (page_index, page) in text.split(FORM_FEED).enumerate() {
        if let Some(reason) = boilerplate_page_reason(page) {
            exclusions.push(BoilerplateExclusion {
                reason,
                boundary_unit: "page",
                start: page_index,
                end: page_index + 1,
                removed_words: page.split_whitespace().count(),
            });
        } else {
            let page = page.trim();
            if !page.is_empty() {
                kept.push(page.to_string());
            }
        }
    }
    (kept.join("\n"), exclusions)
}

fn filter_unpaged_boilerplate(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return (String::new(), Vec::new());
    }

    let front_search_end = lines.len().min(400).max((lines.len() / 5).min(400));
    let front_signal = lines
        .iter()
        .take(front_search_end)
        .position(|line| is_front_matter_signal(line));
    let front_end = front_signal
        .and_then(|signal| {
            lines
                .iter()
                .enumerate()
                .take(front_search_end)
                .skip(signal + 1)
                .find(|(_, line)| is_body_start_heading(line))
                .map(|(index, _)| index)
        })
        .unwrap_or(0);

    let back_search_start = word_fraction_line_index(&lines, 2, 3).max(front_end);
    let back_boundary =
        lines
            .iter()
            .enumerate()
            .skip(back_search_start)
            .find_map(|(index, line)| {
                back_matter_reason(&lines, index, line).map(|reason| (index, reason))
            });
    let back_start = back_boundary.map_or(lines.len(), |(index, _)| index);

    let mut exclusions = Vec::new();
    if front_end > 0 {
        exclusions.push(line_exclusion("front_matter", &lines, 0, front_end));
    }
    if let Some((index, reason)) = back_boundary {
        exclusions.push(line_exclusion(reason, &lines, index, lines.len()));
    }

    let retained = lines
        .get(front_end..back_start)
        .unwrap_or_default()
        .join("\n")
        .trim()
        .to_string();
    (retained, exclusions)
}

fn word_fraction_line_index(lines: &[&str], numerator: usize, denominator: usize) -> usize {
    let total_words = lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .count();
    let target_words = total_words.saturating_mul(numerator) / denominator.max(1);
    let mut words_before = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if words_before >= target_words {
            return index;
        }
        words_before = words_before.saturating_add(line.split_whitespace().count());
    }
    lines.len()
}

fn line_exclusion(
    reason: &'static str,
    lines: &[&str],
    start: usize,
    end: usize,
) -> BoilerplateExclusion {
    let removed_words = lines
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .flat_map(|line| line.split_whitespace())
        .count();
    BoilerplateExclusion {
        reason,
        boundary_unit: "line",
        start,
        end,
        removed_words,
    }
}

fn normalized_heading(line: &str) -> String {
    line.trim()
        .trim_matches(|character: char| !character.is_alphanumeric() && !character.is_whitespace())
        .to_lowercase()
}

fn is_front_matter_signal(line: &str) -> bool {
    let heading = normalized_heading(line);
    heading == "contents"
        || heading == "table of contents"
        || heading.contains("copyright")
        || heading.contains("all rights reserved")
        || heading.contains("isbn")
        || heading.contains("printed in")
}

fn is_body_start_heading(line: &str) -> bool {
    let heading = normalized_heading(line);
    let words: Vec<&str> = heading.split_whitespace().collect();
    if heading.contains("...")
        || (words.len() > 2
            && words
                .last()
                .is_some_and(|word| word.parse::<usize>().is_ok()))
    {
        return false;
    }
    heading == "preface"
        || heading == "foreword"
        || heading == "introduction"
        || heading == "prologue"
        || heading.starts_with("chapter ")
        || heading.starts_with("part ")
}

fn back_matter_reason(lines: &[&str], index: usize, line: &str) -> Option<&'static str> {
    match normalized_heading(line).as_str() {
        "bibliography" => Some("bibliography"),
        "references" => Some("references"),
        "works cited" => Some("works_cited"),
        "index" if looks_like_index_tail(lines.get(index + 1..).unwrap_or_default()) => {
            Some("index")
        }
        _ => None,
    }
}

fn looks_like_index_tail(lines: &[&str]) -> bool {
    let candidates: Vec<&str> = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .take(100)
        .collect();
    if candidates.len() < 5 {
        return false;
    }
    let index_entries = candidates
        .iter()
        .filter(|line| is_index_entry(line))
        .count();
    index_entries * 2 >= candidates.len()
}

fn is_index_entry(line: &str) -> bool {
    line.len() < 120 && line.matches(',').count() >= 2 && {
        let numbers = line.rsplit(',').take(3);
        numbers
            .filter(|part| part.trim().parse::<usize>().is_ok())
            .count()
            >= 2
    }
}

/// Classify a single page as boilerplate (title, TOC, index, copyright, blank)
/// or content. Returns true if the page should be dropped.
#[cfg(test)]
fn is_boilerplate_page(page: &str) -> bool {
    boilerplate_page_reason(page).is_some()
}

fn boilerplate_page_reason(page: &str) -> Option<&'static str> {
    let trimmed = page.trim();
    if trimmed.is_empty() {
        return Some("blank");
    }
    let char_count = trimmed.chars().count();
    let lower = trimmed.to_lowercase();

    if char_count < 50 {
        return Some("title_or_short_page");
    }
    if char_count > 3000 {
        return None;
    }
    if char_count < 2000
        && (lower.contains("all rights reserved")
            || lower.contains("copyright")
            || lower.contains("printed in")
            || lower.contains("isbn"))
    {
        return Some("copyright");
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let dot_leader_count = lines
        .iter()
        .filter(|line| {
            let line = line.trim();
            line.len() < 100 && (line.contains("...") || line.matches('.').count() > 5)
        })
        .count();
    if lines.len() > 3 && dot_leader_count as f64 / lines.len() as f64 > 0.4 {
        return Some("contents");
    }

    let toc_line_count = lines
        .iter()
        .filter(|line| {
            let line = line.trim();
            line.len() < 100
                && (line.starts_with(|character: char| character.is_ascii_digit())
                    || line.starts_with("Chapter"))
                && line.chars().any(|character| character.is_ascii_digit())
                && line.contains('.')
                && line.split_whitespace().count() < 12
        })
        .count();
    if lines.len() > 5 && toc_line_count as f64 / lines.len() as f64 > 0.6 {
        return Some("contents");
    }

    let index_entry_count = lines
        .iter()
        .filter(|line| is_index_entry(line.trim()))
        .count();
    if lines.len() > 5 && index_entry_count as f64 / lines.len() as f64 > 0.5 {
        return Some("index");
    }
    if lower.starts_with("contents") && char_count < 2000 {
        return Some("contents");
    }
    if lower.starts_with("index") && char_count < 2000 {
        return Some("index");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: [P3] Zero overlap obeys the same word bound and preserves every source word through either public entry point.
    #[test]
    fn zero_overlap_obeys_current_bounds_and_rejects_invalid_budgets() -> anyhow::Result<()> {
        let text = "one two three four five six. seven eight";
        for chunks in [
            chunk_text(text, "test", 1, 5, ".!?"),
            chunk_text_with_overlap(text, "test", 1, 5, ".!?", 0)?,
        ] {
            assert!(
                chunks
                    .iter()
                    .all(|(_, text)| text.split_whitespace().count() <= 5)
            );
            assert_eq!(
                chunks
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                text
            );
        }
        for (max, overlap) in [(0, 0), (2, 2), (2, 3)] {
            assert!(chunk_text_with_overlap("", "test", 1, max, ".!?", overlap).is_err());
        }
        Ok(())
    }

    /// expect: [P3] Word windows reconstruct the entire sanitized source for every valid small overlap and budget, including near-full overlap.
    #[test]
    fn overlapping_windows_reconstruct_across_structural_boundaries() -> anyhow::Result<()> {
        let text = "# Header\nλ1 λ2. λ3 \"λ4!\"\n\nλ5 λ6\n---\nλ7 λ8 λ9. λ10\tλ11 λ12 λ13 λ14 λ15";
        let expected: Vec<_> = text.split_whitespace().collect();
        for max in 1..=20 {
            for overlap in 0..max {
                let chunks = chunk_text_with_overlap(text, "t", 1, max, ".!?", overlap)?;
                let mut reconstructed: Vec<&str> = Vec::new();
                for (index, (_, passage)) in chunks.iter().enumerate() {
                    let words: Vec<_> = passage.split_whitespace().collect();
                    let repeated = if index == 0 { 0 } else { overlap };
                    assert!(words.len() <= max && words.len() > repeated);
                    if repeated > 0 {
                        assert_eq!(
                            &reconstructed[reconstructed.len() - repeated..],
                            &words[..repeated]
                        );
                    }
                    reconstructed.extend(&words[repeated..]);
                }
                assert_eq!(reconstructed, expected);
            }
        }
        Ok(())
    }

    /// expect: [P3] Sentence ends and heading boundaries are preferred when they fit and still advance the window.
    #[test]
    fn overlapping_windows_prefer_source_boundaries() -> anyhow::Result<()> {
        let sentences = chunk_text_with_overlap(
            "alpha beta. gamma delta epsilon zeta eta theta iota",
            "t",
            1,
            5,
            ".!?",
            1,
        )?;
        assert_eq!(sentences[0].1, "alpha beta.");
        let structure = chunk_text_with_overlap(
            "alpha beta gamma\n\n# Heading\ndelta epsilon zeta eta theta",
            "t",
            1,
            6,
            ".!?",
            1,
        )?;
        assert_eq!(structure[0].1, "alpha beta gamma # Heading");
        Ok(())
    }

    #[test]
    fn sanitize_replaces_control_chars_with_space() {
        let input = "hello\x01world\x06test\x0eend";
        assert_eq!(sanitize_text(input), "hello world test end");
    }

    /// A whole document that MENTIONS "copyright" (an OCR'd book, a
    /// Project Gutenberg text) is a single form-feed-free "page" to
    /// `filter_boilerplate_pages`. The keyword rule must be size-gated or
    /// the entire document is silently classified as one giant boilerplate
    /// page — observed live: 17 of 138 corpus sources vanished from a chunk
    /// run while the tool reported total_documents=138.
    #[test]
    fn whole_document_mentioning_copyright_is_not_boilerplate() {
        let body = "The hedgehog knows one big thing. ".repeat(200); // ~6K chars
        let document = format!("{body}All rights reserved by the publisher. {body}");
        let filtered = filter_boilerplate_pages(&document);
        assert!(
            !filtered.trim().is_empty(),
            "a whole document mentioning copyright must survive filtering"
        );
    }

    /// A whole OCR'd document is few very long lines of prose — each line
    /// carries >5 sentence periods, which the dot-leader TOC rule counted
    /// as "dot leaders" at a 41% line ratio, classifying entire books as
    /// tables of contents. The page-size ceiling must keep it.
    #[test]
    fn whole_ocr_document_with_long_prose_lines_is_not_boilerplate() {
        let page = "This is a sentence that ends with a period. ".repeat(40); // ~1.8K chars
        let document = format!("{page}\n{page}\n{page}"); // ~5.4K chars, 3 long lines
        let filtered = filter_boilerplate_pages(&document);
        assert!(
            !filtered.trim().is_empty(),
            "a whole OCR'd document of long prose lines must survive filtering"
        );
    }

    /// Below-floor fragments must merge into the next passage instead of
    /// standing alone as tiny chunks, and no content may be dropped along
    /// the way. Pins two defects: standalone 1-word chunks (observed in a
    /// 32K-chunk corpus run) and the split-loop `buffer.clear()` that
    /// silently discarded a sub-floor buffer when an oversized paragraph
    /// entered the loop.
    #[test]
    fn below_floor_fragments_merge_and_no_content_is_dropped() {
        // The structural boundary at word 130 must not force a 30-word
        // mid-document chunk. The next window carries those words forward.
        let para1: Vec<String> = (0..130).map(|i| format!("alpha{i}")).collect();
        let para2: Vec<String> = (0..120).map(|i| format!("beta{i}")).collect();
        let text = format!("{}\n\n{}", para1.join(" "), para2.join(" "));
        let passages = chunk_text(&text, "t", 50, 100, ".!? ");

        let total: usize = passages
            .iter()
            .map(|(_, passage)| passage.split_whitespace().count())
            .sum();
        assert_eq!(total, 250, "every input word must survive chunking");

        // No mid-document passage below the floor — only the final remainder
        // may carry one.
        for (index, (entity_ref, passage)) in passages.iter().enumerate() {
            let words = passage.split_whitespace().count();
            if index < passages.len() - 1 {
                assert!(
                    words >= 50,
                    "mid-document passage below the floor: {entity_ref} ({words} words)"
                );
            }
        }

        // The 30-word fragment must have merged into the next passage, not
        // stand alone.
        let merged = passages
            .iter()
            .any(|(_, passage)| passage.contains("alpha129") && passage.contains("beta0"));
        assert!(
            merged,
            "the below-floor fragment must merge into the following passage"
        );
    }

    /// A real copyright/colophon page is small — the size gate must still
    /// drop it.
    #[test]
    fn small_copyright_page_is_still_dropped() {
        let page = "Copyright © 2005 Vigyan Prasar. All rights reserved. ISBN 81-7480-1234-5. Printed in India.";
        let filtered = filter_boilerplate_pages(page);
        assert!(
            filtered.trim().is_empty(),
            "a small copyright page must still be classified as boilerplate"
        );
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

    /// expect: I can exclude front matter and a trailing bibliography from a
    /// form-feed-free book without deleting its substantive body.
    /// [P3] Motivating: Generative Space — downstream corpus stages receive content, not book furniture.
    /// [P1] Constraining: Human Agency — every removal carries a reviewable reason and boundary.
    /// [P2] Constraining: Cognitive Sovereignty — prose mentions do not become deletion commands.
    /// pre: the document has explicit front/back section headings and substantive body text
    /// post: only bounded front/back sections are removed and every removed range is reported
    #[test]
    fn unpaged_book_filters_bounded_front_and_back_matter_with_report() {
        let body = "This chapter contains substantive analysis and evidence. ".repeat(80);
        let document = format!(
            "A Useful Book\nJane Author\nCopyright 2026 Example Press\nAll rights reserved\n\nContents\nChapter 1 .... 1\nChapter 2 .... 25\n\nChapter 1\n{body}\nBibliography\nSmith, A. Example Work.\nJones, B. Another Work."
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.starts_with("Chapter 1\n"));
        assert!(result.text.contains("substantive analysis"));
        assert!(!result.text.contains("A Useful Book"));
        assert!(!result.text.contains("Contents"));
        assert!(!result.text.contains("Bibliography"));
        assert_eq!(result.input_words, document.split_whitespace().count());
        assert_eq!(
            result.retained_words,
            result.text.split_whitespace().count()
        );
        assert_eq!(result.exclusions.len(), 2);
        assert_eq!(result.exclusions[0].reason, "front_matter");
        assert_eq!(result.exclusions[0].boundary_unit, "line");
        assert_eq!(result.exclusions[1].reason, "bibliography");
        assert!(result.exclusions.iter().all(|item| item.removed_words > 0));
    }

    /// expect: I keep substantive prose that merely discusses bibliographies or indices.
    /// [P3] Motivating: Generative Space — content survives unless it crosses an explicit bounded section gate.
    /// [P2] Constraining: Cognitive Sovereignty — keyword mentions cannot silently erase a document.
    /// pre: a long form-feed-free document contains bibliography/index words only inside prose
    /// post: the complete document is retained and no exclusion is reported
    #[test]
    fn unpaged_prose_mentions_do_not_trigger_section_removal() {
        let document = "The author discusses how a bibliography supports inquiry and how an index helps readers navigate evidence. ".repeat(100);

        let result = filter_boilerplate_pages_with_report(&document);

        assert_eq!(result.text, document.trim());
        assert!(result.exclusions.is_empty());
    }

    /// expect: I remove a trailing index only when its entries prove that the heading is structural.
    /// [P3] Motivating: Generative Space — navigation entries do not become retrieval passages.
    /// [P2] Constraining: Cognitive Sovereignty — an ordinary section named Index remains content.
    /// pre: an Index heading occurs in the final third of a form-feed-free document
    /// post: structured index entries are removed while prose under the same heading is retained
    #[test]
    fn unpaged_index_requires_index_entry_structure() {
        let body = "Substantive chapter evidence remains available. ".repeat(120);
        let structured = format!(
            "Chapter 1\n{body}\nIndex\nalpha, 1, 2\nbeta, 3, 4\ngamma, 5, 6\ndelta, 7, 8\nepsilon, 9, 10"
        );
        let narrative = format!(
            "Chapter 1\n{body}\nIndex\nThis section explains how an index supports navigation without presenting index entries."
        );

        let filtered = filter_boilerplate_pages_with_report(&structured);
        assert!(!filtered.text.contains("alpha, 1, 2"));
        assert_eq!(filtered.exclusions.len(), 1);
        assert_eq!(filtered.exclusions[0].reason, "index");

        let retained = filter_boilerplate_pages_with_report(&narrative);
        assert!(retained.text.contains("This section explains"));
        assert!(retained.exclusions.is_empty());
    }
}
