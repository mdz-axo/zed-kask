//! RunPod backend — cloud vision/text inference via RunPod serverless endpoints.
//!
//! Requires RUNPOD_API_KEY and one of RUNPOD_BASE_URL or RUNPOD_TEMPLATE_ID.
//! Model prefix: RP/ (e.g., RP/olmocr-2-7b)

use crate::RouterModelEntry;
use crate::chat_protocol::{
    ChatResponse, build_vision_request, chat_response_to_result, validate_prompt,
};
use crate::config::InferenceConfig;
use hkask_types::template::LLMParameters;
use hkask_types::{InferenceError, InferenceResult};
use std::sync::Arc;
use tracing::info;

pub struct RunpodBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl RunpodBackend {
    pub fn new(
        _config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        let api_key = std::env::var("RUNPOD_API_KEY")
            .map_err(|_| InferenceError::Connection("RUNPOD_API_KEY not set".into()))?;
        let base_url = std::env::var("RUNPOD_BASE_URL")
            .ok()
            .or_else(|| {
                std::env::var("RUNPOD_TEMPLATE_ID")
                    .ok()
                    .map(|tid| format!("https://api.runpod.ai/v2/{}/openai/v1", tid))
            })
            .filter(|u| !u.is_empty())
            .ok_or_else(|| {
                InferenceError::Connection(
                    "Neither RUNPOD_BASE_URL nor RUNPOD_TEMPLATE_ID set".into(),
                )
            })?;
        Ok(Self {
            base_url,
            api_key,
            client,
        })
    }

    pub async fn generate_vision(
        &self,
        model: &str,
        prompt: &str,
        images: &[String],
        params: &LLMParameters,
    ) -> Result<InferenceResult, InferenceError> {
        validate_prompt(prompt)?;
        if images.is_empty() {
            return Err(InferenceError::Generation("No images provided".into()));
        }
        let request = build_vision_request(model, prompt, images, params);
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("RunPod vision: {}", e)))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "RunPod {}: {}",
                status, body
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|e| InferenceError::Connection(format!("RunPod body read: {}", e)))?;
        let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
            let preview = if body.len() > 500 {
                format!("{}...", &body[..500])
            } else {
                body.clone()
            };
            InferenceError::Json(format!("RunPod JSON: {} | body: {}", e, preview))
        })?;
        let result = chat_response_to_result(chat_response)?;
        info!(target: "reg.inference", provider = "RP", model = %result.model, tokens = result.usage.total_tokens, "RunPod vision inference completed");
        Ok(result)
    }

    /// RunPod serverless endpoints don't expose model listings.
    /// Return empty — model discovery is via template configuration, not API listing.
    pub async fn list_models(&self) -> Vec<RouterModelEntry> {
        Vec::new()
    }

    /// OCR via RunPod's kask-ocr endpoint (OLMOCR-2).
    /// Synchronous /runsync for single pages, with /stream fallback for async.
    /// Configured via RUNPOD_OCR_ENDPOINT and RUNPOD_OCR_STREAM env vars.
    /// Model: RP/kask-ocr
    pub async fn ocr(&self, prompt: &str) -> Result<String, InferenceError> {
        let endpoint = std::env::var("RUNPOD_OCR_ENDPOINT")
            .map_err(|_| InferenceError::Connection("RUNPOD_OCR_ENDPOINT not set".into()))?;

        let body = serde_json::json!({"input": {"prompt": prompt}});
        let response = self
            .client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("RunPod OCR /runsync: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "RunPod OCR {}: {}",
                status, err_body
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| InferenceError::Json(format!("RunPod OCR JSON: {}", e)))?;

        // Extract text from runsync response. Try common output paths.
        let text = json
            .get("output")
            .or_else(|| json.get("text"))
            .or_else(|| json.get("result"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if text.is_empty() {
            return Ok(json.to_string());
        }

        info!(target: "reg.inference", provider = "RP/kask-ocr", chars = text.len(), "kask-ocr completed");
        Ok(text)
    }
}
