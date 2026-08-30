use super::{
    ProviderSearchOutput, WebBrowseProvider, WebError, WebExtractProvider, WebSearchProvider,
    reqwest_error_detail,
};
use crate::research::types::*;
use async_trait::async_trait;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct FirecrawlProvider {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl FirecrawlProvider {
    pub fn new(api_key: Option<String>) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
        })
    }

    fn auth_header(&self) -> Result<String, WebError> {
        self.api_key
            .as_ref()
            .map(|k| format!("Bearer {k}"))
            .ok_or(WebError::NoProvider)
    }
}

/// Parse a Firecrawl v2 search response. v2 nests the web results under
/// `data.web` (`{"success":true,"data":{"web":[…],"news":[…]}}`); the
/// former `data`-as-array read was the v1 shape and silently returned zero
/// results on every successful v2 call.
fn parse_v2_search_results(parsed: &serde_json::Value) -> Vec<SearchResult> {
    parsed["data"]["web"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(SearchResult {
                        title: item["title"].as_str()?.to_string(),
                        url: item["url"].as_str()?.to_string(),
                        description: item["description"]
                            .as_str()
                            .or_else(|| item["snippet"].as_str())
                            .map(|s| s.to_string()),
                        source: None,
                        published: None,
                        provider: None,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
#[async_trait]
impl WebSearchProvider for FirecrawlProvider {
    fn kind(&self) -> &str {
        "firecrawl"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Keyword, SearchCapability::Semantic]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let auth = self.auth_header()?;
        let payload = serde_json::json!({ "query": query.query, "limit": query.num_results });
        let resp = self
            .client
            .post(format!("{FIRECRAWL_API_BASE}/search"))
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!(
                    "Firecrawl request failed: {}",
                    reqwest_error_detail(&e)
                ))
            })?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Firecrawl body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => {
                    WebError::ProviderUnavailable(format!("Firecrawl auth error: {status}"))
                }
                429 => WebError::RateLimited(format!("Firecrawl rate limited: {status}")),
                _ => WebError::ProviderError(format!("Firecrawl API error {status}")),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Firecrawl response: {e}"))
        })?;

        let results = parse_v2_search_results(&parsed);

        Ok(ProviderSearchOutput {
            results,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        if self.api_key.is_none() {
            return Err(WebError::NoProvider);
        }
        Ok(())
    }
}
#[async_trait]
impl WebExtractProvider for FirecrawlProvider {
    fn kind(&self) -> &str {
        "firecrawl"
    }

    async fn extract(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        // SSRF validation is at the pool boundary (extract_with_fallback).
        let auth = self.auth_header()?;
        let mut payload = serde_json::json!({ "url": url });
        match opts.format.as_str() {
            "json" => {
                payload["formats"] = serde_json::json!(["json"]);
                if let Some(ref prompt) = opts.json_prompt {
                    payload["jsonOptions"] = serde_json::json!({ "prompt": prompt });
                }
            }
            _ => {
                payload["formats"] = serde_json::json!(["markdown"]);
            }
        }
        if opts.main_content_only {
            payload["onlyMainContent"] = serde_json::json!(true);
        }
        if opts.wait_for_ms > 0 {
            payload["waitFor"] = serde_json::json!(opts.wait_for_ms);
        }

        let resp = self
            .client
            .post(format!("{FIRECRAWL_API_BASE}/scrape"))
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Firecrawl extract failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Firecrawl body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Firecrawl extract error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Firecrawl extract response: {e}"))
        })?;

        let content = if opts.format == "json" {
            parsed["data"]["json"].to_string()
        } else {
            parsed["data"]["markdown"]
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        let metadata = parsed["data"]["metadata"]
            .as_object()
            .map(|m| serde_json::Value::Object(m.clone()));

        Ok(ExtractedContent {
            url: url.to_string(),
            content,
            format: opts.format.clone(),
            metadata,
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        if self.api_key.is_none() {
            return Err(WebError::NoProvider);
        }
        Ok(())
    }
}
#[async_trait]
impl WebBrowseProvider for FirecrawlProvider {
    fn kind(&self) -> &str {
        "firecrawl"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        // SSRF validation is at the pool boundary (browse_with_fallback).
        let auth = self.auth_header()?;
        let payload = serde_json::json!({
            "url": url, "formats": ["markdown"],
            "actions": [{ "type": "wait", "milliseconds": 2000u64 }],
        });
        let resp = self
            .client
            .post(format!("{FIRECRAWL_API_BASE}/scrape"))
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Firecrawl browse failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Firecrawl body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Firecrawl browse error {status}"
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Firecrawl browse response: {e}"))
        })?;

        Ok(BrowseResult {
            url: url.to_string(),
            content: parsed["data"]["markdown"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            instruction: Some(instruction.to_string()),
            actions_taken: vec!["scrape".to_string()],
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        if self.api_key.is_none() {
            return Err(WebError::NoProvider);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Firecrawl v2 nests web results under `data.web`. The former
    /// `data`-as-array read (the v1 shape) silently returned zero results on
    /// every successful v2 call — an empty success, the exact defect class
    /// this server's contracts forbid. Pins the v2 shape.
    #[test]
    fn parse_v2_search_results_reads_data_web_array() {
        let body = serde_json::json!({
            "success": true,
            "data": {
                "web": [
                    {"title": "Speedtest", "url": "https://www.speedtest.net/",
                     "description": "Broadband speed test"},
                    {"title": "No URL", "description": "skipped — url is required"}
                ],
                "news": []
            }
        });
        let results = parse_v2_search_results(&body);
        assert_eq!(results.len(), 1, "only the entry with a url is kept");
        assert_eq!(results[0].title, "Speedtest");
        assert_eq!(results[0].url, "https://www.speedtest.net/");
        assert_eq!(
            results[0].description.as_deref(),
            Some("Broadband speed test")
        );
    }

    /// A v1-shaped body (`data` as a bare array) must NOT parse as success â
    /// v2 is the API version in `FIRECRAWL_API_BASE`; silently accepting both
    /// shapes would re-open the empty-success hole if the v1 read returned.
    #[test]
    fn parse_v2_search_results_rejects_v1_data_array() {
        let v1_body = serde_json::json!({
            "success": true,
            "data": [{"title": "t", "url": "https://example.com"}]
        });
        assert!(
            parse_v2_search_results(&v1_body).is_empty(),
            "v1-shaped data array must yield no results â v2 nests under data.web"
        );
    }
}
