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
mod memory;
mod settings;
mod skill_executor;
mod tool_port;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;
pub use fusion_model::{
    FUSION_MODEL_ID, FUSION_PROVIDER_ID, FusionLanguageModel, FusionLanguageModelProvider,
    discover_favorites, favorite_model_selections, fusion_model_selection, resolve_fusion_models,
    should_auto_discover,
};
/// Re-export so the composition root can name the type without depending on
/// `hkask-inference` directly.
pub use hkask_inference::openrouter_backend::FavoriteModel;
pub use identity::{
    ProvisionedAgent, agent_name_from_username, provision_agent, webid_from_username,
};
pub use inference::LanguageModelInferencePort;
pub use inference_ipc_server::InferenceIpcServer;
pub use inference_providers::{
    DATA_SERVICE_CREDENTIALS, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    credential_urls_for_mcp, delete_data_service_api_key, delete_provider_api_key,
    ensure_openai_compatible_entries, has_data_service_api_key, has_provider_api_key,
    provider_credential_url, write_data_service_api_key, write_provider_api_key,
};
pub use memory::{BridgeMemoryPort, LoggingMemoryPort, RealMemoryPort};
pub use settings::KaskSettings;
pub use skill_executor::BridgeManifestExecutor;
pub use tool_port::BridgeToolPort;

mod metacognition_bridge;
pub use metacognition_bridge::BridgeMetacognitionProvider;

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Used by the settings UI to read/write API keys via zed's CredentialsProvider.
pub const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";
