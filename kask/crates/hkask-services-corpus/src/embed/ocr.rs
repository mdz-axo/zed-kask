//! OCR utilities for scanned PDF fallback.

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

/// Default OCR model for scanned PDF fallback.
/// Gemma 4 31B — Google's multimodal model with explicit OCR/document parsing support.
/// Override with HKASK_OCR_MODEL env var.
fn ocr_model() -> String {
    hkask_services_core::HkaskSettings::load().ocr_model()
}

/// OCR system prompt — instructs the vision model to extract text faithfully.
const OCR_SYSTEM_PROMPT: &str = "Extract all text from this document image. Output the text exactly as it appears, preserving the document structure and layout as closely as possible. If the document contains tables, preserve them in a readable format. Do not add commentary or description — only the extracted text.";

/// Attempt OCR on PDF bytes using pdftoppm decimation + per-page vision OCR.
///
/// 1. Writes PDF bytes to a temp file.
/// 2. Decimates to per-page PNG images via pdftoppm.
/// 3. OCRs each page via the inference router.
/// 4. Returns concatenated text.
///
/// Falls back to sending raw PDF bytes as base64 if pdftoppm is not installed.
#[must_use = "result must be used"]
pub async fn ocr_pdf_bytes(bytes: &[u8], url: &str) -> Result<String, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.embed", operation = "ocr_pdf_bytes", url = %url, byte_len = bytes.len(), "REG");

    let ocr_model = std::env::var("HKASK_OCR_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(ocr_model);

    // Try pdftoppm decimation first
    if let Ok(text) = ocr_via_decimation(bytes, &ocr_model).await {
        return Ok(text);
    }

    // Fallback: send raw PDF bytes as base64 to kask-ocr runsync
    let b64_data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    let runpod_key = std::env::var("RUNPOD_API_KEY").unwrap_or_default();
    let ocr_endpoint = std::env::var("RUNPOD_OCR_ENDPOINT")
        .unwrap_or_else(|_| "https://api.runpod.ai/v2/hsldzov6932wf5/runsync".into());
    let client = reqwest::Client::new();
    let prompt = format!("data:application/pdf;base64,{b64_data}\n\n{OCR_SYSTEM_PROMPT}");
    let body = serde_json::json!({"input": {"prompt": prompt}});
    let result = client
        .post(&ocr_endpoint)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", runpod_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("kask-ocr request failed: {}", e),
        })?;

    if !result.status().is_success() {
        let err = result.text().await.unwrap_or_default();
        return Err(ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("kask-ocr error: {}", err),
        });
    }

    let json: serde_json::Value = result.json().await.map_err(|e| ServiceError::Domain {
        domain: DomainKind::Wallet,
        kind: ErrorKind::ServiceUnavailable,
        source: None,
        message: format!("kask-ocr JSON parse: {}", e),
    })?;
    let text = json
        .get("output")
        .or_else(|| json.get("text"))
        .or_else(|| json.get("result"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(text)
}

/// Decimate PDF to page images and OCR each page individually.
///
/// Returns concatenated text from all pages, or an error if pdftoppm
/// is unavailable or OCR fails on any page.
async fn ocr_via_decimation(bytes: &[u8], _model: &str) -> anyhow::Result<String> {
    // Write bytes to temp PDF file
    let temp_dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!("tempdir: {}", e))?;
    let pdf_path = temp_dir.path().join("input.pdf");
    std::fs::write(&pdf_path, bytes).map_err(|e| anyhow::anyhow!("write temp PDF: {}", e))?;

    // Decimate via pdftoppm — JPEG at 72 DPI to stay within 128K token context limit
    let prefix = temp_dir.path().join("page");
    let output = std::process::Command::new("pdftoppm")
        .arg("-jpeg")
        .arg("-jpegopt")
        .arg("quality=85")
        .arg("-r")
        .arg("72")
        .arg(&pdf_path)
        .arg(&prefix)
        .output()
        .map_err(|e| anyhow::anyhow!("pdftoppm not available: {}", e))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "pdftoppm failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // Collect page images
    let mut page = 1;
    let mut page_images: Vec<(usize, Vec<u8>)> = Vec::new();
    loop {
        let page_path = format!("{}-{}.jpg", prefix.display(), page);
        let path = std::path::Path::new(&page_path);
        if !path.exists() {
            break;
        }
        let png_bytes =
            std::fs::read(path).map_err(|e| anyhow::anyhow!("read page {}: {}", page, e))?;
        page_images.push((page, png_bytes));
        page += 1;
    }

    if page_images.is_empty() {
        return Err(anyhow::anyhow!("pdftoppm produced no output images"));
    }

    // OCR each page via kask-ocr runsync endpoint
    let runpod_key = std::env::var("RUNPOD_API_KEY").unwrap_or_default();
    let ocr_endpoint = std::env::var("RUNPOD_OCR_ENDPOINT")
        .unwrap_or_else(|_| "https://api.runpod.ai/v2/hsldzov6932wf5/runsync".into());
    let client = reqwest::Client::new();

    let mut texts: Vec<String> = Vec::with_capacity(page_images.len());
    for (page_num, png_bytes) in &page_images {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png_bytes);
        let prompt = format!("data:image/jpeg;base64,{b64}\n\n{OCR_SYSTEM_PROMPT}");

        let body = serde_json::json!({"input": {"prompt": prompt}});
        let result = client
            .post(&ocr_endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", runpod_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("kask-ocr request failed for page {}: {}", page_num, e))?;

        if !result.status().is_success() {
            let err = result.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "kask-ocr error for page {}: {}",
                page_num,
                err
            ));
        }

        let json: serde_json::Value = result
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("kask-ocr JSON parse for page {}: {}", page_num, e))?;
        let text = json
            .get("output")
            .or_else(|| json.get("text"))
            .or_else(|| json.get("result"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if !text.trim().is_empty() {
            texts.push(text);
        }
    }

    Ok(texts.join("\n\n"))
}
