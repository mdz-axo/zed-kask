use super::{ProviderSearchOutput, WebBrowseProvider, WebError, WebSearchProvider};
use crate::research::types::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct TavilyProvider {
    client: reqwest::Client,
    api_key: String,
}

impl TavilyProvider {
    pub fn new(api_key: String) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
        })
    }
}
#[async_trait]
impl WebSearchProvider for TavilyProvider {
    fn kind(&self) -> &str {
        "tavily"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Keyword, SearchCapability::Semantic]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let mut payload = serde_json::json!({
            "api_key": self.api_key,
            "query": query.query,
            "max_results": query.num_results,
            "search_depth": "basic",
        });
        if !query.include_domains.is_empty() {
            payload["include_domains"] = serde_json::json!(query.include_domains);
        }
        if !query.exclude_domains.is_empty() {
            payload["exclude_domains"] = serde_json::json!(query.exclude_domains);
        }

        let resp = self
            .client
            .post(format!("{TAVILY_API_BASE}/search"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Tavily request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Tavily body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("Tavily auth error: {status}")),
                429 => WebError::RateLimited(format!("Tavily rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Tavily API error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Tavily response: {e}"))
        })?;

        let mut content_previews = HashMap::new();
        let results = parsed["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let url = item["url"].as_str()?;
                        if let Some(content) = item["content"].as_str() {
                            content_previews.insert(url.to_lowercase(), content.to_string());
                        }
                        Some(SearchResult {
                            title: item["title"].as_str()?.to_string(),
                            url: url.to_string(),
                            description: item["content"]
                                .as_str()
                                .or_else(|| item["snippet"].as_str())
                                .map(|s| s.to_string()),
                            source: None,
                            published: item["published_date"].as_str().map(|s| s.to_string()),
                            provider: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ProviderSearchOutput {
            results,
            content_previews,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Lightweight liveness check: send a minimal search request and verify
        // we get a non-5xx response. A 401/403 means the key is invalid;
        // a 429 means the service is alive but rate-limited (healthy).
        let payload = serde_json::json!({
            "api_key": self.api_key,
            "query": "test",
            "max_results": 1,
        });
        let resp = self
            .client
            .post(format!("{TAVILY_API_BASE}/search"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Tavily health check failed: {e}"))
            })?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Tavily health check returned {status}"
            )))
        }
    }
}

/// Tavily's `/extract` endpoint returns rendered page content — a legitimate
/// browse substitute for JS-light pages. Not a headless browser: it won't
/// execute JS or handle interactive flows. For JS-heavy pages, Firecrawl
/// remains the better choice. Wired as a `WebBrowseProvider` so `web_browse`
/// falls back across Firecrawl → Tavily → Exa.
#[async_trait]
impl WebBrowseProvider for TavilyProvider {
    fn kind(&self) -> &str {
        "tavily"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        // SSRF validation is at the pool boundary (browse_with_fallback).
        let payload = serde_json::json!({ "api_key": self.api_key, "urls": [url] });
        let resp = self
            .client
            .post(format!("{TAVILY_API_BASE}/extract"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Tavily browse failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Tavily body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("Tavily auth error: {status}")),
                429 => WebError::RateLimited(format!("Tavily rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Tavily browse error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Tavily browse response: {e}"))
        })?;

        // Tavily /extract returns {"results": [{"url": ..., "raw_content": ...}]}
        let content = parsed["results"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|item| item["raw_content"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(BrowseResult {
            url: url.to_string(),
            content,
            instruction: Some(instruction.to_string()),
            actions_taken: vec!["extract".to_string()],
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Reuse the search health check — same auth path.
        let payload =
            serde_json::json!({ "api_key": self.api_key, "query": "test", "max_results": 1 });
        let resp = self
            .client
            .post(format!("{TAVILY_API_BASE}/search"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Tavily health check failed: {e}"))
            })?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Tavily health check returned {status}"
            )))
        }
    }
}
