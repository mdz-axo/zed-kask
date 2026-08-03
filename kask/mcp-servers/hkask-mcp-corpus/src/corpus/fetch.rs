//! Shared text fetch — HTTP download with PDF/OCR fallback and HTML stripping.
//!
//! Eliminates the duplicated HTTP-fetch → content-type-sniff → PDF-extract →
//! OCR-fallback → HTML-strip pipeline that existed in both
//! `corpus/discover/cache.rs::download_and_cache` and
//! `corpus/embed/download.rs::download_text`.

use crate::corpus::embed::{ocr_pdf_bytes, strip_html_tags};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

fn http_error(
    msg: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
) -> ServiceError {
    ServiceError::Domain {
        kind: ErrorKind::ServiceUnavailable,
        domain: DomainKind::Wallet,
        source,
        message: msg,
    }
}

/// User-Agent string for HTTP fetches.
const USER_AGENT: &str = concat!("hkask-corpus/", env!("CARGO_PKG_VERSION"));

/// Fetch content from a URL and extract text.
///
/// Handles PDFs (text extraction with OCR fallback), HTML (tag stripping),
/// and plain text. Propagates `pdf_extract` errors rather than swallowing them
/// — the previous `download_and_cache` swallowed extraction errors via
/// `unwrap_or_default()`, masking failures as empty text.
pub(crate) async fn fetch_text(url: &str) -> Result<String, ServiceError> {
    let resp = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            http_error(
                format!("Failed to build HTTP client: {e}"),
                Some(Box::new(e)),
            )
        })?
        .get(url)
        .send()
        .await
        .map_err(|e| {
            http_error(
                format!("HTTP request failed for '{url}': {e}"),
                Some(Box::new(e)),
            )
        })?;

    if !resp.status().is_success() {
        return Err(http_error(
            format!("HTTP {} for '{url}'", resp.status()),
            None,
        ));
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| {
        http_error(
            format!("Failed to read response body: {e}"),
            Some(Box::new(e)),
        )
    })?;

    let is_pdf = content_type.contains("application/pdf")
        || url.ends_with(".pdf")
        || bytes.starts_with(b"%PDF");

    if is_pdf {
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("hkask-fetch-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, &bytes)
            .map_err(|e| http_error(format!("Failed to write temp PDF: {e}"), Some(Box::new(e))))?;

        let text = pdf_extract::extract_text(&tmp_path).map_err(|e| {
            http_error(
                format!("Failed to extract text from PDF '{url}': {e}"),
                Some(Box::new(e)),
            )
        })?;

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

    let raw = String::from_utf8_lossy(&bytes).to_string();

    let is_html = content_type.contains("text/html")
        || content_type.contains("application/xhtml")
        || raw.starts_with("<!DOCTYPE")
        || raw.starts_with("<html");

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
