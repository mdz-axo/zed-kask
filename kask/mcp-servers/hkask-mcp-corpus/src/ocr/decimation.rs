//! PDF Decimation — Render PDF pages to images via pdftoppm.
//!
//! Converts a PDF file into per-page `DynamicImage` buffers for the
//! OCR pipeline. Uses `pdftoppm` from poppler-utils as a subprocess.
//! Falls back gracefully if poppler is not installed.
//!
//! Applies Otsu binarization to each page image for clean B&W output
//! optimized for OCR.

use crate::ocr::PipelineError;
use image::DynamicImage;
use std::path::{Path, PathBuf};

/// Render a PDF to per-page images.
///
/// # Arguments
/// * `pdf_path` — Path to the PDF file.
/// * `dpi` — Render resolution (default: 300). Higher = better OCR quality.
///
/// # Returns
/// Ordered vector of page images, or a `PipelineError` if decimation fails.
///
/// # Preprocessing
/// Each page image is preprocessed for OCR quality via local Otsu
/// binarization (O(w·h), instant, free).
///
/// # Dependencies
/// Requires `pdftoppm` from poppler-utils. On failure, returns
/// `DecimationFailed` with installation guidance.
pub async fn pdf_to_images(pdf_path: &Path, dpi: u32) -> Result<Vec<DynamicImage>, PipelineError> {
    if !pdf_path.exists() {
        return Err(PipelineError::DecimationFailed(format!(
            "PDF file not found: {}",
            pdf_path.display()
        )));
    }

    // Create temp directory for page images
    let temp_dir = tempfile::tempdir().map_err(|e| {
        PipelineError::DecimationFailed(format!("Failed to create temp directory: {}", e))
    })?;
    let prefix = temp_dir.path().join("page");

    // Invoke pdftoppm
    let output = tokio::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg(dpi.to_string())
        .arg(pdf_path)
        .arg(&prefix)
        .output()
        .await
        .map_err(|e| pdftoppm_error(&e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Detect common failure modes
        if stderr.contains("May not be a PDF file") || stderr.contains("Error") {
            return Err(PipelineError::DecimationFailed(format!(
                "PDF may be corrupted or encrypted: {}",
                stderr.trim()
            )));
        }
        return Err(PipelineError::DecimationFailed(format!(
            "pdftoppm failed: {}",
            stderr.trim()
        )));
    }

    // Collect output images in page order.
    // Per-page fault tolerance: individual page load failures are logged and
    // skipped rather than aborting the entire decimation (GAP-2). This prevents
    // one corrupt page image from discarding valid pages.
    let mut images: Vec<DynamicImage> = Vec::new();
    let mut failed_pages: Vec<usize> = Vec::new();
    // pdftoppm zero-pads page numbers to varying widths (3+ digits).
    // Scan the output directory instead of guessing the padding format.
    let mut page_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(temp_dir.path()) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "png")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("page-"))
            {
                page_files.push(p);
            }
        }
    }
    page_files.sort();

    for (page, path) in page_files.iter().enumerate() {
        let page_num = page + 1;
        match image::open(path) {
            Ok(mut img) => {
                otsu_binarize(&mut img);
                images.push(img);
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.pipeline.decimation",
                    page = page_num,
                    error = %e,
                    "Failed to load page image — skipping"
                );
                failed_pages.push(page_num);
            }
        }
    }

    if images.is_empty() {
        return Err(PipelineError::DecimationFailed(
            "pdftoppm produced no usable output images — PDF may be empty or unrenderable".into(),
        ));
    }

    if !failed_pages.is_empty() {
        tracing::warn!(
            target: "reg.pipeline.decimation",
            failed_count = failed_pages.len(),
            total_pages = images.len() + failed_pages.len(),
            failed = ?failed_pages,
            "Some pages failed to load — proceeding with partial results"
        );
    }

    // temp_dir is dropped here, cleaning up page files
    Ok(images)
}

/// Render only a specified subset of PDF pages to images.
///
/// Selective decimation for per-page triage: when only some pages need OCR,
/// render the contiguous range `[min, max]` of the requested pages via a single
/// `pdftoppm -f -l` call, then select only the requested page indices. This
/// avoids rendering (and OCR-ing) text-native pages outside the set.
///
/// `page_indices` are 0-based. The returned images are in the order of
/// `page_indices` (not document order) — callers map them back by index.
pub async fn pdf_to_images_for_pages(
    pdf_path: &Path,
    dpi: u32,
    page_indices: &[usize],
) -> Result<Vec<DynamicImage>, PipelineError> {
    if page_indices.is_empty() {
        return Ok(Vec::new());
    }
    if !pdf_path.exists() {
        return Err(PipelineError::DecimationFailed(format!(
            "PDF file not found: {}",
            pdf_path.display()
        )));
    }

    let min_page = *page_indices.iter().min().expect("non-empty") + 1; // 1-based
    let max_page = *page_indices.iter().max().expect("non-empty") + 1;

    let temp_dir = tempfile::tempdir().map_err(|e| {
        PipelineError::DecimationFailed(format!("Failed to create temp directory: {}", e))
    })?;
    let prefix = temp_dir.path().join("page");

    let output = tokio::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg(dpi.to_string())
        .arg("-f")
        .arg(min_page.to_string())
        .arg("-l")
        .arg(max_page.to_string())
        .arg(pdf_path)
        .arg(&prefix)
        .output()
        .await
        .map_err(|e| pdftoppm_error(&e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PipelineError::DecimationFailed(format!(
            "pdftoppm failed: {}",
            stderr.trim()
        )));
    }

    // Collect rendered page files, sorted lexicographically (pdftoppm pads to a
    // consistent width, so sort order matches page order within the range).
    let mut page_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(temp_dir.path()) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "png")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("page-"))
            {
                page_files.push(p);
            }
        }
    }
    page_files.sort();

    // Select only the requested indices. page_files[k] is page (min_page + k).
    let mut images: Vec<DynamicImage> = Vec::with_capacity(page_indices.len());
    for &idx in page_indices {
        let file_pos = idx + 1 - min_page;
        let Some(path) = page_files.get(file_pos) else {
            tracing::warn!(
                target: "reg.pipeline.decimation",
                page = idx + 1,
                "requested page not rendered — skipping"
            );
            continue;
        };
        match image::open(path) {
            Ok(mut img) => {
                otsu_binarize(&mut img);
                images.push(img);
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.pipeline.decimation",
                    page = idx + 1,
                    error = %e,
                    "Failed to load page image — skipping"
                );
            }
        }
    }

    Ok(images)
}

/// Otsu binarization — local, instant, free.
///
/// Computes the optimal threshold that minimizes intra-class variance,
/// then applies it to produce a clean black/white image.
/// O(w·h), no allocations beyond the output buffer.
fn otsu_binarize(image: &mut DynamicImage) {
    // Convert to grayscale for histogram computation
    let gray = image.to_luma8();

    // Compute Otsu threshold from histogram
    let hist = histogram(&gray);
    let otsu_level = otsu_level(&hist);

    // Apply binary threshold inline (pixels > otsu_level → 255, else → 0).
    // Inlined from imageproc::contrast::threshold to drop the imageproc dep,
    // which pulls rayon, ttf-parser, ab_glyph, and num-complex.
    let level = otsu_level;
    let mut binarized = gray;
    for px in binarized.iter_mut() {
        *px = if *px > level { 255 } else { 0 };
    }

    // GAP-4: Regulation variety — detect potential over-thresholding (uniform output)
    let unique: std::collections::BTreeSet<u8> = binarized.as_raw().iter().copied().collect();
    if unique.len() <= 1 {
        tracing::warn!(
            target: "reg.pipeline.decimation.binarize",
            otsu_level = otsu_level,
            unique_values = unique.len(),
            "Otsu binarization produced uniform output — possible over-thresholding"
        );
    }

    *image = DynamicImage::ImageLuma8(binarized);
}

/// Build a 256-bin histogram from a grayscale image.
fn histogram(gray: &image::GrayImage) -> [u32; 256] {
    let mut hist = [0u32; 256];
    for &p in gray.as_raw().iter() {
        hist[p as usize] += 1;
    }
    hist
}

/// Otsu's method: find threshold that minimizes intra-class variance.
fn otsu_level(hist: &[u32; 256]) -> u8 {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return 128; // fallback for empty images
    }

    let mut sum_b: f64 = 0.0;
    let mut w_b: f64 = 0.0;
    let mut max_variance: f64 = 0.0;
    let mut best_threshold: u8 = 0;

    let sum_total: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &count)| i as f64 * count as f64)
        .sum();

    for (t, &count_val) in hist.iter().enumerate() {
        let count = count_val as f64;
        w_b += count;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total as f64 - w_b;
        if w_f == 0.0 {
            break;
        }

        sum_b += t as f64 * count;
        let mean_b = sum_b / w_b;
        let mean_f = (sum_total - sum_b) / w_f;

        let variance = w_b * w_f * (mean_b - mean_f).powi(2);
        if variance > max_variance {
            max_variance = variance;
            best_threshold = t as u8;
        }
    }

    best_threshold
}

/// Format a user-friendly error when pdftoppm is not found.
fn pdftoppm_error(detail: &str) -> PipelineError {
    if detail.contains("No such file") || detail.contains("not found") {
        PipelineError::DecimationFailed(
            "pdftoppm is not installed. Install poppler-utils:\n  Ubuntu/Debian: sudo apt install poppler-utils\n  macOS: brew install poppler\n  Fedora: sudo dnf install poppler-utils"
                .into(),
        )
    } else {
        PipelineError::DecimationFailed(format!("Failed to run pdftoppm: {}", detail))
    }
}
