//! Model name resolution — env-configurable with compile-time defaults.
//!
//! Every model used in the system has a corresponding env var for override.
//! The constants here are DEFAULT values; env vars take precedence.
//! This eliminates the need to recompile when models are superseded.
//!
//! Naming convention:
//! - `HKASK_CLASSIFIER_MODEL` — primary classifier model
//! - `HKASK_EMBEDDING_MODEL` — default embedding model
//! - `HKASK_OCR_MODEL` — OCR model for scanned PDF fallback
//! - `HKASK_MODEL_DEFAULT` — fallback when provider-specific not set

/// Canonical classifier model for all classification surfaces (corpus
/// pipeline, QA triage, convergence evaluation, h_mem extraction).
/// GLM-5.2 via OpenRouter: strongest classification accuracy on the real
/// hKask label-space eval (39/47; see kask/docs/review/classifier-model-review.md).
///
/// This is the single source of truth for the classifier model id. Every
/// call site resolves it via [`classifier_model`] (env `HKASK_CLASSIFIER_MODEL`
/// → this constant). Registry YAMLs in `registry/classify/` leave their
/// `model:` field empty to defer to this path; `ClassifierConfig::from_def`
/// passes the full provider-prefixed string to `InferencePort::generate_with_model`,
/// and the `LanguageModelRegistry` resolves the `OpenRouter/` prefix to the provider.
pub const DEFAULT_CLASSIFIER_MODEL: &str = "OpenRouter/z-ai/glm-5.2";

/// Default embedding model. No bundled cloud provider serves embeddings
/// (OpenRouter doesn't; RunPod is vision/OCR-only), so this defaults to a
/// local Ollama embedding model. Operators without a local Ollama must set
/// `HKASK_EMBEDDING_MODEL` (or the corpus embedding settings) to a provider
/// they have credentials for — embedding calls fail with a clear error
/// otherwise.
pub const DEFAULT_EMBEDDING_MODEL: &str = "ollama/qwen3-embedding:0.6b";

/// Default OCR model for scanned PDF fallback.
/// Uses OLMOCR-2 on RunPod serverless (endpoint `hsldzov6932wf5`, named `kask-ocr`
/// in the RunPod console). The vLLM worker serves the model under its HuggingFace
/// id `allenai/olmOCR-2-7B-1025`; the provider-prefixed name `RunPod/kask-ocr`
/// resolves through Zed's `LanguageModelRegistry` via the dedicated `runpod`
/// provider (D29), which carries each endpoint's per-model API URL and discovers
/// endpoints via the RunPod GraphQL API.
pub const DEFAULT_OCR_MODEL: &str = "RunPod/kask-ocr";

/// Fallback model when no other model is configured.
/// Prefixed with `OpenRouter/` so it routes to OpenRouter (which hosts this exact id).
/// Matches `InferenceConfig::from_env()` default.
pub const DEFAULT_FALLBACK_MODEL: &str = "OpenRouter/z-ai/glm-5.2";

/// Default agent model for local swarm agents (the model the agent runs on).
/// Used by `SwarmConfig::default()` and `KaskSwarmSettings::default()`.
pub const DEFAULT_AGENT_MODEL: &str = "claude-haiku-4-5-20251001";

// ── Resolved model accessors (env var → default) ──────────────────────────

/// Resolve the primary classifier: `HKASK_CLASSIFIER_MODEL` → default.
pub fn classifier_model() -> String {
    std::env::var("HKASK_CLASSIFIER_MODEL").unwrap_or_else(|_| DEFAULT_CLASSIFIER_MODEL.to_string())
}

/// Resolve the embedding model: `HKASK_EMBEDDING_MODEL` → default.
pub fn embedding_model() -> String {
    std::env::var("HKASK_EMBEDDING_MODEL").unwrap_or_else(|_| DEFAULT_EMBEDDING_MODEL.to_string())
}

/// Resolve the OCR model: `HKASK_OCR_MODEL` → default.
pub fn ocr_model() -> String {
    std::env::var("HKASK_OCR_MODEL").unwrap_or_else(|_| DEFAULT_OCR_MODEL.to_string())
}
