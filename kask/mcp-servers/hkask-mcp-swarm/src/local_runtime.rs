//! Local swarm runtime — inference for `Local` mode (v2 §15).
//!
//! Extracted from the swarm server root. `LazyLocalSwarmRuntime` defers
//! construction to the first tool call (the `run_server` factory is sync).
//! `LocalSwarmRuntime::delegate` runs a local agent: tool loop → measured
//! result. The inference/skill/tool ports are resolved once at construction.
//! There is no local budget: local agents run on the operator's own substrate
//! (operator ruling 2026-09-04 — the budget concept is deprecated; timeouts
//! are the enforcement/kill mechanism), so nothing is priced, gated, or
//! reconciled here.

use std::time::Instant;

/// Bounded capacity of the capture channel. Small on purpose: capture is
/// best-effort telemetry, and a full channel drops-and-counts rather than
/// ever blocking a generation call.
const CAPTURE_CHANNEL_CAPACITY: usize = 256;

use crate::agent_executor::{AgentExecutor, RawDelegateResult};
use crate::error::LocalSwarmError;
use crate::local_registry::LocalAgentCard;
use crate::sanitize::strip_leading_mentions;

/// The local swarm runtime — lazily constructed inference.
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
    /// The per-agent execution-stats store — shared with the server (reads:
    /// `swarm_get_local_agent` / `swarm_list_local_agents`; writes: the
    /// runtime's result path). Passed through to the runtime so recording
    /// and surfacing see one store.
    agent_stats: std::sync::Arc<crate::agent_stats::AgentStatsStore>,
    inner: tokio::sync::OnceCell<LocalSwarmRuntime>,
}

/// The rollout event store, constructed lazily on first write. The store
/// lives at `mcp/swarm/events.db` under the data dir
/// (operator-configurable via `HKASK_SWARM_EVENTS_PATH`) and is the data
/// plane of the event-substrate proposal: `model_request` and `verdict`
/// events for rollout trajectories, opaque pass-through for everything
/// else. Position in the log is identity.
pub struct LazyEventStore {
    db_path: String,
    inner: std::sync::OnceLock<std::sync::Arc<hkask_event_store::EventStore>>,
}

impl LazyEventStore {
    /// Store the config without initializing. The store is constructed on
    /// first call to `get_or_init`.
    pub fn lazy(db_path: String) -> Self {
        Self {
            db_path,
            inner: std::sync::OnceLock::new(),
        }
    }

    /// Get the store, initializing it on first call. Returns `Err` if the
    /// database cannot be opened. Subsequent calls return the cached store.
    pub fn get_or_init(
        &self,
    ) -> Result<std::sync::Arc<hkask_event_store::EventStore>, LocalSwarmError> {
        if let Some(store) = self.inner.get() {
            return Ok(std::sync::Arc::clone(store));
        }
        let store = self.open()?;
        // A racing first-write may have won; either store is equivalent
        // (same schema, same file), so keep whichever is present.
        if self.inner.set(std::sync::Arc::clone(&store)).is_err() {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "event store already initialized by a racing first-write — keeping the winner"
            );
        }
        Ok(store)
    }

    fn open(&self) -> Result<std::sync::Arc<hkask_event_store::EventStore>, LocalSwarmError> {
        if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to create event store dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let manager = hkask_storage::SqliteConnectionManager::file(&self.db_path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| {
                LocalSwarmError::Database(format!("failed to create event store pool: {e}"))
            })?;
        let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
            std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool));
        let store = hkask_event_store::EventStore::from_driver(driver)
            .map_err(|e| LocalSwarmError::Database(format!("failed to init event store: {e}")))?;
        Ok(std::sync::Arc::new(store))
    }
}

impl LazyLocalSwarmRuntime {
    /// Store the config without initializing. The runtime is constructed
    /// on first call to `get_or_init`.
    pub fn lazy(agent_stats: std::sync::Arc<crate::agent_stats::AgentStatsStore>) -> Self {
        Self {
            agent_stats,
            inner: tokio::sync::OnceCell::new(),
        }
    }

    /// Get the runtime, initializing it on first call. Returns `Err` if
    /// initialization fails (inference port resolution). Subsequent calls
    /// return the cached runtime.
    pub async fn get_or_init(&self) -> Result<&LocalSwarmRuntime, LocalSwarmError> {
        self.inner
            .get_or_try_init(|| async { LocalSwarmRuntime::new(self.agent_stats.clone()).await })
            .await
    }
}

/// The initialized local swarm runtime — agent executor + measured stats.
///
/// The *agent-run* policy (skill execution, tool-loop orchestration) lives in
/// `AgentExecutor`; the runtime measures the run (tokens, latency) and
/// records per-agent stats. There is no spending policy: local agents run on
/// the operator's own substrate, so nothing is priced or gated.
pub struct LocalSwarmRuntime {
    /// The agent-run policy (inference + tool dispatch + skill exec).
    /// Constructed once from the resolved IPC-bridge ports; the runtime
    /// calls `executor.run` and measures the result.
    executor: AgentExecutor,
    /// Count of captures dropped because the capture channel was full or
    /// the store append failed. Surfaced via `capture_drops()` in the eval
    /// harness report — a drop is never silent (each drop site also warns
    /// in real time). Shared so the drainer task can increment it while the
    /// runtime hands out `&self`.
    capture_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Per-agent execution stats — the local analog of fermi's
    /// `measured_exec_stats`. Recorded at the result path (single-writer by
    /// construction) and surfaced by the local agent tools.
    stats: std::sync::Arc<crate::agent_stats::AgentStatsStore>,
}

impl LocalSwarmRuntime {
    /// Construct the runtime. Resolves the inference and tool-dispatch
    /// ports once at construction.
    pub(crate) async fn new(
        stats: std::sync::Arc<crate::agent_stats::AgentStatsStore>,
    ) -> Result<Self, LocalSwarmError> {
        // Resolve the agent-run ports once at construction: inference and
        // tool dispatch both route through the zed IPC bridge (or fall back
        // to media/stub when the socket is absent). These compose into the
        // `AgentExecutor`, which owns the agent-run policy. Resolving them
        // here (rather than inside `AgentExecutor::new`) keeps the env-var
        // reads at the runtime construction seam, mirroring the other kask
        // MCP servers.
        let inference = hkask_inference::resolve_inference_port().await;
        let tool_dispatch = hkask_inference::resolve_tool_dispatch_port().await;
        let executor = AgentExecutor::new(inference, tool_dispatch);

        Ok(Self {
            executor,
            capture_drops: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            stats,
        })
    }

    /// Test-only constructor with injected inference and tool-dispatch ports.
    /// The production `new` resolves ports from env (zed IPC bridge or
    /// MediaRouter fallback), which is unsuitable for unit tests. This
    /// constructor accepts the two agent-run ports, which it composes into
    /// an AgentExecutor, so tests can exercise the delegate logic without a
    /// real backend.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        inference: std::sync::Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: std::sync::Arc<dyn hkask_types::ToolDispatchPort>,
    ) -> Self {
        let executor = AgentExecutor::new(inference, tool_dispatch);
        // A throwaway stats store — tests exercise the delegate logic, not
        // stats persistence (that is unit-tested on `AgentStatsStore`
        // directly).
        let stats_dir =
            std::env::temp_dir().join(format!("hkask-swarm-runtime-test-{}", uuid::Uuid::new_v4()));
        let stats = std::sync::Arc::new(crate::agent_stats::AgentStatsStore::load(
            &stats_dir.to_string_lossy(),
        ));
        Self {
            executor,
            capture_drops: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            stats,
        }
    }

    /// Captures dropped due to channel backpressure (send side, counted in
    /// the executor) or store append failure (drainer side). A sensor
    /// signal, not an error: the delegation path must never fail because
    /// capture is degraded — but the degradation must be visible. Consumed
    /// by `swarm_eval_agent_local`, which surfaces the count in the harness
    /// result's `capture_drops` field.
    pub(crate) fn capture_drops(&self) -> usize {
        let send = self
            .executor
            .capture_send_drops()
            .load(std::sync::atomic::Ordering::Relaxed);
        let append = self
            .capture_drops
            .load(std::sync::atomic::Ordering::Relaxed);
        send + append
    }

    /// Wire the event-store capture path: a bounded channel from the
    /// executor's inference loop to a drainer task that appends
    /// `model_request` events. Called by the harness (the store's first
    /// consumer) after the store opens; a second call replaces the channel
    /// (the old drainer exits when its sender is dropped).
    ///
    /// A full channel drops the capture (warned + counted at the send site)
    /// and a store failure warns here and increments the drop counter —
    /// surfaced via `capture_drops()` in the harness report, never silent,
    /// never blocking generation.
    pub(crate) fn wire_capture(&self, store: std::sync::Arc<hkask_event_store::EventStore>) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::agent_executor::CapturedInference>(
            CAPTURE_CHANNEL_CAPACITY,
        );
        self.executor.set_capture(tx);
        let drop_counter = std::sync::Arc::clone(&self.capture_drops);
        tokio::spawn(async move {
            while let Some(captured) = rx.recv().await {
                let payload = serde_json::json!({
                    "model": captured.model,
                    "status": captured.status,
                    "latency_ms": captured.latency_ms,
                    "usage": { "total_tokens": captured.total_tokens },
                    "tool_calls": captured.tool_calls,
                    "round": captured.round,
                    // Bodies for the training bridge. Empty on the error path
                    // (no response) — absent, not fabricated.
                    "request_body": captured.request_body,
                    "response_body": captured.response_body,
                });
                if let Err(error) = store.append(&captured.rollout_id, "model_request", &payload) {
                    drop_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        rollout = %captured.rollout_id,
                        error = %error,
                        "event store append failed — model_request capture dropped"
                    );
                }
            }
        });
    }

    /// Test-only constructor with injected dependencies. Mirrors the
    /// `StubInferencePort` pattern: the production
    /// `new(db_path)` resolves the inference port from env (zed IPC bridge or
    /// MediaRouter fallback), which is unsuitable for unit tests. This
    /// constructor accepts the three agent-run ports
    /// (inference, tool dispatch, skill exec) which it composes into an
    /// `AgentExecutor`, so tests can exercise the `delegate` logic without a
    /// real backend.
    ///
    /// Ensures the operator account exists (same as `new`) so `balance`/
    /// `fund`/`debit` work out of the box.
    /// The resolved local inference port. Exposed so the local knowledge tools
    /// (`swarm_generate_prompt_local` / `swarm_generate_ontology_local`) can do a
    /// one-shot generate via the same inference port the delegate loop uses —
    /// reuse, not a second resolution.
    pub(crate) fn inference(&self) -> std::sync::Arc<dyn hkask_types::InferencePort> {
        self.executor.inference()
    }

    /// Execute a local agent: run the agent (skill execution + tool loop, via
    /// `AgentExecutor::run`) and measure the result. Returns the response
    /// text, model, token usage, latency, and a tool-call summary.
    ///
    /// The agent-run policy (skill execution, tool-loop orchestration) lives
    /// in `AgentExecutor::run`; the runtime measures the run. There is no
    /// budget: local agents run on the operator's own substrate (their
    /// machine, their inference credentials), so there is nothing to price,
    /// gate, or reconcile — funding gates belong on *cloud swarm* delegation,
    /// where credits buy someone else's compute (`spend_gate.rs` + the ABW
    /// consent token).
    ///
    /// Tool dispatch is allowlisted twice: the declared `mcp_tools` set is
    /// the only tool set shown to the model AND the qualified list travels
    /// with every dispatch so the zed-side IPC server enforces it at the
    /// dispatch boundary (a tool outside the card's declared set is never
    /// minted a panel token).
    pub async fn delegate(
        &self,
        agent: &LocalAgentCard,
        task: &str,
    ) -> Result<LocalDelegateResult, LocalSwarmError> {
        let started = Instant::now();
        // Strip leading @mentions (defense-in-depth, mirrors ABW delegate).
        let task_clean = strip_leading_mentions(task);

        // Run the agent (tool loop). A failed run is a real execution
        // failure (fermi counts failed episodes the same way), so it is
        // recorded on the agent's stats before propagating.
        let raw: RawDelegateResult = match self.executor.run(agent, &task_clean).await {
            Ok(raw) => raw,
            Err(error) => {
                self.stats.record_failure(&agent.agent_id);
                return Err(error);
            }
        };

        let mut result = self.build_result(
            raw,
            &agent.agent_id,
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        );
        // Contract checks (the local analog of fermi's verification gates):
        // advisory verdicts stamped here, in the shared execution path, so
        // every delegation surface (delegate, fanout, a2a, pipeline, the
        // eval harness) reports them uniformly — not per-tool.
        let (input_contract_check, output_contract_check) = contract_checks(
            agent.capabilities.input_contract.as_ref(),
            agent.capabilities.output_contract.as_ref(),
            &task_clean,
            &result.response,
        );
        result.input_contract_check = input_contract_check;
        result.output_contract_check = output_contract_check;
        Ok(result)
    }

    /// Build a `LocalDelegateResult` from a raw delegate result and record
    /// the completed execution on the agent's stats. Shared by `delegate`
    /// (sequential) and `delegate_batch` (parallel) so the measurement and
    /// stats recording stay in one place.
    fn build_result(
        &self,
        raw: RawDelegateResult,
        agent_id: &str,
        latency_ms: u64,
    ) -> LocalDelegateResult {
        let tokens = raw.tokens_used;

        // Record the completed execution on the agent's stats — the one
        // point where tokens and latency are both known (fermi's
        // `measured_exec_stats` analog). Non-fatal by contract: a failed
        // flush is warned inside the store and never fails the delegation.
        self.stats.record_success(agent_id, tokens, latency_ms);

        LocalDelegateResult {
            agent_id: agent_id.to_string(),
            response: raw.text,
            model: raw.model,
            tokens_used: tokens,
            latency_ms,
            tool_calls: raw.tool_calls,
            task_success: None,
            bind_matched: None,
            rollout_id: Some(raw.rollout_id),
            reasoning_steps: raw.reasoning_steps,
            input_contract_check: None,
            output_contract_check: None,
        }
    }

    /// Run N delegations concurrently (inference calls in parallel).
    pub async fn delegate_batch(
        &self,
        delegations: Vec<(LocalAgentCard, String)>,
    ) -> Vec<Result<LocalDelegateResult, LocalSwarmError>> {
        // Extract the agent ids for phase 2, since the spawned tasks
        // consume the owned cards.
        let agent_ids: Vec<String> = delegations
            .iter()
            .map(|(agent, _)| agent.agent_id.clone())
            .collect();
        // Contract snapshots and cleaned tasks, collected before the cards
        // are moved into the spawned tasks — phase 2 stamps the contract
        // checks with them, keeping batch parity with `delegate`'s checks.
        let contracts: Vec<(Option<serde_json::Value>, Option<serde_json::Value>)> = delegations
            .iter()
            .map(|(agent, _)| {
                (
                    agent.capabilities.input_contract.clone(),
                    agent.capabilities.output_contract.clone(),
                )
            })
            .collect();
        let tasks: Vec<String> = delegations
            .iter()
            .map(|(_, task)| strip_leading_mentions(task))
            .collect();
        let total = delegations.len();

        // Phase 1: run all inference calls concurrently via tokio JoinSet.
        // Each task returns (index, result) so we can match results back to
        // delegations after join (JoinSet does not preserve submission order).
        let mut join_set = tokio::task::JoinSet::new();
        for (index, (agent, task)) in delegations.into_iter().enumerate() {
            let task_clean = strip_leading_mentions(&task);
            let executor = self.executor.clone();
            join_set.spawn(async move {
                let result = executor.run(&agent, &task_clean).await;
                (index, result)
            });
        }
        let mut raw_results: Vec<(usize, Result<RawDelegateResult, LocalSwarmError>)> =
            Vec::with_capacity(total);
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((index, result)) => raw_results.push((index, result)),
                Err(e) => {
                    // A panicked task loses its index (JoinError does not carry
                    // it). Log and fill the gap after all joins complete.
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        error = %e,
                        "parallel delegation task panicked — filling missing index after join"
                    );
                }
            }
        }
        // Fill missing indices (panicked tasks) with errors so the sort + zip
        // with agent_ids stays aligned. Without this, a missing entry shifts
        // every subsequent result to the wrong agent.
        if raw_results.len() < total {
            let present: std::collections::HashSet<usize> =
                raw_results.iter().map(|(i, _)| *i).collect();
            for index in 0..total {
                if !present.contains(&index) {
                    raw_results.push((
                        index,
                        Err(LocalSwarmError::InvalidInput(
                            "parallel delegation task panicked".to_string(),
                        )),
                    ));
                }
            }
        }
        // Sort by index to restore submission order.
        raw_results.sort_by_key(|(i, _)| *i);
        // Strip the index — we only needed it for sorting.
        let raw_results: Vec<Result<RawDelegateResult, LocalSwarmError>> =
            raw_results.into_iter().map(|(_, r)| r).collect();

        // Phase 2: build the results.
        let mut results = Vec::with_capacity(total);
        for (index, (raw_result, agent_id)) in
            raw_results.into_iter().zip(agent_ids.iter()).enumerate()
        {
            let raw = match raw_result {
                Ok(raw) => raw,
                Err(error) => {
                    // A failed batch entry is a real execution failure
                    // (run error or panicked task) — recorded on the
                    // agent's stats before propagating.
                    self.stats.record_failure(&agent_ids[index]);
                    results.push(Err(error));
                    continue;
                }
            };
            let mut built = self.build_result(raw, agent_id, 0);
            // Contract checks — batch parity with `delegate` (see the note
            // there). `index` is the delegation's submission index: the
            // fill-missing pass guarantees one sorted entry per index, so
            // it aligns with `contracts`/`tasks`.
            let (input_contract_check, output_contract_check) = contract_checks(
                contracts[index].0.as_ref(),
                contracts[index].1.as_ref(),
                &tasks[index],
                &built.response,
            );
            built.input_contract_check = input_contract_check;
            built.output_contract_check = output_contract_check;
            results.push(Ok(built));
        }
        results
    }
}

/// function's job.
pub fn check_bind(card: &crate::local_registry::LocalAgentCard, _task: &str) -> Option<bool> {
    if card.accepts.is_empty() {
        return None;
    }
    if card.accepts.iter().any(|a| a == "text") {
        return Some(true);
    }
    None
}

/// Compute the input/output contract checks for one delegation — the local
/// analog of fermi's verification pipeline (`envelope::validate_input` /
/// `envelope::build`, surfaced as `Gate::InputBinding` / `Gate::OutputSchema`).
/// Both checks are ADVISORY: an `invalid` verdict is recorded on the result,
/// never a blocked execution — mirroring fermi's soft validation, where an
/// invalid input or output is a report, not a refusal.
///
/// Verdict vocabulary (fermi gate names, `schema_validate` statuses):
/// - `valid` — checked and conforming
/// - `invalid` — the document contradicts the declared schema (violations
///   carried alongside)
/// - `unverified_unsupported` — the schema uses keywords the minimal
///   validator cannot interpret; NOT a pass
/// - `unverified_no_schema` — the contract object carries no `schema` key
/// - `unverified_not_json` — the payload is not JSON at all (for a text
///   response against a structured contract, or a plain-text task against
///   a structured input contract — advisory, like fermi's InputBinding)
///
/// Returns `(input_check, output_check)`; each is `None` when the agent
/// declares no contract for that side (fermi: `input_contract: None` means
/// no input validation, same as `output_contract: None`).
pub fn contract_checks(
    input_contract: Option<&serde_json::Value>,
    output_contract: Option<&serde_json::Value>,
    task: &str,
    response: &str,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let input_check = input_contract
        .map(|contract| check_one_side(contract, task))
        .map(|check| {
            check_note(
                check,
                "the caller's task checked against the agent's input_contract",
            )
        });
    let output_check = output_contract
        .map(|contract| check_one_side(contract, response))
        .map(|check| {
            check_note(
                check,
                "the agent's response checked against its output_contract",
            )
        });
    (input_check, output_check)
}

/// Validate one payload (task or response) against one contract's
/// `schema` key. The contract shape is fermi's compiled form:
/// `input_contract: {accepts_schema, title, required, schema}` /
/// `output_contract: {domain, produces_schema, schema, ...}`.
fn check_one_side(contract: &serde_json::Value, payload: &str) -> serde_json::Value {
    let Some(schema) = contract.get("schema") else {
        return serde_json::json!({ "status": "unverified_no_schema" });
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(payload) else {
        return serde_json::json!({
            "status": "unverified_not_json",
            "note": "payload is not JSON; the contract declares a structured schema",
        });
    };
    let result = crate::schema_validate::validate(schema, &doc);
    if !result.unsupported.is_empty() {
        return serde_json::json!({
            "status": "unverified_unsupported",
            "unsupported": result.unsupported,
        });
    }
    if result.violations.is_empty() {
        serde_json::json!({ "status": "valid" })
    } else {
        serde_json::json!({
            "status": "invalid",
            "violations": result
                .violations
                .iter()
                .map(|v| serde_json::json!({ "path": v.path, "message": v.message }))
                .collect::<Vec<_>>(),
        })
    }
}

/// Attach the check's descriptive note (which side was checked) to the
/// verdict object.
fn check_note(mut check: serde_json::Value, note: &str) -> serde_json::Value {
    if let Some(obj) = check.as_object_mut() {
        obj.insert("check".to_string(), serde_json::json!(note));
    }
    check
}

/// Maximum agents dispatched in a single `swarm_fanout_local` call (Cybernetic
/// Swarm Plan — bounds the work amplification of one fan-out: N agents ×
/// MAX_TOOL_ROUNDS). Also the worst-case serial latency multiplier.
pub const MAX_FANOUT: usize = 10;

/// Result of a local delegation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct LocalDelegateResult {
    pub agent_id: String,
    pub response: String,
    pub model: String,
    pub tokens_used: i64,
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
    /// Rung 4 (Binding): whether the request matched at least one declared
    /// `accepts` label. `None` = not checked (no `accepts` declared, or the
    /// label is not `"text"" — absence ≠ contradiction, paper Rule 5.3).
    /// `Some(true)` = the agent declares `accepts: ["text"]`, which is a
    /// universal accept. The runtime classification heuristic that produced
    /// `Some(false)` was deleted (no correct setting — paper Rule 5.2); the
    /// typing layer at admission (`validate_typing`) is the gate that
    /// enforces `accepts` labels resolve to registered types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_matched: Option<bool>,
    /// The rollout id under which this delegation's `model_request` events
    /// were captured (the executor assigns it). Consumers that stamp
    /// verdicts (the harness, the Curator) use it so the verdict groups
    /// with the captured events in the store. Skipped from serialization
    /// when absent (capture unwired) so the response shape is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_id: Option<String>,
    /// Structured reasoning steps recorded by the model via the
    /// `reasoning/think` tool (when the agent card opts into reasoning via
    /// `capabilities.reasoning: true`). Empty when reasoning is not enabled
    /// or the model never called the tool. Consumed by the Curator's ORIENT
    /// phase as a reasoning trace alongside `tool_calls`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_steps: Vec<crate::agent_executor::ReasoningStep>,
    /// Input-contract check — the local analog of fermi's `Gate::InputBinding`
    /// / `Gate::InputSchema`: the caller's task validated against the
    /// agent's declared `input_contract.schema`. `None` = no input contract
    /// declared (unverified, not a pass). Advisory — never blocks execution,
    /// mirroring fermi's soft input validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_contract_check: Option<serde_json::Value>,
    /// Output-contract check — the local analog of fermi's `Gate::OutputSchema`:
    /// the agent's response validated against its declared
    /// `output_contract.schema`. `None` = no output contract declared.
    /// Advisory — an `invalid` verdict is a recorded finding (the agent
    /// contradicted its own declared type), not a failed delegation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract_check: Option<serde_json::Value>,
}

impl LocalDelegateResult {
    /// Shape this delegation result as the per-entry JSON object used by
    /// `swarm_fanout_local`, `swarm_pipeline_local`, and
    /// `swarm_execute_plan_local`. The three tools previously duplicated
    /// this JSON construction inline (with minor field differences); this
    /// method is the single source of truth for the per-delegation result
    /// shape.
    ///
    /// `include_details` controls whether `tool_calls` is included — fanout
    /// surfaces it, pipeline omits it (the pipeline caller cares about the
    /// output chain, not the tool trace).
    pub(crate) fn to_result_json(&self, include_details: bool) -> serde_json::Value {
        let mut entry = serde_json::json!({
            "agent_name": self.agent_id,
            "ok": true,
            "response": self.response,
            "model": self.model,
            "tokens_used": self.tokens_used,
            "latency_ms": self.latency_ms,
        });
        if include_details {
            entry["tool_calls"] = serde_json::Value::Array(self.tool_calls.clone());
            if !self.reasoning_steps.is_empty() {
                entry["reasoning_steps"] =
                    serde_json::to_value(&self.reasoning_steps).unwrap_or(serde_json::Value::Null);
            }
        }
        entry
    }

    /// Shape a failed delegation as the per-entry JSON object. Used by
    /// fanout/pipeline/execute_plan when `delegate` returns `Err`.
    pub(crate) fn error_json(agent_name: &str, error: &str) -> serde_json::Value {
        serde_json::json!({
            "agent_name": agent_name,
            "ok": false,
            "error": error,
        })
    }
}

/// Re-export the canonical verdict provenance type from the event store.
/// `VerdictSource` is the single provenance enum for all verdicts in the
/// system (event-store layer + runtime layer), grounded in Agent
/// Lightning's `RewardData.source` model. The former runtime-only
/// `TaskSuccessProvenance` was merged into it to eliminate the parallel
/// duplication — see `hkask_event_store::types::VerdictSource` for the
/// canonical definition and trust-level documentation.
pub use hkask_event_store::VerdictSource;

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
    /// How the verdict was produced. `DeterministicEvaluator` is the
    /// only automated source trusted for the C0 `s` axis; `LlmJudged`
    /// triggers an ORIENT warning (Gap S3). See `VerdictSource` for the
    /// full trust-level table.
    pub provenance: VerdictSource,
}

#[cfg(test)]
mod contract_check_tests {
    use super::contract_checks;
    use serde_json::json;

    fn input_contract() -> serde_json::Value {
        json!({
            "accepts_schema": "scro/bom-query/1",
            "title": "BOM pricing request",
            "required": ["task", "bom_items"],
            "schema": {
                "type": "object",
                "required": ["task", "bom_items"],
                "properties": {
                    "task": { "type": "string" },
                    "bom_items": { "type": "array" }
                }
            }
        })
    }

    fn output_contract() -> serde_json::Value {
        json!({
            "domain": "supply-chain",
            "produces_schema": "scro/bom-response/1",
            "schema": {
                "type": "object",
                "required": ["items"],
                "properties": {
                    "items": { "type": "array" },
                    "currency": { "type": "string" }
                }
            }
        })
    }

    #[test]
    fn no_contracts_yield_none_checks() {
        let (input, output) = contract_checks(None, None, "any task", "any response");
        assert!(input.is_none());
        assert!(output.is_none());
    }

    #[test]
    fn valid_payloads_pass_both_checks() {
        let task = r#"{"task":"resolve_bom","bom_items":[{"name":"sugar"}]}"#;
        let response = r#"{"items":[{"name":"sugar"}],"currency":"EUR"}"#;
        let (input, output) = contract_checks(
            Some(&input_contract()),
            Some(&output_contract()),
            task,
            response,
        );
        assert_eq!(input.unwrap()["status"], "valid");
        assert_eq!(output.unwrap()["status"], "valid");
    }

    #[test]
    fn schema_violations_report_invalid_with_violations() {
        // Missing required `bom_items` on input; missing required `items`
        // on output.
        let task = r#"{"task":"resolve_bom"}"#;
        let response = r#"{"currency":"EUR"}"#;
        let (input, output) = contract_checks(
            Some(&input_contract()),
            Some(&output_contract()),
            task,
            response,
        );
        let input = input.unwrap();
        assert_eq!(input["status"], "invalid");
        assert!(input["violations"].as_array().unwrap().len() > 0);
        let output = output.unwrap();
        assert_eq!(output["status"], "invalid");
        assert!(output["violations"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn non_json_payloads_are_unverified_not_invalid() {
        // A plain-text task against a structured input contract, and a
        // prose response against a structured output contract — advisory
        // unverified, mirroring fermi's soft InputBinding verdict.
        let (input, output) = contract_checks(
            Some(&input_contract()),
            Some(&output_contract()),
            "price my sugar please",
            "Here is your pricing summary.",
        );
        assert_eq!(input.unwrap()["status"], "unverified_not_json");
        assert_eq!(output.unwrap()["status"], "unverified_not_json");
    }

    #[test]
    fn contract_without_schema_key_is_unverified_no_schema() {
        let contract = json!({ "accepts_schema": "x/1" });
        let (input, _) = contract_checks(Some(&contract), None, "{}", "ok");
        assert_eq!(input.unwrap()["status"], "unverified_no_schema");
    }

    #[test]
    fn unsupported_schema_keywords_are_not_a_pass() {
        // `patternProperties` is outside the minimal validator's closed
        // keyword set — the verdict must say unverified_unsupported, never
        // valid (the `.rules` trap: an unsupported keyword is NOT a pass).
        let contract = json!({
            "schema": { "type": "object", "patternProperties": { "^a": { "type": "string" } } }
        });
        let (input, _) = contract_checks(Some(&contract), None, "{}", "ok");
        let input = input.unwrap();
        assert_eq!(input["status"], "unverified_unsupported");
        assert!(input["unsupported"].as_array().unwrap().len() > 0);
    }
}
