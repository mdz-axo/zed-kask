#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! kask_bridge — the sole bidirectional seam between hKask and zed-kask (D8).
//!
//! hKask crates define port traits in `hkask-types` (`InferencePort`,
//! `ToolPort`, etc.). This crate implements those ports over zed-kask
//! facilities (`LanguageModel`, the in-process tool registry).
//!
//! Governing invariant: hKask crates NEVER depend on zed crates; zed-kask
//! depends on hKask. This bridge is the only crate that depends on both sides.

mod condenser_bridge;
mod context_injector;
mod github_update;
mod identity;
mod inference;
mod inference_ipc_server;
mod inference_providers;
mod mcp_servers;
mod memory;
mod model_resolution;
mod settings;
mod skill_executor;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;
pub use github_update::{ZedKaskReleaseAsset, get_zed_kask_release_asset};
/// Re-exports for the media IPC bridge — the composition root constructs the
/// media router and passes it to `InferenceIpcServer::start`. Re-exported here
/// so `zed` doesn't need a direct `hkask-inference` dependency for these two
/// types.
pub use hkask_inference::{InferenceConfig, MediaRouter};
pub use identity::{
    ProvisionError, ProvisionedAgent, agent_name_from_username, provision_agent,
    webid_from_username,
};
pub use inference::LanguageModelEmbeddingPort;
pub use inference::LanguageModelInferencePort;
pub use inference::NoModelInferencePort;
pub use inference_ipc_server::InferenceIpcServer;
pub use inference_providers::{
    DATA_SERVICE_CREDENTIALS, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    credential_urls_for_mcp, delete_data_service_api_key, delete_provider_api_key,
    ensure_openai_compatible_entries, has_provider_api_key,
    mirror_env_keys_to_keychain, provider_credential_url, resolve_embedding_credentials,
    write_data_service_api_key, write_provider_api_key,
};
pub use mcp_servers::{
    BUILT_IN_MCP_SERVERS, BUILT_IN_MCP_SERVERS_IDS, BUILT_IN_MCP_SERVERS_PAIRS, BuiltinMcpServer,
    filter_config_env_for_server, filter_credentials_for_server, find_server,
};
pub use memory::{
    BridgeAlertEscalationSink, BridgeMemoryPort, RealMemoryPort, open_curator_escalation_queue,
    open_curator_regulation_archive,
};
pub use model_resolution::resolve_model_names;
pub use settings::{
    KaskCodegraphSettings, KaskCollabSettings, KaskCompaniesSettings, KaskCondenserSettings,
    KaskCorpusSettings, KaskCuratorEmailSettings, KaskCuratorSettings, KaskDataServiceSettings,
    KaskInferenceProvidersSettings, KaskMcpSettings, KaskMediaSettings, KaskMemorySettings,
    KaskModelsSettings, KaskPredictionMarketsSettings, KaskScenariosSettings, KaskSettings,
    KaskSwarmSettings, KaskTrainingSettings, SwarmModeConfig,
};
pub use skill_executor::{
    BridgeManifestExecutor, ProfileResolver, SnapshotProfileResolver, seed_registry_to_disk,
};

mod metacognition_bridge;
pub use metacognition_bridge::BridgeMetacognitionProvider;

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Used by the settings UI to read/write API keys via zed's CredentialsProvider.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// Send a test email to verify MXroute credentials are working.
///
/// Spawns the send on the kask tokio runtime (reqwest needs tokio for I/O).
/// Returns immediately — the caller (settings UI) can't observe the result
/// synchronously, but the `reg.email.sent` / `reg.alert` tracing spans surface
/// success/failure in the logs.
///
/// No-op when email is not configured (`send_test_email` returns
/// `Err(NotConfigured)` which is logged at `warn` level by the spawned task).
pub fn spawn_test_email(recipient: String, cx: &gpui::App) {
    gpui_tokio::Tokio::spawn(cx, async move {
        match hkask_email::send_test_email(&recipient).await {
            Ok(()) => tracing::info!(
                target: "reg.email.sent",
                recipient = %recipient,
                "Test email sent successfully"
            ),
            Err(e) => tracing::warn!(
                target: "reg.email.sent",
                error = %e,
                recipient = %recipient,
                "Test email failed"
            ),
        }
    })
    .detach();
}

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils {
    pub use crate::context_injector::BridgeContextInjector;

    /// Expose the pure prompt-length recall gate as a free function for
    /// proptest. `should_recall` is an associated function on
    /// `BridgeContextInjector`; a method cannot be re-exported via `pub use`,
    /// so this thin wrapper forwards to the `pub(crate)` impl.
    pub fn should_recall(prompt: &str) -> bool {
        BridgeContextInjector::should_recall(prompt)
    }
}
