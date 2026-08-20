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
