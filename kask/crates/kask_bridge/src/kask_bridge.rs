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
//!
//! ## Socket cleanup (InferenceIpcServer)
//!
//! `InferenceIpcServer::start` spawns a detached tokio task that owns the
//! `UnixListener`. The struct itself is kept alive for the process lifetime
//! (the call site binds it to `_ipc_server`). Rust does not drop detached
//! tasks or process-global statics on exit, so a `Drop` impl would be dead
//! code — and the `let _ = std::fs::remove_file(...)` it would contain is the
//! silent-error-swallow trap the `.rules` file warns against. The socket lives
//! in a per-user private tmpdir (`/tmp/kask-inference-{uid}/kask-inference-{pid}-{nonce}`,
//! 0600 in a 0700 dir) that the OS reaps on reboot or tmpdir cleanup.

mod condenser_bridge;
mod context_injector;
mod credentials;

mod identity;
mod inference_chat;
mod inference_edit_prediction;
mod inference_embedding;
mod inference_ipc_server;
mod inference_providers;
mod inference_socket;
mod mcp_env;
mod mcp_servers;
mod memory;
mod model_resolution;
mod settings;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;
pub use hkask_inference::model_constants::{
    DEFAULT_CLASSIFIER_MODEL, DEFAULT_EMBEDDING_MODEL, DEFAULT_FALLBACK_MODEL, DEFAULT_OCR_MODEL,
};
pub use hkask_types::agent_paths::resolve_data_dir;

pub use identity::{
    BridgeRotationError, ProvisionError, ProvisionedAgent, agent_name_from_username,
    provision_agent, provision_swarm_memory_passphrase, rotate_curator_db_passphrase,
    rotate_swarm_memory_db_passphrase,
};
pub use inference_chat::{LanguageModelInferencePort, NoModelInferencePort};
pub use inference_edit_prediction::BridgeEditPredictionPort;
pub use inference_embedding::LanguageModelEmbeddingPort;
pub use inference_ipc_server::{InferenceIpcServer, WorktreeSpawner, set_worktree_spawner};
pub use inference_providers::{
    DATA_SERVICES, DataServiceDescriptor, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    credential_urls_for_mcp, mirror_credential_to_provider, mirror_kask_credentials_to_providers,
    resolve_embedding_credentials,
};
pub use inference_socket::{
    get_inference_socket_path, get_inference_timeout_secs, set_inference_socket_path,
    set_inference_timeout_secs,
};
pub use mcp_servers::{
    BUILT_IN_MCP_SERVERS, BuiltinMcpServer, build_mcp_server_env, builtin_mcp_server_ids,
    builtin_mcp_server_pairs, filter_credentials_for_server,
};
pub use memory::{
    BridgeAlertEscalationSink, BridgeMemoryPort, RealMemoryPort, open_curator_escalation_queue,
    open_curator_regulation_archive,
};
pub use model_resolution::resolve_model_names;
pub use settings::{
    KaskCompaniesSettings, KaskCondenserSettings, KaskCorpusSettings, KaskCuratorEmailSettings,
    KaskCuratorSettings, KaskGeneralSettings, KaskMcpSettings, KaskMemorySettings,
    KaskModelsSettings, KaskPredictionMarketsSettings, KaskResearchSettings, KaskScenariosSettings,
    KaskSettings, KaskSwarmSettings, KaskToolRouterSettings, KaskTrainingSettings, SwarmModeConfig,
};

mod metacognition_bridge;
pub use metacognition_bridge::BridgeMetacognitionProvider;

mod directive_bridge;
pub use directive_bridge::BridgeCuratorDirectiveSink;

mod algedonic_log_bridge;
pub use algedonic_log_bridge::BridgeAlgedonicLogSink;

mod context_server_health_bridge;
pub use context_server_health_bridge::BridgeContextServerHealthSource;

mod rollout_event_bridge;

pub use rollout_event_bridge::{
    BridgeRolloutEventSource, HarnessRegression, check_harness_regressions,
};

// The credential namespace constant and the test-email helper live in the
// `credentials` module; re-exported here to preserve the historical public
// surface (`kask_bridge::KASK_CREDENTIAL_NAMESPACE`, `kask_bridge::spawn_test_email`).
pub use credentials::{KASK_CREDENTIAL_NAMESPACE, spawn_test_email};
