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
//! silent-error-swallow trap the `.rules` file warns against. The socket
//! lives in a per-user private runtime dir
//! (`$XDG_RUNTIME_DIR/kask/kask-inference-{pid}.sock` — pid-keyed and
//! stable for the process, no per-instance nonce, 0600 in a 0700 dir;
//! `/tmp/kask-inference-{uid}/` is only the pre-XDG fallback) that the
//! OS reaps on reboot or tmpdir cleanup.

pub mod affordance;
mod condenser_bridge;
mod context_injector;
pub use affordance::{AffordanceInjector, PortAffordanceEmbedding};
pub mod credential_recovery;
mod credentials;

mod database_maintenance;
pub use database_maintenance::{
    ConfirmedInventory, DatabaseInventory, InventoryError, database_inventory_path,
    initialize_database_inventory, preview_database_inventory,
};
mod delegation_grants;
mod goals;
mod identity;
pub use goals::GoalHost;
mod inference_chat;
mod inference_resilience;
mod passphrase_rotation;
pub use delegation_grants::revoke_delegation_grant;
mod inference_embedding;
mod inference_ipc_server;
mod inference_providers;
mod inference_socket;
mod mcp_env;
mod mcp_servers;
mod memory;
mod model_resolution;
pub mod private_inference;
pub mod private_inference_supervisor;
mod settings;
#[cfg(test)]
mod test_support;

pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;
pub use hkask_types::agent_paths::{resolve_artifacts_dir, resolve_data_dir};

pub use identity::{ProvisionError, ProvisionedAgent, agent_name_from_username, provision_agent};
pub use inference_chat::{
    LanguageModelInferencePort, NoModelInferencePort, global_inference_port,
    set_global_inference_port,
};
pub use inference_embedding::{
    LanguageModelEmbeddingPort, build_private_embedding_port, resolve_private_classifier_model,
};
pub use inference_ipc_server::{
    InferenceIpcServer, WorktreeSpawner, set_worktree_spawner, swap_inference_ipc_ports,
};
pub use inference_providers::{
    DATA_SERVICES, DataServiceDescriptor, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    ResolvedEmbeddingCredentials, credential_url_for_key, credential_urls_for_mcp,
    data_service_credential_urls, local_ollama_embedding_credentials,
    resolve_embedding_credentials,
};
pub use inference_resilience::InferenceResilienceConfig;
pub use inference_socket::{
    get_inference_socket_path, get_inference_timeout_secs, set_inference_socket_path,
    set_inference_timeout_secs,
};
pub use mcp_servers::{
    BUILT_IN_MCP_SERVERS, BuiltinMcpServer, build_mcp_server_env, builtin_mcp_server_ids,
    builtin_mcp_server_pairs, filter_credentials_for_server,
    remove_shadowing_context_server_entries, shadowed_context_server_entry_ids,
};
pub use memory::{
    BridgeMemoryPort, CuratorRegulationArchive, RealMemoryPort, hydrate_operator_feedback,
    open_curator_regulation_archive, persist_operator_feedback, persist_skill_outcome,
    persist_skill_tool_failure,
};
pub use model_resolution::resolve_model_names;
pub use passphrase_rotation::{
    PassphraseRotationError, PendingRotationState, cancel_pending_db_rotation,
    read_pending_rotation_state, run_pending_db_passphrase_rotation,
    schedule_db_passphrase_rotation,
};
pub use settings::{
    KaskCondenserSettings, KaskCorpusSettings, KaskCuratorSettings, KaskGeneralSettings,
    KaskMcpSettings, KaskMediaSettings, KaskMemorySettings, KaskModelsSettings,
    KaskPredictionMarketsSettings, KaskPrivateInferenceSettings, KaskResearchSettings,
    KaskScenariosSettings, KaskSettings,
};

mod metacognition_bridge;
pub use metacognition_bridge::BridgeMetacognitionProvider;

mod directive_bridge;
pub use directive_bridge::BridgeCuratorDirectiveSink;

mod algedonic_log_bridge;
pub use algedonic_log_bridge::BridgeAlgedonicLogSink;

mod context_server_health_bridge;
pub use context_server_health_bridge::BridgeContextServerHealthSource;

mod ocr_health_bridge;
pub use ocr_health_bridge::BridgeOcrHealthSource;

mod experimentation_health_bridge;
pub use experimentation_health_bridge::BridgeExperimentationHealthSource;

mod algedonic_board;
#[cfg(test)]
mod algedonic_board_real_tests;
pub use algedonic_board::{ALGEDONIC_BOARD_NAME, BoardAlertEscalationSink};

// The credential namespace constant lives in the `credentials` module;
// re-exported here to preserve the historical public surface
// (`kask_bridge::KASK_CREDENTIAL_NAMESPACE`).
// `purge_removed_abw_api_key` and `purge_removed_reduct_api_key` are the
// startup purges of the removed ABW and Reduct.cloud keys (D5) — called once
// from zed's app init.
pub use credentials::{
    KASK_CREDENTIAL_NAMESPACE, purge_removed_abw_api_key, purge_removed_reduct_api_key,
};
