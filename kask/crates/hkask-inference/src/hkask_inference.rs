#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Inference — media generation + IPC bridge client.
//!
//! In zed-kask, chat inference routes through the zed IPC bridge
//! (`InferenceIpcClient` → `LanguageModelRegistry`). This crate provides:
//! - `MediaRouter` — fal.ai/DeepInfra media generation (image/video/speech/
//!   transcription), not covered by zed's `LanguageModel` abstraction.
//! - `InferenceIpcClient` — the IPC bridge client used by MCP servers to route
//!   chat/vision/embed through zed's `LanguageModelRegistry`.
//! - `InferenceConfig` — shared configuration (base URLs, API keys, default model).
//! - `ProviderId` — provider routing enum used by the training adapter router.
//! - `fusion_orchestrator` — multi-model panel deliberation.
//! - `artificial_analysis::FavoriteModel` — model favorites discovery.
//! - `artificial_analysis` — independent benchmark data for fusion panel discovery.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]
//!
//! # Architecture
//!
//! ```text
//! MediaRouter (implements InferencePort — media only)
//!   ├── FalBackend       — fal.ai media (image/video/speech/workflow)
//!   └── DeepInfraBackend — DeepInfra media (background removal/speech/transcription)
//!
//! InferenceIpcClient (implements InferencePort — chat/vision/embed via zed)
//!   └── Unix socket → zed LanguageModelRegistry
//!
//! resolve_inference_port() — tries IPC bridge first, falls back to MediaRouter
//! ```rust,no_run
//!
//! # Model Naming
//!
//! - `DeepInfra/meta-llama/Llama-3.3-70B-Instruct` → DeepInfra (via IPC bridge)
//! - `fal.ai/paddleocr` → fal.ai (media)
//! - `OpenRouter/openai/gpt-4o` → OpenRouter (via IPC bridge)
//! - No prefix → default model (configurable, default: OpenRouter/z-ai/glm-5.2)

pub mod artificial_analysis;
pub mod chat_protocol;
pub mod config;
pub mod deepinfra_backend;
pub mod fal_backend;
pub mod fal_workflow;
pub mod fusion_orchestrator;
pub mod inference_ipc_client;
pub mod media_router;
pub mod model_constants;
pub mod ollama_registry;
pub mod openai_compat;
pub mod openrouter_backend;

// Re-exports — public API
pub use config::{
    AlgoMethod, FusionConfig, FusionMode, FusionSkill, InferenceConfig, ProviderConfig, ProviderId,
};
pub use inference_ipc_client::InferenceIpcClient;
pub use media_router::MediaRouter;
pub use ollama_registry::{
    LocalAdapter, ModelFrom, ModelfileSpec, OllamaRegistry, RegisteredModel, RegistryError,
};

/// Unified model entry from any provider, with provider prefix applied.
#[derive(Debug, Clone)]
pub struct RouterModelEntry {
    /// Full model name with provider prefix (e.g., "ollama/qwen3:8b")
    pub prefixed_name: String,
    /// Provider this model belongs to
    pub provider: ProviderId,
    /// Raw model name without prefix
    pub model: String,
    /// Model family (e.g., "llama", "qwen2")
    pub family: Option<String>,
    /// Parameter count (e.g., "8B", "70B")
    pub parameter_size: Option<String>,
    /// Quantization level (e.g., "Q4_0")
    pub quantization_level: Option<String>,
    /// Model size in bytes (if available)
    pub size_bytes: Option<u64>,
    /// Whether the model supports vision/multimodal input.
    /// Populated via heuristic on model family name (not runtime probing).
    pub supports_vision: Option<bool>,
}

impl RouterModelEntry {
    /// Construct a RouterModelEntry from a provider and model id.
    ///
    /// expect: "The system heuristically routes multimodal models"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — canonical model entry construction
    /// pre:  model_id is non-empty
    /// post: returns RouterModelEntry with prefixed name, provider, and inferred vision support
    pub fn from_model_entry(provider: ProviderId, model_id: &str) -> Self {
        Self {
            prefixed_name: provider.prefix_model(model_id),
            provider,
            model: model_id.to_string(),
            supports_vision: Self::infer_vision_support(model_id, None),
            family: None,
            parameter_size: None,
            quantization_level: None,
            size_bytes: None,
        }
    }

    /// Heuristic: known vision-capable model families.
    ///
    /// Checks model name and family against a compiled-in allowlist
    /// plus any models listed in the `HKASK_VISION_FAMILIES` env var
    /// (comma-separated). Runtime-addition avoids recompiles.
    #[must_use]
    pub fn infer_vision_support(model: &str, family: Option<&str>) -> Option<bool> {
        const DEFAULT_VISION_FAMILIES: &[&str] = &[
            "llava",
            "bakllava",
            "minicpm-v",
            "gemma3",
            "llama3.2-vision",
            "cogvlm",
            "moondream",
            "pixtral",
            "florence",
            "paligemma",
            "qwen2-vl",
            "qwen2.5-vl",
            "qwen3-vl",
            "qwen-vl",
            "internvl",
            "phi-3-vision",
            "lighton",
            "paddleocr",
            "nemotron-parse",
            "olmocr",
            "deepseek-ocr",
        ];

        let model_lower = model.to_lowercase();
        let family_lower = family.map(|f| f.to_lowercase());

        // Check compiled-in families
        for vf in DEFAULT_VISION_FAMILIES {
            if model_lower.contains(vf) {
                return Some(true);
            }
            if let Some(ref fam) = family_lower
                && fam.contains(vf)
            {
                return Some(true);
            }
        }

        // Check env-configured families
        if let Ok(extra) = std::env::var("HKASK_VISION_FAMILIES") {
            for vf in extra.split(',').map(|s| s.trim().to_lowercase()) {
                if !vf.is_empty() && model_lower.contains(&vf) {
                    return Some(true);
                }
                if let Some(ref fam) = family_lower
                    && !vf.is_empty()
                    && fam.contains(&vf)
                {
                    return Some(true);
                }
            }
        }

        None
    }
}

/// Resolve the best available `InferencePort` for an MCP server.
///
/// This is the canonical entry point for MCP servers at startup. It tries
/// the IPC bridge first (connecting back to zed's `LanguageModelRegistry`
/// via a Unix socket), and falls back to constructing a `MediaRouter`
/// from env-var API keys when the IPC socket is not available.
///
/// # Priority
///
/// 1. `InferenceIpcClient` — if `HKASK_INFERENCE_SOCKET` is set and the
///    socket is reachable. This routes inference through zed's
///    `LanguageModelRegistry` (with fusion, guard, and zed's configured
///    API keys). Chat, vision, embed, and list_models all go through here.
/// 2. `MediaRouter` — constructed from `InferenceConfig::from_env()`.
///    This handles only media generation (fal.ai/DeepInfra). Chat/vision/
///    embed return a clear error directing the operator to the IPC bridge.
///    Used when running standalone or when the IPC socket is not available.
///
/// # Logs
///
/// Logs which path was taken at `info` level so operators can verify the
/// inference routing from server startup logs.
///
/// expect: "MCP servers route inference through zed when available, fall back to MediaRouter for media-only"
/// pre:  none (reads env vars)
/// post: returns an `Arc<dyn InferencePort>` ready for inference calls
#[must_use]
pub async fn resolve_inference_port() -> std::sync::Arc<dyn hkask_types::InferencePort> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "MCP inference routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            std::sync::Arc::new(client)
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — falling back to MediaRouter (media-only; chat/vision unavailable)"
            );
            std::sync::Arc::new(MediaRouter::new(InferenceConfig::from_env()))
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "HKASK_INFERENCE_SOCKET not set — using MediaRouter (media-only; chat/vision unavailable)"
            );
            std::sync::Arc::new(MediaRouter::new(InferenceConfig::from_env()))
        }
    }
}
