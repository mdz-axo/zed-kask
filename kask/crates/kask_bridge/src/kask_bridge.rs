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
mod fusion_model;
mod identity;
mod inference;
mod inference_ipc_server;
mod inference_providers;
mod mcp_servers;
mod memory;
mod settings;
mod skill_executor;
mod tool_port;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::{BridgeContextInjector, BridgeCuratorContextInjector};
pub use fusion_model::{
    FUSION_MODEL_ID, FUSION_PROVIDER_ID, FusionLanguageModel, FusionLanguageModelProvider,
    discover_favorites, favorite_model_selections, fusion_model_selection, resolve_fusion_models,
    should_auto_discover,
};
/// Re-export so the composition root can name the type without depending on
/// `hkask-inference` directly.
pub use hkask_inference::artificial_analysis::FavoriteModel;
/// Re-exports for the media IPC bridge — the composition root constructs the
/// media router and passes it to `InferenceIpcServer::start`. Re-exported here
/// so `zed` doesn't need a direct `hkask-inference` dependency for these two
/// types (matching the `FavoriteModel` pattern above).
pub use hkask_inference::{InferenceConfig, MediaRouter};
pub use identity::{
    ProvisionedAgent, agent_name_from_username, provision_agent, webid_from_username,
};
pub use inference::LanguageModelEmbeddingPort;
pub use inference::LanguageModelInferencePort;
pub use inference::NoModelInferencePort;
pub use inference_ipc_server::InferenceIpcServer;
pub use inference_providers::{
    DATA_SERVICE_CREDENTIALS, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    credential_urls_for_mcp, delete_data_service_api_key, delete_provider_api_key,
    ensure_openai_compatible_entries, has_data_service_api_key, has_provider_api_key,
    provider_credential_url, resolve_embedding_credentials, write_data_service_api_key,
    write_provider_api_key,
};
pub use mcp_servers::{
    BUILT_IN_MCP_SERVERS, BUILT_IN_MCP_SERVERS_IDS, BUILT_IN_MCP_SERVERS_PAIRS, BuiltinMcpServer,
    filter_config_env_for_server, filter_credentials_for_server, find_server,
};
pub use memory::{BridgeMemoryPort, LoggingMemoryPort, RealMemoryPort};
pub use settings::{
    KaskCodegraphSettings, KaskCompaniesSettings, KaskCondenserSettings, KaskCorpusSettings,
    KaskCuratorEmailSettings, KaskCuratorSettings, KaskDataServiceSettings, KaskFusionSettings,
    KaskGuardSettings, KaskInferenceProvidersSettings, KaskMcpSettings, KaskMediaSettings,
    KaskMemorySettings, KaskModelsSettings, KaskScenariosSettings, KaskSettings,
    KaskTrainingSettings,
};
pub use skill_executor::BridgeManifestExecutor;
pub use tool_port::BridgeToolPort;

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
