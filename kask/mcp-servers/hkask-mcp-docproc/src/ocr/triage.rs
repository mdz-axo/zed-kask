//! Page Triage — cheap per-page complexity detection *before* committing to OCR.
//!
//! The docproc-native analogue of LiteParse's `lit is-complex`. Runs a text-layer
//! pass (`pdftotext`, split per page on form-feed) plus an image inventory
//! (`pdfimages -list`) — no page rendering — and classifies each page as
//! text-native or needing OCR, with typed reasons.
//!
//! Why this exists: docproc's former `extract_text` ran one `pdftotext` call on
//! the whole document and checked a single doc-level word count against
//! `OCR_FALLBACK_WORD_THRESHOLD` (100). A mixed PDF with ≥100 total words
//! returned `Success` and skipped OCR entirely, silently dropping any per-page
//! scanned/image-only regions. Making the routing unit the *page* fixes that.
//!
//! Detectable reasons (no PDFium):
//! - `NoText` / `SparseText` — per-page `pdftotext` word count.
//! - `Scanned` / `EmbeddedImages` — `pdfimages -list` physical image size.
//!
//! Not yet detectable (require a PDFium text-object layer — Tier 2):
//! - `Garbled` (broken cmap / Type3 fallback), `VectorText` (filled vector outlines).
//!
//! P4 calibration: triage thresholds live in `TriageConfig` and follow the same
//! affirmative-consent discipline as `ThresholdConfig` — never auto-adjusted.

use std::path::Path;

use crate::ocr::{TriageConfig, TriageReason, TriageVerdict};

/// Typed triage errors. (No `Result<_, String>` — project rule.)
#[derive(Debug, Clone, thiserror::Error)]
pub enum TriageError {
    #[error("pdftotext failed: {0}")]
    PdftotextFailed(String),
    #[error("pdfimages failed: {0}")]
    PdfimagesFailed(String),
    #[error("page count mismatch: pdftotext={text_pages} pdfimages={image_pages}")]
    PageCountMismatch {
        text_pages: usize,
        image_pages: usize,
    },
    #[error("invalid target-pages spec: {0}")]
    InvalidPageSpec(String),
}

/// Per-page image signal parsed from `pdfimages -list`.
#[derive(Debug, Clone, Default)]
struct PageImageSignal {
    /// Largest image on the page, as (width_pt, height_pt).
    largest: Option<(f32, f32)>,
    /// Whether any substantial image (≥ `min_image_size_pt` both sides) is present.
    has_substantial: bool,
    /// Whether a near-full-page image is present.
    has_full_page: bool,
}

/// Triage a PDF: classify every page as text-native or needing OCR.
///
/// One `pdftotext -layout` call (split on form-feed) + one `pdfimages -list`
/// call. No page rendering. Returns one `TriageVerdict` per page, 1-based.
///
/// Use this standalone entry from the `docproc_is_complex` tool. Callers that
/// already hold the per-page text (e.g. `extract_text`, which runs `pdftotext`
/// itself) should use [`triage_pages`] to avoid a second `pdftotext` spawn.
pub async fn triage_pdf(
    path: &Path,
    config: &TriageConfig,
) -> Result<Vec<TriageVerdict>, TriageError> {
    let per_page_texts = extract_per_page_text(path).await?;
    triage_pages(path, &per_page_texts, config).await
}

/// Triage pages from pre-split page text. The `path` is used only for the
/// `pdfimages -list` image inventory (best-effort); the text is supplied by the
/// caller to avoid a redundant `pdftotext` call.
pub(crate) async fn triage_pages(
    path: &Path,
    per_page_texts: &[String],
    config: &TriageConfig,
) -> Result<Vec<TriageVerdict>, TriageError> {
    let page_count = per_page_texts.len();

    // Image inventory is best-effort: if pdfimages is unavailable, triage
    // degrades to word-count-only reasons (NoText/SparseText). This never
    // causes a silent loss — pages with no text are still flagged for OCR.
    let image_signals = match extract_per_page_images(path, page_count, config).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.pipeline.triage",
                error = %e,
                "pdfimages unavailable — triage using word-count only",
            );
            vec![PageImageSignal::default(); page_count]
        }
    };

    if image_signals.len() != page_count {
        return Err(TriageError::PageCountMismatch {
            text_pages: page_count,
            image_pages: image_signals.len(),
        });
    }

    let mut verdicts = Vec::with_capacity(page_count);
    for (i, page_text) in per_page_texts.iter().enumerate() {
        let page_number = i + 1;
        let words = page_text.split_whitespace().count();
        let img = &image_signals[i];
        verdicts.push(classify_page(page_number, words, img, config));
    }
    Ok(verdicts)
}

/// 0-based indices of pages that need OCR, from a triage verdict list.
pub(crate) fn ocr_page_indices(verdicts: &[TriageVerdict]) -> Vec<usize> {
    verdicts
        .iter()
        .filter(|v| v.needs_ocr)
        .map(|v| v.page_number - 1)
        .collect()
}

/// Parse a page-range spec like `"1-5,10,15-20"` into a sorted, deduplicated
/// list of 1-based page numbers. Empty/whitespace input yields an empty vec.
///
/// Mirrors LiteParse's `--target-pages` CLI grammar. Errors on malformed
/// ranges (non-numeric, inverted `5-2`, zero page).
pub fn parse_target_pages(spec: &str) -> Result<Vec<usize>, TriageError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(Vec::new());
    }
    let mut pages: Vec<usize> = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let lo: usize = lo.trim().parse().map_err(|_| {
                TriageError::InvalidPageSpec(format!("invalid range bound '{}'", lo))
            })?;
            let hi: usize = hi.trim().parse().map_err(|_| {
                TriageError::InvalidPageSpec(format!("invalid range bound '{}'", hi))
            })?;
            if lo == 0 || hi == 0 {
                return Err(TriageError::PdftotextFailed(
                    "target_pages are 1-based; page 0 is invalid".into(),
                ));
            }
            if lo > hi {
                return Err(TriageError::PdftotextFailed(format!(
                    "inverted target_pages range {lo}-{hi}"
                )));
            }
            pages.extend(lo..=hi);
        } else {
            let p: usize = part
                .parse()
                .map_err(|_| TriageError::InvalidPageSpec(format!("invalid page '{}'", part)))?;
            if p == 0 {
                return Err(TriageError::PdftotextFailed(
                    "target_pages are 1-based; page 0 is invalid".into(),
                ));
            }
            pages.push(p);
        }
    }
    pages.sort();
    pages.dedup();
    Ok(pages)
}

/// Classify a single page from its word count and image signal.
fn classify_page(
    page_number: usize,
    word_count: usize,
    img: &PageImageSignal,
    config: &TriageConfig,
) -> TriageVerdict {
    let mut reasons: Vec<TriageReason> = Vec::new();

    let text_native = word_count >= config.text_native_min_words;

    if word_count == 0 {
        // No extractable text — distinguish scanned (has full-page raster) from blank.
        if img.has_full_page {
            reasons.push(TriageReason::Scanned);
        } else {
            reasons.push(TriageReason::NoText);
        }
    } else if !text_native {
        // Some text but below the text-native threshold.
        reasons.push(TriageReason::SparseText);
        // A sparse page that is mostly a raster is still a scan.
        if img.has_full_page {
            reasons.push(TriageReason::Scanned);
        }
    } else if img.has_substantial {
        // Substantial text + substantial embedded figures.
        reasons.push(TriageReason::EmbeddedImages);
    }

    if reasons.is_empty() {
        TriageVerdict::text_native(page_number, word_count)
    } else {
        TriageVerdict::needs_ocr_with(page_number, word_count, reasons)
    }
}

/// Run `pdftotext -layout` once and split the output on form-feed (`\x0c`)
/// into per-page text. Avoids N per-page subprocess spawns.
async fn extract_per_page_text(path: &Path) -> Result<Vec<String>, TriageError> {
    let output = tokio::process::Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        .await
        .map_err(|e| TriageError::PdftotextFailed(format!("spawn: {e}")))?;
    if !output.status.success() {
        return Err(TriageError::PdftotextFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // pdftotext separates pages with form-feed (`\x0c`). An N-page PDF yields
    // N form-feed-separated sections plus a trailing empty string (the text
    // after the final form-feed). Drop exactly one trailing empty element so a
    // genuinely-empty final page is preserved while the spurious tail is not.
    // Interior zero-word pages are KEPT — they are the scanned/blank pages
    // triage exists to catch.
    let mut pages: Vec<String> = text.split('\x0c').map(String::from).collect();
    if pages.last().is_some_and(|p| p.trim().is_empty()) {
        pages.pop();
    }
    Ok(pages)
}

/// Run `pdfimages -list` and aggregate per-page image signals.
///
/// Output rows (after the 2-line header):
/// ```text
/// page num type width height color comp bpc enc interp object ID x-ppi y-ppi size ratio
/// ```
/// Only `image`/`stencil`/`tiff`/`jpeg`/`jpx2` types count (soft-masks are
/// alpha channels, not visible content). Physical size in points is
/// `(px / ppi) * 72`.
async fn extract_per_page_images(
    path: &Path,
    page_count: usize,
    config: &TriageConfig,
) -> Result<Vec<PageImageSignal>, TriageError> {
    let output = tokio::process::Command::new("pdfimages")
        .arg("-list")
        .arg(path)
        .output()
        .await
        .map_err(|e| TriageError::PdfimagesFailed(format!("spawn: {e}")))?;
    if !output.status.success() {
        return Err(TriageError::PdfimagesFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut signals = vec![PageImageSignal::default(); page_count];

    for line in stdout.lines().skip(2) {
        if let Some(signal) = parse_image_row(line, config) {
            let (page, img) = signal;
            if page >= 1 && page <= page_count {
                let slot = &mut signals[page - 1];
                if img.has_substantial {
                    slot.has_substantial = true;
                }
                if img.has_full_page {
                    slot.has_full_page = true;
                }
                if let Some((w, h)) = img.largest {
                    slot.largest = Some(match slot.largest {
                        Some((pw, ph)) if pw * ph >= w * h => (pw, ph),
                        _ => (w, h),
                    });
                }
            }
        }
    }
    Ok(signals)
}

/// Parse a single `pdfimages -list` row into (page, signal) if the row is a
/// visible image (not a soft-mask). Returns `None` for headers/unparseable rows.
fn parse_image_row(line: &str, config: &TriageConfig) -> Option<(usize, PageImageSignal)> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 13 {
        return None;
    }
    let page: usize = cols[0].parse().ok()?;
    let kind = cols[2];
    // Soft-masks (`smask`) are alpha channels, not visible images.
    if kind == "smask" {
        return None;
    }
    let width_px: f32 = cols[3].parse().ok()?;
    let height_px: f32 = cols[4].parse().ok()?;
    // Header: page num type width height color comp bpc enc interp object ID x-ppi y-ppi ...
    //          0   1   2    3     4      5     6    7   8    9       10    11  12     13
    let x_ppi: f32 = cols[12].parse().ok().filter(|&p: &f32| p > 0.0)?;
    let y_ppi: f32 = cols[13].parse().ok().filter(|&p: &f32| p > 0.0)?;
    let width_pt = width_px / x_ppi * 72.0;
    let height_pt = height_px / y_ppi * 72.0;

    let mut signal = PageImageSignal::default();
    let min_side = width_pt.min(height_pt);
    if min_side >= config.min_image_size_pt {
        signal.has_substantial = true;
        signal.largest = Some((width_pt, height_pt));
    }
    // A near-full-page raster: both dimensions meet the full-page threshold.
    if width_pt >= config.full_page_image_min_pt && height_pt >= config.full_page_image_min_pt {
        signal.has_full_page = true;
    }
    // Embedded-figures threshold: a substantial image on a text page.
    if min_side >= config.embedded_image_min_pt {
        signal.has_substantial = true;
    }
    Some((page, signal))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TriageConfig {
        TriageConfig::default()
    }

    #[test]
    fn text_native_page_has_no_reasons() {
        let v = classify_page(1, 400, &PageImageSignal::default(), &cfg());
        assert!(!v.needs_ocr);
        assert!(v.reasons.is_empty());
        assert_eq!(v.word_count, 400);
    }

    #[test]
    fn zero_word_blank_page_is_no_text() {
        let v = classify_page(2, 0, &PageImageSignal::default(), &cfg());
        assert!(v.needs_ocr);
        assert_eq!(v.reasons, vec![TriageReason::NoText]);
    }

    #[test]
    fn zero_word_page_with_full_page_image_is_scanned() {
        let img = PageImageSignal {
            largest: Some((520.0, 700.0)),
            has_substantial: true,
            has_full_page: true,
        };
        let v = classify_page(3, 0, &img, &cfg());
        assert!(v.needs_ocr);
        assert_eq!(v.reasons, vec![TriageReason::Scanned]);
    }

    #[test]
    fn sparse_text_page_is_flagged() {
        let v = classify_page(4, 5, &PageImageSignal::default(), &cfg());
        assert!(v.needs_ocr);
        assert_eq!(v.reasons, vec![TriageReason::SparseText]);
    }

    #[test]
    fn sparse_page_with_full_page_image_is_also_scanned() {
        let img = PageImageSignal {
            largest: Some((510.0, 690.0)),
            has_substantial: true,
            has_full_page: true,
        };
        let v = classify_page(5, 8, &img, &cfg());
        assert!(v.needs_ocr);
        assert!(v.reasons.contains(&TriageReason::SparseText));
        assert!(v.reasons.contains(&TriageReason::Scanned));
    }

    #[test]
    fn text_page_with_substantial_image_is_embedded_images() {
        let img = PageImageSignal {
            largest: Some((200.0, 300.0)),
            has_substantial: true,
            has_full_page: false,
        };
        let v = classify_page(6, 500, &img, &cfg());
        assert!(v.needs_ocr);
        assert_eq!(v.reasons, vec![TriageReason::EmbeddedImages]);
    }

    #[test]
    fn tiny_image_below_min_size_is_ignored() {
        // 10pt image — below min_image_size_pt (25). A text page stays text-native.
        let img = PageImageSignal {
            largest: Some((10.0, 10.0)),
            has_substantial: false,
            has_full_page: false,
        };
        let v = classify_page(7, 600, &img, &cfg());
        assert!(!v.needs_ocr, "tiny icon should not flag a text page");
    }

    #[test]
    fn parse_image_row_skips_smask() {
        let line = "   3     1 smask    1520  2239  gray    1   8  image  no       128  0   500   500 27.5K 0.8%";
        assert!(parse_image_row(line, &cfg()).is_none());
    }

    #[test]
    fn parse_image_row_parses_visible_image() {
        // 1520x2239 at 500ppi -> 218.9 x 322.4 pt (substantial, not full-page).
        let line = "   3     0 image    1520  2239  rgb     3   8  image  no       128  0   500   500 86.2K 0.9%";
        let (page, signal) = parse_image_row(line, &cfg()).expect("parse");
        assert_eq!(page, 3);
        assert!(signal.has_substantial);
        assert!(!signal.has_full_page);
    }

    #[test]
    fn parse_image_row_full_page_detection() {
        // 2550x3300 at 150ppi -> 1224 x 1584 pt — full page.
        let line = "   1     0 image    2550  3300  gray    1   8  image  no       128  0   150   150 86.2K 0.9%";
        let (_page, signal) = parse_image_row(line, &cfg()).expect("parse");
        assert!(signal.has_full_page);
    }

    #[test]
    fn parse_image_row_rejects_malformed() {
        assert!(parse_image_row("not a row", &cfg()).is_none());
        assert!(
            parse_image_row("1 2 image 10 10 rgb 3 8 image no 1 0 0 0 1K 1%", &cfg()).is_none()
        );
    }
    #[test]
    fn parse_target_pages_mixed_ranges_and_singles() {
        let p = parse_target_pages("1-5,10,15-20").expect("parse");
        assert_eq!(p, vec![1, 2, 3, 4, 5, 10, 15, 16, 17, 18, 19, 20]);
    }

    #[test]
    fn parse_target_pages_dedup_and_sort() {
        let p = parse_target_pages("10,1-3,2,1").expect("parse");
        assert_eq!(p, vec![1, 2, 3, 10]);
    }

    #[test]
    fn parse_target_pages_empty_is_empty() {
        assert!(parse_target_pages("").expect("parse").is_empty());
        assert!(parse_target_pages("   ").expect("parse").is_empty());
    }

    #[test]
    fn parse_target_pages_rejects_zero_and_inverted() {
        assert!(parse_target_pages("0").is_err());
        assert!(parse_target_pages("5-2").is_err());
        assert!(parse_target_pages("1-0").is_err());
    }

    #[test]
    fn parse_target_pages_rejects_non_numeric() {
        assert!(parse_target_pages("a-b").is_err());
        assert!(parse_target_pages("x").is_err());
    }
}
