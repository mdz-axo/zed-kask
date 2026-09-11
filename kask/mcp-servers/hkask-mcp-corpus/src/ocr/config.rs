use serde::{Deserialize, Serialize};

// ── Page Triage (pre-OCR complexity detection) ────────────────────────────
//
// Inspired by LiteParse's `ComplexityReason` / `PageComplexityStats`, but
// limited to signals docproc can detect from `pdftotext` + `pdfimages` (no
// PDFium text-object access). `Garbled` / `VectorText` reasons require a
// PDFium-level extraction layer and are deferred (Tier 2).
//
// The former complexity-tier routing (Simple/Moderate/Complex →
// Tesseract/LLM) was removed with the Tesseract backend (2026-09-10):
// every page that enters the pipeline goes to the configured LLM OCR model,
// and output quality is gated by `ocr::quality` instead of routed by
// pixel-density heuristics that silently sent book pages to Tesseract.

/// Why a single page was flagged as needing more than the cheap text-only
/// path. Multiple reasons can apply to one page.
///
/// Maps to LiteParse's `ComplexityReason` where detectable:
/// - `NoText` / `SparseText` — from per-page `pdftotext` word count.
/// - `Scanned` / `EmbeddedImages` — from `pdfimages -list` physical image size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TriageReason {
    /// A single raster covers most of the page and there is no extractable
    /// text behind it — a scanned/photographed page.
    Scanned,
    /// No extractable native text and no substantial raster (a blank page,
    /// or a near-empty cover/divider).
    NoText,
    /// Some real text, but below the text-native threshold — typically a
    /// figure-heavy page with only thin captions.
    SparseText,
    /// Substantial text alongside substantial embedded raster figures.
    EmbeddedImages,
}

impl TriageReason {
    /// Kebab-case string used in Regulation spans and tool output.
    pub fn as_str(&self) -> &'static str {
        match self {
            TriageReason::Scanned => "scanned",
            TriageReason::NoText => "no-text",
            TriageReason::SparseText => "sparse-text",
            TriageReason::EmbeddedImages => "embedded-images",
        }
    }
}

/// Triage verdict for a single page: the per-page analogue of docproc's
/// former whole-document `word_count vs OCR_FALLBACK_WORD_THRESHOLD` check.
///
/// Making the routing unit the *page* (not the document) is what fixes the
/// silent-loss bug where a mixed PDF with ≥100 total words skipped OCR
/// entirely and dropped any per-page scanned regions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TriageVerdict {
    /// 1-based page number.
    pub page_number: usize,
    /// Native text word count on this page (from `pdftotext`).
    pub word_count: usize,
    /// Whether this page should be sent to the OCR pipeline. Equivalent to
    /// `!reasons.is_empty()`; kept as a flat bool for the common predicate.
    pub needs_ocr: bool,
    /// Every reason the page was flagged, in no particular order.
    pub reasons: Vec<TriageReason>,
}

/// Per-page triage thresholds. These gate *whether* a page enters the
/// pipeline at all — distinct from the removed in-pipeline Sobel routing.
///
/// `tuneable` gates Regulation calibration: drift may be *suggested*
/// (Regulation alert, ≥100 samples, >95% agreement) but **never
/// auto-applied** — P4 affirmative consent requires human approval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TriageConfig {
    /// Per-page word count at/above which a page is text-native (no OCR).
    pub text_native_min_words: usize,
    /// Minimum image side length (points) to count as a substantial image.
    /// Filters out bullets, rule lines, and icons.
    pub min_image_size_pt: f32,
    /// Physical image dimensions (pt) at/above which a no-text page is
    /// classified `Scanned` (a near-full-page raster). Both width and height
    /// must meet this for the image to count as full-page.
    pub full_page_image_min_pt: f32,
    /// Minimum image side (pt) for an image to flag `EmbeddedImages` on a
    /// page that already has substantial text.
    pub embedded_image_min_pt: f32,
    /// Whether Regulation calibration may suggest threshold adjustments.
    #[serde(default = "default_tuneable")]
    pub tuneable: bool,
}

fn default_tuneable() -> bool {
    true
}

impl Default for TriageConfig {
    fn default() -> Self {
        Self {
            // A text-native page typically has ≥20 words; below that it is
            // likely figure/divider chrome or a scanned page. Tuned to be
            // conservative (favor OCR over silent loss).
            text_native_min_words: 20,
            min_image_size_pt: 25.0,
            // ~80% of a letter-page width (612pt) / height (792pt).
            full_page_image_min_pt: 500.0,
            embedded_image_min_pt: 150.0,
            tuneable: true,
        }
    }
}

impl TriageConfig {
    /// Build from env vars, falling back to defaults.
    pub fn from_env() -> Self {
        Self {
            text_native_min_words: hkask_mcp_server::parse_env_warn(
                "HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN",
                20,
            ),
            min_image_size_pt: hkask_mcp_server::parse_env_warn(
                "HKASK_OCR_TRIAGE_MIN_IMAGE_PT",
                25.0,
            ),
            full_page_image_min_pt: hkask_mcp_server::parse_env_warn(
                "HKASK_OCR_TRIAGE_FULL_PAGE_PT",
                500.0,
            ),
            embedded_image_min_pt: hkask_mcp_server::parse_env_warn(
                "HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT",
                150.0,
            ),
            tuneable: std::env::var("HKASK_OCR_TRIAGE_TUNEABLE")
                .ok()
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
        }
    }
}

impl TriageVerdict {
    /// Construct a verdict for a text-native page (no reasons, no OCR).
    pub fn text_native(page_number: usize, word_count: usize) -> Self {
        Self {
            page_number,
            word_count,
            needs_ocr: false,
            reasons: vec![],
        }
    }

    /// Construct a verdict for a page needing OCR, with the given reasons.
    pub fn needs_ocr_with(
        page_number: usize,
        word_count: usize,
        reasons: Vec<TriageReason>,
    ) -> Self {
        Self {
            page_number,
            word_count,
            needs_ocr: !reasons.is_empty(),
            reasons,
        }
    }
}
