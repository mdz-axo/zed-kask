//! OCR text quality gates — deterministic, per-page and per-file.
//!
//! The demonstrated failure modes of vision-LLM OCR (2026-09-10 corpus
//! session, all of which passed the word-count floor):
//! - **CJK hallucination**: the model drifts into Chinese characters on hard
//!   scans (Berlin: 277K CJK chars, clark: 746K — on files that looked
//!   plausible by word count).
//! - **Repetition loops**: degenerate n-gram cycling (glm-ocr
//!   "weather/owellowell" loops; word counts inflate to look normal).
//! - **Symbol soup**: punctuation-symbol streams on unreadable typography
//!   (Maturana's prior extraction: 13% dictionary-miss symbol fragments).
//!
//! None of these fail a word-count floor. These gates catch all three with
//! wide margins: clean book text measures ~0 CJK, ~0 repetition, <0.05
//! symbol ratio; the garbage cases measure 0.3–0.9 on their failing gate.
//!
//! Gate failures do not discard the text — the page keeps its output, the
//! verification report names the page and the failed gates, and
//! `VerificationReport.passed` goes false. Garbage is surfaced, never
//! silently substituted and never silently merged.

/// Per-page (or per-file) quality assessment of OCR output text.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PageQuality {
    /// Whitespace-separated word count.
    pub word_count: usize,
    /// CJK characters / non-whitespace characters. Hallucinated CJK on a
    /// Latin-script page is the dominant degeneration mode; clean English
    /// book text measures 0.0.
    pub cjk_ratio: f32,
    /// Fraction of 8-word shingles that occur more than once. Degenerate
    /// repetition loops measure ~0.9; clean prose ~0.0 (a refrain or running
    /// header repeats a handful of shingles at most).
    pub repetition_ratio: f32,
    /// Fraction of tokens that are not well-formed (letters with optional
    /// internal hyphens/apostrophes, or numerals). Symbol-soup extractions
    /// measure ~0.8 (fragments like `f/J`, `•uz`, `0Z`); clean prose ~0.0.
    /// Whitelist-independent: token shape, not character classification.
    pub garble_ratio: f32,
    /// Names of every gate this text failed. Empty means all gates passed.
    pub failed_gates: Vec<String>,
}

/// CJK ratio above this fails the gate. A Latin-script book page is 0.0;
/// hallucination cases measured 0.3+. An operator OCRing actual CJK
/// sources will see every page flagged with `cjk-hallucination` — the gate
/// reason is surfaced per page so that reading is explicit, not silent.
const CJK_RATIO_MAX: f32 = 0.02;

/// Repetition ratio above this fails the gate.
const REPETITION_RATIO_MAX: f32 = 0.10;

/// Garble ratio above this fails the gate.
const GARBLE_RATIO_MAX: f32 = 0.30;

/// Shingle width for the repetition metric, in words.
const SHINGLE_WORDS: usize = 8;

/// Assess OCR output text against the quality gates.
pub(crate) fn assess(text: &str) -> PageQuality {
    let words: Vec<&str> = text.split_whitespace().collect();
    let non_whitespace: usize = text.chars().filter(|c| !c.is_whitespace()).count();

    let cjk = text.chars().filter(|c| is_cjk(*c)).count();
    let cjk_ratio = if non_whitespace == 0 {
        0.0
    } else {
        cjk as f32 / non_whitespace as f32
    };

    let repetition_ratio = repetition_ratio(&words);
    let garble_ratio = garble_ratio(&words);

    let mut failed_gates: Vec<String> = Vec::new();
    if cjk_ratio > CJK_RATIO_MAX {
        failed_gates.push("cjk-hallucination".to_string());
    }
    if repetition_ratio > REPETITION_RATIO_MAX {
        failed_gates.push("repetition-loop".to_string());
    }
    if garble_ratio > GARBLE_RATIO_MAX {
        failed_gates.push("garbled-tokens".to_string());
    }

    PageQuality {
        word_count: words.len(),
        cjk_ratio,
        repetition_ratio,
        garble_ratio,
        failed_gates,
    }
}

/// Whether the text passes every quality gate. Used by the directory-mode
/// skip check: an existing output that fails a gate is re-extracted on the
/// next run instead of being honored — garbage never persists as idempotency.
pub(crate) fn passes_gates(text: &str) -> bool {
    assess(text).failed_gates.is_empty()
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
}

fn is_well_formed_token(token: &str) -> bool {
    let stripped = token.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', ')', ']']);
    if stripped.is_empty() {
        return false;
    }
    let body = stripped
        .strip_prefix('(')
        .or_else(|| stripped.strip_prefix('['))
        .unwrap_or(stripped);
    if body
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '\'')
    {
        return true;
    }
    body.chars()
        .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
}

/// Fraction of tokens that are not well-formed. See [`PageQuality::garble_ratio`].
fn garble_ratio(words: &[&str]) -> f32 {
    if words.is_empty() {
        return 0.0;
    }
    let garbled = words.iter().filter(|w| !is_well_formed_token(w)).count();
    garbled as f32 / words.len() as f32
}

/// Fraction of 8-word shingles that occur more than once. Short texts
/// (fewer than two shingles) cannot loop and report 0.0.
fn repetition_ratio(words: &[&str]) -> f32 {
    if words.len() < SHINGLE_WORDS * 2 {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for shingle in words.windows(SHINGLE_WORDS) {
        *counts.entry(shingle).or_insert(0usize) += 1;
    }
    let repeated = counts.values().filter(|c| **c > 1).count();
    repeated as f32 / counts.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean_page() -> String {
        "Sleepless and persevering, the government over the next few months \
         hauled in suspects named Caruso, Abato, Ferro, Fasulo, and De Filipos. \
         No firm evidence could be found against any of them. A few clues and \
         leads gradually turned up, and a farrier with a shop in New Chambers \
         Street identified the shoes of the dismembered horse as his work."
            .to_string()
    }

    #[test]
    fn clean_prose_passes_all_gates() {
        let q = assess(&clean_page());
        assert!(q.failed_gates.is_empty(), "gates: {:?}", q.failed_gates);
        assert_eq!(q.cjk_ratio, 0.0);
        assert!(q.repetition_ratio < 0.05);
        assert!(q.garble_ratio < 0.05);
        assert!(passes_gates(&clean_page()));
    }

    #[test]
    fn cjk_hallucination_fails() {
        let mut text = clean_page();
        // Hallucinated CJK at roughly the observed density (a third of chars)
        text.push_str(&"长久的领域重点是不少的人".repeat(20));
        let q = assess(&text);
        assert!(q.failed_gates.contains(&"cjk-hallucination".to_string()));
        assert!(!passes_gates(&text));
    }

    #[test]
    fn repetition_loop_fails() {
        // The observed degeneration: one phrase cycled for most of the page.
        let phrase = "another carrier in Elizabeth Street insisted that he had made the shoes ";
        let text = phrase.repeat(30);
        let q = assess(&text);
        assert!(
            q.failed_gates.contains(&"repetition-loop".to_string()),
            "ratio: {}",
            q.repetition_ratio
        );
        assert!(!passes_gates(&text));
    }

    #[test]
    fn symbol_soup_fails() {
        // The observed Maturana failure: symbol fragments, not words.
        let text = "< - •• < > 0 - \" % u u < •\" ~ < < % f/J f /J u.J Co. o • • •' F Z I- « 0Z 0 ~ ., i1 ·0 ~ ~ ~ u ;::• < \" ~ t • •• •• • • •• :'i -0 Cl -0 - 1,, •, Z - \"\" ' J - 0 l- < 0 0 u ! •0 ~ \" ~ < • « •0 ••\" -•• •uz •• •u •o ~< - •• -> ••";
        let q = assess(text);
        assert!(
            q.failed_gates.contains(&"garbled-tokens".to_string()),
            "ratio: {}",
            q.garble_ratio
        );
        assert!(!passes_gates(text));
    }

    #[test]
    fn short_text_cannot_trip_repetition() {
        let q = assess("BLANK");
        assert!(q.failed_gates.is_empty());
        assert_eq!(q.word_count, 1);
    }

    #[test]
    fn empty_text_passes_structurally() {
        // Zero text is the empty-page check's domain (verification), not the
        // quality gates' — ratios are defined as 0.0 for empty input.
        let q = assess("");
        assert!(q.failed_gates.is_empty());
        assert_eq!(q.word_count, 0);
    }
}
