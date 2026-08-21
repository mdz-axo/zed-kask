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

/// Task-set cap for `swarm_eval_agent_local`. Bounds one harness call's
/// breadth; the total-rollout cap below bounds its depth × breadth.
pub const MAX_EVAL_TASKS: usize = 10;

/// Default and cap for `repeats` in `swarm_eval_agent_local`. The default (3)
/// is enough to expose a hard 0-or-1 failure mode; the cap keeps a single
/// call's cost bounded.
pub const DEFAULT_EVAL_REPEATS: u32 = 3;
pub const MAX_EVAL_REPEATS: u32 = 10;

/// Total rollouts (tasks × repeats) cap for `swarm_eval_agent_local`. Each
/// rollout is a real inference call with real token cost, so the product —
/// not just the factors — needs a ceiling.
pub const MAX_EVAL_ROLLOUTS: usize = 50;
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
async fn run_evaluator(response: &str, evaluator: &str, spec: &str) -> Result<bool, McpToolError> {
    match evaluator {
        "contains" => Ok(response.contains(spec)),
        "not_contains" => Ok(!response.contains(spec)),
        "regex" => {
            let re = regex::Regex::new(spec)
                .map_err(|e| McpToolError::invalid_argument(format!("invalid regex spec: {e}")))?;
            Ok(re.is_match(response))
        }
        // External ground-truth evaluators (Goodhart mitigation per the
        // LLM-vs-LLM rule). The string-match evaluators above are the
        // scoring function of a training loop — adapters can learn to game
        // them by emitting the expected substring without solving the
        // task. `exit_code` and `file_exists` check real-world effects,
        // not response text, so gaming requires actually doing the work.
        //
        // The spec is trusted (operator-provided in the task definition),
        // so running commands is acceptable in this context.
        "exit_code" => {
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(spec)
                .env("RESPONSE", response)
                .output()
                .await
                .map_err(|e| {
                    McpToolError::internal(format!(
                        "exit_code evaluator failed to run command: {e}"
                    ))
                })?;
            Ok(output.status.success())
        }
        "file_exists" => Ok(std::path::Path::new(spec).exists()),
        other => Err(McpToolError::invalid_argument(format!(
            "evaluator must be 'contains', 'not_contains', 'regex', \
             'exit_code', or 'file_exists'; got '{other}'"
        ))),
    }
}

/// Build the per-task report for one `swarm_eval_agent_local` task. Pure —
/// takes the counted outcomes, returns the JSON entry. Extracted so the
/// pass-rate and standard-error math is unit-testable without inference.
fn eval_task_report(
    task: &EvalAgentTask,
    passes: usize,
    errors: usize,
    latencies_ms: &[u64],
) -> serde_json::Value {
    let attempts = passes + errors;
    let pass_rate = if attempts == 0 {
        // `repeats >= 1` is enforced by the caller, so attempts == 0 is
        // unreachable; guard anyway rather than divide by zero.
        f64::NAN
    } else {
        passes as f64 / attempts as f64
    };
    // Standard error of a proportion: sqrt(p(1-p)/n). With small n this is
    // wide — that is the point: a pass rate without variance is noise
    // (non-determinism risk in the event-substrate proposal).
    let std_error = if attempts > 1 {
        (pass_rate * (1.0 - pass_rate) / attempts as f64).sqrt()
    } else {
        f64::NAN
    };
    let mean_latency_ms = if latencies_ms.is_empty() {
        None
    } else {
        Some(latencies_ms.iter().sum::<u64>() / latencies_ms.len() as u64)
    };
    serde_json::json!({
        "task": task.task,
        "evaluator": task.evaluator.evaluator,
        "spec": task.evaluator.spec,
        "repeats": attempts,
        "passes": passes,
        "errors": errors,
        "pass_rate": pass_rate,
        "pass_rate_std_error": std_error,
        "mean_latency_ms": mean_latency_ms,
    })
}

#[tool_router(router = local_router, vis = "pub")]
impl SwarmServer {
    /// Delegate a task to a local agent. The agent must exist in the local
    /// registry (`agents/local/curated/<id>/agent_card.json`). The task is
    /// executed via `hkask-inference`. When the
    /// agent's card declares `capabilities.mcp_tools` (qualified
    /// `server/tool` names), those tools are declared to the model and model
    /// tool calls are dispatched through the zed IPC bridge's governed
    /// `McpRuntime` — the declared list is the allowlist.
    /// Spend is recorded per token across all tool-loop rounds
    /// (1 credit / 1000 tokens, capped at `credits_authorized`).
    ///
    /// **No funding gate and no consent token.** Local agents run on the
    /// operator's own substrate, so there is nothing to authorize — an unfunded
    /// ledger does not block this call and the balance may go negative
    /// (accumulated local spend). `credits_authorized` still caps the *recorded*
    /// cost, and the per-dispatch ceiling still bounds a single runaway dispatch.
    #[tool(
        description = "Delegate a task to a local agent (from agents/local/curated/). Executes via hkask-inference (Ollama/cloud) and records spend in the local ledger per token. Agents may declare capabilities.mcp_tools (qualified server/tool names) — those tools are dispatched through the zed IPC bridge's governed McpRuntime (allowlisted to the declared set). No ABW calls. NO funding gate and no consent token — an unfunded ledger does not block this call; the ledger records spend rather than authorizing it. Returns the response, model, token usage, cost, resulting balance (may be negative), and tool_calls summary."
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
            // `accepts` labels. `None` = no accepts declared or non-text label
            // (absence ≠ contradiction, paper Rule 5.3). `Some(true)` = the
            // agent declares `accepts: ["text"]` (universal accept). The
            // classification heuristic was deleted — the typing layer at
            // admission (`validate_typing`) is the gate.
            let bind_matched = crate::local_runtime::check_bind(&agent, &req.task);
            // Execute via the local runtime.
            let ceiling = self.client.config().max_credits_per_dispatch;
            let mut result = runtime
                .delegate(&agent, &req.task, req.credits_authorized, ceiling)
                .await
                .map_err(map_local_swarm_error)?;
            // Evaluator contract (phase 4): a card-declared evaluator is the
            // agent's own oracle. Run each; the verdict passes only if ALL
            // declared evaluators pass (they are conjunctive expectations
            // about the response, not alternatives). A bad evaluator spec
            // propagates as an error rather than stamping `pass: false` —
            // the agent must not be blamed for a broken oracle.
            if !agent.capabilities.evaluators.is_empty() {
                let mut all_passed = true;
                let mut detail_parts = Vec::new();
                for declared in &agent.capabilities.evaluators {
                    let passed = run_evaluator(
                        &result.response,
                        &declared.evaluator,
                        &declared.spec,
                    )
                    .await?;
                    detail_parts.push(format!(
                        "evaluator={}, spec_len={}, pass={}",
                        declared.evaluator,
                        declared.spec.len(),
                        passed
                    ));
                    if !passed {
                        all_passed = false;
                    }
                }
                result.task_success = Some(crate::local_runtime::TaskSuccessVerdict {
                    pass: all_passed,
                    score: None,
                    detail: Some(detail_parts.join("; ")),
                    provenance: crate::local_runtime::VerdictSource::DeterministicEvaluator,
                });
            }
            // Stamp the bind check result onto the delegation result.
            result.bind_matched = bind_matched;
            // Rung 2 (Typing) post-invocation: validate the agent's output
            // against the schema for its `produces` port type (paper's "one
            // artifact, two uses").
            self.validate_produces(&req.agent_name, &agent.produces, &result.response);
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
            // Episodic turn memory (the shared knowledgebase): store the FULL
            // turn (task + response + model) as one h_mem plus an embedding of
            // the task, so the turn is retrievable by `swarm_recall_local` via
            // semantic similarity across all swarms. `record_delegation` above
            // is the stigmergy trail (fitness); this is the experience record
            // (knowledge). Failures are logged (non-fatal) — the delegation
            // result is returned regardless.
            local_knowledge::ingest_turn(
                &self.local_memory,
                &runtime.inference(),
                &req.agent_name,
                &req.task,
                &result.response,
                &result.model,
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
                        Ok(r) => {
                            self.validate_produces(&entry.agent_name, &agent.produces, &r.response);
                            // Episodic turn memory (shared knowledgebase) — mirrors
                            // swarm_delegate_local so fan-out delegations build
                            // the KB too. Non-fatal.
                            local_knowledge::ingest_turn(
                                &self.local_memory,
                                &runtime.inference(),
                                &entry.agent_name,
                                &entry.task,
                                &r.response,
                                &r.model,
                            )
                            .await;
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
                        Ok(r) => {
                            self.validate_produces(&step.agent_name, &agent.produces, &r.response);
                            // Episodic turn memory (shared knowledgebase) —
                            // mirrors swarm_delegate_local so pipeline steps
                            // build the KB too. `task` carries the
                            // {prev_output}-substituted prompt the agent actually
                            // received, so the recorded turn is the real input.
                            // Non-fatal.
                            local_knowledge::ingest_turn(
                                &self.local_memory,
                                &runtime.inference(),
                                &step.agent_name,
                                &task,
                                &r.response,
                                &r.model,
                            )
                            .await;
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
                    sample_queries: Vec::new(),
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
                        evaluators: req.evaluators.unwrap_or_default(),
                    },
                    cloud_swarm_id: None,
                    tags: req.tags,
                    sample_queries: req.sample_queries,
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
    /// spends nothing. Uses the inference port directly to generate composition
    /// guidance from the form fields.
    #[tool(
        description = "AI assist for the swarm panel authoring forms (agent/swarm). Suggests completions for partial inputs or validates well-formedness. Authoring aid — read-only, spends nothing. Uses the inference port directly to generate composition guidance. The mode field (abw/local) tailors the guidance; no ABW calls in either mode."
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

            // Serialize the form fields as a JSON object string for the
            // inference prompt. The LLM generates composition guidance from
            // these fields directly.
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
                "tags": req.tags,
                "sample_queries": req.sample_queries,
                "accepts": req.accepts,
                "produces": req.produces,
                "has_valence": req.has_valence,
            }))
            .map_err(|e| {
                map_local_swarm_error(LocalSwarmError::Unavailable(format!(
                    "failed to serialize ai-assist task: {e}"
                )))
            })?;

            // Validate runs the deterministic ABW contract FIRST (the fermi
            // `agent_contract` requirement table, ported to `contract.rs`),
            // then asks the LLM for advisory warnings. The contract decides
            // `valid`; the LLM can never flip it — the same Error/Warning
            // split as fermi's publish pipeline. If inference fails, the
            // deterministic verdict still returns (an inference outage must
            // not read as "the agent is malformed").
            if req.action == "validate" {
                let mut payload = if req.surface == "agent" {
                    let input = crate::contract::AgentContractInput {
                        name: req.name.clone(),
                        description: req.description.clone(),
                        system_prompt: req.system_prompt.clone(),
                        tags: crate::contract::split_csv(&req.tags),
                        sample_queries: crate::contract::split_lines(&req.sample_queries),
                        accepts: crate::contract::split_csv(&req.accepts),
                        produces: crate::contract::split_csv(&req.produces),
                        has_valence: req.has_valence,
                        mode: req.mode.clone(),
                    };
                    crate::contract::checks_to_payload(
                        &crate::contract::agent_contract_checks(&input),
                    )
                } else {
                    let input = crate::contract::SwarmContractInput {
                        name: req.name.clone(),
                        mission: req.mission.clone(),
                        agents: crate::contract::split_csv(&req.agents),
                        mode: req.mode.clone(),
                    };
                    crate::contract::checks_to_payload(
                        &crate::contract::swarm_contract_checks(&input),
                    )
                };

                // Advisory layer: the LLM reviews the same fields for what a
                // table cannot judge (prompt quality, role overlap, mission
                // fit). Its findings are appended to `warnings` — never to
                // `issues` — and an inference failure is reported as a note
                // while the deterministic verdict stands.
                match self.ai_assist_advisory(&json_task).await {
                    Ok(advisory) => {
                        if let Some(warnings) = payload["warnings"].as_array_mut() {
                            for issue in advisory {
                                warnings.push(serde_json::Value::String(issue));
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            "swarm_ai_assist advisory layer failed — deterministic contract verdict stands: {err}"
                        );
                        payload["notes"] = serde_json::Value::String(format!(
                            "Advisory review unavailable: {err}. The contract checks above \
                             are deterministic and unaffected."
                        ));
                    }
                }
                payload["action"] = serde_json::Value::String(req.action.clone());
                payload["surface"] = serde_json::Value::String(req.surface.clone());
                payload["mode"] = serde_json::Value::String(req.mode.clone());
                payload["suggestions"] = serde_json::Value::Null;
                return Ok(payload);
            }

            // Use the inference port directly for AI-assisted composition guidance.
            let runtime = self
                .local_runtime
                .get_or_init()
                .await
                .map_err(map_local_swarm_error)?;
            let inference = runtime.inference();
            let result = inference
                .generate(
                    &format!("You are an expert at composing AI agent teams. Based on the following request, generate a JSON response with suggested agent/swarm configuration:\n\n{}", json_task),
                    &hkask_types::LLMParameters::default(),
                    None,
                )
                .await
                .map_err(|e| {
                    map_local_swarm_error(LocalSwarmError::Unavailable(format!(
                        "AI assist inference failed: {e}"
                    )))
                })?;
            let text = result.text;

            let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
            // Only "suggest" reaches here — validate returned early via the
            // deterministic contract path above.
            let result = match parsed {
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
                        "swarm_ai_assist (suggest) output was not valid JSON — returning raw text in notes"
                    );
                    serde_json::json!({
                        "action": req.action,
                        "surface": req.surface,
                        "mode": req.mode,
                        "suggestions": serde_json::json!({
                            "name": "", "agent_type": "", "description": "",
                            "system_prompt": "", "mission": "", "agents": "",
                        }),
                        "valid": serde_json::Value::Null,
                        "issues": serde_json::json!([]),
                        "notes": text,
                    })
                }
            };
            Ok(result)
        })
        .await
    }

    /// The advisory layer for `swarm_ai_assist` validate: one inference call
    /// over the serialized form fields, returning advisory warnings only.
    /// The caller appends these to `warnings` — they can never flip `valid`.
    async fn ai_assist_advisory(&self, json_task: &str) -> Result<Vec<String>, LocalSwarmError> {
        let runtime = self.local_runtime.get_or_init().await?;
        let inference = runtime.inference();
        let result = inference
            .generate(
                &format!(
                    "You are reviewing an AI agent or swarm composition form for \
                     quality issues a checklist cannot judge: prompt coherence, role \
                     overlap in the roster, mission fit, missing methodology steps. \
                     The deterministic contract (names, required fields, roster size) \
                     has already been checked — do NOT repeat those. Return ONLY a JSON \
                     array of short advisory strings (empty array if nothing to add):\n\n{}",
                    json_task
                ),
                &hkask_types::LLMParameters::default(),
                None,
            )
            .await
            .map_err(|e| {
                LocalSwarmError::Unavailable(format!("AI assist inference failed: {e}"))
            })?;
        let text = result.text.trim();
        let parsed = serde_json::from_str::<serde_json::Value>(text).map_err(|e| {
            LocalSwarmError::Unavailable(format!("advisory output was not a JSON array: {e}"))
        })?;
        Ok(parsed
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Deterministic task-success evaluator. The Curator (or a human) calls
    /// this after `swarm_delegate_local` to stamp a `TaskSuccessVerdict` with
    /// `provenance: DeterministicEvaluator` onto the delegation result. This is the
    /// enforcement point for the C5/C6 fault-attribution loop: ORIENT's
    /// highest-fidelity fault signal (rule 1: per-delegation task failure)
    /// requires a deterministic `task_success` verdict — an LLM-judged verdict
    /// is downgraded by ORIENT (Gap S3). No ABW calls, no ledger spend —
    /// evaluation is free.
    #[tool(
        description = "Deterministic task-success evaluator for local swarm delegations. Takes an agent's response and a deterministic check (contains / not_contains / regex / exit_code / file_exists) and returns a TaskSuccessVerdict with provenance: DeterministicEvaluator. The Curator calls this after swarm_delegate_local to stamp task_success for the C5/C6 fault-attribution loop. No ABW calls, no ledger spend — evaluation is free."
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
                let pass = run_evaluator(&req.response, &req.evaluator, &req.spec).await?;
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
                    provenance: crate::local_runtime::VerdictSource::DeterministicEvaluator,
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
                                let pass =
                                    run_evaluator(&r.response, &ev.evaluator, &ev.spec).await?;
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
                                        crate::local_runtime::VerdictSource::DeterministicEvaluator,
                                });
                            }
                            // Record stigmergy (same as swarm_delegate_local).
                            self.validate_produces(&entry.agent_name, &agent.produces, &r.response);
                            local_knowledge::record_delegation(
                                &self.local_memory,
                                &entry.agent_name,
                                r.latency_ms,
                                r.task_success.as_ref().map(|t| t.pass),
                                &r.response,
                            )
                            .await;
                            // Episodic turn memory (shared knowledgebase) —
                            // mirrors swarm_delegate_local so plan-executed
                            // delegations build the KB too. Non-fatal.
                            local_knowledge::ingest_turn(
                                &self.local_memory,
                                &runtime.inference(),
                                &entry.agent_name,
                                &entry.task,
                                &r.response,
                                &r.model,
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

    /// The rollout harness (event-substrate): run one agent card against a
    /// task set N times each, stamp deterministic verdicts, and report
    /// per-task pass rates with standard error. Each rollout appends a
    /// `model_request` event and a `verdict` event to the event store —
    /// the harness is the store's first writer.
    ///
    /// Rollouts run sequentially (the local ledger is single-writer, same
    /// constraint as fanout/plan). A delegation error counts as a failed
    /// rollout for that task — the harness measures end-to-end pass rate,
    /// which includes crashes, not just wrong answers.
    #[tool(
        description = "Rollout harness: run one local agent against a task set N times each, evaluate each rollout with a deterministic evaluator (contains/not_contains/regex/exit_code/file_exists), and report per-task pass rates with standard error and totals. Each rollout is recorded as model_request + verdict events in the event store. Tasks capped at 10, repeats at 10, total rollouts at 50. After each run, old model_request bodies are stripped and very old rollouts are compacted (bodies_stripped/rollouts_compacted in the report)."
    )]
    pub(crate) async fn swarm_eval_agent_local(
        &self,
        parameters: Parameters<EvalAgentLocalRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "swarm_eval_agent_local",
            Some(hkask_bridge_ontology::pko::PROCEDURE),
            async {
                let req = parameters.0;
                if req.agent_name.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "agent_name must be non-empty".to_string(),
                    ));
                }
                if req.tasks.is_empty() || req.tasks.len() > MAX_EVAL_TASKS {
                    return Err(McpToolError::invalid_argument(format!(
                        "tasks must contain 1..={MAX_EVAL_TASKS} entries, got {}",
                        req.tasks.len()
                    )));
                }
                let repeats = req.repeats.unwrap_or(DEFAULT_EVAL_REPEATS);
                if repeats == 0 || repeats > MAX_EVAL_REPEATS {
                    return Err(McpToolError::invalid_argument(format!(
                        "repeats must be 1..={MAX_EVAL_REPEATS}, got {repeats}"
                    )));
                }
                let total_rollouts = req.tasks.len().saturating_mul(repeats as usize);
                if total_rollouts > MAX_EVAL_ROLLOUTS {
                    return Err(McpToolError::invalid_argument(format!(
                        "total rollouts (tasks × repeats = {total_rollouts}) exceeds the cap of \
                         {MAX_EVAL_ROLLOUTS} — each rollout is a real inference call with real \
                         token cost"
                    )));
                }
                // Validate every evaluator spec upfront: a bad regex must fail
                // the whole call before any tokens are spent, not halfway
                // through the run.
                for (index, task) in req.tasks.iter().enumerate() {
                    if task.task.trim().is_empty() {
                        return Err(McpToolError::invalid_argument(format!(
                            "tasks[{index}].task must be non-empty"
                        )));
                    }
                    run_evaluator("", &task.evaluator.evaluator, &task.evaluator.spec).await?;
                }
                let runtime = self
                    .local_runtime
                    .get_or_init()
                    .await
                    .map_err(map_local_swarm_error)?;
                let agent = self.local_registry.get(&req.agent_name).ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "agent '{}' not found in local registry — load agents from \
                         agents/local/curated/<id>/agent_card.json",
                        req.agent_name
                    ))
                })?;
                let ceiling = self.client.config().max_credits_per_dispatch;
                // The event store is the data plane. The harness wires the
                // executor's capture path (phase 3) so every inference call
                // emits a model_request event automatically, then stamps the
                // verdict under the executor-assigned rollout id. A store
                // failure is logged and counted (`events_dropped`), never
                // swallowed — the report must distinguish "recorded" from
                // "record lost". An OPEN failure is warned with the path
                // named (the failure-signal rule: "not configured" must be
                // distinguishable from "configured but broken") and the run
                // proceeds uncaptured — eval does not depend on the store.
                let event_store = match self.event_store.get_or_init() {
                    Ok(store) => Some(store),
                    Err(error) => {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            error = %error,
                            "event store open failed — harness run proceeds uncaptured"
                        );
                        None
                    }
                };
                if let Some(store) = &event_store {
                    runtime.wire_capture(std::sync::Arc::clone(store));
                }
                let mut events_dropped = 0usize;
                let harness_run_id = format!("harness-{}-{}", req.agent_name, uuid::Uuid::new_v4());

                let mut task_reports = Vec::with_capacity(req.tasks.len());
                let mut total_passes = 0usize;
                let mut total_cost = 0i64;
                let mut total_cost_uncapped = 0i64;
                let mut total_tokens = 0i64;
                for (task_index, task) in req.tasks.iter().enumerate() {
                    let mut passes = 0usize;
                    let mut errors = 0usize;
                    let mut latencies_ms: Vec<u64> = Vec::with_capacity(repeats as usize);
                    for repeat_index in 0..repeats {
                        match runtime
                            .delegate(&agent, &task.task, task.credits_authorized, ceiling)
                            .await
                        {
                            Ok(result) => {
                                total_cost += result.cost;
                                total_cost_uncapped += result.cost_uncapped;
                                total_tokens += result.tokens_used;
                                latencies_ms.push(result.latency_ms);
                                // The evaluator is deterministic, so a pass
                                // here is a real verdict, not a sample.
                                let passed = run_evaluator(
                                    &result.response,
                                    &task.evaluator.evaluator,
                                    &task.evaluator.spec,
                                )
                                .await?;
                                if passed {
                                    passes += 1;
                                }
                                // The verdict lands under the executor-assigned
                                // rollout id so it groups with the
                                // model_request events the capture path
                                // already appended for this delegation.
                                if let (Some(store), Some(rollout_id)) =
                                    (&event_store, result.rollout_id.as_ref())
                                {
                                    let verdict = serde_json::json!({
                                        "pass": passed,
                                        "source": hkask_event_store::VerdictSource::DeterministicEvaluator.as_str(),
                                        "rollout_kind": hkask_event_store::RolloutKind::Delegation.as_str(),
                                        "evaluator": task.evaluator.evaluator,
                                        "spec_len": task.evaluator.spec.len(),
                                        "harness_run_id": harness_run_id,
                                        "task_index": task_index,
                                        "repeat_index": repeat_index,
                                    });
                                    if let Err(error) =
                                        store.append(rollout_id, "verdict", &verdict)
                                    {
                                        events_dropped += 1;
                                        tracing::warn!(
                                            target: "hkask.mcp.swarm",
                                            rollout = %rollout_id,
                                            error = %error,
                                            "event store append failed — verdict not recorded"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                // A crashed rollout is a failed rollout: the
                                // pass rate measures end-to-end reliability,
                                // which includes crashes.
                                errors += 1;
                                tracing::warn!(
                                    target: "hkask.mcp.swarm",
                                    agent = %req.agent_name,
                                    error = %e,
                                    "rollout failed during eval harness run"
                                );
                            }
                        }
                    }
                    total_passes += passes;
                    task_reports.push(eval_task_report(task, passes, errors, &latencies_ms));
                }
                let overall_pass_rate = total_passes as f64 / total_rollouts as f64;
                let balance: Option<i64> = runtime.balance();
                // Both drop counters, surfaced: verdict-append failures from
                // this loop, capture drops (send-side backpressure +
                // drainer-side append failures) from the runtime. A drop is
                // never silent.
                let capture_drops = runtime.capture_drops();
                // Write a harness_summary event so the zed-side regression
                // monitor can compare pass rates across runs for the same
                // agent. The rollout_id is the agent name — this groups all
                // harness_summary events for one agent under a single
                // queryable key, so `metric_before_and_after` can find the
                // before/after values across runs. A write failure is counted
                // in `events_dropped` — never silent (the failure-signal rule:
                // a missing summary means the regression monitor is blind,
                // which must be distinguishable from "no run happened").
                if let Some(store) = &event_store {
                    let summary = serde_json::json!({
                        "agent_name": req.agent_name,
                        "harness_run_id": harness_run_id,
                        "overall_pass_rate": overall_pass_rate,
                        "total_rollouts": total_rollouts,
                        "total_passes": total_passes,
                        "rollout_kind": hkask_event_store::RolloutKind::HarnessRun.as_str(),
                    });
                    if let Err(error) = store.append(&req.agent_name, "harness_summary", &summary) {
                        events_dropped += 1;
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            agent = %req.agent_name,
                            error = %error,
                            "harness_summary event append failed — regression monitor blind to this run"
                        );
                    }
                }
                // Compaction caller (event-substrate item 2): after each
                // harness run, strip bodies from old model_request events
                // and drop very old rollouts. The training bridge has had
                // its chance by now (operator-triggered, runs between harness
                // calls); bodies from previous runs are bulk with no remaining
                // consumer. Both counts are surfaced — never silent (the
                // .rules failure-signal rule: a missing count means "nothing
                // old enough to strip/drop", which must be distinguishable
                // from "compaction failed").
                //
                // Body retention: default 1 hour (configurable via
                // `HKASK_SWARM_BODY_RETENTION_HOURS`). Rollout retention:
                // default 7 days (configurable via
                // `HKASK_SWARM_ROLLOUT_RETENTION_DAYS`).
                let (bodies_stripped, rollouts_compacted) = if let Some(store) = &event_store {
                    let body_retention_hours = std::env::var("HKASK_SWARM_BODY_RETENTION_HOURS")
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(1);
                    let body_cutoff = (chrono::Utc::now()
                        - chrono::Duration::hours(body_retention_hours))
                        .to_rfc3339();
                    let stripped = match store.strip_bodies(&body_cutoff) {
                        Ok(n) => n,
                        Err(error) => {
                            tracing::warn!(
                                target: "hkask.mcp.swarm",
                                error = %error,
                                "strip_bodies failed after harness run"
                            );
                            0
                        }
                    };
                    let rollout_retention_days = std::env::var("HKASK_SWARM_ROLLOUT_RETENTION_DAYS")
                        .ok()
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(7);
                    let rollout_cutoff = (chrono::Utc::now()
                        - chrono::Duration::days(rollout_retention_days))
                        .to_rfc3339();
                    let compacted = match store.compact(&rollout_cutoff) {
                        Ok(n) => n,
                        Err(error) => {
                            tracing::warn!(
                                target: "hkask.mcp.swarm",
                                error = %error,
                                "compact failed after harness run"
                            );
                            0
                        }
                    };
                    (stripped, compacted)
                } else {
                    (0, 0)
                };
                Ok(serde_json::json!({
                    "agent_name": req.agent_name,
                    "harness_run_id": harness_run_id,
                    "tasks": task_reports,
                    "total_rollouts": total_rollouts,
                    "total_passes": total_passes,
                    "overall_pass_rate": overall_pass_rate,
                    "total_cost": total_cost,
                    "total_cost_uncapped": total_cost_uncapped,
                    "total_tokens": total_tokens,
                    "balance": balance,
                    "events_dropped": events_dropped,
                    "capture_drops": capture_drops,
                    "bodies_stripped": bodies_stripped,
                    "rollouts_compacted": rollouts_compacted,
                }))
            },
        )
        .await
    }

    /// Rung 2 (Typing) post-invocation: validate the agent's output against
    /// the schema for its `produces` port type (paper's "one artifact, two
    /// uses"). Returns `Some(ValidationResult)` when the agent declares a
    /// `produces` port, `None` otherwise.
    pub(crate) fn validate_produces(
        &self,
        agent_id: &str,
        produces: &[String],
        response: &str,
    ) -> Option<crate::schema_validate::StatusValidationResult> {
        if produces.is_empty() {
            return None;
        }
        let cleaned: serde_json::Value =
            serde_json::from_str(response).unwrap_or(serde_json::Value::Null);
        let val = self
            .local_registry
            .port_registry()
            .validate_output(produces, &cleaned);
        if val.status != crate::schema_validate::ValidationStatus::Valid
            && val.status != crate::schema_validate::ValidationStatus::NoSchema
        {
            tracing::warn!(
                target: "hkask.swarm.port_registry",
                agent = %agent_id,
                produces = ?produces,
                status = ?val.status,
                violations = ?val.violations,
                "Port schema validation failed — agent output does not match its declared produces schema"
            );
        }
        Some(val)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_types::{EvalAgentLocalRequest, EvalAgentTask, PlanEvaluator};

    fn task(text: &str, evaluator: &str, spec: &str) -> EvalAgentTask {
        EvalAgentTask {
            task: text.to_string(),
            credits_authorized: 10,
            evaluator: PlanEvaluator {
                evaluator: evaluator.to_string(),
                spec: spec.to_string(),
            },
        }
    }

    #[test]
    fn eval_task_report_computes_pass_rate_and_std_error() {
        let t = task("summarize", "contains", "done");
        let report = eval_task_report(&t, 2, 2, &[100, 200, 300, 500]);
        assert_eq!(report["repeats"], 4);
        assert_eq!(report["passes"], 2);
        assert_eq!(report["errors"], 2);
        assert_eq!(report["pass_rate"].as_f64().unwrap(), 0.5);
        // sqrt(0.5 * 0.5 / 4) = 0.25
        assert_eq!(report["pass_rate_std_error"].as_f64().unwrap(), 0.25);
        assert_eq!(report["mean_latency_ms"], 275);
    }

    #[test]
    fn eval_task_report_counts_errors_as_failures() {
        // A crashed rollout is a failed rollout: 1 pass + 2 errors = 1/3.
        let t = task("t", "contains", "x");
        let report = eval_task_report(&t, 1, 2, &[50]);
        assert_eq!(report["repeats"], 3);
        let rate = report["pass_rate"].as_f64().unwrap();
        assert!((rate - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn eval_task_report_single_attempt_has_no_std_error() {
        // n = 1: the standard error of one observation is undefined, not 0 —
        // 0 would claim certainty the sample cannot support.
        let t = task("t", "contains", "x");
        let report = eval_task_report(&t, 1, 0, &[42]);
        assert_eq!(report["pass_rate"], 1.0);
        assert!(report["pass_rate_std_error"].is_null());
    }

    #[test]
    fn eval_task_report_zero_attempts_is_nan_not_zero() {
        // Unreachable via the tool (repeats >= 1 enforced), but the pure
        // function must not fabricate a 0.0 pass rate if ever reached.
        let t = task("t", "contains", "x");
        let report = eval_task_report(&t, 0, 0, &[]);
        assert!(report["pass_rate"].is_null());
        assert!(report["mean_latency_ms"].is_null());
    }

    #[tokio::test]
    async fn run_evaluator_rejects_unknown_kind() {
        let err = run_evaluator("resp", "jsonpath", "$.x").await.unwrap_err();
        assert!(err.to_string().contains("jsonpath"));
    }

    #[tokio::test]
    async fn run_evaluator_contains_and_regex() {
        assert!(
            run_evaluator("hello world", "contains", "world")
                .await
                .unwrap()
        );
        assert!(
            !run_evaluator("hello world", "not_contains", "world")
                .await
                .unwrap()
        );
        assert!(run_evaluator("a1b2", "regex", "[0-9]").await.unwrap());
        // Invalid regex must error, not stamp pass:false (false fault
        // attribution — the agent would be blamed for a bad evaluator).
        assert!(run_evaluator("x", "regex", "(").await.is_err());
    }

    #[test]
    fn eval_request_defaults_and_caps() {
        // Deserialization-level contract: repeats is optional, tasks carry a
        // required evaluator (an unmeasurable task is rejected at the schema,
        // not silently counted as neither pass nor fail).
        let raw = serde_json::json!({
            "agent_name": "researcher",
            "tasks": [{
                "task": "do the thing",
                "credits_authorized": 5,
                "evaluator": { "evaluator": "contains", "spec": "ok" }
            }]
        });
        let req: EvalAgentLocalRequest = serde_json::from_value(raw).unwrap();
        assert_eq!(req.repeats, None);
        assert_eq!(req.tasks.len(), 1);
        assert_eq!(req.tasks[0].evaluator.evaluator, "contains");
        // A task without an evaluator must fail to deserialize.
        let bad = serde_json::json!({
            "agent_name": "researcher",
            "tasks": [{ "task": "t", "credits_authorized": 5 }]
        });
        assert!(serde_json::from_value::<EvalAgentLocalRequest>(bad).is_err());
    }

    #[tokio::test]
    async fn run_evaluator_exit_code_passes_on_zero_exit() {
        // `true` always exits 0 — the evaluator must report pass.
        assert!(
            run_evaluator("anything", "exit_code", "true")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_exit_code_fails_on_nonzero_exit() {
        // `false` always exits 1 — the evaluator must report fail, not error.
        assert!(
            !run_evaluator("anything", "exit_code", "false")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_exit_code_receives_response_env() {
        // The command can access $RESPONSE — this is how external ground-truth
        // checks validate the agent's actual output rather than gaming it.
        assert!(
            run_evaluator("hello", "exit_code", "test \"$RESPONSE\" = hello")
                .await
                .unwrap()
        );
        assert!(
            !run_evaluator("wrong", "exit_code", "test \"$RESPONSE\" = hello")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_exit_code_fails_for_nonexistent_command() {
        // A nonexistent command makes sh exit 127 — a non-zero exit. The
        // evaluator must report Ok(false) (task did not pass), not Err
        // (sh ran fine; the command it was told to run didn't exist). Err
        // is reserved for when sh itself cannot be spawned (system failure).
        assert!(
            !run_evaluator("x", "exit_code", "/nonexistent/binary_xyz")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_file_exists_checks_real_filesystem() {
        // A real file that exists in the test environment.
        let path = std::env::temp_dir().join("hkask_eval_test_file_exists");
        std::fs::write(&path, "test").unwrap();
        assert!(
            run_evaluator("x", "file_exists", path.to_str().unwrap())
                .await
                .unwrap()
        );
        std::fs::remove_file(&path).unwrap();
        // After deletion, the file no longer exists.
        assert!(
            !run_evaluator("x", "file_exists", path.to_str().unwrap())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_file_exists_fails_for_missing_file() {
        assert!(
            !run_evaluator("x", "file_exists", "/nonexistent/path_xyz_123")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn run_evaluator_rejects_unknown_kind_mentions_all_kinds() {
        // The error message must name all valid evaluator kinds so the
        // operator knows what's available without reading the source.
        let err = run_evaluator("resp", "bogus", "x").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("contains"));
        assert!(msg.contains("not_contains"));
        assert!(msg.contains("regex"));
        assert!(msg.contains("exit_code"));
        assert!(msg.contains("file_exists"));
    }

    // ── Discriminative power probe (event-substrate item 4) ───────────
    //
    // The 27/27 probe validated structural task sharing across cards, not
    // that the task set can distinguish good from bad agents. This probe runs
    // two agents with different system prompts through the real delegate +
    // evaluator path (mock inference, real evaluator) and verifies the harness
    // produces non-trivial, divergent pass rates — the harness actually
    // measures something, not just noise.

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock inference that returns different responses based on the system
    /// prompt embedded in the prompt text. An agent whose system prompt
    /// contains respond correctly gets the right answer; everyone else gets
    /// a wrong one. This models a real capability gap without a real model.
    struct DiscriminativeInference {
        call_count: Arc<AtomicUsize>,
    }

    impl hkask_types::InferencePort for DiscriminativeInference {
        fn generate(
            &self,
            prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            let text = if prompt.contains("respond correctly") {
                "42".to_string()
            } else {
                "I dont know".to_string()
            };
            Box::pin(async move {
                Ok(hkask_types::InferenceResult {
                    text,
                    model: "mock".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    struct NoopDispatch;

    impl hkask_types::ToolDispatchPort for NoopDispatch {
        fn invoke_tool<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _allowlist: &'a [String],
        ) -> Pin<
            Box<
                dyn Future<Output = Result<serde_json::Value, hkask_types::InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(serde_json::json!({})) })
        }
    }

    fn mock_agent_card(agent_id: &str, system_prompt: &str) -> LocalAgentCard {
        LocalAgentCard {
            agent_id: agent_id.to_string(),
            agent_type: "local".to_string(),
            description: "test agent".to_string(),
            display_name: agent_id.to_string(),
            accepts: vec![],
            produces: vec![],
            dependencies: LocalAgentDependencies::default(),
            capabilities: LocalAgentCapabilities {
                model: "mock".to_string(),
                min_provider_class: "".to_string(),
                system_prompt: Some(system_prompt.to_string()),
                mcp_tools: vec![],
                skills: vec![],
                output_contract: None,
                evaluators: vec![],
            },
            cloud_swarm_id: None,
            tags: vec![],
            visibility: String::new(),
            sample_queries: vec![],
            valence: None,
        }
    }

    #[tokio::test]
    async fn harness_distinguishes_good_from_bad_agent() {
        // Two agents with different system prompts. The mock inference
        // returns 42 when the system prompt says respond correctly, and
        // a wrong answer otherwise. A task asking for 6x7 with evaluator
        // contains 42 should pass for the good agent and fail for the bad
        // one — non-trivial, divergent pass rates confirm the harness
        // measures something.
        use crate::local_runtime::LocalSwarmRuntime;
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let ledger = Arc::new(hkask_ledger::Ledger::from_driver(driver).unwrap());
        let call_count = Arc::new(AtomicUsize::new(0));
        let inference: Arc<dyn hkask_types::InferencePort> = Arc::new(DiscriminativeInference {
            call_count: call_count.clone(),
        });
        let dispatch: Arc<dyn hkask_types::ToolDispatchPort> = Arc::new(NoopDispatch);
        let runtime = LocalSwarmRuntime::new_for_test(ledger, inference, dispatch);

        let good_agent = mock_agent_card(
            "good",
            "You are a helpful assistant. Always respond correctly.",
        );
        let bad_agent = mock_agent_card("bad", "You are a confused assistant.");

        let good_result = runtime
            .delegate(&good_agent, "What is 6 times 7?", 10, 100)
            .await
            .unwrap();
        let bad_result = runtime
            .delegate(&bad_agent, "What is 6 times 7?", 10, 100)
            .await
            .unwrap();

        // The good agent passes; the bad agent fails.
        assert!(
            run_evaluator(&good_result.response, "contains", "42")
                .await
                .unwrap(),
            "good agent should produce the correct answer"
        );
        assert!(
            !run_evaluator(&bad_result.response, "contains", "42")
                .await
                .unwrap(),
            "bad agent should NOT produce the correct answer"
        );

        // Non-trivial: both agents ran real inference (not short-circuited).
        assert!(
            call_count.load(Ordering::Relaxed) >= 2,
            "both agents must have called inference"
        );
    }

    #[tokio::test]
    async fn harness_exit_code_evaluator_distinguishes_correct_from_wrong_output() {
        // The exit_code evaluator is external ground truth: it runs a command
        // that checks the response, not the response text itself. This is the
        // Goodhart-resistant evaluator — an adapter that learns to emit 42
        // without solving the task cannot game an exit_code check that
        // validates the answer against a real computation.
        // Use grep instead of test to avoid shell quoting issues in the spec.
        assert!(
            run_evaluator("42", "exit_code", "echo $RESPONSE | grep -qx 42")
                .await
                .unwrap()
        );
        assert!(
            !run_evaluator("wrong", "exit_code", "echo $RESPONSE | grep -qx 42")
                .await
                .unwrap()
        );
    }
}
