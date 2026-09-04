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
/// 3. Degraded-page detection: flag pages served by a fallback backend
///    (`was_fallback`) — the routed primary failed or was unavailable.
/// 4. Error tally: count all pipeline errors.
///
/// `passed = (error_count == 0 && all_checks_pass)`. Degraded pages do not
/// fail `passed` — the fallback is by design and sensed — but they are
/// reported so a wholesale degradation (dead LLM endpoint, open breaker)
/// is visible in the report instead of hiding behind a passing verdict.
pub(crate) fn verify_output(
    expected_pages: usize,
    results: &[OcrResult],
    errors: &[PipelineError],
) -> VerificationReport {
    let actual_pages = results.len();
    let page_count_match = actual_pages == expected_pages;

    // Detect empty pages and collect per-page details from results
    let mut empty_pages: Vec<usize> = Vec::new();
    let mut degraded_pages: Vec<usize> = Vec::new();

    for (idx, result) in results.iter().enumerate().take(actual_pages) {
        if result.text.trim().is_empty() {
            empty_pages.push(idx);
        }
        if result.was_fallback {
            degraded_pages.push(result.page_index);
        }
    }

    let error_count = errors.len();

    VerificationReport::new(page_count_match, empty_pages, degraded_pages, error_count)
}
