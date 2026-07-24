//! Download and cache content for the discovery pipeline.

use super::types::USER_AGENT;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use std::path::Path;

/// Download content from a URL and cache it to disk.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  url must be a valid HTTP/HTTPS URL; cache_path's parent directory must exist
/// post: content is downloaded, PDFs are text-extracted (with OCR fallback), HTML is stripped, and result is written to cache_path; Err on HTTP failure, empty content, or I/O error
#[must_use = "result must be used"]
pub async fn download_and_cache(url: &str, cache_path: &Path) -> Result<(), ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.discover", operation = "download_and_cache", url = %url, cache = %cache_path.display(), "REG");

    let resp = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            let msg = format!("HTTP client build failed: {e}");
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Wallet,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?
        .get(url)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("HTTP request failed for '{url}': {e}");
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Wallet,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    if !resp.status().is_success() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: None,
            message: format!("HTTP {} for '{url}'", resp.status()),
        });
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| {
        let msg = format!("Failed to read response body: {e}");
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let is_pdf = content_type.contains("application/pdf")
        || url.ends_with(".pdf")
        || bytes.starts_with(b"%PDF");

    let text = if is_pdf {
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("hkask-discover-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, &bytes).map_err(|e| {
            let msg = format!("Failed to write temp PDF: {e}");
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Wallet,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;
        let extracted = pdf_extract::extract_text(&tmp_path).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp_path);

        let word_count = extracted.split_whitespace().count();
        if word_count < 10 {
            tracing::warn!(url = %url, word_count = word_count, "PDF extraction near-empty — attempting OCR fallback");
            match crate::embed::ocr_pdf_bytes(&bytes, url).await {
                Ok(ocr_text) => {
                    let ocr_words = ocr_text.split_whitespace().count();
                    if ocr_words > word_count {
                        tracing::info!(url = %url, ocr_words = ocr_words, "OCR succeeded");
                        ocr_text
                    } else {
                        tracing::warn!(url = %url, "OCR also low — using extraction result");
                        extracted
                    }
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "OCR failed — using extraction result");
                    extracted
                }
            }
        } else {
            extracted
        }
    } else {
        let raw = String::from_utf8_lossy(&bytes).to_string();
        if content_type.contains("text/html")
            || raw.starts_with("<!DOCTYPE")
            || raw.starts_with("<html")
        {
            crate::embed::strip_html_tags(&raw)
        } else {
            raw
        }
    };

    if text.split_whitespace().count() < 10 {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: None,
            message: format!(
                "Downloaded content from '{url}' is too short (likely paywalled or scanned PDF without OCR)"
            ),
        });
    }

    std::fs::write(cache_path, &text).map_err(|e| {
        let msg = format!("Failed to write cache: {e}");
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    tracing::info!(target: "hkask.discover", path = %cache_path.display(), bytes = bytes.len(), words = text.split_whitespace().count(), "Cached work");

    Ok(())
}
