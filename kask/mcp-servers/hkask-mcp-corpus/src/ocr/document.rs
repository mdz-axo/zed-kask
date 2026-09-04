use serde::{Deserialize, Serialize};

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

/// The output of a single OCR backend invocation on one page.
///
/// Carries provenance metadata for verification and cross-validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OcrResult {
    /// 0-based page index within the source document.
    pub page_index: usize,
    /// Which backend produced this result.
    pub backend: super::config::OcrBackend,
    /// Extracted text content.
    pub text: String,
    /// Backend-reported confidence [0.0, 1.0].
    pub confidence: f32,
    /// Wall-clock duration of the OCR invocation in milliseconds.
    pub duration_ms: u64,
    /// True if this result was produced by the fallback (second-attempt) path.
    pub was_fallback: bool,
}

// ── Cross-Validation ──────────────────────────────────────────────────────

/// Cross-validation data for a dual-routed page (Moderate tier + sampling).
///
/// Observation only — does not autonomously change routing (P4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossValidation {
    /// Page index that was dual-routed.
    pub page_index: usize,
    /// Normalized Levenshtein similarity [0.0, 1.0] between the two results.
    pub similarity: f32,
    /// Complexity tier at routing time.
    pub(crate) tier: super::config::ComplexityTier,
    /// First backend used.
    pub(crate) backend_a: super::config::OcrBackend,
    /// Second backend used.
    pub(crate) backend_b: super::config::OcrBackend,
}

// ── Pipeline Errors ───────────────────────────────────────────────────────

/// Errors that occur during pipeline execution. Collected per-page;
/// no error aborts the whole pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum PipelineError {
    /// Decimation (PDF → images) failed.
    DecimationFailed(String),
    /// All OCR backends exhausted for a page without success.
    OcrFailed {
        page_index: usize,
        backends_tried: Vec<super::config::OcrBackend>,
    },
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::DecimationFailed(msg) => write!(f, "decimation failed: {}", msg),
            PipelineError::OcrFailed {
                page_index,
                backends_tried,
            } => {
                write!(
                    f,
                    "OCR failed for page {} (tried: {})",
                    page_index,
                    backends_tried
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
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
    /// Indices (0-based) of pages served by a degraded path: the routed
    /// primary backend failed (empty LLM output) or was unavailable (circuit
    /// breaker open), so a fallback backend produced the text. Distinct from
    /// `empty_pages` — a degraded page has text, just from the tier's
    /// second-choice backend. `passed` does not fail on degradation (it is
    /// by design and sensed); consumers read this field to tell by-design
    /// Simple-tier tesseract routing apart from a dead LLM endpoint.
    pub degraded_pages: Vec<usize>,
    /// Total number of pipeline errors across all pages.
    pub error_count: usize,
    /// Aggregate verification result. Derived from all checks.
    pub passed: bool,
}

impl VerificationReport {
    /// Compute `passed` from constituent checks.
    ///
    /// A report passes when: page count matches, no empty pages, and zero
    /// errors. (The word-count-delta check was removed — see verification.rs.)
    pub fn compute_passed(&mut self) {
        self.passed = self.page_count_match && self.empty_pages.is_empty() && self.error_count == 0;
    }

    /// Create a report and compute `passed` inline.
    pub fn new(
        page_count_match: bool,
        empty_pages: Vec<usize>,
        degraded_pages: Vec<usize>,
        error_count: usize,
    ) -> Self {
        let mut report = Self {
            page_count_match,
            empty_pages,
            degraded_pages,
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
    /// Final-backend distribution across pages (e.g. `{"tesseract": 20,
    /// "llm-ocr": 1}`). Surfaced to tool results so consumers can see whether
    /// the LLM backend produced anything at all — an all-tesseract map with
    /// empty `report.degraded_pages` is by-design Simple-tier routing; one
    /// with every LLM-routed page degraded is a dead endpoint.
    #[serde(default)]
    pub backends: std::collections::HashMap<String, usize>,
    /// Cross-validation data from dual-routed pages (calibration mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_validations: Vec<CrossValidation>,
    /// Pipeline errors collected across all pages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<PipelineError>,
}
