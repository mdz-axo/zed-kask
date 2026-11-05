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

use gpui::{App, AsyncApp};
use gpui_tokio::Tokio;
use settings::Settings as _;
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

/// Launch a private model — the page's action. The keychain token is
/// read at launch (the provider `api_url` slot — the one location, D5),
/// the launch runs on the kask tokio runtime, and a typed refusal
/// surfaces as the page's outcome memo plus a log line, never silently.
pub fn launch_private_model(model_id: String, cx: &mut App) {
    let settings = KaskSettings::get_global(cx).private_inference.clone();
    let token_url = credential_url(&settings);
    let provider = zed_credentials::global(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let token = match provider.read_credentials(&token_url, cx).await {
            Ok(Some((_, bytes))) => {
                let text = String::from_utf8_lossy(&bytes).trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            Ok(None) => None,
            Err(err) => {
                record_outcome(Some(format!("the token read failed at {token_url}: {err}")));
                return;
            }
        };
        let config = match resolve_launch_config(&model_id, &settings, token) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fleet_cards_carry_the_shipped_fleet() {
        let cards = fleet_cards();
        assert!(!cards.is_empty(), "the shipped fleet is non-empty");
        assert!(cards.iter().any(|(id, _)| id == "qwen3-8-flash-next"));
        assert!(cards.iter().any(|(id, _)| id == "gemma-4-31b"));
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
