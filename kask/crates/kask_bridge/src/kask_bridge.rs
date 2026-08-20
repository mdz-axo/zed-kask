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

mod cascade_context;
mod concurrency;
mod condenser_bridge;
mod context_injector;

mod identity;
mod inference;
mod inference_ipc_server;
mod inference_providers;
mod mcp_servers;
mod memory;
mod model_resolution;
mod settings;
mod skill_executor;

pub use cascade_context::{AgentCascadeContextProviderAdapter, BridgeCascadeContextProvider};
pub use concurrency::{global_concurrency_limiter, set_global_concurrency_limiter};
pub use condenser_bridge::BridgeThreadCondenser;
pub use context_injector::BridgeContextInjector;

pub use hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL;
/// Re-exports for the media IPC bridge — the composition root constructs the
/// media router and passes it to `InferenceIpcServer::start`. Re-exported here
/// so `zed` doesn't need a direct `hkask-inference` dependency for these two
/// types.
pub use hkask_inference::{InferenceConfig, MediaRouter};
/// Re-exported so the settings UI can display the resolved default data
/// directory without a direct `hkask-types` dependency.
pub use hkask_types::agent_paths::resolve_data_dir;
pub use identity::{
    ProvisionError, ProvisionedAgent, agent_name_from_username, mirror_provisioned_db_passphrase,
    mirror_runpod_api_key, provision_agent,
};
pub use inference::BridgeEditPredictionPort;
pub use inference::LanguageModelEmbeddingPort;
pub use inference::LanguageModelInferencePort;
pub use inference::NoModelInferencePort;
pub use inference_ipc_server::{InferenceIpcServer, WorktreeSpawner, set_worktree_spawner};
pub use inference_providers::{
    DATA_SERVICES, DataServiceDescriptor, INFERENCE_PROVIDERS, InferenceProviderDescriptor,
    credential_urls_for_mcp, ensure_openai_compatible_entries, mirror_env_keys_to_keychain,
    resolve_embedding_credentials,
};
pub use mcp_servers::{
    BUILT_IN_MCP_SERVERS, BUILT_IN_MCP_SERVERS_IDS, BUILT_IN_MCP_SERVERS_PAIRS, BuiltinMcpServer,
    build_mcp_server_env, filter_config_env_for_server, filter_credentials_for_server, find_server,
};
pub use memory::{
    BridgeAlertEscalationSink, BridgeMemoryPort, RealMemoryPort, open_curator_escalation_queue,
    open_curator_regulation_archive,
};
pub use model_resolution::resolve_model_names;
pub use settings::{
    KaskCodegraphSettings, KaskCollabSettings, KaskCompaniesSettings, KaskCondenserSettings,
    KaskCorpusSettings, KaskCuratorEmailSettings, KaskCuratorSettings, KaskDataServiceSettings,
    KaskGeneralSettings, KaskInferenceProvidersSettings, KaskMcpSettings, KaskMediaSettings,
    KaskMemorySettings, KaskModelsSettings, KaskPredictionMarketsSettings, KaskResearchSettings,
    KaskScenariosSettings, KaskSettings, KaskSwarmSettings, KaskToolRouterSettings,
    KaskTrainingSettings, SwarmModeConfig,
};
pub use skill_executor::{
    BridgeManifestExecutor, ProfileResolver, SnapshotProfileResolver, seed_registry_to_disk,
};

mod metacognition_bridge;
pub use metacognition_bridge::BridgeMetacognitionProvider;

mod directive_bridge;
pub use directive_bridge::BridgeCuratorDirectiveSink;

mod algedonic_log_bridge;
pub use algedonic_log_bridge::BridgeAlgedonicLogSink;

/// Open the swarm ledger's `DelegationCounter` at the standard path. Returns
/// `None` if the ledger cannot be opened. The `GroundingSensor` returns 0.0
/// (honest: "no gap detected") until the counter is wired.
pub fn open_swarm_delegation_counter()
-> Option<std::sync::Arc<dyn hkask_verification::DelegationCounter>> {
    let ledger_path = std::env::var("HKASK_SWARM_LEDGER_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::agent_paths::mcp_server_db("swarm", "ledger"),
            )
            .to_string_lossy()
            .to_string()
        });
    let pool = hkask_storage::SqliteDriver::file_pool(&ledger_path).ok()?;
    let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
        std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
    let ledger = hkask_ledger::Ledger::from_driver(driver).ok()?;
    Some(std::sync::Arc::new(SwarmLedgerDelegationCounter {
        ledger: std::sync::Arc::new(ledger),
    }))
}

/// Adapter that implements `DelegationCounter` for the swarm ledger.
/// Each delegation is a debit transaction with `metadata: { "action": "debit" }`.
/// Fund transactions are deposits, not delegations. Returns `None` on query
/// failure (absence ≠ 0 — a failed read is not a measured zero).
///
/// This duplicates the query logic in `hkask_mcp_swarm::local_runtime::
/// SwarmDelegationCounter`. They exist in separate crates because `kask_bridge`
/// cannot depend on `hkask-mcp-swarm` as a regular dependency (it is
/// dev-only). If the swarm ledger's transaction metadata schema changes
/// (e.g. the `"action": "debit"` filter), both implementations must be
/// updated. The `local_runtime` version is used by the swarm server process;
/// this version is used by the main zed process to wire the liveness-gap
/// sensor.
struct SwarmLedgerDelegationCounter {
    ledger: std::sync::Arc<hkask_ledger::Ledger>,
}

impl hkask_verification::DelegationCounter for SwarmLedgerDelegationCounter {
    fn delegation_count(&self) -> Option<u64> {
        let range = hkask_ledger::DateRange {
            start: "0000-01-01T00:00:00Z".to_string(),
            end: "9999-12-31T23:59:59Z".to_string(),
        };
        let filter = hkask_ledger::QueryFilter {
            account: Some("operator".to_string()),
            asset: Some("credits".to_string()),
            namespace: None,
        };
        let txs = self.ledger.query(&range, &filter).ok()?;
        Some(
            txs.iter()
                .filter(|tx| {
                    tx.metadata
                        .get("action")
                        .and_then(|a| a.as_str())
                        .is_some_and(|a| a == "debit")
                })
                .count() as u64,
        )
    }
}

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
