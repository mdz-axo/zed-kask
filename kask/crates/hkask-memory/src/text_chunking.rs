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

/// Validated word-window parameters shared by corpus and turn-memory callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkConfig<'a> {
    min_words: usize,
    max_words: usize,
    overlap_words: usize,
    sentence_boundary: &'a str,
}

impl<'a> ChunkConfig<'a> {
    /// Build a valid chunk contract before processing source text.
    pub fn new(
        min_words: usize,
        max_words: usize,
        overlap_words: usize,
        sentence_boundary: &'a str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(min_words > 0, "min_words must be positive");
        anyhow::ensure!(
            max_words >= min_words,
            "max_words must be at least min_words"
        );
        anyhow::ensure!(
            overlap_words < max_words,
            "overlap_words must be less than max_words"
        );
        Ok(Self {
            min_words,
            max_words,
            overlap_words,
            sentence_boundary,
        })
    }
}

/// One typed passage emitted by the shared chunk contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChunk {
    pub entity_ref: String,
    pub text: String,
}

/// Deterministic accounting for one chunking operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkingReport {
    pub source_words: usize,
    pub emitted_words: usize,
    pub overlap_words: usize,
    pub chunk_count: usize,
}

/// Typed chunks and their deterministic accounting report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkingOutput {
    pub chunks: Vec<TextChunk>,
    pub report: ChunkingReport,
}

/// Chunk text using a validated shared contract.
pub fn chunk_text_with_config(
    text: &str,
    entity_ref_prefix: &str,
    config: ChunkConfig<'_>,
) -> ChunkingOutput {
    let source_words = sanitize_text(text).split_whitespace().count();
    let chunks: Vec<TextChunk> = chunk_windows(
        text,
        entity_ref_prefix,
        config.min_words,
        config.max_words,
        config.sentence_boundary,
        config.overlap_words,
    )
    .into_iter()
    .map(|(entity_ref, text)| TextChunk { entity_ref, text })
    .collect();
    let emitted_words = chunks
        .iter()
        .map(|chunk| chunk.text.split_whitespace().count())
        .sum();
    let report = ChunkingReport {
        source_words,
        emitted_words,
        overlap_words: config.overlap_words,
        chunk_count: chunks.len(),
    };
    ChunkingOutput { chunks, report }
}

/// Chunk text into passages for embedding.
///
/// No-overlap entry point used by turn-memory ingestion. The shared window
/// engine sanitizes control characters and prefers structural/sentence ends
/// within the word budget. Short structural fragments are carried forward
/// until min_words is reached; the final remainder is always retained.
///
/// # Panics
/// Panics if `min_words == 0` or `max_words < min_words` (programmer errors
/// in this infallible compatibility API). Caller-controlled budgets should use
/// [`ChunkConfig::new`].
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
    let config = ChunkConfig::new(min_words, max_words, 0, sentence_boundary)
        .expect("chunk_text requires a valid min/max word budget");
    chunk_text_with_config(text, entity_ref_prefix, config)
        .chunks
        .into_iter()
        .map(|chunk| (chunk.entity_ref, chunk.text))
        .collect()
}

/// Chunk with explicit repeated context, measured in whitespace-delimited words.
///
/// expect: "Every overlapping passage repeats context and contributes new source words."
/// [P3] Motivating: Generative Space — retain source content for retrieval.
/// [P4] Constraining: bounded, advancing windows; no model-token guarantee.
/// pre: min_words > 0, max_words >= min_words, overlap_words < max_words
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
    let config = ChunkConfig::new(min_words, max_words, overlap_words, sentence_boundary)?;
    Ok(chunk_text_with_config(text, entity_ref_prefix, config)
        .chunks
        .into_iter()
        .map(|chunk| (chunk.entity_ref, chunk.text))
        .collect())
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

fn is_ocr_figure_destination(destination: &str) -> bool {
    let Some(coordinates) = destination
        .strip_prefix("page_")
        .and_then(|value| value.strip_suffix(".png"))
    else {
        return false;
    };
    let mut parts = coordinates.split('_');
    (0..4).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
    }) && parts.next().is_none()
}

fn strip_newsletter_calls_to_action(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    const START: &str = "Thanks for reading ";
    const END: &str = "Subscribe for free to receive new posts and support my work.";

    let mut output = String::with_capacity(text.len());
    let mut exclusions = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = text[cursor..].find(START) {
        let start = cursor + relative_start;
        let Some(relative_end) = text[start..].find(END) else {
            break;
        };
        let end = start + relative_end + END.len();
        output.push_str(&text[cursor..start]);
        output.push(' ');
        exclusions.push(BoilerplateExclusion {
            reason: "promotional_call_to_action",
            boundary_unit: "byte",
            start,
            end,
            removed_words: text[start..end].split_whitespace().count(),
        });
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    (output, exclusions)
}

fn strip_pdf_distribution_watermarks(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    const MARKER: &str = "OceanofPDF.com";

    let mut output = String::with_capacity(text.len());
    let mut exclusions = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = text[cursor..].find(MARKER) {
        let start = cursor + relative_start;
        let marker_end = start + MARKER.len();
        let remainder = &text[marker_end..];
        let whitespace_bytes = remainder
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(remainder.len(), |(index, _)| index);
        let after_whitespace = &remainder[whitespace_bytes..];
        let end = after_whitespace
            .strip_prefix("Page ")
            .map(|after_page| {
                marker_end
                    + whitespace_bytes
                    + "Page ".len()
                    + after_page
                        .chars()
                        .take_while(|character| character.is_ascii_digit())
                        .map(char::len_utf8)
                        .sum::<usize>()
            })
            .filter(|end| *end > marker_end + whitespace_bytes + "Page ".len())
            .unwrap_or(marker_end);
        output.push_str(&text[cursor..start]);
        output.push(' ');
        exclusions.push(BoilerplateExclusion {
            reason: "distribution_watermark",
            boundary_unit: "byte",
            start,
            end,
            removed_words: text[start..end].split_whitespace().count(),
        });
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    (output, exclusions)
}

const PUBLICATION_CHROME_MARKERS: &[&str] = &[
    "Share Merchant Adventures",
    "Subscribe now",
    "Leave a comment",
    "Play in Reduct",
];
const REPEATED_FORMATTING_TOKEN: &str = "\\qquad";
const REPEATED_FORMATTING_MIN_OCCURRENCES: usize = 8;

fn strip_publication_chrome(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let mut filtered = text.to_string();
    let mut exclusions = Vec::new();
    for marker in PUBLICATION_CHROME_MARKERS {
        let mut output = String::with_capacity(filtered.len());
        let mut cursor = 0;
        while let Some(relative_start) = filtered[cursor..].find(marker) {
            let start = cursor + relative_start;
            let end = start + marker.len();
            output.push_str(&filtered[cursor..start]);
            output.push(' ');
            exclusions.push(BoilerplateExclusion {
                reason: "publication_chrome",
                boundary_unit: "byte",
                start,
                end,
                removed_words: marker.split_whitespace().count(),
            });
            cursor = end;
        }
        output.push_str(&filtered[cursor..]);
        filtered = output;
    }
    (filtered, exclusions)
}

fn repeated_formatting_dominates(text: &str) -> bool {
    let occurrences = text.matches(REPEATED_FORMATTING_TOKEN).count();
    occurrences >= REPEATED_FORMATTING_MIN_OCCURRENCES
        && occurrences * 2 >= text.split_whitespace().count()
}

fn strip_repeated_formatting_lines(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let mut output = String::with_capacity(text.len());
    let mut exclusions = Vec::new();
    let mut cursor = 0;
    for segment in text.split_inclusive('\n') {
        let end = cursor + segment.len();
        if repeated_formatting_dominates(segment) {
            exclusions.push(BoilerplateExclusion {
                reason: "formatting_artifact",
                boundary_unit: "byte",
                start: cursor,
                end,
                removed_words: segment.split_whitespace().count(),
            });
        } else {
            output.push_str(segment);
        }
        cursor = end;
    }
    (output, exclusions)
}

/// Return canonical boilerplate signals that remain after filtering.
///
/// expect: I can fail a corpus build before embedding if known watermark,
/// publication-chrome, blank-page, or repeated-layout artifacts survive.
/// [P4] Motivating: Transparent Imperfection — a dirty retained view fails visibly.
/// [P1] Constraining: Human Agency — the signal names the retained artifact.
/// pre: text is the canonical retained source view
/// post: every returned label names a still-present ineligible artifact class
pub fn retained_boilerplate_signals(text: &str) -> Vec<&'static str> {
    let mut signals = Vec::new();
    for (label, marker) in [
        ("distribution_watermark", "OceanofPDF.com"),
        ("newsletter_call_to_action", "Thanks for reading "),
        (
            "newsletter_call_to_action",
            "Subscribe for free to receive new posts and support my work.",
        ),
        (
            "intentionally_blank_page",
            "This page intentionally left blank",
        ),
    ] {
        if text.contains(marker) && !signals.contains(&label) {
            signals.push(label);
        }
    }
    if PUBLICATION_CHROME_MARKERS
        .iter()
        .any(|marker| text.contains(marker))
    {
        signals.push("publication_chrome");
    }
    if text.lines().any(repeated_formatting_dominates) {
        signals.push("formatting_artifact");
    }
    signals
}

fn strip_markdown_images(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let mut output = String::with_capacity(text.len());
    let mut exclusions = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = text[cursor..].find("![") {
        let start = cursor + relative_start;
        let label_start = start + 2;
        let Some(relative_separator) = text[label_start..].find("](") else {
            break;
        };
        if text[label_start..label_start + relative_separator].contains("![") {
            output.push_str(&text[cursor..label_start]);
            cursor = label_start;
            continue;
        }
        let destination_start = label_start + relative_separator + 2;
        let Some(relative_end) = text[destination_start..].find(')') else {
            break;
        };
        let end = destination_start + relative_end + 1;
        let destination = &text[destination_start..end - 1];

        if is_ocr_figure_destination(destination) {
            output.push_str(&text[cursor..start]);
            output.push(' ');
            exclusions.push(BoilerplateExclusion {
                reason: "model_inference_image",
                boundary_unit: "byte",
                start,
                end,
                removed_words: text[start..end].split_whitespace().count(),
            });
        } else {
            output.push_str(&text[cursor..end]);
        }
        cursor = end;
    }

    output.push_str(&text[cursor..]);
    (output, exclusions)
}

/// expect: I can remove bounded book furniture and promotional calls to action while retaining substantive source content and reviewing every exclusion.
/// [P3] Motivating: Generative Space — downstream corpus stages receive content rather than front/back matter.
/// [P1] Constraining: Human Agency — every removal carries a reason and source boundary.
/// [P2] Constraining: Cognitive Sovereignty — prose mentions never act as deletion commands.
/// pre: text is valid UTF-8 and may be page-delimited or a form-feed-free extraction
/// post: removes only structurally bounded furniture before a detected body or bounded promotional spans, retaining source text plus complete word-count and exclusion-range accounting
pub fn filter_boilerplate_pages_with_report(text: &str) -> BoilerplateFilterResult {
    let input_words = text.split_whitespace().count();
    let (filtered, mut exclusions) = if text.contains(FORM_FEED) {
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
    let (filtered, promotional_exclusions) = strip_newsletter_calls_to_action(&filtered);
    exclusions.extend(promotional_exclusions);
    let (filtered, watermark_exclusions) = strip_pdf_distribution_watermarks(&filtered);
    exclusions.extend(watermark_exclusions);
    let (filtered, chrome_exclusions) = strip_publication_chrome(&filtered);
    exclusions.extend(chrome_exclusions);
    let (filtered, formatting_exclusions) = strip_repeated_formatting_lines(&filtered);
    exclusions.extend(formatting_exclusions);
    let (filtered, image_exclusions) = strip_markdown_images(&filtered);
    exclusions.extend(image_exclusions);
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
    let pages = text.split(FORM_FEED).collect::<Vec<_>>();
    let front_end = bounded_front_page_end(&pages);
    let mut kept = Vec::new();
    let mut exclusions = Vec::new();
    for (page_index, page) in pages.into_iter().enumerate() {
        let reason = if page_index < front_end && !page.trim().is_empty() {
            Some("front_matter")
        } else {
            boilerplate_page_reason(page)
        };
        if let Some(reason) = reason {
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

fn bounded_front_page_end(pages: &[&str]) -> usize {
    let Some(body_start) = pages.iter().position(|page| is_substantive_page(page)) else {
        return 0;
    };
    if body_start == 0 {
        return 0;
    }
    let total_words = pages
        .iter()
        .flat_map(|page| page.split_whitespace())
        .count();
    let candidate_words = pages
        .get(..body_start)
        .unwrap_or_default()
        .iter()
        .flat_map(|page| page.split_whitespace())
        .count();
    if candidate_words.saturating_mul(20) <= total_words {
        body_start
    } else {
        0
    }
}

fn is_substantive_page(page: &str) -> bool {
    if boilerplate_page_reason(page).is_some() {
        return false;
    }
    let trimmed = page.trim();
    let first_heading = trimmed
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(normalized_heading)
        .unwrap_or_default();
    if is_front_furniture_heading(&first_heading) {
        return false;
    }
    let word_count = trimmed.split_whitespace().count();
    let sentence_ends = trimmed
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?'))
        .count();
    (word_count >= 40 && sentence_ends >= 2)
        || (word_count >= 20 && is_body_start_heading(&first_heading))
}

fn is_front_furniture_heading(heading: &str) -> bool {
    [
        "praise for",
        "advance praise",
        "other books by",
        "also by",
        "books by",
        "sign up",
        "subscribe",
        "join our mailing list",
    ]
    .iter()
    .any(|prefix| heading.starts_with(prefix))
}

fn filter_unpaged_boilerplate(text: &str) -> (String, Vec<BoilerplateExclusion>) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return (String::new(), Vec::new());
    }

    let front_search_end = lines.len().min(400);
    let front_end = front_matter_end(&lines, front_search_end);

    let back_search_start = word_fraction_line_index(&lines, 2, 3).max(front_end);
    let back_boundary = terminal_back_boundary(&lines, back_search_start);
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

fn front_matter_end(lines: &[&str], search_end: usize) -> usize {
    let contents_signal = lines
        .iter()
        .take(search_end)
        .enumerate()
        .filter(|(_, line)| is_contents_heading(line))
        .map(|(index, _)| index)
        .next_back();
    let metadata_signal = lines
        .iter()
        .take(search_end)
        .enumerate()
        .filter(|(_, line)| is_front_matter_signal(line))
        .map(|(index, _)| index)
        .next_back();
    let signal = contents_signal.or(metadata_signal);
    let Some(signal) = signal else {
        return 0;
    };

    let prose_start = lines
        .iter()
        .enumerate()
        .take(search_end)
        .skip(signal + 1)
        .find(|(_, line)| is_substantive_prose_line(line))
        .map(|(index, _)| index);
    let Some(prose_start) = prose_start else {
        return 0;
    };

    let candidate = (signal + 1..prose_start)
        .rev()
        .find(|index| {
            lines
                .get(*index)
                .is_some_and(|line| !line.trim().is_empty())
        })
        .filter(|index| {
            lines.get(*index).is_some_and(|line| {
                is_body_start_heading(line)
                    || (!is_toc_like_line(line) && line.split_whitespace().count() <= 12)
            })
        })
        .unwrap_or(prose_start);
    let total_words = lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .count();
    let candidate_words = lines
        .get(..candidate)
        .unwrap_or_default()
        .iter()
        .flat_map(|line| line.split_whitespace())
        .count();
    if candidate_words.saturating_mul(20) > total_words {
        0
    } else {
        candidate
    }
}

fn is_contents_heading(line: &str) -> bool {
    matches!(
        normalized_heading(line).as_str(),
        "contents" | "table of contents"
    )
}

fn is_front_matter_signal(line: &str) -> bool {
    let heading = normalized_heading(line);
    is_contents_heading(line)
        || heading.contains("copyright")
        || heading.contains("all rights reserved")
        || heading.contains("isbn")
        || heading.contains("printed in")
}

fn is_substantive_prose_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.split_whitespace().count() >= 8 && !is_toc_like_line(trimmed)
}

fn is_toc_like_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains("...")
        || (trimmed.split_whitespace().count() < 20
            && trimmed
                .split_whitespace()
                .last()
                .map(|word| word.trim_matches(|character: char| !character.is_ascii_digit()))
                .is_some_and(|word| !word.is_empty() && word.parse::<usize>().is_ok()))
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
    heading.starts_with("preface")
        || heading.starts_with("foreword")
        || heading.starts_with("introduction")
        || heading.starts_with("prologue")
        || heading.starts_with("chapter ")
        || heading.starts_with("part ")
}

fn terminal_back_boundary(lines: &[&str], search_start: usize) -> Option<(usize, &'static str)> {
    let candidates = lines
        .iter()
        .enumerate()
        .skip(search_start)
        .filter_map(|(index, line)| {
            back_matter_reason(lines, index, line).map(|reason| (index, reason))
        })
        .collect::<Vec<_>>();

    candidates
        .iter()
        .filter(|(index, reason)| {
            !candidates
                .iter()
                .any(|(later_index, later_reason)| later_reason == reason && later_index > index)
        })
        .min_by_key(|(index, _)| *index)
        .copied()
}

fn back_matter_reason(lines: &[&str], index: usize, line: &str) -> Option<&'static str> {
    let tail = lines.get(index + 1..).unwrap_or_default();
    match normalized_heading(line).as_str() {
        "bibliography" if looks_like_reference_tail(tail) => Some("bibliography"),
        "references" if looks_like_reference_tail(tail) => Some("references"),
        "works cited" if looks_like_reference_tail(tail) => Some("works_cited"),
        "index" if looks_like_index_tail(tail) => Some("index"),
        _ => None,
    }
}

fn looks_like_reference_tail(lines: &[&str]) -> bool {
    let candidates = lines
        .iter()
        .map(|line| line.trim())
        .take_while(|line| !is_following_section_heading(line))
        .filter(|line| !line.is_empty())
        .take(100)
        .collect::<Vec<_>>();
    if candidates.len() < 2 {
        return false;
    }
    let reference_entries = candidates
        .iter()
        .filter(|line| is_reference_entry(line))
        .count();
    reference_entries * 4 >= candidates.len()
}

fn is_following_section_heading(line: &str) -> bool {
    matches!(
        normalized_heading(line).as_str(),
        "bibliography" | "references" | "works cited" | "index"
    ) || is_body_start_heading(line)
}

fn is_reference_entry(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("doi:")
        || lower.contains("doi.org")
        || lower.contains("http://")
        || lower.contains("https://")
        || line
            .split(|character: char| !character.is_ascii_digit())
            .filter(|part| part.len() == 4)
            .filter_map(|part| part.parse::<u16>().ok())
            .any(|year| (1800..=2099).contains(&year))
        || line
            .split_whitespace()
            .next()
            .map(|first| first.trim_matches(|character: char| !character.is_ascii_digit()))
            .is_some_and(|first| !first.is_empty() && first.parse::<usize>().is_ok())
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
    let word_count = trimmed.split_whitespace().count();
    let line_count = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    if word_count < 50
        && line_count <= 3
        && ["fig. ", "figure ", "table "]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
    {
        return Some("isolated_caption");
    }
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

    /// expect: [P3] A validated zero-overlap contract returns typed chunks at the exact word ceiling and reports lossless source reconstruction.
    #[test]
    fn typed_zero_overlap_chunks_obey_exact_bounds_and_reconstruct() -> anyhow::Result<()> {
        let text = "one two three four five six. seven eight nine ten";
        let config = ChunkConfig::new(3, 5, 0, ".!?")?;
        let output = chunk_text_with_config(text, "test", config);

        assert_eq!(output.report.source_words, 10);
        assert_eq!(output.report.emitted_words, 10);
        assert_eq!(output.report.overlap_words, 0);
        assert_eq!(output.report.chunk_count, output.chunks.len());
        assert!(
            output
                .chunks
                .iter()
                .all(|chunk| chunk.text.split_whitespace().count() <= 5)
        );
        assert_eq!(
            output
                .chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            text
        );
        assert_eq!(output.chunks[0].entity_ref, "test:0");
        Ok(())
    }

    /// expect: [P3] Invalid minimum, maximum, and overlap relationships are rejected before chunking.
    #[test]
    fn chunk_config_validates_min_max_and_overlap() {
        assert!(ChunkConfig::new(0, 5, 0, ".!?").is_err());
        assert!(ChunkConfig::new(6, 5, 0, ".!?").is_err());
        assert!(ChunkConfig::new(1, 0, 0, ".!?").is_err());
        assert!(ChunkConfig::new(1, 2, 2, ".!?").is_err());
        assert!(ChunkConfig::new(1, 2, 3, ".!?").is_err());
    }

    /// expect: [P3] Existing tuple callers retain zero-overlap bounds and reconstruction behavior.
    #[test]
    fn compatibility_entry_points_preserve_tuple_behavior() -> anyhow::Result<()> {
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

    /// expect: I can retrieve the article around a newsletter sign-up block without learning the promotion as source knowledge.
    /// [P3] Motivating: Generative Space — article evidence remains available without promotional contamination.
    /// [P1] Constraining: Human Agency — the exact removed span is reviewable.
    /// [P2] Constraining: Cognitive Sovereignty — neighboring source prose is unchanged.
    /// pre: an exact bounded newsletter call-to-action occurs between substantive source passages
    /// post: only the promotional span is removed and reported as a byte range
    #[test]
    fn filter_removes_bounded_newsletter_cta_between_source_prose() {
        let before =
            "The research explains why feedback improves metacognitive accuracy. ".repeat(20);
        let promotion = "Thanks for reading Merchant Adventures! Subscribe for free to receive new posts and support my work.";
        let after =
            "The following section applies that evidence to forecasting practice. ".repeat(20);
        let document = format!("{before}{promotion} {after}");

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("metacognitive accuracy"));
        assert!(result.text.contains("forecasting practice"));
        assert!(!result.text.contains("Thanks for reading"));
        assert!(!result.text.contains("Subscribe for free"));
        let promotional = result
            .exclusions
            .iter()
            .filter(|exclusion| exclusion.reason == "promotional_call_to_action")
            .collect::<Vec<_>>();
        assert_eq!(promotional.len(), 1);
        assert_eq!(promotional[0].boundary_unit, "byte");
        assert!(promotional[0].end > promotional[0].start);
        assert_eq!(
            promotional[0].removed_words,
            promotion.split_whitespace().count()
        );
    }

    /// expect: I can retrieve substantive page text without distribution watermarks becoming remembered evidence.
    /// [P3] Motivating: Generative Space — retrieval context contains the work rather than a distributor marker.
    /// [P1] Constraining: Human Agency — the removed watermark remains reviewable as a byte range.
    /// [P2] Constraining: Cognitive Sovereignty — surrounding source prose is retained exactly.
    /// pre: an OceanofPDF page watermark precedes substantive prose
    /// post: the bounded marker and page number are removed while all surrounding prose remains
    #[test]
    fn filter_removes_bounded_pdf_distribution_watermark() {
        let document = "OceanofPDF.com Page 160 One way or another, each loyalty leader built a foundation of stable ownership that freed management to attend to long-term value creation.";

        let result = filter_boilerplate_pages_with_report(document);

        assert!(!result.text.contains("OceanofPDF.com"));
        assert!(!result.text.contains("Page 160"));
        assert!(result.text.contains("each loyalty leader"));
        let watermark = result
            .exclusions
            .iter()
            .find(|exclusion| exclusion.reason == "distribution_watermark")
            .expect("watermark exclusion");
        assert_eq!(watermark.boundary_unit, "byte");
        assert_eq!(watermark.removed_words, 3);
    }

    /// expect: I can retrieve article prose without embedded publication chrome becoming remembered evidence.
    #[test]
    fn filter_removes_inline_publication_chrome_without_touching_prose() {
        let before =
            "Substantive analysis before the publication controls remains source evidence. "
                .repeat(20);
        let after = "Substantive analysis after the publication controls remains source evidence. "
            .repeat(20);
        let document = format!(
            "{before}Share Merchant Adventures Subscribe now Leave a comment Play in Reduct {after}"
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("analysis before"));
        assert!(result.text.contains("analysis after"));
        for marker in [
            "Share Merchant Adventures",
            "Subscribe now",
            "Leave a comment",
            "Play in Reduct",
        ] {
            assert!(!result.text.contains(marker), "retained marker: {marker}");
        }
        assert_eq!(
            result
                .exclusions
                .iter()
                .filter(|exclusion| exclusion.reason == "publication_chrome")
                .count(),
            4
        );
    }

    /// expect: I can retrieve surrounding financial prose without a repeated LaTeX-layout artifact becoming passages.
    #[test]
    fn filter_removes_qquad_dominated_lines_without_touching_prose() {
        let before =
            "Substantive financial analysis before the malformed layout line remains evidence. "
                .repeat(20);
        let after =
            "Substantive financial analysis after the malformed layout line remains evidence. "
                .repeat(20);
        let formatting_artifact = format!("\\uparrow {}\\q�", "\\qquad ".repeat(20));
        let document = format!("{before}\n{formatting_artifact}\n{after}");

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("analysis before"));
        assert!(result.text.contains("analysis after"));
        assert!(!result.text.contains("\\qquad"));
        assert_eq!(
            result
                .exclusions
                .iter()
                .filter(|exclusion| exclusion.reason == "formatting_artifact")
                .count(),
            1
        );
    }

    /// expect: valid financial equations and their explanatory prose remain source evidence.
    #[test]
    fn filter_preserves_qquad_in_prose_and_equations() {
        let prose =
            "Financial analysis explains the notation and preserves the working of the equation. "
                .repeat(20);
        let equation = format!("{prose} {} {prose}", "\\qquad ".repeat(8));
        let result = filter_boilerplate_pages_with_report(&equation);
        assert!(result.text.contains("\\qquad"));
        assert!(
            !result
                .exclusions
                .iter()
                .any(|exclusion| exclusion.reason == "formatting_artifact")
        );
        assert!(retained_boilerplate_signals(&result.text).is_empty());
    }

    /// expect: an Open Graph example and a prose mention of subscriptions are evidence, not publication controls.
    #[test]
    fn filter_preserves_html_example_and_subscription_discussion() {
        let passage =
            "An Open Graph example explains how structured data is published. ".repeat(20);
        let input = format!(
            "{passage}\n<meta property='og:type' content=\"video.movie\" />\n<meta name=\"title\" content=\"The Lion King (2019) – IMDb\" />\nFigure 15.2: An example of an OGP snippet embedded in HTML.\n{passage} A subscription can be discussed in substantive analysis."
        );
        let filtered = filter_boilerplate_pages_with_report(&input);
        assert!(filtered.text.contains("og:type"));
        assert!(filtered.text.contains("Figure 15.2"));
        assert!(filtered.text.contains("A subscription can be discussed"));
        assert!(retained_boilerplate_signals(&filtered.text).is_empty());
    }

    #[test]
    fn filter_removes_ocr_images_but_preserves_surrounding_source_text() {
        let prose = "Substantive source prose remains available for evidence\n".repeat(30);
        let input = format!(
            r#"{prose}<table><tr><th>Ho-Lee model: \( \mu = 0.005 \)</th></tr><tr><td>![Ten paths generated from Ho-Lee model](page_349_768_482_388.png)</td></tr></table> ![Literal Markdown](source.png) {prose}{FORM_FEED}{prose}"#
        );

        let filtered = filter_boilerplate_pages_with_report(&input);

        assert!(
            filtered
                .text
                .contains("<th>Ho-Lee model: \\( \\mu = 0.005 \\)</th>")
        );
        assert!(filtered.text.contains("Substantive source prose remains"));
        assert!(filtered.text.contains("![Literal Markdown](source.png)"));
        assert!(!filtered.text.contains("Ten paths generated"));
        assert!(!filtered.text.contains("page_349_768_482_388.png"));
        let image_exclusions: Vec<_> = filtered
            .exclusions
            .iter()
            .filter(|exclusion| exclusion.reason == "model_inference_image")
            .collect();
        assert_eq!(image_exclusions.len(), 1);
        assert_eq!(image_exclusions[0].boundary_unit, "byte");
        assert!(image_exclusions[0].removed_words > 0);
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

    /// expect: I receive a book's substantive opening rather than its title, publisher, and contents pages.
    /// [P3] Motivating: Generative Space — retrieval starts from source content instead of book furniture.
    /// [P1] Constraining: Human Agency — removed pages remain visible in the exclusion report.
    /// [P2] Constraining: Cognitive Sovereignty — removal stops at demonstrated substantive prose.
    /// pre: page-delimited front matter precedes a substantive opening within five percent of document words
    /// post: all bounded front pages are excluded with page identities and the substantive opening is retained
    #[test]
    fn page_delimited_book_filters_bounded_front_matter_with_report() {
        let title = "A PHILOSOPHY OF SOFTWARE DESIGN\nJOHN OUSTERHOUT";
        let publisher =
            "A Philosophy of Software Design\nJohn Ousterhout\nStanford University\nOceanofPDF.com";
        let contents = "Contents\nPreface\n1 Introduction\n1.1 How to use this book\n2 The Nature of Complexity";
        let body = "1 Introduction\nSoftware design is the process of decomposing a complex system into modules with interfaces that hide implementation details. Good design reduces the amount of information developers must hold in mind while making a change. This chapter develops that argument with concrete examples and explains why complexity accumulates over time. ".repeat(40);
        let document = [
            title.to_string(),
            publisher.to_string(),
            contents.to_string(),
            body,
        ]
        .join(&FORM_FEED.to_string());

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(!result.text.contains("JOHN OUSTERHOUT"));
        assert!(!result.text.contains("OceanofPDF.com"));
        assert!(!result.text.contains("Contents"));
        assert!(result.text.starts_with("1 Introduction"));
        assert!(result.text.contains("complex system into modules"));
        assert_eq!(result.exclusions.len(), 3);
        assert!(
            result
                .exclusions
                .iter()
                .enumerate()
                .all(|(index, exclusion)| {
                    exclusion.reason == "front_matter"
                        && exclusion.boundary_unit == "page"
                        && exclusion.start == index
                        && exclusion.end == index + 1
                        && exclusion.removed_words > 0
                })
        );
    }

    /// expect: I do not receive cover endorsements as the opening knowledge of a book.
    /// [P3] Motivating: Generative Space — retrieval begins with the work rather than promotional praise.
    /// [P1] Constraining: Human Agency — the removed praise page remains reviewable by page identity.
    /// [P2] Constraining: Cognitive Sovereignty — a following substantive page terminates front-matter removal.
    /// pre: a heading-marked praise page precedes substantive body prose within the bounded front section
    /// post: title and praise pages are excluded while body prose remains
    #[test]
    fn page_delimited_book_filters_heading_marked_promotional_praise() {
        let title = "SUPERFORECASTING\nThe Art and Science of Prediction";
        let praise = "PRAISE FOR SUPERFORECASTING\nThis remarkable book changes how readers understand prediction. It offers a compelling account of judgment and disciplined learning. The examples are vivid, practical, and memorable. Every decision maker should read this important work. Its methods will transform institutions, improve choices, and inspire careful readers throughout the world. —A Reviewer";
        let body = "Introduction\nForecasting skill can be measured when predictions are stated precisely and scored against outcomes. Teams improve by decomposing questions, updating estimates, and examining calibration over repeated judgments. This chapter explains the evidence for those practices and the limits of each result. ".repeat(40);
        let document = [title.to_string(), praise.to_string(), body].join(&FORM_FEED.to_string());

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(!result.text.contains("PRAISE FOR"));
        assert!(!result.text.contains("A Reviewer"));
        assert!(result.text.starts_with("Introduction"));
        assert_eq!(
            result
                .exclusions
                .iter()
                .filter(|exclusion| exclusion.reason == "front_matter")
                .count(),
            2
        );
    }

    /// expect: I do not receive an isolated figure caption as text knowledge when its figure is unavailable.
    /// [P3] Motivating: Generative Space — text retrieval remains grounded in substantive source passages.
    /// [P1] Constraining: Human Agency — the caption page is reported rather than silently lost.
    /// [P2] Constraining: Cognitive Sovereignty — neighboring prose pages remain available.
    /// pre: a short caption-only page occurs between substantive page-delimited prose
    /// post: the caption page is excluded and both prose pages are retained
    #[test]
    fn filter_excludes_isolated_caption_only_page() {
        let before = "The algorithm compares candidate solutions by dominance and diversity. This substantive discussion explains the decision process, its assumptions, and the resulting tradeoffs for optimization practice. ".repeat(20);
        let caption = "Fig. 6 The feasible decision variable and objective spaces for the TNK problem. This is a reprint of Fig. 3 from Deb et al. (2001).";
        let after = "The next section evaluates convergence behavior under several benchmark conditions. It reports the observed patterns and explains how those patterns affect interpretation of the method. ".repeat(20);
        let document = [before, caption.to_string(), after].join(&FORM_FEED.to_string());

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(
            result
                .text
                .contains("algorithm compares candidate solutions")
        );
        assert!(result.text.contains("next section evaluates convergence"));
        assert!(!result.text.contains("Fig. 6"));
        let exclusion = result
            .exclusions
            .iter()
            .find(|exclusion| exclusion.reason == "isolated_caption")
            .expect("caption exclusion");
        assert_eq!(exclusion.boundary_unit, "page");
        assert_eq!(exclusion.start, 1);
        assert_eq!(exclusion.end, 2);
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
            "A Useful Book\nJane Author\nCopyright 2026 Example Press\nAll rights reserved\n\nContents\nChapter 1 .... 1\nChapter 2 .... 25\n\nChapter 1\n{body}\nBibliography\nSmith, A. (2024). Example Work.\nJones, B. (2025). Another Work."
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

    /// expect: I keep a substantive introduction or preface even when its heading is not a bare canonical word.
    /// [P3] Motivating: Generative Space — introductory analysis remains available to retrieval.
    /// [P2] Constraining: Cognitive Sovereignty — front-matter detection stops when substantive prose begins.
    /// pre: metadata and a TOC precede a titled preface containing prose before Chapter 1
    /// post: metadata/TOC are removed while the preface heading and prose are retained
    #[test]
    fn unpaged_front_filter_preserves_titled_preface_prose() {
        let preface =
            "This revised preface explains the argument, evidence, and changes made for readers. "
                .repeat(30);
        let body = "The first chapter develops the central analysis. ".repeat(40);
        let document = format!(
            "A Useful Book\nJane Author\nCopyright 2026 Example Press\nAll rights reserved\n\nContents\nPreface .... ix\nChapter 1 .... 1\n\nPreface to the Revised Edition\n{preface}\nChapter 1\n{body}"
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.starts_with("Preface to the Revised Edition\n"));
        assert!(result.text.contains("This revised preface explains"));
        assert!(result.text.contains("Chapter 1"));
        assert!(!result.text.contains("All rights reserved"));
        assert!(!result.text.contains("Preface .... ix"));
    }

    /// expect: I never erase an earlier work because a multi-work volume contains a later Contents page.
    /// [P3] Motivating: Generative Space — substantive works remain available to retrieval.
    /// [P1] Constraining: Human Agency — unsafe front candidates remain visible in source text for review.
    /// pre: a candidate front boundary occurs after more than five percent of document words
    /// post: no front exclusion occurs and all preceding substantive content remains
    #[test]
    fn unpaged_front_filter_refuses_late_multiwork_contents() {
        let first_work =
            "The first work contains substantive biological and epistemological analysis. "
                .repeat(120);
        let second_work =
            "The second work continues with substantive cognition research. ".repeat(40);
        let document = format!(
            "Copyright 2026 Example Press\nContents\nFirst Work .... 1\n\nChapter 1\n{first_work}\nContents\nSecond Work .... 80\n\nI. Introduction\n{second_work}"
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("The first work contains"));
        assert!(result.text.contains("The second work continues"));
        assert!(result.exclusions.is_empty());
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

    /// expect: I keep chapters after chapter-level reference lists and remove only the terminal references section.
    /// [P3] Motivating: Generative Space — later chapters remain available for retrieval.
    /// [P2] Constraining: Cognitive Sovereignty — repeated section names cannot erase subsequent content.
    /// pre: multiple citation-like References headings occur after two-thirds of document words
    /// post: filtering starts at the final References heading and retains intervening chapters
    #[test]
    fn unpaged_repeated_references_use_the_terminal_boundary() {
        let body = "Substantive systems analysis remains available to readers. ".repeat(180);
        let later_body = "This later chapter must remain in the corpus. ".repeat(60);
        let document = format!(
            "Chapter 1\n{body}\nReferences\nAckoff, R. (1981). Creating the Corporate Future.\nBeer, S. (1972). Brain of the Firm.\nChapter 9\n{later_body}\nReferences\nSmith, A. (2024). Final Source.\nJones, B. (2025). Final Evidence."
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("Chapter 9"));
        assert!(result.text.contains("This later chapter"));
        assert!(!result.text.contains("Final Source"));
        assert_eq!(result.exclusions.len(), 1);
        assert_eq!(result.exclusions[0].reason, "references");
    }

    /// expect: I do not treat a table field named references as a terminal bibliography.
    /// [P3] Motivating: Generative Space — substantive content survives until a real citation section begins.
    /// [P2] Constraining: Cognitive Sovereignty — exact headings still require structural evidence.
    /// pre: a late false References heading precedes prose and a genuine citation-like Bibliography
    /// post: prose after the false heading remains and filtering starts at the genuine bibliography
    #[test]
    fn unpaged_reference_heading_requires_citation_structure() {
        let body = "Substantive ontology discussion establishes the document context. ".repeat(170);
        let later_body = "This material follows a table field and remains substantive. ".repeat(50);
        let document = format!(
            "Chapter 1\n{body}\nTerm\nReferences\nDatabase cross references identify related objects in other databases.\n{later_body}\nBibliography\nSmith, A. (2024). Ontology Source.\nJones, B. (2025). Knowledge Graph Evidence."
        );

        let result = filter_boilerplate_pages_with_report(&document);

        assert!(result.text.contains("Database cross references"));
        assert!(result.text.contains("This material follows"));
        assert!(!result.text.contains("Ontology Source"));
        assert_eq!(result.exclusions.len(), 1);
        assert_eq!(result.exclusions[0].reason, "bibliography");
    }
}
