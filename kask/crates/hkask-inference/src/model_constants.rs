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
//! QA generation is intentionally unconfigured by default (2026-09-11).
//! Unlike general inference, its resolver never consults the chat default.
//!
//! - `HKASK_QA_GENERATION_MODEL` — dedicated non-thinking QA generator
//! - `HKASK_CLASSIFIER_MODEL` — primary classifier model
//! - `HKASK_EMBEDDING_MODEL` — default embedding model
//! - `HKASK_OCR_MODEL` — OCR model for scanned PDF fallback
//! - `HKASK_RERANK_MODEL` — rerank model for research deep-search rerank
//! - `HKASK_MODEL_DEFAULT` — fallback when provider-specific not set

/// Environment binding for `kask.models.qa_generation_model`.
pub const QA_GENERATION_MODEL_ENV: &str = "HKASK_QA_GENERATION_MODEL";

/// Resolve an explicit tool model before the dedicated QA setting. There is
/// deliberately no generator ID default, chat fallback, or training-base input.
/// Registry/provider validation remains authoritative for model availability.
pub fn resolve_qa_generation_model(
    requested: Option<&str>,
) -> Result<String, hkask_types::InferenceError> {
    let configured = if requested.is_none() {
        match std::env::var(QA_GENERATION_MODEL_ENV) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => {
                return Err(hkask_types::InferenceError::Model(format!(
                    "invalid {QA_GENERATION_MODEL_ENV}: {error}"
                )));
            }
        }
    } else {
        None
    };
    select_qa_generation_model(requested, configured.as_deref())
}

fn select_qa_generation_model(
    requested: Option<&str>,
    configured: Option<&str>,
) -> Result<String, hkask_types::InferenceError> {
    let model = requested.or(configured).ok_or_else(|| {
        hkask_types::InferenceError::NotConfigured(format!(
            "QA generation requires kask.models.qa_generation_model ({QA_GENERATION_MODEL_ENV}) \
             or an explicit tool model; the chat and training base models are never used"
        ))
    })?;
    let qualified = model.split_once('/').is_some_and(|(provider, local)| {
        !provider.is_empty()
            && provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !local.is_empty()
            && !local.starts_with('/')
    });
    if !qualified || model.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(hkask_types::InferenceError::Model(format!(
            "invalid QA generation model {model:?}: set kask.models.qa_generation_model \
             ({QA_GENERATION_MODEL_ENV}) or tool model to Provider/model-id; no fallback"
        )));
    }
    Ok(model.to_owned())
}

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
/// `None` when unset. The settings chain carries `DEFAULT_RERANK_MODEL`
/// as the code default; a `None` here means the env was not injected
/// (direct CLI callers) — the research server surfaces a typed error
/// naming the setting rather than silently skipping the rerank stage.
pub fn rerank_model() -> Option<String> {
    std::env::var("HKASK_RERANK_MODEL")
        .ok()
        .filter(|m| !m.trim().is_empty())
}

#[cfg(test)]
mod qa_generation_tests {
    use super::*;

    #[test]
    fn explicit_model_wins_without_rewriting_the_operator_alias() {
        let model = "OpenRouter/~openai/gpt-sol-latest";
        assert_eq!(
            select_qa_generation_model(Some(model), Some("invalid")).expect("override"),
            model
        );
        assert_eq!(
            select_qa_generation_model(None, Some(model)).expect("setting"),
            model
        );
        let batch_model = format!("{model}:batch");
        assert_eq!(
            select_qa_generation_model(Some(&batch_model), None).expect("batch override"),
            batch_model
        );
    }

    #[test]
    fn unconfigured_and_invalid_are_errors_not_fallbacks() {
        assert!(matches!(
            select_qa_generation_model(None, None),
            Err(hkask_types::InferenceError::NotConfigured(_))
        ));
        for invalid in [
            "",
            " ",
            "bare-model",
            "/model",
            "OpenRouter/",
            "OpenRouter/ model",
            "~openai/gpt-sol-latest",
        ] {
            assert!(
                matches!(
                    select_qa_generation_model(
                        Some(invalid),
                        Some("OpenRouter/~openai/gpt-sol-latest")
                    ),
                    Err(hkask_types::InferenceError::Model(_))
                ),
                "{invalid:?}"
            );
            assert!(
                matches!(
                    select_qa_generation_model(None, Some(invalid)),
                    Err(hkask_types::InferenceError::Model(_))
                ),
                "{invalid:?}"
            );
        }
    }
}

// ── Media pipeline defaults ───────────────────────────────────────────────
//
// The media server has no in-server model fallback (its accessors return
// `None` when the env var is unset and callers fail visibly). These
// constants are the SETTINGS layer's defaults: `KaskMediaSettings::default()`
// carries them, settings.json overrides them, and `mcp_env()` injects the
// resolved value as `HKASK_MEDIA_*_MODEL` into the media server child
// (operator ruling 2026-09-04 — settings layers carry the code defaults).

/// Default STT model for the transcribe pipeline (env `HKASK_MEDIA_STT_MODEL`
/// via `KaskMediaSettings::stt_model`). OpenRouter's transcriptions endpoint
/// serves this model with word-level timestamps — the educt layer system
/// anchors to word indices, so a segments-only STT response cannot back a
/// reel. Verified 2026-09-04: a 62-minute talk transcribes in a single call
/// (~$0.04) with 8.5K word timings.
pub const DEFAULT_MEDIA_STT_MODEL: &str = "OpenRouter/openai/whisper-large-v3-turbo";

/// Default vision model for gallery analysis and image description (env
/// `HKASK_MEDIA_VISION_MODEL` via `KaskMediaSettings::vision_model`). The
/// tagging pipelines send non-reasoning requests; reasoning-mandatory models
/// reject those ("Reasoning is mandatory for this endpoint and cannot be
/// disabled"). This model is non-reasoning, vision-capable, and cheap — the
/// tagging workload's shape.
pub const DEFAULT_MEDIA_VISION_MODEL: &str = "OpenRouter/openai/gpt-4o-mini";

/// Default rerank model for the research server's deep-search rerank stage
/// (env `HKASK_RERANK_MODEL` via `KaskModelsSettings::rerank_model`). The
/// rerank stage sends non-reasoning relevance-scoring requests. Verified on
/// DeepInfra's catalog 2026-09-11: `Qwen/Qwen3-Reranker-8B`, $0.05/1M tokens,
/// 32K context, instruction-aware (operator ruling 2026-09-11).
pub const DEFAULT_RERANK_MODEL: &str = "deepinfra/Qwen/Qwen3-Reranker-8B";
