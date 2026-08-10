//! Download and cache content for the discovery pipeline.
//!
//! Delegates to `corpus::fetch::fetch_text` for the HTTP-fetch + PDF/OCR/HTML
//! pipeline, then writes the result to disk. The shared `fetch_text` propagates
//! `pdf_extract` errors — the previous version swallowed them via
//! `unwrap_or_default()`, masking extraction failures as empty text.

use crate::corpus::fetch::fetch_text;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;
use std::path::Path;

/// Download content from a URL and cache it to disk.
///
/// pre:  url must be a valid HTTP/HTTPS URL; cache_path's parent directory must exist
/// post: content is downloaded, PDFs are text-extracted (with OCR fallback), HTML is
///       stripped, and result is written to cache_path; Err on HTTP failure, empty
///       content, or I/O error
#[must_use = "result must be used"]
pub async fn download_and_cache(
    url: &str,
    cache_path: &Path,
    inference_port: &dyn InferencePort,
) -> Result<(), ServiceError> {
    tracing::info!(target: "hkask.discover", operation = "download_and_cache", url = %url, cache = %cache_path.display(), "REG");

    let text = fetch_text(url, inference_port).await?;

    if text.split_whitespace().count() < 10 {
        return Err(ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
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
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    tracing::info!(target: "hkask.discover", path = %cache_path.display(), words = text.split_whitespace().count(), "Cached work");

    Ok(())
}
