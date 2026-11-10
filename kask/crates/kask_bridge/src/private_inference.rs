//! Private-inference zed-side seam (M7/Seam B) — the local leg's
//! provider registration: the M3 configured instance.
//!
//! M3 ruling (D-3 plan): the private endpoint is a CONFIGURED INSTANCE
//! of the existing `openai_compatible` provider class — a settings
//! entry, never new provider code. The entry is PURE ENDPOINT CONFIG,
//! derived state (operator ruling 2026-11-08): `api_url` from the
//! PI-owned policy, `auto_discover` on, NO model cards — the picker
//! shows what the endpoint actually serves (live discovery at
//! `/v1/models` while a llama-server is up), never a stale snapshot of
//! the fleet. The entry SYNCs automatically (compute, diff, write —
//! the MCP-server sync pattern): absent or drifted from the policy →
//! written; matching → untouched. No register button, no manual
//! refresh, no hand-edit-wins rule — the policy fields (`provider_id`,
//! `host`, `port`) are the operator's rows; the entry is their
//! projection.
//!
//! The endpoint token is NOT handled here: it lives at the provider
//! `api_url` keychain slot (the same slot zed's `ApiKeyState` reads —
//! D5 one-key-one-location) and is written by the settings UI's
//! existing `write_credential` path plus the provider's live
//! `set_api_key` refresh.

use std::path::{Path, PathBuf};

use hkask_private_inference::local_leg::fleet::{self, FleetError, FleetModel};
use settings_content::{OpenAiCompatibleSettingsContent, SettingsContent};

use crate::settings::KaskPrivateInferenceSettings;

/// Entry-emission failures — per-variant, never a blanket string (the
/// string-errors discipline). With cards gone the emission is pure
/// endpoint config; an empty provider id is the one refusal.
#[derive(Debug, thiserror::Error)]
pub enum ProviderEntryError {
    /// The configured provider id is empty — the write is refused
    /// rather than registering an unnamed provider (fail-closed).
    #[error(
        "the private-inference provider id is empty — set \
         kask.private_inference.provider_id before registering"
    )]
    EmptyProviderId,
}

/// The discovered briefs directory under the artifacts root — the
/// fleet's source of truth (operator ruling 2026-11-08).
pub fn briefs_dir() -> PathBuf {
    hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/model-briefs",
    ))
}

/// Discover the local fleet NOW — idempotent seeding (the compiled-in
/// set writes only when absent) followed by a directory scan + brief
/// decode. Discovery is cheap (a handful of small TOML reads), so
/// callers run it on demand; the discovered set is never memoized,
/// because a memoized fleet is a stale fleet (the same rule as the
/// endpoint cards: derive, never snapshot).
pub fn discover_local_fleet() -> Result<Vec<FleetModel>, FleetError> {
    let briefs = briefs_dir();
    // Seeding is the convenience, discovery the source of truth: a
    // seeding IO failure is surfaced (warned) and discovery's typed
    // directory error follows (the path is named either way).
    if let Err(err) = fleet::seed_briefs(&briefs) {
        tracing::warn!("private-inference brief seeding failed: {err}");
    }
    let models_root = hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
        "private-inference/models",
    ));
    fleet::discover(&briefs, &models_root)
}

/// The desired provider entry — pure endpoint config: `api_url` from
/// the endpoint construction home
/// (`hkask_private_inference::local_leg::launch::endpoint_url`) so the
/// entry and a launched plan can never disagree on the endpoint shape,
/// `auto_discover` on (the picker's live model list IS the endpoint's
/// `/v1/models` while a server is up), and NO model cards — the fleet
/// never enters settings.json (the drift class is structurally gone).
pub fn provider_entry(pi: &KaskPrivateInferenceSettings) -> OpenAiCompatibleSettingsContent {
    OpenAiCompatibleSettingsContent {
        api_url: hkask_private_inference::local_leg::launch::endpoint_url(&pi.host, pi.port),
        api_key_url: None,
        available_models: Vec::new(),
        custom_headers: None,
        auto_discover: true,
    }
}

/// Whether the settings content already carries the desired entry —
/// the sync's diff (compute, compare, write only when drifted; the
/// `sync_kask_mcp_runtime_servers` env-diff pattern).
pub fn provider_entry_matches(
    content: &SettingsContent,
    pi: &KaskPrivateInferenceSettings,
) -> bool {
    let Some(current) = content
        .language_models
        .as_ref()
        .and_then(|lm| lm.openai_compatible.as_ref())
        .and_then(|oc| oc.get(pi.provider_id.as_str()))
    else {
        return false;
    };
    *current == provider_entry(pi)
}

/// The keychain URL the private leg's endpoint token lives at — the
/// provider entry's `api_url`, i.e. the SAME slot zed's `ApiKeyState`
/// reads for the registered provider (D5 one-key-one-location: the
/// settings UI's token row writes here and only here).
pub fn credential_url(pi: &KaskPrivateInferenceSettings) -> String {
    hkask_private_inference::local_leg::launch::endpoint_url(&pi.host, pi.port)
}

/// Apply the provider entry into settings content — the sync's write
/// leg. Idempotent (the desired entry is a pure function of the
/// policy); fails closed on an empty provider id, writing nothing.
pub fn apply_provider_entry(
    content: &mut SettingsContent,
    pi: &KaskPrivateInferenceSettings,
) -> Result<(), ProviderEntryError> {
    if pi.provider_id.trim().is_empty() {
        return Err(ProviderEntryError::EmptyProviderId);
    }
    content
        .language_models
        .get_or_insert_with(Default::default)
        .openai_compatible
        .get_or_insert_with(Default::default)
        .insert(pi.provider_id.as_str().into(), provider_entry(pi));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_entry_is_pure_endpoint_config() {
        let pi = KaskPrivateInferenceSettings::default();
        let entry = provider_entry(&pi);
        assert_eq!(entry.api_url, "http://127.0.0.1:8010/v1");
        assert!(entry.auto_discover);
        assert!(
            entry.available_models.is_empty(),
            "no model cards — the picker's list is live endpoint discovery, \
             never a fleet snapshot"
        );
    }

    #[test]
    fn the_sync_diff_writes_only_when_drifted() {
        let pi = KaskPrivateInferenceSettings::default();

        // Absent → not matching → the write lands.
        let mut content = SettingsContent::default();
        assert!(!provider_entry_matches(&content, &pi));
        apply_provider_entry(&mut content, &pi).expect("the sync writes the absent entry");
        assert!(
            provider_entry_matches(&content, &pi),
            "after the write the entry matches the policy"
        );

        // Drifted (a stale hand-edited or old- endpoint) → rewritten to
        // the policy's projection — the entry is derived state, never a
        // second home for the endpoint.
        let mut drifted = provider_entry(&pi);
        drifted.api_url = "http://127.0.0.1:9999/v1".into();
        content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default()
            .insert("private-local".into(), drifted);
        assert!(!provider_entry_matches(&content, &pi));
        apply_provider_entry(&mut content, &pi).expect("the sync rewrites the drifted entry");
        assert!(provider_entry_matches(&content, &pi));

        // A policy change (host/port) → the desired entry moves → the
        // next sync rewrites (the diff sees the drift).
        let moved = KaskPrivateInferenceSettings {
            host: "127.0.0.2".into(),
            port: 9020,
            ..pi.clone()
        };
        assert!(!provider_entry_matches(&content, &moved));
        apply_provider_entry(&mut content, &moved).expect("the sync follows the policy");
        assert!(provider_entry_matches(&content, &moved));
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
        assert_eq!(credential_url(&pi), provider_entry(&pi).api_url);
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
        assert_eq!(credential_url(&custom), provider_entry(&custom).api_url);
    }

    #[test]
    fn seeding_and_discovery_surface_dropped_briefs() {
        // The on-demand discovery's pure seam: seed → discover → a
        // hand-dropped brief joins on the NEXT discovery (no memoized
        // fleet to refresh — the scan IS the refresh).
        let briefs = tempfile::tempdir().expect("briefs tempdir");
        fleet::seed_briefs(briefs.path()).expect("seed");
        let models_root = tempfile::tempdir().expect("models tempdir");
        let fleet =
            fleet::discover(briefs.path(), models_root.path()).expect("the seeded discovery");
        assert_eq!(fleet.len(), fleet::SEED_BRIEFS.len());

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
        let rediscovered =
            fleet::discover(briefs.path(), models_root.path()).expect("re-discovery");
        assert!(
            rediscovered
                .iter()
                .any(|m| m.brief.model_id == "hand-added"),
            "a dropped brief joins the fleet on the next discovery — the scan \
             IS the refresh, never a memoized cache"
        );
    }
}
