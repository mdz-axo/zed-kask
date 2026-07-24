//! kask_bridge — the sole bidirectional seam between hKask and zed-kask (D8).
//!
//! hKask crates define port traits in `hkask-types` (`InferencePort`, `SecretsPort`,
//! `ToolPort`, etc.). This crate implements those ports over zed-kask facilities
//! (`LanguageModel`, `CredentialsProvider`, the in-process tool registry).
//!
//! Governing invariant: hKask crates NEVER depend on zed crates; zed-kask depends on
//! hKask. This bridge is the only crate that depends on both sides.

mod inference;
mod memory;
mod secrets;
mod settings;
mod skill_executor;
mod tool_port;

pub use inference::LanguageModelInferencePort;
pub use memory::{BridgeMemoryPort, LoggingMemoryPort};
pub use secrets::{CredentialsSecretsPort, KASK_CREDENTIAL_NAMESPACE};
pub use settings::KaskSettings;
pub use skill_executor::BridgeManifestExecutor;
pub use tool_port::BridgeToolPort;
