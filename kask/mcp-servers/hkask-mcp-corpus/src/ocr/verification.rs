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
/// 3. Quality-gate detection: flag pages whose output failed a
///    deterministic quality gate (CJK hallucination, repetition loop,
///    symbol soup — see `ocr::quality`). The text is retained; the page is
///    named so garbage can never merge into a corpus silently.
/// 4. Error tally: count all pipeline errors.
///
/// `passed = (error_count == 0 && all_checks_pass)`. Quality-gate failures
/// fail `passed` — a run that produced hallucinated or degenerate text is
/// NOT a passing run, whatever the page counts say.
pub(crate) fn verify_output(
    expected_pages: usize,
    results: &[OcrResult],
    errors: &[PipelineError],
) -> VerificationReport {
    let actual_pages = results.len();
    let page_count_match = actual_pages == expected_pages;

    let mut empty_pages: Vec<usize> = Vec::new();
    let mut quality_failed_pages: Vec<usize> = Vec::new();

    for result in results.iter().take(actual_pages) {
        if result.text.trim().is_empty() {
            empty_pages.push(result.page_index);
        }
        if !result.quality.failed_gates.is_empty() {
            quality_failed_pages.push(result.page_index);
        }
    }

    let error_count = errors.len();

    VerificationReport::new(
        page_count_match,
        empty_pages,
        quality_failed_pages,
        error_count,
    )
}
