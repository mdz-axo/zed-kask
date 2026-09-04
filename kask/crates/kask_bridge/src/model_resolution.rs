//! Generic provider-prefixed model-name resolution from the `LanguageModelRegistry`.
//!
//! Resolves names like `"OpenRouter/openai/gpt-5.2"` to `Arc<dyn LanguageModel>`
//! instances. Used by `LanguageModelInferencePort::generate_with_model` (model
//! overrides from MCP servers) and by the composition root (`kask.models.default_model`).
//! Formerly part of the fusion bridge (`resolve_fusion_models`) — the resolver
//! is generic, so it outlived the fusion system it was introduced for.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::App;
use language_model::{LanguageModel, LanguageModelProviderId, LanguageModelRegistry};

/// Resolve provider-prefixed model names from the `LanguageModelRegistry`.
///
/// Each name in `model_names` is resolved to an `Arc<dyn LanguageModel>`.
/// The key in the returned map is the original name string (so callers can
/// route by name).
///
/// Resolution strategy:
/// 1. If the name contains `/`, split on the first `/` to get
///    `(provider_id, model_id)` and look up the provider (case-insensitive
///    on the provider id — `OpenRouter` matches the registered `openrouter`).
/// 2. Search the provider's models for one whose `id()` matches the model
///    part, whose `name()` (display name — the same key the model list is
///    built from) matches the model part case-insensitively, or whose
///    `telemetry_id()` matches the full prefixed name (case-insensitive —
///    `OpenRouter/minimax/minimax3` matches `openrouter/minimax/minimax3`).
/// 3. If no prefix or no match, search all providers' models by
///    `telemetry_id()` or `name()` (case-insensitive).
///
/// Returns `(resolved, unresolvable)` where `unresolvable` is the set of
/// names that could not be resolved.
#[must_use]
pub fn resolve_model_names(
    registry: &LanguageModelRegistry,
    model_names: &[String],
    cx: &App,
) -> (HashMap<String, Arc<dyn LanguageModel>>, HashSet<String>) {
    let mut resolved: HashMap<String, Arc<dyn LanguageModel>> = HashMap::new();
    let mut unresolvable: HashSet<String> = HashSet::new();

    for name in model_names {
        if let Some(model) = resolve_model(registry, name, cx) {
            resolved.insert(name.clone(), model);
        } else {
            unresolvable.insert(name.clone());
        }
    }

    (resolved, unresolvable)
}

/// The per-model match predicate for a provider-prefixed override — a pure
/// function because the full resolver needs a gpui `App` (registry +
/// provider construction) the unit harness doesn't have. An override matches
/// on three keys: the model id (exact — the slug form, e.g. `z-ai/glm-5.2`),
/// the display name (case-insensitive — the key the model list is built
/// from; ollama's id is `glm-ocr:latest` but its name is `glm-ocr`), and
/// the full telemetry id (case-insensitive).
fn override_matches(
    model_id: &str,
    model_name: &str,
    telemetry_id: &str,
    override_model_part: &str,
    override_full: &str,
) -> bool {
    model_id == override_model_part
        || model_name.eq_ignore_ascii_case(override_model_part)
        || telemetry_id.eq_ignore_ascii_case(override_full)
}

/// Resolve a single provider-prefixed model name.
///
/// Provider-ID lookup is case-insensitive: config uses `"OpenRouter/..."`
/// (capitalized) while the `LanguageModelRegistry` registers OpenRouter under
/// `"openrouter"` (lowercase). Rather than normalizing one side, we normalize
/// at the lookup boundary — exact-case first, then a case-insensitive fallback
/// across all registered providers.
fn resolve_model(
    registry: &LanguageModelRegistry,
    prefixed_name: &str,
    cx: &App,
) -> Option<Arc<dyn LanguageModel>> {
    // Try to split on the first `/` to get provider/model.
    if let Some((provider_id_str, model_id)) = prefixed_name.split_once('/') {
        let provider_id = LanguageModelProviderId(provider_id_str.to_string().into());

        // Exact-case lookup first (fast path — the common case when the user
        // types the provider id exactly as registered).
        let provider = registry.provider(&provider_id).or_else(|| {
            // Case-insensitive fallback. `LanguageModelProviderId` derives
            // `Eq`/`Hash` with case-sensitive `SharedString`, so a capitalized
            // prefix like "OpenRouter" won't match the registered "openrouter".
            // Iterate all providers and compare case-insensitively.
            registry
                .providers()
                .into_iter()
                .find(|p| p.id().0.as_ref().eq_ignore_ascii_case(provider_id_str))
        });

        if let Some(provider) = provider {
            // The model ID after the prefix may itself contain a `/` (e.g.
            // "anthropic/claude-sonnet-4.5" under provider "OR"). Search the
            // provider's models for a match on id or telemetry_id.
            //
            // The telemetry_id comparison is case-insensitive on the full
            // prefixed name: config uses `OpenRouter/...` (capitalized) while
            // the OpenRouter provider's telemetry_id returns `openrouter/...`
            // (lowercase). Without case-insensitive comparison,
            // `OpenRouter/minimax/minimax3` never matches
            // `openrouter/minimax/minimax3` even when the model exists.
            //
            // The name() comparison is the display-name key: the model list
            // (the list_models IPC surface the corpus tools check against) is
            // built as `{provider_id}/{model.name()}`, and providers whose
            // id() differs from name() (ollama: id `glm-ocr:latest`, name
            // `glm-ocr`; OpenRouter: id is the slug, name is the display name)
            // could never be resolved by their own advertised names without
            // it — every such vision override silently fell back to the
            // default (text) model, which drops images.
            for model in provider.provided_models(cx) {
                if override_matches(
                    model.id().0.as_ref(),
                    model.name().0.as_ref(),
                    model.telemetry_id().as_str(),
                    model_id,
                    prefixed_name,
                ) {
                    return Some(model);
                }
            }
        }
    }

    // No prefix or prefix match failed — search all providers, matching the
    // full string against every key (an unprefixed override can be a slug
    // id, a display name, or a telemetry id).
    registry.available_models(cx).find(|m| {
        override_matches(
            m.id().0.as_ref(),
            m.name().0.as_ref(),
            m.telemetry_id().as_str(),
            prefixed_name,
            prefixed_name,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::override_matches;

    /// The 2026-09-04 incident: ollama models have id `glm-ocr:latest` but
    /// display name `glm-ocr` (the key the model list is built from) — the
    /// resolver matched only id/telemetry, so the registry's own advertised
    /// name never resolved and vision overrides fell back to a text model,
    /// which drops images.
    #[test]
    fn display_name_matches_when_id_differs() {
        assert!(override_matches(
            "glm-ocr:latest",
            "glm-ocr",
            "ollama/glm-ocr:latest",
            "glm-ocr",
            "ollama/glm-ocr",
        ));
    }

    /// Slug-form overrides (the model constants) match by exact id.
    #[test]
    fn slug_id_matches_exact() {
        assert!(override_matches(
            "z-ai/glm-5.2",
            "GLM 5.2",
            "openrouter/z-ai/glm-5.2",
            "z-ai/glm-5.2",
            "OpenRouter/z-ai/glm-5.2",
        ));
    }

    /// Case-insensitive telemetry match — capitalized config prefixes
    /// resolve against lowercase provider telemetry ids.
    #[test]
    fn telemetry_id_matches_case_insensitive() {
        assert!(override_matches(
            "minimax/minimax3",
            "MiniMax M3",
            "openrouter/minimax/minimax3",
            "no-such-model",
            "OpenRouter/minimax/minimax3",
        ));
    }

    /// A different model's keys must not match.
    #[test]
    fn no_match_when_no_key_matches() {
        assert!(!override_matches(
            "glm-ocr:latest",
            "glm-ocr",
            "ollama/glm-ocr:latest",
            "lightonocr2",
            "ollama/lightonocr2",
        ));
    }

    /// Unprefixed fallback: the full string can match the display name.
    #[test]
    fn unprefixed_full_string_matches_display_name() {
        assert!(override_matches(
            "qwen/qwen3-vl-32b-instruct",
            "Qwen3 VL 32B Instruct",
            "openrouter/qwen/qwen3-vl-32b-instruct",
            "Qwen3 VL 32B Instruct",
            "Qwen3 VL 32B Instruct",
        ));
    }
}
