//! Shared text fetch — HTTP download with PDF/OCR fallback and HTML stripping.
//!
//! Eliminates the duplicated HTTP-fetch → content-type-sniff → PDF-extract →
//! OCR-fallback → HTML-strip pipeline that existed in both
//! `corpus/discover/cache.rs::download_and_cache` and the former
//! `corpus/embed/download.rs` (now deleted — callers use `fetch_text` directly).

use crate::corpus::embed::{ocr_pdf_bytes, strip_html_tags};
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;

fn http_error(
    msg: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
) -> ServiceError {
    // HTTP content fetching retrieves source material for the corpus pipeline.
    // `DomainKind` has no `Ingest`/`Corpus` variant, so `Storage` (content
    // retrieval/persistence) is the closest fit — not `Wallet`.
    ServiceError::Domain {
        kind: ErrorKind::ServiceUnavailable,
        domain: DomainKind::Storage,
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
///
/// # SSRF
///
/// URLs reaching here originate from tool input and from `corpus_discover`
/// output, i.e. from untrusted sources. This function is the single choke
/// point for both `download_and_cache` and the embed path, so the SSRF gate
/// is applied here rather than at each call site — mirroring the research
/// server, which validates the equivalent operation at its pool boundary.
/// Without it, discover output can drive a GET to `169.254.169.254` or
/// `localhost`.
pub(crate) async fn fetch_text(
    url: &str,
    inference_port: &dyn InferencePort,
) -> Result<String, ServiceError> {
    hkask_mcp_server::validate_tool_url_with_dns(url)
        .await
        .map_err(|e| {
            // Rejected before any outbound request; not a transport failure.
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: format!("URL rejected by SSRF validation for '{url}': {e}"),
            }
        })?;

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

        if let Err(e) = std::fs::remove_file(&tmp_path) {
            tracing::warn!(
                path = %tmp_path.display(),
                error = %e,
                "temp PDF cleanup failed"
            );
        }

        let word_count = text.split_whitespace().count();
        if word_count < 10 {
            tracing::warn!(
                url = %url,
                word_count = word_count,
                "PDF text extraction returned near-empty result — attempting OCR fallback"
            );

            match ocr_pdf_bytes(&bytes, url, inference_port).await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::InferencePort;
    use std::future::Future;
    use std::pin::Pin;

    /// Stub `InferencePort` for SSRF tests. The SSRF gate rejects before any
    /// outbound request, so OCR (the only inference call in `fetch_text`) is
    /// never reached — this stub returns an error if ever called, which would
    /// surface as a test failure rather than a silent pass.
    struct StubInference;
    impl InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>> + Send + '_>>
        {
            Box::pin(async { Err(hkask_types::InferenceError::Generation("stub: SSRF gate should have rejected first".into())) })
        }
    }

    /// The SSRF gate must reject before any outbound request. These addresses
    /// are the canonical cloud-metadata and loopback targets; reaching the
    /// transport at all would be the vulnerability.
    #[tokio::test]
    async fn fetch_text_rejects_link_local_metadata_address() {
        let err = fetch_text("http://169.254.169.254/latest/meta-data/", &StubInference)
            .await
            .expect_err("link-local metadata address must be rejected");
        let message = err.to_string();
        assert!(
            message.contains("SSRF validation"),
            "expected an SSRF rejection, got: {message}"
        );
    }

    #[tokio::test]
    async fn fetch_text_rejects_loopback() {
        let err = fetch_text("http://127.0.0.1:8080/", &StubInference)
            .await
            .expect_err("loopback must be rejected");
        assert!(
            err.to_string().contains("SSRF validation"),
            "expected an SSRF rejection, got: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_text_rejects_non_http_scheme() {
        let err = fetch_text("file:///etc/passwd", &StubInference)
            .await
            .expect_err("non-http scheme must be rejected");
        assert!(
            err.to_string().contains("SSRF validation"),
            "expected an SSRF rejection, got: {err}"
        );
    }
}
