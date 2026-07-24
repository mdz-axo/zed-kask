#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Inference — multi-provider inference router
//!
//! Routes LLM requests to 8 providers (DeepInfra, fal.ai, Together AI,
//! OpenRouter, KiloCode, Ollama, Cline, RunPod) based on a 2-letter provider
//! prefix in the model name. Chat dispatch goes through `chat_backend` /
//! `vision_backend` match-fns returning `&dyn ChatBackend` / `&dyn VisionBackend`
//! borrowed from typed `Option<Backend>` fields (single source of truth — no
//! separate map). Fusion orchestration (`fusion_orchestrator`) provides
//! provider-agnostic multi-model deliberation when configured.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]
//!
//! # Architecture
//!
//! ```text
//! InferenceRouter (implements InferencePort)
//!   ├── DeepInfraBackend    — DI/ prefix → api.deepinfra.com      (chat + vision)
//!   ├── FalBackend          — FA/ prefix → api.fal.ai            (chat + vision + media)
//!   ├── TogetherBackend     — TG/ prefix → api.together.xyz     (chat + vision)
//!   ├── OpenRouterBackend   — OR/ prefix → openrouter.ai/api   (chat + vision)
//!   ├── KiloCodeBackend     — KC/ prefix → api.kilo.ai/api/gateway (chat + vision)
//!   ├── OllamaBackend       — OM/ prefix → localhost:11434 (local, no key) (chat + vision)
//!   ├── ClineBackend        — CL/ prefix → api.cline.bot/api (cloud gateway) (chat + vision)
//!   └── RunpodBackend       — RP/ prefix → RunPod serverless (vision/OCR only, NOT chat)
//!
//! Dispatch: chat_backend() / vision_backend() match-fns return &dyn trait objects
//! borrowed from the typed Option<Backend> fields above.
//!
//! EmbeddingRouter
//!   ├── DeepInfraEmbedding — DI/ prefix → /v1/embeddings
//!   └── OpenRouterEmbedding — OR/ prefix → /v1/embeddings
//! ```rust,no_run
//!
//! # Model Naming
//!
//! - `DI/meta-llama/Llama-3.3-70B-Instruct` → DeepInfra
//! - `TG/Qwen/Qwen2.5-7B-Instruct-Turbo` → Together AI
//! - `FA/paddleocr` → fal.ai
//! - `OR/openai/gpt-4o` → OpenRouter
//! - `OM/qwen3:8b` → Ollama (local)
//! - `CL/anthropic/claude-sonnet-4-6` → Cline (cloud gateway)
//! - `RP/kask-ocr` → RunPod (vision/OCR only — not available for chat)
//! - No prefix → default provider (configurable, default: DeepInfra)

pub mod chat_protocol;
pub mod cline_backend;
pub mod config;
pub mod deepinfra_backend;
pub mod embedding_router;
pub mod fal_backend;
pub mod fal_workflow;
pub mod fusion_orchestrator;
pub mod inference_router;
pub mod kilocode_backend;
pub mod model_constants;
pub mod ollama_backend;
pub mod ollama_registry;
pub mod openai_compat;
pub mod openrouter_backend;
pub mod runpod_backend;
pub mod together_backend;

// Re-exports — public API
pub use config::{
    AlgoMethod, FusionConfig, FusionMode, FusionSkill, InferenceConfig, ProviderConfig, ProviderId,
};
pub use embedding_router::EmbeddingRouter;
pub use inference_router::InferenceRouter;
pub use ollama_registry::{
    LocalAdapter, ModelFrom, ModelfileSpec, OllamaRegistry, RegisteredModel, RegistryError,
};

/// Unified model entry from any provider, with provider prefix applied.
#[derive(Debug, Clone)]
pub struct RouterModelEntry {
    /// Full model name with provider prefix (e.g., "OM/qwen3:8b")
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
