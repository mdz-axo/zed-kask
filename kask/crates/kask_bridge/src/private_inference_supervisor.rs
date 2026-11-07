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

use crate::private_inference::credential_url;
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

/// Whether the provider entry already exists in the USER settings —
/// the auto-registration's write-only-if-absent decision (an
/// operator's hand-configured or hand-edited entry always wins; a
/// deliberate deletion returns on the next restart — the row is
/// system-owned, like the seeded templates).
pub fn entry_present(content: &settings_content::SettingsContent, provider_id: &str) -> bool {
    content
        .language_models
        .as_ref()
        .and_then(|lm| lm.openai_compatible.as_ref())
        .is_some_and(|entries| entries.contains_key(provider_id))
}

/// The served fleet for the page's Launch buttons: `(model_id,
/// display_name)` projected from the shipped briefs — the same source
/// the provider entry's model cards render.
pub fn fleet_cards() -> Vec<(String, String)> {
    fleet::shipped_models()
        .unwrap_or_default()
        .into_iter()
        .map(|model| (model.brief.model_id, model.brief.display_name))
        .collect()
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
        cx.update(|cx| cx.refresh_windows());
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
        cx.update(|cx| cx.refresh_windows());
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

/// Register the provider entry — the page's explicit Register action:
/// mint-if-absent provisioning, then the M3 emission (an overwrite —
/// Register is the explicit refresh after host/port edits). Idempotent.
pub fn register_provider_entry(cx: &mut App) {
    let settings = KaskSettings::get_global(cx).private_inference.clone();
    let provider = zed_credentials::global(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
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
                    if let Err(err) = crate::private_inference::apply_provider_entry(settings, &pi)
                    {
                        tracing::warn!("private-inference registration refused: {err}");
                    }
                },
            );
        });
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// The startup registration — the "downloaded = available" half: when
/// the provider entry is absent, register it (mint + write, once, at
/// startup — the settings-store notification registers the provider
/// live); when it is present, provision the token if the slot is empty
/// and NEVER touch the entry (an operator's hand-edited row wins).
/// A deliberate deletion of the entry returns at the next restart: the
/// row is system-owned, like the seeded templates.
pub fn ensure_registered(cx: &mut App) {
    let settings = KaskSettings::get_global(cx).private_inference.clone();
    if settings.provider_id.trim().is_empty() {
        // An honest misconfiguration, never auto-written under.
        return;
    }
    let present = SettingsStore::global(cx)
        .raw_user_settings()
        .is_some_and(|user| entry_present(&user.content, &settings.provider_id));
    if present {
        let provider = zed_credentials::global(cx);
        cx.spawn(async move |cx: &mut AsyncApp| {
            mint_flow(&settings, provider.as_ref(), cx).await;
        })
        .detach();
        return;
    }
    register_provider_entry(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn mint_token_is_fresh_and_prefixed() {
        let a = mint_token();
        let b = mint_token();
        assert!(a.starts_with("pi-"), "{a}");
        assert!(a.len() > 8, "the minted token carries real entropy");
        assert_ne!(a, b, "every mint is fresh");
    }

    #[test]
    fn entry_present_decides_write_only_if_absent() {
        let mut content = settings_content::SettingsContent::default();
        assert!(
            !entry_present(&content, "private-local"),
            "absent registers"
        );

        // An entry under ANOTHER id is absent for this one.
        content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default()
            .insert(
                "other".into(),
                crate::private_inference::provider_entry(&KaskPrivateInferenceSettings::default())
                    .expect("entry builds"),
            );
        assert!(!entry_present(&content, "private-local"));

        // An existing entry always wins — never overwritten at startup.
        content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default()
            .insert(
                "private-local".into(),
                crate::private_inference::provider_entry(&KaskPrivateInferenceSettings::default())
                    .expect("entry builds"),
            );
        assert!(entry_present(&content, "private-local"));
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
    fn fleet_cards_carry_the_shipped_fleet() {
        let cards = fleet_cards();
        // Every shipped fleet brief is carded — the "downloaded =
        // available" contract (operator ruling 2026-11-05), extended by the
        // DME arc's fleet additions (P6, 2026-11-06: the canonical models
        // dir + the models.toml manifest are the one accounting — qwen35-0.8b
        // and its validated edited15 variant are fleet-served;
        // ornith-1.5-9b's LAUNCH entry fails closed (ArtifactNotFound) until
        // its repaired BF16 lands, but the brief stays carded).
        let mut ids: Vec<&str> = cards.iter().map(|(id, _)| id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "flint",
                "gemma-4-31b",
                "muse-glimmer-30b",
                "ornith-1.5-9b",
                "qwen3-8-flash-next",
                "qwen35-0.8b",
                "qwen35-0.8b-edited"
            ]
        );
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
