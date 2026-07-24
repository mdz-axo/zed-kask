use super::{WebBrowseProvider, WebError, WebExtractProvider, validate_provider_url};
use crate::research::strip_html;
use crate::research::types::*;
use async_trait::async_trait;
use std::time::Duration;

pub struct RawFetchProvider {
    client: reqwest::Client,
}

impl Default for RawFetchProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RawFetchProvider {
    pub fn new() -> Self {
        Self {
            client: super::provider_http_client(),
        }
    }
}
#[async_trait]
impl WebExtractProvider for RawFetchProvider {
    fn kind(&self) -> &str {
        "rawfetch"
    }

    async fn extract(
        &self,
        url: &str,
        _opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        // Task 6: Validate URL at provider boundary — RawFetch is the most SSRF-sensitive provider
        validate_provider_url(url)?;
        let resp =
            self.client.get(url).send().await.map_err(|e| {
                WebError::ProviderUnavailable(format!("RawFetch request failed: {e}"))
            })?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderError(format!("RawFetch read error: {e}")))?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "RawFetch error {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        Ok(ExtractedContent {
            url: url.to_string(),
            content: strip_html::strip_html(&body),
            format: "markdown".to_string(),
            metadata: None,
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Task 1: Liveness check — fetch example.com
        let resp = self
            .client
            .get("https://example.com")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("RawFetch health check failed: {e}"))
            })?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "RawFetch health check returned {}",
                resp.status()
            )))
        }
    }
}
#[async_trait]
impl WebBrowseProvider for RawFetchProvider {
    fn kind(&self) -> &str {
        "rawfetch"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        validate_provider_url(url)?;
        let resp = self
            .client
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("RawFetch browse request failed: {e}"))
            })?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderError(format!("RawFetch browse read error: {e}")))?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "RawFetch browse error {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        Ok(BrowseResult {
            url: url.to_string(),
            content: strip_html::strip_html(&body),
            instruction: Some(instruction.to_string()),
            actions_taken: vec!["raw_fetch".to_string()],
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Delegate to the extract health check (same logic)
        WebExtractProvider::health(self).await
    }
}

pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
