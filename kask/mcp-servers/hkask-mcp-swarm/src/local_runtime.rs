//! Local swarm runtime — ledger + inference + guard for `Local` mode (v2 §15).
//!
//! Extracted from the swarm server root. `LazyLocalSwarmRuntime` defers
//! construction to the first tool call (the `run_server` factory is sync).
//! `LocalSwarmRuntime::delegate` runs a local agent: scan input → tool loop
//! → cost → debit → scan output. The ledger is operator-funded; the
//! inference/guard/skill/tool ports are resolved once at construction.

use crate::error::SwarmError;
use crate::local_registry::LocalAgentCard;
use crate::sanitize::strip_leading_mentions;

/// The local swarm runtime — ledger + inference + guard.
///
/// Constructed lazily on first tool call (the `run_server` factory closure
/// is sync — it cannot `.await` the inference port resolution). `lazy()`
/// stores the config; `get_or_init()` does the async init on first use.
///
/// Design tradeoff (R1): the `OnceCell` caches the resolved ports forever.
/// If the server starts before `HKASK_INFERENCE_SOCKET` is set (e.g.
/// the McpRuntime launch fires before the deferred task sets the socket),
/// `resolve_tool_dispatch_port` returns the `UnavailableToolDispatch` stub
/// and the stub is cached for the process lifetime. This is a transient
/// degradation, not a silent failure: the stub errors are `tracing::warn!`-logged
/// and carry a clear remediation message. The `SettingsStore` restart observer
/// (`sync_kask_mcp_runtime_servers` in `main.rs`) detects the env diff and
/// restarts the server with a fresh `OnceCell` on the next kask settings
/// change. In practice the governed servers are launched in the deferred
/// task after the IPC socket is already set (`main.rs` sets
/// `INFERENCE_SOCKET_PATH` before the governed launch loop), so the env at
/// launch includes the socket and the stub is never cached. The
/// `SettingsStore` observer fires on kask settings changes, not on
/// `INFERENCE_SOCKET_PATH` being set (a `OnceLock`, not a settings change) —
/// the socket-becoming-available case is covered by the launch ordering, not
/// by the observer.
pub(crate) struct LazyLocalSwarmRuntime {
    ledger_path: String,
    inner: tokio::sync::OnceCell<LocalSwarmRuntime>,
}

impl LazyLocalSwarmRuntime {
    /// Store the config without initializing. The runtime is constructed
    /// on first call to `get_or_init`.
    pub(crate) fn lazy(ledger_path: String) -> Self {
        Self {
            ledger_path,
            inner: tokio::sync::OnceCell::new(),
        }
    }

    /// Get the runtime, initializing it on first call. Returns `Err` if
    /// initialization fails (ledger open, inference port resolution, guard
    /// init). Subsequent calls return the cached runtime.
    pub(crate) async fn get_or_init(&self) -> Result<&LocalSwarmRuntime, String> {
        self.inner
            .get_or_try_init(|| async { LocalSwarmRuntime::new(&self.ledger_path).await })
            .await
    }
}

/// The initialized local swarm runtime — ledger + inference + guard.
pub(crate) struct LocalSwarmRuntime {
    ledger: std::sync::Arc<hkask_ledger::Ledger>,
    inference: std::sync::Arc<dyn hkask_types::InferencePort>,
    guard: std::sync::Arc<hkask_guard::ContentGuard>,
    /// Tool dispatch back to the zed process (governed `McpRuntime` via the
    /// IPC bridge). Resolved once at construction — see `resolve_tool_dispatch_port`.
    tool_dispatch: std::sync::Arc<dyn hkask_types::ToolDispatchPort>,
    /// Skill execution back to the zed process (`ManifestExecutor` via the
    /// IPC bridge). Resolved once at construction — see `resolve_skill_exec_port`.
    skill_exec: std::sync::Arc<dyn hkask_types::SkillExecPort>,
    /// The operator's account id in the ledger (funded via `swarm_fund_local`).
    operator_account: String,
    /// The asset name for local credits.
    asset: String,
}

impl LocalSwarmRuntime {
    /// Construct the runtime. Opens (or creates) the ledger at `db_path`,
    /// resolves the inference port, and initializes the guard.
    ///
    /// The operator account is ensured in the ledger namespace "local_swarm".
    /// It starts at balance 0 — the operator funds it via `swarm_fund_local`.
    pub(crate) async fn new(db_path: &str) -> Result<Self, String> {
        // Open the ledger at the file path. Create the directory if needed.
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create ledger dir {}: {e}", parent.display()))?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| format!("failed to create ledger pool: {e}"))?;
        let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
            std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
        let ledger = hkask_ledger::Ledger::from_driver(driver)
            .map_err(|e| format!("failed to init ledger: {e}"))?;

        // Resolve the inference port (zed IPC bridge or MediaRouter fallback).
        let inference = hkask_inference::resolve_inference_port().await;

        // Resolve the tool dispatch port (zed IPC bridge or unavailable stub).
        let tool_dispatch = hkask_inference::resolve_tool_dispatch_port().await;

        // Resolve the skill execution port (zed IPC bridge or unavailable stub).
        let skill_exec = hkask_inference::resolve_skill_exec_port().await;

        // Initialize the content guard with mandatory scanners.
        let guard_config = hkask_guard::GuardConfig::from_env();
        let guard = hkask_guard::ContentGuard::mandatory(&guard_config);

        // Ensure the operator account exists.
        let operator_account = "operator".to_string();
        let asset = "credits".to_string();
        ledger
            .ensure_account(&operator_account, "local_swarm")
            .map_err(|e| format!("failed to ensure operator account: {e}"))?;

        Ok(Self {
            ledger: std::sync::Arc::new(ledger),
            inference,
            guard: std::sync::Arc::new(guard),
            tool_dispatch,
            skill_exec,
            operator_account,
            asset,
        })
    }

    /// Test-only constructor with injected dependencies. Mirrors the
    /// `StubInferencePort` pattern in `hkask-templates` and `hkask-guard`:
    /// the production `new(db_path)` resolves the inference port from env
    /// (zed IPC bridge or MediaRouter fallback), which is unsuitable for
    /// unit tests. This constructor accepts a pre-built ledger, inference
    /// port, guard, and the two zed-side ports so tests can exercise the
    /// `fund`/`debit`/`delegate` logic without a real backend.
    ///
    /// Ensures the operator account exists (same as `new`) so `balance`/
    /// `fund`/`debit` work out of the box.
    #[cfg(test)]
    pub(crate) fn with_deps(
        ledger: hkask_ledger::Ledger,
        inference: std::sync::Arc<dyn hkask_types::InferencePort>,
        guard: hkask_guard::ContentGuard,
        tool_dispatch: std::sync::Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: std::sync::Arc<dyn hkask_types::SkillExecPort>,
    ) -> Result<Self, String> {
        let operator_account = "operator".to_string();
        let asset = "credits".to_string();
        ledger
            .ensure_account(&operator_account, "local_swarm")
            .map_err(|e| format!("failed to ensure operator account: {e}"))?;
        Ok(Self {
            ledger: std::sync::Arc::new(ledger),
            inference,
            guard: std::sync::Arc::new(guard),
            tool_dispatch,
            skill_exec,
            operator_account,
            asset,
        })
    }

    /// The operator's current ledger balance. Returns `None` on query error
    /// (the `.rules` trap — never fabricate a zero balance on a failed
    /// measurement).
    pub(crate) fn balance(&self) -> Option<i64> {
        self.ledger
            .balance(&self.operator_account, Some(&self.asset))
            .ok()
    }

    /// Recent ledger transactions for the operator account, newest first,
    /// capped at `limit`. Each entry carries the operator-relevant signed
    /// amount (fund = +, debit = −) and the metadata `action` ("fund" |
    /// "debit"). Returns `Err` on a query failure — a failed query is not an
    /// empty history (the `.rules` trap).
    pub(crate) fn history(&self, limit: usize) -> Result<Vec<serde_json::Value>, String> {
        let range = hkask_ledger::DateRange {
            start: "0000-01-01T00:00:00Z".to_string(),
            end: "9999-12-31T23:59:59Z".to_string(),
        };
        let filter = hkask_ledger::QueryFilter {
            account: Some(self.operator_account.clone()),
            asset: Some(self.asset.clone()),
            namespace: None,
        };
        let mut txs = self
            .ledger
            .query(&range, &filter)
            .map_err(|e| format!("ledger query failed: {e}"))?;
        // The ledger query returns oldest-first; the tool wants newest-first.
        txs.reverse();
        txs.truncate(limit);
        Ok(txs
            .into_iter()
            .map(|tx| {
                // The operator-relevant posting: fund = external→operator
                // (+), debit = operator→external (−).
                let amount = tx
                    .postings
                    .iter()
                    .find_map(|p| {
                        if p.destination == self.operator_account {
                            Some(p.amount)
                        } else if p.source == self.operator_account {
                            Some(-p.amount)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let kind = tx
                    .metadata
                    .get("action")
                    .and_then(|a| a.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                serde_json::json!({
                    "id": tx.id,
                    "timestamp": tx.timestamp,
                    "reference": tx.reference,
                    "kind": kind,
                    "amount": amount,
                    "asset": self.asset,
                })
            })
            .collect())
    }

    /// Deposit credits into the operator's account. Returns the new balance.
    /// Used by `swarm_fund_local`.
    pub(crate) fn fund(&self, amount: i64) -> Result<i64, String> {
        if amount <= 0 {
            return Err("fund amount must be positive".to_string());
        }
        let tx_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let reference = format!("fund-{tx_id}");
        let tx = hkask_ledger::LedgerTransaction {
            id: tx_id,
            timestamp: now,
            reference,
            postings: vec![hkask_ledger::Posting {
                source: "external".to_string(),
                destination: self.operator_account.clone(),
                asset: self.asset.clone(),
                amount,
            }],
            metadata: serde_json::json!({ "action": "fund" }),
        };
        self.ledger
            .commit(&tx)
            .map_err(|e| format!("ledger commit failed: {e}"))?;
        self.balance().ok_or_else(|| {
            "balance query failed after fund — ledger may be in a bad state".to_string()
        })
    }

    /// Debit credits from the operator's account. Returns the new balance.
    /// Returns `Err(PaymentRequired)` if the balance is insufficient.
    pub(crate) fn debit(&self, amount: i64, reference: &str) -> Result<i64, SwarmError> {
        if amount <= 0 {
            return Err(SwarmError::PaymentRequired(
                "debit amount must be positive".to_string(),
            ));
        }
        let balance = self.balance().ok_or_else(|| {
            SwarmError::Unavailable("ledger balance query failed — cannot verify funds".to_string())
        })?;
        if balance < amount {
            return Err(SwarmError::PaymentRequired(format!(
                "insufficient local credits: have {balance}, need {amount} \
                 — fund via swarm_fund_local"
            )));
        }
        let tx_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let tx = hkask_ledger::LedgerTransaction {
            id: tx_id,
            timestamp: now,
            reference: reference.to_string(),
            postings: vec![hkask_ledger::Posting {
                source: self.operator_account.clone(),
                destination: "external".to_string(),
                asset: self.asset.clone(),
                amount,
            }],
            metadata: serde_json::json!({ "action": "debit" }),
        };
        self.ledger
            .commit(&tx)
            .map_err(|e| SwarmError::Unavailable(format!("ledger commit failed: {e}")))?;
        self.balance().ok_or_else(|| {
            SwarmError::Unavailable(
                "balance query failed after debit — ledger may be in a bad state".to_string(),
            )
        })
    }

    /// Scan input text through the content guard. Returns `Err` if the guard
    /// rejects the input (prompt injection, role override, etc.).
    pub(crate) fn scan_input(&self, text: &str) -> Result<(), SwarmError> {
        let result = self.guard.scan_input(text);
        if !result.passed {
            let violations: Vec<String> = result
                .violations
                .iter()
                .map(|v| format!("{}: {}", v.scanner, v.description))
                .collect();
            return Err(SwarmError::Unavailable(format!(
                "input guard rejected: {}",
                violations.join("; ")
            )));
        }
        Ok(())
    }

    /// Scan output text through the content guard. Returns the (possibly
    /// sanitized) output text, or `Err` if canary exfiltration is detected.
    ///
    /// Policy: canary exfiltration is a hard failure (the system prompt was
    /// leaked — OWASP LLM07), but secret leakage is sanitized and returned
    /// (the output may be legitimately useful despite a false-positive secret
    /// match). This asymmetry is intentional: canary = exfiltration = reject;
    /// secret = leakage = sanitize and return. Do not "fix" this by making
    /// both paths hard-fail — that would reject legitimate outputs that
    /// happen to match a secret scanner pattern.
    pub(crate) fn scan_output(&self, text: &str) -> Result<String, SwarmError> {
        let result = self.guard.scan_output(text);
        if self.guard.check_canary(text) {
            return Err(SwarmError::Unavailable(
                "canary token detected in output — system prompt exfiltration suspected"
                    .to_string(),
            ));
        }
        if !result.passed {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                violations = ?result.violations,
                "output guard violations — sanitizing"
            );
        }
        Ok(result.output.content(text).to_string())
    }

    /// Execute a local agent: scan input → run the tool loop (declare the
    /// card's `mcp_tools`, dispatch model tool calls through the zed IPC
    /// bridge) → compute cost → debit ledger → scan output. Returns the
    /// response text, model, token usage, cost, remaining balance, and a
    /// tool-call summary. The debit happens before the output guard scan so
    /// a guard-quarantined result still costs credits (matching ABW's
    /// "compute was spent" semantics).
    ///
    /// Tool dispatch is allowlisted twice: the declared `mcp_tools` set is
    /// the only tool set shown to the model AND the qualified list travels
    /// with every dispatch so the zed-side IPC server enforces it at the
    /// dispatch boundary (a tool outside the card's declared set is never
    /// minted a panel token). Tool *results* are third-party data injected
    /// into the model's context — each is run through the input guard and
    /// redacted (not fatal) on violation: a false-positive pattern in
    /// legitimate tool data must not abort the delegation, but the payload
    /// must not reach the model.
    pub(crate) async fn delegate(
        &self,
        agent: &LocalAgentCard,
        task: &str,
        credits_authorized: u32,
        max_credits_per_dispatch: u32,
    ) -> Result<LocalDelegateResult, SwarmError> {
        // Strip leading @mentions (defense-in-depth, mirrors ABW delegate).
        let task_clean = strip_leading_mentions(task);

        // Scan the input through the guard.
        self.scan_input(&task_clean)?;

        // Check the per-dispatch ceiling.
        if credits_authorized > max_credits_per_dispatch {
            return Err(SwarmError::PaymentRequired(format!(
                "credits_authorized {credits_authorized} exceeds per-dispatch ceiling \
                 {max_credits_per_dispatch} (raise HKASK_ABW_MAX_CREDITS to authorize)"
            )));
        }

        // Check the ledger balance — the operator must have funded it.
        // The pre-inference check uses `credits_authorized` (the operator's
        // declared budget). The actual debit after inference uses the real
        // token-based cost, capped at `credits_authorized`.
        let balance = self.balance().ok_or_else(|| {
            SwarmError::Unavailable("ledger balance query failed — cannot verify funds".to_string())
        })?;
        if balance < i64::from(credits_authorized) {
            return Err(SwarmError::PaymentRequired(format!(
                "insufficient local credits: have {balance}, need {credits_authorized} \
                 — fund via swarm_fund_local"
            )));
        }

        // Build the prompt: system prompt + task.
        let system_prompt = agent
            .capabilities
            .system_prompt
            .as_deref()
            .unwrap_or("You are a helpful assistant.");

        // Guard-scan the system_prompt before injecting it into the prompt.
        // The task was already scanned above, and each skill output is scanned
        // below — but the system_prompt was not. For locally-authored cards the
        // operator controls it; for cloned cards (`swarm_clone_to_local`) it is
        // third-party ABW data that could carry prompt injection. The clone path
        // strips obvious patterns via `sanitize_abw_text`, but the guard is the
        // hard gate: a system_prompt that trips the input guard IS fatal.
        // The `.rules` trap: the input guard is the advertised enforcement point
        // for the delegate path — it must scan all untrusted text that reaches the
        // model, not just the task.
        self.scan_input(system_prompt)?;

        // Run the declared skills (capped) against the task BEFORE the LLM
        // call. Each cascade runs on the zed side (`ManifestExecutor`, own
        // gas/OCAP enforcement). Skill output is untrusted context — it flows
        // into the prompt, so it is guard-scanned before injection; a skill
        // output that trips the input guard IS fatal (an injection from a
        // skill is a finding, not a cosmetic issue). A missing skill or
        // cascade failure is recorded, not fatal — the delegation proceeds
        // with whatever context the successful skills produced.
        let mut executed_skills: Vec<serde_json::Value> = Vec::new();
        let mut skill_context = String::new();
        for skill in agent
            .capabilities
            .skills
            .iter()
            .take(MAX_SKILLS_PER_DELEGATION)
        {
            match self.skill_exec.execute_skill(skill, &task_clean).await {
                Ok(output) => {
                    self.scan_input(&output)?;
                    executed_skills.push(serde_json::json!({ "skill": skill, "ok": true }));
                    skill_context.push_str(&format!("\n\n## Skill '{skill}' output\n{output}"));
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        skill,
                        error = %e,
                        "declared skill failed — delegation proceeds without it"
                    );
                    executed_skills.push(serde_json::json!({
                        "skill": skill,
                        "ok": false,
                        "error": e,
                    }));
                }
            }
        }
        let prompt = format!("{system_prompt}{skill_context}\n\n---\n\nTask: {task_clean}");

        // Build the declared tool set from the card's `mcp_tools` (qualified
        // `server/tool` names). This list is the allowlist: a model call for
        // any tool not declared here is never dispatched.
        let declared_tools: Vec<(String, String)> = agent
            .capabilities
            .mcp_tools
            .iter()
            .filter_map(|qualified| {
                qualified
                    .split_once('/')
                    .map(|(s, t)| (s.to_string(), t.to_string()))
            })
            .collect();
        // The qualified allowlist travels with every dispatch so the zed-side
        // IPC server can enforce it at the dispatch boundary — a tool outside
        // the card's declared set is never minted a panel token there.
        let qualified_allowed: Vec<String> = declared_tools
            .iter()
            .map(|(s, t)| format!("{s}/{t}"))
            .collect();
        let tool_defs: Vec<hkask_types::ChatToolDefinition> = declared_tools
            .iter()
            .map(|(server, tool)| hkask_types::ChatToolDefinition {
                tool_type: "function".to_string(),
                function: hkask_types::ChatToolFunction {
                    name: format!("{server}/{tool}"),
                    description: format!("Invoke `{tool}` on the `{server}` MCP server."),
                    parameters: serde_json::json!({ "type": "object", "properties": {} }),
                },
            })
            .collect();
        let tools_slice: Option<&[hkask_types::ChatToolDefinition]> =
            (!tool_defs.is_empty()).then_some(&tool_defs[..]);

        // Run the tool loop: messages → inference → (tool calls → dispatch →
        // append results) → inference … The round cap bounds cost
        // amplification; the per-dispatch ceiling is the credit gate.
        let params = hkask_types::LLMParameters::default();
        let model_override = if agent.capabilities.model.is_empty() {
            None
        } else {
            Some(agent.capabilities.model.clone())
        };
        let mut messages = vec![hkask_types::ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        let mut tool_calls_made: Vec<serde_json::Value> = Vec::new();
        let mut total_tokens: i64 = 0;
        let mut final_text = String::new();
        let mut final_model = String::new();
        for _round in 0..MAX_TOOL_ROUNDS {
            let result = self
                .inference
                .generate_with_messages(&messages, &params, model_override.as_deref(), tools_slice)
                .await
                .map_err(|e| SwarmError::UpstreamModelError {
                    provider: "local".to_string(),
                    message: format!("inference failed: {e}"),
                })?;
            total_tokens += i64::from(result.usage.total_tokens);
            final_model = result.model.clone();
            if result.tool_calls.is_empty() {
                final_text = result.text;
                break;
            }

            // Dispatch each model tool call, allowlisted against the card's
            // declared mcp_tools. Results are appended as a user message so
            // the next round sees them (provider-safe message shape).
            let mut round_results = Vec::new();
            for call in &result.tool_calls {
                let qualified = &call.tool;
                let declared = declared_tools
                    .iter()
                    .find(|(s, t)| format!("{s}/{t}") == *qualified);
                let (outcome, summary) = match declared {
                    Some((server, tool)) => {
                        match self
                            .tool_dispatch
                            .invoke_tool(server, tool, call.args.clone(), &qualified_allowed)
                            .await
                        {
                            Ok(value) => {
                                let text = serde_json::to_string(&value)
                                    .unwrap_or_else(|_| value.to_string());
                                // Redact-and-continue (see fn doc): a tool result
                                // that trips the input guard is quarantined from the
                                // model context, but the delegation proceeds — tool
                                // output is data, and a false positive must not abort
                                // the run.
                                let (injected, ok, error) = match self.scan_input(&text) {
                                    Ok(()) => (text, true, None),
                                    Err(e) => (
                                        "[redacted: tool output tripped the input guard — not injected]".to_string(),
                                        false,
                                        Some(e.to_string()),
                                    ),
                                };
                                let mut summary =
                                    serde_json::json!({ "tool": qualified, "ok": ok });
                                if let Some(err) = error {
                                    summary["error"] = serde_json::Value::String(err);
                                }
                                (
                                    format!("Tool call '{qualified}' returned:\n{injected}"),
                                    summary,
                                )
                            }
                            Err(e) => {
                                let msg = format!("dispatch failed: {e}");
                                (
                                    format!("Tool call '{qualified}' {msg}"),
                                    serde_json::json!({
                                        "tool": qualified,
                                        "ok": false,
                                        "error": e.to_string(),
                                    }),
                                )
                            }
                        }
                    }
                    None => (
                        format!(
                            "Tool call '{qualified}' is not in this agent's declared mcp_tools \
                             allowlist — not dispatched"
                        ),
                        serde_json::json!({
                            "tool": qualified,
                            "ok": false,
                            "error": "not in declared mcp_tools allowlist",
                        }),
                    ),
                };
                tool_calls_made.push(summary);
                round_results.push(outcome);
            }
            messages.push(hkask_types::ChatMessage {
                role: "assistant".to_string(),
                content: format!("(requested {} tool call(s))", result.tool_calls.len()),
            });
            messages.push(hkask_types::ChatMessage {
                role: "user".to_string(),
                content: round_results.join("\n\n"),
            });
        }

        // Compute the cost: 1 credit per 1000 tokens (mirrors ABW's
        // `execution_fee`), summed across tool-loop rounds, capped at
        // `credits_authorized`.
        let tokens = total_tokens;
        let base_cost = std::cmp::max(1, tokens / 1000);
        let cost = std::cmp::min(base_cost, i64::from(credits_authorized));

        // Debit the ledger immediately after inference succeeds — before the
        // output guard scan. This matches ABW's "compute was spent" semantics:
        // a guard-quarantined result still costs credits because the inference
        // compute already happened. Moving the debit before `scan_output` (which
        // uses `?` to return early) ensures the operator is charged even when
        // the output is rejected for canary exfiltration or secret leakage.
        let reference = format!("delegate-{}-{}", agent.agent_id, uuid::Uuid::new_v4());
        let new_balance = self.debit(cost, &reference)?;

        // Scan the output through the guard. If this rejects (canary
        // exfiltration, secret leakage), the debit has already happened — the
        // compute was spent. The error propagates, but the operator's balance
        // reflects the cost of the rejected call.
        let output_text = self.scan_output(&final_text)?;

        Ok(LocalDelegateResult {
            agent_id: agent.agent_id.clone(),
            response: output_text,
            model: final_model,
            tokens_used: tokens,
            cost,
            balance: new_balance,
            tool_calls: tool_calls_made,
            executed_skills,
        })
    }
}

/// Maximum tool-call rounds per delegation. Each round is a full inference
/// call; the cap bounds cost amplification (the per-dispatch credit ceiling
/// is the credit gate, this is the round gate).
pub(crate) const MAX_TOOL_ROUNDS: usize = 4;

/// Maximum declared skills executed per delegation. Each skill is a cascade
/// with its own gas budget on the zed side; the cap bounds context bloat and
/// cascade amplification from a maliciously-large `skills` list.
pub(crate) const MAX_SKILLS_PER_DELEGATION: usize = 3;

/// Result of a local delegation.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct LocalDelegateResult {
    pub(crate) agent_id: String,
    pub(crate) response: String,
    pub(crate) model: String,
    pub(crate) tokens_used: i64,
    pub(crate) cost: i64,
    pub(crate) balance: i64,
    /// Summary of tool calls made during the delegation (qualified
    /// `server/tool` name + ok/error). Empty when the agent declares no
    /// `mcp_tools` or the model made no calls.
    pub(crate) tool_calls: Vec<serde_json::Value>,
    /// Summary of skill cascades executed before the LLM call (skill id +
    /// ok/error). Empty when the agent declares no `skills`.
    pub(crate) executed_skills: Vec<serde_json::Value>,
}
