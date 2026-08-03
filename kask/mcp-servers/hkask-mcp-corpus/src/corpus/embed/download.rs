//! Download helper for the embedding pipeline.
//!
//! Delegates to `corpus::fetch::fetch_text` for the HTTP-fetch + PDF/OCR/HTML
//! pipeline. The shared `fetch_text` propagates `pdf_extract` errors (the
//! previous version in `discover/cache.rs` swallowed them via
//! `unwrap_or_default()`, masking failures as empty text).

use crate::corpus::fetch::fetch_text;
use hkask_services_core::ServiceError;

pub(crate) async fn download_text(url: &str) -> Result<String, ServiceError> {
    fetch_text(url).await
}
