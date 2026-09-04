//! Model name resolution — env-configurable with code defaults.
//!
//! Every model used in the system has a corresponding env var for override.
//! The accessors here read the ENV LAYER only: `HKASK_*` → `None` when unset.
//! They are not the whole chain — the settings layers carry the code defaults
//! (operator ruling 2026-09-04, superseding the former no-hidden-models
//! spec): `KaskModelsSettings::default()` / `HkaskSettings::default()` hold
//! the default model names, settings.json / the settings UI override them,
//! and `mcp_env()` injects the resolved values into MCP server children as
//! these env vars. A `None` from these accessors therefore means "env not
//! injected" — in practice only reachable for direct CLI callers that
//! bypass the settings chain.
//!
//! Naming convention:
//! - `HKASK_CLASSIFIER_MODEL` — primary classifier model
//! - `HKASK_EMBEDDING_MODEL` — default embedding model
//! - `HKASK_OCR_MODEL` — OCR model for scanned PDF fallback
//! - `HKASK_RERANK_MODEL` — rerank model for research deep-search rerank
//! - `HKASK_MODEL_DEFAULT` — fallback when provider-specific not set

/// Read the classifier model from the env layer: `HKASK_CLASSIFIER_MODEL`
/// → `None` when unset. The settings chain (which carries the code
/// default) injects this env var for MCP server children.
pub fn classifier_model() -> Option<String> {
    std::env::var("HKASK_CLASSIFIER_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Read the embedding model from the env layer: `HKASK_EMBEDDING_MODEL`
/// → `None` when unset. The settings chain (which carries the code
/// default) injects this env var for MCP server children.
pub fn embedding_model() -> Option<String> {
    std::env::var("HKASK_EMBEDDING_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Read the OCR model from the env layer: `HKASK_OCR_MODEL` → `None`
/// when unset. The settings chain (which carries the code default)
/// injects this env var for MCP server children.
pub fn ocr_model() -> Option<String> {
    std::env::var("HKASK_OCR_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

/// Read the rerank model from the env layer: `HKASK_RERANK_MODEL` →
/// `None` when unset. No code default exists for rerank (no configured
/// model to draw from) — callers surface a typed error naming the
/// setting.
pub fn rerank_model() -> Option<String> {
    std::env::var("HKASK_RERANK_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}
