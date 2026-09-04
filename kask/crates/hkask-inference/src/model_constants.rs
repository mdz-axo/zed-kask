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
//! - `HKASK_RERANK_MODEL` — rerank model for research deep-search rerank
//! - `HKASK_MODEL_DEFAULT` — fallback when provider-specific not set

/// Default embedding model. Served by DeepInfra (OpenAI-compatible
/// `/v1/embeddings` endpoint). The `DeepInfra/` prefix routes through
/// `resolve_embedding_credentials` to `https://api.deepinfra.com/v1/openai`
/// with the `DEEPINFRA_API_KEY` env var. Operators must set this key via
/// Settings → AI → LLM Providers (it lives at the provider's `api_url`
/// keychain slot — the ONE location) for embedding-based recall to work.
///
/// Previously defaulted to `ollama/qwen3-embedding:0.6b` (local Ollama),
/// which works but is impractically slow on CPU for large corpora (33K+
/// chunks). The cloud endpoint serves the same Qwen model at scale.
pub const DEFAULT_EMBEDDING_MODEL: &str = "DeepInfra/Qwen/Qwen3-Embedding-0.6B";
// NOTE: the LAST remaining constant default — its single functional
// consumer (kask_bridge/src/settings.rs `effective_embedding_model`) is in
// the parallel session's in-flight file; delete both together when that
// pass lands (the operator's no-hidden-models spec).

// ── Resolved model accessors (env var → Option; None = not configured) ────
//
// The operator's no-hidden-models spec: these accessors have NO constant
// fallback. `None` means "not configured" — callers fail visibly (a typed
// error naming the setting to set), never a silent hidden default. The env
// vars are injected from the visible kask settings
// (`kask.models.classifier_model`, etc.).

/// Resolve the classifier model: `HKASK_CLASSIFIER_MODEL` → `None` when
/// unset (callers fail visibly).
pub fn classifier_model() -> Option<String> {
    std::env::var("HKASK_CLASSIFIER_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Resolve the embedding model: `HKASK_EMBEDDING_MODEL` → `None` when
/// unset (callers fail visibly).
pub fn embedding_model() -> Option<String> {
    std::env::var("HKASK_EMBEDDING_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Resolve the OCR model: `HKASK_OCR_MODEL` → `None` when unset (callers
/// fail visibly).
pub fn ocr_model() -> Option<String> {
    std::env::var("HKASK_OCR_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Resolve the rerank model: `HKASK_RERANK_MODEL` → `None` when unset
/// (callers fail visibly).
pub fn rerank_model() -> Option<String> {
    std::env::var("HKASK_RERANK_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}
