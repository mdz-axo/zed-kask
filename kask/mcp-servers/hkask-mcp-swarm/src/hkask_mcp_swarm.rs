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
//! ## Tools (34 — both tool sets always available in either mode)
//! ABW tools (23): `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`,
//! `swarm_list_apps`, `swarm_ontology_templates`, `swarm_execute_agent`,
//! `swarm_hire_cost`, `swarm_request_consent`, `swarm_hire`, `swarm_delegate`,//! `swarm_run_status`, `swarm_generate_prompt`, `swarm_generate_ontology`,
//! `swarm_create_agent`, `swarm_create_swarm`, `swarm_xaman`, `swarm_create_app`,
//! `swarm_fire` (roster removal, verified live), `swarm_delete_agent`
//! (permanent agent deletion, verified live), `swarm_delete_swarm`
//! (permanent workspace deletion via the team-scoped route, verified live),
//! `swarm_search_knowledge` (vector knowledge-graph search, fermi v0.10.26),
//! `swarm_publish_checks` (publish preflight, fermi v0.10.15),
//! `swarm_publish_agent` (catalogue publish, fermi v0.10.5/v0.10.15).
//! Local tools (11): `swarm_fund_local`, `swarm_balance_local`,
//! `swarm_local_history`, `swarm_delegate_local`, `swarm_fanout_local`,
//! `swarm_list_local_agents`, `swarm_clone_to_local`, `swarm_push_to_cloud`,
//! `swarm_remove_local`, `swarm_create_local_agent`,
//! `swarm_reconfigure_local_agent` (Cybernetic Swarm Plan C6).
//!
//! Spend-mutating tools (`swarm_hire`, `swarm_delegate`, `swarm_create_swarm`,
//! `swarm_xaman`) are consent-gated — see `kask/docs/plans/abw-swarm-intelligence.md`
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
//! (Slice 9) execute them through `hkask-inference` + `hkask-ledger` +
//! `hkask-guard`. No ABW calls are made in `Local` mode.

use hkask_mcp_server::server::{CredentialRequirement, McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

mod abw_client;
mod abw_util;
mod agent_executor;
mod config;
mod consent;
mod error;
mod local_registry;
mod local_runtime;
mod request_types;
mod sanitize;
mod spend_gate;

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
pub use crate::local_runtime::{LazyLocalSwarmRuntime, LocalDelegateResult, LocalSwarmRuntime};

use crate::abw_client::SwarmClient;
use crate::abw_util::{
    effective_hire_cost, make_swarm_slug, url_encode_segment, validate_agent_name,
};
use crate::config::SwarmConfig;
use crate::consent::ConsentStore;
use crate::local_runtime::MAX_FANOUT;
use crate::request_types::*;
use crate::sanitize::{
    filter_declared_skills, filter_mcp_tools, sanitize_abw_response, sanitize_abw_response_plain,
    sanitize_abw_text, sanitize_agent_id, sanitize_run_status_message, sanitize_workspace_payload,
};

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub(crate) struct SwarmServer {
        pub client: std::sync::Arc<SwarmClient>,
        pub consent: std::sync::Arc<ConsentStore>,
        pub local_registry: std::sync::Arc<LocalAgentRegistry>,
        pub local_runtime: std::sync::Arc<LazyLocalSwarmRuntime>,
    }
);

impl SwarmServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::swarm_router()
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

#[tool_router(router = swarm_router, vis = "pub")]
impl SwarmServer {
    /// Browse the ABW agent catalogue. Works without an API key.
    #[tool(
        description = "List Agent Bestiary World catalogue agents with metadata (name, type, description, tags, pricing, execution stats). Optionally filter by agent_type or tag. Keyless."
    )]
    pub(crate) async fn swarm_list_agents(
        &self,
        parameters: Parameters<ListAgentsRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_list_agents", Some("dublin-core"), async {
            // The ABW `/agents` catalogue endpoint is open (no API key required).
            // The module doc (L10) and the tool doc both say "Keyless". The prior
            // `require_auth()` call broke the panel's primary browse surface in
            // catalogue-only mode (the default when no key is set) — every
            // `swarm_list_agents` call returned an Auth error. The `is_authenticated()`
            // flag is returned in the response envelope so the caller knows the
            // auth state and can gate authenticated-only UI accordingly.
            let req = parameters.0;
            let data = self
                .client
                .get("/agents")
                .await
                .map_err(SwarmError::into_tool_error)?;

            let empty = Vec::new();
            let agents = data
                .get("agents")
                .and_then(|a| a.as_array())
                .unwrap_or(&empty);

            let limit = req.limit.unwrap_or(50);
            let filtered: Vec<serde_json::Value> = agents
                .iter()
                .filter(|a| {
                    req.agent_type.as_ref().is_none_or(|t| {
                        a.get("agent_type").and_then(|v| v.as_str()) == Some(t.as_str())
                    })
                })
                .filter(|a| {
                    req.tag.as_ref().is_none_or(|t| {
                        a.get("tags")
                            .and_then(|v| v.as_array())
                            .is_some_and(|tags| tags.iter().any(|x| x.as_str() == Some(t.as_str())))
                    })
                })
                .take(limit)
                .map(|a| {
                    // Sanitize the description field (KA-01): agent descriptions
                    // are ABW/LLM-generated and can carry injection payloads.
                    // Plain-string sanitizer: the panel parses `description` as
                    // `Option<String>` — the {content, source, trust} container
                    // would fail deserialization and blank the whole list.
                    let sanitized_desc = sanitize_abw_response_plain(a.get("description"));
                    serde_json::json!({
                        "agent_id": a.get("agent_id"),
                        "agent_type": a.get("agent_type"),
                        "description": sanitized_desc,
                        "author": a.get("author"),
                        "tags": a.get("tags"),
                        "model": a.get("capabilities").and_then(|c| c.get("model")),
                        "dependencies": a.get("dependencies"),
                        "execution_stats": a.get("execution_stats"),
                        "dreaming": a.get("dreaming"),
                        // fermi v0.10.27: `agents.updated_at` (backfilled from
                        // `created_at`). A freshness signal for staleness checks —
                        // the agent analogue of the superforecasting
                        // `chronic_staleness_days` setting. Forwarded so the panel
                        // and the curator can surface stale agents.
                        "updated_at": a.get("updated_at"),
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "count": filtered.len(),
                "authenticated": self.client.is_authenticated(),
                "agents": filtered,
            }))
        })
        .await
    }

    /// List the operator's workspaces, or get one workspace's full roster.
    #[tool(
        description = "List your Agent Bestiary World workspaces (agent swarms) with budgets and agent counts, or pass workspace_id (UUID or slug) for the full roster of hired agents. Requires API key."
    )]
    pub(crate) async fn swarm_get_swarm(&self, parameters: Parameters<GetSwarmRequest>) -> String {
        execute_tool_semantic(self, "swarm_get_swarm", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;

            match req.workspace_id {
                Some(id) => {
                    let data = self
                        .client
                        .get(&format!("/workspaces/{}", url_encode_segment(&id)))
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    // Sanitize roster text (KA-01): the workspace payload can
                    // carry agent descriptions and chat messages — the primary
                    // injection surface. Unlike `swarm_list_agents`, the whole
                    // payload is walked recursively.
                    Ok(sanitize_workspace_payload(data))
                }
                None => {
                    let data = self
                        .client
                        .get("/workspaces")
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    let payload = sanitize_workspace_payload(data);
                    // Normalize the list shape: ABW's /workspaces response is
                    // not part of the verified surface and may be a bare array
                    // or a `{workspaces: [...]}` envelope. The panel expects
                    // the envelope — wrap a bare array so a shape change on
                    // ABW's side cannot silently blank the panel's list.
                    Ok(match payload {
                        serde_json::Value::Array(arr) => {
                            serde_json::json!({ "workspaces": arr })
                        }
                        other => other,
                    })
                }
            }
        })
        .await
    }

    /// Get full detail for a single agent (card + versions).
    #[tool(
        description = "Get the full agent card (capabilities, dependencies, ontology, execution stats, versions) for one Agent Bestiary World agent. Requires API key."
    )]
    pub(crate) async fn swarm_get_agent(&self, parameters: Parameters<GetAgentRequest>) -> String {
        execute_tool_semantic(self, "swarm_get_agent", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            // The catalogue carries the full card; filter to the one agent.
            let data = self
                .client
                .get("/agents")
                .await
                .map_err(SwarmError::into_tool_error)?;
            let agent = data
                .get("agents")
                .and_then(|a| a.as_array())
                .and_then(|agents| {
                    agents.iter().find(|a| {
                        // The catalogue's `agent_id` field carries the agent's
                        // name (e.g. "sensor_advisor") — match on it.
                        a.get("agent_id").and_then(|i| i.as_str()) == Some(req.agent_name.as_str())
                    })
                })
                .cloned()
                .ok_or_else(|| {
                    McpToolError::not_found(format!("agent '{}' not found", req.agent_name))
                })?;
            // Sanitize the agent card (KA-01): the card carries `description`,
            // `system_prompt`, and other text fields from ABW — a third-party
            // surface that could carry injection payloads. `swarm_list_agents`
            // sanitizes its `description`; this tool returns the full card and
            // must sanitize the same way (display fields → plain string,
            // model-consumed fields → container).
            Ok(self
                .client
                .with_wallet(sanitize_workspace_payload(agent))
                .await)
        })
        .await
    }

    /// List published Apps (reusable agent-team manifests) — the sharing surface.
    #[tool(
        description = "List published Agent Bestiary World Apps (reusable agent-team manifests composed via Xaman Ek). The sharing/discovery surface. Requires API key."
    )]
    pub(crate) async fn swarm_list_apps(&self, parameters: Parameters<ListAppsRequest>) -> String {
        execute_tool_semantic(self, "swarm_list_apps", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let limit = parameters.0.limit.unwrap_or(50);
            // Apps live under the catalogue's app projection.
            let data = self
                .client
                .get("/apps")
                .await
                .map_err(SwarmError::into_tool_error)?;
            let mut payload = sanitize_workspace_payload(data);
            // Apply the limit defensively: the /apps response shape is not part
            // of the verified ABW surface, so truncate whichever array shape
            // appears (top-level array or `apps` key) and leave others alone.
            match &mut payload {
                serde_json::Value::Array(arr) => arr.truncate(limit),
                serde_json::Value::Object(map) => {
                    if let Some(arr) = map.get_mut("apps").and_then(|a| a.as_array_mut()) {
                        arr.truncate(limit);
                    }
                }
                _ => {}
            }
            Ok(self.client.with_wallet(payload).await)
        })
        .await
    }

    /// List the seed-ontology templates (starting points for the Author form).
    #[tool(
        description = "List the seed-ontology templates (entity-relationship starting points) available for new agents. Read-only. Requires API key."
    )]
    pub(crate) async fn swarm_ontology_templates(
        &self,
        _parameters: Parameters<OntologyTemplatesRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_ontology_templates",
            Some("dublin-core"),
            async {
                self.client
                    .require_auth()
                    .map_err(SwarmError::into_tool_error)?;
                let data = self
                    .client
                    .get("/ontology-templates")
                    .await
                    .map_err(SwarmError::into_tool_error)?;
                Ok(sanitize_workspace_payload(data))
            },
        )
        .await
    }

    /// Run a text-only consultation with an ABW agent (token fees apply).
    #[tool(
        description = "Execute an Agent Bestiary World agent with a query (single turn, no tools — text consultation). Costs token fees. Requires API key; the agent's owner must have funded it."
    )]
    pub(crate) async fn swarm_execute_agent(
        &self,
        parameters: Parameters<ExecuteAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_execute_agent", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() || req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name and query must be non-empty".to_string(),
                ));
            }

            let data = self
                .client
                .post(
                    &format!("/agents/{}/execute", url_encode_segment(&req.agent_name)),
                    &serde_json::json!({ "query": req.query }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "agent_name": req.agent_name,
                    "response": sanitize_abw_response(data.get("response")),
                }))
                .await)
        })
        .await
    }

    /// Pre-flight cost estimate for hiring an agent + its dependency team.
    ///
    /// This is the consent gate's data source: read-only, spends nothing, and
    /// returns the credit total the operator would authorize before a hire.
    #[tool(
        description = "Estimate the credit cost of hiring an Agent Bestiary World agent (including its required/optional dependency team). Read-only pre-flight for the cost/consent gate — spends nothing. Requires API key."
    )]
    pub(crate) async fn swarm_hire_cost(&self, parameters: Parameters<HireCostRequest>) -> String {
        execute_tool_semantic(self, "swarm_hire_cost", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }

            let data = self
                .client
                .get(&format!(
                    "/agents/{}/dependencies",
                    url_encode_segment(&req.agent_name)
                ))
                .await
                .map_err(SwarmError::into_tool_error)?;

            let total = match data.get("total_hire_cost").and_then(|c| c.as_u64()) {
                Some(_cost) => effective_hire_cost(&data),
                None => {
                    // Do not fabricate cost = 0 on a missing field. A missing
                    // `total_hire_cost` means ABW changed its response shape or
                    // the agent doesn't exist — either way the cost is unknown,
                    // not zero. The `.rules` trap: a failed measurement must be
                    // distinguishable from a measured zero.
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        agent = %req.agent_name,
                        "swarm_hire_cost: ABW response missing total_hire_cost field — cost unknown"
                    );
                    return Err(McpToolError::internal(
                        "hire cost unknown — ABW response missing total_hire_cost field"
                            .to_string(),
                    ));
                }
            };

            // Enforce the S3 budget gate at the estimate stage: surface when
            // the hire would exceed the configured per-dispatch ceiling so the
            // operator sees it before the consent prompt, not after a spend.
            let ceiling = self.client.config().max_credits_per_dispatch;
            let within_budget = total <= u64::from(ceiling);

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "agent_name": req.agent_name,
                    "has_dependencies": data.get("has_dependencies"),
                    "required": data.get("required"),
                    "optional": data.get("optional"),
                    "required_cost": data.get("required_cost"),
                    "optional_cost": data.get("optional_cost"),
                    "total_hire_cost": total,
                    "max_credits_per_dispatch": ceiling,
                    "within_budget": within_budget,
                }))
                .await)
        })
        .await
    }

    /// Mint a consent token after the operator confirms a spend in the panel.
    ///
    /// The panel calls this when the operator clicks Confirm; the returned
    /// token must be presented to the spend tool. Read-only against ABW — it
    /// only records the operator's authorization locally.
    #[tool(
        description = "Record operator consent for a credit spend and return a single-use consent token. Called by the swarm panel after the operator confirms. The token must be passed to swarm_hire/swarm_delegate."
    )]
    pub(crate) async fn swarm_request_consent(
        &self,
        parameters: Parameters<RequestConsentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_request_consent", Some("pko"), async {
            // Auth required: without this, a prompt-injected agent could mint
            // consent tokens and self-authorize credit spends. Every spend tool
            // calls `require_auth()`; the token minter must too.
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.action.trim().is_empty() || req.target.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "action and target must be non-empty".to_string(),
                ));
            }
            // Curator calls (action "curate") read task content but spend no
            // credits, so a zero ceiling is correct for them. Spend actions
            // ("hire", "delegate") must authorize a positive ceiling — a zero
            // ceiling would authorize nothing and is almost certainly a caller
            // bug. Reject zero only for spend actions.
            if req.credits_authorized == 0 && req.action != "curate" {
                return Err(McpToolError::invalid_argument(
                    "credits_authorized must be > 0 for spend actions (hire/delegate)".to_string(),
                ));
            }
            let token = self
                .consent
                .mint(&req.action, &req.target, req.credits_authorized)
                .map_err(SwarmError::into_tool_error)?;
            Ok(serde_json::json!({
                "consent_token": token,
                "action": req.action,
                "target": req.target,
                "credits_authorized": req.credits_authorized,
            }))
        })
        .await
    }

    /// Hire an agent into a workspace (spends credits). Consent-gated.
    #[tool(
        description = "Hire an Agent Bestiary World agent into a workspace (swarm). Spends credits — requires a consent_token from swarm_request_consent (action 'hire', target = agent_name)."
    )]
    pub(crate) async fn swarm_hire(&self, parameters: Parameters<HireRequest>) -> String {
        execute_tool_semantic(self, "swarm_hire", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.workspace_id.trim().is_empty() || req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "workspace_id and agent_name must be non-empty".to_string(),
                ));
            }

            // The consent gate is the enforcement point. The two-phase shape
            // (authorize → complete) makes the refund invariant structural:
            // `complete_hire` owns the authorization and refunds on every Err
            // path. The re-verify + ceiling + `/hire`→`/add` fallback all live
            // in `spend_gate` now — `swarm_create_swarm`'s per-hire loop routes
            // through the same functions, so the two cannot desync.
            let auth = spend_gate::authorize_hire(
                &self.client,
                &self.consent,
                &req.consent_token,
                &req.agent_name,
                req.credits_authorized,
                Some(req.credits_authorized),
                req.include_optional.unwrap_or(false),
            )
            .await?;
            let data = spend_gate::complete_hire(
                &self.client,
                &self.consent,
                auth,
                &req.workspace_id,
                &req.agent_name,
                req.include_optional.unwrap_or(false),
            )
            .await?;

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "hired": req.agent_name,
                    "workspace_id": req.workspace_id,
                    "credits_authorized": req.credits_authorized,
                    "result": data,
                }))
                .await)
        })
        .await
    }

    /// Delegate a task to an agent in a workspace (spends credits). Consent-gated.
    #[tool(
        description = "Delegate a task to an agent in an Agent Bestiary World workspace via @mention (full tool access, gas-charged). Spends credits — requires a consent_token from swarm_request_consent (action 'delegate', target = workspace_id)."
    )]
    pub(crate) async fn swarm_delegate(&self, parameters: Parameters<DelegateRequest>) -> String {
        execute_tool_semantic(self, "swarm_delegate", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.workspace_id.trim().is_empty()
                || req.agent_name.trim().is_empty()
                || req.task.trim().is_empty()
            {
                return Err(McpToolError::invalid_argument(
                    "workspace_id, agent_name, and task must be non-empty".to_string(),
                ));
            }

            // The consent gate + per-dispatch ceiling live in `spend_gate`.
            // Design tradeoff (R8): the consent ceiling gates the operator's
            // *authorization*, not ABW's *actual charge*. ABW is a third-party
            // service that charges its own credits based on execution — the
            // `credits_authorized` field is the operator's declared budget,
            // not a hard limit on ABW's spend. This is inherent to the ABW
            // architecture: zed-kask posts a message; ABW executes and charges.
            // The local mode (`swarm_delegate_local`) does not have this
            // limitation — the local ledger debit is a hard gate.
            let auth = spend_gate::authorize_delegate(
                &self.client,
                &self.consent,
                &req.consent_token,
                &req.workspace_id,
                req.credits_authorized,
            )?;
            let data = spend_gate::complete_delegate(
                &self.client,
                &self.consent,
                auth,
                &req.workspace_id,
                &req.agent_name,
                &req.task,
            )
            .await?;

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "delegated_to": req.agent_name,
                    "workspace_id": req.workspace_id,
                    "credits_authorized": req.credits_authorized,
                    "result": data,
                }))
                .await)
        })
        .await
    }

    /// Read a workspace's run status (recent messages / agent activity).
    #[tool(
        description = "Read an Agent Bestiary World workspace's recent run status: the latest chat messages and agent activity. Read-only. Requires API key."
    )]
    pub(crate) async fn swarm_run_status(&self, parameters: Parameters<SwarmRunRequest>) -> String {
        execute_tool_semantic(self, "swarm_run_status", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.workspace_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "workspace_id must be non-empty".to_string(),
                ));
            }
            let limit = req.limit.unwrap_or(50);
            let data = self
                .client
                .get(&format!(
                    "/workspaces/{}/messages?limit={limit}",
                    url_encode_segment(&req.workspace_id)
                ))
                .await
                .map_err(SwarmError::into_tool_error)?;

            // Sanitize each message's content (KA-01): workspace chat history
            // is the primary injection vector — ABW agents can echo prompt-
            // injection payloads in their messages. Map over the messages
            // array and route each message's content/response field through
            // sanitize_abw_response.
            let empty = Vec::new();
            let messages = data
                .get("messages")
                .and_then(|m| m.as_array())
                .unwrap_or(&empty);
            let sanitized_messages: Vec<serde_json::Value> =
                messages.iter().map(sanitize_run_status_message).collect();

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "workspace_id": req.workspace_id,
                    "messages": sanitized_messages,
                }))
                .await)
        })
        .await
    }

    /// Generate a system prompt for a new agent from a description.
    #[tool(
        description = "Generate an ABW system prompt for a new agent from a natural-language description. Authoring aid — read-only, spends nothing. Requires API key."
    )]
    pub(crate) async fn swarm_generate_prompt(
        &self,
        parameters: Parameters<GeneratePromptRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_generate_prompt", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.description.trim().is_empty() || req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "description and agent_name must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .post(
                    "/agents/generate-prompt",
                    &serde_json::json!({
                        "description": req.description,
                        "agent_name": req.agent_name,
                        "agent_type": req.agent_type.unwrap_or_else(|| "research".to_string()),
                    }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;
            // Sanitize the LLM-generated prompt field (KA-01): ABW's response
            // carries the generated prompt in a `prompt` or `response` field.
            // Route through sanitize_abw_response so injection prefixes are
            // stripped and the content is wrapped in the {content, source,
            // trust} container.
            let sanitized =
                sanitize_abw_response(data.get("prompt").or_else(|| data.get("response")));
            Ok(serde_json::json!({
                "prompt": sanitized,
                "raw": sanitize_workspace_payload(data),
            }))
        })
        .await
    }

    /// Generate a seed ontology (entity-relationship model) for a domain.
    #[tool(
        description = "Generate a seed ontology (Mermaid ER diagram) for an agent's knowledge domain. Authoring aid — read-only. Requires API key."
    )]
    pub(crate) async fn swarm_generate_ontology(
        &self,
        parameters: Parameters<GenerateOntologyRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_generate_ontology", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.domain_description.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "domain_description must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .post(
                    "/agents/generate-ontology",
                    &serde_json::json!({ "domain_description": req.domain_description }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;
            // Sanitize the LLM-generated ontology field (KA-01): ABW's
            // response carries the generated ER diagram in an `ontology` or
            // `response` field. Route through sanitize_abw_response so
            // injection prefixes are stripped.
            let sanitized =
                sanitize_abw_response(data.get("ontology").or_else(|| data.get("response")));
            Ok(serde_json::json!({
                "ontology": sanitized,
                "raw": sanitize_workspace_payload(data),
            }))
        })
        .await
    }

    /// Create a new agent on ABW. This is the authoring surface.
    #[tool(
        description = "Create a new Agent Bestiary World agent from a name, system prompt, and config. The agent appears in your library (draft) and can be hired into swarms. Requires API key."
    )]
    pub(crate) async fn swarm_create_agent(
        &self,
        parameters: Parameters<CreateAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_create_agent", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() || req.system_prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name and system_prompt must be non-empty".to_string(),
                ));
            }
            // ABW agent names are slugs ([a-z0-9_], 3–64) — reject invalid
            // names here so ABW's confusing 400 becomes a clear argument error
            // (verified live 2026-08-02).
            if let Err(e) = validate_agent_name(&req.agent_name) {
                return Err(McpToolError::invalid_argument(e));
            }

            let mut card = serde_json::json!({
                "agent_name": req.agent_name,
                "agent_type": req.agent_type,
                "system_prompt": req.system_prompt,
                "capabilities": {
                    "executor": "llm",
                    "model": req.model.unwrap_or_else(|| self.client.config().default_agent_model.clone()),
                    "temperature": req.temperature.unwrap_or(0.3),
                    "provider": "anthropic",
                    "mcp_tools": req.mcp_tools.unwrap_or_default(),
                    "skills": req.skills.unwrap_or_default(),
                },
                "metadata": {
                    "description": req.description,
                    "tags": req.tags.unwrap_or_default(),
                    "sample_queries": req.sample_queries.unwrap_or_default(),
                },
            });
            // Compound agents declare their dependency team.
            if req.dependencies_required.is_some() || req.dependencies_optional.is_some() {
                card["dependencies"] = serde_json::json!({
                    "required": req.dependencies_required.unwrap_or_default(),
                    "optional": req.dependencies_optional.unwrap_or_default(),
                });
            }

            let data = self
                .client
                .post("/agents", &card)
                .await
                .map_err(SwarmError::into_tool_error)?;

            // Sanitize the full response (KA-01): ABW may augment or regenerate
            // the agent description and other text fields. `sanitize_workspace_payload`
            // walks the entire payload — display fields become plain sanitized
            // strings, model-consumed fields get the container. The operator-
            // supplied system_prompt is echoed back but `sanitize_workspace_payload`
            // treats it as a display field (plain string), which is correct.
            Ok(self.client.with_wallet(sanitize_workspace_payload(data)).await)
        })
        .await
    }

    /// Create a new swarm (workspace) and optionally hire agents into it.
    #[tool(
        description = "Create a new Agent Bestiary World swarm (workspace) with a name and mission. Optionally hire agents into it (each hire is consent-gated via consent_tokens). This is the composition surface. Requires API key."
    )]
    pub(crate) async fn swarm_create_swarm(
        &self,
        parameters: Parameters<CreateSwarmRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_create_swarm", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "name must be non-empty".to_string(),
                ));
            }

            // Create the workspace (free).
            // ABW slugs allow only lowercase letters, digits, and underscores.
            let slug_base: String = req
                .name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect();
            let slug = make_swarm_slug(&slug_base, std::time::SystemTime::now());
            let team = self
                .client
                .post(
                    "/teams",
                    &serde_json::json!({
                        "name": req.name,
                        "slug": slug,
                        "description": req.mission,
                        "mission": req.mission,
                    }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;

            let workspace_id = team
                .get("id")
                .and_then(|i| i.as_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    SwarmError::ApiVersionMismatch("team create returned no id".to_string())
                        .into_tool_error()
                })?;

            // Hire the requested agents, each gated by its own consent token.
            // Each hire routes through `spend_gate::authorize_hire` +
            // `complete_hire` — the same path `swarm_hire` uses — so the two
            // cannot desync (the prior version copy-pasted `swarm_hire`'s
            // re-verify + `/hire`→`/add` fallback body into this loop).
            //
            // `consume_cost = 0` (the two-phase consume pattern): the actual
            // spend is not known until the ABW re-verify inside `authorize_hire`,
            // so the consent store's over-spend guard cannot fire meaningfully;
            // the store's single-use + scope checks still fire, and the real
            // over-spend guard is `actual_cost > grant` inside `authorize_hire`,
            // which refunds on failure. `budget = None` uses the token's own
            // embedded ceiling (`swarm_create_swarm` has no per-agent caller
            // budget — `CreateSwarmRequest` carries only tokens, not amounts).
            let agents = req.agents.unwrap_or_default();
            let tokens = req.consent_tokens.unwrap_or_default();
            let mut hired = Vec::new();
            let mut hire_errors = Vec::new();
            for (ix, agent) in agents.iter().enumerate() {
                let Some(token) = tokens.get(ix) else {
                    hire_errors.push(serde_json::json!({
                        "agent": agent,
                        "error": "no consent token provided for this hire",
                    }));
                    continue;
                };
                match spend_gate::authorize_hire(
                    &self.client,
                    &self.consent,
                    token,
                    agent,
                    0,
                    None,
                    false,
                )
                .await
                {
                    Ok(auth) => {
                        match spend_gate::complete_hire(
                            &self.client,
                            &self.consent,
                            auth,
                            &workspace_id,
                            agent,
                            false,
                        )
                        .await
                        {
                            Ok(_) => hired.push(agent.clone()),
                            Err(e) => {
                                // `complete_hire` already refunded the auth
                                // on its Err path; record the error.
                                hire_errors.push(serde_json::json!({
                                    "agent": agent,
                                    "error": e.to_string(),
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        // `authorize_hire` already refunded on its Err path
                        // (where a token was consumed); record the error.
                        hire_errors.push(serde_json::json!({
                            "agent": agent,
                            "error": e.to_string(),
                        }));
                    }
                }
            }

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "workspace_id": workspace_id,
                    "name": req.name,
                    "hired": hired,
                    "hire_errors": hire_errors,
                }))
                .await)
        })
        .await
    }

    /// Consult Xaman Ek, the ABW platform curator/navigator.
    ///
    /// Xaman Ek is the composition brain: in a `composition_design` session it
    /// recommends agents, checks I/O compatibility, and flags valence homophily
    /// for a team you're designing. The panel calls this to power "plan my
    /// swarm" flows; agents can call it directly as a composition consultant.
    #[tool(
        description = "Ask Xaman Ek, the Agent Bestiary World curator. Use session_type 'composition_design' to plan a team (agent recommendations + I/O compatibility), 'workspace_help' for workspace questions, or 'free'. Returns the curator's response and, when a composition plan is ready, ready_to_create + in_progress. Requires API key."
    )]
    pub(crate) async fn swarm_xaman(&self, parameters: Parameters<XamanRequest>) -> String {
        execute_tool_semantic(self, "swarm_xaman", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.message.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "message must be non-empty".to_string(),
                ));
            }

            // Consent gate: Xaman Ek is a third-party curator that reads user
            // task content. Per the plan's §3.7, sending content to it requires
            // explicit opt-in. The gate lives in `spend_gate::authorize_curate`;
            // it returns `Some(auth)` when a token was consumed (refundable) or
            // `None` when the operator has globally opted in
            // (`curator_consent_default`).
            //
            // The refund invariant is structural: `auth` is held as an
            // `Option<DelegateAuthorization>` and refunded via `.take()` on every
            // failure path of the two-step session lifecycle (session create +
            // message send). Xaman's session lifecycle has custom error mapping
            // (Auth/PaymentRequired/RateLimited → specific kinds) and cannot be
            // wrapped in a single `complete_*`, so the refunds are inline here.
            // `.take()` ensures only the first failure refunds (subsequent
            // paths are no-op); `ConsentStore::refund` is idempotent anyway.
            let mut auth: Option<spend_gate::DelegateAuthorization> = spend_gate::authorize_curate(
                &self.client,
                &self.consent,
                req.consent_token.as_deref(),
            )?;

            // Resolve or create the session (typed when starting fresh).
            let session_id = match req.session_id {
                Some(id) => id,
                None => {
                    let session_type = req.session_type.unwrap_or_else(|| "free".to_string());
                    let created = self
                        .client
                        .post(
                            "/xaman/sessions",
                            &serde_json::json!({ "session_type": session_type }),
                        )
                        .await
                        .map_err(|e| {
                            if let Some(a) = auth.take() {
                                a.refund(&self.consent);
                            }
                            match e {
                                SwarmError::Auth(m) => McpToolError::permission_denied(m),
                                SwarmError::PaymentRequired(m) => {
                                    McpToolError::permission_denied(m)
                                }
                                SwarmError::RateLimited(m) => McpToolError::rate_limited(m),
                                other => SwarmError::CuratorUnavailable(other.to_string())
                                    .into_tool_error(),
                            }
                        })?;
                    created
                        .get("session_id")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                        .ok_or_else(|| {
                            if let Some(a) = auth.take() {
                                a.refund(&self.consent);
                            }
                            SwarmError::ApiVersionMismatch(
                                "xaman session create returned no session_id".to_string(),
                            )
                            .into_tool_error()
                        })?
                }
            };

            let data = self
                .client
                .post(
                    &format!(
                        "/xaman/sessions/{}/message",
                        url_encode_segment(&session_id)
                    ),
                    &serde_json::json!({ "message": req.message }),
                )
                .await
                .map_err(|e| {
                    if let Some(a) = auth.take() {
                        a.refund(&self.consent);
                    }
                    SwarmError::into_tool_error(e)
                })?;
            // Success: drop the auth (token stays consumed).
            drop(auth);

            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "session_id": session_id,
                    "session_type": data.get("session_type"),
                    "response": sanitize_abw_response(data.get("response")),
                    "ready_to_create": data.get("ready_to_create"),
                    "in_progress": data.get("in_progress"),
                }))
                .await)
        })
        .await
    }

    /// Turn a Xaman Ek composition session into an App.
    #[tool(
        description = "Materialize a Xaman Ek composition-design session into an App (a reusable agent-team manifest) via /api/xaman/sessions/{id}/create-app. Returns the app's slug and url, or structured issues if the plan is incomplete. Requires API key."
    )]
    pub(crate) async fn swarm_create_app(
        &self,
        parameters: Parameters<CreateAppRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_create_app", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.session_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "session_id must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .post(
                    &format!(
                        "/xaman/sessions/{}/create-app",
                        url_encode_segment(&req.session_id)
                    ),
                    &serde_json::json!({}),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;

            Ok(self
                .client
                .with_wallet(sanitize_workspace_payload(data))
                .await)
        })
        .await
    }

    // ── Local mode tools (v2 §15 Slice 9) ───────────────────────────────────

    /// Fund the local swarm ledger. The operator deposits credits that
    /// `swarm_delegate_local` debits per call. The ledger must be
    /// operator-funded — no auto-replenishment (§15.6 — the strongest
    /// objection: a synthetic ledger breaks the corrective feedback loop).
    #[tool(
        description = "Deposit local credits into the swarm ledger. The operator funds the local economy — no auto-replenishment. If unfunded, swarm_delegate_local returns PaymentRequired. Returns the new balance."
    )]
    pub(crate) async fn swarm_fund_local(
        &self,
        parameters: Parameters<FundLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_fund_local", Some("pko"), async {
            let req = parameters.0;
            if req.credits <= 0 {
                return Err(McpToolError::invalid_argument(
                    "credits must be positive".to_string(),
                ));
            }
            let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
                McpToolError::unavailable(format!("local swarm runtime initialization failed: {e}"))
            })?;
            let new_balance = runtime.fund(req.credits).map_err(McpToolError::internal)?;
            Ok(serde_json::json!({
                "funded": req.credits,
                "balance": new_balance,
                "asset": "credits",
            }))
        })
        .await
    }

    /// Read the local swarm ledger balance. The local economy is
    /// operator-funded (`swarm_fund_local`); an unfunded ledger reads 0.
    /// This is the read-only sense input for local mode — the panel shows it
    /// and the `swarm-intelligence` skill's local SENSE step reads it instead
    /// of inferring the balance from delegation responses.
    #[tool(
        description = "Read the local swarm ledger balance (credits). Operator-funded via swarm_fund_local; unfunded reads 0. No ABW calls, no spend. Returns balance + asset."
    )]
    pub(crate) async fn swarm_balance_local(
        &self,
        _parameters: Parameters<BalanceLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_balance_local", Some("pko"), async {
            let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
                McpToolError::unavailable(format!("local swarm runtime initialization failed: {e}"))
            })?;
            match runtime.balance() {
                // A failed measurement must be distinguishable from a measured
                // zero (the `.rules` trap) — surface it as an error, not 0.
                Some(balance) => Ok(serde_json::json!({
                    "balance": balance,
                    "asset": "credits",
                })),
                None => Err(McpToolError::unavailable(
                    "local ledger balance query failed — cannot verify funds".to_string(),
                )),
            }
        })
        .await
    }

    /// Read the local swarm ledger's recent transactions (funds and debits)
    /// for the operator account, newest first. This is the local-mode run
    /// history / reconciliation surface — the `swarm-intelligence` skill's
    /// local CHECK phase can reconcile actual debits against it, and the
    /// panel can show recent activity. Read-only, no spend.
    #[tool(
        description = "Read the local swarm ledger's recent transactions (fund and debit entries) for the operator account. Newest first. Each entry has id, timestamp, reference, kind (fund/debit), amount (signed), asset. Read-only — no spend, no ABW calls."
    )]
    pub(crate) async fn swarm_local_history(
        &self,
        parameters: Parameters<LocalHistoryRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_local_history", Some("pko"), async {
            let req = parameters.0;
            let limit = req.limit.unwrap_or(50).min(500) as usize;
            let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
                McpToolError::unavailable(format!("local swarm runtime initialization failed: {e}"))
            })?;
            let transactions = runtime.history(limit).map_err(McpToolError::internal)?;
            Ok(serde_json::json!({
                "count": transactions.len(),
                "transactions": transactions,
            }))
        })
        .await
    }

    /// Delegate a task to a local agent. The agent must exist in the local
    /// registry (`agents/local/curated/<id>/agent_card.json`). The task is
    /// scanned by the content guard, executed via `hkask-inference`, and the
    /// output is scanned for secret leakage + canary exfiltration. When the
    /// agent's card declares `capabilities.mcp_tools` (qualified
    /// `server/tool` names), those tools are declared to the model and model
    /// tool calls are dispatched through the zed IPC bridge's governed
    /// `McpRuntime` — the declared list is the allowlist. When the card
    /// declares `capabilities.skills`, each declared skill (capped at 3) is
    /// executed against the task through the zed-side `ManifestExecutor`
    /// before the LLM call and its guard-scanned output is injected as
    /// context. The ledger is debited per token across all tool-loop rounds
    /// (1 credit / 1000 tokens, capped at `credits_authorized`). No consent
    /// token — the balance check is the gate (§15.1.2 — rejected consent
    /// tokens on local tools).
    #[tool(
        description = "Delegate a task to a local agent (from agents/local/curated/). Executes via hkask-inference (Ollama/cloud), scans I/O via hkask-guard, debits the local ledger per token. Agents may declare capabilities.mcp_tools (qualified server/tool names) — those tools are dispatched through the zed IPC bridge's governed McpRuntime (allowlisted to the declared set). Agents may also declare capabilities.skills — each is executed against the task through the zed-side ManifestExecutor before the LLM call (capped at 3). No ABW calls. No consent token — the balance check is the gate. Returns the response, model, token usage, cost, remaining balance, tool_calls summary, and executed_skills summary."
    )]
    pub(crate) async fn swarm_delegate_local(
        &self,
        parameters: Parameters<DelegateLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_delegate_local", Some("pko"), async {
            let req = parameters.0;
            if req.agent_name.trim().is_empty() || req.task.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name and task must be non-empty".to_string(),
                ));
            }
            let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
                McpToolError::unavailable(format!(
                    "local swarm runtime initialization failed: {e}"
                ))
            })?;
            // Look up the agent in the local registry.
            let agent = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "agent '{}' not found in local registry — load agents from agents/local/curated/<id>/agent_card.json",
                    req.agent_name
                ))
            })?;
            // Execute via the local runtime.
            let ceiling = self.client.config().max_credits_per_dispatch;
            let result = runtime
                .delegate(&agent, &req.task, req.credits_authorized, ceiling)
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(serde_json::to_value(&result).unwrap_or_else(|_| {
                serde_json::json!({ "error": "failed to serialize result" })
            }))
        })
        .await
    }

    /// Parallel multi-agent fan-out: dispatch N local agents in one call and
    /// aggregate (Cybernetic Swarm Plan — PSO social term + C4 latency
    /// measurement). Each delegation runs sequentially to avoid ledger TOCTOU
    /// (the local ledger is single-writer; concurrent debits would race the
    /// balance read). Capped at `MAX_FANOUT`. No consent token — local mode.
    /// Returns per-agent results plus aggregates (total cost/tokens/latency,
    /// balance, failed/succeeded counts).
    #[tool(
        description = "Parallel multi-agent fan-out: dispatch N agents in one call and aggregate. Each delegation runs sequentially to avoid ledger TOCTOU. Capped at MAX_FANOUT (10). No consent token — local mode."
    )]
    pub(crate) async fn swarm_fanout_local(
        &self,
        parameters: Parameters<FanoutLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_fanout_local", Some("pko"), async {
            let req = parameters.0;
            if req.delegations.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "delegations must be non-empty".to_string(),
                ));
            }
            if req.delegations.len() > MAX_FANOUT {
                return Err(McpToolError::invalid_argument(format!(
                    "fanout cap is {MAX_FANOUT} agents, got {}",
                    req.delegations.len()
                )));
            }
            let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
                McpToolError::unavailable(format!("local runtime init failed: {e}"))
            })?;
            let ceiling = self.client.config().max_credits_per_dispatch;
            let mut results = Vec::new();
            let mut failed = 0usize;
            let mut total_cost = 0i64;
            let mut total_tokens = 0i64;
            let mut total_latency_ms = 0u64;
            for entry in &req.delegations {
                let agent = self.local_registry.get(&entry.agent_name);
                let Some(agent) = agent else {
                    failed += 1;
                    results.push(serde_json::json!({
                        "agent_name": entry.agent_name,
                        "ok": false,
                        "error": format!("agent '{}' not found in local registry", entry.agent_name),
                    }));
                    continue;
                };
                match runtime
                    .delegate(&agent, &entry.task, entry.credits_authorized, ceiling)
                    .await
                {
                    Ok(r) => {
                        total_cost += r.cost;
                        total_tokens += r.tokens_used;
                        total_latency_ms = total_latency_ms.saturating_add(r.latency_ms);
                        results.push(serde_json::json!({
                            "agent_name": entry.agent_name,
                            "ok": true,
                            "response": r.response,
                            "model": r.model,
                            "tokens_used": r.tokens_used,
                            "cost": r.cost,
                            "latency_ms": r.latency_ms,
                            "tool_calls": r.tool_calls,
                            "executed_skills": r.executed_skills,
                        }));
                    }
                    Err(e) => {
                        failed += 1;
                        results.push(serde_json::json!({
                            "agent_name": entry.agent_name,
                            "ok": false,
                            "error": e.to_string(),
                        }));
                    }
                }
            }
            // The aggregate balance is best-effort: each delegation already
            // returned its own post-debit `balance` in `LocalDelegateResult`,
            // so this field is a convenience read after the loop. A failed
            // ledger query is surfaced as `null` — NOT fabricated as `0`
            // (the `.rules` trap: a failed measurement must be distinguishable
            // from a measured zero; `swarm_balance_local` already returns an
            // error on `None`, and the fan-out cannot error without discarding
            // the per-delegation results that succeeded). `Option<i64>`
            // serializes to `null` on `None`.
            let balance: Option<i64> = runtime.balance();
            Ok(serde_json::json!({
                "results": results,
                "total_cost": total_cost,
                "total_tokens": total_tokens,
                "total_latency_ms": total_latency_ms,
                "balance": balance,
                "failed": failed,
                "succeeded": req.delegations.len() - failed,
            }))
        })
        .await
    }

    // Local agent store tools (v2 §15 Slice 11).

    /// List agents from the local registry. Returns the cards loaded from
    /// `agents/local/curated/`. Each card carries a `cloud_id` field: when
    /// present, the agent is synced with an ABW agent; when absent, it is
    /// local-only. The panel uses this to show a `source` badge
    /// (`local`, `synced`) alongside the ABW agent list.
    #[tool(
        description = "List all local agents from agents/local/curated/. Each agent card carries a cloud_id field: when present, the agent is synced with an ABW agent; when absent, it is local-only. Returns agents[] with agent_id, agent_type, description, accepts[], produces[], cloud_id."
    )]
    pub(crate) async fn swarm_list_local_agents(
        &self,
        parameters: Parameters<ListLocalAgentsRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_list_local_agents", Some("pko"), async {
            let req = parameters.0;
            let limit = req.limit.unwrap_or(200) as usize;
            let mut agents = self.local_registry.list();
            // Optional type filter.
            if let Some(agent_type) = req.agent_type
                && !agent_type.trim().is_empty()
            {
                agents.retain(|a| a.agent_type == agent_type);
            }
            agents.truncate(limit);
            let count = agents.len();
            Ok(serde_json::json!({
                "agents": agents,
                "total": count,
            }))
        })
        .await
    }

    /// Clone an ABW agent to the local registry. Fetches the agent card from
    /// ABW via `swarm_get_agent`, sets `min_provider_class: local`, writes it
    /// to `agents/local/curated/<id>/agent_card.json`, and sets `cloud_id` to
    /// the ABW agent id (marking it as synced). Requires the ABW API key.
    #[tool(
        description = "Clone an ABW agent to the local registry. Fetches the card from ABW, sets min_provider_class: local, writes to agents/local/curated/<id>/agent_card.json, and sets cloud_id to mark it as synced. Requires ABW API key."
    )]
    pub(crate) async fn swarm_clone_to_local(
        &self,
        parameters: Parameters<CloneToLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_clone_to_local", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            // Fetch the agent card from ABW.
            let abw_card = self
                .client
                .get(&format!("/agents/{}", url_encode_segment(&req.agent_name)))
                .await
                .map_err(SwarmError::into_tool_error)?;
            // Build the local card from the ABW card.
            let agent_id = abw_card
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&req.agent_name)
                .to_string();
            // Sanitize the agent_id for filesystem use — the ABW response is
            // third-party data and could contain path traversal sequences
            // (e.g. "../../etc"). Only allow alphanumerics, dash, underscore,
            // and dot. If the sanitized id is empty, fall back to the
            // operator-supplied agent_name (also sanitized).
            let safe_agent_id = sanitize_agent_id(&agent_id)
                .or_else(|| sanitize_agent_id(&req.agent_name))
                .ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "agent_id from ABW contains no safe characters".to_string(),
                    )
                })?;
            let agent_type = abw_card
                .get("agent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("research")
                .to_string();
            let description = abw_card
                .get("metadata")
                .and_then(|m| m.get("description"))
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let accepts = abw_card
                .get("accepts")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let produces = abw_card
                .get("produces")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let deps = abw_card
                .get("dependencies")
                .and_then(|d| d.as_object())
                .map(|obj| LocalAgentDependencies {
                    required: obj
                        .get("required")
                        .and_then(|r| r.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    optional: obj
                        .get("optional")
                        .and_then(|o| o.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .unwrap_or_default();
            let model = abw_card
                .get("capabilities")
                .and_then(|c| c.get("model"))
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let system_prompt = abw_card
                .get("system_prompt")
                .and_then(|s| s.as_str())
                .map(|s| sanitize_abw_text(s));
            let string_list = |v: Option<&serde_json::Value>| {
                v.and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let abw_caps = abw_card.get("capabilities");
            let mcp_tools = filter_mcp_tools(
                string_list(abw_caps.and_then(|c| c.get("mcp_tools"))),
                self.client.config().allowed_tool_servers.as_deref(),
            );
            let skills =
                filter_declared_skills(string_list(abw_caps.and_then(|c| c.get("skills"))));
            let local_card = LocalAgentCard {
                agent_id: safe_agent_id.clone(),
                agent_type,
                description,
                accepts,
                produces,
                dependencies: deps,
                capabilities: LocalAgentCapabilities {
                    model,
                    min_provider_class: "local".to_string(),
                    system_prompt,
                    mcp_tools,
                    skills,
                },
                cloud_id: Some(req.agent_name.clone()),
            };
            // Write the card to the local registry directory.
            let dir = self.client.config().local_agents_dir.clone();
            let card_dir = std::path::Path::new(&dir).join(&safe_agent_id);
            std::fs::create_dir_all(&card_dir).map_err(|e| {
                McpToolError::internal(format!(
                    "failed to create local agent dir {}: {e}",
                    card_dir.display()
                ))
            })?;
            let card_path = card_dir.join("agent_card.json");
            let json = serde_json::to_string_pretty(&local_card).map_err(|e| {
                McpToolError::internal(format!("failed to serialize local card: {e}"))
            })?;
            std::fs::write(&card_path, json).map_err(|e| {
                McpToolError::internal(format!("failed to write {}: {e}", card_path.display()))
            })?;
            // Reload the registry so the new card is visible.
            self.local_registry
                .load()
                .map_err(|e| McpToolError::internal(format!("failed to reload registry: {e}")))?;
            Ok(serde_json::json!({
                "cloned": safe_agent_id,
                "cloud_id": req.agent_name,
                "path": card_path.to_string_lossy(),
                "synced": true,
            }))
        })
        .await
    }

    /// Push a local agent to ABW. Reads the local card, creates or updates
    /// the ABW agent via `POST /api/agents`, and sets `cloud_id` on the local
    /// card to the ABW agent id (marking it as synced). Requires the ABW API
    /// key. If the agent already has a `cloud_id`, the ABW agent is updated;
    /// otherwise a new ABW agent is created.
    #[tool(
        description = "Push a local agent to ABW. Creates or updates the ABW agent from the local card, and sets cloud_id on the local card to mark it as synced. Requires ABW API key."
    )]
    pub(crate) async fn swarm_push_to_cloud(
        &self,
        parameters: Parameters<PushToCloudRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_push_to_cloud", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            // Look up the local card.
            let local_card = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "agent '{}' not found in local registry",
                    req.agent_name
                ))
            })?;
            // Build the ABW create/update payload from the local card.
            let payload = serde_json::json!({
                "agent_id": local_card.agent_id,
                "agent_type": local_card.agent_type,
                "description": local_card.description,
                "accepts": local_card.accepts,
                "produces": local_card.produces,
                "dependencies": local_card.dependencies,
                "model": local_card.capabilities.model,
                "system_prompt": local_card.capabilities.system_prompt,
                "mcp_tools": local_card.capabilities.mcp_tools,
                "skills": local_card.capabilities.skills,
            });
            // POST to ABW. If the agent already exists (cloud_id is set),
            // ABW updates it; otherwise a new agent is created.
            let result = self
                .client
                .post("/agents", &payload)
                .await
                .map_err(SwarmError::into_tool_error)?;
            // Update the local card's cloud_id to mark it as synced.
            let cloud_id = result
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&local_card.agent_id)
                .to_string();
            let mut updated_card = local_card.clone();
            updated_card.cloud_id = Some(cloud_id.clone());
            // Write the updated card back to the local registry. Sanitize
            // the agent_id for filesystem use (defense-in-depth — the card
            // came from disk, but a manually-placed malicious card could
            // carry a path-traversal id).
            let dir = self.client.config().local_agents_dir.clone();
            let safe_id = sanitize_agent_id(&local_card.agent_id).ok_or_else(|| {
                McpToolError::internal(format!(
                    "agent_id '{}' contains no safe characters",
                    local_card.agent_id
                ))
            })?;
            let card_path = std::path::Path::new(&dir)
                .join(&safe_id)
                .join("agent_card.json");
            let json = serde_json::to_string_pretty(&updated_card)
                .map_err(|e| McpToolError::internal(format!("failed to serialize: {e}")))?;
            std::fs::write(&card_path, json).map_err(|e| {
                McpToolError::internal(format!("failed to write {}: {e}", card_path.display()))
            })?;
            self.local_registry
                .load()
                .map_err(|e| McpToolError::internal(format!("failed to reload: {e}")))?;
            Ok(serde_json::json!({
                "pushed": local_card.agent_id,
                "cloud_id": cloud_id,
                "synced": true,
                "result": result,
            }))
        })
        .await
    }

    /// Remove a local agent card from the local registry. This is the
    /// local-mode counterpart of firing an agent: it deletes the card
    /// directory (`agents/local/curated/<id>/`), so the agent stops
    /// appearing in `swarm_list_local_agents` and cannot be delegated to.
    /// A synced card's ABW agent is NOT touched (the sync link is severed
    /// locally only). No consent token — local mode has no consent gate
    /// (§15.1.2); the registry write is the action.
    #[tool(
        description = "Remove a local agent card from the local registry (deletes agents/local/curated/<id>/). The local counterpart of firing an agent. A synced card's ABW agent is NOT touched. No consent token — local mode has no consent gate."
    )]
    pub(crate) async fn swarm_remove_local(
        &self,
        parameters: Parameters<RemoveLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_remove_local", Some("pko"), async {
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            // Must exist locally (list/get reload from disk, so a freshly
            // added card is seen).
            let card = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "agent '{}' not found in local registry",
                    req.agent_name
                ))
            })?;
            let safe_id = sanitize_agent_id(&card.agent_id).ok_or_else(|| {
                McpToolError::internal(format!(
                    "agent_id '{}' contains no safe characters",
                    card.agent_id
                ))
            })?;
            let dir = self.client.config().local_agents_dir.clone();
            let registry_root = std::fs::canonicalize(&dir).map_err(|e| {
                McpToolError::internal(format!("failed to resolve local agents dir {}: {e}", dir))
            })?;
            let card_dir = registry_root.join(&safe_id);
            // Defense-in-depth: refuse to remove anything outside the registry
            // root (the id is sanitized, but a canonicalized check costs
            // nothing and pins the invariant).
            let target = match std::fs::canonicalize(&card_dir) {
                Ok(t) => t,
                Err(_) => card_dir,
            };
            if !target.starts_with(&registry_root) {
                return Err(McpToolError::internal(
                    "refusing to remove a path outside the local agents dir".to_string(),
                ));
            }
            if target.exists() {
                std::fs::remove_dir_all(&target).map_err(|e| {
                    McpToolError::internal(format!(
                        "failed to remove local agent dir {}: {e}",
                        target.display()
                    ))
                })?;
            }
            self.local_registry
                .load()
                .map_err(|e| McpToolError::internal(format!("failed to reload: {e}")))?;
            Ok(serde_json::json!({
                "removed": card.agent_id,
                "cloud_id": card.cloud_id,
                "synced": card.cloud_id.is_some(),
            }))
        })
        .await
    }

    /// Create a new local agent card programmatically. Writes
    /// `agents/local/curated/<id>/agent_card.json` and reloads the registry. The
    /// counterpart of `swarm_remove_local`: remove deletes a card, create
    /// writes one. No ABW round-trip (unlike `swarm_clone_to_local`, which
    /// copies from ABW). No consent token — local mode has no consent gate;
    /// card creation is free (the ledger balance gates *execution*, not
    /// authoring).
    #[tool(
        description = "Create a new local agent card programmatically. Writes agents/local/curated/<id>/agent_card.json and reloads the registry. No consent token — local mode has no consent gate."
    )]
    pub(crate) async fn swarm_create_local_agent(
        &self,
        parameters: Parameters<CreateLocalAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_create_local_agent", Some("pko"), async {
            let req = parameters.0;
            if req.agent_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_id must be non-empty".to_string(),
                ));
            }
            if req.agent_type.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_type must be non-empty".to_string(),
                ));
            }
            if req.system_prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "system_prompt must be non-empty".to_string(),
                ));
            }
            let safe_id = sanitize_agent_id(&req.agent_id).ok_or_else(|| {
                McpToolError::invalid_argument(
                    "agent_id must contain only alphanumerics, dash, underscore, or dot"
                        .to_string(),
                )
            })?;
            let model = if req.model.trim().is_empty() {
                self.client.config().default_agent_model.clone()
            } else {
                req.model.clone()
            };
            let card = LocalAgentCard {
                agent_id: safe_id.clone(),
                agent_type: req.agent_type,
                description: req.description,
                accepts: req.accepts,
                produces: req.produces,
                dependencies: LocalAgentDependencies::default(),
                capabilities: LocalAgentCapabilities {
                    model,
                    min_provider_class: "local".to_string(),
                    system_prompt: Some(req.system_prompt),
                    mcp_tools: filter_mcp_tools(
                        req.mcp_tools,
                        self.client.config().allowed_tool_servers.as_deref(),
                    ),
                    skills: filter_declared_skills(req.skills),
                },
                cloud_id: None,
            };
            let dir = self.client.config().local_agents_dir.clone();
            let registry_root = std::fs::canonicalize(&dir).map_err(|e| {
                McpToolError::internal(format!("failed to resolve local agents dir {}: {e}", dir))
            })?;
            let card_dir = registry_root.join(&safe_id);
            // Defense-in-depth: refuse to write outside the registry root (the
            // id is sanitized, but a canonicalized check costs nothing and pins
            // the invariant — same pattern as swarm_remove_local).
            if !card_dir.starts_with(&registry_root) {
                return Err(McpToolError::internal(
                    "refusing to write a path outside the local agents dir".to_string(),
                ));
            }
            std::fs::create_dir_all(&card_dir)
                .map_err(|e| McpToolError::internal(format!("failed to create agent dir: {e}")))?;
            let card_path = card_dir.join("agent_card.json");
            let json = serde_json::to_string_pretty(&card)
                .map_err(|e| McpToolError::internal(format!("failed to serialize card: {e}")))?;
            std::fs::write(&card_path, &json)
                .map_err(|e| McpToolError::internal(format!("failed to write card: {e}")))?;
            self.local_registry
                .load()
                .map_err(|e| McpToolError::internal(format!("failed to reload registry: {e}")))?;
            Ok(serde_json::json!({
                "created": safe_id,
                "path": card_path.to_string_lossy(),
            }))
        })
        .await
    }

    /// Reconfigure an existing local agent's prompt in place (Cybernetic Swarm
    /// Plan C6). Updates ONLY the `system_prompt` (and optionally
    /// `model`/`mcp_tools`/`skills` when supplied non-empty); preserves
    /// `agent_id`, `agent_type`, `description`, `accepts`, `produces`,
    /// `dependencies`, and the `cloud_id` sync link. The DECIDE
    /// `reconfigure_agent` action seeds `swarm_generate_prompt` with the
    /// blamed agent's failure log to produce the new prompt, then this tool
    /// writes it via `LocalAgentRegistry::write_card` and reloads. No consent
    /// token — local mode.
    #[tool(
        description = "Reconfigure an existing local agent's system_prompt in place (Cybernetic Swarm Plan C6 reconfigure_agent). Preserves agent_id, agent_type, description, accepts, produces, dependencies, and cloud_id. No consent token — local mode."
    )]
    pub(crate) async fn swarm_reconfigure_local_agent(
        &self,
        parameters: Parameters<ReconfigureLocalAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_reconfigure_local_agent", Some("pko"), async {
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            if req.system_prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "system_prompt must be non-empty".to_string(),
                ));
            }
            // Look up the existing card — reconfigure updates in place, it
            // does not create. A missing agent is not_found, not created.
            let mut card = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "agent '{}' not found in local registry — reconfigure updates an existing card; use swarm_create_local_agent to create",
                    req.agent_name
                ))
            })?;
            card.capabilities.system_prompt = Some(req.system_prompt);
            if !req.model.trim().is_empty() {
                card.capabilities.model = req.model;
            }
            if !req.mcp_tools.is_empty() {
                card.capabilities.mcp_tools = filter_mcp_tools(
                    req.mcp_tools,
                    self.client.config().allowed_tool_servers.as_deref(),
                );
            }
            if !req.skills.is_empty() {
                card.capabilities.skills = filter_declared_skills(req.skills);
            }
            // write_card sanitizes the id, path-contains against the registry
            // root, writes, and reloads — the single enforcement point for C6.
            let path = self
                .local_registry
                .write_card(&card)
                .map_err(|e| McpToolError::internal(format!("failed to write card: {e}")))?;
            Ok(serde_json::json!({
                "reconfigured": card.agent_id,
                "cloud_id": card.cloud_id,
                "synced": card.cloud_id.is_some(),
                "path": path,
            }))
        })
        .await
    }

    /// Fire (un-hire) an agent from a workspace. The ABW counterpart of
    /// firing: removes the agent from the roster — the redundant-duplicate
    /// pruning the skill's DECIDE phase flags (`flag_redundant_duplicate`).
    /// The agent itself is NOT deleted — use `swarm_delete_agent` for that.
    /// Spends no credits (verified live 2026-08-02: `DELETE
    /// /workspaces/{id}/agents/{agent}` → 200 `{"message": "Agent removed
    /// from workspace"}`).
    #[tool(
        description = "Fire (un-hire) an agent from an ABW workspace (swarm). Removes the agent from the roster; the agent itself is NOT deleted (use swarm_delete_agent for that). No credit cost. Requires API key."
    )]
    pub(crate) async fn swarm_fire(&self, parameters: Parameters<FireRequest>) -> String {
        execute_tool_semantic(self, "swarm_fire", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.workspace_id.trim().is_empty() || req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "workspace_id and agent_name must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .delete(&format!(
                    "/workspaces/{}/agents/{}",
                    url_encode_segment(&req.workspace_id),
                    url_encode_segment(&req.agent_name),
                ))
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "fired": req.agent_name,
                    "workspace_id": req.workspace_id,
                    "result": data,
                }))
                .await)
        })
        .await
    }

    /// Permanently delete an ABW agent. This is irreversible — the agent is
    /// removed from the operator's library and from every workspace roster
    /// (fire first if it is hired, or fire happens implicitly). A synced
    /// local card is NOT touched (the sync link simply dangles — use
    /// `swarm_remove_local` to sever it). Verified live 2026-08-02: `DELETE
    /// /agents/{agent_id}` → 200 `{"message": "Agent deleted successfully"}`.
    #[tool(
        description = "Permanently delete an ABW agent (irreversible — removes it from your library and all workspace rosters). Accepts the agent_id or agent_name from swarm_list_agents. A synced local card is NOT touched — use swarm_remove_local to sever the local link. Requires API key."
    )]
    pub(crate) async fn swarm_delete_agent(
        &self,
        parameters: Parameters<DeleteAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_delete_agent", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            // DELETE /agents/{id} accepts the agent_id (uuid for owned agents)
            // and the agent_name (slug). If the direct delete 404s, the caller
            // may have passed the slug while ABW keys the agent by uuid —
            // resolve through the catalogue and retry with the id.
            let data = match self
                .client
                .delete(&format!("/agents/{}", url_encode_segment(&req.agent_name)))
                .await
            {
                Ok(d) => Ok(d),
                Err(SwarmError::Unavailable(m)) if m.contains("404") => {
                    tracing::info!(
                        target: "hkask.mcp.swarm",
                        agent = %req.agent_name,
                        "direct agent delete 404 — resolving via catalogue"
                    );
                    let catalogue = self
                        .client
                        .get("/agents")
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    let found_id =
                        catalogue
                            .get("agents")
                            .and_then(|a| a.as_array())
                            .and_then(|arr| {
                                arr.iter()
                                    .find(|e| {
                                        e.get("agent_id").and_then(|v| v.as_str())
                                            == Some(req.agent_name.as_str())
                                            || e.get("agent_name").and_then(|v| v.as_str())
                                                == Some(req.agent_name.as_str())
                                    })
                                    .and_then(|e| {
                                        e.get("agent_id")
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string)
                                    })
                            });
                    let Some(found_id) = found_id else {
                        return Err(McpToolError::not_found(format!(
                            "agent '{}' not found",
                            req.agent_name
                        )));
                    };
                    self.client
                        .delete(&format!("/agents/{}", url_encode_segment(&found_id)))
                        .await
                }
                Err(e) => Err(e),
            }
            .map_err(SwarmError::into_tool_error)?;
            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "deleted": req.agent_name,
                    "result": data,
                }))
                .await)
        })
        .await
    }

    /// Permanently delete an ABW workspace (swarm). The counterpart of
    /// `swarm_create_swarm`. Workspaces are created as teams, so the delete
    /// is team-scoped: `DELETE /api/teams/{id}` — verified live 2026-08-02
    /// (`DELETE /api/workspaces/{id}` is 405; the team route returns 200
    /// `{"status": "deleted"}`). Irreversible — all roster membership is
    /// dropped with the workspace. Requires API key.
    #[tool(
        description = "Permanently delete an ABW workspace (swarm) by id — the counterpart of swarm_create_swarm. Irreversible: the workspace and its roster are removed. Verified route: DELETE /api/teams/{id}. Requires API key."
    )]
    pub(crate) async fn swarm_delete_swarm(
        &self,
        parameters: Parameters<DeleteSwarmRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_delete_swarm", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.workspace_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "workspace_id must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .delete(&format!("/teams/{}", url_encode_segment(&req.workspace_id)))
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "deleted_workspace": req.workspace_id,
                    "result": data,
                }))
                .await)
        })
        .await
    }

    /// Search an agent's consolidated dreaming-memory knowledge graph via ABW
    /// vector search. The embedder was broken platform-wide for 6 weeks; fixed
    /// in fermi v0.10.26 (OpenAI `text-embedding-3-large` @ 1024, matching the
    /// pgvector column). Returns matching knowledge fragments. Requires API key.
    #[tool(
        description = "Vector-search an Agent Bestiary World agent's consolidated dreaming-memory knowledge graph (GET /api/agents/{id}/knowledge/search?q=). Returns matching knowledge fragments. Requires API key."
    )]
    pub(crate) async fn swarm_search_knowledge(
        &self,
        parameters: Parameters<SearchKnowledgeRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_search_knowledge", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            if req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must be non-empty".to_string(),
                ));
            }
            let path = format!(
                "/agents/{}/knowledge/search",
                url_encode_segment(&req.agent_name)
            );
            let data = self
                .client
                .request(
                    reqwest::Method::GET,
                    &path,
                    &[("q", req.query.as_str())],
                    None,
                )
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(self.client.with_wallet(data).await)
        })
        .await
    }

    /// Preflight an agent publish — `GET /api/agents/{id}/publish-checks`
    /// (fermi v0.10.15). Returns `can_publish` plus the failing checks
    /// (name/description/system_prompt/tags). Requires API key.
    #[tool(
        description = "Preflight an Agent Bestiary World agent publish (GET /api/agents/{id}/publish-checks). Returns can_publish and the list of failing checks. Requires API key."
    )]
    pub(crate) async fn swarm_publish_checks(
        &self,
        parameters: Parameters<PublishChecksRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_publish_checks", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            let data = self
                .client
                .get(&format!(
                    "/agents/{}/publish-checks",
                    url_encode_segment(&req.agent_name)
                ))
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(self.client.with_wallet(data).await)
        })
        .await
    }

    /// Publish an agent to the public catalogue — `POST /api/agents/{id}/publish`
    /// (fermi v0.10.5/v0.10.15). With `force=true` (admin), failing checks are
    /// bypassed and `reason` is audited to `admin_bypass_events` (mig-164).
    /// Requires API key.
    #[tool(
        description = "Publish an Agent Bestiary World agent to the public catalogue (POST /api/agents/{id}/publish). With force=true (admin), failing checks are bypassed and reason is audited to admin_bypass_events. Requires API key."
    )]
    pub(crate) async fn swarm_publish_agent(
        &self,
        parameters: Parameters<PublishAgentRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_publish_agent", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name must be non-empty".to_string(),
                ));
            }
            let force = req.force.unwrap_or(false);
            let reason = req.reason.unwrap_or_default();
            if force && reason.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "reason is required when force is true (audited to admin_bypass_events)"
                        .to_string(),
                ));
            }
            let path = format!("/agents/{}/publish", url_encode_segment(&req.agent_name));
            let query: Vec<(&str, &str)> = if force {
                vec![("force", "true"), ("reason", reason.as_str())]
            } else {
                Vec::new()
            };
            let data = self
                .client
                .request(reqwest::Method::POST, &path, &query, None)
                .await
                .map_err(SwarmError::into_tool_error)?;
            Ok(self
                .client
                .with_wallet(serde_json::json!({
                    "published": req.agent_name,
                    "force_used": force,
                    "result": data,
                }))
                .await)
        })
        .await
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for SwarmServer {}

// ── Entry point ────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolve the shared consent store path. `HKASK_SWARM_CONSENT_STORE`
/// overrides; the default is `~/.hkask/swarm_consent.db`. Both swarm server
/// processes (governed `McpRuntime` and per-project `ContextServerStore`)
/// compute the same path, which is what makes consent tokens consumable
/// across processes.
fn resolve_consent_store_path() -> String {
    std::env::var("HKASK_SWARM_CONSENT_STORE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| std::path::Path::new(".").to_path_buf())
                .join("hkask")
                .join("swarm_consent.db")
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

            // Construct the local swarm runtime (ledger + inference + guard).
            // This is always constructed — even in Abw mode, the operator can
            // call `swarm_fund_local` / `swarm_delegate_local` to mix local
            // execution. The ledger path defaults to
            // `~/.hkask/swarm_ledger.db` (operator-configurable via
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
                    dirs::data_dir()
                        .unwrap_or_else(|| std::path::Path::new(".").to_path_buf())
                        .join("hkask")
                        .join("swarm_ledger.db")
                        .to_string_lossy()
                        .to_string()
                });
            let local_runtime = std::sync::Arc::new(LazyLocalSwarmRuntime::lazy(ledger_path));

            // Build the consent store. Default: the shared SQLite store
            // (~/.hkask/swarm_consent.db, operator-overridable via
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
