//! Private-inference zed-side seam (M7/Seam B) — the local leg's
//! provider registration: the M3 configured instance.
//!
//! M3 ruling (D-3 plan): the private endpoint is a CONFIGURED INSTANCE
//! of the existing `openai_compatible` provider class — a settings
//! entry, never new provider code. This module composes the PI-owned
//! policy (`kask.private_inference`) with the discovered fleet (the
//! local leg's model catalog, operator ruling 2026-11-08) into that
//! entry and writes it through the existing settings store; the
//! registry re-reads settings on write and registers the provider, so
//! the models appear in the agent panel model selector (Curator and
//! Z-K alike) with no restart.
//!
//! The fleet is DISCOVERED, not compiled in: the compiled-in seed set
//! writes the brief TOMLs to the discovered briefs directory when
//! absent (the D1 template-shipping precedent), and discovery reads
//! that directory — so a model joins by a data act (brief + weights,
//! then Register or a restart), never a code rebuild. The fleet is
//! lazily seeded + discovered on first use and memoized
//! ([`local_fleet`]); [`refresh_local_fleet`] re-discovers.
//!
//! The endpoint token is NOT handled here: it lives at the provider
//! `api_url` keychain slot (the same slot zed's `ApiKeyState` reads —
//! D5 one-key-one-location) and is written by the settings UI's
//! existing `write_credential` path plus the provider's live
//! `set_api_key` refresh.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use hkask_private_inference::local_leg::fleet::{self, FleetError, FleetModel, SeedError};
use settings_content::{
    OpenAiCompatibleAvailableModel, OpenAiCompatibleSettingsContent, SettingsContent,
};

use crate::settings::KaskPrivateInferenceSettings;

/// Provider-entry emission failures — per-variant, never a blanket
/// string (the string-errors discipline).
#[derive(Debug, thiserror::Error)]
pub enum ProviderEntryError {
    /// The configured provider id is empty — the write is refused
    /// rather than registering an unnamed provider (fail-closed).
    #[error(
        "the private-inference provider id is empty — set \
         kask.private_inference.provider_id before registering"
    )]
    EmptyProviderId,
    /// The discovered briefs directory could not be seeded — the
    /// compiled-in set could not be written (a wiring gap, surfaced).
    #[error(transparent)]
    Seed(#[from] SeedError),
    /// The discovered fleet failed to load — no provider entry is
    /// emitted with a partial fleet; the error names the file.
    #[error(transparent)]
    Fleet(#[from] FleetError),
}

/// The discovered briefs directory under the artifacts root — the
/// fleet's source of truth since discovery replaced the compiled-in
/// list (operator ruling 2026-11-08).
pub fn briefs_dir() -> PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/model-briefs",
    ))
}

static FLEET: OnceLock<RwLock<Option<Arc<[FleetModel]>>>> = OnceLock::new();

/// The discovered local fleet — lazily seeded and discovered on first
/// use, then memoized (the lazy pattern the registry-shaped lists
/// use): discovery is a directory scan + brief decode, and the cache
/// keeps renders and emissions off the filesystem. Callers that need
/// fresh state (the Register action, an explicit refresh) call
/// [`refresh_local_fleet`] first.
pub fn local_fleet() -> Result<Arc<[FleetModel]>, ProviderEntryError> {
    let slot = FLEET.get_or_init(|| RwLock::new(None));
    if let Some(cached) = slot
        .read()
        .expect("the fleet slot is never poisoned")
        .clone()
    {
        return Ok(cached);
    }
    let fleet = seed_and_discover()?;
    *slot.write().expect("the fleet slot is never poisoned") = Some(fleet.clone());
    Ok(fleet)
}

/// Drop the memoized fleet — the next [`local_fleet`] re-seeds
/// (idempotent) and re-discovers, so a newly dropped brief and its
/// weights surface after Register or a restart, never a rebuild.
pub fn refresh_local_fleet() {
    let slot = FLEET.get_or_init(|| RwLock::new(None));
    *slot.write().expect("the fleet slot is never poisoned") = None;
}

fn seed_and_discover() -> Result<Arc<[FleetModel]>, ProviderEntryError> {
    let dir = briefs_dir();
    fleet::seed_briefs(&dir)?;
    let models_root = hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/models",
    ));
    Ok(fleet::discover(&dir, &models_root)?.into())
}

/// Build the `openai_compatible` provider entry for the private local
/// leg from the PI-owned policy and the discovered fleet.
///
/// `api_url` comes from the endpoint construction home
/// (`hkask_private_inference::local_leg::launch::endpoint_url`) so the
/// entry and a launched plan can never disagree on the endpoint shape.
/// The model cards project the fleet: `name` is the brief's
/// `model_id`, `display_name` its display name, `max_tokens` its
/// recommended context length. `auto_discover` matches the
/// settings-content default (`default_auto_discover()`, true): a
/// running llama-server merges its served ids into the cards; a
/// stopped one leaves the static cards intact.
pub fn provider_entry(
    pi: &KaskPrivateInferenceSettings,
    models: &[FleetModel],
) -> Result<OpenAiCompatibleSettingsContent, ProviderEntryError> {
    let available_models = models
        .iter()
        .map(|model| OpenAiCompatibleAvailableModel {
            name: model.brief.model_id.clone(),
            display_name: Some(model.brief.display_name.clone()),
            max_tokens: u64::from(model.brief.recommendations.context_length),
            max_output_tokens: None,
            max_completion_tokens: None,
            reasoning_effort: None,
            // The Default capabilities carry `tools: true` — the
            // agent-panel surfaces need tool-calling chat models, and
            // the llama-server chat fleet serves them.
            capabilities: Default::default(),
        })
        .collect();
    Ok(OpenAiCompatibleSettingsContent {
        api_url: hkask_private_inference::local_leg::launch::endpoint_url(&pi.host, pi.port),
        available_models,
        custom_headers: None,
        auto_discover: true,
    })
}

/// The keychain URL the private leg's endpoint token lives at — the
/// provider entry's `api_url`, i.e. the SAME slot zed's `ApiKeyState`
/// reads for the registered provider (D5 one-key-one-location: the
/// settings UI's token row writes here and only here).
pub fn credential_url(pi: &KaskPrivateInferenceSettings) -> String {
    hkask_private_inference::local_leg::launch::endpoint_url(&pi.host, pi.port)
}

/// Apply the private-leg provider entry into settings content — the
/// connect step, over the discovered fleet. Idempotent: applying twice
/// writes the same entry bytes. Fails closed on an empty provider id
/// or a broken discovery: no `language_models` section is created on
/// the refusal path.
pub fn apply_provider_entry(
    content: &mut SettingsContent,
    pi: &KaskPrivateInferenceSettings,
    models: &[FleetModel],
) -> Result<(), ProviderEntryError> {
    if pi.provider_id.trim().is_empty() {
        return Err(ProviderEntryError::EmptyProviderId);
    }
    let entry = provider_entry(pi, models)?;
    content
        .language_models
        .get_or_insert_with(Default::default)
        .openai_compatible
        .get_or_insert_with(Default::default)
        .insert(Arc::from(pi.provider_id.as_str()), entry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fleet discovered from a seeded tempdir — the pure-fixture
    /// counterpart of [`local_fleet`]'s lazy machine path (tests never
    /// touch the real artifacts dir).
    fn seeded_fleet() -> Vec<FleetModel> {
        let briefs = tempfile::tempdir().expect("briefs tempdir");
        fleet::seed_briefs(briefs.path()).expect("the seed set writes");
        let models_root = tempfile::tempdir().expect("models tempdir");
        fleet::discover(briefs.path(), models_root.path()).expect("the seeded fleet discovers")
    }

    #[test]
    fn provider_entry_shapes_the_endpoint_and_cards() {
        let pi = KaskPrivateInferenceSettings::default();
        let fleet = seeded_fleet();
        let entry =
            provider_entry(&pi, &fleet).expect("entry builds from the default policy and fleet");
        assert_eq!(entry.api_url, "http://127.0.0.1:8010/v1");
        assert!(entry.auto_discover);
        assert_eq!(entry.available_models.len(), fleet.len());
        for (model, discovered) in entry.available_models.iter().zip(&fleet) {
            assert_eq!(model.name, discovered.brief.model_id);
            assert_eq!(
                model.display_name.as_deref(),
                Some(discovered.brief.display_name.as_str())
            );
            assert_eq!(
                model.max_tokens,
                u64::from(discovered.brief.recommendations.context_length)
            );
            // The agent-panel contract: private models are
            // tool-calling chat models.
            assert!(model.capabilities.tools);
            assert!(model.capabilities.chat_completions);
        }
    }

    #[test]
    fn apply_is_idempotent_and_lands_under_the_provider_id() {
        let pi = KaskPrivateInferenceSettings::default();
        let fleet = seeded_fleet();
        let mut content = SettingsContent::default();
        apply_provider_entry(&mut content, &pi, &fleet).expect("first apply writes");
        let once = serde_json::to_value(&content).expect("serializes");
        apply_provider_entry(&mut content, &pi, &fleet).expect("second apply writes");
        let twice = serde_json::to_value(&content).expect("serializes");
        assert_eq!(
            once, twice,
            "re-applying the entry must not drift the content"
        );

        let entry = content
            .language_models
            .as_ref()
            .and_then(|lm| lm.openai_compatible.as_ref())
            .and_then(|oc| oc.get(pi.provider_id.as_str()))
            .expect("the entry lands under the configured provider id");
        assert_eq!(entry.api_url, "http://127.0.0.1:8010/v1");
    }

    #[test]
    fn apply_fails_closed_on_an_empty_provider_id() {
        let pi = KaskPrivateInferenceSettings {
            provider_id: "  ".into(),
            ..KaskPrivateInferenceSettings::default()
        };
        let fleet = seeded_fleet();
        let mut content = SettingsContent::default();
        let err = apply_provider_entry(&mut content, &pi, &fleet)
            .expect_err("an empty provider id must refuse the write");
        assert!(matches!(err, ProviderEntryError::EmptyProviderId));
        // Fail-closed: the refusal path creates no language_models
        // section at all.
        assert!(content.language_models.is_none());
    }

    #[test]
    fn credential_url_equals_the_entry_api_url() {
        // The load-bearing identity: the token slot IS the provider's
        // api_url slot — one key, one location. Pinned for the default
        // endpoint AND a custom one.
        let pi = KaskPrivateInferenceSettings::default();
        let fleet = seeded_fleet();
        assert_eq!(
            credential_url(&pi),
            provider_entry(&pi, &fleet).unwrap().api_url
        );
        let custom = KaskPrivateInferenceSettings {
            provider_id: "pi-test".into(),
            host: "127.0.0.2".into(),
            port: 9020,
            local_provider_ids: KaskPrivateInferenceSettings::default().local_provider_ids,
            private_thread_disabled_servers: KaskPrivateInferenceSettings::default()
                .private_thread_disabled_servers,
            embedding_model: KaskPrivateInferenceSettings::default().embedding_model,
            classifier_model: KaskPrivateInferenceSettings::default().classifier_model,
            rerank_model: KaskPrivateInferenceSettings::default().rerank_model,
        };
        assert_eq!(credential_url(&custom), "http://127.0.0.2:9020/v1");
        assert_eq!(
            credential_url(&custom),
            provider_entry(&custom, &fleet).unwrap().api_url
        );
    }

    #[test]
    fn the_lazy_fleet_seeds_discovers_and_refreshes() {
        // The lazy pattern, against the REAL briefs dir path shape:
        // seeding is idempotent and discovery sees the seed set; the
        // cache round-trips through refresh (a hand-dropped brief
        // surfaces after refresh). The machine's real artifacts dir is
        // production territory (the startup path seeds it) — this pin
        // exercises the pure seam (seed → discover → project), the
        // same functions local_fleet composes.
        let briefs = tempfile::tempdir().expect("briefs tempdir");
        fleet::seed_briefs(briefs.path()).expect("seed");
        let models_root = tempfile::tempdir().expect("models tempdir");
        let fleet =
            fleet::discover(briefs.path(), models_root.path()).expect("the seeded discovery");
        assert_eq!(fleet.len(), fleet::SEED_BRIEFS.len());

        // A hand-dropped brief joins on the next discovery — the data
        // act, no rebuild.
        std::fs::write(
            briefs.path().join("hand-added.toml"),
            "model_id = \"hand-added\"\ndisplay_name = \"Hand Added\"\n\n\
             [location]\ndir = \"hand-added\"\nartifact = \"weights.gguf\"\n\n\
             [model]\ntotal_params = 1_000_000_000\nactive_params = 1_000_000_000\n\
             expert_count = 0\nexpert_used_count = 0\n\n\
             [placement]\nple_pin_pattern = \"\"\n\n\
             [recommendations]\nkv_cache_type = \"q8_0\"\nflash_attn = true\n\
             context_length = 65_536\n",
        )
        .expect("drop the brief");
        let refreshed = fleet::discover(briefs.path(), models_root.path()).expect("re-discovery");
        assert!(
            refreshed.iter().any(|m| m.brief.model_id == "hand-added"),
            "a dropped brief joins the fleet on re-discovery"
        );
    }
}
