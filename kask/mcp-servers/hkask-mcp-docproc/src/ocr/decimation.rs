//! PDF Decimation — Render PDF pages to images via pdftoppm.
//!
//! Converts a PDF file into per-page `DynamicImage` buffers for the
//! OCR pipeline. Uses `pdftoppm` from poppler-utils as a subprocess.
//! Falls back gracefully if poppler is not installed.
//!
//! Applies Otsu binarization to each page image for clean B&W output
//! optimized for OCR. Optional fal.ai `docres` enhancement available
//! when `HKASK_USE_FAL_DOCRES=true` and `FA_API_KEY` is set.

use crate::ocr::PipelineError;
use image::DynamicImage;
use std::path::{Path, PathBuf};
use std::process::Command;

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
/// Each page image is preprocessed for OCR quality:
/// - Default: local Otsu binarization (O(w·h), instant, free).
/// - Optional: fal.ai `docres` when `FA_API_KEY` is set
///   (falls back to Otsu on any failure).
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
    let output = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg(dpi.to_string())
        .arg(pdf_path)
        .arg(&prefix)
        .output()
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
                preprocess_via_fal(&mut img).await;
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

    let output = Command::new("pdftoppm")
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
                preprocess_via_fal(&mut img).await;
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

/// Preprocess a page image for OCR quality improvement.
///
/// Default: local Otsu binarization — O(w·h), instant, free.
/// Optional: fal.ai `docres` when `HKASK_USE_FAL_DOCRES=true` AND
/// `FA_API_KEY` is set. ~40s latency — opt-in only.
pub(crate) async fn preprocess_via_fal(image: &mut DynamicImage) {
    // Otsu first — always instant
    otsu_binarize(image);

    // fal.ai docres is opt-in only (explicit env var required due to ~40s latency)
    let use_fal = std::env::var("HKASK_USE_FAL_DOCRES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    if !use_fal {
        return;
    }

    let api_key = std::env::var("FA_API_KEY").unwrap_or_default();

    if api_key.is_empty() {
        tracing::warn!(target: "reg.pipeline.ocr", "HKASK_USE_FAL_DOCRES set but no API key found");
        return;
    }

    // Try fal.ai enhancement on top of Otsu-binarized image
    if let Some(enhanced) = try_fal_docres(image, &api_key).await {
        tracing::info!(target: "reg.pipeline.ocr", "fal.ai docres enhancement applied");
        *image = enhanced;
    } else {
        tracing::warn!(target: "reg.pipeline.ocr", "fal.ai docres failed, keeping Otsu result");
    }
}

/// Try fal.ai docres binarization. Returns None on any failure.
async fn try_fal_docres(image: &DynamicImage, api_key: &str) -> Option<DynamicImage> {
    // Encode image as PNG base64 data URI
    let mut png_bytes: Vec<u8> = Vec::new();
    if image
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .is_err()
    {
        return None;
    }

    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
    let data_uri = format!("data:image/png;base64,{}", b64);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .ok()?;

    let request_body = serde_json::json!({
        "image_url": data_uri,
        "task": "binarization",
    });

    let response = client
        .post("https://fal.run/fal-ai/docres")
        .header("Authorization", format!("Key {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let result: serde_json::Value = response.json().await.ok()?;
    let image_url = result["image"]["url"].as_str()?;

    let enhanced_bytes = client
        .get(image_url)
        .send()
        .await
        .ok()?
        .bytes()
        .await
        .ok()?;
    image::load_from_memory(&enhanced_bytes).ok()
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
    let level = otsu_level as u8;
    let mut binarized = gray.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Check if pdftoppm is available on this system.
    fn pdftoppm_available() -> bool {
        Command::new("pdftoppm")
            .arg("-v")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Create a minimal valid PDF for testing.
    fn minimal_pdf() -> Vec<u8> {
        // Minimal hand-crafted PDF with one page containing "Hello"
        b"%PDF-1.4\n\
          1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
          2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
          3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>endobj\n\
          4 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
          5 0 obj<</Length 44>>stream\n\
          BT /F1 24 Tf 100 700 Td (Hello) Tj ET\n\
          endstream\n\
          endobj\n\
          xref\n\
          0 6\n\
          0000000000 65535 f \n\
          0000000009 00000 n \n\
          0000000058 00000 n \n\
          0000000115 00000 n \n\
          0000000277 00000 n \n\
          0000000349 00000 n \n\
          trailer<</Size 6/Root 1 0 R>>\n\
          startxref\n\
          441\n\
          %%EOF\n"
            .to_vec()
    }

    #[tokio::test]
    async fn valid_pdf_produces_images() {
        if !pdftoppm_available() {
            eprintln!("SKIP: pdftoppm not installed");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("test.pdf");
        std::fs::write(&pdf_path, minimal_pdf()).unwrap();

        let images = pdf_to_images(&pdf_path, 150).await.unwrap();
        assert_eq!(images.len(), 1, "one-page PDF should produce one image");
        assert!(images[0].width() > 0);
        assert!(images[0].height() > 0);
    }

    #[tokio::test]
    async fn missing_file_returns_error() {
        let result = pdf_to_images(Path::new("/nonexistent/path.pdf"), 150).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, PipelineError::DecimationFailed(_)),
            "expected DecimationFailed, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn corrupt_pdf_returns_error() {
        if !pdftoppm_available() {
            eprintln!("SKIP: pdftoppm not installed");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("corrupt.pdf");
        std::fs::write(&pdf_path, b"not a pdf file").unwrap();

        let result = pdf_to_images(&pdf_path, 150).await;
        assert!(result.is_err());
    }

    #[test]
    fn otsu_binarization_bw_output() {
        // Create a text-like test image (dark text on light background)
        let mut img = DynamicImage::ImageLuma8(image::ImageBuffer::from_fn(400, 100, |x, y| {
            if !(30..=70).contains(&y) || (x / 10 + y / 15) % 3 == 0 {
                image::Luma([240]) // Light background
            } else {
                image::Luma([30]) // Dark "text" pixels
            }
        }));

        otsu_binarize(&mut img);

        // Should produce clean B&W output
        let luma = img.as_luma8().unwrap();
        let pixels = luma.as_raw();
        let unique: std::collections::BTreeSet<u8> = pixels.iter().copied().collect();
        assert!(
            unique.len() <= 2,
            "Otsu should produce ≤2 unique values (B&W), got {}: {:?}",
            unique.len(),
            unique
        );
        assert!(unique.contains(&0), "should contain black pixels");
        assert!(unique.contains(&255), "should contain white pixels");
    }

    #[test]
    fn otsu_uniform_image() {
        // Uniform gray image — Otsu should still produce valid output
        let mut img =
            DynamicImage::ImageLuma8(image::ImageBuffer::from_pixel(100, 100, image::Luma([128])));
        otsu_binarize(&mut img);
        // Should not panic, output is valid
        assert!(img.as_luma8().is_some());
    }

    #[tokio::test]
    async fn fal_docres_preprocessing_live() {
        // Only run when explicitly opted in (avoids 40s latency in default test suite)
        let use_fal = std::env::var("HKASK_USE_FAL_DOCRES")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if !use_fal {
            eprintln!("SKIP: HKASK_USE_FAL_DOCRES not set to true");
            return;
        }

        // .env is at workspace root; cargo test runs from crate dir
        let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".env");
        dotenvy::from_filename(&env_path).ok();

        let api_key = std::env::var("FA_API_KEY").unwrap_or_default();

        if api_key.is_empty() {
            eprintln!("SKIP: no fal.ai API key found");
            return;
        }

        // Create a text-like test image
        let img = DynamicImage::ImageLuma8(image::ImageBuffer::from_fn(400, 100, |x, y| {
            if !(30..=70).contains(&y) || (x / 10 + y / 15) % 3 == 0 {
                image::Luma([240])
            } else {
                image::Luma([30])
            }
        }));

        eprintln!(
            "Sending {}x{} to fal.ai docres (binarization)...",
            img.width(),
            img.height()
        );
        let start = std::time::Instant::now();

        let result = try_fal_docres(&img, &api_key).await;

        let elapsed = start.elapsed();
        match result {
            Some(enhanced) => {
                eprintln!(
                    "fal.ai returned {}x{} in {:?}",
                    enhanced.width(),
                    enhanced.height(),
                    elapsed
                );
                if let Some(luma) = enhanced.as_luma8() {
                    let unique: std::collections::BTreeSet<u8> =
                        luma.as_raw().iter().copied().collect();
                    eprintln!("Unique pixel values: {} ({:?})", unique.len(), unique);
                }
            }
            None => {
                eprintln!("fal.ai call failed after {:?}", elapsed);
            }
        }
    }
}
