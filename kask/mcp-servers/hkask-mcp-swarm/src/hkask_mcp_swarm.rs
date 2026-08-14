#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Swarm — Agent Bestiary World (ABW) integration server.
//!
//! Exposes ABW's agent catalogue, workspaces ("swarms"), and the Xaman Ek
//! curator as MCP tools, governed by the kask MCP runtime (OCAP, gas, spans).
//!
//! ## API surface (verified 2026-08-01 against the live service; lifecycle
//! endpoints — agent create/delete, fire, hire-via-`/add`, workspace delete
//! via the team route — re-verified 2026-08-02)
//! - Base URL: `https://agent-bestiary.world` (no `api.` subdomain)
//! - Auth: `Authorization: Bearer <key>` (Pro-tier API key, scopes read/write/execute)
//! - Open: `GET /api/agents`, `GET /api/models/catalogue`
//! - Authed: `/api/workspaces`, `/api/agents/{name}/execute`, `/api/xaman/sessions`,
//!   `/api/wallet`, `/api/wallet/transactions` (reconciliation read, verified
//!   2026-08-02)
//!
//! ## Error model
//! ABW returns HTTP 200 envelopes containing upstream LLM errors in the body
//! (e.g. Xaman Ek passing through Anthropic credit exhaustion verbatim), and
//! HTTP 500 for domain failures like unfunded agents. `SwarmError` mapping
//! therefore inspects response bodies, not just status codes.
//!
//! ## Tools (41 — both tool sets always available in either mode)
//! ABW tools (27): `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`,
//! `swarm_list_apps`, `swarm_ontology_templates`, `swarm_execute_agent`,
//! `swarm_hire_cost`, `swarm_request_consent`, `swarm_authorize_session`,
//! `swarm_hire`, `swarm_delegate`, `swarm_delegate_and_wait`, `swarm_fanout`,
//! `swarm_run_status`, `swarm_generate_prompt`, `swarm_generate_ontology`,
//! `swarm_create_agent`, `swarm_create_swarm`, `swarm_xaman`, `swarm_create_app`,
//! `swarm_fire` (roster removal, verified live), `swarm_delete_agent`
//! (permanent agent deletion, verified live), `swarm_delete_swarm`
//! (permanent workspace deletion via the team-scoped route, verified live),
//! `swarm_search_knowledge` (knowledge-graph search, fermi v0.10.26),
//! `swarm_publish_checks` (publish preflight, fermi v0.10.15),
//! `swarm_publish_agent` (catalogue publish, fermi v0.10.5/v0.10.15),
//! `swarm_fork_agent` (derivative fork, fermi v0.10.16).
//! Local tools (14): `swarm_fund_local`, `swarm_balance_local`,
//! `swarm_local_history`, `swarm_delegate_local`, `swarm_fanout_local`,
//! `swarm_pipeline_local`, `swarm_a2a_send` (A2A protocol message, in-process),
//! `swarm_a2a_card` (A2A Agent Card discovery),
//! `swarm_list_local_agents`, `swarm_clone_to_local`,
//! `swarm_push_to_cloud`, `swarm_remove_local`, `swarm_create_local_agent`,
//! `swarm_reconfigure_local_agent` (Cybernetic Swarm Plan C6).
//!
//! Spend-mutating tools (`swarm_hire`, `swarm_delegate`, `swarm_delegate_and_wait`,
//! `swarm_fanout`, `swarm_create_swarm`, `swarm_xaman`) are consent-gated — see
//! `kask/docs/plans/abw-swarm-intelligence.md`
//! §3.6. Workspace update has NO ABW endpoint (405, verified live) and must
//! not be added. Workspace delete IS implemented as `swarm_delete_swarm` via
//! the team-scoped `DELETE /api/teams/{id}` (verified live 2026-08-02);
//! `DELETE /api/workspaces/{id}` is 405. Workspace create (`POST /api/teams`)
//! is verified; the create-path response shapes are pinned in §0.
//!
//! ## v2 Local mode (§15)
//! `SwarmConfig.mode` selects between `Abw` (v1, default) and `Local`
//! (v2). In `Local` mode, the server reads agent cards from a local
//! directory (`agents/local/curated/`) via `LocalAgentRegistry` and will
//! (Slice 9) execute them through `hkask-inference` + `hkask-ledger`.
//! No ABW calls are made in `Local` mode.

use hkask_mcp_server::server::CredentialRequirement;

mod a2a;
mod a2a_http;
mod a2a_tools;
mod abw_client;
mod abw_util;
mod agent_executor;
mod cloud;
mod cloud_tools;
mod config;
mod consent;
mod error;
mod knowledge_tools;
mod ledger_tools;
mod local_knowledge;
mod local_registry;
mod local_runtime;
mod local_swarms;
mod local_tools;
pub mod request_types;
mod sanitize;
mod spend_gate;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils {
    pub use crate::abw_util::*;
    pub use crate::cloud_tools::build_create_agent_card;
    pub use crate::config::{SwarmConfig, SwarmMode, resolve_local_agents_dir};
    pub use crate::consent::{ConsentStore, fnv1a, mint_token};
    pub use crate::request_types::{
        CapabilityGate, CreateAgentRequest, McpServerAuthSpec, McpServerSpec, ModelLadderRung,
        ValenceInput,
    };
    pub use crate::sanitize::*;
}

// ── Public local-swarm surface (reused by other kask MCP servers) ──────────
//
// The local-swarm runtime, agent registry, card types, and error type are
// reused by `hkask-mcp-kata-kanban`'s `kanban_task_spawn` to delegate tasks to
// local agents in-process. Only the local-mode execution surface is exposed;
// the ABW client, consent store, and spend gate stay crate-private.
pub use crate::error::SwarmError;
pub use crate::local_registry::{
    LocalAgentCapabilities, LocalAgentCard, LocalAgentDependencies, LocalAgentRegistry,
};
pub use crate::local_runtime::{
    LazyLocalSwarmRuntime, LocalDelegateResult, LocalSwarmRuntime, TaskSuccessProvenance,
    TaskSuccessVerdict,
};
pub use crate::local_swarms::{LocalSwarm, LocalSwarmRegistry};

use crate::abw_client::SwarmClient;
use crate::config::SwarmConfig;
use crate::consent::ConsentStore;

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub(crate) struct SwarmServer {
        pub client: std::sync::Arc<SwarmClient>,
        pub consent: std::sync::Arc<ConsentStore>,
        pub local_registry: std::sync::Arc<LocalAgentRegistry>,
        pub local_runtime: std::sync::Arc<LazyLocalSwarmRuntime>,
        pub local_swarms: std::sync::Arc<LocalSwarmRegistry>,
        pub local_memory: std::sync::Arc<local_knowledge::LazyLocalMemory>,
    }
);

impl SwarmServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::cloud_router()
            + Self::ledger_router()
            + Self::local_router()
            + Self::a2a_router()
            + Self::knowledge_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for SwarmServer {}

// ── Entry point ────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolve the shared consent store path. `HKASK_SWARM_CONSENT_STORE`
/// overrides; the default is `mcp/swarm/consent.db`. Both swarm server
/// processes (governed `McpRuntime` and per-project `ContextServerStore`)
/// compute the same path, which is what makes consent tokens consumable
/// across processes.
fn resolve_consent_store_path() -> String {
    std::env::var("HKASK_SWARM_CONSENT_STORE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            // D28 — Standardized Artifact Storage. Consent store lives at
            // `mcp/swarm/consent.db`.
            hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::agent_paths::mcp_server_db("swarm", "consent"),
            )
            .to_string_lossy()
            .to_string()
        })
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-swarm",
        SERVER_VERSION,
        |ctx| {
            let api_key = ctx.credentials.get("HKASK_ABW_API_KEY").cloned();
            let (config, warning) = SwarmConfig::from_env(api_key);
            // Catalogue-only mode is degraded, not broken — surface it so an
            // operator reading logs can distinguish "not configured" from
            // "configured but broken" (the startup-failure-signal rule).
            if let Some(w) = warning {
                tracing::warn!(target: "hkask.mcp.swarm", "{w}");
            }
            // Load local agent cards (v2 §15). In Abw mode this is a no-op
            // if the directory doesn't exist — the registry stays empty and
            // local tools (Slice 9) will return zero agents. In Local mode
            // the startup warning above already covers the missing-dir case.
            let local_registry =
                std::sync::Arc::new(LocalAgentRegistry::new(config.local_agents_dir.clone()));
            match local_registry.load() {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!(
                            target: "hkask.mcp.swarm",
                            dir = %config.local_agents_dir,
                            count,
                            "loaded local agent cards"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        "failed to load local agent cards: {e}"
                    );
                }
            }

            // Construct the local swarm runtime (ledger + inference).
            // This is always constructed — even in Abw mode, the operator can
            // call `swarm_fund_local` / `swarm_delegate_local` to mix local
            // execution. The ledger path defaults to
            // `mcp/swarm/ledger.db` (operator-configurable via
            // `HKASK_SWARM_LEDGER_PATH`).
            //
            // The runtime is constructed lazily on first tool call (the
            // `run_server` factory closure is sync — it cannot `.await` the
            // inference port resolution). `LocalSwarmRuntime::lazy` stores
            // the config; `LocalSwarmRuntime::get_or_init` does the async
            // init on first use.
            let ledger_path = std::env::var("HKASK_SWARM_LEDGER_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    // D28 — Standardized Artifact Storage. Default ledger
                    // path is `{kask_data_dir}/mcp/swarm/ledger.db`.
                    hkask_types::agent_paths::resolve_under_data_dir(
                        &hkask_types::agent_paths::mcp_server_db("swarm", "ledger"),
                    )
                    .to_string_lossy()
                    .to_string()
                });
            let local_runtime = std::sync::Arc::new(LazyLocalSwarmRuntime::lazy(
                ledger_path,
                config.skills_dir.clone(),
            ));

            // Local swarm registry — the local replica of an ABW workspace
            // roster. A missing directory is not an error (created on first
            // `swarm_create_local_swarm`); an empty roster is the normal
            // initial state. Created lazily on first write.
            let local_swarms =
                std::sync::Arc::new(LocalSwarmRegistry::new(config.local_swarms_dir.clone()));
            if let Err(e) = local_swarms.load() {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    "local swarms load failed (continuing with empty roster): {e}"
                );
            }

            // Local swarm semantic memory — backs `swarm_search_knowledge_local`
            // (and seeds the generate tools). Lazily opened on first use. The
            // passphrase defaults to "allostery" (pre-release) so the tools work
            // out of the box; override via `HKASK_SWARM_MEMORY_PASSPHRASE`. If the
            // store cannot be opened (e.g., an existing DB was created under a
            // different passphrase), the search tool degrades to an empty result
            // and the generate tools proceed unseeded (memory is an enhancement,
            // not a dependency).
            let local_memory = std::sync::Arc::new(local_knowledge::LazyLocalMemory::lazy(
                config.memory_db_path.clone(),
                config.memory_passphrase.clone(),
                config.embedding_dim,
            ));

            // A2A HTTP gateway (opt-in via HKASK_A2A_HTTP_ENABLE). Exposes local
            // agents to external A2A clients over loopback JSON-RPC. Disabled by
            // default - it opens a loopback port. The startup-failure signals
            // below let an operator distinguish "disabled" from "enabled but
            // broken" (the .rules trap).
            if config.a2a_http_enabled {
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => match a2a_http::A2aHttpServer::start(
                        local_runtime.clone(),
                        local_registry.clone(),
                        handle,
                        config.max_credits_per_dispatch,
                    ) {
                        Ok(server) => tracing::info!(
                            target: "hkask.mcp.swarm",
                            port = server.port(),
                            "A2A HTTP gateway enabled on 127.0.0.1:{} (JSON-RPC over POST /)",
                            server.port()
                        ),
                        Err(e) => tracing::warn!(
                            target: "hkask.mcp.swarm",
                            error = %e,
                            "A2A HTTP gateway failed to start - external A2A clients cannot reach                              local agents. Check the loopback port binding."
                        ),
                    },
                    Err(e) => tracing::warn!(
                        target: "hkask.mcp.swarm",
                        error = %e,
                        "A2A HTTP gateway enabled (HKASK_A2A_HTTP_ENABLE=1) but no tokio runtime                          is available - gateway not started"
                    ),
                }
            } else {
                tracing::info!(
                    target: "hkask.mcp.swarm",
                    "A2A HTTP gateway disabled (set HKASK_A2A_HTTP_ENABLE=1 to expose local agents                      to external A2A clients)"
                );
            }

            // Build the consent store. Default: the shared SQLite store
            // (mcp/swarm/consent.db, operator-overridable via
            // `HKASK_SWARM_CONSENT_STORE`) so a token minted by the panel's
            // governed server process is consumable by the Steer curator's
            // per-project server process (both resolve the same path). On open
            // failure, degrade to the session-local in-memory store with a loud
            // error — same-process consent still works; cross-process flows
            // (panel confirm → Steer spend) do not.
            let consent_store = match ConsentStore::open_sqlite(&resolve_consent_store_path()) {
                Ok(store) => {
                    tracing::info!(
                        target: "hkask.mcp.swarm",
                        "consent store: shared SQLite (cross-process tokens enabled)"
                    );
                    store
                }
                Err(e) => {
                    tracing::error!(
                        target: "hkask.mcp.swarm",
                        error = %e,
                        "consent store unavailable — falling back to the session-local in-memory \
                         store; cross-process consent flows (panel confirm → Steer spend) will \
                         not work. Set HKASK_SWARM_CONSENT_STORE to a writable path."
                    );
                    ConsentStore::default()
                }
            };

            Ok(SwarmServer::new(
                ctx.webid,
                std::sync::Arc::new(SwarmClient::new(
                    reqwest::Client::builder()
                        .connect_timeout(std::time::Duration::from_secs(10))
                        .timeout(std::time::Duration::from_secs(60))
                        .build()
                        .unwrap_or_else(|_| reqwest::Client::new()),
                    config,
                )),
                std::sync::Arc::new(consent_store),
                local_registry,
                local_runtime,
                local_swarms,
                local_memory,
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_ABW_API_KEY",
            "Agent Bestiary World Pro API key (catalogue-only mode if absent)",
        )],
    )
    .await
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_surface_is_exactly_53_registered_tools() {
        let router = SwarmServer::combined_router();
        let mut names: Vec<String> = router
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        names.sort();
        let mut expected: Vec<String> = [
            // ABW (27).
            "swarm_list_agents",
            "swarm_get_swarm",
            "swarm_get_agent",
            "swarm_list_apps",
            "swarm_ontology_templates",
            "swarm_execute_agent",
            "swarm_hire_cost",
            "swarm_request_consent",
            "swarm_authorize_session",
            "swarm_hire",
            "swarm_delegate",
            "swarm_delegate_and_wait",
            "swarm_fanout",
            "swarm_run_status",
            "swarm_generate_prompt",
            "swarm_generate_ontology",
            "swarm_create_agent",
            "swarm_create_swarm",
            "swarm_xaman",
            "swarm_create_app",
            "swarm_fire",
            "swarm_delete_agent",
            "swarm_delete_swarm",
            "swarm_search_knowledge",
            "swarm_publish_checks",
            "swarm_publish_agent",
            "swarm_fork_agent",
            // Local (24).
            "swarm_fund_local",
            "swarm_balance_local",
            "swarm_local_history",
            "swarm_delegate_local",
            "swarm_fanout_local",
            "swarm_pipeline_local",
            "swarm_a2a_send",
            "swarm_a2a_card",
            "swarm_list_local_agents",
            "swarm_clone_to_local",
            "swarm_push_to_cloud",
            "swarm_remove_local",
            "swarm_create_local_agent",
            "swarm_reconfigure_local_agent",
            "swarm_create_local_swarm",
            "swarm_list_local_swarms",
            "swarm_get_local_swarm",
            "swarm_delete_local_swarm",
            "swarm_add_agent_local",
            "swarm_remove_agent_local",
            "swarm_search_knowledge_local",
            "swarm_generate_prompt_local",
            "swarm_generate_ontology_local",
            "swarm_ai_assist",
            "swarm_evaluate_local",
            "swarm_execute_plan_local",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        expected.sort();
        assert_eq!(
            names, expected,
            "registered tool surface drifted from the documented 53"
        );
    }
}

// D28 — pins the default ledger + consent DB paths.
#[test]
fn default_db_paths_follow_standardized_layout() {
    let ledger = hkask_types::agent_paths::mcp_server_db("swarm", "ledger");
    assert_eq!(
        ledger,
        std::path::PathBuf::from("mcp")
            .join("swarm")
            .join("ledger.db"),
        "swarm ledger path must follow mcp/swarm/ledger.db"
    );
    let consent = hkask_types::agent_paths::mcp_server_db("swarm", "consent");
    assert_eq!(
        consent,
        std::path::PathBuf::from("mcp")
            .join("swarm")
            .join("consent.db"),
        "swarm consent path must follow mcp/swarm/consent.db"
    );
}
