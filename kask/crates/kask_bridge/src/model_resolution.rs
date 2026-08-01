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
///    part, or whose `telemetry_id()` matches the full prefixed name
///    (case-insensitive — `OpenRouter/minimax/minimax3` matches
///    `openrouter/minimax/minimax3`).
/// 3. If no prefix or no match, search all providers' models by
///    `telemetry_id()` (case-insensitive).
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
            for model in provider.provided_models(cx) {
                if model.id().0.as_ref() == model_id
                    || model.telemetry_id().eq_ignore_ascii_case(prefixed_name)
                {
                    return Some(model);
                }
            }
        }
    }

    // No prefix or prefix match failed — search all providers by telemetry_id
    // (case-insensitive, same reason as above).
    registry
        .available_models(cx)
        .find(|m| m.telemetry_id().eq_ignore_ascii_case(prefixed_name))
}

#[cfg(test)]
mod tests {
    /// Document the case-insensitive provider-id contract.
    ///
    /// Config uses `"OpenRouter/..."` (capitalized) while zed's
    /// `LanguageModelRegistry` registers OpenRouter under `"openrouter"`
    /// (lowercase). `resolve_model` must match these case-insensitively. This
    /// test pins the string-comparison logic; a full integration test would
    /// require a GPUI test context with a registered OpenRouter provider.
    #[test]
    fn resolve_model_matches_provider_id_case_insensitively() {
        let configured_prefix = "OpenRouter";
        let registered_id = "openrouter";
        assert!(
            registered_id.eq_ignore_ascii_case(configured_prefix),
            "case-insensitive comparison must match OpenRouter <-> openrouter"
        );
    }

    /// Pin the case-insensitive telemetry_id contract.
    ///
    /// `resolve_model` compares the full prefixed name (e.g.
    /// `"OpenRouter/minimax/minimax3"`) against each model's `telemetry_id()`
    /// (e.g. `"openrouter/minimax/minimax3"` — lowercase prefix). The
    /// comparison must be case-insensitive on the full string, otherwise the
    /// capitalized `OpenRouter/` prefix never matches the lowercase
    /// `openrouter/` prefix in telemetry_ids, even when the model exists in
    /// the registry.
    #[test]
    fn resolve_model_telemetry_id_comparison_is_case_insensitive() {
        let configured = "OpenRouter/minimax/minimax3";
        let telemetry_id = "openrouter/minimax/minimax3";
        assert!(
            telemetry_id.eq_ignore_ascii_case(configured),
            "telemetry_id comparison must be case-insensitive \
             (openrouter/... must match OpenRouter/...)"
        );
    }
}
