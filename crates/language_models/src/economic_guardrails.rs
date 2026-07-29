//! Cross-provider economic guardrails.
//!
//! The OpenRouter provider fetches per-model pricing from OpenRouter's `/models`
//! endpoint. We use that data to build a deny-list of model names whose output
//! price exceeds the configured threshold, then apply that deny-list across
//! **all** providers (Zed cloud, Anthropic, OpenAI, DeepInfra, Together, etc.)
//! via `LanguageModelRegistry::set_model_filter_fn`.
//!
//! This generalizes the OpenRouter-specific filter (`passes_output_price_filter`
//! in `provider/open_router.rs`) to every provider. The OpenRouter provider
//! still applies its own filter first (so expensive models never appear in its
//! `provided_models` list); this module catches the same models when they
//! surface through other providers that don't publish pricing.
//!
//! ## Matching strategy
//!
//! OpenRouter model IDs are `provider/model` (e.g. `openai/o1-pro`,
//! `anthropic/claude-opus-4.1`). Other providers use bare model IDs
//! (e.g. `o1-pro`, `claude-opus-4-1`). We normalize by taking the last path
//! segment of the OpenRouter ID and comparing case-insensitively against the
//! candidate model's `id()` and `name()`. This is intentionally fuzzy — it
//! errs on the side of de-listing a cheap model that happens to share a name
//! with an expensive one, rather than letting an expensive model through.
//!
//! ## Deny-list lifecycle
//!
//! The deny-list is rebuilt whenever the OpenRouter provider fetches models
//! (at startup and on settings/api-key change). It's stored in a
//! process-global `Mutex<Arc<HashSet>>` so the filter closure (which cannot
//! capture mutable state) reads the latest list via an `Arc` clone. The
//! threshold is read live from settings on every filter call, so changing the
//! threshold in the UI takes effect immediately without rebuilding the list.

use std::sync::{Arc, Mutex};

use collections::HashSet;
use gpui::App;
use language_model::{LanguageModel, LanguageModelRegistry};
use settings::Settings;

use crate::settings::AllLanguageModelSettings;

/// Process-global deny-list of normalized model names that exceed the price
/// threshold. Stored as `Arc<HashSet>` inside a `Mutex` so the filter closure
/// can clone the `Arc` for lock-free reads, while updates swap the `Arc`
/// atomically.
static EXPENSIVE_MODEL_DENYLIST: Mutex<Option<Arc<HashSet<String>>>> = Mutex::new(None);

/// Normalize a model identifier for cross-provider matching.
///
/// Strips any `provider/` prefix (OpenRouter uses `openai/o1-pro`), lowercases,
/// and trims. Returns the bare model name (e.g. `o1-pro`, `claude-opus-4.1`).
fn normalize_model_name(id_or_name: &str) -> String {
    id_or_name
        .rsplit('/')
        .next()
        .unwrap_or(id_or_name)
        .trim()
        .to_lowercase()
}

/// Update the process-global deny-list of expensive model names.
///
/// Called by the OpenRouter provider after it fetches models. `models` is the
/// full list of OpenRouter models with their `output_price_per_token`; this
/// function extracts the names of models whose price exceeds the threshold
/// configured in `language_models.open_router.max_output_price_per_million_tokens`.
///
/// Models with no price (`None`) or sentinel prices (`< 0`, non-finite) are
/// never added to the deny-list. When the threshold is `None` (filter
/// disabled), the deny-list is cleared.
pub fn update_expensive_model_denylist(models: &[open_router::Model], cx: &App) {
    let settings = &AllLanguageModelSettings::get_global(cx).open_router;
    let new_denylist = match settings.max_output_price_per_million_tokens {
        None => Arc::new(HashSet::default()),
        Some(max_per_million) => {
            let max_per_token = max_per_million / 1_000_000.0;
            let mut denylist = HashSet::default();
            for model in models {
                let Some(price_per_token) = model.output_price_per_token else {
                    continue;
                };
                if !price_per_token.is_finite() || price_per_token < 0.0 {
                    continue;
                }
                if price_per_token > max_per_token {
                    denylist.insert(normalize_model_name(model.id()));
                }
            }
            if !denylist.is_empty() {
                log::info!(
                    "economic-guardrails: de-listing {} expensive model(s) across all providers",
                    denylist.len()
                );
            }
            Arc::new(denylist)
        }
    };

    if let Ok(mut slot) = EXPENSIVE_MODEL_DENYLIST.lock() {
        *slot = Some(new_denylist);
    }
}

/// Install the cross-provider model filter on the registry.
///
/// The filter closure reads the deny-list from the process-global
/// `EXPENSIVE_MODEL_DENYLIST` on every call. The deny-list is populated by
/// `update_expensive_model_denylist` when the OpenRouter provider fetches
/// models. Until the first fetch completes, the deny-list is `None` and all
/// models pass.
pub fn install_model_filter(registry: &mut LanguageModelRegistry) {
    registry.set_model_filter_fn(Box::new(|model: &dyn LanguageModel| {
        let denylist_arc = EXPENSIVE_MODEL_DENYLIST
            .lock()
            .ok()
            .and_then(|slot| slot.clone());
        let Some(denylist) = denylist_arc else {
            return true; // no deny-list populated yet
        };
        if denylist.is_empty() {
            return true; // filter disabled or no expensive models found
        }
        let id = normalize_model_name(&model.id().0);
        let name = normalize_model_name(&model.name().0);
        !denylist.contains(&id) && !denylist.contains(&name)
    }));
}

/// Returns `true` if `model` would pass the cross-provider deny-list filter.
/// Exposed for testing: lets a test verify matching logic without spinning up
/// a full `LanguageModelRegistry` + settings store.
#[cfg(test)]
fn passes_cross_provider_filter(model: &dyn LanguageModel, denylist: &HashSet<String>) -> bool {
    if denylist.is_empty() {
        return true;
    }
    let id = normalize_model_name(&model.id().0);
    let name = normalize_model_name(&model.name().0);
    !denylist.contains(&id) && !denylist.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use language_model::fake_provider::FakeLanguageModel;

    #[test]
    fn normalize_strips_provider_prefix() {
        assert_eq!(normalize_model_name("openai/o1-pro"), "o1-pro");
        assert_eq!(
            normalize_model_name("anthropic/claude-opus-4.1"),
            "claude-opus-4.1"
        );
        assert_eq!(normalize_model_name("o1-pro"), "o1-pro");
        assert_eq!(normalize_model_name("O1-PRO"), "o1-pro");
    }

    #[test]
    fn cross_provider_filter_drops_expensive_models_by_id_or_name() {
        // Deny-list built from OpenRouter IDs (provider-prefixed).
        let mut denylist = HashSet::default();
        denylist.insert(normalize_model_name("openai/o1-pro"));
        denylist.insert(normalize_model_name("anthropic/claude-opus-4.1"));

        // Same model surfaced through a different provider with a bare ID.
        let openai_o1_pro =
            FakeLanguageModel::with_id_and_thinking("openai", "o1-pro", "o1 Pro", false);
        assert!(!passes_cross_provider_filter(&openai_o1_pro, &denylist));

        // Match by name (display name) when the ID differs.
        let claude = FakeLanguageModel::with_id_and_thinking(
            "anthropic",
            "claude-opus-4-1",
            "claude-opus-4.1",
            false,
        );
        assert!(!passes_cross_provider_filter(&claude, &denylist));

        // Cheap model passes.
        let cheap =
            FakeLanguageModel::with_id_and_thinking("openai", "gpt-4o-mini", "GPT-4o mini", false);
        assert!(passes_cross_provider_filter(&cheap, &denylist));

        // Empty deny-list passes everything.
        let empty: HashSet<String> = HashSet::default();
        assert!(passes_cross_provider_filter(&openai_o1_pro, &empty));
    }
}
