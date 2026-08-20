//! Local swarm runtime — ledger + inference for `Local` mode (v2 §15).
//!
//! Extracted from the swarm server root. `LazyLocalSwarmRuntime` defers
//! construction to the first tool call (the `run_server` factory is sync).
//! `LocalSwarmRuntime::delegate` runs a local agent: tool loop → cost → debit.
//! The ledger is operator-funded; the inference/skill/tool ports are resolved
//! once at construction.

use std::time::Instant;

/// Bounded capacity of the capture channel. Small on purpose: capture is
/// best-effort telemetry, and a full channel drops-and-counts rather than
/// ever blocking a generation call.
const CAPTURE_CHANNEL_CAPACITY: usize = 256;

use crate::agent_executor::{AgentExecutor, RawDelegateResult};
use crate::error::LocalSwarmError;
use crate::local_registry::LocalAgentCard;
use crate::sanitize::strip_leading_mentions;

/// The local swarm runtime — ledger + inference.
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
    inner: tokio::sync::OnceCell<LocalSwarmRuntime>,
}

/// The rollout event store, constructed lazily on first write. The store
/// lives beside the ledger (`mcp/swarm/events.db` under the data dir,
/// operator-configurable via `HKASK_SWARM_EVENTS_PATH`) and is the data
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
        let _ = self.inner.set(std::sync::Arc::clone(&store));
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
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&self.db_path)
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
    pub fn lazy(ledger_path: String) -> Self {
        Self {
            ledger_path,
            inner: tokio::sync::OnceCell::new(),
        }
    }

    /// Get the runtime, initializing it on first call. Returns `Err` if
    /// initialization fails (ledger open, inference port resolution).
    /// Subsequent calls return the cached runtime.
    pub async fn get_or_init(&self) -> Result<&LocalSwarmRuntime, LocalSwarmError> {
        self.inner
            .get_or_try_init(|| async { LocalSwarmRuntime::new(&self.ledger_path).await })
            .await
    }
}

/// The initialized local swarm runtime — ledger + agent executor.
///
/// The runtime owns the *spending* policy (ceiling check, cost computation,
/// spend recording — there is no balance gate). The *agent-run* policy (skill cascade,
/// tool-loop orchestration) lives in `AgentExecutor`.
pub struct LocalSwarmRuntime {
    ledger: std::sync::Arc<hkask_ledger::Ledger>,
    /// The agent-run policy (inference + tool dispatch + skill exec).
    /// Constructed once from the resolved IPC-bridge ports; the runtime calls
    /// `executor.run` then debits.
    executor: AgentExecutor,
    /// The operator's account id in the ledger (funded via `swarm_fund_local`).
    operator_account: String,
    /// The asset name for local credits.
    asset: String,
    /// Count of captures dropped because the capture channel was full or
    /// the store append failed. Surfaced via `capture_drops()` — a drop is
    /// never silent. Shared so the drainer task can increment it while the
    /// runtime hands out `&self`.
    capture_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl LocalSwarmRuntime {
    /// Construct the runtime. Opens (or creates) the ledger at `db_path`,
    /// resolves the inference port.
    ///
    /// The operator account is ensured in the ledger namespace "local_swarm".
    /// It starts at balance 0 — the operator funds it via `swarm_fund_local`.
    pub(crate) async fn new(db_path: &str) -> Result<Self, LocalSwarmError> {
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

        // Resolve the agent-run ports once at construction: inference and
        // tool dispatch both route through the zed IPC bridge (or fall back
        // to media/stub when the socket is absent). These compose into the
        // `AgentExecutor`, which owns the agent-run policy (the runtime owns
        // the spending policy). Resolving them here (rather than inside
        // `AgentExecutor::new`) keeps the env-var reads at the runtime
        // construction seam, mirroring the other kask MCP servers.
        let inference = hkask_inference::resolve_inference_port().await;
        let tool_dispatch = hkask_inference::resolve_tool_dispatch_port().await;
        let executor = AgentExecutor::new(inference, tool_dispatch);

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
            capture_drops: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    /// Captures dropped due to channel backpressure or store failures.
    /// A sensor signal, not an error: the delegation path must never fail
    /// because capture is degraded.
    pub(crate) fn capture_drops(&self) -> usize {
        self.capture_drops
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Wire the event-store capture path: a bounded channel from the
    /// executor's inference loop to a drainer task that appends
    /// `model_request` events. Called by the harness (the store's first
    /// consumer) after the store opens; a second call replaces the channel
    /// (the old drainer exits when its sender is dropped).
    ///
    /// A full channel drops the capture and a store failure increments the
    /// drop counter — surfaced via `capture_drops()`, never silent, never
    /// blocking generation.
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
    /// constructor accepts a pre-built ledger + the three agent-run ports
    /// (inference, tool dispatch, skill exec) which it composes into an
    /// `AgentExecutor`, so tests can exercise the `fund`/`debit`/`delegate`
    /// logic without a real backend.
    ///
    /// Ensures the operator account exists (same as `new`) so `balance`/
    /// `fund`/`debit` work out of the box.
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

    /// Record local spend against the operator's account. Returns the new
    /// balance, which **may be negative**.
    ///
    /// Accounting, not authorization. Local agents run on the operator's own
    /// substrate, so there is no funding gate to enforce (see `delegate`); this
    /// records what was consumed so `swarm_balance_local` and
    /// `swarm_local_history` can reconcile it. A negative balance is the
    /// operator's unreconciled local spend, not a fault.
    ///
    /// Posts the same double-entry transaction `fund` does, in the opposite
    /// direction. Deliberately NOT `Ledger::debit_if_funds`: that refuses on an
    /// insufficient balance, which is exactly the gate local mode must not have.
    /// The TOCTOU concern `debit_if_funds` addressed does not apply — there is no
    /// balance precondition left to race.
    pub(crate) fn record_spend(
        &self,
        amount: i64,
        reference: &str,
    ) -> Result<i64, LocalSwarmError> {
        if amount <= 0 {
            return Err(LocalSwarmError::InvalidInput(
                "spend amount must be positive".to_string(),
            ));
        }
        let tx = hkask_ledger::LedgerTransaction {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
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
            .map_err(|e| LocalSwarmError::Ledger(format!("ledger commit failed: {e}")))?;
        self.balance().ok_or_else(|| {
            LocalSwarmError::Ledger(
                "balance query failed after recording spend — the spend is committed but the \
                 new balance could not be read"
                    .to_string(),
            )
        })
    }

    /// Execute a local agent: run the agent (skill cascade + tool loop, via
    /// `AgentExecutor::run`) → compute cost → debit ledger. Returns the
    /// response text, model, token usage, cost, remaining balance, and a
    /// tool-call summary.
    ///
    /// The agent-run policy (skill cascade, tool-loop orchestration) lives in
    /// `AgentExecutor::run`; the runtime owns the spending policy (ceiling,
    /// balance, cost, debit).
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
        credits_authorized: u32,
        max_credits_per_dispatch: u32,
    ) -> Result<LocalDelegateResult, LocalSwarmError> {
        let started = Instant::now();
        // Strip leading @mentions (defense-in-depth, mirrors ABW delegate).
        let task_clean = strip_leading_mentions(task);

        // Check the per-dispatch ceiling.
        if credits_authorized > max_credits_per_dispatch {
            return Err(LocalSwarmError::InvalidInput(format!(
                "credits_authorized {credits_authorized} exceeds per-dispatch ceiling \
                 {max_credits_per_dispatch} (raise HKASK_ABW_MAX_CREDITS to authorize)"
            )));
        }

        // NO balance gate. Local agents run on the operator's own substrate
        // (their machine, their inference credentials), so there is nothing for
        // this server to withhold: refusing to run costs the operator the work
        // while saving them nothing. Funding gates belong on *cloud swarm* delegation,
        // where credits buy someone else's compute (`spend_gate.rs` + the ABW
        // consent token).
        //
        // The local ledger is retained as **accounting, not authorization** —
        // `swarm_balance_local` / `swarm_local_history` remain the reconciliation
        // surface, and the debit below still records what was spent. A negative
        // balance is therefore normal and meaningful: it is the operator's
        // unreconciled local spend, not a fault.
        //
        // The per-dispatch ceiling above IS retained: it bounds a single runaway
        // dispatch (a cost-amplification limit), which is a different concern
        // from whether an account is funded.

        // Run the agent (tool loop). The executor returns the RAW output —
        // it does NOT debit the ledger.
        let raw: RawDelegateResult = self.executor.run(agent, &task_clean).await?;

        // Compute the cost: 1 credit per 1000 tokens (mirrors ABW's
        // `execution_fee`), summed across tool-loop rounds.
        //
        // `cost` stays capped at `credits_authorized` — that is the operator's
        // declared budget and what the ledger charges. But the cap makes the
        // recorded figure UNDER-state real spend whenever a delegation overruns
        // it, and the local ledger is now purely a reconciliation surface, so a
        // silent understatement corrupts the only data that surface exists to
        // provide. `cost_uncapped` is carried alongside so the gap is visible,
        // and a bounded overrun is warned about rather than swallowed.
        let tokens = raw.tokens_used;
        let cost_uncapped = std::cmp::max(1, tokens / 1000);
        let cost = std::cmp::min(cost_uncapped, i64::from(credits_authorized));
        if cost_uncapped > cost {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                agent = %agent.agent_id,
                tokens,
                recorded_cost = cost,
                actual_cost = cost_uncapped,
                credits_authorized,
                "delegation exceeded its authorized budget - the ledger records the \
                 capped cost, so it under-states real spend by {} credits",
                cost_uncapped - cost
            );
        }

        // Record the spend after the agent run succeeds. Accounting only: a
        // failure here must not fail a delegation that already happened (and
        // already consumed the operator's inference credentials). Losing the
        // record is a reconciliation gap, so it is logged loudly.
        //
        // `balance` stays `None` when it could not be measured. It must NOT fall
        // back to a number: SENSE reads this as the Onto4MAT `energy` property and
        // DECIDE branches on it, so a fabricated value would be read as a real
        // measurement (the `.rules` "unwrap_or(0) on regulation sense inputs is a
        // broken feedback loop" trap — a failed read is not a measured zero).
        let reference = format!("delegate-{}-{}", agent.agent_id, uuid::Uuid::new_v4());
        let new_balance: Option<i64> = match self.record_spend(cost, &reference) {
            Ok(balance) => Some(balance),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    agent = %agent.agent_id,
                    cost,
                    %error,
                    "local spend could not be recorded - the delegation succeeded but the \
                     ledger is now behind by this amount (reconciliation gap)"
                );
                // Try one direct read: the commit may have failed while the
                // balance remains readable. Still `None` if that also fails.
                let fallback = self.balance();
                if fallback.is_none() {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        agent = %agent.agent_id,
                        "balance is unmeasurable after a failed spend record - reporting \
                         null rather than a fabricated value"
                    );
                }
                fallback
            }
        };

        Ok(LocalDelegateResult {
            agent_id: agent.agent_id.clone(),
            response: raw.text,
            model: raw.model,
            tokens_used: tokens,
            cost,
            cost_uncapped,
            balance: new_balance,
            latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            tool_calls: raw.tool_calls,
            // The server cannot judge task success — the executor (Curator or
            // human) stamps this after running a declared deterministic
            // evaluator against `response`. Left `None` here; ORIENT reads it
            // from the executor-populated `delegate_results`.
            task_success: None,
            bind_matched: None,
            rollout_id: Some(raw.rollout_id),
        })
    }
}
/// Rung 4 (Binding): does the request match any declared `accepts` label?
///
/// Returns `None` when the agent declares no `accepts` (absence ≠
/// contradiction, paper Rule 5.3). `text` is treated as a universal accept
/// — an agent declaring `accepts: ["text"]` matches any request. For any
/// other label, the bind check returns `None` (cannot determine): runtime
/// classification of free-text requests is a heuristic with no correct
/// setting (widen it and it swallows structured ports, narrow it and it
/// misses real declarations), so it was deleted. The typing layer at
/// admission (`validate_typing`) is the gate that enforces `accepts`
/// labels resolve to registered types; runtime bind matching against
/// those labels is the typing layer's unfinished transition, not this
/// function's job.
pub(crate) fn check_bind(
    card: &crate::local_registry::LocalAgentCard,
    _task: &str,
) -> Option<bool> {
    if card.accepts.is_empty() {
        return None;
    }
    if card.accepts.iter().any(|a| a == "text") {
        return Some(true);
    }
    None
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
    /// Credits recorded for this delegation.
    ///
    /// **Accounting note:** this is capped at `credits_authorized`, so when actual
    /// token spend exceeds the authorized budget it UNDER-states real cost. See
    /// `cost_uncapped` for the uncapped figure; the two differ exactly when the
    /// cap bound the recording.
    pub cost: i64,
    /// What this delegation would have cost with no cap applied.
    ///
    /// Present so the ledger's understatement is visible rather than silent: when
    /// `cost_uncapped > cost`, the ledger is behind real spend by the difference.
    /// `credits_authorized` remains a genuine bound on what is *charged*, but a
    /// reconciliation surface must not hide what was actually consumed.
    pub cost_uncapped: i64,
    /// The ledger balance after recording this delegation's spend.
    ///
    /// `None` means **not measured** (the balance read failed), never "zero".
    /// SENSE consumes this as the Onto4MAT `energy` property and DECIDE branches
    /// on it, so a fabricated number would enter the regulation loop as a real
    /// measurement (the `.rules` broken-feedback-loop trap).
    ///
    /// May be negative in local mode: the local ledger records spend rather than
    /// authorizing it, so a negative balance is accumulated unreconciled spend.
    pub balance: Option<i64>,
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
            "cost": self.cost,
            "cost_uncapped": self.cost_uncapped,
            "latency_ms": self.latency_ms,
        });
        if include_details {
            entry["tool_calls"] = serde_json::Value::Array(self.tool_calls.clone());
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
