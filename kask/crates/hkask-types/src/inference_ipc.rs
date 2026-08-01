//! Inference IPC protocol — shared types for the MCP inference bridge.
//!
//! When zed-kask launches MCP server child processes, it passes a Unix socket
//! path via the `HKASK_INFERENCE_SOCKET` env var. The MCP server connects to
//! this socket and sends inference requests as JSON-RPC messages. Zed handles
//! the requests using its own `LanguageModelRegistry` (with guard, and
//! zed's configured API keys), eliminating the need for MCP servers to have
//! their own API keys.
//!
//! ## Protocol
//!
//! Each request is a single JSON object on one line (newline-delimited JSON):
//!
//! ```json
//! {"id": 1, "method": "generate", "params": {"prompt": "...", "parameters": {...}}}
//! ```
//!
//! The response is also a single JSON object on one line:
//!
//! ```json
//! {"id": 1, "result": {"text": "...", "model": "...", "usage": {...}}}
//! ```
//!
//! or on error:
//!
//! ```json
//! {"id": 1, "error": {"code": "Generation", "message": "..."}}
//! ```
//!
//! ## Methods
//!
//! - `generate` — single prompt → result
//! - `generate_with_model` — prompt + model override → result
//! - `generate_with_messages` — message array → result
//! - `generate_vision` — prompt + images → result
//! - `embed` — model + texts → embedding vectors (OpenAI-compatible `/embeddings`)
//! - `list_models` — list available models from zed's `LanguageModelRegistry`
//! - `media_generate` — generate media (image, video, speech, transcription) via fal.ai/DeepInfra
//!
//! Streaming methods (`generate_stream*`) are not supported over IPC — the
//! IPC bridge collects the stream server-side and returns a single result.
//! This matches the existing `LanguageModelInferencePort` pattern.

use serde::{Deserialize, Serialize};

use crate::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, LLMParameters};

/// Environment variable name for the Unix socket path.
pub const INFERENCE_SOCKET_ENV: &str = "HKASK_INFERENCE_SOCKET";

/// A request from the MCP server to the zed inference bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    /// Correlation ID — matches the response to the request.
    pub id: u64,
    /// The method to call.
    pub method: InferenceMethod,
    /// Method parameters.
    pub params: InferenceParams,
}

/// The inference method to invoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceMethod {
    Generate,
    GenerateWithModel,
    GenerateWithMessages,
    GenerateVision,
    /// Generate embeddings for a batch of texts. Uses `embed_model` and
    /// `embed_texts` from `InferenceParams`. The result is returned as
    /// `InferenceOutcome::Embeddings`.
    Embed,
    /// List available models from zed's `LanguageModelRegistry`.
    /// The result is returned as `InferenceOutcome::ModelList`.
    ListModels,
    /// Generate media (image, video, speech, transcription, etc.) via
    /// fal.ai/DeepInfra backends. Uses `media_op`, `media_prompt`,
    /// `media_image_url`, `media_text`, `media_size`, `media_count`,
    /// `media_strength`, `media_duration`, `media_workflow` from
    /// `InferenceParams`. The result is returned as
    /// `InferenceOutcome::Media`.
    MediaGenerate,
}

/// Parameters for an inference request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceParams {
    pub prompt: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
    pub images: Option<Vec<String>>,
    pub parameters: LLMParameters,
    pub model_override: Option<String>,
    pub tools: Option<Vec<ChatToolDefinition>>,
    /// Embedding model string (provider-prefixed) for `InferenceMethod::Embed`.
    pub embed_model: Option<String>,
    /// Texts to embed for `InferenceMethod::Embed`.
    pub embed_texts: Option<Vec<String>>,
    // ── Media generation fields (for `InferenceMethod::MediaGenerate`) ──
    /// The media operation to perform (e.g. "generate_image", "transcribe").
    pub media_op: Option<String>,
    /// Text prompt for image/video generation.
    pub media_prompt: Option<String>,
    /// Image URL for image-to-image, image-to-video, upscale, etc.
    pub media_image_url: Option<String>,
    /// Audio URL for transcription.
    pub media_audio_url: Option<String>,
    /// Text for speech synthesis.
    pub media_text: Option<String>,
    /// Voice name for speech synthesis.
    pub media_voice: Option<String>,
    /// Image size for image generation.
    pub media_size: Option<String>,
    /// Number of images to generate.
    pub media_count: Option<u32>,
    /// Strength for image-to-image.
    pub media_strength: Option<f32>,
    /// Scale factor for upscaling.
    pub media_scale: Option<u32>,
    /// Duration for video generation.
    pub media_duration: Option<f32>,
    /// Object description for segmentation.
    pub media_object_description: Option<String>,
    /// Language hint for transcription.
    pub media_language: Option<String>,
    /// Workflow JSON for `execute_workflow`.
    pub media_workflow: Option<serde_json::Value>,
}

/// A response from the zed inference bridge to the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    /// Correlation ID — matches the request.
    pub id: u64,
    /// The result, or an error.
    #[serde(flatten)]
    pub outcome: InferenceOutcome,
}

/// The outcome of an inference request — either success or error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InferenceOutcome {
    /// Successful result.
    Result {
        #[serde(rename = "result")]
        result: InferenceResult,
    },
    /// Embedding vectors from `InferenceMethod::Embed`.
    Embeddings {
        #[serde(rename = "embeddings")]
        embeddings: Vec<Vec<f32>>,
    },
    /// Model list from `InferenceMethod::ListModels`.
    ModelList {
        #[serde(rename = "models")]
        models: Vec<ModelListEntry>,
    },
    /// Media generation result from `InferenceMethod::MediaGenerate`.
    /// The value is the raw JSON returned by fal.ai/DeepInfra.
    Media {
        #[serde(rename = "media")]
        media: serde_json::Value,
    },
    /// Error from the inference port.
    Error {
        #[serde(rename = "error")]
        error: InferenceErrorPayload,
    },
}

/// A model entry in a `ListModels` response — a serializable subset of
/// zed's `LanguageModel` trait surface, carrying the fields the corpus
/// server's `ModelInfo` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListEntry {
    /// Full model name with provider prefix (e.g. "deepinfra/qwen/qwen3-embedding-0.6b").
    pub name: String,
    /// Provider id (e.g. "deepinfra", "openrouter").
    pub provider: String,
    /// Whether the model supports vision/multimodal input.
    pub supports_vision: bool,
}

/// Serializable form of `InferenceError`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceErrorPayload {
    /// The error kind as a string (matches `InferenceError` variant names).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl From<InferenceError> for InferenceErrorPayload {
    fn from(e: InferenceError) -> Self {
        let (code, message) = match e {
            InferenceError::Connection(m) => ("Connection", m),
            InferenceError::Model(m) => ("Model", m),
            InferenceError::Generation(m) => ("Generation", m),
            InferenceError::Json(m) => ("Json", m),
            InferenceError::CircuitOpen(m) => ("CircuitOpen", m),
            InferenceError::VisionUnsupported(m) => ("VisionUnsupported", m),
        };
        Self {
            code: code.to_string(),
            message,
        }
    }
}

impl From<InferenceErrorPayload> for InferenceError {
    fn from(e: InferenceErrorPayload) -> Self {
        match e.code.as_str() {
            "Connection" => InferenceError::Connection(e.message),
            "Model" => InferenceError::Model(e.message),
            "Generation" => InferenceError::Generation(e.message),
            "Json" => InferenceError::Json(e.message),
            "CircuitOpen" => InferenceError::CircuitOpen(e.message),
            "VisionUnsupported" => InferenceError::VisionUnsupported(e.message),
            _ => InferenceError::Generation(e.message),
        }
    }
}
