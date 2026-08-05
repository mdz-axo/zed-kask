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
/// DeepSeek V4 Flash via OpenRouter: fast, cheap, strong classification quality.
///
/// This is the single source of truth for the classifier model id. Every
/// call site resolves it via [`classifier_model`] (env `HKASK_CLASSIFIER_MODEL`
/// → this constant). Registry YAMLs in `registry/classify/` leave their
/// `model:` field empty to defer to this path; `ClassifierConfig::from_def`
/// passes the full provider-prefixed string to `InferencePort::generate_with_model`,
/// and the `LanguageModelRegistry` resolves the `OpenRouter/` prefix to the provider.
pub const DEFAULT_CLASSIFIER_MODEL: &str = "OpenRouter/deepseek/deepseek-v4-flash";

/// Default embedding model.
pub const DEFAULT_EMBEDDING_MODEL: &str = "DeepInfra/Qwen/Qwen3-Embedding-0.6B";

/// Default OCR model for scanned PDF fallback.
/// Uses kask-ocr on RunPod (OLMOCR-2).
pub const DEFAULT_OCR_MODEL: &str = "RunPod/kask-ocr";

/// Fallback model when no other model is configured.
/// Prefixed with `OpenRouter/` so it routes to OpenRouter (which hosts this exact id).
/// Matches `InferenceConfig::from_env()` default.
pub const DEFAULT_FALLBACK_MODEL: &str = "OpenRouter/z-ai/glm-5.2";

/// Default TTS (text-to-speech) model.
pub const DEFAULT_TTS_MODEL: &str = "fal.ai/Qwen3-TTS";

/// Default STT (speech-to-text) model.
pub const DEFAULT_STT_MODEL: &str = "fal.ai/wizper";

/// Default vision model for image analysis.
pub const DEFAULT_VISION_MODEL: &str = "KiloCode/Qwen/Qwen3-VL-235B-A22B-Instruct";

/// Default image generation model.
pub const DEFAULT_IMAGE_GEN_MODEL: &str = "fal.ai/flux-2";

// ── Test fixtures (arbitrary identifiers, no network calls) ──────────────

pub const TEST_MODEL_SMALL: &str = "DeepInfra/google/gemma-4-9b-it";
pub const TEST_MODEL_MEDIUM: &str = "DeepInfra/meta-llama/Llama-4-Scout-17B-16E-Instruct";

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
