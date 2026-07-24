//! Download helper for the embedding pipeline.

use super::ocr::ocr_pdf_bytes;
use super::types::USER_AGENT;
use crate::embed::html::strip_html_tags;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

pub(crate) async fn download_text(url: &str) -> Result<String, ServiceError> {
    let resp = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            let msg = format!("Failed to build HTTP client: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?
        .get(url)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("HTTP request failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    if !resp.status().is_success() {
        return Err(ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("HTTP {} for {}", resp.status(), url),
        });
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| {
        let msg = format!("Failed to read response: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    // ── PDF detection: Content-Type or .pdf extension ──
    let is_pdf = content_type.contains("application/pdf")
        || url.ends_with(".pdf")
        || bytes.starts_with(b"%PDF");

    if is_pdf {
        // Write PDF bytes to a temp file for pdf-extract
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("hkask-download-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, &bytes).map_err(|e| {
            let msg = format!("Failed to write temp PDF: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        let text = pdf_extract::extract_text(&tmp_path).map_err(|e| {
            let msg = format!("Failed to extract text from PDF '{}': {e}", url);
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_path);

        let word_count = text.split_whitespace().count();
        if word_count < 10 {
            tracing::warn!(
                url = %url,
                word_count = word_count,
                "PDF text extraction returned near-empty result — attempting OCR fallback"
            );

            match ocr_pdf_bytes(&bytes, url).await {
                Ok(ocr_text) => {
                    let ocr_words = ocr_text.split_whitespace().count();
                    if ocr_words > word_count {
                        tracing::info!(
                            url = %url,
                            ocr_words = ocr_words,
                            extracted_words = word_count,
                            method = "ocr_fallback",
                            "OCR succeeded where text extraction failed"
                        );
                        return Ok(ocr_text);
                    }
                    tracing::warn!(
                        url = %url,
                        ocr_words = ocr_words,
                        "OCR also returned low word count — returning extraction result"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        url = %url,
                        error = %e,
                        "OCR fallback failed — returning extraction result"
                    );
                }
            }
        }

        tracing::info!(
            url = %url,
            word_count = word_count,
            method = "pdf_extract",
            "Downloaded and extracted PDF"
        );
        return Ok(text);
    }

    // ── HTML detection ──
    let is_html = content_type.contains("text/html")
        || content_type.contains("application/xhtml")
        || bytes.starts_with(b"<!DOCTYPE")
        || bytes.starts_with(b"<html");

    let raw = String::from_utf8_lossy(&bytes).to_string();

    if is_html {
        let text = strip_html_tags(&raw);
        tracing::info!(
            url = %url,
            word_count = text.split_whitespace().count(),
            method = "html_strip",
            "Downloaded and stripped HTML"
        );
        return Ok(text);
    }

    Ok(raw)
}
