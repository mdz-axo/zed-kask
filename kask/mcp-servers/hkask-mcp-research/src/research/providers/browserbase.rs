use super::{WebBrowseProvider, WebError};
use crate::research::types::*;
use async_trait::async_trait;
use std::time::Duration;

pub struct BrowserbaseProvider {
    client: reqwest::Client,
    api_key: String,
}

impl BrowserbaseProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: super::provider_http_client(),
            api_key,
        }
    }
}
#[async_trait]
impl WebBrowseProvider for BrowserbaseProvider {
    fn kind(&self) -> &str {
        "browserbase"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        let payload = serde_json::json!({
            "url": url,
            "actions": [{ "type": "wait", "milliseconds": 2000u64 }],
        });

        let resp = self
            .client
            .post(format!("{BROWSERBASE_API_BASE}/sessions"))
            .header("x-bb-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Browserbase request failed: {e}"))
            })?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Browserbase error {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Browserbase response: {e}"))
        })?;

        let content = parsed["content"]
            .as_str()
            .or_else(|| parsed["data"]["markdown"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(BrowseResult {
            url: url.to_string(),
            content,
            instruction: Some(instruction.to_string()),
            actions_taken: vec!["headless_browse".to_string()],
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Task 1: Substantive health check — liveness test via example.com
        let resp = self
            .client
            .get("https://api.browserbase.com/v1/sessions")
            .header("x-bb-api-key", &self.api_key)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Browserbase health check failed: {e}"))
            })?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else if status.as_u16() == 401 || status.as_u16() == 403 {
            Err(WebError::ProviderUnavailable(
                "Browserbase authentication failed".to_string(),
            ))
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Browserbase health check returned {status}"
            )))
        }
    }
}
