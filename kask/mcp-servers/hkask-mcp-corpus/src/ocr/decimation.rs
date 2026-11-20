//! Render PDF pages as color PNGs with an aspect-preserving pixel bound.
//! The vision OCR contract uses source appearance, not Otsu-thresholded glyphs.
use crate::ocr::PipelineError;
use image::DynamicImage;
use std::collections::BTreeMap;
use std::path::Path;

/// Render all pages. `long_edge` is a pixel bound, not DPI.
pub async fn pdf_to_images(
    pdf_path: &Path,
    long_edge: u32,
) -> Result<Vec<DynamicImage>, PipelineError> {
    render_pages(pdf_path, long_edge, None).await
}

/// Render selected zero-based page indices, preserving the requested order.
/// Any missing or unreadable requested page fails instead of shifting identities.
pub async fn pdf_to_images_for_pages(
    pdf_path: &Path,
    long_edge: u32,
    page_indices: &[usize],
) -> Result<Vec<DynamicImage>, PipelineError> {
    if page_indices.is_empty() {
        return Ok(Vec::new());
    }
    render_pages(pdf_path, long_edge, Some(page_indices)).await
}

async fn render_pages(
    pdf_path: &Path,
    long_edge: u32,
    selected: Option<&[usize]>,
) -> Result<Vec<DynamicImage>, PipelineError> {
    let fail = |message: String| PipelineError::DecimationFailed(message);
    if long_edge == 0 {
        return Err(fail("OCR image pixel bound must be positive".into()));
    }
    if !pdf_path.is_file() {
        return Err(fail(format!("PDF file not found: {}", pdf_path.display())));
    }
    let directory = tempfile::tempdir()
        .map_err(|e| fail(format!("Cannot create page-render directory: {e}")))?;
    let mut command = tokio::process::Command::new("pdftoppm");
    command
        .kill_on_drop(true)
        .args(["-png", "-scale-to"])
        .arg(long_edge.to_string());
    if let Some(indices) = selected {
        let first = indices
            .iter()
            .min()
            .and_then(|i| i.checked_add(1))
            .ok_or_else(|| fail("Invalid first page index".into()))?;
        let last = indices
            .iter()
            .max()
            .and_then(|i| i.checked_add(1))
            .ok_or_else(|| fail("Invalid last page index".into()))?;
        command
            .arg("-f")
            .arg(first.to_string())
            .arg("-l")
            .arg(last.to_string());
    }
    let output = command
        .arg(pdf_path)
        .arg(directory.path().join("page"))
        .output()
        .await
        .map_err(|e| fail(format!("Cannot run pdftoppm; install poppler-utils: {e}")))?;
    if !output.status.success() {
        return Err(fail(format!(
            "pdftoppm failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(directory.path())
        .map_err(|e| fail(format!("Cannot read rendered pages: {e}")))?
    {
        let path = entry
            .map_err(|e| fail(format!("Cannot enumerate rendered page: {e}")))?
            .path();
        if path.extension().is_none_or(|ext| ext != "png") {
            continue;
        }
        let index = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("page-"))
            .and_then(|number| number.parse::<usize>().ok())
            .and_then(|number| number.checked_sub(1))
            .ok_or_else(|| {
                fail(format!(
                    "Invalid rendered page identity: {}",
                    path.display()
                ))
            })?;
        if files.insert(index, path).is_some() {
            return Err(fail(format!("Duplicate rendered page index {index}")));
        }
    }
    if files.is_empty() {
        return Err(fail("pdftoppm produced no pages".into()));
    }
    let indices: Vec<usize> = match selected {
        Some(indices) => indices.to_vec(),
        None => {
            for (expected, actual) in files.keys().enumerate() {
                if expected != *actual {
                    return Err(fail(format!("Missing rendered page {}", expected + 1)));
                }
            }
            files.keys().copied().collect()
        }
    };
    indices
        .into_iter()
        .map(|index| {
            let path = files
                .get(&index)
                .ok_or_else(|| fail(format!("Requested page {} was not rendered", index + 1)))?;
            let image = image::open(path)
                .map_err(|e| fail(format!("Cannot load rendered page {}: {e}", index + 1)))?;
            Ok(DynamicImage::ImageRgb8(image.to_rgb8()))
        })
        .collect()
}
