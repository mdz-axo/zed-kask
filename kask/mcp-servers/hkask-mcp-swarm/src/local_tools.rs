//! Local swarm tools — delegation execution (delegate/fanout/pipeline), the
//! local agent store (list/clone/push/remove/create/reconfigure), and local
//! swarm membership (create/list/get/delete/add/remove). Split from
//! `hkask_mcp_swarm.rs` (M2). All operate on the local registry/runtime; no
//! ABW round-trips except `swarm_clone_to_local`/`swarm_push_to_cloud`.
use crate::SwarmServer;
use crate::abw_util::url_encode_segment;
use crate::error::{LocalSwarmError, SwarmError, map_local_swarm_error};
use crate::local_knowledge;
use crate::local_registry::{
    LocalAgentCapabilities, LocalAgentCard, LocalAgentDependencies, LocalAgentValence,
};
use crate::local_runtime::{LocalDelegateResult, MAX_FANOUT};
use crate::request_types::*;
use crate::sanitize::{
    filter_declared_skills, filter_mcp_tools, sanitize_abw_text, sanitize_agent_id,
};
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

/// Run a deterministic evaluator check against a response. Shared by
/// `swarm_evaluate_local` and `swarm_execute_plan_local` so the evaluation
/// logic lives once — a bad evaluator spec or regex errors propagate to the
/// caller rather than silently stamping `pass: false` (which would produce a
/// false fault attribution: the agent gets blamed for a bad evaluator).
fn run_evaluator(response: &str, evaluator: &str, spec: &str) -> Result<bool, McpToolError> {
    match evaluator {
        "contains" => Ok(response.contains(spec)),
        "not_contains" => Ok(!response.contains(spec)),
        "regex" => {
            let re = regex::Regex::new(spec)
                .map_err(|e| McpToolError::invalid_argument(format!("invalid regex spec: {e}")))?;
            Ok(re.is_match(response))
        }
        other => Err(McpToolError::invalid_argument(format!(
            "evaluator must be 'contains', 'not_contains', or 'regex'; got '{other}'"
        ))),
    }
}

#[tool_router(router = local_router, vis = "pub")]
impl SwarmServer {
    /// Delegate a task to a local agent. The agent must exist in the local
    /// registry (`agents/local/curated/<id>/agent_card.json`). The task is
    /// executed via `hkask-inference`. When the
    /// agent's card declares `capabilities.mcp_tools` (qualified
    /// `server/tool` names), those tools are declared to the model and model
    /// tool calls are dispatched through the zed IPC bridge's governed
    /// `McpRuntime` — the declared list is the allowlist. When the card
    /// declares `capabilities.skills`, each declared skill (capped at 3) is
    /// executed against the task through the zed-side `ManifestExecutor`
    /// before the LLM call and its output is injected as
    /// context. Spend is recorded per token across all tool-loop rounds
    /// (1 credit / 1000 tokens, capped at `credits_authorized`).
    ///
    /// **No funding gate and no consent token.** Local agents run on the
    /// operator's own substrate, so there is nothing to authorize — an unfunded
    /// ledger does not block this call and the balance may go negative
    /// (accumulated local spend). `credits_authorized` still caps the *recorded*
    /// cost, and the per-dispatch ceiling still bounds a single runaway dispatch.
    #[tool(
        description = "Delegate a task to a local agent (from agents/local/curated/). Executes via hkask-inference (Ollama/cloud) and records spend in the local ledger per token. Agents may declare capabilities.mcp_tools (qualified server/tool names) — those tools are dispatched through the zed IPC bridge's governed McpRuntime (allowlisted to the declared set). Agents may also declare capabilities.skills — each is executed against the task through the zed-side ManifestExecutor before the LLM call (capped at 3). No ABW calls. NO funding gate and no consent token — an unfunded ledger does not block this call; the ledger records spend rather than authorizing it. Returns the response, model, token usage, cost, resulting balance (may be negative), tool_calls summary, and executed_skills summary."
    )]
    pub(crate) async fn swarm_delegate_local(
        &self,
        parameters: Parameters<DelegateLocalRequest>,
    ) -> String {
        execute_tool_semantic(self, "swarm_delegate_local", Some(hkask_bridge_ontology::pko::PROCEDURE), async {
            let req = parameters.0;
            if req.agent_name.trim().is_empty() || req.task.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name and task must be non-empty".to_string(),
                ));
            }
            let runtime = self.local_runtime.get_or_init().await.map_err(map_local_swarm_error)?;
            // Look up the agent in the local registry.
            let agent = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "agent '{}' not found in local registry — load agents from agents/local/curated/<id>/agent_card.json",
                    req.agent_name
                ))
            })?;
            // Rung 4 (Binding): check the request against the agent's declared
            // `accepts` labels. Recorded, not fatal — the paper's "absence ≠
            // contradiction". `None` = no accepts declared; `Some(false)` =
            // mismatch (logged at warn).
            let bind_matched = crate::local_runtime::check_bind(&agent, &req.task);
            // Execute via the local runtime.
            let ceiling = self.client.config().max_credits_per_dispatch;
            let mut result = runtime
                .delegate(&agent, &req.task, req.credits_authorized, ceiling)
                .await
                .map_err(map_local_swarm_error)?;
            // Stamp the bind check result onto the delegation result.
            result.bind_matched = bind_matched;
            // Rung 3 (Grounding): enforce the grounding contract via the
            // central verification ledger. The store runs
            // `enforce_grounding` (when a contract exists for the
            // agent_type), writes a `GroundingRecord` to the cross-tool
            // ledger, and returns the result + cleaned JSON. All four local
            // delegation paths (`swarm_delegate_local`,
            // `swarm_execute_plan_local`, `swarm_fanout_local`,
            // `swarm_pipeline_local`) now enforce; the cloud paths use
            // `enforce_narrative` (prose-only grounding).
            let outcome = self.verification_store.enforce_and_stamp(
                "swarm_delegate_local",
                &req.agent_name,
                &agent.agent_type,
                &result.response,
                &result.tool_calls,
                // Top-level delegation — no parent envelope. The
                // monotone-provenance cap applies to agent-to-agent
                // composition hops (e.g. `swarm_pipeline_local`), not to
                // the operator's first delegation into the swarm.
                &[],
            );
            result.apply_grounding(outcome);
            // Rung 2 (Typing) post-invocation: validate the agent's output
            // against the schema for its `produces` port type (paper's "one
            // artifact, two uses"). Non-fatal — logged at warn so the operator
            // sees the mismatch, but the delegation result is still returned.
            // The output has already been grounded (unsourced fields nulled);
            // schema validation is an additional structural check on what
            // remains.
            if !agent.produces.is_empty() {
                if let Ok(output_json) = serde_json::from_str::<serde_json::Value>(&result.response) {
                    if let Err(schema_err) = self.local_registry.port_registry().validate_output(&agent.produces, &output_json) {
                        tracing::warn!(
                            target: "hkask.swarm.port_registry",
                            agent = %req.agent_name,
                            produces = ?agent.produces,
                            error = %schema_err,
                            "Port schema validation failed — agent output does not match its declared produces schema"
                        );
                    }
                }
            }
            // Stigmergy (ACO pheromone trail): record the delegation's
            // performance annotation to the agent's prefix-scoped semantic
            // memory. The SENSE phase can read these via
            // `swarm_search_knowledge_local` to assess agent fitness across
            // cascade invocations. Failures are logged (non-fatal) — the
            // delegation result is returned regardless.
            local_knowledge::record_delegation(
                &self.local_memory,
                &req.agent_name,
                result.latency_ms,
                result.task_success.as_ref().map(|t| t.pass),
                &result.response,
            )
            .await;
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
        execute_tool_semantic(
            self,
            "swarm_fanout_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let ceiling = self.client.config().max_credits_per_dispatch;
                let mut results = Vec::new();
                let mut failed = 0usize;
                let mut total_cost = 0i64;
                // Sum the uncapped figures too: `total_cost` is the sum of per-delegation
                // capped costs, so it inherits their understatement. Reporting both
                // keeps the aggregate reconciliation surface honest.
                let mut total_cost_uncapped = 0i64;
                let mut total_tokens = 0i64;
                let mut total_latency_ms = 0u64;
                for entry in &req.delegations {
                    let agent = self.local_registry.get(&entry.agent_name);
                    let Some(agent) = agent else {
                        failed += 1;
                        results.push(LocalDelegateResult::error_json(
                            &entry.agent_name,
                            &format!("agent '{}' not found in local registry", entry.agent_name),
                        ));
                        continue;
                    };
                    match runtime
                        .delegate(&agent, &entry.task, entry.credits_authorized, ceiling)
                        .await
                    {
                        Ok(mut r) => {
                            // Rung 3 (Grounding): enforce via the central
                            // verification ledger, same as
                            // `swarm_delegate_local`. Previously this path
                            // skipped grounding — delegations ran unrecorded
                            // and the trend query under-counted (the
                            // `.rules` "every delegating path must call
                            // enforce_and_stamp" Prohibition).
                            let outcome = self.verification_store.enforce_and_stamp(
                                "swarm_fanout_local",
                                &entry.agent_name,
                                &agent.agent_type,
                                &r.response,
                                &r.tool_calls,
                                // Fan-out: each delegation is a parallel
                                // top-level dispatch from the caller, not an
                                // agent-to-agent composition hop. No parent
                                // envelope to cap against.
                                &[],
                            );
                            r.apply_grounding(outcome);
                            total_cost += r.cost;
                            total_cost_uncapped += r.cost_uncapped;
                            total_tokens += r.tokens_used;
                            total_latency_ms = total_latency_ms.saturating_add(r.latency_ms);
                            results.push(r.to_result_json(true));
                        }
                        Err(e) => {
                            failed += 1;
                            results.push(LocalDelegateResult::error_json(
                                &entry.agent_name,
                                &e.to_string(),
                            ));
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
                    "total_cost_uncapped": total_cost_uncapped,
                    "total_tokens": total_tokens,
                    "total_latency_ms": total_latency_ms,
                    "balance": balance,
                    "failed": failed,
                    "succeeded": req.delegations.len() - failed,
                }))
            },
        )
        .await
    }

    /// Sequential pipeline: run N local agents in order, passing each agent's
    /// output as context to the next via `{prev_output}` substitution. Capped
    /// at `MAX_PIPELINE_STEPS` (10). No consent token — local mode.
    #[tool(
        description = "Sequential local pipeline: run N agents in order with {prev_output} substitution. Each step's task may contain {prev_output} which is replaced with the previous step's response. Capped at 10 steps. No consent token — local mode."
    )]
    pub(crate) async fn swarm_pipeline_local(
        &self,
        parameters: Parameters<PipelineLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_pipeline_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.steps.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "steps must be non-empty".to_string(),
                    ));
                }
                const MAX_PIPELINE_STEPS: usize = 10;
                if req.steps.len() > MAX_PIPELINE_STEPS {
                    return Err(McpToolError::invalid_argument(format!(
                        "pipeline cap is {MAX_PIPELINE_STEPS} steps, got {}",
                        req.steps.len()
                    )));
                }
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let ceiling = self.client.config().max_credits_per_dispatch;
                let mut results = Vec::new();
                let mut prev_output = String::new();
                // Track the previous step's envelope `blocks` array so the
                // next step's grounding enforcement can apply the
                // monotone-provenance (weakest-link) cap — a downstream
                // field's provenance may never exceed the same-named upstream
                // field's (paper §6 / .rules). The pipeline is the canonical
                // agent-to-agent composition hop: step N receives step N-1's
                // output via `{prev_output}`, so step N's provenance is capped
                // by step N-1's envelope blocks. `prev_upstream_blocks` is
                // empty for the first step (no parent).
                let mut prev_upstream_blocks: Vec<serde_json::Value> = Vec::new();
                let mut total_cost = 0i64;
                // Sum the uncapped figures too: `total_cost` is the sum of per-delegation
                // capped costs, so it inherits their understatement. Reporting both
                // keeps the aggregate reconciliation surface honest.
                let mut total_cost_uncapped = 0i64;
                let mut total_tokens = 0i64;
                for (i, step) in req.steps.iter().enumerate() {
                    // Substitute {prev_output} with the previous step's response.
                    let task = if i == 0 {
                        step.task.clone()
                    } else {
                        step.task.replace("{prev_output}", &prev_output)
                    };
                    let agent = self.local_registry.get(&step.agent_name);
                    let Some(agent) = agent else {
                        results.push(serde_json::json!({
                            "step": i,
                            "agent_name": step.agent_name,
                            "ok": false,
                            "error": format!(
                                "agent '{}' not found in local registry",
                                step.agent_name
                            ),
                        }));
                        break; // pipeline stops on agent-not-found
                    };
                    match runtime
                        .delegate(&agent, &task, step.credits_authorized, ceiling)
                        .await
                    {
                        Ok(mut r) => {
                            // Rung 3 (Grounding): enforce via the central
                            // verification ledger, same as
                            // `swarm_delegate_local`. Previously this path
                            // skipped grounding — delegations ran unrecorded
                            // and the trend query under-counted (the
                            // `.rules` "every delegating path must call
                            // enforce_and_stamp" Prohibition). The cleaned
                            // (grounded) response is what feeds the next
                            // pipeline step via `prev_output`, so ungrounded
                            // fields do not propagate down the chain.
                            //
                            // Monotone provenance (paper §6 / .rules): pass
                            // the previous step's envelope `blocks` as
                            // `upstream_blocks` so a downstream field cannot
                            // be re-emitted with stronger provenance than the
                            // same-named upstream field (the laundering case).
                            let outcome = self.verification_store.enforce_and_stamp(
                                "swarm_pipeline_local",
                                &step.agent_name,
                                &agent.agent_type,
                                &r.response,
                                &r.tool_calls,
                                &prev_upstream_blocks,
                            );
                            r.apply_grounding(outcome);
                            // Capture this step's envelope blocks for the next
                            // iteration's weakest-link cap. When grounding did
                            // not run (no contract / unenforceable), the
                            // envelope carries an empty `blocks` array —
                            // passing it is a no-op (no fields to cap).
                            prev_upstream_blocks = r
                                .envelope
                                .as_ref()
                                .and_then(|env| env.get("blocks").cloned())
                                .and_then(|blocks| blocks.as_array().cloned())
                                .unwrap_or_default();
                            prev_output = r.response.clone();
                            total_cost += r.cost;
                            total_cost_uncapped += r.cost_uncapped;
                            total_tokens += r.tokens_used;
                            let mut entry = r.to_result_json(false);
                            entry["step"] = serde_json::json!(i);
                            results.push(entry);
                        }
                        Err(e) => {
                            let mut entry =
                                LocalDelegateResult::error_json(&step.agent_name, &e.to_string());
                            entry["step"] = serde_json::json!(i);
                            results.push(entry);
                            break; // pipeline stops on failure
                        }
                    }
                }
                let balance: Option<i64> = runtime.balance();
                Ok(serde_json::json!({
                    "steps_completed": results.len(),
                    "results": results,
                    "total_cost": total_cost,
                    "total_cost_uncapped": total_cost_uncapped,
                    "total_tokens": total_tokens,
                    "final_output": prev_output,
                    "balance": balance,
                }))
            },
        )
        .await
    }

    // Local agent store tools (v2 §15 Slice 11).

    /// List agents from the local registry. Returns the cards loaded from
    /// `agents/local/curated/`. Each card carries a `cloud_swarm_id` field: when
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
        execute_tool_semantic(
            self,
            "swarm_list_local_agents",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
            },
        )
        .await
    }

    /// Clone an ABW agent to the local registry. Fetches the agent card from
    /// ABW via `swarm_get_agent`, sets `min_provider_class: local`, writes it
    /// to `agents/local/curated/<id>/agent_card.json`, and sets `cloud_swarm_id` to
    /// the ABW agent id (marking it as synced). The ABW catalogue is open
    /// (no API key required) — same as `swarm_list_agents`. The clone is a
    /// read-from-ABW + write-to-local-filesystem operation with no ABW
    /// mutation, so `require_auth` is not needed.
    #[tool(
        description = "Clone an ABW agent to the local registry. Fetches the card from ABW, sets min_provider_class: local, writes to agents/local/curated/<id>/agent_card.json, and sets cloud_id to mark it as synced. The ABW catalogue is open — no API key required."
    )]
    pub(crate) async fn swarm_clone_to_local(
        &self,
        parameters: Parameters<CloneToLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_clone_to_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                // The local clone gets a distinct agent_id so it cannot collide
                // with the cloud agent's id. This is the root-cause fix for the
                // panel merge bug where a cloned card with `agent_id ==
                // cloud_swarm_id` self-suppressed and never appeared as a Local
                // row. The `-clone` suffix also differentiates the local card
                // from the cloud card in `swarm_delegate_local` lookups.
                let clone_agent_id = self.local_registry.unique_clone_id(&safe_agent_id);
                // Display label: prefer the ABW agent_name (human-readable),
                // append " (Clone)" so the operator can distinguish the local
                // clone from the cloud original in the panel.
                let display_name = format!("{} (Clone)", req.agent_name);
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
                let accepts: Vec<String> = abw_card
                    .get("accepts")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let produces: Vec<String> = abw_card
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
                    .map(sanitize_abw_text);
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
                let import_labels: Vec<String> =
                    accepts.iter().chain(produces.iter()).cloned().collect();
                let local_card = LocalAgentCard {
                    agent_id: clone_agent_id.clone(),
                    agent_type,
                    description,
                    display_name,
                    accepts,
                    produces,
                    dependencies: deps,
                    capabilities: LocalAgentCapabilities {
                        model,
                        min_provider_class: "local".to_string(),
                        system_prompt,
                        mcp_tools,
                        skills,
                        ..Default::default()
                    },
                    cloud_swarm_id: Some(req.agent_name),
                    tags: Vec::new(),
                    visibility: String::new(),
                    valence: None,
                };
                // The ABW catalogue's port labels are the card's own
                // taxonomy, not locally-authored free strings. Register them
                // as persisted extension types so `write_card`'s typing gate
                // (and every future load) resolves them. Labels that already
                // resolve are no-ops.
                self.local_registry
                    .promote_imported_port_types(&import_labels)
                    .map_err(map_local_swarm_error)?;
                // write_card runs Rung 1 (Presence) + Rung 2 (Typing) +
                // sanitize/canonicalize/dir-write/load — same invariant as
                // `swarm_create_local_agent`.
                let card_path = self
                    .local_registry
                    .write_card(&local_card)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({
                    "cloned": clone_agent_id,
                    "cloud_id": local_card.cloud_swarm_id,
                    "path": card_path,
                    "synced": true,
                }))
            },
        )
        .await
    }

    /// Push a local agent to ABW. Reads the local card, creates or updates
    /// the ABW agent via `POST /api/agents`, and sets `cloud_swarm_id` on the local
    /// card to the ABW agent id (marking it as synced). Requires the ABW API
    /// key. If the agent already has a `cloud_swarm_id`, the ABW agent is updated;
    /// otherwise a new ABW agent is created.
    #[tool(
        description = "Push a local agent to ABW. Creates or updates the ABW agent from the local card, and sets cloud_id on the local card to mark it as synced. Requires ABW API key."
    )]
    pub(crate) async fn swarm_push_to_cloud(
        &self,
        parameters: Parameters<PushToCloudSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_push_to_cloud",
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
                // POST to ABW. If the agent already exists (cloud_swarm_id is set),
                // ABW updates it; otherwise a new agent is created.
                let result = self
                    .client
                    .post("/agents", &payload)
                    .await
                    .map_err(SwarmError::into_tool_error)?;
                // Update the local card's cloud_swarm_id to mark it as synced.
                let cloud_swarm_id = result
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&local_card.agent_id)
                    .to_string();
                let mut updated_card = local_card.clone();
                updated_card.cloud_swarm_id = Some(cloud_swarm_id.clone());
                // Write the updated card back to the local registry. Sanitize
                // the agent_id for filesystem use (defense-in-depth — the card
                // came from disk, but a manually-placed malicious card could
                // carry a path-traversal id).
                let dir = self.client.config().local_agents_dir.clone();
                let safe_id = sanitize_agent_id(&local_card.agent_id).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "agent_id '{}' contains no safe characters",
                        local_card.agent_id
                    ))
                })?;
                let card_path = std::path::Path::new(&dir)
                    .join(&safe_id)
                    .join("agent_card.json");
                let json = serde_json::to_string_pretty(&updated_card)
                    .map_err(|e| McpToolError::internal(format!("failed to serialize: {e}")))?; // rr0044-ok: serde serialization of own struct
                std::fs::write(&card_path, json).map_err(|e| {
                    hkask_mcp_server::map_io_error(
                        e,
                        &format!("failed to write {}", card_path.display()),
                    )
                })?;
                self.local_registry.load().map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({
                    "pushed": local_card.agent_id,
                    "cloud_id": cloud_swarm_id,
                    "synced": true,
                    "result": result,
                }))
            },
        )
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
        execute_tool_semantic(
            self,
            "swarm_remove_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                    McpToolError::invalid_argument(format!(
                        "agent_id '{}' contains no safe characters",
                        card.agent_id
                    ))
                })?;
                let dir = self.client.config().local_agents_dir.clone();
                let registry_root = std::fs::canonicalize(&dir).map_err(|e| {
                    hkask_mcp_server::map_io_error(
                        e,
                        &format!("failed to resolve local agents dir {}", dir),
                    )
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
                    return Err(McpToolError::invalid_argument(
                        "refusing to remove a path outside the local agents dir".to_string(),
                    ));
                }
                if target.exists() {
                    std::fs::remove_dir_all(&target).map_err(|e| {
                        hkask_mcp_server::map_io_error(
                            e,
                            &format!("failed to remove local agent dir {}", target.display()),
                        )
                    })?;
                }
                self.local_registry.load().map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({
                    "removed": card.agent_id,
                    "cloud_id": card.cloud_swarm_id,
                    "synced": card.cloud_swarm_id.is_some(),
                }))
            },
        )
        .await
    }

    /// Create a new local agent card programmatically. Writes
    /// `agents/local/curated/<id>/agent_card.json` and reloads the registry. The
    /// counterpart of `swarm_remove_local`: remove deletes a card, create
    /// writes one. No ABW round-trip (unlike `swarm_clone_to_local`, which
    /// copies from ABW). No consent token — local mode has no consent gate;
    /// card creation is free, and local execution is not gated on funds either (the
    /// ledger records spend rather than authorizing it).
    #[tool(
        description = "Create a new local agent card programmatically. Writes agents/local/curated/<id>/agent_card.json and reloads the registry. No consent token — local mode has no consent gate."
    )]
    pub(crate) async fn swarm_create_local_agent(
        &self,
        parameters: Parameters<CreateLocalAgentRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_create_local_agent",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
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
                    display_name: String::new(),
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
                        skills: req.skills,
                        output_contract: req.output_contract,
                    },
                    cloud_swarm_id: None,
                    tags: req.tags,
                    visibility: if req.visibility.trim().is_empty() {
                        "private".to_string()
                    } else {
                        req.visibility
                    },
                    valence: req.valence.map(|v| LocalAgentValence {
                        arousal: v.arousal,
                        valence: v.valence,
                        primary_affect: v.primary_affect,
                        personality_traits: v.personality_traits.unwrap_or_default(),
                    }),
                };
                let card_path = self
                    .local_registry
                    .write_card(&card)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({
                    "created": safe_id,
                    "path": card_path,
                }))
            },
        )
        .await
    }

    /// Reconfigure an existing local agent's prompt in place (Cybernetic Swarm
    /// Plan C6). Updates ONLY the `system_prompt` (and optionally
    /// `model`/`mcp_tools`/`skills` when supplied non-empty); preserves
    /// `agent_id`, `agent_type`, `description`, `accepts`, `produces`,
    /// `dependencies`, and the `cloud_swarm_id` sync link. The DECIDE
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
        execute_tool_semantic(self, "swarm_reconfigure_local_agent", Some(hkask_bridge_ontology::pko::PROCEDURE), async {
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
                .map_err(map_local_swarm_error)?;
            Ok(serde_json::json!({
                "reconfigured": card.agent_id,
                "cloud_id": card.cloud_swarm_id,
                "synced": card.cloud_swarm_id.is_some(),
                "path": path,
            }))
        })
        .await
    }

    // ── Local swarm membership (local replica of an ABW workspace) ─────────────

    /// Create a local swarm - the local replica of an ABW workspace/team. A
    /// named, mission-bearing grouping of local agent ids. No cost and no
    /// consent token. The local ledger records spend; it gates neither
    /// delegation nor roster edits.
    /// Optionally seeds the roster with `agents`. Returns the new swarm with
    /// its generated `swarm_id`. The counterpart of `swarm_create_swarm` for
    /// the local backend.
    #[tool(
        description = "Create a local swarm (the local replica of an ABW workspace). A named grouping of local agent ids with a mission. No cost, no consent token. Optionally seed members via `agents`. Counterpart of swarm_create_swarm for the local backend."
    )]
    pub(crate) async fn swarm_create_local_swarm(
        &self,
        parameters: Parameters<CreateLocalSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_create_local_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "name must be non-empty".to_string(),
                    ));
                }
                let swarm = self
                    .local_swarms
                    .create(&req.name, &req.mission, req.agents)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::to_value(&swarm).unwrap_or_else(
                    |_| serde_json::json!({ "swarm_id": swarm.swarm_id, "name": swarm.name }),
                ))
            },
        )
        .await
    }

    /// List all local swarms (id, name, mission, members, created_at). The
    /// local counterpart of `swarm_get_swarm` (no-id list mode).
    #[tool(
        description = "List all local swarms. Each entry has swarm_id, name, mission, members (agent ids), and created_at. Read-only. Local counterpart of swarm_get_swarm list mode."
    )]
    pub(crate) async fn swarm_list_local_swarms(
        &self,
        _parameters: Parameters<ListLocalSwarmsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_list_local_swarms",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let swarms = self.local_swarms.list();
                Ok(serde_json::json!({
                    "count": swarms.len(),
                    "swarms": swarms,
                }))
            },
        )
        .await
    }

    /// Get a single local swarm by id, including its roster.
    #[tool(
        description = "Get a single local swarm by swarm_id, including its member roster (agent ids). Returns not-found if the swarm does not exist."
    )]
    pub(crate) async fn swarm_get_local_swarm(
        &self,
        parameters: Parameters<GetLocalSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_get_local_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.swarm_id.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "swarm_id must be non-empty".to_string(),
                    ));
                }
                let swarm = self.local_swarms.get(&req.swarm_id).ok_or_else(|| {
                    McpToolError::not_found(format!("local swarm '{}' not found", req.swarm_id))
                })?;
                Ok(serde_json::to_value(&swarm).unwrap_or_else(
                    |_| serde_json::json!({ "swarm_id": swarm.swarm_id, "name": swarm.name }),
                ))
            },
        )
        .await
    }

    /// Permanently delete a local swarm. The roster is dropped with the swarm;
    /// member agents are NOT touched (they stay in `LocalAgentRegistry`). The
    /// local counterpart of `swarm_delete_swarm`.
    #[tool(
        description = "Permanently delete a local swarm by swarm_id. The roster is dropped; member agents are NOT deleted (use swarm_remove_local for that). Counterpart of swarm_delete_swarm for the local backend."
    )]
    pub(crate) async fn swarm_delete_local_swarm(
        &self,
        parameters: Parameters<DeleteLocalSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_delete_local_swarm",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.swarm_id.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "swarm_id must be non-empty".to_string(),
                    ));
                }
                self.local_swarms
                    .delete(&req.swarm_id)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::json!({ "deleted": req.swarm_id }))
            },
        )
        .await
    }

    /// Add a local agent to a local swarm's roster (idempotent - a duplicate
    /// add is a no-op). The local counterpart of `swarm_hire` (add member),
    /// without the cost or consent gate. The agent need not already exist in
    /// `LocalAgentRegistry` (the roster is ids; resolution happens at
    /// delegation time, mirroring ABW workspaces).
    #[tool(
        description = "Add a local agent to a local swarm's roster by swarm_id + agent_name. Idempotent. No cost, no consent token. The agent need not exist in the registry yet (roster is ids). Counterpart of swarm_hire (add member) for the local backend."
    )]
    pub(crate) async fn swarm_add_agent_local(
        &self,
        parameters: Parameters<AddAgentToLocalSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_add_agent_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.swarm_id.trim().is_empty() || req.agent_name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "swarm_id and agent_name must be non-empty".to_string(),
                    ));
                }
                let swarm = self
                    .local_swarms
                    .add_member(&req.swarm_id, &req.agent_name)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::to_value(&swarm).unwrap_or_else(
                    |_| serde_json::json!({ "swarm_id": swarm.swarm_id, "members": swarm.members }),
                ))
            },
        )
        .await
    }

    /// Remove a local agent from a local swarm's roster (idempotent - removing
    /// a non-member is a no-op). The local counterpart of `swarm_fire`
    /// (remove member). Does NOT delete the agent from `LocalAgentRegistry`.
    #[tool(
        description = "Remove a local agent from a local swarm's roster by swarm_id + agent_name. Idempotent. Does NOT delete the agent (use swarm_remove_local for that). Counterpart of swarm_fire (remove member) for the local backend."
    )]
    pub(crate) async fn swarm_remove_agent_local(
        &self,
        parameters: Parameters<RemoveAgentFromLocalSwarmRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_remove_agent_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.swarm_id.trim().is_empty() || req.agent_name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "swarm_id and agent_name must be non-empty".to_string(),
                    ));
                }
                let swarm = self
                    .local_swarms
                    .remove_member(&req.swarm_id, &req.agent_name)
                    .map_err(map_local_swarm_error)?;
                Ok(serde_json::to_value(&swarm).unwrap_or_else(
                    |_| serde_json::json!({ "swarm_id": swarm.swarm_id, "members": swarm.members }),
                ))
            },
        )
        .await
    }

    /// AI assist for the swarm panel authoring forms — suggests completions for
    /// partial inputs or validates well-formedness. Authoring aid — read-only,
    /// spends nothing. Runs the on-disk `swarm-compose-guide` skill cascade
    /// (rendering the `swarm-compose-guide.j2` Jinja2 guidance template) via the
    /// resolved `SkillExecPort` — the template is the single source of truth for
    /// field definitions and ABW/Local composition considerations, not hardcoded
    /// Rust. The form fields are serialized as a JSON object string and passed
    /// as the `task`; `AgentSkillExec` merges JSON-object tasks into the cascade
    /// context as top-level template variables.
    #[tool(
        description = "AI assist for the swarm panel authoring forms (agent/swarm). Suggests completions for partial inputs or validates well-formedness. Authoring aid — read-only, spends nothing. Runs the swarm-compose-guide skill cascade (Jinja2 guidance template) via the SkillExecPort — the template is the source of truth, not hardcoded Rust. The mode field (abw/local) tailors the guidance; no ABW calls in either mode."
    )]
    pub(crate) async fn swarm_ai_assist(&self, parameters: Parameters<AiAssistRequest>) -> String {
        execute_tool_semantic(self, "swarm_ai_assist", Some(hkask_bridge_ontology::pko::PROCEDURE), async {
            let req = parameters.0;
            match req.action.as_str() {
                "suggest" | "validate" => {}
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "action must be 'suggest' or 'validate', got '{other}'"
                    )));
                }
            }
            match req.surface.as_str() {
                "agent" | "swarm" => {}
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "surface must be 'agent' or 'swarm', got '{other}'"
                    )));
                }
            }
            match req.mode.as_str() {
                "abw" | "local" => {}
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "mode must be 'abw' or 'local', got '{other}'"
                    )));
                }
            }

            // Serialize the form fields as a JSON object string. The
            // `AgentSkillExec` wrapper (zed side) detects JSON-object tasks and
            // merges their fields into the cascade context as top-level template
            // variables, so the `swarm-compose-guide.j2` template sees
            // `{{ surface }}`, `{{ mode }}`, `{{ action }}`, `{{ name }}`, etc.
            // directly. The raw JSON string is also carried as `{{ task }}`.
            let json_task = serde_json::to_string(&serde_json::json!({
                "action": req.action,
                "surface": req.surface,
                "mode": req.mode,
                "name": req.name,
                "agent_type": req.agent_type,
                "description": req.description,
                "system_prompt": req.system_prompt,
                "mission": req.mission,
                "agents": req.agents,
            }))
            .map_err(|e| {
                map_local_swarm_error(LocalSwarmError::Unavailable(format!(
                    "failed to serialize ai-assist task: {e}"
                )))
            })?;

            // Run the on-disk `swarm-compose-guide` skill cascade. The cascade
            // renders the Jinja2 guidance template (the single source of truth
            // for composition guidance) and returns the LLM's JSON output as
            // text. The skill exec port routes through the zed IPC bridge to the
            // `BridgeManifestExecutor`, which loads the manifest + template from
            // `{kask_data_dir}/skills/registry/` — edits to the on-disk template take
            // effect without recompiling.
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            let skill_exec = runtime.skill_exec();
            let text = skill_exec
                .execute_skill("swarm-compose-guide", &json_task)
                .await
                .map_err(|e| {
                    map_local_swarm_error(LocalSwarmError::Unavailable(format!(
                        "swarm-compose-guide skill execution failed: {e}"
                    )))
                })?;

            let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
            let result = if req.action == "suggest" {
                match parsed {
                    Some(v) => {
                        let suggestions = serde_json::json!({
                            "name": v.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                            "agent_type": v.get("agent_type").and_then(|x| x.as_str()).unwrap_or(""),
                            "description": v.get("description").and_then(|x| x.as_str()).unwrap_or(""),
                            "system_prompt": v.get("system_prompt").and_then(|x| x.as_str()).unwrap_or(""),
                            "mission": v.get("mission").and_then(|x| x.as_str()).unwrap_or(""),
                            "agents": v.get("agents").and_then(|x| x.as_str()).unwrap_or(""),
                        });
                        serde_json::json!({
                            "action": req.action,
                            "surface": req.surface,
                            "mode": req.mode,
                            "suggestions": suggestions,
                            "valid": serde_json::Value::Null,
                            "issues": serde_json::json!([]),
                            "notes": "",
                        })
                    }
                    None => {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            "swarm_ai_assist (suggest) cascade output was not valid JSON — returning raw text in notes"
                        );
                        serde_json::json!({
                            "action": req.action,
                            "surface": req.surface,
                            "mode": req.mode,
                            "suggestions": serde_json::json!({
                                "name": "", "agent_type": "", "description": "",
                                "system_prompt": "", "mission": "", "agents": "",
                            }),
                            "valid": false,
                            "issues": serde_json::json!([]),
                            "notes": text,
                        })
                    }
                }
            } else {
                match parsed {
                    Some(v) => {
                        let valid = v
                            .get("valid")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);
                        let issues = v.get("issues").cloned().unwrap_or(serde_json::json!([]));
                        serde_json::json!({
                            "action": req.action,
                            "surface": req.surface,
                            "mode": req.mode,
                            "suggestions": serde_json::Value::Null,
                            "valid": valid,
                            "issues": issues,
                            "notes": "",
                        })
                    }
                    None => {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            "swarm_ai_assist (validate) cascade output was not valid JSON — returning raw text in notes"
                        );
                        serde_json::json!({
                            "action": req.action,
                            "surface": req.surface,
                            "mode": req.mode,
                            "suggestions": serde_json::Value::Null,
                            "valid": false,
                            "issues": serde_json::json!([]),
                            "notes": text,
                        })
                    }
                }
            };
            Ok(result)
        })
        .await
    }

    /// Deterministic task-success evaluator. The Curator (or a human) calls
    /// this after `swarm_delegate_local` to stamp a `TaskSuccessVerdict` with
    /// `provenance: Deterministic` onto the delegation result. This is the
    /// enforcement point for the C5/C6 fault-attribution loop: ORIENT's
    /// highest-fidelity fault signal (rule 1: per-delegation task failure)
    /// requires a deterministic `task_success` verdict — an LLM-judged verdict
    /// is downgraded by ORIENT (Gap S3). No ABW calls, no ledger spend —
    /// evaluation is free.
    #[tool(
        description = "Deterministic task-success evaluator for local swarm delegations. Takes an agent's response and a deterministic check (contains / not_contains / regex) and returns a TaskSuccessVerdict with provenance: Deterministic. The Curator calls this after swarm_delegate_local to stamp task_success for the C5/C6 fault-attribution loop. No ABW calls, no ledger spend — evaluation is free."
    )]
    pub(crate) async fn swarm_evaluate_local(
        &self,
        parameters: Parameters<EvaluateLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_evaluate_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.response.trim().is_empty() || req.spec.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "response and spec must be non-empty".to_string(),
                    ));
                }
                let pass = run_evaluator(&req.response, &req.evaluator, &req.spec)?;
                let detail = format!(
                    "evaluator={}, spec_len={}, pass={}",
                    req.evaluator,
                    req.spec.len(),
                    pass
                );
                let verdict = crate::local_runtime::TaskSuccessVerdict {
                    pass,
                    score: None,
                    detail: Some(detail),
                    provenance: crate::local_runtime::TaskSuccessProvenance::Deterministic,
                };
                Ok(serde_json::to_value(&verdict).unwrap_or_else(
                    |_| serde_json::json!({ "error": "failed to serialize verdict" }),
                ))
            },
        )
        .await
    }

    /// Execute a swarm-intelligence plan: run each delegation, evaluate each
    /// result (when an evaluator is provided), and return the collected
    /// `LocalDelegateResult` array with `task_success` verdicts stamped. This
    /// closes the loop deterministically — the caller passes the plan, the
    /// tool executes it and stamps verdicts, the caller passes the results
    /// back to swarm-intelligence. Works in any context: chat, autonomous
    /// pipeline, or API. Capped at 10 delegations (same as fanout). Each
    /// delegation runs sequentially to avoid ledger TOCTOU.
    #[tool(
        description = "Execute a swarm-intelligence plan: run each delegation via the local runtime, evaluate each result with a deterministic check (when an evaluator is provided), and return the collected LocalDelegateResult array with task_success verdicts stamped. Capped at 10 delegations. Each delegation runs sequentially to avoid ledger TOCTOU. The returned array is ready to feed back to swarm-intelligence as delegate_results. No consent token — local mode."
    )]
    pub(crate) async fn swarm_execute_plan_local(
        &self,
        parameters: Parameters<ExecutePlanLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_execute_plan_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.delegations.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "delegations must be non-empty".to_string(),
                    ));
                }
                if req.delegations.len() > MAX_FANOUT {
                    return Err(McpToolError::invalid_argument(format!(
                        "plan cap is {MAX_FANOUT} delegations, got {}",
                        req.delegations.len()
                    )));
                }
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let ceiling = self.client.config().max_credits_per_dispatch;
                let mut results = Vec::new();
                let mut failed = 0usize;
                let mut total_cost = 0i64;
                // Sum the uncapped figures too: `total_cost` is the sum of per-delegation
                // capped costs, so it inherits their understatement. Reporting both
                // keeps the aggregate reconciliation surface honest.
                let mut total_cost_uncapped = 0i64;
                let mut total_tokens = 0i64;
                for entry in &req.delegations {
                    let agent = self.local_registry.get(&entry.agent_name);
                    let Some(agent) = agent else {
                        failed += 1;
                        results.push(LocalDelegateResult::error_json(
                            &entry.agent_name,
                            &format!("agent '{}' not found in local registry", entry.agent_name),
                        ));
                        continue;
                    };
                    match runtime
                        .delegate(&agent, &entry.task, entry.credits_authorized, ceiling)
                        .await
                    {
                        Ok(mut r) => {
                            total_cost += r.cost;
                            total_cost_uncapped += r.cost_uncapped;
                            total_tokens += r.tokens_used;
                            // Stamp the deterministic verdict when an evaluator is provided.
                            if let Some(ev) = &entry.evaluator {
                                let pass = run_evaluator(&r.response, &ev.evaluator, &ev.spec)?;
                                r.task_success = Some(crate::local_runtime::TaskSuccessVerdict {
                                    pass,
                                    score: None,
                                    detail: Some(format!(
                                        "evaluator={}, spec_len={}, pass={}",
                                        ev.evaluator,
                                        ev.spec.len(),
                                        pass
                                    )),
                                    provenance:
                                        crate::local_runtime::TaskSuccessProvenance::Deterministic,
                                });
                            }
                            // Rung 3 (Grounding): enforce via the central
                            // verification ledger (same as
                            // `swarm_delegate_local`).
                            let outcome = self.verification_store.enforce_and_stamp(
                                "swarm_execute_plan_local",
                                &entry.agent_name,
                                &agent.agent_type,
                                &r.response,
                                &r.tool_calls,
                                // Execute-plan: each delegation is a
                                // top-level dispatch from the Curator's plan,
                                // not an agent-to-agent composition hop. No
                                // parent envelope to cap against.
                                &[],
                            );
                            r.apply_grounding(outcome);
                            // Record stigmergy (same as swarm_delegate_local).
                            local_knowledge::record_delegation(
                                &self.local_memory,
                                &entry.agent_name,
                                r.latency_ms,
                                r.task_success.as_ref().map(|t| t.pass),
                                &r.response,
                            )
                            .await;
                            results.push(serde_json::to_value(&r).unwrap_or_else(
                                |_| serde_json::json!({ "error": "failed to serialize result" }),
                            ));
                        }
                        Err(e) => {
                            failed += 1;
                            results.push(LocalDelegateResult::error_json(
                                &entry.agent_name,
                                &e.to_string(),
                            ));
                        }
                    }
                }
                let balance: Option<i64> = runtime.balance();
                Ok(serde_json::json!({
                    "results": results,
                    "total_cost": total_cost,
                    "total_cost_uncapped": total_cost_uncapped,
                    "total_tokens": total_tokens,
                    "balance": balance,
                    "failed": failed,
                    "succeeded": req.delegations.len() - failed,
                }))
            },
        )
        .await
    }
}

#[cfg(test)]
mod grounding_wiring_tests {
    use super::*;
    use crate::config::SwarmConfig;
    use crate::local_runtime::LocalSwarmRuntime;
    use hkask_types::ports::inference_port::{InferencePort, SkillExecPort, ToolDispatchPort};
    use hkask_types::{InferenceError, InferenceResult, InferenceUsage, SkillExecError, WebID};
    use hkask_verification::{TrendScope, VerificationStore};
    use std::future::Future;
    use std::pin::Pin;

    /// A stub `InferencePort` that returns a canned JSON response with no tool
    /// calls, so `AgentExecutor::run` exits the tool loop after one round.
    struct StubInference {
        response: String,
    }
    impl InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<Output = std::result::Result<InferenceResult, InferenceError>>
                    + Send
                    + '_,
            >,
        > {
            let response = self.response.clone();
            Box::pin(async move {
                Ok(InferenceResult {
                    text: response,
                    model: "stub-model".to_string(),
                    usage: InferenceUsage {
                        prompt_tokens: 10,
                        completion_tokens: 10,
                        total_tokens: 20,
                    },
                    finish_reason: "stop".to_string(),
                    token_probabilities: None,
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// A stub `ToolDispatchPort` that errors if called (the stub inference
    /// returns no tool calls, so this should never fire).
    struct StubToolDispatch;
    impl ToolDispatchPort for StubToolDispatch {
        fn invoke_tool<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _allowed: &'a [String],
        ) -> Pin<
            Box<
                dyn Future<Output = std::result::Result<serde_json::Value, InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(InferenceError::Generation("stub tool dispatch".into())) })
        }
    }

    /// A stub `ToolDispatchPort` that succeeds for a specific qualified tool
    /// name and returns a canned result. Used by the monotone-provenance
    /// pipeline test to produce a `Sourced` field (the grounding contract
    /// requires a successful `zed/write_file` tool call to source
    /// `deliverable_path`).
    struct SucceedingToolDispatch {
        qualified_tool: String,
        result: serde_json::Value,
    }
    impl ToolDispatchPort for SucceedingToolDispatch {
        fn invoke_tool<'a>(
            &'a self,
            server: &'a str,
            tool: &'a str,
            _args: serde_json::Value,
            _allowed: &'a [String],
        ) -> Pin<
            Box<
                dyn Future<Output = std::result::Result<serde_json::Value, InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            let qualified = format!("{server}/{tool}");
            let result = self.result.clone();
            let expected = self.qualified_tool.clone();
            Box::pin(async move {
                if qualified == expected {
                    Ok(result)
                } else {
                    Err(InferenceError::Generation(format!(
                        "unexpected tool call: {qualified} (expected {expected})"
                    )))
                }
            })
        }
    }

    /// A stub `InferencePort` that returns a sequence of canned responses.
    /// Each call pops the next response from the front. Used by the
    /// monotone-provenance pipeline test: step 1 gets a text-only response
    /// (no tool calls), step 2 gets a response with a tool call (round 1)
    /// followed by a text-only response (round 2, final).
    struct SequenceStubInference {
        responses: std::sync::Mutex<std::collections::VecDeque<InferenceResult>>,
    }
    impl SequenceStubInference {
        fn new(responses: Vec<InferenceResult>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses.into()),
            }
        }
    }
    impl InferencePort for SequenceStubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<Output = std::result::Result<InferenceResult, InferenceError>>
                    + Send
                    + '_,
            >,
        > {
            let result = self.responses.lock().expect("responses mutex").pop_front();
            Box::pin(async move {
                result.ok_or_else(|| {
                    InferenceError::Generation("sequence stub exhausted".to_string())
                })
            })
        }
    }

    /// A stub `SkillExecPort` that errors if called (the test agent declares
    /// no skills, so this should never fire).
    struct StubSkillExec;
    impl SkillExecPort for StubSkillExec {
        fn execute_skill<'a>(
            &'a self,
            _name: &'a str,
            _task: &'a str,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, SkillExecError>> + Send + 'a>>
        {
            Box::pin(async { Err(SkillExecError::Unavailable("stub".into())) })
        }
    }

    /// Build a `SwarmServer` with stub deps and an in-memory verification store.
    /// The local runtime returns a canned JSON response so grounding can run.
    async fn server_with_stub_runtime(
        response: String,
    ) -> (SwarmServer, std::sync::Arc<VerificationStore>) {
        let ledger = hkask_ledger::Ledger::from_driver(
            hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
        )
        .expect("ledger");
        let runtime = LocalSwarmRuntime::with_deps(
            ledger,
            std::sync::Arc::new(StubInference { response }),
            std::sync::Arc::new(StubToolDispatch),
            std::sync::Arc::new(StubSkillExec),
        )
        .expect("runtime with stub deps");

        let lazy_runtime = std::sync::Arc::new(crate::local_runtime::LazyLocalSwarmRuntime::lazy(
            "/tmp/hkask-test-ledger.db".to_string(),
            None,
        ));
        lazy_runtime.set_runtime(runtime);

        let verification_store = std::sync::Arc::new(VerificationStore::in_memory());

        let config = SwarmConfig::default();
        let server = SwarmServer::new(
            WebID::for_agent_name("test-operator"),
            std::sync::Arc::new(crate::abw_client::SwarmClient::new(
                reqwest::Client::new(),
                config,
            )),
            std::sync::Arc::new(crate::consent::ConsentStore::default()),
            std::sync::Arc::new(crate::local_registry::LocalAgentRegistry::new(
                "/nonexistent",
            )),
            lazy_runtime,
            std::sync::Arc::new(crate::local_swarms::LocalSwarmRegistry::new("/nonexistent")),
            std::sync::Arc::new(crate::local_knowledge::LazyLocalMemory::lazy(
                "/tmp/never.db".to_string(),
                "short".to_string(),
                1024,
            )),
            verification_store.clone(),
        );
        (server, verification_store)
    }

    /// Write a minimal local agent card to a temp dir and return a registry
    /// pointing at it.
    fn registry_with_agent(
        dir: &std::path::Path,
    ) -> std::sync::Arc<crate::local_registry::LocalAgentRegistry> {
        let registry = std::sync::Arc::new(crate::local_registry::LocalAgentRegistry::new(
            dir.to_string_lossy().to_string(),
        ));
        let card = LocalAgentCard {
            agent_id: "test_agent".to_string(),
            agent_type: "task".to_string(),
            description: "test".to_string(),
            accepts: vec!["text".to_string()],
            produces: vec!["text".to_string()],
            capabilities: LocalAgentCapabilities {
                system_prompt: Some("You are a test agent.".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        registry.write_card(&card).expect("write card");
        registry.load().expect("load");
        registry
    }

    /// Write a local agent card that declares `zed/write_file` in its
    /// `mcp_tools` allowlist. Used by the monotone-provenance pipeline test
    /// so the `AgentExecutor` will dispatch the stub tool call.
    fn registry_with_tool_agent(
        dir: &std::path::Path,
    ) -> std::sync::Arc<crate::local_registry::LocalAgentRegistry> {
        let registry = std::sync::Arc::new(crate::local_registry::LocalAgentRegistry::new(
            dir.to_string_lossy().to_string(),
        ));
        let card = LocalAgentCard {
            agent_id: "test_agent".to_string(),
            agent_type: "task".to_string(),
            description: "test".to_string(),
            accepts: vec!["text".to_string()],
            produces: vec!["text".to_string()],
            capabilities: LocalAgentCapabilities {
                system_prompt: Some("You are a test agent.".to_string()),
                mcp_tools: vec!["zed/write_file".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        registry.write_card(&card).expect("write card");
        registry.load().expect("load");
        registry
    }

    /// Build a `SwarmServer` with a sequence stub inference and a succeeding
    /// tool dispatch. Used by the monotone-provenance pipeline test to
    /// produce a `Sourced` field in step 2 (via a successful `zed/write_file`
    /// tool call) that the upstream (step 1) carried as `Unsourced`.
    async fn server_with_sequence_runtime(
        inference: SequenceStubInference,
        tool_dispatch: SucceedingToolDispatch,
    ) -> (SwarmServer, std::sync::Arc<VerificationStore>) {
        let ledger = hkask_ledger::Ledger::from_driver(
            hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
        )
        .expect("ledger");
        let runtime = LocalSwarmRuntime::with_deps(
            ledger,
            std::sync::Arc::new(inference),
            std::sync::Arc::new(tool_dispatch),
            std::sync::Arc::new(StubSkillExec),
        )
        .expect("runtime with stub deps");

        let lazy_runtime = std::sync::Arc::new(crate::local_runtime::LazyLocalSwarmRuntime::lazy(
            "/tmp/hkask-test-ledger.db".to_string(),
            None,
        ));
        lazy_runtime.set_runtime(runtime);

        let verification_store = std::sync::Arc::new(VerificationStore::in_memory());

        let config = SwarmConfig::default();
        let server = SwarmServer::new(
            WebID::for_agent_name("test-operator"),
            std::sync::Arc::new(crate::abw_client::SwarmClient::new(
                reqwest::Client::new(),
                config,
            )),
            std::sync::Arc::new(crate::consent::ConsentStore::default()),
            std::sync::Arc::new(crate::local_registry::LocalAgentRegistry::new(
                "/nonexistent",
            )),
            lazy_runtime,
            std::sync::Arc::new(crate::local_swarms::LocalSwarmRegistry::new("/nonexistent")),
            std::sync::Arc::new(crate::local_knowledge::LazyLocalMemory::lazy(
                "/tmp/never.db".to_string(),
                "short".to_string(),
                1024,
            )),
            verification_store.clone(),
        );
        (server, verification_store)
    }

    /// `swarm_fanout_local` must call `enforce_and_stamp` per-delegation so the
    /// central ledger receives a record with `source: "swarm_fanout_local"`.
    /// Previously this path skipped grounding — delegations ran unrecorded and
    /// the trend query under-counted (the `.rules` Prohibition).
    #[tokio::test]
    async fn swarm_fanout_local_records_grounding_in_the_ledger() {
        let temp_dir = std::env::temp_dir().join("hkask-fanout-grounding-test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let (mut server, verification_store) = server_with_stub_runtime(
            "{\"summary\": \"did the work\", \"approach\": \"directly\"}".to_string(),
        )
        .await;
        server.local_registry = registry_with_agent(&temp_dir);

        let request = Parameters(FanoutLocalRequest {
            delegations: vec![FanoutEntry {
                agent_name: "test_agent".to_string(),
                task: "do the work".to_string(),
                credits_authorized: 10,
            }],
        });
        let _ = server.swarm_fanout_local(request).await;

        let trend = verification_store
            .grounding_trend(&TrendScope::BySource("swarm_fanout_local".to_string()))
            .expect("trend query");
        assert_eq!(
            trend.total_delegations, 1,
            "fanout must record one grounding delegation in the ledger"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// `swarm_pipeline_local` must call `enforce_and_stamp` per-delegation so
    /// the central ledger receives a record with `source: "swarm_pipeline_local"`.
    /// Previously this path skipped grounding — delegations ran unrecorded and
    /// the trend query under-counted (the `.rules` Prohibition).
    #[tokio::test]
    async fn swarm_pipeline_local_records_grounding_in_the_ledger() {
        let temp_dir = std::env::temp_dir().join("hkask-pipeline-grounding-test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let (mut server, verification_store) = server_with_stub_runtime(
            "{\"summary\": \"did the work\", \"approach\": \"directly\"}".to_string(),
        )
        .await;
        server.local_registry = registry_with_agent(&temp_dir);

        let request = Parameters(PipelineLocalRequest {
            steps: vec![PipelineStep {
                agent_name: "test_agent".to_string(),
                task: "do the work".to_string(),
                credits_authorized: 10,
            }],
        });
        let _ = server.swarm_pipeline_local(request).await;

        let trend = verification_store
            .grounding_trend(&TrendScope::BySource("swarm_pipeline_local".to_string()))
            .expect("trend query");
        assert_eq!(
            trend.total_delegations, 1,
            "pipeline must record one grounding delegation in the ledger"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// **Monotone provenance wiring test (paper §6 / .rules).**
    ///
    /// `swarm_pipeline_local` must pass the previous step's envelope `blocks`
    /// as `upstream_blocks` to `enforce_and_stamp` so the weakest-link cap
    /// applies across the composition hop. This is the production-path
    /// falsifier: if the `&prev_upstream_blocks` argument is removed from
    /// the `enforce_and_stamp` call in `swarm_pipeline_local`, this test
    /// fails.
    ///
    /// The laundering case: step 1 emits `deliverable_path` without a tool
    /// call → `enforce_grounding` nulls it as `Unsourced` (strength 0) →
    /// step 1's envelope carries `deliverable_path` as `tool_no_match`.
    /// Step 2 calls `zed/write_file` (which returns `{"path": "/src/main.rs"}`)
    /// and emits `deliverable_path` → `enforce_grounding` tags it `Sourced`
    /// (strength 5). Without the cap, step 2's envelope would carry
    /// `deliverable_path` as `tool_verified`. With the cap, the weakest-link
    /// rule downgrades it to `tool_no_match` (strength 0) — the child cannot
    /// launder the upstream's unsourced field by claiming it sourced it.
    #[tokio::test]
    async fn swarm_pipeline_local_caps_provenance_across_composition_hop() {
        let temp_dir = std::env::temp_dir().join("hkask-pipeline-monotone-test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Step 1 response: text-only, no tool calls. `deliverable_path` is
        // unsourced (no write_file call) → nulled, envelope carries it as
        // `tool_no_match` (strength 0).
        let step1_response = InferenceResult {
            text: r#"{"deliverable_path": "/src/main.rs", "summary": "step 1"}"#.to_string(),
            model: "stub-model".to_string(),
            usage: InferenceUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            finish_reason: "stop".to_string(),
            token_probabilities: None,
            tool_calls: vec![],
            reasoning: None,
            cost_usd: None,
        };

        // Step 2, round 1: the model requests a `zed/write_file` tool call.
        // The stub tool dispatch succeeds and returns `{"path": "/src/main.rs"}`.
        let step2_tool_call = InferenceResult {
            text: String::new(),
            model: "stub-model".to_string(),
            usage: InferenceUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            finish_reason: "tool_calls".to_string(),
            token_probabilities: None,
            tool_calls: vec![hkask_types::StructuredToolCall {
                server: "zed".to_string(),
                tool: "zed/write_file".to_string(),
                args: serde_json::json!({"path": "/src/main.rs"}),
                call_id: None,
            }],
            reasoning: None,
            cost_usd: None,
        };

        // Step 2, round 2: the model produces the final text with
        // `deliverable_path`. `enforce_grounding` would tag it `Sourced`
        // (strength 5) because the write_file tool call succeeded and the
        // path appears in its result.
        let step2_final = InferenceResult {
            text: r#"{"deliverable_path": "/src/main.rs", "summary": "step 2"}"#.to_string(),
            model: "stub-model".to_string(),
            usage: InferenceUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            finish_reason: "stop".to_string(),
            token_probabilities: None,
            tool_calls: vec![],
            reasoning: None,
            cost_usd: None,
        };

        let inference =
            SequenceStubInference::new(vec![step1_response, step2_tool_call, step2_final]);
        let tool_dispatch = SucceedingToolDispatch {
            qualified_tool: "zed/write_file".to_string(),
            result: serde_json::json!({"path": "/src/main.rs"}),
        };

        let (mut server, _verification_store) =
            server_with_sequence_runtime(inference, tool_dispatch).await;
        server.local_registry = registry_with_tool_agent(&temp_dir);

        let request = Parameters(PipelineLocalRequest {
            steps: vec![
                PipelineStep {
                    agent_name: "test_agent".to_string(),
                    task: "do step 1".to_string(),
                    credits_authorized: 10,
                },
                PipelineStep {
                    agent_name: "test_agent".to_string(),
                    task: "do step 2 with {prev_output}".to_string(),
                    credits_authorized: 10,
                },
            ],
        });
        let result = server.swarm_pipeline_local(request).await;
        let result_json: serde_json::Value =
            serde_json::from_str(&result).expect("pipeline result is valid JSON");
        // The MCP tool response is wrapped in a `{"content": ...}` envelope.
        let content = &result_json["content"];
        let step2_result = &content["results"][1];
        assert!(
            step2_result.get("ok").is_some(),
            "step 2 must have an `ok` field"
        );
        // The envelope carries the `blocks` array. Find the `deliverable_path`
        // block and verify its provenance was capped.
        let envelope = step2_result
            .get("envelope")
            .expect("step 2 must carry an envelope");
        let blocks = envelope
            .get("blocks")
            .and_then(|b| b.as_array())
            .expect("envelope must carry a blocks array");
        let deliverable_block = blocks
            .iter()
            .find(|b| b.get("field").and_then(|f| f.as_str()) == Some("deliverable_path"))
            .expect("envelope must carry a deliverable_path block");
        let provenance = deliverable_block
            .get("provenance")
            .and_then(|p| p.as_str())
            .expect("deliverable_path block must carry a provenance stamp");
        assert_eq!(
            provenance, "unavailable_no_tool_source",
            "step 2's deliverable_path must be capped to unavailable_no_tool_source (strength 0) — \
             the upstream (step 1) carried it as Unsourced, so the child cannot \
             launder it by claiming Sourced. Got '{provenance}' — the monotone-provenance \
             cap is not wired in swarm_pipeline_local."
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
