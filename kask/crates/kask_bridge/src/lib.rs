//! kask_bridge — the sole bidirectional seam between hKask and zed-kask (D8).
//!
//! hKask crates define port traits in `hkask-types` (`InferencePort`, `SecretsPort`,
//! `ToolPort`, etc.). This crate implements those ports over zed-kask facilities
//! (`LanguageModel`, `CredentialsProvider`, the in-process tool registry).
//!
//! Governing invariant: hKask crates NEVER depend on zed crates; zed-kask depends on
//! hKask. This bridge is the only crate that depends on both sides.

mod condenser_bridge;
mod context_injector;
mod fusion_model;
mod identity;
mod inference;
mod memory;
mod secrets;
mod settings;
mod skill_executor;
mod tool_port;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;
pub use fusion_model::{
    FUSION_MODEL_ID, FUSION_PROVIDER_ID, FusionLanguageModel, FusionLanguageModelProvider,
    resolve_fusion_models,
};
pub use identity::{
    ProvisionedUserpod, provision_userpod, userpod_name_from_username, webid_from_username,
};
pub use inference::LanguageModelInferencePort;
pub use memory::{BridgeMemoryPort, LoggingMemoryPort, RealMemoryPort};
pub use secrets::{CredentialsSecretsPort, KASK_CREDENTIAL_NAMESPACE};
pub use settings::KaskSettings;
pub use skill_executor::BridgeManifestExecutor;
pub use tool_port::BridgeToolPort;
