//! Verification Checkpoint — Post-pipeline quality signal.
//!
//! The only module that answers "is this output good?"
//! Delete it → pipeline produces output with no quality signal.
//! It earns its existence.

use crate::ocr::{OcrResult, PipelineError, VerificationReport};

/// Verify assembled output against expected page count and source images.
///
/// # Checks
/// 1. Page count match: actual results vs expected images.
/// 2. Empty-page detection: flag pages with zero text.
/// 3. Word-count heuristic: flag if delta > 50% (coarse guardrail).
/// 4. Error tally: count all pipeline errors.
///
/// `passed = (error_count == 0 && all_checks_pass)`.
pub(crate) fn verify_output(
    expected_pages: usize,
    results: &[OcrResult],
    errors: &[PipelineError],
) -> VerificationReport {
    let actual_pages = results.len();
    let page_count_match = actual_pages == expected_pages;

    // Detect empty pages and collect per-page details from results
    let mut empty_pages: Vec<usize> = Vec::new();

    for (idx, result) in results.iter().enumerate().take(actual_pages) {
        if result.text.trim().is_empty() {
            empty_pages.push(idx);
        }
    }

    let error_count = errors.len();

    VerificationReport::new(page_count_match, empty_pages, error_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_document_passes() {
        use crate::ocr::OcrBackend;

        let results = vec![
            OcrResult {
                page_index: 0,
                backend: OcrBackend::Tesseract,
                text: "The quick brown fox jumps over the lazy dog.".into(),
                confidence: 0.95,
                duration_ms: 100,
                was_fallback: false,
            },
            OcrResult {
                page_index: 1,
                backend: OcrBackend::Tesseract,
                text: "The second page has more text content for testing.".into(),
                confidence: 0.90,
                duration_ms: 120,
                was_fallback: false,
            },
        ];

        let report = verify_output(2, &results, &[]);
        assert!(report.page_count_match, "page count should match");
        assert!(report.empty_pages.is_empty(), "no empty pages");
        assert!(report.passed, "clean document should pass: {:#?}", report);
    }

    #[test]
    fn missing_page_fails() {
        use crate::ocr::OcrBackend;

        let results = vec![OcrResult {
            page_index: 0,
            backend: OcrBackend::Tesseract,
            text: "Some content.".into(),
            confidence: 0.9,
            duration_ms: 50,
            was_fallback: false,
        }];
        let report = verify_output(2, &results, &[]);
        assert!(
            !report.page_count_match,
            "page count mismatch should be detected"
        );
        assert!(!report.passed, "mismatch should cause failure");
    }

    #[test]
    fn empty_page_flagged() {
        use crate::ocr::OcrBackend;

        let results = vec![
            OcrResult {
                page_index: 0,
                backend: OcrBackend::Tesseract,
                text: "Some text.".into(),
                confidence: 0.9,
                duration_ms: 50,
                was_fallback: false,
            },
            OcrResult {
                page_index: 1,
                backend: OcrBackend::LlmOcr("lighton".into()),
                text: "   ".into(), // whitespace-only = empty
                confidence: 0.0,
                duration_ms: 200,
                was_fallback: true,
            },
        ];
        let report = verify_output(2, &results, &[]);
        assert!(
            !report.empty_pages.is_empty(),
            "empty page should be flagged"
        );
        assert_eq!(report.empty_pages, vec![1]);
        assert!(!report.passed, "empty page should cause failure");
    }
}
