use super::{ProviderSearchOutput, WebError, WebSearchProvider, search_results_from};
use crate::research::types::*;
use async_trait::async_trait;

pub(crate) struct BraveProvider {
    client: reqwest::Client,
    api_key: String,
}

impl BraveProvider {
    pub fn new(api_key: String) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
        })
    }
}
#[async_trait]
impl WebSearchProvider for BraveProvider {
    fn kind(&self) -> &str {
        "brave"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![
            SearchCapability::Keyword,
            SearchCapability::News,
            SearchCapability::Freshness,
        ]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let mut params: Vec<(&str, String)> = vec![
            ("q", query.query.clone()),
            ("count", query.num_results.to_string()),
        ];
        if let Some(ref freshness) = query.freshness {
            params.push(("freshness", freshness_brave(freshness)));
        }

        let resp = self
            .client
            .get(format!("{BRAVE_API_BASE}/web/search"))
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&params)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Brave request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Brave body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("Brave auth error: {status}")),
                429 => WebError::RateLimited(format!("Brave rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Brave API error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| WebError::ProviderError(format!("Failed to parse Brave response: {e}")))?;

        let results = search_results_from(
            &parsed["web"]["results"],
            |item| item["description"].as_str().map(str::to_string),
            |item| item["source"].as_str().map(str::to_string),
            |item| item["age"].as_str().map(str::to_string),
        );

        Ok(ProviderSearchOutput {
            results,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        let resp = self
            .client
            .get(format!("{BRAVE_API_BASE}/web/search"))
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", "test"), ("count", "1")])
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Brave health check failed: {e}"))
            })?;
        if resp.status().is_success() || resp.status().as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Brave health check returned {}",
                resp.status()
            )))
        }
    }
}
