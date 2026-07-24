use serde::{Deserialize, Serialize};

// ── Complexity Tiers ──────────────────────────────────────────────────────

/// Complexity tier derived from pixel-density heuristics.
///
/// Determines backend routing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ComplexityTier {
    /// Low visual complexity — text-only or near-empty pages.
    Simple,
    /// Moderate complexity — tables, mixed content, forms.
    Moderate,
    /// High complexity — photographs, dense graphics, handwritten text.
    Complex,
}

/// A scored complexity value for a single page image.
///
/// `tier` is derived from `value` via threshold dispatch and is the
/// authoritative routing signal; `value` is the raw heuristic score for
/// logging and threshold self-tuning.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComplexityScore {
    /// Raw edge-density ratio [0.0, 1.0].
    pub value: f32,
    /// Threshold-derived tier.
    pub tier: ComplexityTier,
}

// ── OCR Backends ──────────────────────────────────────────────────────────

/// Exhaustive set of OCR backends. Each variant maps to a concrete
/// invocation path within the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OcrBackend {
    /// Classical OCR via Tesseract (fast, best for text-only).
    Tesseract,
    /// Vision-language model OCR via hkask-inference router.
    /// The inner `String` is the model name (e.g., `DI/allenai/olmOCR-2-7B-1025`).
    LlmOcr(String),
}

impl OcrBackend {
    /// Human-readable label for logging and Regulation spans.
    pub fn label(&self) -> &str {
        match self {
            OcrBackend::Tesseract => "tesseract",
            OcrBackend::LlmOcr(_) => "llm-ocr",
        }
    }
}

impl std::fmt::Display for OcrBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrBackend::Tesseract => write!(f, "tesseract"),
            OcrBackend::LlmOcr(model) => write!(f, "llm-ocr({})", model),
        }
    }
}

// ── Thresholds Module ─────────────────────────────────────────────────────

/// Default vision LLM model for OCR.
/// Uses kask-ocr on RunPod (OLMOCR-2, synchronous /runsync endpoint).
/// Override via `HKASK_OCR_MODEL` env var or `llm_model` pipeline parameter.
/// Requires RUNPOD_API_KEY and RUNPOD_OCR_ENDPOINT env vars.
pub const DEFAULT_LLM_OCR_MODEL: &str = "RP/kask-ocr";

/// Configurable OCR complexity thresholds.
///
/// When `tuneable` is `true`, the Regulation calibration system may suggest
/// adjustments based on accumulated cross-validation data (P4: human
/// approval required before any change takes effect).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Edge-density ratio below which a page is considered Simple.
    pub simple_max: f32,
    /// Edge-density ratio below which a page is considered Moderate.
    /// Values ≥ this threshold are Complex.
    pub moderate_max: f32,
    /// Dual-routing sampling rate for Moderate-tier pages [0.0, 1.0].
    pub moderate_sample_rate: f32,
    /// Whether Regulation may suggest threshold adjustments based on observed accuracy.
    /// When `false`, thresholds are locked at configured values.
    #[serde(default = "default_tuneable")]
    pub tuneable: bool,
}

fn default_tuneable() -> bool {
    true
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            simple_max: 0.05,
            moderate_max: 0.15,
            moderate_sample_rate: 0.10,
            tuneable: true,
        }
    }
}

impl ThresholdConfig {
    /// Classify an edge-density value into a complexity tier.
    pub fn classify(&self, edge_density: f32) -> ComplexityTier {
        if edge_density < self.simple_max {
            ComplexityTier::Simple
        } else if edge_density < self.moderate_max {
            ComplexityTier::Moderate
        } else {
            ComplexityTier::Complex
        }
    }
}

// ── Page Triage (pre-OCR complexity detection) ────────────────────────────
//
// Inspired by LiteParse's `ComplexityReason` / `PageComplexityStats`, but
// limited to signals docproc can detect from `pdftotext` + `pdfimages` (no
// PDFium text-object access). `Garbled` / `VectorText` reasons require a
// PDFium-level extraction layer and are deferred (Tier 2).

/// Why a single page was flagged as needing more than the cheap text-only
/// path. Multiple reasons can apply to one page.
///
/// Maps to LiteParse's `ComplexityReason` where detectable:
/// - `NoText` / `SparseText` — from per-page `pdftotext` word count.
/// - `Scanned` / `EmbeddedImages` — from `pdfimages -list` physical image size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriageReason {
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
pub struct TriageVerdict {
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

/// Per-page triage thresholds. Distinct from `ThresholdConfig` (which governs
/// in-pipeline Sobel routing) — these gate *whether* a page enters the pipeline
/// at all.
///
/// Like `ThresholdConfig`, `tuneable` gates Regulation calibration: drift may
/// be *suggested* (Regulation alert, ≥100 samples, >95% agreement) but **never
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
    /// Build from env vars, falling back to defaults. Mirrors the
    /// `HKASK_OCR_*` threshold pattern in `lib.rs::run`.
    pub fn from_env() -> Self {
        Self {
            text_native_min_words: std::env::var("HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            min_image_size_pt: std::env::var("HKASK_OCR_TRIAGE_MIN_IMAGE_PT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(25.0),
            full_page_image_min_pt: std::env::var("HKASK_OCR_TRIAGE_FULL_PAGE_PT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500.0),
            embedded_image_min_pt: std::env::var("HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(150.0),
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
