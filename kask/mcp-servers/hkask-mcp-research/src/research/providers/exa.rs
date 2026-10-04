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

    /// One request path for every Exa endpoint: send the JSON payload, map
    /// the HTTP status onto the typed error ladder (auth → unavailable,
    /// 429 → rate-limited, other → provider error with the sanitized body),
    /// and parse the response body. `op` labels every error message.
    async fn exa_post(
        &self,
        op: &str,
        path: &str,
        payload: serde_json::Value,
        timeout: Option<Duration>,
    ) -> Result<serde_json::Value, WebError> {
        let mut request = self
            .client
            .post(format!("{EXA_API_BASE}{path}"))
            .header("x-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let resp = request
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa {op} failed: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Exa body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => {
                    WebError::ProviderUnavailable(format!("Exa {op} auth error: {status}"))
                }
                429 => WebError::RateLimited(format!("Exa {op} rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Exa {op} error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }
        serde_json::from_str(&body)
            .map_err(|e| WebError::ProviderError(format!("Failed to parse Exa {op} response: {e}")))
    }

    /// Both search-shaped endpoints (`/search`, `/findSimilar`) return the
    /// same result rows — one parser builds the results, semantic scores,
    /// and content previews.
    fn parse_search_results(parsed: &serde_json::Value) -> ProviderSearchOutput {
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
                            oa_pdf_url: None,
                            provider: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ProviderSearchOutput {
            results,
            semantic_scores,
            content_previews,
            ..Default::default()
        }
    }

    /// The lightweight liveness probe both trait `health` methods share: a
    /// minimal search request is healthy on any non-5xx response — 401/403
    /// means the key is invalid (unhealthy), 429 means the service is alive
    /// but rate-limited (healthy).
    async fn health_probe(&self) -> Result<(), WebError> {
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
        let parsed = self
            .exa_post("findSimilar", "/findSimilar", payload, None)
            .await?;
        Ok(Self::parse_search_results(&parsed))
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
        let parsed = self.exa_post("search", "/search", payload, None).await?;
        Ok(Self::parse_search_results(&parsed))
    }

    async fn health(&self) -> Result<(), WebError> {
        self.health_probe().await
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
        let parsed = self
            .exa_post("browse", "/contents", payload, Some(timeout))
            .await?;

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
        self.health_probe().await
    }
}
