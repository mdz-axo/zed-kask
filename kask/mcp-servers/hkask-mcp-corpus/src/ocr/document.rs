use serde::{Deserialize, Serialize};

use super::quality::{self, PageQuality};

/// The form-feed character `pdftotext` uses to separate pages.
const FORM_FEED: char = '\u{000c}';

/// Split `pdftotext` output on form-feed into per-page text.
///
/// `pdftotext` separates pages with form-feed. An N-page PDF yields N
/// form-feed-separated sections plus a trailing empty string (the text after
/// the final form-feed). Drop exactly one trailing empty element so a
/// genuinely-empty final page is preserved while the spurious tail is not.
/// Interior zero-word pages are KEPT — they are the scanned/blank pages triage
/// exists to catch.
pub(crate) fn split_pdftotext_pages(raw: &str) -> Vec<String> {
    let mut pages: Vec<String> = raw.split(FORM_FEED).map(String::from).collect();
    if pages.last().is_some_and(|p| p.trim().is_empty()) {
        pages.pop();
    }
    pages
}

// ── OCR Result ────────────────────────────────────────────────────────────

/// The output of a single OCR invocation on one page.
///
/// Carries provenance and quality metadata for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OcrResult {
    /// 0-based page index within the source document.
    pub page_index: usize,
    /// The model that produced this text (e.g. `runpod/kask-ocr`).
    pub model: String,
    /// Extracted text content.
    pub text: String,
    /// Backend-reported confidence [0.0, 1.0].
    pub confidence: f32,
    /// Wall-clock duration of the OCR invocation in milliseconds.
    pub duration_ms: u64,
    /// Deterministic quality assessment of `text` (see `ocr::quality`).
    pub quality: PageQuality,
}

impl OcrResult {
    /// Construct a result, assessing the text against the quality gates at
    /// construction time — the single place quality is computed, so no
    /// result can exist with an unassessed or stale quality record.
    pub fn new(
        page_index: usize,
        model: impl Into<String>,
        text: String,
        confidence: f32,
        duration_ms: u64,
    ) -> Self {
        let quality = quality::assess(&text);
        Self {
            page_index,
            model: model.into(),
            text,
            confidence,
            duration_ms,
            quality,
        }
    }
}

// ── Pipeline Errors ───────────────────────────────────────────────────────

/// Errors that occur during pipeline execution. Collected per-page;
/// no error aborts the whole pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PipelineError {
    /// Decimation (PDF → images) failed.
    DecimationFailed(String),
    /// The OCR invocation failed for a page. There is no fallback backend —
    /// the model and the failure reason are surfaced so the operator can
    /// act on them (fix the endpoint, pick another model, re-run).
    OcrFailed {
        page_index: usize,
        model: String,
        reason: String,
    },
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::DecimationFailed(msg) => write!(f, "decimation failed: {}", msg),
            PipelineError::OcrFailed {
                page_index,
                model,
                reason,
            } => {
                write!(
                    f,
                    "OCR failed for page {} (model: {}): {}",
                    page_index, model, reason
                )
            }
        }
    }
}

// ── Verification Report ───────────────────────────────────────────────────

/// Post-pipeline verification checkpoint. `passed` is a computed field —
/// never settable by consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VerificationReport {
    /// Whether the assembled page count matches the expected page count.
    pub page_count_match: bool,
    /// Indices of pages that produced zero text.
    pub empty_pages: Vec<usize>,
    /// Indices (0-based) of pages whose output failed a deterministic
    /// quality gate (see `ocr::quality`): CJK hallucination, repetition
    /// loops, or symbol soup. The text is retained — the report names the
    /// page so garbage can never merge into a corpus silently.
    pub quality_failed_pages: Vec<usize>,
    /// Total number of pipeline errors across all pages.
    pub error_count: usize,
    /// Aggregate verification result. Derived from all checks.
    pub passed: bool,
}

impl VerificationReport {
    /// Compute `passed` from constituent checks.
    ///
    /// A report passes when: page count matches, no empty pages, no
    /// quality-gate failures, and zero errors.
    pub fn compute_passed(&mut self) {
        self.passed = self.page_count_match
            && self.empty_pages.is_empty()
            && self.quality_failed_pages.is_empty()
            && self.error_count == 0;
    }

    /// Create a report and compute `passed` inline.
    pub fn new(
        page_count_match: bool,
        empty_pages: Vec<usize>,
        quality_failed_pages: Vec<usize>,
        error_count: usize,
    ) -> Self {
        let mut report = Self {
            page_count_match,
            empty_pages,
            quality_failed_pages,
            error_count,
            passed: false,
        };
        report.compute_passed();
        report
    }
}

// ── Pipeline Outcome ──────────────────────────────────────────────────────

/// The single sealed output of the OCR pipeline.
///
/// No partial state escapes — consumers receive either a full
/// `PipelineOutcome` or a top-level error before the pipeline starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PipelineOutcome {
    /// Per-page OCR results in page order.
    pub results: Vec<OcrResult>,
    /// Verification report computed after assembly.
    pub report: VerificationReport,
    /// Model distribution across pages (e.g. `{"runpod/kask-ocr": 85}`).
    /// Surfaced to tool results so consumers can see which model produced
    /// the text.
    #[serde(default)]
    pub models: std::collections::HashMap<String, usize>,
    /// Pipeline errors collected across all pages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<PipelineError>,
}
