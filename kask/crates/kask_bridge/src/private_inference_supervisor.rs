//! The private-inference supervisor's zed-side surface (slice plan step
//! 8): the shared handle, the settings→config resolution, and the
//! Launch/Stop actions the Private Inference page binds. The supervisor
//! itself is the core crate's — this module resolves the zed-side
//! facts (settings, keychain token, canonical roots, backend binary)
//! and hands them over.
//!
//! Process placement: the launch runs on the kask tokio runtime via
//! `gpui_tokio::Tokio::spawn` (tokio-dependent futures panic under
//! `cx.background_spawn`); the llama-server child outlives the editor —
//! adoption recovers it next session (D-S3), the record is verified,
//! never trusted.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use gpui::{App, AsyncApp, ReadGlobal as _};
use gpui_tokio::Tokio;
use language_model::{LanguageModelProviderId, LanguageModelRegistry};
use settings::{Settings as _, SettingsStore};
use zed_credentials_provider as zed_credentials;

use hkask_private_inference::local_leg::fleet;
use hkask_private_inference::local_leg::placement::{GpuBackend, PlacementPolicy, probe_backend};
use hkask_private_inference::local_leg::supervisor::{LaunchConfig, Supervisor};

use crate::private_inference::{
    apply_provider_entry, credential_url, discover_local_fleet, provider_entry_matches,
};
use crate::settings::{KaskPrivateInferenceSettings, KaskSettings};

// The page renders the supervisor's live state through the bridge —
// settings_ui depends on kask_bridge, not on the core crate.
pub use hkask_private_inference::local_leg::supervisor::StatusSnapshot;

/// The shared supervisor — one serving instance per editor process;
/// the launch record (verified, D-S3) carries adoption across restarts.
static SUPERVISOR: OnceLock<Supervisor> = OnceLock::new();

/// The last Launch/Stop action's refusal reason — a session-local UI
/// memo the page renders next to the status; never authority (the
/// supervisor's status snapshot is the live state).
static LAST_ACTION_OUTCOME: Mutex<Option<String>> = Mutex::new(None);

pub fn supervisor() -> Supervisor {
    SUPERVISOR.get_or_init(Supervisor::default).clone()
}

/// A freshly-minted endpoint token — the system provisions its own
/// secret (the `provision_db_passphrase_string` precedent: the chain
/// is env → keychain → first-run generation; here the operator's
/// explicit token wins and an absent slot is first-run). Never shown,
/// never typed.
pub fn mint_token() -> String {
    format!("pi-{}", uuid::Uuid::new_v4().simple())
}

/// Token-provisioning failures — typed per-variant, never a blanket
/// string.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("reading the endpoint token at {url} failed: {source}")]
    Read { url: String, source: anyhow::Error },
    #[error("minting the endpoint token at {url} failed: {source}")]
    Mint { url: String, source: anyhow::Error },
}

/// The resolved endpoint token — existing or freshly minted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointToken {
    pub value: String,
    pub minted: bool,
}

/// Resolve the endpoint token, minting one when the slot is absent or
/// blank (first-run provisioning, never a fault): the minted token is
/// written to the SAME keychain slot the provider's `ApiKeyState`
/// reads (one key, one location) — the leg is usable with zero manual
/// credential entry.
pub async fn ensure_endpoint_token(
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    url: &str,
    cx: &AsyncApp,
) -> Result<EndpointToken, TokenError> {
    match credentials_provider.read_credentials(url, cx).await {
        Ok(Some((_, bytes))) => {
            let existing = String::from_utf8_lossy(&bytes).trim().to_string();
            if !existing.is_empty() {
                return Ok(EndpointToken {
                    value: existing,
                    minted: false,
                });
            }
        }
        Ok(None) => {}
        Err(source) => {
            return Err(TokenError::Read {
                url: url.to_string(),
                source,
            });
        }
    }
    let minted = mint_token();
    credentials_provider
        .write_credentials(url, "kask", minted.as_bytes(), cx)
        .await
        .map_err(|source| TokenError::Mint {
            url: url.to_string(),
            source,
        })?;
    Ok(EndpointToken {
        value: minted,
        minted: true,
    })
}

/// Ask the private-leg provider (a configured `openai_compatible`
/// instance — it exists only after the settings entry registers it) to
/// adopt `api_key` through its existing `set_api_key` path — the D29
/// RunPod-row precedent: the keychain write persists, the live refresh
/// makes a registered provider read it without an editor restart.
pub fn refresh_provider_key(api_key: Option<String>, provider_id: &str, cx: &mut App) {
    let Some(provider) = LanguageModelRegistry::global(cx)
        .read(cx)
        .provider(&LanguageModelProviderId::from(provider_id.to_string()))
    else {
        // Provider not registered yet (startup ordering): the keychain
        // write persists and the provider reads the slot at registration.
        return;
    };
    provider.set_api_key(api_key, cx).detach();
}

/// The served fleet for the page's Launch buttons — the discovered
/// fleet's cards `(model_id, display_name)`, the same source the
/// provider entry's model cards render. Pure, so the projection is
/// pinnable against a seeded fixture.
pub fn fleet_cards_from(fleet: &[fleet::FleetModel]) -> Vec<(String, String)> {
    fleet
        .iter()
        .map(|model| {
            (
                model.brief.model_id.clone(),
                model.brief.display_name.clone(),
            )
        })
        .collect()
}

/// The fleet's cards, discovered NOW — seed (idempotent) + scan +
/// decode on every call: the scan IS the refresh, never a memoized
/// cache (a memoized fleet is a stale fleet). The page's render path
/// stays cheap (a handful of small TOML reads) and a dropped brief +
/// weights surface on the next render with no button and no restart.
pub fn fleet_cards() -> Vec<(String, String)> {
    match discover_local_fleet() {
        Ok(fleet) => fleet_cards_from(&fleet),
        Err(err) => {
            tracing::warn!("private-inference fleet discovery failed: {err}");
            Vec::new()
        }
    }
}

/// The locally-available ollama models — the OTHER local leg, served
/// by ollama itself (no launch button: ollama serves its own models).
/// Cloud-proxied entries are excluded (the shared F1 classifier — the
/// list names what stays on this machine). The unified local set the
/// PI page surfaces: the discovered fleet plus these.
pub fn local_ollama_models(cx: &App) -> Vec<String> {
    let registry = LanguageModelRegistry::read_global(cx);
    registry
        .provider(&LanguageModelProviderId("ollama".to_string().into()))
        .map(|provider| {
            provider
                .provided_models(cx)
                .iter()
                .filter(|model| {
                    !settings_content::is_ollama_cloud_model_name(model.id().0.as_ref())
                })
                .map(|model| model.name().0.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The last action's refusal reason, if any (the page clears nothing;
/// a successful action resets it — the status line speaks then).
pub fn last_action_outcome() -> Option<String> {
    LAST_ACTION_OUTCOME
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .filter(|reason| !reason.is_empty())
}

fn record_outcome(reason: Option<String>) {
    if let Ok(mut slot) = LAST_ACTION_OUTCOME.lock() {
        *slot = reason;
    }
}

/// Resolution failures — typed, never a blanket string.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceError {
    /// No llama-server build under the canonical engine root — the
    /// engine install is missing, not silently fallen back.
    #[error(
        "no llama-server build found under {root} — the engine install is \
         missing (the probe walks HIP → Vulkan; build via the engine dir's \
         build scripts)"
    )]
    NoEngineBuild { root: String },
}

/// Resolve the launch config from the PI's settings, the keychain
/// token and the canonical roots, against an explicit backend — the
/// test seam (the None arm is pinned without touching the machine's
/// real engine install).
pub fn resolve_launch_config_with(
    backend: GpuBackend,
    model_id: &str,
    settings: &KaskPrivateInferenceSettings,
    api_key: Option<String>,
) -> Result<LaunchConfig, SurfaceError> {
    let binary = match backend {
        GpuBackend::Hip { binary } | GpuBackend::Vulkan { binary } => binary,
        GpuBackend::None => {
            return Err(SurfaceError::NoEngineBuild {
                root: hkask_private_inference::local_leg::placement::engine_root()
                    .to_string_lossy()
                    .to_string(),
            });
        }
    };
    Ok(LaunchConfig {
        model_id: model_id.to_string(),
        binary,
        api_key,
        state_dir: hkask_types::agent_paths::resolve_under_data_dir(Path::new("private-inference")),
        models_root: hkask_types::agent_paths::resolve_under_artifacts_dir(Path::new(
            "private-inference/models",
        )),
        // The discovered briefs dir — the fleet's source (seeding is
        // idempotent; the launch discovers the brief from here).
        briefs_dir: crate::private_inference::briefs_dir(),
        // The endpoint policy is the PI's settings row; the placement
        // knobs stay at the placement policy's own defaults (the
        // settings host/port delegation keeps the two in agreement).
        policy: PlacementPolicy {
            host: settings.host.clone(),
            port: settings.port,
            ..PlacementPolicy::default()
        },
    })
}

/// Resolve against the live machine (the measured HIP → Vulkan order).
pub fn resolve_launch_config(
    model_id: &str,
    settings: &KaskPrivateInferenceSettings,
    api_key: Option<String>,
) -> Result<LaunchConfig, SurfaceError> {
    resolve_launch_config_with(probe_backend(), model_id, settings, api_key)
}

/// Launch a private model — the page's action. The endpoint token is
/// resolved at launch (the provider `api_url` slot — the one location,
/// D5) with mint-if-absent provisioning (zero manual credential entry);
/// the launch runs on the kask tokio runtime, and a typed refusal
/// surfaces as the page's outcome memo plus a log line, never silently.
pub fn launch_private_model(model_id: String, cx: &mut App) {
    let settings = KaskSettings::get_global(cx).private_inference.clone();
    let token_url = credential_url(&settings);
    let provider = zed_credentials::global(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let token = match ensure_endpoint_token(provider.as_ref(), &token_url, cx).await {
            Ok(token) => token.value,
            Err(err) => {
                record_outcome(Some(err.to_string()));
                tracing::warn!("private-inference token provisioning failed: {err}");
                return;
            }
        };
        let config = match resolve_launch_config(&model_id, &settings, Some(token)) {
            Ok(config) => config,
            Err(err) => {
                record_outcome(Some(err.to_string()));
                tracing::warn!("private-inference launch refused: {err}");
                return;
            }
        };
        let supervisor = supervisor();
        // The future owns the handle + the config ('static for
        // `Tokio::spawn`); the config is consumed by value.
        let launch = async move { supervisor.launch(config).await };
        let outcome = match Tokio::spawn(cx, launch).await {
            Ok(Ok(serving)) => {
                tracing::info!(
                    "private leg serving {} at {}{}",
                    serving.model_id,
                    serving.endpoint,
                    if serving.adopted { " (adopted)" } else { "" }
                );
                None
            }
            Ok(Err(err)) => {
                tracing::warn!("private-inference launch refused: {err}");
                Some(err.to_string())
            }
            Err(join) => {
                tracing::warn!("private-inference launch task failed: {join}");
                Some(join.to_string())
            }
        };
        record_outcome(outcome);
        cx.update(|cx| {
            // The nudge: the registry re-discovers the provider's
            // models on the settings-observer channel (the same push
            // the credential-write nudge uses), so the picker's live
            // endpoint list reflects the launch — no button, no manual
            // refresh. The entry sync rides the same observer and is a
            // no-op here (the entry is derived and unchanged).
            settings::SettingsStore::notify_observers(cx);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Stop the serving server — SIGTERM, bounded drain, state cleanup.
pub fn stop_private_model(cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let supervisor = supervisor();
        let stop = async move { supervisor.stop().await };
        let outcome = match Tokio::spawn(cx, stop).await {
            Ok(Ok(())) => None,
            Ok(Err(err)) => {
                tracing::warn!("private-inference stop failed: {err}");
                Some(err.to_string())
            }
            Err(join) => {
                tracing::warn!("private-inference stop task failed: {join}");
                Some(join.to_string())
            }
        };
        record_outcome(outcome);
        cx.update(|cx| {
            // The same nudge on stop: the picker's live list drops the
            // stopped model on the registry's re-discovery.
            settings::SettingsStore::notify_observers(cx);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// The mint-if-absent provisioning shared by the registration paths —
/// writes the fresh token to the keychain slot and live-refreshes a
/// registered provider (the RunPod-row pattern). A provisioning
/// failure surfaces as a log line: the caller proceeds with whatever
/// the slot held (the launch resolves its own token anyway).
async fn mint_flow(
    settings: &KaskPrivateInferenceSettings,
    provider: &dyn credentials_provider::CredentialsProvider,
    cx: &mut AsyncApp,
) {
    let token_url = credential_url(settings);
    match ensure_endpoint_token(provider, &token_url, cx).await {
        Ok(token) if token.minted => {
            tracing::info!("private-inference endpoint token minted at {token_url}");
            cx.update(|cx| {
                refresh_provider_key(Some(token.value.clone()), &settings.provider_id, cx)
            });
        }
        Ok(_) => {}
        Err(err) => {
            tracing::warn!("private-inference token provisioning failed: {err}");
        }
    }
}

/// Sync the provider entry — the automatic registration (operator
/// ruling 2026-11-08: no button, no manual refresh): compute the
/// desired entry from the CURRENT policy, diff against the settings
/// content, write only when drifted (the `sync_kask_mcp_runtime_servers`
/// env-diff pattern), with mint-if-absent token provisioning first.
/// The startup path and the settings observer call it; a matching
/// entry is never touched (the no-op-write skip holds), so the
/// operator's policy rows are the ONE home and the entry is their
/// projection — never a second home for the endpoint or the fleet.
pub fn sync_provider_entry(cx: &mut App) {
    let provider = zed_credentials::global(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let settings = cx.update(|cx| KaskSettings::get_global(cx).private_inference.clone());
        mint_flow(&settings, provider.as_ref(), cx).await;
        cx.update(|cx| {
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    let pi: KaskPrivateInferenceSettings = settings
                        .kask
                        .as_ref()
                        .and_then(|kask| kask.private_inference.clone())
                        .map(Into::into)
                        .unwrap_or_default();
                    if pi.provider_id.trim().is_empty() {
                        // An honest misconfiguration, never auto-written under.
                        return;
                    }
                    if provider_entry_matches(settings, &pi) {
                        return; // The diff: a matching entry is never touched.
                    }
                    if let Err(err) = apply_provider_entry(settings, &pi) {
                        tracing::warn!("private-inference entry sync refused: {err}");
                        record_outcome(Some(err.to_string()));
                    }
                },
            );
            cx.refresh_windows();
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fleet discovered from a seeded tempdir — the pure-fixture
    /// counterpart of `local_fleet`'s lazy machine path (tests never
    /// touch the real artifacts dir).
    fn seeded_fleet() -> Vec<fleet::FleetModel> {
        let briefs = tempfile::tempdir().expect("briefs tempdir");
        fleet::seed_briefs(briefs.path()).expect("the seed set writes");
        let models_root = tempfile::tempdir().expect("models tempdir");
        fleet::discover(briefs.path(), models_root.path()).expect("the seeded fleet discovers")
    }

    #[test]
    fn mint_token_is_fresh_and_prefixed() {
        let a = mint_token();
        let b = mint_token();
        assert!(a.starts_with("pi-"), "{a}");
        assert!(a.len() > 8, "the minted token carries real entropy");
        assert_ne!(a, b, "every mint is fresh");
    }

    #[test]
    fn the_entry_is_derived_state_present_or_drifted_means_synced() {
        // The sync's observable (the bridge's `the_sync_diff_writes_only_when_drifted`
        // pins the diff; this pins the supervisor's read of it through the
        // same content shape): absent → not matching; under ANOTHER id →
        // not matching; matching the policy → matching. There is no
        // "present wins" rule anymore — the entry is derived state.
        let pi = KaskPrivateInferenceSettings::default();
        let mut content = settings_content::SettingsContent::default();
        assert!(
            !crate::private_inference::provider_entry_matches(&content, &pi),
            "absent entries never match — the sync writes them"
        );

        // An entry under ANOTHER id is still absent for this one.
        content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default()
            .insert(
                "other".into(),
                crate::private_inference::provider_entry(&pi),
            );
        assert!(!crate::private_inference::provider_entry_matches(
            &content, &pi
        ));

        // The policy's own entry → matching (the sync never touches it).
        crate::private_inference::apply_provider_entry(&mut content, &pi).expect("apply");
        assert!(
            crate::private_inference::provider_entry_matches(&content, &pi),
            "the derived entry matches the policy — the sync skips the write"
        );
    }

    #[gpui::test]
    async fn ensure_endpoint_token_mints_first_run_and_keeps_an_explicit_token(
        cx: &mut gpui::TestAppContext,
    ) {
        use credentials_provider::test_support::FakeCredentialsProvider;

        let async_cx = cx.to_async();
        let url = "http://127.0.0.1:8010/v1";

        // First run: the slot is absent → mint + write (the write is
        // visible — the provider reads back the minted token).
        let fresh = FakeCredentialsProvider::default();
        let token = ensure_endpoint_token(&fresh, url, &async_cx)
            .await
            .expect("first run mints");
        assert!(token.minted);
        assert!(token.value.starts_with("pi-"));
        let read_back = ensure_endpoint_token(&fresh, url, &async_cx)
            .await
            .expect("reads back");
        assert_eq!(
            read_back.value, token.value,
            "the write persisted at the slot"
        );
        assert!(
            !read_back.minted,
            "the second resolve keeps the existing token"
        );

        // An explicit operator token always wins — never re-minted.
        let explicit = FakeCredentialsProvider::default();
        explicit
            .state
            .lock()
            .keys
            .insert(url.to_string(), b"the-operators-token".to_vec());
        let token = ensure_endpoint_token(&explicit, url, &async_cx)
            .await
            .expect("explicit resolves");
        assert_eq!(token.value, "the-operators-token");
        assert!(!token.minted);
    }

    #[test]
    fn fleet_cards_project_the_discovered_fleet() {
        // The "downloaded = available" contract (operator ruling
        // 2026-11-05): every DISCOVERED fleet model is carded — the
        // card list is a projection of the discovered set, so adding a
        // model is a data act and the projection never drifts from the
        // fleet (the drift this pin's hardcoded predecessor let happen
        // at the -q8 landing, 2026-11-07).
        let fleet = seeded_fleet();
        let cards = fleet_cards_from(&fleet);
        assert_eq!(cards.len(), fleet.len());
        let mut carded: Vec<&str> = cards.iter().map(|(id, _)| id.as_str()).collect();
        let mut discovered: Vec<&str> = fleet.iter().map(|m| m.brief.model_id.as_str()).collect();
        carded.sort_unstable();
        discovered.sort_unstable();
        assert_eq!(carded, discovered, "the cards ARE the discovered fleet");
        // The seed set is what discovery sees after seeding.
        let mut seeded: Vec<&str> = fleet::SEED_BRIEFS
            .iter()
            .map(|(file, _)| file.trim_end_matches(".toml"))
            .collect();
        seeded.sort_unstable();
        assert_eq!(discovered, seeded);
        for (_, display) in &cards {
            assert!(!display.is_empty(), "every card renders a display name");
        }
    }

    #[test]
    fn resolve_config_pins_the_endpoint_from_the_settings_row() {
        let settings = KaskPrivateInferenceSettings::default();
        let backend = GpuBackend::Hip {
            binary: PathBuf::from("/stub/llama-server"),
        };
        let config = resolve_launch_config_with(
            backend,
            "qwen3-8-flash-next",
            &settings,
            Some("tok".into()),
        )
        .expect("a Hip backend resolves");
        assert_eq!(config.model_id, "qwen3-8-flash-next");
        assert_eq!(config.policy.host, "127.0.0.1");
        assert_eq!(config.policy.port, 8010);
        assert_eq!(
            config.binary,
            PathBuf::from("/stub/llama-server"),
            "the backend probe's binary is the launch binary"
        );
        // A custom settings row reaches the policy (the one agreement
        // seam, pinned by construction in the Default delegation).
        let custom = KaskPrivateInferenceSettings {
            host: "127.0.0.2".into(),
            port: 9020,
            ..settings
        };
        let backend = GpuBackend::Vulkan {
            binary: PathBuf::from("/stub/llama-server"),
        };
        let config = resolve_launch_config_with(backend, "gemma-4-31b", &custom, None)
            .expect("a Vulkan backend resolves");
        assert_eq!(config.policy.host, "127.0.0.2");
        assert_eq!(config.policy.port, 9020);
    }

    #[test]
    fn resolve_config_without_an_engine_build_fails_closed() {
        // The None arm names the canonical engine root — no silent
        // fallback to a CPU plan or a guessed binary.
        let err = resolve_launch_config_with(
            GpuBackend::None,
            "qwen3-8-flash-next",
            &KaskPrivateInferenceSettings::default(),
            Some("tok".into()),
        )
        .expect_err("no build, no launch");
        match err {
            SurfaceError::NoEngineBuild { root } => {
                assert!(root.contains("private-inference/engine"), "{root}");
            }
        }
    }
}
