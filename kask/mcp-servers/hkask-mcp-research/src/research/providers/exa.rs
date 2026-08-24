use super::{ProviderSearchOutput, WebBrowseProvider, WebError, WebSearchProvider, truncate_str};
use crate::research::types::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct ExaProvider {
    client: reqwest::Client,
    api_key: String,
}

impl ExaProvider {
    pub fn new(api_key: String) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
        })
    }

    pub async fn find_similar(
        &self,
        url: &str,
        num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        let payload = serde_json::json!({
            "url": url,
            "numResults": num_results,
            "contents": { "text": { "maxCharacters": 300 } },
        });

        let resp = self
            .client
            .post(format!("{EXA_API_BASE}/findSimilar"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa findSimilar failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => {
                    WebError::ProviderUnavailable(format!("Exa findSimilar auth error: {status}"))
                }
                429 => WebError::RateLimited(format!("Exa findSimilar rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Exa findSimilar error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Exa findSimilar response: {e}"))
        })?;

        let mut semantic_scores = HashMap::new();
        let mut content_previews = HashMap::new();
        let results = parsed["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let url = item["url"].as_str()?;
                        if let Some(score) = item["score"].as_f64() {
                            semantic_scores.insert(url.to_lowercase(), score);
                        }
                        if let Some(text) = item["text"].as_str() {
                            content_previews.insert(url.to_lowercase(), text.to_string());
                        }
                        Some(SearchResult {
                            title: item["title"].as_str()?.to_string(),
                            url: url.to_string(),
                            description: item["text"].as_str().map(|s| truncate_str(s, 300)),
                            source: item["author"].as_str().map(|s| s.to_string()),
                            published: item["publishedDate"].as_str().map(|s| s.to_string()),
                            provider: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ProviderSearchOutput {
            results,
            semantic_scores,
            content_previews,
            ..Default::default()
        })
    }
}
#[async_trait]
impl WebSearchProvider for ExaProvider {
    fn kind(&self) -> &str {
        "exa"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Semantic, SearchCapability::Keyword]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let mut payload = serde_json::json!({
            "query": query.query,
            "numResults": query.num_results,
            "type": "neural",
            "contents": { "text": { "maxCharacters": 300 } },
        });
        if !query.include_domains.is_empty() {
            payload["includeDomains"] = serde_json::json!(query.include_domains);
        }
        if !query.exclude_domains.is_empty() {
            payload["excludeDomains"] = serde_json::json!(query.exclude_domains);
        }

        let resp = self
            .client
            .post(format!("{EXA_API_BASE}/search"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa request failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("Exa auth error: {status}")),
                429 => WebError::RateLimited(format!("Exa rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Exa API error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| WebError::ProviderError(format!("Failed to parse Exa response: {e}")))?;

        let mut semantic_scores = HashMap::new();
        let mut content_previews = HashMap::new();
        let results = parsed["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let url = item["url"].as_str()?;
                        if let Some(score) = item["score"].as_f64() {
                            semantic_scores.insert(url.to_lowercase(), score);
                        }
                        if let Some(text) = item["text"].as_str() {
                            content_previews.insert(url.to_lowercase(), text.to_string());
                        }
                        Some(SearchResult {
                            title: item["title"].as_str()?.to_string(),
                            url: url.to_string(),
                            description: item["text"].as_str().map(|s| truncate_str(s, 300)),
                            source: item["author"].as_str().map(|s| s.to_string()),
                            published: item["publishedDate"].as_str().map(|s| s.to_string()),
                            provider: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ProviderSearchOutput {
            results,
            semantic_scores,
            content_previews,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Lightweight check: send a minimal search request and verify we get
        // a non-5xx response. A 401/403 means the key is invalid (unhealthy);
        // a 429 means the service is alive but rate-limited (healthy).
        let payload = serde_json::json!({ "query": "test", "numResults": 1 });
        let resp = self
            .client
            .post(format!("{EXA_API_BASE}/search"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa health check failed: {e}")))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Exa health check returned {status}"
            )))
        }
    }
}

/// Exa's `/contents` endpoint returns page text — a browse substitute for
/// content-rich pages. Not a headless browser: no JS execution. Wired as a
/// `WebBrowseProvider` so `web_browse` falls back across Firecrawl → Tavily → Exa.
#[async_trait]
impl WebBrowseProvider for ExaProvider {
    fn kind(&self) -> &str {
        "exa"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        // SSRF validation is at the pool boundary (browse_with_fallback).
        // Exa's /contents endpoint takes URLs and returns their text content.
        let payload = serde_json::json!({
            "urls": [url],
            "contents": { "text": { "maxCharacters": 10000 } },
        });
        let resp = self
            .client
            .post(format!("{EXA_API_BASE}/contents"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa browse failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("Exa auth error: {status}")),
                429 => WebError::RateLimited(format!("Exa rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Exa browse error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Exa browse response: {e}"))
        })?;

        // Exa /contents returns {"results": [{"url": ..., "text": ...}]}
        let content = parsed["results"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|item| item["text"].as_str())
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
        let payload = serde_json::json!({ "query": "test", "numResults": 1 });
        let resp = self
            .client
            .post(format!("{EXA_API_BASE}/search"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa health check failed: {e}")))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Exa health check returned {status}"
            )))
        }
    }
}
