//! Private-inference zed-side seam (M7/Seam B) — the local leg's
//! provider registration: the M3 configured instance.
//!
//! M3 ruling (D-3 plan): the private endpoint is a CONFIGURED INSTANCE
//! of the existing `openai_compatible` provider class — a settings
//! entry, never new provider code. This module composes the PI-owned
//! policy (`kask.private_inference`) with the served fleet (the
//! shipped model briefs) into that entry and writes it through the
//! existing settings store; the registry re-reads settings on write
//! and registers the provider, so the models appear in the agent panel
//! model selector (Curator and Z-K alike) with no restart.
//!
//! The endpoint token is NOT handled here: it lives at the provider
//! `api_url` keychain slot (the same slot zed's `ApiKeyState` reads —
//! D5 one-key-one-location) and is written by the settings UI's
//! existing `write_credential` path plus the provider's live
//! `set_api_key` refresh.

use std::sync::Arc;

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
    /// The shipped model-brief fleet failed to decode — no provider
    /// entry is emitted with a partial fleet.
    #[error(transparent)]
    Fleet(#[from] hkask_private_inference::local_leg::fleet::FleetError),
}

/// Build the `openai_compatible` provider entry for the private local
/// leg from the PI-owned policy and the shipped model briefs.
///
/// `api_url` comes from the endpoint construction home
/// (`hkask_private_inference::local_leg::launch::endpoint_url`) so the
/// entry and a launched plan can never disagree on the endpoint shape.
/// The model cards project the brief fleet: `name` is the brief's
/// `model_id`, `display_name` its display name, `max_tokens` its
/// recommended context length. `auto_discover` matches the
/// settings-content default (`default_auto_discover()`, true): a
/// running llama-server merges its served ids into the cards; a
/// stopped one leaves the static cards intact.
pub fn provider_entry(
    pi: &KaskPrivateInferenceSettings,
) -> Result<OpenAiCompatibleSettingsContent, ProviderEntryError> {
    let briefs = hkask_private_inference::local_leg::fleet::shipped_model_briefs()?;
    let available_models = briefs
        .iter()
        .map(|brief| OpenAiCompatibleAvailableModel {
            name: brief.model_id.clone(),
            display_name: Some(brief.display_name.clone()),
            max_tokens: u64::from(brief.recommendations.context_length),
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
/// connect step. Idempotent: applying twice writes the same entry
/// bytes. Fails closed on an empty provider id or a broken fleet: no
/// `language_models` section is created on the refusal path.
pub fn apply_provider_entry(
    content: &mut SettingsContent,
    pi: &KaskPrivateInferenceSettings,
) -> Result<(), ProviderEntryError> {
    if pi.provider_id.trim().is_empty() {
        return Err(ProviderEntryError::EmptyProviderId);
    }
    let entry = provider_entry(pi)?;
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

    #[test]
    fn provider_entry_shapes_the_endpoint_and_cards() {
        let pi = KaskPrivateInferenceSettings::default();
        let entry = provider_entry(&pi).expect("entry builds from the default policy");
        assert_eq!(entry.api_url, "http://127.0.0.1:8010/v1");
        assert!(entry.auto_discover);
        let briefs = hkask_private_inference::local_leg::fleet::shipped_model_briefs()
            .expect("fleet decodes");
        assert_eq!(entry.available_models.len(), briefs.len());
        for (model, brief) in entry.available_models.iter().zip(&briefs) {
            assert_eq!(model.name, brief.model_id);
            assert_eq!(
                model.display_name.as_deref(),
                Some(brief.display_name.as_str())
            );
            assert_eq!(
                model.max_tokens,
                u64::from(brief.recommendations.context_length)
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
        let mut content = SettingsContent::default();
        apply_provider_entry(&mut content, &pi).expect("first apply writes");
        let once = serde_json::to_value(&content).expect("serializes");
        apply_provider_entry(&mut content, &pi).expect("second apply writes");
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
        let mut content = SettingsContent::default();
        let err = apply_provider_entry(&mut content, &pi)
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
        assert_eq!(credential_url(&pi), provider_entry(&pi).unwrap().api_url);
        let custom = KaskPrivateInferenceSettings {
            provider_id: "pi-test".into(),
            host: "127.0.0.2".into(),
            port: 9020,
        };
        assert_eq!(credential_url(&custom), "http://127.0.0.2:9020/v1");
        assert_eq!(
            credential_url(&custom),
            provider_entry(&custom).unwrap().api_url
        );
    }
}
