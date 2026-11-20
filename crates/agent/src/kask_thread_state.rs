//! Kask-specific per-thread state, grouped so upstream `Thread`'s struct
//! definition stays clean and upstream rebases touch one field instead of
//! nine.
//!
//! Methods on this struct encapsulate the kask-specific turn-loop behaviors
//! that were previously inline in `Thread`'s methods. See
//! git history (`plans/thread-hooks-refactor-2026-08-26.md`, deleted after
//! implementation) for the full plan.
//!
//! ## D-seam mapping
//!
//! | Field | D-seam | Purpose |
//! |-------|--------|---------|
//! | `agent_id` | D6 | Memory ingestion routing (Curator vs user). Identity
//! | | | is by construction: `ZED_AGENT_ID` at birth, curator overlay or
//! | | | parent inheritance overrides — no read-site default, so a thread
//! | | | can never be silently classified (2026-09-27 h_mem `cd1ef984`) |
//! | `agent_static_context` | D2 | Curator overlay / Steer mode system prompt |

//! | `tool_retry_tracker` | .rules | Tool retry death spiral prevention |
//! | `last_completion_truncated` | D25 | Distinguish MaxTokens truncation from user cancel |
//! | `cached_system_prompt` | — | System prompt digest caching |
//! | `cached_filtered_context` | — | Filtered context caching |
//! | `active_skill` | D59 | Attribute tool failures to the running skill |
//! | `private_inference` | D100 | Thread privacy mark: the routing gate + the ingest provenance |
//! | | | signal. PI-surface-set (the thread-header toggle), never |
//! | | | agent-callable; subagents inherit it (content flows through them). |
//! | | | A marked thread runs only Zed's local Ollama or llama.cpp models |

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use acp_thread::{ClientUserMessageId, RequestProvenance};
use agent_client_protocol::v2::schema as acp_v2;
use collections::HashMap;
use futures::channel::oneshot;
use gpui::SharedString;
use language_model::LanguageModelCompletionError;
use project::AgentId;

use crate::thread::{CachedFilteredContext, CachedSystemPrompt};
use crate::tool_retry_tracker::ToolRetryTracker;

/// All kask-specific per-thread state. Created with `new()` for both
/// upstream Zed and kask threads; the thread is born a `ZED_AGENT_ID`
/// thread, and kask-specific setters (`set_agent_id`, `set_static_context`,
/// etc., called by `NativeAgent::new_session`) override from there.
pub(crate) struct KaskThreadState {
    // Identity (D6) — always set; never read through a default
    agent_id: AgentId,

    // System prompt overlays (D2)
    agent_static_context: Option<SharedString>,

    // Tool retry cap (.rules)
    tool_retry_tracker: Rc<RefCell<ToolRetryTracker>>,

    // Truncation detection (D25)
    last_completion_truncated: bool,

    // zed-kask: false-halt fix (card bd7dbf68) — set when the event loop
    // breaks on a streaming tool's error while its (or a sibling's) input is
    // still streaming, abandoning the in-flight parallel batch. Read by
    // `flush_pending_message` to report never-started tool uses as
    // interrupted-by-sibling-failure rather than falsely as canceled by the
    // user. Reset alongside `last_completion_truncated` at the start of each
    // completion request.
    interrupted_by_tool_failure: bool,

    // Caching
    cached_system_prompt: Option<CachedSystemPrompt>,
    cached_filtered_context: Option<CachedFilteredContext>,

    // D59 — the most recently activated skill in this thread; tool failures
    // while it is set are recorded against it without the model reporting.
    active_skill: Rc<RefCell<Option<SharedString>>>,

    // D100 (Seam A) — the thread privacy mark. PI-surface-set only (the
    // thread-header toggle; the rejected-module register rules out an
    // agent-callable unmark tool — unmarking widens exposure). Never a
    // default-read: `false` is the honest pre-D100 history, and the routing
    // gates read the explicit flag, never an inference from other state.
    private_inference: bool,

    // Verified per-turn execution provenance keyed by ClientUserMessageId, plus
    // the most recent turn's provenance for direct inspection.
    request_provenance: HashMap<ClientUserMessageId, RequestProvenance>,
    latest_provenance: Option<RequestProvenance>,

    // Session-scoped RAII elicitation hold barrier: serializes overlapping
    // human ActionChoice / Elicitation / mandatory PermissionGrant modals in
    // FIFO order and holds mutating sibling tools while a human prompt is live.
    elicitation_barrier: ElicitationHoldBarrier,

    // D100-isolated, BLAKE3 content-addressed spillover vault for truncated
    // and condensed tool outputs (`kask://spillover/<blake3>`).
    spillover_vault: hkask_condenser::spillover::SpilloverVault,
}

impl KaskThreadState {
    pub fn new() -> Self {
        Self {
            agent_id: crate::ZED_AGENT_ID.clone(),
            agent_static_context: None,
            tool_retry_tracker: Rc::new(RefCell::new(ToolRetryTracker::default())),
            last_completion_truncated: false,
            interrupted_by_tool_failure: false,
            cached_system_prompt: None,
            cached_filtered_context: None,
            active_skill: Rc::new(RefCell::new(None)),
            private_inference: false,
            request_provenance: HashMap::default(),
            latest_provenance: None,
            elicitation_barrier: ElicitationHoldBarrier::new(),
            spillover_vault: hkask_condenser::spillover::SpilloverVault::new(),
        }
    }

    pub(crate) fn elicitation_barrier(&self) -> ElicitationHoldBarrier {
        self.elicitation_barrier.clone()
    }

    pub(crate) fn spillover_vault(&self) -> hkask_condenser::spillover::SpilloverVault {
        self.spillover_vault.clone()
    }

    pub(crate) fn active_skill_handle(&self) -> Rc<RefCell<Option<SharedString>>> {
        self.active_skill.clone()
    }

    // ── Truncation detection (D25) ────────────────────────────────────

    /// Called when a completion is truncated before it finished: either a
    /// `StopReason::MaxTokens` stop (D25) or a stream that ended
    /// mid-tool-call (D36 `ToolCallTruncated`). Sets the flag
    /// `flush_pending_message` reads to report pending tool calls as
    /// truncated rather than canceled.
    pub fn on_completion_truncated(&mut self) {
        self.last_completion_truncated = true;
    }

    /// zed-kask: D36 — whether a completion stream error is the truncation
    /// classification: the stream ended (`finish_reason: "stop"`) with a
    /// tool call's arguments still incomplete, so the mapper refused to
    /// dispatch it. Read at the stream-error boundary to set
    /// [`Self::on_completion_truncated`], so pending partial tool input is reported
    /// as truncated rather than canceled.
    pub fn is_tool_call_truncation(error: &LanguageModelCompletionError) -> bool {
        matches!(
            error,
            LanguageModelCompletionError::ToolCallTruncated { .. }
        )
    }

    /// Whether the last completion was truncated. Read by
    /// `flush_pending_message` to distinguish stream-truncated tool calls
    /// from genuine user cancellations.
    pub fn last_completion_truncated(&self) -> bool {
        self.last_completion_truncated
    }

    /// Reset the truncation flag. Called at the start of each completion
    /// request so the flag reflects only the most recent completion.
    pub fn reset_truncation_flag(&mut self) {
        self.last_completion_truncated = false;
    }

    /// zed-kask: false-halt fix (card bd7dbf68) — set when the event loop
    /// breaks on a streaming tool's error while input is still streaming,
    /// abandoning the in-flight batch. The tool uses that never started (a
    /// sibling failure interrupted the stream before they were dispatched)
    /// are reported as interrupted-by-sibling-failure, never as a user
    /// cancellation.
    pub fn on_tool_failure_interrupted_batch(&mut self) {
        self.interrupted_by_tool_failure = true;
    }

    /// Whether the last completion's batch was interrupted by a streaming
    /// tool failure. Read by `flush_pending_message` to distinguish
    /// sibling-failure interruption from genuine user cancellation.
    pub fn batch_interrupted_by_tool_failure(&self) -> bool {
        self.interrupted_by_tool_failure
    }

    /// Reset the interruption flag. Called at the start of each completion
    /// request alongside `reset_truncation_flag` so the flag reflects only
    /// the most recent completion.
    pub fn reset_batch_interruption_flag(&mut self) {
        self.interrupted_by_tool_failure = false;
    }

    /// Warning for a turn that ends with `StopReason::MaxTokens` (D43).
    /// Upstream sends the stop reason with no operator-visible signal, so
    /// threads appear to "just stop" mid-generation. The message
    /// distinguishes the zero-content signature (prompt → silence →
    /// nothing — the operator sees a dead thread) from partial-content
    /// truncation, and names the model so the log line is actionable
    /// without cross-referencing the thread.
    pub(crate) fn max_tokens_turn_end_warning(model_name: &str, produced_content: bool) -> String {
        if produced_content {
            format!(
                "Turn ended at the model's token limit (StopReason::MaxTokens) after \
                 producing partial content (model: {model_name}). The turn stopped \
                 mid-generation with no error shown to the operator."
            )
        } else {
            format!(
                "Turn ended at the model's token limit (StopReason::MaxTokens) with NO \
                 content (model: {model_name}) — the silent-stop signature: prompt → \
                 silence → nothing. Check the provider's reasoning/output token budget."
            )
        }
    }

    /// Detail line for a turn that fails with a provider rejection (D43).
    /// `ProviderRejection`'s Display is the provider's wire message only —
    /// the status, provider code, category, and retry-after that the variant
    /// preserves "so callers that need the raw wire details (diagnostics,
    /// provider-specific workarounds) never lose them" reach no log, because
    /// the turn-failure site formats the anyhow wrapper, whose Display chain
    /// carries none of them. The 2026-09-10 "Provider returned error"
    /// incident (OpenRouter's generic upstream-failure wrapper; five turn
    /// failures across three sessions in three minutes) was unrootable from
    /// the log: credit exhaustion (402), rate limiting (429), an upstream
    /// outage (502 — retried four times over ~75 s before the turn failed)
    /// and a statusless mid-stream rejection are indistinguishable, yet each
    /// demands different operator action. Returns `None` for non-rejections,
    /// whose Display already carries the full story.
    pub(crate) fn provider_rejection_turn_end_warning(
        error: &LanguageModelCompletionError,
    ) -> Option<String> {
        let LanguageModelCompletionError::ProviderRejection {
            provider,
            status,
            code,
            message,
            retry_after,
            category,
        } = error
        else {
            return None;
        };
        let status = status
            .map(|status| status.as_u16().to_string())
            .unwrap_or_else(|| "none (mid-stream rejection)".to_string());
        let code = code.clone().unwrap_or_else(|| "none".to_string());
        let retry_after = retry_after
            .map(|delay| format!("{delay:?}"))
            .unwrap_or_else(|| "none".to_string());
        // `is_transient` is the exact predicate `retry_strategy_for` gates
        // on, so the verdict tells the operator whether the failure followed
        // retry exhaustion or happened on the first attempt.
        let classification = if error.is_transient() {
            "transient — the turn was retried before failing"
        } else {
            "permanent — the turn was not retried"
        };
        Some(format!(
            "Provider rejection detail — provider: {}, status: {status}, code: {code}, \
             category: {category:?}, retry_after: {retry_after}, classification: {classification}. \
             Wire message: {message:?}",
            provider.0
        ))
    }

    // ── System prompt caching ─────────────────────────────────────────

    /// Get the cached system prompt if the digest matches.
    pub fn cached_system_prompt(&self, digest: &[u8; 32]) -> Option<SharedString> {
        self.cached_system_prompt
            .as_ref()
            .filter(|c| c.digest == *digest)
            .map(|c| c.prompt.clone())
    }

    /// Store a rendered system prompt with its digest.
    pub fn cache_system_prompt(&mut self, digest: [u8; 32], prompt: SharedString) {
        self.cached_system_prompt = Some(CachedSystemPrompt { digest, prompt });
    }

    /// Bust the system prompt cache when static context changes.
    pub fn bust_system_prompt_cache(&mut self) {
        self.cached_system_prompt = None;
    }

    // ── Filtered context caching ─────────────────────────────────────

    /// Get the cached filtered context if the digest matches.
    pub fn cached_filtered_context(
        &self,
        digest: &[u8; 32],
    ) -> Option<&prompt_store::ProjectContext> {
        self.cached_filtered_context
            .as_ref()
            .filter(|c| c.filter_digest == *digest)
            .map(|c| &c.context)
    }

    /// Store a filtered context with its digest.
    pub fn cache_filtered_context(
        &mut self,
        digest: [u8; 32],
        context: prompt_store::ProjectContext,
    ) {
        self.cached_filtered_context = Some(CachedFilteredContext {
            filter_digest: digest,
            context,
        });
    }

    /// Whether a filtered context cache entry exists. For test assertions.
    #[cfg(test)]
    pub fn has_cached_filtered_context(&self) -> bool {
        self.cached_filtered_context.is_some()
    }

    // ── Static context (D2) ──────────────────────────────────────────

    /// Static context rendered in the system prompt's Session Context
    /// section (e.g., Curator overlay, Steer panel overlay).
    pub fn static_context(&self) -> Option<&SharedString> {
        self.agent_static_context.as_ref()
    }

    /// Set the static context. Busts the system prompt cache.
    pub fn set_static_context(&mut self, context: SharedString) {
        self.agent_static_context = Some(context);
        self.bust_system_prompt_cache();
    }

    // ── Agent identity (D6) ──────────────────────────────────────────

    /// The agent ID that owns this thread (D6 routing key). Always set —
    /// `ZED_AGENT_ID` at construction, overridden by the curator overlay
    /// (`new_session`) or parent inheritance (`inherit_from`).
    pub fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    /// Set the agent ID.
    pub fn set_agent_id(&mut self, agent_id: AgentId) {
        self.agent_id = agent_id;
    }

    // ── Private-inference routing mark (D100) ───────────────────────

    /// Whether this thread is privacy-marked (D100): its inference
    /// resolves only to a local Ollama or llama.cpp model, and content
    /// it produces carries the private provenance mark at ingest. Curator and Z-K threads carry
    /// the identical mark — the enforcement reads this flag, never the
    /// agent identity (PI ruling 2026-10-30).
    pub fn is_private_inference(&self) -> bool {
        self.private_inference
    }

    /// Whether any recorded turn provenance on this thread was private.
    pub(crate) fn has_private_history(&self) -> bool {
        self.request_provenance.values().any(|p| p.private)
            || self.latest_provenance.as_ref().is_some_and(|p| p.private)
    }

    /// Set the privacy-mark flag (D100). The thread-level setter
    /// (`Thread::set_private_inference`) owns the routing consequences
    /// (clearing a held public model); this is the raw flag home. The
    /// persisted restore (`Thread::from_db`) and subagent inheritance
    /// (`inherit_from`) are the other legitimate callers.
    ///
    /// S109-F01: monotonic one-way latch — refuses `true -> false` once any
    /// private provenance has touched the thread.
    pub(crate) fn set_private_inference(&mut self, private: bool) {
        if !private && self.private_inference && self.has_private_history() {
            log::warn!(
                target: "agent.thread",
                "S109-F01: refused private_inference true -> false transition after private history"
            );
            return;
        }
        if self.private_inference != private {
            self.private_inference = private;
            self.bust_system_prompt_cache();
        }
    }

    pub(crate) fn record_request_provenance(
        &mut self,
        user_message_id: Option<&ClientUserMessageId>,
        provenance: RequestProvenance,
    ) {
        if let Some(id) = user_message_id {
            self.request_provenance
                .insert(id.clone(), provenance.clone());
        }
        self.latest_provenance = Some(provenance);
    }

    pub(crate) fn update_latest_provenance_model(
        &mut self,
        user_message_id: Option<&ClientUserMessageId>,
        model: SharedString,
    ) -> Option<RequestProvenance> {
        if let Some(latest) = self.latest_provenance.as_mut() {
            latest.model = model.clone();
        }
        if let Some(id) = user_message_id
            && let Some(entry) = self.request_provenance.get_mut(id)
        {
            entry.model = model;
            return Some(entry.clone());
        }
        self.latest_provenance.clone()
    }

    pub(crate) fn request_provenance_map(
        &self,
    ) -> &HashMap<ClientUserMessageId, RequestProvenance> {
        &self.request_provenance
    }

    pub(crate) fn restore_request_provenance(
        &mut self,
        map: HashMap<ClientUserMessageId, RequestProvenance>,
    ) {
        self.latest_provenance = map.values().last().cloned();
        self.request_provenance = map;
    }

    pub(crate) fn request_provenance_for(
        &self,
        id: &ClientUserMessageId,
    ) -> Option<&RequestProvenance> {
        self.request_provenance.get(id)
    }

    pub(crate) fn latest_request_provenance(&self) -> Option<&RequestProvenance> {
        self.latest_provenance.as_ref()
    }

    // ── Tool retry cap (.rules) ──────────────────────────────────────

    /// Get a handle to the retry tracker for cloning into async blocks.
    /// The `Rc<RefCell<>>` is cloned and moved into the spawned task,
    /// which is safe because `run_tool` spawns on the foreground executor.
    pub fn retry_tracker_handle(&self) -> Rc<RefCell<ToolRetryTracker>> {
        self.tool_retry_tracker.clone()
    }

    /// Check the tool retry cap before running a tool.
    pub fn check_tool_retry(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> crate::tool_retry_tracker::RetryVerdict {
        self.tool_retry_tracker.borrow().check(tool_name, input)
    }

    /// Check the success-repetition streak before running a tool: the guard
    /// over consecutive identical SUCCESSFUL executions of the same
    /// (tool, input) pair — the class the failure-side check above cannot
    /// see (a success resets the failure counters).
    pub fn check_repetition(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> crate::tool_retry_tracker::RepeatVerdict {
        self.tool_retry_tracker
            .borrow()
            .check_repetition(tool_name, input)
    }

    // ── Subagent inheritance (D6/D34) ────────────────────────────────

    /// Inherit state from a parent thread's KaskThreadState (for subagents).
    /// `agent_id` routes the child's turns to the parent's sovereign store;
    /// `private_inference` (D100) keeps the child inside the parent's routing
    /// boundary — a private thread's subagent content is private-thread
    /// content, so it must not fall back to a public provider the parent is
    /// barred from. All other state starts fresh. The parent's identity is
    /// always set (identity by construction), so the child's is too.
    pub fn inherit_from(parent: &KaskThreadState) -> Self {
        let mut state = Self::new();
        state.agent_id = parent.agent_id.clone();
        state.private_inference = parent.private_inference;
        state.spillover_vault = parent.spillover_vault.clone();
        state
    }
}

/// Map `hkask_types::InferenceProvenance` (`D100`) to `SpilloverPrivacy`.
pub(crate) fn provenance_to_spillover_privacy(
    provenance: hkask_types::InferenceProvenance,
) -> hkask_condenser::spillover::SpilloverPrivacy {
    match provenance {
        hkask_types::InferenceProvenance::PublicProvider => {
            hkask_condenser::spillover::SpilloverPrivacy::Public
        }
        hkask_types::InferenceProvenance::PrivateInference => {
            hkask_condenser::spillover::SpilloverPrivacy::Private
        }
    }
}

impl Default for KaskThreadState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Session-scoped RAII Elicitation Hold Barrier ──────────────────────────

/// Concurrency mode for a human prompt on [`ToolCallEventStream`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElicitationPromptMode {
    /// Exclusive human prompt (`ActionChoice`, `Elicitation`, or mandatory
    /// non-settings-delegable `PermissionGrant` such as `authorize_always_prompt`,
    /// `authorize_windows_fs_warning`, or `authorize_sandbox_fallback`).
    /// Never overlaps with any other human prompt on the same thread.
    Exclusive,
    /// Settings-driven `PermissionGrant` (`authorize`,
    /// `authorize_third_party_tool`, `authorize_sandbox`), where an "Always
    /// allow" response on one pending card can auto-resolve sibling
    /// settings-driven permission cards via `SettingsStore`. May share only
    /// with adjacent `SharedSettingsPermission` prompts when no `Exclusive`
    /// prompt is active or ahead in the FIFO queue; still holds unprompted
    /// mutating siblings until resolved.
    SharedSettingsPermission,
}

#[derive(Default)]
enum ActivePrompts {
    #[default]
    Idle,
    Exclusive(u64),
    SharedSettings(Vec<u64>),
}

struct QueuedPrompt {
    id: u64,
    mode: ElicitationPromptMode,
    wake_tx: oneshot::Sender<()>,
}

#[derive(Default)]
struct ElicitationBarrierState {
    next_id: u64,
    pending_reservations: usize,
    active: ActivePrompts,
    prompt_waiters: VecDeque<QueuedPrompt>,
    mutation_waiters: Vec<oneshot::Sender<()>>,
}

/// Session-scoped coordinator that enforces two invariants on parallel tool batches:
/// 1. Exclusive human-decision / elicitation prompts execute in strict FIFO order
///    with at most 1 active modal on the thread at any instant.
/// 2. Unprompted mutating sibling tools pause before mutating while any human
///    prompt (or pre-prompt reservation) is active on the thread, while
///    read-only non-eliciting siblings execute concurrently without blocking.
#[derive(Clone, Default)]
pub(crate) struct ElicitationHoldBarrier {
    inner: Arc<Mutex<ElicitationBarrierState>>,
}

impl ElicitationHoldBarrier {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ElicitationBarrierState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve a pre-prompt hold synchronously inside `Thread::run_tool` for
    /// tools that unconditionally elicit human judgment (`kanban_goal_close`,
    /// `kanban_goal_correct_outcome`, `ask_user`, `lean_check`), closing the
    /// batch-scheduling window before the tool's spawned task reaches its
    /// prompt call.
    pub(crate) fn reserve_for_interactive_tool(&self) -> PendingElicitationReservation {
        let mut state = self.lock();
        state.pending_reservations += 1;
        PendingElicitationReservation {
            barrier: self.clone(),
            consumed: false,
        }
    }

    /// Enqueue a human prompt in strict synchronous call order (FIFO),
    /// atomically absorbing any pre-prompt reservation held by the caller.
    pub(crate) fn enqueue_prompt(
        &self,
        mode: ElicitationPromptMode,
        reservation: Option<PendingElicitationReservation>,
    ) -> ElicitationTicket {
        let mut state = self.lock();
        if let Some(mut res) = reservation
            && !res.consumed
        {
            res.consumed = true;
            state.pending_reservations = state.pending_reservations.saturating_sub(1);
        }

        let id = state.next_id;
        state.next_id += 1;

        let can_activate_now = state.prompt_waiters.is_empty()
            && match (&state.active, mode) {
                (ActivePrompts::Idle, _) => true,
                (
                    ActivePrompts::SharedSettings(_),
                    ElicitationPromptMode::SharedSettingsPermission,
                ) => true,
                _ => false,
            };

        if can_activate_now {
            match (&mut state.active, mode) {
                (ActivePrompts::Idle, ElicitationPromptMode::Exclusive) => {
                    state.active = ActivePrompts::Exclusive(id);
                }
                (ActivePrompts::Idle, ElicitationPromptMode::SharedSettingsPermission) => {
                    state.active = ActivePrompts::SharedSettings(vec![id]);
                }
                (
                    ActivePrompts::SharedSettings(ids),
                    ElicitationPromptMode::SharedSettingsPermission,
                ) => {
                    ids.push(id);
                }
                _ => unreachable!("checked by can_activate_now"),
            }
            ElicitationTicket::Active(ElicitationHoldGuard {
                barrier: self.clone(),
                id,
            })
        } else {
            let (wake_tx, wake_rx) = oneshot::channel();
            state
                .prompt_waiters
                .push_back(QueuedPrompt { id, mode, wake_tx });
            ElicitationTicket::Queued(QueuedElicitationTicket {
                barrier: self.clone(),
                id,
                wake_rx,
                promoted: false,
            })
        }
    }

    /// Suspend a mutating sibling tool while any human prompt or interactive
    /// reservation is active on this thread. Holds no lock across `.await`,
    /// avoiding RwLock upgrade deadlocks when two mutating tools both prompt.
    pub(crate) async fn wait_while_held(&self) {
        loop {
            let rx = {
                let mut state = self.lock();
                if !has_active_or_pending_hold(&state) {
                    return;
                }
                let (tx, rx) = oneshot::channel();
                state.mutation_waiters.push(tx);
                rx
            };
            let _ = rx.await;
        }
    }

    fn release_prompt_id(&self, id: u64) {
        let mut state = self.lock();
        match &mut state.active {
            ActivePrompts::Idle => {}
            ActivePrompts::Exclusive(active_id) => {
                if *active_id == id {
                    state.active = ActivePrompts::Idle;
                }
            }
            ActivePrompts::SharedSettings(active_ids) => {
                active_ids.retain(|active_id| *active_id != id);
                if active_ids.is_empty() {
                    state.active = ActivePrompts::Idle;
                }
            }
        }
        state.prompt_waiters.retain(|w| w.id != id);
        promote_ready_waiters(&mut state);
        wake_mutation_waiters_if_clear(&mut state);
    }
}

fn has_active_or_pending_hold(state: &ElicitationBarrierState) -> bool {
    state.pending_reservations > 0
        || !matches!(state.active, ActivePrompts::Idle)
        || !state.prompt_waiters.is_empty()
}

fn promote_ready_waiters(state: &mut ElicitationBarrierState) {
    while let Some(front_mode) = state.prompt_waiters.front().map(|w| w.mode) {
        let can_promote = match (&state.active, front_mode) {
            (ActivePrompts::Idle, _) => true,
            (ActivePrompts::SharedSettings(_), ElicitationPromptMode::SharedSettingsPermission) => {
                true
            }
            _ => false,
        };
        if !can_promote {
            break;
        }

        let waiter = state.prompt_waiters.pop_front().expect("checked front");
        if waiter.wake_tx.send(()).is_ok() {
            match (&mut state.active, waiter.mode) {
                (ActivePrompts::Idle, ElicitationPromptMode::Exclusive) => {
                    state.active = ActivePrompts::Exclusive(waiter.id);
                }
                (ActivePrompts::Idle, ElicitationPromptMode::SharedSettingsPermission) => {
                    state.active = ActivePrompts::SharedSettings(vec![waiter.id]);
                }
                (
                    ActivePrompts::SharedSettings(ids),
                    ElicitationPromptMode::SharedSettingsPermission,
                ) => {
                    ids.push(waiter.id);
                }
                _ => unreachable!("checked by can_promote"),
            }
        }
    }
}

fn wake_mutation_waiters_if_clear(state: &mut ElicitationBarrierState) {
    if !has_active_or_pending_hold(state) {
        for tx in state.mutation_waiters.drain(..) {
            let _ = tx.send(());
        }
    }
}

/// RAII pre-prompt reservation for unconditionally interactive tools.
pub(crate) struct PendingElicitationReservation {
    barrier: ElicitationHoldBarrier,
    consumed: bool,
}

impl Drop for PendingElicitationReservation {
    fn drop(&mut self) {
        if !self.consumed {
            let mut state = self.barrier.lock();
            state.pending_reservations = state.pending_reservations.saturating_sub(1);
            wake_mutation_waiters_if_clear(&mut state);
        }
    }
}

/// Ticket returned synchronously by [`ElicitationHoldBarrier::enqueue_prompt`].
pub(crate) enum ElicitationTicket {
    Active(ElicitationHoldGuard),
    Queued(QueuedElicitationTicket),
}

impl ElicitationTicket {
    pub(crate) async fn wait_until_active(self) -> ElicitationHoldGuard {
        match self {
            Self::Active(guard) => guard,
            Self::Queued(mut queued) => {
                let _ = (&mut queued.wake_rx).await;
                queued.promoted = true;
                ElicitationHoldGuard {
                    barrier: queued.barrier.clone(),
                    id: queued.id,
                }
            }
        }
    }
}

pub(crate) struct QueuedElicitationTicket {
    barrier: ElicitationHoldBarrier,
    id: u64,
    wake_rx: oneshot::Receiver<()>,
    promoted: bool,
}

impl Drop for QueuedElicitationTicket {
    fn drop(&mut self) {
        if !self.promoted {
            self.barrier.release_prompt_id(self.id);
        }
    }
}

/// Active RAII hold guard. Dropping on resolution, channel close, panic unwind,
/// or GPUI `Task` cancellation releases the prompt slot and promotes the next
/// FIFO waiter (or wakes waiting mutating siblings once idle).
pub(crate) struct ElicitationHoldGuard {
    barrier: ElicitationHoldBarrier,
    id: u64,
}

impl Drop for ElicitationHoldGuard {
    fn drop(&mut self) {
        self.barrier.release_prompt_id(self.id);
    }
}

struct YieldOnce {
    yielded: bool,
}

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// Yield once to the GPUI foreground executor without simulated-clock timers so
/// sibling eliciting tools in the same completion batch enqueue their prompt
/// holds before mutating siblings begin writing.
pub(crate) fn yield_once() -> impl Future<Output = ()> {
    YieldOnce { yielded: false }
}

/// Built-in tools whose execution always elicits human judgment regardless of
/// `always_allow` permission settings.
pub(crate) fn is_unconditionally_interactive_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "kanban_goal_close" | "kanban_goal_correct_outcome" | "ask_user" | "lean_check"
    )
}

/// Tools that perform workspace, process, subagent, or archive mutations and
/// must pause while an unresolved human elicitation/authorization hold is live
/// on the same thread. Read-only and pure-computation tools return `false` so
/// parallel non-eliciting batches execute at full concurrency parity.
pub(crate) fn is_mutating_tool(tool_name: &str, kind: acp_v2::ToolKind) -> bool {
    matches!(
        kind,
        acp_v2::ToolKind::Edit
            | acp_v2::ToolKind::Delete
            | acp_v2::ToolKind::Move
            | acp_v2::ToolKind::Execute
    ) || matches!(
        tool_name,
        "apply_code_action"
            | "rename_symbol"
            | "curator_directive"
            | "record_skill_feedback"
            | "spawn_agent"
            | "create_thread"
    )
}

// ── Native-first rate sensor (spec D) ─────────────────────────────────────

/// The host default tools that count as kask instruments for the native-first
/// classification. Context-server (MCP) tools are classified against the
/// live registry by the caller; these built-ins are name-matched.
pub(crate) const KASK_HOST_INSTRUMENT_TOOLS: &[&str] =
    &["lisp_eval", "render_template", "skill", "unfold_plan"];

/// A turn's user prompt under this many words is not substantive work —
/// acknowledgments and mode toggles must not count as native-first drift.
pub(crate) const SUBSTANTIVE_MIN_WORDS: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TurnInstrumentSummary {
    pub(crate) substantive: bool,
    pub(crate) native_first: bool,
}

/// Classify a completed turn's instrument usage for the native-first rate
/// sensor. Pure over the turn's messages plus the caller-provided set of
/// context-server tool names (the live registry read happens in
/// `Thread::run_turn`, which owns the entity context).
///
/// A turn is native-first when it contains no instrument tool use: no
/// context-server (MCP) tool call and none of the host default instruments
/// (`lisp_eval`, `render_template`, `skill`). Editor-native tools (terminal,
/// file tools) are not kask instruments — a turn that only ran a terminal
/// command is still native-first for this sensor.
pub(crate) fn turn_instrument_summary(
    turn_messages: &[std::sync::Arc<crate::thread::Message>],
    context_server_tool_names: &std::collections::HashSet<String>,
) -> TurnInstrumentSummary {
    let substantive = turn_messages
        .iter()
        .find_map(|msg| {
            if let crate::thread::Message::User(user_msg) = &**msg {
                Some(user_msg.to_markdown().split_whitespace().count())
            } else {
                None
            }
        })
        .is_some_and(|words| words >= SUBSTANTIVE_MIN_WORDS);
    let native_first = !turn_messages.iter().any(|msg| {
        if let crate::thread::Message::Agent(agent_msg) = &**msg {
            agent_msg.content.iter().any(|content| match content {
                crate::thread::AgentMessageContent::ToolUse(tool_use) => {
                    let name: &str = tool_use.name.as_ref();
                    context_server_tool_names.contains(name)
                        || KASK_HOST_INSTRUMENT_TOOLS.contains(&name)
                }
                _ => false,
            })
        } else {
            false
        }
    });
    TurnInstrumentSummary {
        substantive,
        native_first,
    }
}

// ── Claim gate v1 (D94 — spec B's observing gate) ────────────────────────

/// A deterministic claim-gate finding on a completed substantive
/// native-first turn: the delivered assistant text carries an
/// instrument-shaped claim with no instrument behind it. v1 observes and
/// records (target `reg.gate`); the corrective re-prompt half (v2) is armed
/// once these readings calibrate the checker's precision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimGateFinding {
    /// A percentage/computed figure was delivered without `lisp_eval`
    /// verifying it anywhere in the turn.
    PercentWithoutDeterministicVerification { excerpt: String },
    /// A web citation was delivered without the research ledger's
    /// evidence path in the turn.
    CitationWithoutResearchLedger { excerpt: String },
}

impl ClaimGateFinding {
    /// Stable label for the record surface.
    pub fn label(&self) -> &'static str {
        match self {
            Self::PercentWithoutDeterministicVerification { .. } => {
                "percent-without-deterministic-verification"
            }
            Self::CitationWithoutResearchLedger { .. } => "citation-without-research-ledger",
        }
    }

    /// The bounded excerpt around the matched claim, for the record.
    pub fn excerpt(&self) -> &str {
        match self {
            Self::PercentWithoutDeterministicVerification { excerpt }
            | Self::CitationWithoutResearchLedger { excerpt } => excerpt,
        }
    }
}

/// Deterministic claim-class detection over a completed turn's messages
/// (D94 — spec B's observing gate, v1). Call only for substantive
/// native-first turns — the drift population the native-first sensor
/// counts — where an instrument-shaped claim is exactly the mismatch the
/// gate exists to observe. Scans only the FINAL delivered assistant text
/// (Thinking segments are scratch, not delivered claims); at most one
/// finding per class (bounded record), each with a bounded excerpt. No
/// model cooperation — pure string classification.
pub(crate) fn claim_gate_findings(
    turn_messages: &[std::sync::Arc<crate::thread::Message>],
) -> Vec<ClaimGateFinding> {
    let final_text = turn_messages
        .iter()
        .rev()
        .find_map(|msg| match &**msg {
            crate::thread::Message::Agent(agent_msg) => Some(
                agent_msg
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        crate::thread::AgentMessageContent::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let mut findings = Vec::new();
    if let Some(excerpt) = first_percent_excerpt(&final_text) {
        findings.push(ClaimGateFinding::PercentWithoutDeterministicVerification { excerpt });
    }
    if let Some(excerpt) = first_url_excerpt(&final_text) {
        findings.push(ClaimGateFinding::CitationWithoutResearchLedger { excerpt });
    }
    findings
}

/// The first percentage figure: a digit run (with optional decimal parts)
/// immediately before an optional space and '%'. Returns a bounded excerpt
/// around the match.
fn first_percent_excerpt(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for (index, _) in text.match_indices('%') {
        let mut end = index;
        if end > 0 && bytes[end - 1] == b' ' {
            end -= 1;
        }
        let mut start = end;
        let mut saw_digit = false;
        while start > 0 {
            let byte = bytes[start - 1];
            if byte.is_ascii_digit() {
                saw_digit = true;
                start -= 1;
            } else if byte == b'.' && saw_digit {
                start -= 1;
            } else {
                break;
            }
        }
        if saw_digit {
            return Some(bounded_excerpt(text, start));
        }
    }
    None
}

/// The first http(s) URL in the text, as a bounded excerpt.
fn first_url_excerpt(text: &str) -> Option<String> {
    let index = text.find("http://").or_else(|| text.find("https://"))?;
    Some(bounded_excerpt(text, index))
}

/// A bounded ~80-char window around `index`, snapped to char boundaries —
/// `.get` returns None on a non-boundary, and the loops guarantee one.
fn bounded_excerpt(text: &str, index: usize) -> String {
    let mut start = index.saturating_sub(40);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (index + 40).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(start..end).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::KaskThreadState;
    use anyhow::anyhow;
    use http_client::StatusCode;
    use language_model::ProviderErrorCategory;
    use language_model::{LanguageModelCompletionError, LanguageModelProviderName};

    use super::{KASK_HOST_INSTRUMENT_TOOLS, turn_instrument_summary};
    use crate::thread::{
        AgentMessage, AgentMessageContent, Message, UserMessage, UserMessageContent,
    };
    use acp_thread::ClientUserMessageId;
    use language_model::LanguageModelToolUseId;

    fn agent_message_with_text(text: &str) -> std::sync::Arc<Message> {
        let mut message = AgentMessage::default();
        message
            .content
            .push(AgentMessageContent::Text(text.to_string()));
        std::sync::Arc::new(Message::Agent(message))
    }

    fn agent_message_with_thinking_and_text(thinking: &str, text: &str) -> std::sync::Arc<Message> {
        let mut message = AgentMessage::default();
        message.content.push(AgentMessageContent::Thinking {
            text: thinking.to_string(),
            signature: None,
        });
        message
            .content
            .push(AgentMessageContent::Text(text.to_string()));
        std::sync::Arc::new(Message::Agent(message))
    }

    /// D94 — the delivered text carries a percentage with no `lisp_eval`
    /// behind it: one finding, with the figure in the excerpt.
    #[test]
    fn percent_in_delivered_text_fires_with_excerpt() {
        let findings = super::claim_gate_findings(&[agent_message_with_text(
            "the native-first rate reads 62% of 26 substantive turns over the window",
        )]);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].label(),
            "percent-without-deterministic-verification"
        );
        assert!(
            findings[0].excerpt().contains("62%"),
            "got: {}",
            findings[0].excerpt()
        );
    }

    /// D94 — a delivered http citation with no research-ledger tools behind
    /// it: one finding, with the URL in the excerpt.
    #[test]
    fn citation_in_delivered_text_fires_with_excerpt() {
        let findings = super::claim_gate_findings(&[agent_message_with_text(
            "per the upstream discussion at https://example.com/thread the fix landed",
        )]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].label(), "citation-without-research-ledger");
        assert!(
            findings[0].excerpt().contains("https://example.com"),
            "got: {}",
            findings[0].excerpt()
        );
    }

    /// D94 — prose with no instrument-shaped claims fires nothing.
    #[test]
    fn clean_delivered_text_fires_nothing() {
        let findings = super::claim_gate_findings(&[agent_message_with_text(
            "the fix is in; the tests pass and the tree is clean as of this turn",
        )]);
        assert!(findings.is_empty(), "got: {findings:?}");
    }

    /// D94 — Thinking segments are scratch, never delivered claims: a
    /// percentage that exists only in thinking must not fire.
    #[test]
    fn thinking_scratch_is_not_gated() {
        let findings = super::claim_gate_findings(&[agent_message_with_thinking_and_text(
            "maybe the rate is 62% — verify with lisp_eval before sending",
            "the change is complete and the gates are green.",
        )]);
        assert!(findings.is_empty(), "got: {findings:?}");
    }

    /// D94 — only the FINAL delivered agent message is gated: an earlier
    /// message's claims are superseded, not the turn's deliverable.
    #[test]
    fn only_the_final_agent_message_is_gated() {
        let findings = super::claim_gate_findings(&[
            agent_message_with_text("the earlier reading was 50% before the recheck"),
            agent_message_with_text("the recheck is done and everything is consistent"),
        ]);
        assert!(findings.is_empty(), "got: {findings:?}");
        let findings = super::claim_gate_findings(&[
            agent_message_with_text("the earlier reading was fine"),
            agent_message_with_text("the final reading is 50% of the window"),
        ]);
        assert_eq!(findings.len(), 1);
    }

    /// D94 — both classes fire together on one turn, bounded at one finding
    /// per class.
    #[test]
    fn both_classes_fire_together() {
        let findings = super::claim_gate_findings(&[agent_message_with_text(
            "coverage improved to 80% and the source is https://example.com/spec",
        )]);
        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .any(|f| f.label() == "percent-without-deterministic-verification")
        );
        assert!(
            findings
                .iter()
                .any(|f| f.label() == "citation-without-research-ledger")
        );
    }

    fn substantive_prompt() -> String {
        // 24 words — above SUBSTANTIVE_MIN_WORDS (20).
        "please analyze the quarterly revenue trajectory and identify the three largest \
         cohort deltas with their contribution margins across every region we operate in today"
            .to_string()
    }

    fn user_text_message(text: &str) -> std::sync::Arc<Message> {
        std::sync::Arc::new(Message::User(UserMessage {
            id: ClientUserMessageId::new(),
            content: vec![UserMessageContent::Text(text.to_string())].into(),
        }))
    }

    fn agent_message_with_tool(name: &str) -> std::sync::Arc<Message> {
        let mut message = AgentMessage::default();
        message.content.push(AgentMessageContent::ToolUse(
            language_model::LanguageModelToolUse {
                id: LanguageModelToolUseId::from(name),
                name: std::sync::Arc::from(name),
                raw_input: "{}".to_string(),
                input: language_model::LanguageModelToolUseInput::Json(serde_json::json!({})),
                is_input_complete: true,
                thought_signature: None,
            },
        ));
        std::sync::Arc::new(Message::Agent(message))
    }

    #[test]
    fn substantive_turn_without_instruments_is_native_first() {
        let summary = turn_instrument_summary(
            &[user_text_message(&substantive_prompt())],
            &Default::default(),
        );
        assert_eq!(summary.substantive, true);
        assert_eq!(summary.native_first, true);
    }

    #[test]
    fn short_prompt_is_not_substantive() {
        // Acknowledgments and mode toggles must not count as drift: the
        // ledger skips non-substantive turns entirely.
        let summary = turn_instrument_summary(&[user_text_message("thanks")], &Default::default());
        assert_eq!(summary.substantive, false);
        assert!(
            summary.native_first,
            "the classification is still computed; the ledger skips the record"
        );
    }

    #[test]
    fn context_server_tool_use_clears_native_first() {
        let mut names = std::collections::HashSet::new();
        names.insert("corpus_query".to_string());
        let summary = turn_instrument_summary(
            &[
                user_text_message(&substantive_prompt()),
                agent_message_with_tool("corpus_query"),
            ],
            &names,
        );
        assert_eq!(summary.substantive, true);
        assert_eq!(summary.native_first, false);
    }

    #[test]
    fn host_instrument_tool_use_clears_native_first() {
        for name in KASK_HOST_INSTRUMENT_TOOLS {
            let summary = turn_instrument_summary(
                &[
                    user_text_message(&substantive_prompt()),
                    agent_message_with_tool(name),
                ],
                &Default::default(),
            );
            assert_eq!(summary.native_first, false, "{name} is a kask instrument");
        }
    }

    #[test]
    fn editor_native_tool_use_is_still_native_first() {
        // Terminal and file tools are zed-native, not kask instruments: a
        // turn that only ran a terminal command did real work but used zero
        // kask instruments — still drift for this sensor.
        let summary = turn_instrument_summary(
            &[
                user_text_message(&substantive_prompt()),
                agent_message_with_tool("terminal"),
            ],
            &Default::default(),
        );
        assert_eq!(summary.native_first, true);
    }

    #[test]
    fn max_tokens_warning_names_the_stop_reason_and_content_state() {
        // D43: the warn must carry the stop reason, the model, and the
        // zero-content distinction — the fields that made the 2026-08-30
        // silent-turn-stop incident diagnosable only by inference.
        let zero_content = KaskThreadState::max_tokens_turn_end_warning("GLM 5.3", false);
        assert!(zero_content.contains("StopReason::MaxTokens"));
        assert!(zero_content.contains("GLM 5.3"));
        assert!(zero_content.contains("NO"));
        assert!(zero_content.contains("silent-stop"));

        let partial = KaskThreadState::max_tokens_turn_end_warning("GLM 5.3", true);
        assert!(partial.contains("StopReason::MaxTokens"));
        assert!(partial.contains("partial content"));
        assert!(!partial.contains("silent-stop"));
    }

    #[test]
    fn provider_rejection_detail_names_wire_fields_and_transient_verdict() {
        // D43: the 2026-09-10 "Provider returned error" incident — five
        // turn failures whose status/code/category reached no log. The
        // detail must carry every preserved wire field plus the retry
        // classification, so a 502 (retried to exhaustion) is
        // distinguishable from a 402 (permanent, credits) by the log line
        // alone.
        let rejection = LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: Some(StatusCode::BAD_GATEWAY),
            code: Some("502".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::InternalServer,
        };
        let detail = KaskThreadState::provider_rejection_turn_end_warning(&rejection)
            .expect("a ProviderRejection must produce a detail line");
        assert!(detail.contains("OpenRouter"));
        assert!(detail.contains("502"));
        assert!(detail.contains("InternalServer"));
        assert!(detail.contains("transient"));
        assert!(detail.contains("retried"));
        assert!(detail.contains("Provider returned error"));
    }

    #[test]
    fn provider_rejection_detail_marks_statusless_permanent_rejections() {
        // Mid-stream rejections carry no HTTP status; a code mapping to no
        // known category is classified permanent and never retried — the
        // sub-second failure signature from the 2026-09-10 incident log.
        let rejection = LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: None,
            code: Some("499".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::Other,
        };
        let detail = KaskThreadState::provider_rejection_turn_end_warning(&rejection)
            .expect("a ProviderRejection must produce a detail line");
        assert!(detail.contains("none (mid-stream rejection)"));
        assert!(detail.contains("499"));
        assert!(detail.contains("Other"));
        assert!(detail.contains("permanent"));
        assert!(detail.contains("not retried"));
    }

    #[test]
    fn provider_rejection_detail_is_none_for_non_rejections() {
        let transport = LanguageModelCompletionError::Other(anyhow!("transport error"));
        assert!(KaskThreadState::provider_rejection_turn_end_warning(&transport).is_none());
    }

    #[test]
    fn is_tool_call_truncation_classifies_only_the_typed_variant() {
        // zed-kask: D36 — only the typed truncation classification sets
        // the D25 flag; a malformed-transport `Other` must not (it
        // retries), and neither must an unexpected stream end.
        let truncated = LanguageModelCompletionError::ToolCallTruncated {
            tool_name: "reg_query".to_string(),
            json_parse_error: "EOF while parsing a value at line 1 column 14".to_string(),
            raw_input_chars: 47,
        };
        assert!(KaskThreadState::is_tool_call_truncation(&truncated));
        assert!(!KaskThreadState::is_tool_call_truncation(
            &LanguageModelCompletionError::Other(anyhow!("stream ended unexpectedly"))
        ));
        assert!(!KaskThreadState::is_tool_call_truncation(
            &LanguageModelCompletionError::StreamEndedUnexpectedly {
                provider: LanguageModelProviderName::new("OpenRouter"),
            }
        ));
    }

    /// zed-kask: D100 (Seam A) — the privacy mark: threads start unmarked
    /// (the honest pre-D100 history), and subagents inherit the mark —
    /// a private thread's subagent content is private-thread content, so
    /// the child must sit inside the parent's routing boundary.
    #[test]
    fn inherit_from_carries_the_privacy_mark() {
        let mut parent = KaskThreadState::new();
        assert!(!parent.is_private_inference(), "threads start unmarked");
        parent.set_private_inference(true);
        let child = KaskThreadState::inherit_from(&parent);
        assert!(
            child.is_private_inference(),
            "a private thread's subagent inherits the mark"
        );

        let unmarked_child = KaskThreadState::inherit_from(&KaskThreadState::new());
        assert!(
            !unmarked_child.is_private_inference(),
            "a public thread's subagent starts unmarked"
        );
    }
}
