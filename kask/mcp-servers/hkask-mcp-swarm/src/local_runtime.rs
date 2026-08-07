//! Local swarm runtime — ledger + inference + guard for `Local` mode (v2 §15).
//!
//! Extracted from the swarm server root. `LazyLocalSwarmRuntime` defers
//! construction to the first tool call (the `run_server` factory is sync).
//! `LocalSwarmRuntime::delegate` runs a local agent: scan input → tool loop
//! → cost → debit → scan output. The ledger is operator-funded; the
//! inference/guard/skill/tool ports are resolved once at construction.

use std::time::Instant;

use crate::agent_executor::{AgentExecutor, RawDelegateResult};
use crate::error::{LocalSwarmError, SwarmError};
use crate::local_registry::LocalAgentCard;
use crate::sanitize::strip_leading_mentions;

use hkask_ledger::LedgerError;

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
pub struct LazyLocalSwarmRuntime {
    ledger_path: String,
    skills_dir: Option<String>,
    inner: tokio::sync::OnceCell<LocalSwarmRuntime>,
}

impl LazyLocalSwarmRuntime {
    /// Store the config without initializing. The runtime is constructed
    /// on first call to `get_or_init`.
    pub fn lazy(ledger_path: String, skills_dir: Option<String>) -> Self {
        Self {
            ledger_path,
            skills_dir,
            inner: tokio::sync::OnceCell::new(),
        }
    }

    /// Get the runtime, initializing it on first call. Returns `Err` if
    /// initialization fails (ledger open, inference port resolution, guard
    /// init). Subsequent calls return the cached runtime.
    pub async fn get_or_init(&self) -> Result<&LocalSwarmRuntime, LocalSwarmError> {
        self.inner
            .get_or_try_init(|| async {
                LocalSwarmRuntime::new(&self.ledger_path, self.skills_dir.as_deref()).await
            })
            .await
    }
}

/// The initialized local swarm runtime — ledger + agent executor.
///
/// The runtime owns the *spending* policy (ceiling check, balance check,
/// cost computation, debit) and the final output scan. The *agent-run*
/// policy (input scanning, skill cascade, tool-loop orchestration) lives in
/// `AgentExecutor`. The split preserves the debit-before-scan invariant: the
/// runtime debits the ledger, *then* calls `executor.scan_output` on the raw
/// result — so a guard-quarantined result still costs credits (the compute was
/// already spent).
pub struct LocalSwarmRuntime {
    ledger: std::sync::Arc<hkask_ledger::Ledger>,
    /// The agent-run policy (inference + tool dispatch + skill exec + guard).
    /// Constructed once from the resolved IPC-bridge ports; the runtime calls
    /// `executor.run` then debits then `executor.scan_output`.
    executor: AgentExecutor,
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
    pub(crate) async fn new(
        db_path: &str,
        skills_dir: Option<&str>,
    ) -> Result<Self, LocalSwarmError> {
        // Open the ledger at the file path. Create the directory if needed.
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to create ledger dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| LocalSwarmError::Database(format!("failed to create ledger pool: {e}")))?;
        let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
            std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
        let ledger = hkask_ledger::Ledger::from_driver(driver)
            .map_err(|e| LocalSwarmError::Database(format!("failed to init ledger: {e}")))?;

        // Resolve the agent-run ports once at construction: inference,
        // tool dispatch, and skill execution all route through the zed IPC
        // bridge (or fall back to media/stub when the socket is absent). The
        // guard scans all untrusted text that reaches the model. These four
        // compose into the `AgentExecutor`, which owns the agent-run policy
        // (the runtime owns the spending policy). Resolving them here (rather
        // than inside `AgentExecutor::new`) keeps the env-var reads at the
        // runtime construction seam, mirroring the other kask MCP servers.
        let inference = hkask_inference::resolve_inference_port().await;
        let tool_dispatch = hkask_inference::resolve_tool_dispatch_port().await;
        let skill_exec = hkask_inference::resolve_skill_exec_port().await;
        let guard_config = hkask_guard::GuardConfig::from_env();
        let guard = hkask_guard::ContentGuard::mandatory(&guard_config);
        // Resolve the skill-corpus directory for `AgentExecutor`'s Slice-6
        // skill-awareness (None = skill-blind). Passed from
        // `LazyLocalSwarmRuntime`, which reads `HKASK_SKILLS_DIR` in
        // `SwarmConfig::from_env`.
        let skills_dir = skills_dir.map(std::path::PathBuf::from);
        let executor = AgentExecutor::new(inference, tool_dispatch, skill_exec, guard, skills_dir);

        // Ensure the operator account exists.
        let operator_account = "operator".to_string();
        let asset = "credits".to_string();
        ledger
            .ensure_account(&operator_account, "local_swarm")
            .map_err(|e| {
                LocalSwarmError::Ledger(format!("failed to ensure operator account: {e}"))
            })?;

        Ok(Self {
            ledger: std::sync::Arc::new(ledger),
            executor,
            operator_account,
            asset,
        })
    }

    /// Test-only constructor with injected dependencies. Mirrors the
    /// `StubInferencePort` pattern in `hkask-templates` and `hkask-guard`:
    /// the production `new(db_path)` resolves the inference port from env
    /// (zed IPC bridge or MediaRouter fallback), which is unsuitable for
    /// unit tests. This constructor accepts a pre-built ledger + the four
    /// agent-run ports (inference, tool dispatch, skill exec, guard) which it
    /// composes into an `AgentExecutor`, so tests can exercise the
    /// `fund`/`debit`/`delegate` logic without a real backend.
    ///
    /// Ensures the operator account exists (same as `new`) so `balance`/
    /// `fund`/`debit` work out of the box.
    #[cfg(test)]
    #[expect(dead_code)]
    pub(crate) fn with_deps(
        ledger: hkask_ledger::Ledger,
        inference: std::sync::Arc<dyn hkask_types::InferencePort>,
        guard: hkask_guard::ContentGuard,
        tool_dispatch: std::sync::Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: std::sync::Arc<dyn hkask_types::SkillExecPort>,
    ) -> Result<Self, LocalSwarmError> {
        let operator_account = "operator".to_string();
        let asset = "credits".to_string();
        ledger
            .ensure_account(&operator_account, "local_swarm")
            .map_err(|e| {
                LocalSwarmError::Ledger(format!("failed to ensure operator account: {e}"))
            })?;
        let executor = AgentExecutor::with_deps(inference, tool_dispatch, skill_exec, guard);
        Ok(Self {
            ledger: std::sync::Arc::new(ledger),
            executor,
            operator_account,
            asset,
        })
    }

    /// The operator's current ledger balance. Returns `None` on query error
    /// (the `.rules` trap — never fabricate a zero balance on a failed
    /// measurement).
    pub fn balance(&self) -> Option<i64> {
        self.ledger
            .balance(&self.operator_account, Some(&self.asset))
            .ok()
    }

    /// The resolved local inference port. Exposed so the local knowledge tools
    /// (`swarm_generate_prompt_local` / `swarm_generate_ontology_local`) can do a
    /// one-shot generate via the same inference port the delegate loop uses —
    /// reuse, not a second resolution.
    pub(crate) fn inference(&self) -> std::sync::Arc<dyn hkask_types::InferencePort> {
        self.executor.inference()
    }

    /// The content guard. Exposed so the local knowledge tools can scan their
    /// LLM-generated output for canary/secret leakage before returning it.
    pub(crate) fn guard(&self) -> std::sync::Arc<hkask_guard::ContentGuard> {
        self.executor.guard()
    }

    /// The resolved skill-execution port. Exposed so `swarm_ai_assist` can run
    /// the on-disk `swarm-compose-guide` skill cascade — the Jinja2 template is
    /// the single source of truth for composition guidance, not hardcoded Rust.
    /// Mirrors the `inference()`/`guard()` accessor pattern.
    pub(crate) fn skill_exec(&self) -> std::sync::Arc<dyn hkask_types::SkillExecPort> {
        self.executor.skill_exec()
    }

    /// Recent ledger transactions for the operator account, newest first,
    /// capped at `limit`. Each entry carries the operator-relevant signed
    /// amount (fund = +, debit = −) and the metadata `action` ("fund" |
    /// "debit"). Returns `Err` on a query failure — a failed query is not an
    /// empty history (the `.rules` trap).
    pub(crate) fn history(&self, limit: usize) -> Result<Vec<serde_json::Value>, LocalSwarmError> {
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
            .map_err(|e| LocalSwarmError::Ledger(format!("ledger query failed: {e}")))?;
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
    pub(crate) fn fund(&self, amount: i64) -> Result<i64, LocalSwarmError> {
        if amount <= 0 {
            return Err(LocalSwarmError::InvalidInput(
                "fund amount must be positive".to_string(),
            ));
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
            .map_err(|e| LocalSwarmError::Ledger(format!("ledger commit failed: {e}")))?;
        self.balance().ok_or_else(|| {
            LocalSwarmError::Ledger(
                "balance query failed after fund — ledger may be in a bad state".to_string(),
            )
        })
    }

    /// Debit credits from the operator's account. Returns the new balance.
    /// Returns `Err(PaymentRequired)` if the balance is insufficient.
    ///
    /// The balance check and the commit happen atomically inside a single
    /// `BEGIN IMMEDIATE` transaction in `Ledger::debit_if_funds`, closing the
    /// TOCTOU window where two concurrent `delegate` calls could both pass a
    /// separate pre-check and both commit (driving the account negative). The
    /// pre-inference balance check in `delegate` remains as a fast-fail so we
    /// don't run multi-second inference when the account is obviously
    /// unfunded, but it is NOT the authoritative gate.
    pub(crate) fn debit(&self, amount: i64, reference: &str) -> Result<i64, SwarmError> {
        if amount <= 0 {
            return Err(SwarmError::PaymentRequired(
                "debit amount must be positive".to_string(),
            ));
        }
        let new_balance = self
            .ledger
            .debit_if_funds(
                &self.operator_account,
                &self.asset,
                amount,
                reference,
                &serde_json::json!({ "action": "debit" }),
            )
            .map_err(|e| match e {
                LedgerError::InsufficientFunds {
                    balance, required, ..
                } => SwarmError::PaymentRequired(format!(
                    "insufficient local credits: have {balance}, need {required} \
                     — fund via swarm_fund_local"
                )),
                other => SwarmError::Unavailable(format!("ledger debit failed: {other}")),
            })?;
        Ok(new_balance)
    }

    /// Execute a local agent: scan the task → run the agent (skill cascade +
    /// tool loop, via `AgentExecutor::run`) → compute cost → debit ledger →
    /// scan output. Returns the response text, model, token usage, cost,
    /// remaining balance, and a tool-call summary. The debit happens before
    /// the output guard scan so a guard-quarantined result still costs credits
    /// (matching ABW's "compute was spent" semantics).
    ///
    /// The agent-run policy (input scanning of system prompt + skill/tool
    /// outputs, skill cascade, tool-loop orchestration) lives in
    /// `AgentExecutor::run`; the runtime owns the spending policy (ceiling,
    /// balance, cost, debit) and the final output scan. The task is
    /// input-scanned here *before* the funds check, preserving the original
    /// ordering (reject injected input before rejecting insufficient funds).
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
    pub async fn delegate(
        &self,
        agent: &LocalAgentCard,
        task: &str,
        credits_authorized: u32,
        max_credits_per_dispatch: u32,
    ) -> Result<LocalDelegateResult, SwarmError> {
        let started = Instant::now();
        // Strip leading @mentions (defense-in-depth, mirrors ABW delegate).
        let task_clean = strip_leading_mentions(task);

        // Scan the task through the guard BEFORE the funds check, preserving
        // the original ordering (reject injected input before rejecting
        // insufficient funds). The system_prompt + skill/tool outputs are
        // scanned inside `AgentExecutor::run`.
        self.executor.scan_input(&task_clean)?;

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

        // Run the agent (system_prompt scan + skill cascade + tool loop). The
        // executor returns the RAW output — it does NOT scan the final output
        // or debit the ledger. The debit-then-scan invariant is preserved
        // here: debit below, then `scan_output` on the raw text, so a
        // guard-quarantined result still costs credits (the compute was
        // already spent inside `run`).
        let raw: RawDelegateResult = self.executor.run(agent, &task_clean).await?;

        // Compute the cost: 1 credit per 1000 tokens (mirrors ABW's
        // `execution_fee`), summed across tool-loop rounds, capped at
        // `credits_authorized`.
        let tokens = raw.tokens_used;
        let base_cost = std::cmp::max(1, tokens / 1000);
        let cost = std::cmp::min(base_cost, i64::from(credits_authorized));

        // Debit the ledger immediately after the agent run succeeds — before
        // the output guard scan. This matches ABW's "compute was spent"
        // semantics: a guard-quarantined result still costs credits because the
        // inference compute already happened. Moving the debit before
        // `scan_output` (which uses `?` to return early) ensures the operator
        // is charged even when the output is rejected for canary
        // exfiltration or secret leakage.
        let reference = format!("delegate-{}-{}", agent.agent_id, uuid::Uuid::new_v4());
        let new_balance = self.debit(cost, &reference)?;

        // Scan the output through the guard. If this rejects (canary
        // exfiltration, secret leakage), the debit has already happened — the
        // compute was spent. The error propagates, but the operator's balance
        // reflects the cost of the rejected call.
        let output_text = self.executor.scan_output(&raw.text)?;

        Ok(LocalDelegateResult {
            agent_id: agent.agent_id.clone(),
            response: output_text,
            model: raw.model,
            tokens_used: tokens,
            cost,
            balance: new_balance,
            latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            tool_calls: raw.tool_calls,
            executed_skills: raw.executed_skills,
            // The server cannot judge task success — the executor (Curator or
            // human) stamps this after running a declared deterministic
            // evaluator against `response`. Left `None` here; ORIENT reads it
            // from the executor-populated `delegate_results`.
            task_success: None,
        })
    }
}

/// Maximum agents dispatched in a single `swarm_fanout_local` call (Cybernetic
/// Swarm Plan — bounds the cost amplification of one fan-out: N agents ×
/// MAX_TOOL_ROUNDS × per-dispatch ceiling). Each delegation runs sequentially
/// (the local ledger is single-writer; concurrent debits would race the
/// balance read), so this is also the worst-case serial latency multiplier.
pub(crate) const MAX_FANOUT: usize = 10;

/// Result of a local delegation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct LocalDelegateResult {
    pub agent_id: String,
    pub response: String,
    pub model: String,
    pub tokens_used: i64,
    pub cost: i64,
    pub balance: i64,
    /// End-to-end delegation latency in milliseconds (Cybernetic Swarm Plan
    /// component C4 — HyEvo `T_q` measurement). Captured from the start of
    /// `delegate` to just before the result is returned. Pure measurement — no
    /// gate; enables future cost-aware decisions without committing to
    /// evolutionary search. ORIENT surfaces latency outliers so DECIDE can
    /// reconfigure slow agents (audit C4 fix, 2026-08-03).
    pub latency_ms: u64,
    /// Summary of tool calls made during the delegation (qualified
    /// `server/tool` name + ok/error). Empty when the agent declares no
    /// `mcp_tools` or the model made no calls.
    pub tool_calls: Vec<serde_json::Value>,
    /// Summary of skill cascades executed before the LLM call (skill id +
    /// ok/error). Empty when the agent declares no `skills`.
    pub executed_skills: Vec<serde_json::Value>,
    /// Optional deterministic task-success verdict, populated by the executor
    /// (the Kask Curator or a human in the loop) after running a declared
    /// evaluator against `response`. The swarm MCP server cannot judge task
    /// success — `delegate` returns `None` here — so the executor stamps this
    /// before feeding `delegate_results` back to swarm-intelligence. ORIENT
    /// (C5/C6 fault attribution) consumes it to distinguish "executed but
    /// failed the task" from "crashed" (audit Loop B fidelity fix,
    /// 2026-08-03). Skipped from serialization when absent, so the server's
    /// response shape is unchanged for callers that ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_success: Option<TaskSuccessVerdict>,
}

/// How a [`TaskSuccessVerdict`] was produced. The determinism constraint
/// (Cybernetic Swarm Plan C0) requires a deterministic judge; an `llm_judged`
/// provenance is flagged so ORIENT can warn rather than trust the verdict —
/// the audit's Gap S3 (advertised determinism, enforced by convention).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskSuccessProvenance {
    /// Deterministic evaluator: test pass/fail, schema validation, exit code,
    /// regex/reference match. The only provenance ORIENT trusts for the C0
    /// `s` axis of the swarm-state distance.
    Deterministic,
    /// LLM-jged. ORIENT must downgrade this to a hypothesis (warn), not a
    /// trusted `s` — the determinism constraint forbids an LLM judging
    /// `task_success`.
    LlmJudged,
    /// Unknown / not declared by the executor. Treated as untrusted.
    Unknown,
}

/// A deterministic task-success verdict stamped onto a [`LocalDelegateResult`]
/// by the executor (the Kask Curator or a human in the loop) after running a
/// declared evaluator against the delegation `response`. The server returns
/// `None`; the executor populates this. ORIENT consumes it for C5/C6 fault
/// attribution (audit Loop B fidelity fix, 2026-08-03).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskSuccessVerdict {
    /// Whether the delegation's output solved the task per the evaluator.
    pub pass: bool,
    /// Optional graded score in `[0.0, 1.0]` for evaluators that produce one;
    /// when absent, `pass` is the binary signal. ORIENT maps
    /// `s = score` if present else `1.0 if pass else 0.0`.
    pub score: Option<f64>,
    /// Evaluator-readable detail (which check failed, the diff, the exit
    /// code, etc.).
    pub detail: Option<String>,
    /// How the verdict was produced. `Deterministic` is the only trusted
    /// provenance; `LlmJudged` triggers an ORIENT warning (Gap S3).
    pub provenance: TaskSuccessProvenance,
}
