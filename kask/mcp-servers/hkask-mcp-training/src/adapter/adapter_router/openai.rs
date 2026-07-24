//! Shared OpenAI-compatible inference helper.
//!
//! Called by each provider backend's `infer` method. Handles the common
//! HTTP request/response pattern for OpenAI-compatible chat completions.

use crate::adapter::adapter_port::AdapterError;
use hkask_types::template::LLMParameters;
use hkask_types::{InferenceResult, InferenceUsage};

pub(super) async fn openai_compatible_infer(
    client: &reqwest::Client,
    api_key: &str,
    endpoint_url: &str,
    prompt: &str,
    params: &LLMParameters,
    model_name: &str,
) -> Result<InferenceResult, AdapterError> {
    if api_key.is_empty() {
        return Err(AdapterError::ProviderUnavailable("API key not set".into()));
    }
    let body = serde_json::json!({
        "model": model_name,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": params.temperature,
        "top_p": params.top_p,
        "max_tokens": params.max_tokens,
    });
    let response = client
        .post(format!("{}/chat/completions", endpoint_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| AdapterError::Internal(format!("Inference request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(AdapterError::Internal(format!(
            "Inference returned {status}: {error_body}"
        )));
    }
    let response_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AdapterError::Internal(format!("Failed to parse inference response: {e}")))?;
    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let usage = serde_json::from_value(response_json["usage"].clone()).unwrap_or(InferenceUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    });
    Ok(InferenceResult {
        text: content,
        model: model_name.to_string(),
        usage,
        finish_reason: response_json["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("stop")
            .to_string(),
        token_probabilities: None,
        tool_calls: vec![],
        reasoning: None,
    })
}
