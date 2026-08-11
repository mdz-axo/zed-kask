//! AtlasCloud backend — task-based media generation (image/video/audio/asr)
//! via the AtlasCloud API (`https://api.atlascloud.ai/api/v1`).
//!
//! AtlasCloud uses a submit+poll pattern:
//!   1. `POST /model/{generateImage|generateVideo|generateAudio|transcribe}`
//!      → `{ data: { id: predictionId } }`
//!   2. `GET /model/getPrediction?id={predictionId}` → poll every 3s
//!      → `{ data: { status, output, error } }` (status: pending → completed/failed)
//!
//! This backend implements `MediaProvider` for the ops AtlasCloud serves:
//! `GenerateImage`, `GenerateVideo`, `GenerateSpeech`, `Transcribe`.
//! It is registered in `MediaRouter::new` alongside `FalBackend` and
//! `DeepInfraBackend`. Auth: `Authorization: Bearer {ATLASCLOUD_API_KEY}`.

use crate::config::InferenceConfig;
use crate::openai_compat::sanitize_error_body;
use crate::provider::{MediaOp, MediaProvider};
use hkask_types::{InferenceError, MediaGenerateParams};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// Maximum number of poll iterations before giving up on an AtlasCloud
/// prediction. Coupled with [`ATLASCLOUD_POLL_INTERVAL`] to form the timeout.
const ATLASCLOUD_MAX_POLLS: u32 = 200;

/// Sleep between AtlasCloud poll iterations.
const ATLASCLOUD_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// AtlasCloud media generation backend (task-based submit + poll).
pub struct AtlasCloudBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl AtlasCloudBackend {
    /// Create a new AtlasCloud backend from inference config.
    ///
    /// Returns `Err` if `atlascloud_api_key` is empty.
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.atlascloud_api_key.is_empty() {
            return Err(InferenceError::Connection(
                "AtlasCloud API key not configured (set ATLASCLOUD_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.atlascloud_base_url.clone(),
            api_key: config.atlascloud_api_key.clone(),
            client,
        })
    }

    /// Submit a generation request and poll for the result.
    ///
    /// POST `{base_url}{endpoint}` with `{ model, ...params }` → prediction id.
    /// GET `{base_url}/model/getPrediction?id={id}` → poll every 3s, max 200 attempts.
    async fn submit_and_poll(&self, endpoint: &str, body: Value) -> Result<Value, InferenceError> {
        // Submit
        let submit_url = format!("{}{}", self.base_url, endpoint);
        let resp = self
            .client
            .post(&submit_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("AtlasCloud submit failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "AtlasCloud submit {status}: {}",
                sanitize_error_body(&text)
            )));
        }

        let prediction: Value = resp
            .json()
            .await
            .map_err(|e| InferenceError::Json(format!("AtlasCloud response parse: {e}")))?;

        let prediction_id = prediction["data"]["id"].as_str().ok_or_else(|| {
            InferenceError::Connection("AtlasCloud: no prediction id in response".into())
        })?;

        // Poll
        let poll_url = format!("{}/model/getPrediction?id={}", self.base_url, prediction_id);
        for _ in 0..ATLASCLOUD_MAX_POLLS {
            let result = self
                .client
                .get(&poll_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await
                .map_err(|e| InferenceError::Connection(format!("AtlasCloud poll failed: {e}")))?;

            if !result.status().is_success() {
                let status = result.status();
                let text = result.text().await.unwrap_or_default();
                return Err(InferenceError::Connection(format!(
                    "AtlasCloud poll {status}: {}",
                    sanitize_error_body(&text)
                )));
            }

            let result_json: Value = result
                .json()
                .await
                .map_err(|e| InferenceError::Json(format!("AtlasCloud poll parse: {e}")))?;

            if let Some(status) = result_json["data"]["status"].as_str() {
                match status {
                    "completed" | "succeeded" => return Ok(result_json["data"].clone()),
                    "failed" | "error" => {
                        let error = result_json["data"]["error"]
                            .as_str()
                            .unwrap_or("unknown error");
                        return Err(InferenceError::Connection(format!(
                            "AtlasCloud generation failed: {error}"
                        )));
                    }
                    _ => {} // pending, processing — keep polling
                }
            }

            // Wait before next poll
            tokio::time::sleep(ATLASCLOUD_POLL_INTERVAL).await;
        }

        Err(InferenceError::Connection(format!(
            "AtlasCloud: prediction timed out ({}s max)",
            ATLASCLOUD_MAX_POLLS as u64 * ATLASCLOUD_POLL_INTERVAL.as_secs()
        )))
    }
}

impl MediaProvider for AtlasCloudBackend {
    fn id(&self) -> &'static str {
        "atlascloud"
    }

    fn supports(&self, op: MediaOp) -> bool {
        matches!(
            op,
            MediaOp::GenerateImage
                | MediaOp::GenerateVideo
                | MediaOp::GenerateSpeech
                | MediaOp::Transcribe
        )
    }

    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            let (endpoint, model_id) = match op {
                MediaOp::GenerateImage => (
                    "/model/generateImage",
                    "seedream/seedream-v5.0-lite-text-to-image",
                ),
                MediaOp::GenerateVideo => ("/model/generateVideo", "minimax/h3"),
                MediaOp::GenerateSpeech => ("/model/generateAudio", "seed-audio/seed-audio-1.0"),
                MediaOp::Transcribe => ("/model/transcribe", "bytedance/seed-asr-2.0"),
                _ => {
                    return Err(InferenceError::Connection(format!(
                        "AtlasCloud does not support media op: {}",
                        op.as_str()
                    )));
                }
            };

            // Build the AtlasCloud request body: { model, ...params }
            let mut body = serde_json::json!({ "model": model_id });
            if let Some(prompt) = &params.prompt {
                body["prompt"] = Value::String(prompt.clone());
            }
            if let Some(image_url) = &params.image_url {
                body["image_url"] = Value::String(image_url.clone());
            }
            if let Some(size) = &params.size {
                body["image_size"] = Value::String(size.clone());
            }
            if let Some(duration) = params.duration {
                body["duration"] = serde_json::json!(duration);
            }
            if let Some(text) = &params.text {
                body["text"] = Value::String(text.clone());
            }
            if let Some(voice) = &params.voice {
                body["voice"] = Value::String(voice.clone());
            }
            if let Some(audio_url) = &params.audio_url {
                body["audio_url"] = Value::String(audio_url.clone());
            }
            if let Some(language) = &params.language {
                body["language"] = Value::String(language.clone());
            }

            self.submit_and_poll(endpoint, body).await
        })
    }
}
