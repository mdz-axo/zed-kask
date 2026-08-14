//! ABW (Agent Bestiary World) cloud tools — catalogue, workspaces, agents,
//! Xaman Ek curator, hire/delegate/fanout spend tools, lifecycle (fire/delete),
//! knowledge search, publish, fork. Split from `hkask_mcp_swarm.rs` (M2).
//!
//! All 27 tools here talk to the ABW REST API (`agent-bestiary.world`); none
//! touch the local registry or local ledger.
use crate::SwarmServer;
use crate::abw_util::{
    effective_hire_cost, make_swarm_slug, url_encode_segment, validate_agent_name,
};
use crate::cloud;
use crate::error::SwarmError;
use crate::request_types::*;
use crate::sanitize::{
    sanitize_abw_response, sanitize_abw_response_plain, sanitize_run_status_message,
    sanitize_workspace_payload,
};
use crate::spend_gate;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

// Re-export the pure helpers from `cloud` so the `test_utils` module and
// any internal callers can reach them at the crate root.
pub use crate::cloud::{build_create_agent_card, extract_execute_response};

#[tool_router(router = cloud_router, vis = "pub")]
impl SwarmServer {
    /// Browse the ABW agent catalogue. Works without an API key.
    #[tool(
        description = "List Agent Bestiary World catalogue agents with metadata (name, type, description, tags, pricing, execution stats). Optionally filter by agent_type or tag. Keyless."
    )]
    pub(crate) async fn swarm_list_agents(
        &self,
        parameters: Parameters<ListAgentsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_list_agents",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                                .is_some_and(|tags| {
                                    tags.iter().any(|x| x.as_str() == Some(t.as_str()))
                                })
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
                            "uuid": a.get("uuid"),
                            "agent_type": a.get("agent_type"),
                            "tier": a.get("tier"),
                            "status": a.get("status"),
                            "description": sanitized_desc,
                            "author": a.get("author"),
                            "tags": a.get("tags"),
                            "model": a.get("capabilities").and_then(|c| c.get("model")),
                            "llm_provider": a.get("llm_provider"),
                            // Composition signals (fermi `build_agent_json`):
                            // `accepts`/`produces` drive I/O compatibility checks,
                            // `valence` drives homophily scoring, `min_tier` gates
                            // cognition tier, `requires_secrets` enables funding-gate
                            // pre-check (avoids hire-then-fail on unfunded agents).
                            "min_tier": a.get("min_tier"),
                            "accepts": a.get("accepts"),
                            "produces": a.get("produces"),
                            "valence": a.get("valence"),
                            "requires_secrets": a.get("requires_secrets"),
                            "execution_stats": a.get("execution_stats"),
                            "dreaming": a.get("dreaming"),
                            "workspace_count": a.get("workspace_count"),
                            // Not emitted by fermi's list endpoint — fetch via
                            // `swarm_hire_cost` (`GET /agents/{id}/dependencies`).
                            // Forwarded as null for schema stability.
                            "dependencies": a.get("dependencies"),
                            // `agents.updated_at` exists in fermi's DB (mig-166) but
                            // `build_agent_json` does not expose it in the list
                            // response. Forwarded as null for schema stability;
                            // re-enable when fermi adds it.
                            "updated_at": a.get("updated_at"),
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "count": filtered.len(),
                    "authenticated": self.client.is_authenticated(),
                    "agents": filtered,
                }))
            },
        )
        .await
    }

    /// List the operator's workspaces, or get one workspace's full roster.
    #[tool(
        description = "List your Agent Bestiary World workspaces (agent swarms) with budgets and agent counts, or pass workspace_id (UUID or slug) for the full roster of hired agents. Requires API key."
    )]
    pub(crate) async fn swarm_get_swarm(&self, parameters: Parameters<GetSwarmRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_get_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
        .await
    }

    /// Get full detail for a single agent (card + versions).
    #[tool(
        description = "Get the full agent card (capabilities, dependencies, ontology, execution stats, versions) for one Agent Bestiary World agent. Requires API key."
    )]
    pub(crate) async fn swarm_get_agent(&self, parameters: Parameters<GetAgentRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_get_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                            a.get("agent_id").and_then(|i| i.as_str())
                                == Some(req.agent_name.as_str())
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
            },
        )
        .await
    }

    /// List published Apps (reusable agent-team manifests) — the sharing surface.
    #[tool(
        description = "List published Agent Bestiary World Apps (reusable agent-team manifests composed via Xaman Ek). The sharing/discovery surface. Requires API key."
    )]
    pub(crate) async fn swarm_list_apps(&self, parameters: Parameters<ListAppsRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_list_apps",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
            Some(hkask_bridge_ontology::pko::PROCEDURE),
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
        execute_tool_semantic(
            self,
            "swarm_execute_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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

                // Fermi's execute handler returns the agent's output in
                // `metadata.reasoning` and `evidence[]`, not a top-level
                // `response` field. `extract_execute_response` handles the
                // current shape, the evidence fallback, and the legacy
                // `response` field for older deploys.
                let response_text = extract_execute_response(&data);
                let response_value = response_text
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                Ok(self
                    .client
                    .with_wallet(serde_json::json!({
                        "agent_name": req.agent_name,
                        "response": sanitize_abw_response(Some(&response_value)),
                        // Forward the structured fields fermi emits so the
                        // caller sees status, cost, and confidence alongside
                        // the narrative.
                        "status": data.get("status"),
                        "confidence": data.get("confidence"),
                        "episode_id": data.get("episode_id"),
                        "credits_charged": data.get("credits_charged"),
                    }))
                    .await)
            },
        )
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
        execute_tool_semantic(self, "swarm_hire_cost", Some(hkask_bridge_ontology::pko::PROCEDURE), async {
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
                    return Err(McpToolError::unavailable(
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
        execute_tool_semantic(
            self,
            "swarm_request_consent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                        "credits_authorized must be > 0 for spend actions (hire/delegate)"
                            .to_string(),
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
            },
        )
        .await
    }

    /// Open a pre-authorized spend session for headless ABW pipelines. Returns
    /// a session token that can be used in place of per-spend consent tokens.
    /// Each spend deducts from the total; when exhausted, a new session is
    /// needed.
    #[tool(
        description = "Open a pre-authorized spend session for headless ABW pipelines. Returns a session token usable in place of per-spend consent tokens for swarm_hire, swarm_delegate, and swarm_fanout. Each spend deducts from total_credits. The per-dispatch ceiling still gates individual spends."
    )]
    pub(crate) async fn swarm_authorize_session(
        &self,
        parameters: Parameters<AuthorizeSessionRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_authorize_session",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.total_credits == 0 {
                    return Err(McpToolError::invalid_argument(
                        "total_credits must be positive".to_string(),
                    ));
                }
                let token = self
                    .consent
                    .open_session(req.total_credits, &req.actions)
                    .map_err(SwarmError::into_tool_error)?;
                Ok(serde_json::json!({
                    "session_token": token,
                    "total_credits": req.total_credits,
                    "remaining_credits": req.total_credits,
                    "actions": if req.actions.is_empty() {
                        vec!["hire".to_string(), "delegate".to_string()]
                    } else {
                        req.actions
                    },
                }))
            },
        )
        .await
    }

    /// Hire an agent into a workspace (spends credits). Consent-gated.
    #[tool(
        description = "Hire an Agent Bestiary World agent into a workspace (swarm). Spends credits — requires a consent_token from swarm_request_consent (action 'hire', target = agent_name)."
    )]
    pub(crate) async fn swarm_hire(&self, parameters: Parameters<HireRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_hire",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                let include_optional = req.include_optional.unwrap_or(false);
                let auth = spend_gate::authorize_hire(
                    &self.client,
                    &self.consent,
                    spend_gate::resolve_auth(
                        req.consent_token.as_deref(),
                        req.session_token.as_deref(),
                    )?,
                    &req.agent_name,
                    req.credits_authorized,
                    Some(req.credits_authorized),
                    include_optional,
                )
                .await?;
                let data = spend_gate::complete_hire(
                    &self.client,
                    &self.consent,
                    auth,
                    &req.workspace_id,
                    &req.agent_name,
                    include_optional,
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
            },
        )
        .await
    }

    /// Delegate a task to an agent in a workspace (spends credits). Consent-gated.
    #[tool(
        description = "Delegate a task to an agent in an Agent Bestiary World workspace via @mention (full tool access, gas-charged). Spends credits — requires a consent_token from swarm_request_consent (action 'delegate', target = workspace_id)."
    )]
    pub(crate) async fn swarm_delegate(&self, parameters: Parameters<DelegateRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_delegate",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                // // Local mode has no equivalent gate at all: its ledger records spend
                // rather than authorizing it, so neither path hard-caps ABW's charge.
                let auth = spend_gate::authorize_delegate(
                    &self.client,
                    &self.consent,
                    spend_gate::resolve_auth(
                        req.consent_token.as_deref(),
                        req.session_token.as_deref(),
                    )?,
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
            },
        )
        .await
    }

    /// Delegate a task to an ABW agent and poll `swarm_run_status` until the
    /// agent responds or the timeout is reached. Wraps `swarm_delegate` +
    /// polling.
    #[tool(
        description = "Delegate a task to an ABW agent and poll for the response. Posts the @mention via swarm_delegate, then polls swarm_run_status every 2 seconds until the agent responds or timeout_secs (default 60, max 300). Returns the agent's response message or a timeout."
    )]
    pub(crate) async fn swarm_delegate_and_wait(
        &self,
        parameters: Parameters<DelegateAndWaitRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_delegate_and_wait",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                let timeout_secs = req.timeout_secs.unwrap_or(60).min(300);
                // Step 1: post the @mention via the spend gate. A session token
                // (from `swarm_authorize_session`) may be used in place of a
                // single-use consent token — the gate handles both.
                let auth = spend_gate::authorize_delegate(
                    &self.client,
                    &self.consent,
                    spend_gate::resolve_auth(
                        req.consent_token.as_deref(),
                        req.session_token.as_deref(),
                    )?,
                    &req.workspace_id,
                    req.credits_authorized,
                )?;
                let post_result = spend_gate::complete_delegate(
                    &self.client,
                    &self.consent,
                    auth,
                    &req.workspace_id,
                    &req.agent_name,
                    &req.task,
                )
                .await?;
                // Record the post timestamp for filtering messages.
                let post_time = chrono::Utc::now();
                // Step 2: poll for the agent's response.
                let poll_interval = std::time::Duration::from_secs(2);
                let deadline = post_time + chrono::Duration::seconds(timeout_secs as i64);
                let mut agent_response: Option<serde_json::Value> = None;
                let mut poll_count = 0u32;
                while chrono::Utc::now() < deadline {
                    poll_count += 1;
                    tokio::time::sleep(poll_interval).await;
                    let data = self
                        .client
                        .get(&format!(
                            "/workspaces/{}/messages?limit=10",
                            url_encode_segment(&req.workspace_id)
                        ))
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    let empty = Vec::new();
                    let messages = data
                        .get("messages")
                        .and_then(|m| m.as_array())
                        .unwrap_or(&empty);
                    // Look for a message from the delegated agent after the post
                    // time. Iterate in reverse so the latest matching message wins.
                    for msg in messages.iter().rev() {
                        let sender = msg
                            .get("sender")
                            .or_else(|| msg.get("agent_name"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("");
                        let msg_time_str = msg
                            .get("created_at")
                            .or_else(|| msg.get("timestamp"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        let msg_time = chrono::DateTime::parse_from_rfc3339(msg_time_str)
                            .ok()
                            .map(|d| d.with_timezone(&chrono::Utc))
                            .unwrap_or(chrono::Utc::now());
                        if sender == req.agent_name && msg_time > post_time {
                            let content = sanitize_abw_response(
                                msg.get("content").or_else(|| msg.get("response")),
                            );
                            agent_response = Some(serde_json::json!({
                                "content": content,
                                "created_at": msg_time_str,
                            }));
                            break;
                        }
                    }
                    if agent_response.is_some() {
                        break;
                    }
                }
                let timed_out = agent_response.is_none();
                Ok(self
                    .client
                    .with_wallet(serde_json::json!({
                        "delegated_to": req.agent_name,
                        "workspace_id": req.workspace_id,
                        "post_result": post_result,
                        "agent_response": agent_response,
                        "timed_out": timed_out,
                        "poll_count": poll_count,
                    }))
                    .await)
            },
        )
        .await
    }

    /// Read a workspace's run status (recent messages / agent activity).
    #[tool(
        description = "Read an Agent Bestiary World workspace's recent run status: the latest chat messages and agent activity. Read-only. Requires API key."
    )]
    pub(crate) async fn swarm_run_status(&self, parameters: Parameters<SwarmRunRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_run_status",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_generate_prompt",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_generate_ontology",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_create_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                    return Err(crate::error::map_local_swarm_error(e));
                }

                let card = build_create_agent_card(&req, &self.client.config().default_agent_model);

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
                Ok(self
                    .client
                    .with_wallet(sanitize_workspace_payload(data))
                    .await)
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_create_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                let session_token = req.session_token.as_deref().filter(|s| !s.is_empty());
                // Exactly one auth source: a single session token funds all hires,
                // or one consent token per agent. Both is ambiguous; neither is
                // caught per-hire below (the existing "no consent token" path).
                if session_token.is_some() && !tokens.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "provide either consent_tokens or session_token, not both".to_string(),
                    ));
                }
                let mut hired = Vec::new();
                let mut hire_errors = Vec::new();
                for (ix, agent) in agents.iter().enumerate() {
                    // One auth source per hire: a shared session token, or a
                    // per-agent single-use consent token. The gate handles either.
                    let spend_auth = match session_token {
                        Some(st) => spend_gate::SpendAuth::Session(st),
                        None => match tokens.get(ix) {
                            Some(token) => spend_gate::SpendAuth::SingleUse(token.as_str()),
                            None => {
                                hire_errors.push(serde_json::json!({
                                    "agent": agent,
                                    "error": "no consent token provided for this hire",
                                }));
                                continue;
                            }
                        },
                    };
                    match spend_gate::authorize_hire(
                        &self.client,
                        &self.consent,
                        spend_auth,
                        agent,
                        0,
                        None,
                        false,
                    )
                    .await
                    {
                        Ok(auth) => match spend_gate::complete_hire(
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
                            Err(e) => hire_errors.push(serde_json::json!({
                                "agent": agent,
                                "error": e.to_string(),
                            })),
                        },
                        Err(e) => hire_errors.push(serde_json::json!({
                            "agent": agent,
                            "error": e.to_string(),
                        })),
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_xaman",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                // explicit opt-in. The gate lives in `cloud::curator::authorize`
                // (wraps `spend_gate::authorize_curate`); it returns `Some(auth)`
                // when a token was consumed (refundable) or `None` when the
                // operator has globally opted in (`curator_consent_default`).
                //
                // The refund invariant is structural: `CuratorSession` owns the
                // `Option<DelegateAuthorization>` and refunds it on `Drop` unless
                // `send` succeeds (which calls `disarm` internally). The prior
                // inline ladder had four `auth.take().refund()` sites; the guard
                // removes that footgun — a new failure path cannot forget the
                // refund because `Drop` covers it.
                let auth = cloud::curator::authorize(
                    &self.client,
                    &self.consent,
                    req.consent_token.as_deref(),
                )?;

                // Resolve or create the session. `CuratorSession::create` refunds
                // the auth on construction failure; `resume` carries it for the
                // send step.
                let mut session = match req.session_id {
                    Some(id) => cloud::curator::CuratorSession::resume(
                        &self.client,
                        &self.consent,
                        auth,
                        id,
                    ),
                    None => {
                        let session_type = req.session_type.unwrap_or_else(|| "free".to_string());
                        cloud::curator::CuratorSession::create(
                            &self.client,
                            &self.consent,
                            auth,
                            &session_type,
                        )
                        .await?
                    }
                };

                let data = session.send(&req.message).await?;
                let session_id = session.session_id().to_string();
                // `session` drops here; `send` already disarmed it on success, so
                // `Drop` is a no-op (the auth stays consumed).

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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_create_app",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
        .await
    }

    /// Fan-out: delegate to multiple agents in an ABW workspace in one call.
    /// Each delegation is a separate @mention post, each gated by its own
    /// consent token. ABW delegation is fire-and-forget — the tool posts all
    /// messages and returns per-agent status. Responses arrive via
    /// `swarm_run_status` polling. Capped at MAX_FANOUT (10).
    #[tool(
        description = "Parallel multi-agent fan-out to an ABW workspace: post N @mention delegations in one call. Each entry needs its own consent token. ABW is fire-and-forget — responses arrive via swarm_run_status. Capped at 10 agents."
    )]
    pub(crate) async fn swarm_fanout(&self, parameters: Parameters<FanoutRequest>) -> String {
        execute_tool_semantic(
            self,
            "swarm_fanout",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                self.client
                    .require_auth()
                    .map_err(SwarmError::into_tool_error)?;
                let req = parameters.0;
                if req.workspace_id.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "workspace_id must be non-empty".to_string(),
                    ));
                }
                if req.delegations.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "delegations must be non-empty".to_string(),
                    ));
                }
                const MAX_FANOUT_ABW: usize = 10;
                if req.delegations.len() > MAX_FANOUT_ABW {
                    return Err(McpToolError::invalid_argument(format!(
                        "fanout cap is {MAX_FANOUT_ABW} agents, got {}",
                        req.delegations.len()
                    )));
                }
                let mut results = Vec::new();
                let mut failed = 0usize;
                for entry in &req.delegations {
                    if entry.agent_name.trim().is_empty() || entry.task.trim().is_empty() {
                        failed += 1;
                        results.push(serde_json::json!({
                            "agent_name": entry.agent_name,
                            "ok": false,
                            "error": "agent_name and task must be non-empty",
                        }));
                        continue;
                    }
                    // Each delegation routes through the spend gate. A session
                    // token (from `swarm_authorize_session`) may be used in place
                    // of a single-use consent token; the gate handles both.
                    let delegated: Result<serde_json::Value, McpToolError> = async {
                        let auth = spend_gate::authorize_delegate(
                            &self.client,
                            &self.consent,
                            spend_gate::resolve_auth(
                                entry.consent_token.as_deref(),
                                entry.session_token.as_deref(),
                            )?,
                            &req.workspace_id,
                            entry.credits_authorized,
                        )?;
                        spend_gate::complete_delegate(
                            &self.client,
                            &self.consent,
                            auth,
                            &req.workspace_id,
                            &entry.agent_name,
                            &entry.task,
                        )
                        .await
                    }
                    .await;
                    match delegated {
                        Ok(data) => {
                            results.push(serde_json::json!({
                                "agent_name": entry.agent_name,
                                "ok": true,
                                "result": data,
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
                Ok(self
                    .client
                    .with_wallet(serde_json::json!({
                        "workspace_id": req.workspace_id,
                        "results": results,
                        "failed": failed,
                        "succeeded": req.delegations.len() - failed,
                    }))
                    .await)
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_fire",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_delete_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_delete_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
        .await
    }

    /// Search an agent's consolidated dreaming-memory knowledge graph.
    /// Returns matching knowledge fragments (semantic rules + entities)
    /// whose text matches the query.
    ///
    /// fermi does not expose a vector-search HTTP endpoint — the
    /// `MemoryStore` has `search_similar_episodes` and semantic
    /// entity/rule search (pgvector `<=>` cosine distance), but they
    /// are not wired to routes. The actual knowledge graph HTTP
    /// endpoints are `GET /api/agents/{id}/kg/{rules,entities,facts}`
    /// (list + filter, no text search). This tool fetches the rules
    /// and entities and does client-side case-insensitive text
    /// matching against the query — the closest approximation of
    /// "search the knowledge graph" available against fermi's actual
    /// API. When fermi adds a vector-search route, this tool should
    /// switch to it.
    ///
    /// fermi-contract: the embedder fix (v0.10.26, commit `03edd0d6`)
    /// is still load-bearing — without it, consolidation never runs,
    /// the rules/entities tables stay empty, and this tool returns
    /// zero matches regardless of the query. The live probe
    /// (`live_search_knowledge_returns_results_post_v0_10_26`)
    /// is the canary.
    #[tool(
        description = "Search an Agent Bestiary World agent's consolidated dreaming-memory knowledge graph for fragments matching a query. Returns matching semantic rules and entities. Requires API key."
    )]
    pub(crate) async fn swarm_search_knowledge(
        &self,
        parameters: Parameters<SearchKnowledgeRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_search_knowledge",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                let agent_segment = url_encode_segment(&req.agent_name);
                let query_lower = req.query.to_lowercase();
                let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

                // Fetch semantic rules — the consolidated knowledge fragments
                // produced by the dreaming/consolidation loop. These are the
                // closest thing to "knowledge fragments" in fermi's KG.
                let rules_path = format!("/agents/{agent_segment}/kg/rules");
                let rules_data = self
                    .client
                    .request(
                        reqwest::Method::GET,
                        &rules_path,
                        &[("active_only", "true")],
                        None,
                    )
                    .await
                    .map_err(SwarmError::into_tool_error)?;

                // Fetch entities — the named nodes in the knowledge graph.
                let entities_path = format!("/agents/{agent_segment}/kg/entities");
                let entities_data = self
                    .client
                    .request(reqwest::Method::GET, &entities_path, &[], None)
                    .await
                    .map_err(SwarmError::into_tool_error)?;

                // Client-side text matching: a fragment matches if any query
                // term appears in its text fields (case-insensitive substring).
                // This is a fallback for the missing server-side vector search;
                // it is not semantic, but it surfaces relevant fragments.
                let matches_any = |text: &str| {
                    let text_lower = text.to_lowercase();
                    query_terms.iter().any(|term| text_lower.contains(term))
                };

                let mut matching_rules: Vec<serde_json::Value> = Vec::new();
                if let Some(rules) = rules_data.get("rules").and_then(|r| r.as_array()) {
                    for rule in rules {
                        let content = rule
                            .get("rule_content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let description = rule
                            .get("rule_description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if matches_any(content) || matches_any(description) {
                            matching_rules.push(rule.clone());
                        }
                    }
                }

                let mut matching_entities: Vec<serde_json::Value> = Vec::new();
                if let Some(entities) = entities_data.get("entities").and_then(|e| e.as_array()) {
                    for entity in entities {
                        let name = entity
                            .get("entity_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let summary = entity.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                        if matches_any(name) || matches_any(summary) {
                            matching_entities.push(entity.clone());
                        }
                    }
                }

                let total_rules = rules_data
                    .get("total")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let total_entities = entities_data
                    .get("total")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let result = serde_json::json!({
                    "agent_id": req.agent_name,
                    "query": req.query,
                    "matching_rules": matching_rules,
                    "matching_entities": matching_entities,
                    "match_count": matching_rules.len() + matching_entities.len(),
                    "searched_rules": total_rules,
                    "searched_entities": total_entities,
                    "search_method": "client_side_text_match",
                    "note": "fermi does not expose a vector-search HTTP endpoint; \
                             this tool fetches the KG rules + entities and matches \
                             client-side. Switch to server-side vector search when \
                             fermi adds the route.",
                });
                Ok(self.client.with_wallet(result).await)
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_publish_checks",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_publish_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
        .await
    }

    /// Fork an ABW agent into a derivative — `POST /api/agents/{id}/fork`
    /// (fermi v0.10.16 fixed the fork path, which 500'd for everyone since
    /// mig-006 due to an `agents.owner_id` column reference). Creates
    /// `{source}_fork_{n}` with author-royalty tracking; the derived name is
    /// slug-validated (a legacy-name source with `-` or `/` is refused with a
    /// detailed 400 — rename via `/api/admin/agents/legacy-slugs` first).
    /// Requires API key.
    ///
    /// fermi-contract (v0.10.16, commit `4a7cd27f`): the fork endpoint was
    /// broken from mig-006 (2026-05-23) until v0.10.16 (2026-08-01) because
    /// the SELECT and INSERT both referenced `agents.owner_id` — a column
    /// that has never existed (the owner column is `agents.user_id` since
    /// mig-006). Every fork attempt 500'd at the SELECT. The fix aliased
    /// `user_id AS owner_id` in the SELECT and writes `user_id` in the
    /// INSERT. zed-kask's `swarm_fork_agent` just POSTs — the server-side
    /// fix means the tool now works where it previously 500'd. A live probe
    /// (`live_fork_agent_succeeds_post_v0_10_16` below) is the canary.
    #[tool(
        description = "Fork an Agent Bestiary World agent into a derivative (POST /api/agents/{id}/fork). Creates {source}_fork_{n} with author-royalty tracking. The source must have a slug-compliant name (legacy names with '-' or '/' are refused — admin-rename first). Requires API key."
    )]
    pub(crate) async fn swarm_fork_agent(
        &self,
        parameters: Parameters<ForkAgentRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_fork_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                self.client
                    .require_auth()
                    .map_err(SwarmError::into_tool_error)?;
                let req = parameters.0;
                if req.agent_name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "agent_name must be non-empty".to_string(),
                    ));
                }
                let payload = serde_json::json!({
                    "include_ontology": req.include_ontology.unwrap_or(false),
                    "include_embeddings": req.include_embeddings.unwrap_or(false),
                });
                let data = self
                    .client
                    .post(
                        &format!("/agents/{}/fork", url_encode_segment(&req.agent_name)),
                        &payload,
                    )
                    .await
                    .map_err(SwarmError::into_tool_error)?;
                Ok(self
                    .client
                    .with_wallet(serde_json::json!({
                        "forked_from": req.agent_name,
                        "result": data,
                    }))
                    .await)
            },
        )
        .await
    }
}
