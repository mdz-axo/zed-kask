# Thread Divergence Refactor Plan — 2026-08-26

> **Status:** ✅ IMPLEMENTED (2026-08-26). All phases complete — 9 fields moved
> to `KaskThreadState`, 5 pinning tests added, 15 `// zed-kask:` markers,
> DIVERGENCE.md updated (D2/D6/D25), code review passed (6 should-fix + 4
> nits identified, all should-fix applied). Follows the `upstream-rebase`
> skill's mapped-re-application process (Steps 1–7).
>
> **Essentialist verdict:** the grouped-struct + method-extraction approach
> (§3.2) is the recommended end state; the hook trait (§3.1) is prototyped
> for one field to validate the approach but is not recommended for full
> adoption (§4.3 explains why). The grouped-struct was adopted.

## 1. Functional inventory (Step 1 — code-graph extraction)

`crates/agent/src/thread.rs` has 9 kask-specific struct fields and 11
scattered turn-loop branches. Only 3 `// zed-kask:` markers exist in the
file (D6/D34 subagent inheritance, D2 tool router comment, D6/D34 test
comment) — **under-marked** per the upstream-rebase decision rule (< 50%
of kask call sites carry markers).

### 1.1 Kask-specific `Thread` struct fields

| # | Field | D-seam | Type | Per-thread state? |
|---|-------|--------|------|-------------------|
| F1 | `agent_static_context` | D2 | `Option<SharedString>` | yes |
| F2 | `agent_id` | D6 | `Option<AgentId>` | yes |
| F3 | `mcp_server_scope` | D2 | `Option<SharedString>` | yes |
| F4 | `deferred_tool_results` | — | `Vec<DeferredToolResult>` | yes |
| F5 | `last_completion_truncated` | D25 | `bool` | yes |
| F6 | `tool_retry_tracker` | .rules | `Rc<RefCell<ToolRetryTracker>>` | yes |
| F7 | `cached_system_prompt` | — | `Option<CachedSystemPrompt>` | yes |
| F8 | `cached_filtered_context` | — | `Option<CachedFilteredContext>` | yes |
| F9 | `system_prompt_override` | — | `Option<SharedString>` | yes |

All 9 are per-thread state. None are process-global (the process-global
hooks — `memory_port()`, `context_injector()`, `tool_router()`,
`thread_condenser()` — already live in `agent.rs` as `static Mutex`
globals and are not part of this refactoring).

### 1.2 Kask-specific turn-loop branches

| # | Behavior | D-seam | Call site (method, line) | Thread state accessed | Extension state accessed |
|---|----------|--------|--------------------------|----------------------|--------------------------|
| B1 | Memory ingestion on turn completion | D6 | `run_turn` L3014–3075 | `id`, `messages`, `model`, `title` | `agent_id` |
| B2 | Context injection (curator/user recall) | D6/D11 | `run_turn_internal` L3217–3255 | `id` | `agent_id` |
| B3 | Static context rendering | D2 | `render_system_prompt` L5249 | — | `agent_static_context` |
| B4 | System prompt override | D2 | `render_system_prompt` L5234 | — | `system_prompt_override` |
| B5 | MCP server scope filtering | D2 | `enabled_tools` L4989 | — | `mcp_server_scope` |
| B6 | Curator memory-edit tool gating | D6 | `enabled_tools` L5000–5010 | — | `agent_id` |
| B7 | Deferred result drain + inject | — | `run_turn_internal` L3187–3199 | `messages` (inject) | `deferred_tool_results` |
| B8 | Tool retry cap check | .rules | `handle_tool_use_event` L4178–4225 | — | `tool_retry_tracker` |
| B9 | Truncation flag set | D25 | `handle_completion_event` L4007–4008 | — | `last_completion_truncated` |
| B10 | Truncation flag read | D25 | `flush_pending_message` L4792 | — | `last_completion_truncated` |
| B11 | Truncation flag reset | D25 | `run_turn_internal` L3213 | — | `last_completion_truncated` |
| B12 | Subagent `agent_id` inheritance | D6/D34 | `new_subagent` L1544 | parent's `agent_id` | child's `agent_id` |
| B13 | Deferred result cancel clear | — | `cancel` L2574–2575 | — | `deferred_tool_results` |
| B14 | Deferred result enqueue | — | `enqueue_deferred_tool_result` L3697 | — | `deferred_tool_results` |
| B15 | System prompt cache (digest + reuse) | — | `render_system_prompt` L5304–5343 | — | `cached_system_prompt` |
| B16 | Filtered context cache (digest + reuse) | — | `render_system_prompt` L5265–5288 | — | `cached_filtered_context` |

**Key observation:** most branches (B3, B4, B5, B6, B8, B9, B10, B11,
B13, B14, B15, B16) access **only** the extension's own state — no
`Thread` fields needed. Only B1, B2, B7, B12 need both `Thread` state
and extension state.

## 2. Constraint-force classification (Step 2)

| Unit | Force | Enforcement point | Pinning test |
|------|-------|-------------------|-------------|
| F1/B3 `agent_static_context` | Prohibition | `render_system_prompt` L5249 | `test_system_prompt_renders_session_context_without_rules_or_agents_md` (templates.rs) |
| F2/B1 `agent_id` memory ingestion | Prohibition | `run_turn` L3025–3067 | `test_curator_sessions_carry_per_tab_scope_and_prompt` (agent.rs) |
| F2/B2 `agent_id` context injection | Prohibition | `run_turn_internal` L3229 | (indirect — via `test_curator_sessions_carry_per_tab_scope_and_prompt`) |
| F2/B6 `agent_id` curator gating | Prohibition | `enabled_tools` L5000–5010 | (not yet pinned — gap) |
| F2/B12 `agent_id` subagent inheritance | Prohibition | `new_subagent` L1544 | `test_subagent_inherits_parent_agent_id` |
| F3/B5 `mcp_server_scope` | Prohibition | `enabled_tools` L4989 | `mcp_server_scope_filters_to_named_server` |
| F4/B7/B13/B14 `deferred_tool_results` | Prohibition | `run_turn_internal` L3192, `cancel` L2575, `enqueue_deferred_tool_result` L3697 | `test_deferred_tool_result_appears_in_next_request`, `test_deferred_tool_result_pending_stays_in_queue`, `test_deferred_tool_result_cancel_clears_queue` |
| F5/B9/B10/B11 `last_completion_truncated` | Prohibition | `handle_completion_event` L4008, `flush_pending_message` L4792, `run_turn_internal` L3213 | (not yet pinned — gap) |
| F6/B8 `tool_retry_tracker` | Prohibition | `handle_tool_use_event` L4184–4225 | (not yet pinned in thread.rs — pinned in `tool_retry_tracker.rs` unit tests) |
| F7/B15 `cached_system_prompt` | Guardrail | `render_system_prompt` L5304 | `test_system_prompt_digest_stability` |
| F8/B16 `cached_filtered_context` | Guardrail | `render_system_prompt` L5265 | (not yet pinned — gap) |
| F9/B4 `system_prompt_override` | Prohibition | `render_system_prompt` L5234 | (not yet pinned — gap) |

**Gaps:** 5 behaviors lack pinning tests in `thread.rs`. The refactoring
must add these before moving the code (per the upstream-rebase skill
Step 6: "every `// zed-kask:` marker must have a corresponding test").

## 3. The hook surface (Step 4 — insertion points)

### 3.1 Hook trait approach (prototyped, not recommended)

Define a `ThreadExtension` trait with default no-op methods. `Thread`
has a single `extension: Option<Rc<RefCell<dyn ThreadExtension>>>`
field. Kask provides `KaskThreadExtension` implementing the trait.

```rust
// crates/agent/src/thread_extension.rs (new file, kask-owned)

/// Extension points for kask-specific per-thread behavior.
/// Upstream Zed uses the no-op default impl; kask provides a concrete impl.
/// The extension holds per-thread state that upstream Thread doesn't need.
pub trait ThreadExtension: std::any::Any {
    // ── Identity ──────────────────────────────────────────────────────

    /// The agent ID that owns this thread (D6 routing key).
    fn agent_id(&self) -> Option<&AgentId> { None }

    /// Set the agent ID (called by NativeAgent::new_session for Curator).
    fn set_agent_id(&mut self, _agent_id: AgentId) {}

    // ── System prompt ─────────────────────────────────────────────────

    /// Static context rendered in the system prompt's Session Context section (D2).
    fn static_context(&self) -> Option<&SharedString> { None }

    /// Set the static context (Curator overlay, Steer panel overlay).
    fn set_static_context(&mut self, _context: SharedString) {}

    /// System prompt override — when set, returned directly (D2 Curator persona).
    fn system_prompt_override(&self) -> Option<&SharedString> { None }

    /// Set the system prompt override.
    fn set_system_prompt_override(&mut self, _prompt: SharedString) {}

    /// Bust the system prompt cache (called when static_context or override changes).
    fn bust_system_prompt_cache(&mut self) {}

    // ── MCP server scoping ────────────────────────────────────────────

    /// When set, enabled_tools filters MCP tools to only this server (D2).
    fn mcp_server_scope(&self) -> Option<&SharedString> { None }

    /// Set the MCP server scope.
    fn set_mcp_server_scope(&mut self, _scope: Option<SharedString>) {}

    /// Whether a context-server id passes the per-tab MCP scope.
    fn mcp_server_in_scope(&self, server_id: &str) -> bool { true }

    /// Whether this thread is a curator thread (for memory-edit tool gating).
    fn is_curator_thread(&self) -> bool { false }

    // ── Tool retry cap ────────────────────────────────────────────────

    /// Check the tool retry cap before running a tool. Returns Allow,
    /// AllowWithWarning, or Refuse.
    fn check_tool_retry(
        &self,
        _tool_name: &str,
        _input: &serde_json::Value,
    ) -> crate::tool_retry_tracker::RetryVerdict {
        crate::tool_retry_tracker::RetryVerdict::Allow
    }

    /// Record a tool failure (called after a tool returns an error).
    fn record_tool_failure(&self, _tool_name: &str, _input: &serde_json::Value) {}

    /// Record a tool success (called after a tool returns success).
    fn record_tool_success(&self, _tool_name: &str) {}

    // ── Truncation detection (D25) ────────────────────────────────────

    /// Called when a completion stops with MaxTokens.
    fn on_max_tokens(&mut self) {}

    /// Whether the last completion was truncated.
    fn last_completion_truncated(&self) -> bool { false }

    /// Reset the truncation flag (called at the start of each completion request).
    fn reset_truncation_flag(&mut self) {}

    // ── Deferred tool results ────────────────────────────────────────

    /// Enqueue a deferred tool result.
    fn enqueue_deferred_result(&mut self, _result: crate::thread::DeferredToolResult) {}

    /// Drain completed deferred results. Returns the results to inject.
    fn drain_completed_deferred_results(&mut self) -> Vec<crate::thread::CompletedDeferredResult> { Vec::new() }

    /// Clear all deferred results (on cancel).
    fn clear_deferred_results(&mut self) {}

    // ── Caching ───────────────────────────────────────────────────────

    /// Get the cached system prompt if the digest matches.
    fn cached_system_prompt(&self, _digest: &[u8; 32]) -> Option<SharedString> { None }

    /// Store a rendered system prompt with its digest.
    fn cache_system_prompt(&mut self, _digest: [u8; 32], _prompt: SharedString) {}

    /// Get the cached filtered context if the digest matches.
    fn cached_filtered_context(&self, _digest: &[u8; 32]) -> Option<project_context::ProjectContext> { None }

    /// Store a filtered context with its digest.
    fn cache_filtered_context(&mut self, _digest: [u8; 32], _context: project_context::ProjectContext) {}

    // ── Turn lifecycle ────────────────────────────────────────────────

    /// Called after a turn completes successfully. Returns an optional
    /// background task (e.g., memory ingestion). The caller detaches it.
    fn on_turn_complete(
        &self,
        _record: &crate::ThreadTurnRecord,
    ) -> Option<gpui::Task<()>> { None }

    /// Called per-turn to inject context messages after the system prompt.
    /// Returns messages to splice into the request.
    async fn on_context_injection(
        &self,
        _thread_id: &str,
        _user_prompt: &str,
    ) -> Vec<language_model::LanguageModelRequestMessage> {
        Vec::new()
    }

    // ── Subagent inheritance ──────────────────────────────────────────

    /// Called when creating a subagent, to inherit per-thread extension state.
    fn inherit_to_subagent(&self) -> Box<dyn ThreadExtension>;
}
```

**Why not recommended (§4.3):**
1. **Single implementation** — a trait with one impl is speculative
   generality (the `.rules` trap). The trait adds indirection without
   enabling polymorphism.
2. **Borrow checker friction** — hooks called from `&mut self` methods
   need split borrows (`let ext = self.extension.as_mut(); let thread = &*self;`),
   which are fragile and easy to break.
3. **Async trait methods** — `on_context_injection` is async, requiring
   either `async-trait` (allocation per call) or a `Pin<Box<dyn Future>>`
   return type (verbose). Neither is idiomatic for this codebase.
4. **`Rc<RefCell<>>` overhead** — every hook call goes through
   `self.extension.as_ref().map(|e| e.borrow().on_xxx())`, which is
   noisier than `self.kask.on_xxx()`.
5. **Larger divergence surface** — the trait definition, the field,
   and all call sites are divergence. The grouped-struct approach has
   the same call-site count but a smaller definition surface.

### 3.2 Grouped-struct approach (recommended)

Group all 9 fields into a single `KaskThreadState` struct. Extract
turn-loop behaviors into methods on `KaskThreadState`. `Thread` has
one field: `kask: KaskThreadState`.

```rust
// crates/agent/src/kask_thread_state.rs (new file, kask-owned)

/// All kask-specific per-thread state, grouped so upstream Thread's
/// struct definition stays clean and upstream rebases touch one field
/// instead of nine. Methods on this struct encapsulate the kask-specific
/// turn-loop behaviors that were previously inline in Thread's methods.
pub(crate) struct KaskThreadState {
    // Identity (D6)
    agent_id: Option<AgentId>,

    // System prompt overlays (D2)
    agent_static_context: Option<SharedString>,
    system_prompt_override: Option<SharedString>,
    mcp_server_scope: Option<SharedString>,

    // Tool retry cap (.rules)
    tool_retry_tracker: Rc<RefCell<crate::tool_retry_tracker::ToolRetryTracker>>,

    // Deferred tool results
    deferred_tool_results: Vec<crate::thread::DeferredToolResult>,

    // Truncation detection (D25)
    last_completion_truncated: bool,

    // Caching
    cached_system_prompt: Option<crate::thread::CachedSystemPrompt>,
    cached_filtered_context: Option<crate::thread::CachedFilteredContext>,
}

impl KaskThreadState {
    pub fn new() -> Self { /* defaults */ }

    // ── Identity ──────────────────────────────────────────────────────

    pub fn agent_id(&self) -> Option<&AgentId> { self.agent_id.as_ref() }
    pub fn set_agent_id(&mut self, agent_id: AgentId) { self.agent_id = Some(agent_id); }

    // ── System prompt ─────────────────────────────────────────────────

    pub fn static_context(&self) -> Option<&SharedString> { self.agent_static_context.as_ref() }
    pub fn set_static_context(&mut self, context: SharedString) {
        self.agent_static_context = Some(context);
        self.cached_system_prompt = None; // bust cache
    }
    pub fn system_prompt_override(&self) -> Option<&SharedString> { self.system_prompt_override.as_ref() }
    pub fn set_system_prompt_override(&mut self, prompt: SharedString) {
        self.system_prompt_override = Some(prompt);
        self.cached_system_prompt = None; // bust cache
    }

    // ── MCP server scoping ────────────────────────────────────────────

    pub fn mcp_server_scope(&self) -> Option<&SharedString> { self.mcp_server_scope.as_ref() }
    pub fn set_mcp_server_scope(&mut self, scope: Option<SharedString>) { self.mcp_server_scope = scope; }
    pub fn mcp_server_in_scope(&self, server_id: &str) -> bool {
        self.mcp_server_scope.as_ref().is_none_or(|s| s.as_ref() == server_id)
    }
    pub fn is_curator_thread(&self) -> bool {
        self.agent_id.as_ref().is_some_and(|id| id.as_ref() == crate::CURATOR_AGENT_ID.as_ref())
    }

    // ── Tool retry cap ────────────────────────────────────────────────

    pub fn check_tool_retry(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> crate::tool_retry_tracker::RetryVerdict {
        self.tool_retry_tracker.borrow().check(tool_name, input)
    }
    pub fn record_tool_failure(&self, tool_name: &str, input: &serde_json::Value) {
        self.tool_retry_tracker.borrow_mut().record_failure(tool_name, input);
    }
    pub fn record_tool_success(&self, tool_name: &str) {
        self.tool_retry_tracker.borrow_mut().record_success(tool_name);
    }

    // ── Truncation detection (D25) ────────────────────────────────────

    pub fn on_max_tokens(&mut self) { self.last_completion_truncated = true; }
    pub fn last_completion_truncated(&self) -> bool { self.last_completion_truncated }
    pub fn reset_truncation_flag(&mut self) { self.last_completion_truncated = false; }

    // ── Deferred tool results ────────────────────────────────────────

    pub fn enqueue_deferred_result(&mut self, result: crate::thread::DeferredToolResult) {
        self.deferred_tool_results.push(result);
    }
    pub fn drain_completed_deferred_results(&mut self) -> Vec<crate::thread::CompletedDeferredResult> {
        // (extracted from Thread::drain_completed_deferred_results — L3709–3746)
        if self.deferred_tool_results.is_empty() { return Vec::new(); }
        // ... polling logic ...
    }
    pub fn clear_deferred_results(&mut self) { self.deferred_tool_results.clear(); }

    // ── Caching ───────────────────────────────────────────────────────

    pub fn cached_system_prompt(&self, digest: &[u8; 32]) -> Option<SharedString> {
        self.cached_system_prompt.as_ref()
            .filter(|c| c.digest == *digest)
            .map(|c| c.prompt.clone())
    }
    pub fn cache_system_prompt(&mut self, digest: [u8; 32], prompt: SharedString) {
        self.cached_system_prompt = Some(crate::thread::CachedSystemPrompt { digest, prompt });
    }
    pub fn cached_filtered_context(&self, digest: &[u8; 32]) -> Option<&project_context::ProjectContext> {
        self.cached_filtered_context.as_ref()
            .filter(|c| c.filter_digest == *digest)
            .map(|c| &c.context)
    }
    pub fn cache_filtered_context(&mut self, digest: [u8; 32], context: project_context::ProjectContext) {
        self.cached_filtered_context = Some(crate::thread::CachedFilteredContext {
            filter_digest: digest,
            context,
        });
    }

    // ── Turn lifecycle ────────────────────────────────────────────────

    /// Build the ThreadTurnRecord for memory ingestion (D6).
    /// Reads Thread state (id, messages, model, title) — passed in to avoid borrow conflicts.
    pub fn build_turn_record(
        &self,
        thread_id: &str,
        messages: &[Arc<crate::thread::Message>],
        model: Option<&dyn language_model::LanguageModel>,
        title: Option<&SharedString>,
    ) -> crate::ThreadTurnRecord {
        crate::ThreadTurnRecord {
            thread_id: thread_id.to_string(),
            user_input: messages.iter().rev()
                .find_map(|msg| match &**msg {
                    crate::thread::Message::User(user_msg) => Some(user_msg.to_markdown()),
                    _ => None,
                }).unwrap_or_default(),
            agent_response: messages.iter().rev()
                .find_map(|msg| match &**msg {
                    crate::thread::Message::Agent(agent_msg) => Some(agent_msg.to_markdown()),
                    _ => None,
                }).unwrap_or_default(),
            model: model.map(|m| m.name().0.to_string()).unwrap_or_default(),
            thread_title: title.map(|t| t.to_string()),
            agent_id: self.agent_id.clone(),
        }
    }

    /// Inherit state from a parent thread's KaskThreadState (for subagents, D6/D34).
    pub fn inherit_from(parent: &KaskThreadState) -> Self {
        Self {
            agent_id: parent.agent_id.clone(),
            ..Self::new()
        }
    }
}

impl Default for KaskThreadState {
    fn default() -> Self { Self::new() }
}
```

**Thread struct change:**

```rust
pub struct Thread {
    // ... 33 upstream fields unchanged ...

    // zed-kask: D2/D6/D25 — all kask-specific per-thread state grouped
    // into one field so upstream rebases touch one struct init instead
    // of nine field inits. See kask_thread_state.rs for the methods that
    // encapsulate the kask-specific turn-loop behaviors.
    pub(crate) kask: crate::kask_thread_state::KaskThreadState,
}
```

**Turn-loop call sites become:**

```rust
// B1: Memory ingestion (run_turn, after run_turn_internal)
let record = this.update(cx, |thread, _| thread.kask.build_turn_record(
    &thread.id.to_string(),
    &thread.messages,
    thread.model().map(|m| m.as_ref()),
    thread.title(),
));
// ... rest of memory ingestion unchanged (uses process-global memory_port()) ...

// B2: Context injection (run_turn_internal)
let agent_id = this.read_with(cx, |t, _| t.kask.agent_id().cloned()).ok().unwrap_or(None);
if let Some(injector) = crate::context_injector_for(agent_id.as_ref()) { ... }

// B3: Static context (render_system_prompt)
let static_context = self.kask.static_context().cloned();

// B4: System prompt override (render_system_prompt)
if let Some(ref override_prompt) = self.kask.system_prompt_override() { return override_prompt.clone(); }

// B5+B6: MCP scope + curator gating (enabled_tools)
if !self.kask.mcp_server_in_scope(server_id.0.as_ref()) { continue; }
let is_curator_thread = self.kask.is_curator_thread();

// B7: Deferred results (run_turn_internal)
let injected = this.update(cx, |this, cx| this.kask.drain_and_inject(&mut this.messages, event_stream, cx))?;

// B8: Tool retry (handle_tool_use_event)
let retry_warning = match self.kask.check_tool_retry(tool_name_str, &input) { ... };

// B9: Truncation set (handle_completion_event)
self.kask.on_max_tokens();

// B10: Truncation read (flush_pending_message)
let cancel_message = if self.kask.last_completion_truncated() { TOOL_TRUNCATED_MESSAGE } else { TOOL_CANCELED_MESSAGE };

// B11: Truncation reset (run_turn_internal)
this.kask.reset_truncation_flag();

// B12: Subagent inheritance (new_subagent)
thread.kask = KaskThreadState::inherit_from(&parent_thread.read(cx).kask);
```

## 4. Feasibility assessment

### 4.1 What the grouped-struct approach achieves

| Goal | Achieved? | How |
|------|-----------|-----|
| Reduce 9 fields to 1 | ✅ | `kask: KaskThreadState` |
| Make kask-specific state obvious | ✅ | One field, one file |
| Extract turn-loop branches | ✅ | Methods on `KaskThreadState` |
| Upstream `Thread::new_internal` works | ✅ | `kask: KaskThreadState::new()` (one line) |
| Upstream `Thread::from_db` works | ✅ | `kask: KaskThreadState::new()` (one line) |
| Testable in isolation | ✅ | `KaskThreadState` methods are unit-testable |
| Upstream `Thread` stays "clean" | ⚠️ | Turn-loop still has `self.kask.xxx()` calls (but they're one-liners, not inline blocks) |
| No behavioral change | ✅ | Pure structural move |

### 4.2 What the hook trait approach achieves (additionally)

| Goal | Achieved? | How |
|------|-----------|-----|
| Upstream `Thread` compiles without kask code | ✅ | No-op default impl; extension is `None` for upstream |
| Polymorphic dispatch | ❌ | Only one impl exists (speculative generality) |

### 4.3 Why the hook trait is not recommended

1. **Speculative generality** (`.rules` trap): a trait with one impl is
   dead surface. The `KaskThreadExtension` struct would be the only
   `ThreadExtension` impl. The trait adds a vtable dispatch and a
   `Box<dyn>` / `Rc<RefCell<dyn>>` field for no polymorphism benefit.

2. **Borrow checker friction**: hooks called from `&mut self` methods
   need split borrows. For example, in `render_system_prompt`:
   ```rust
   // Must split: extension borrows mutably, Thread borrows immutably
   let static_context = self.extension.as_ref().and_then(|e| e.borrow().static_context().cloned());
   ```
   This is noisier than `self.kask.static_context().cloned()` and
   requires `RefCell` even when the access is immutable.

3. **Async trait methods**: `on_context_injection` is async. Rust traits
   with async methods are not dyn-compatible without `async-trait`
   (allocation per call) or manual `Pin<Box<dyn Future>>` return types.
   Neither is used elsewhere in this codebase.

4. **Larger divergence surface**: the trait definition file, the
   `extension` field, and all call sites are divergence. The grouped-struct
   has the same call-site count but a smaller definition surface (one
   struct + impl vs. one trait + one struct + one impl).

5. **No upstream compatibility benefit**: upstream Zed doesn't use
   `Thread` directly — it uses `NativeAgent::new_session` which constructs
   `Thread::new`. The grouped-struct approach is equally compatible: upstream
   `Thread::new` calls `Self::new_internal` which inits `kask: KaskThreadState::new()`.
   The kask-specific setters (`set_agent_id`, `set_static_context`, etc.)
   become methods on `KaskThreadState`, called via `thread.kask.set_xxx()`
   from `NativeAgent::new_session` — same call sites, just requalified.

### 4.4 Risks (both approaches)

| Risk | Severity | Mitigation |
|------|----------|------------|
| Behavioral change during extraction | High | Add pinning tests for the 5 untested behaviors BEFORE moving code (§5.1) |
| Borrow conflicts in `inject_completed_deferred_results` (modifies `self.messages`) | Medium | The method needs `&mut self.messages` + `&mut self.kask.deferred_tool_results`. Split: `drain` on `KaskThreadState` returns `Vec<CompletedDeferredResult>`, then the caller (`Thread`) applies them to `self.messages`. |
| `CachedSystemPrompt` / `CachedFilteredContext` visibility | Low | These are currently private to `thread.rs`. Move them to `kask_thread_state.rs` or make them `pub(crate)`. |
| `DeferredToolResult` / `CompletedDeferredResult` visibility | Low | Same — make `pub(crate)`. |
| `AgentId` type import | Low | Already imported in `thread.rs`. |
| Test breakage from field renames | Medium | Tests access `thread.deferred_tool_results` directly (L10787). Update to `thread.kask.deferred_tool_results` or add a test-only accessor. |
| `to_db` / `from_db` don't persist kask fields | None | Already the case — kask fields are `None`/default in `from_db`. No change. |

## 5. Migration path (incremental, build-green at every step)

### Phase 0 — Pin the untested behaviors (prerequisite, §2 gaps) ✅ DONE

Before moving any code, add pinning tests for the 5 untested behaviors.
These tests pin the CURRENT behavior so the refactoring can't silently
change it.

1. **`test_curator_memory_edit_tool_classification`** — ✅ pins B6
   (curator memory-edit tool classification predicate). Note: the full
   `enabled_tools` integration test was not written — instead the pure
   predicate `is_curator_memory_edit_tool` was pinned, which is the
   classification logic the gating branch uses.

2. **`test_last_completion_truncated_distinguishes_max_tokens_from_cancel`** —
   ✅ pins B9/B10 (truncation flag set/read).

3. **`test_system_prompt_override_bypasses_template`** — ✅ pins B4.

4. **`test_cached_filtered_context_reuses_on_unchanged_inputs`** — ✅ pins
   B16.

5. **`test_tool_retry_tracker_integration_in_thread`** — ✅ pins B8.

### Phase 1 — Create `KaskThreadState` skeleton (compiles, no behavior change) ✅ DONE

1. ✅ Created `crates/agent/src/kask_thread_state.rs` with the struct
   definition (all 9 fields) and `new()` / `default()`.
2. ✅ Added `mod kask_thread_state;` to `agent.rs`.
3. ✅ Added `pub(crate) kask: KaskThreadState` field to `Thread`.
4. ✅ In `new_internal` and `from_db`, initialized `kask: KaskThreadState::new()`.
5. ✅ Old fields removed in Phase 2 (no simultaneous-existence period —
   the skeleton was immediately populated).
6. ✅ `cargo check -p agent` — compiles.

### Phase 2 — Move fields one at a time (each step compiles + tests pass) ✅ DONE

All 9 fields moved. Each field was removed from `Thread`, all access
sites updated to `self.kask.xxx()`, constructors updated, and tests
updated to use `KaskThreadState` accessors.

**Actual order** (topological — fields with no dependencies first):

| Step | Field | Dependencies | Status |
|------|------|-------------|--------|
| 2.1 | `last_completion_truncated` | none | ✅ |
| 2.2 | `cached_system_prompt` | none | ✅ |
| 2.3 | `cached_filtered_context` | none | ✅ |
| 2.4 | `system_prompt_override` | none | ✅ |
| 2.5 | `agent_static_context` | `cached_system_prompt` (cache bust) | ✅ |
| 2.6 | `mcp_server_scope` | none | ✅ |
| 2.7 | `agent_id` | none | ✅ |
| 2.8 | `tool_retry_tracker` | none | ✅ |
| 2.9 | `deferred_tool_results` | none | ✅ |

### Phase 3 — Extract turn-loop behaviors into `KaskThreadState` methods ✅ DONE (merged into Phase 2)

Behaviors were extracted as methods on `KaskThreadState` during the
field moves (Phase 2), not as a separate phase. The turn-loop call
sites are one-liner method calls.

1. ✅ `drain_completed_deferred_results` → `KaskThreadState::drain_completed_deferred_results()`
   (calls the free function `thread::drain_completed_deferred_results`).
   The `Thread::drain_completed_deferred_results` delegate was inlined
   during code review.
2. ✅ `check_tool_retry` → `KaskThreadState::check_tool_retry()`.
3. ✅ `mcp_server_in_scope` → `KaskThreadState::mcp_server_in_scope()`.
   The free function was deleted during code review (dead code — only
   the test used it; the test was rewritten to use the method).
4. ✅ `is_curator_memory_edit_tool` → stays as a free function (pure
   predicate, not per-thread state).
5. Note: `build_turn_record` was NOT extracted as a separate method —
   the memory ingestion path in `run_turn` still constructs
   `ThreadTurnRecord` inline, reading `thread.agent_id()` (which
   delegates to `kask.agent_id()`). This is because the record needs
   `Thread` state (`id`, `messages`, `model`, `title`) that `KaskThreadState`
   doesn't have access to. The plan's `build_turn_record` method was
   not adopted.

### Phase 4 — Add `// zed-kask:` markers + update DIVERGENCE.md ✅ DONE

1. ✅ Added `// zed-kask: D2/D6/D25` markers at each call site in the turn
   loop (3 → 15 markers).
2. ✅ Updated DIVERGENCE.md D6 row to document `KaskThreadState`.
3. ✅ Updated DIVERGENCE.md D2 row to reference `KaskThreadState` for
   `agent_static_context` / `system_prompt_override` / `mcp_server_scope`.
4. ✅ Updated DIVERGENCE.md D25 row to reference `KaskThreadState::on_max_tokens`
   / `last_completion_truncated`.
5. ✅ Added `kask_thread_state.rs` to DIVERGENCE.md supporting files section.

### Phase 5 — Verification gate ✅ DONE

1. ✅ `cargo check -p agent --tests` — compiles.
2. ✅ `cargo test -p agent -- thread::tests` — 66/66 pass (61 existing + 5 new).
3. ✅ `cargo clippy -p agent --tests -- --deny warnings` — clean.
4. ✅ `grep -c "// zed-kask:" crates/agent/src/thread.rs` — 15 markers (up from 3).
5. ✅ Code review passed: 6 should-fix + 4 nits identified, all should-fix
   applied (dead free function deleted, stale docs fixed, DIVERGENCE.md
   D2/D25 updated, redundant allocation in `inherit_from` fixed, pass-through
   wrapper inlined).
6. Note: 1 pre-existing test failure
   (`test_non_streaming_tool_partial_input_then_retryable_error_flushes_canceled_message`)
   confirmed failing on clean `main` before this refactoring — not caused
   by these changes.

## 6. Prototype: hook trait for `agent_id` / memory ingestion

Per the request, here is a prototype of the hook trait approach for the
`agent_id` field and its associated behaviors (B1 memory ingestion, B2
context injection, B6 curator gating, B12 subagent inheritance).

### 6.1 Trait definition

```rust
// crates/agent/src/thread_extension.rs

use gpui::SharedString;
use language_model::LanguageModelRequestMessage;

/// Per-thread extension point for kask-specific behavior.
/// Upstream Zed constructs Thread with `extension: None`; kask constructs
/// with `extension: Some(Rc::new(RefCell::new(KaskThreadExtension::new())))`.
pub trait ThreadExtension: std::any::Any {
    /// The agent ID that owns this thread (D6 routing key).
    fn agent_id(&self) -> Option<&crate::AgentId> { None }

    /// Set the agent ID (called by NativeAgent::new_session for Curator).
    fn set_agent_id(&mut self, _agent_id: crate::AgentId) {}

    /// Whether this thread is a curator thread (for memory-edit tool gating).
    fn is_curator_thread(&self) -> bool { false }

    /// Build the ThreadTurnRecord for memory ingestion (D6).
    /// Takes Thread state by reference to avoid borrow conflicts.
    fn build_turn_record(
        &self,
        thread_id: &str,
        messages: &[std::sync::Arc<crate::thread::Message>],
        model: Option<&dyn language_model::LanguageModel>,
        title: Option<&SharedString>,
    ) -> Option<crate::ThreadTurnRecord> { None }

    /// Per-turn context injection (D6/D11). Returns messages to splice
    /// after the system prompt. Uses a concrete future type to avoid
    /// async-trait overhead.
    fn on_context_injection<'a>(
        &'a self,
        thread_id: &'a str,
        user_prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + 'a>> {
        Box::pin(async { Vec::new() })
    }

    /// Inherit state for a subagent (D6/D34).
    fn for_subagent(&self) -> Option<Rc<RefCell<dyn ThreadExtension>>> { None }
}
```

### 6.2 Kask implementation

```rust
// crates/agent/src/kask_thread_extension.rs

pub struct KaskThreadExtension {
    agent_id: Option<crate::AgentId>,
}

impl KaskThreadExtension {
    pub fn new() -> Self { Self { agent_id: None } }
}

impl ThreadExtension for KaskThreadExtension {
    fn agent_id(&self) -> Option<&crate::AgentId> { self.agent_id.as_ref() }

    fn set_agent_id(&mut self, agent_id: crate::AgentId) {
        self.agent_id = Some(agent_id);
    }

    fn is_curator_thread(&self) -> bool {
        self.agent_id.as_ref().is_some_and(|id| id.as_ref() == crate::CURATOR_AGENT_ID.as_ref())
    }

    fn build_turn_record(
        &self,
        thread_id: &str,
        messages: &[std::sync::Arc<crate::thread::Message>],
        model: Option<&dyn language_model::LanguageModel>,
        title: Option<&SharedString>,
    ) -> Option<crate::ThreadTurnRecord> {
        // Only curator threads ingest — user/zed agent has no memory.
        if !self.is_curator_thread() {
            return None;
        }
        Some(crate::ThreadTurnRecord {
            thread_id: thread_id.to_string(),
            user_input: messages.iter().rev()
                .find_map(|msg| match &**msg {
                    crate::thread::Message::User(user_msg) => Some(user_msg.to_markdown()),
                    _ => None,
                }).unwrap_or_default(),
            agent_response: messages.iter().rev()
                .find_map(|msg| match &**msg {
                    crate::thread::Message::Agent(agent_msg) => Some(agent_msg.to_markdown()),
                    _ => None,
                }).unwrap_or_default(),
            model: model.map(|m| m.name().0.to_string()).unwrap_or_default(),
            thread_title: title.map(|t| t.to_string()),
            agent_id: self.agent_id.clone(),
        })
    }

    fn on_context_injection<'a>(
        &'a self,
        thread_id: &'a str,
        user_prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + 'a>> {
        let agent_id = self.agent_id.clone();
        Box::pin(async move {
            let Some(agent_id) = agent_id else { return Vec::new() };
            let Some(injector) = crate::context_injector_for(Some(&agent_id)) else { return Vec::new() };
            injector.inject_context(thread_id, user_prompt).await
        })
    }

    fn for_subagent(&self) -> Option<Rc<RefCell<dyn ThreadExtension>>> {
        Some(Rc::new(RefCell::new(KaskThreadExtension {
            agent_id: self.agent_id.clone(),
        })))
    }
}
```

### 6.3 Thread wiring

```rust
// In Thread struct:
pub(crate) extension: Option<Rc<RefCell<dyn ThreadExtension>>>,

// In new_internal / from_db:
extension: None, // upstream default — kask sets it via set_extension()

// New method:
pub fn set_extension(&mut self, ext: Rc<RefCell<dyn ThreadExtension>>) {
    self.extension = Some(ext);
}
```

### 6.4 Call site changes (B1, B2, B6, B12)

```rust
// B1: Memory ingestion (run_turn)
if let Some(ext) = &this.read_with(cx, |t, _| t.extension.clone()).ok().flatten() {
    let record = this.update(cx, |thread, _| ext.borrow().build_turn_record(
        &thread.id.to_string(),
        &thread.messages,
        thread.model().map(|m| m.as_ref()),
        thread.title(),
    ));
    if let Some(Some(record)) = record.ok() {
        if let Some(port) = crate::memory_port() {
            cx.background_spawn(async move {
                if let Err(e) = port.ingest_turn(record).await {
                    log::warn!("Memory ingestion failed: {e}");
                }
            }).detach();
        }
    }
}

// B2: Context injection (run_turn_internal)
let ext = this.read_with(cx, |t, _| t.extension.clone()).ok().flatten();
if let Some(ext) = ext {
    let thread_id = this.read_with(cx, |t, _| t.id.to_string()).ok().unwrap_or_default();
    let injected = ext.borrow().on_context_injection(&thread_id, &user_prompt).await;
    if !injected.is_empty() {
        request.messages.splice(1..1, injected);
    }
}

// B6: Curator gating (enabled_tools)
let is_curator_thread = self.extension.as_ref()
    .is_some_and(|e| e.borrow().is_curator_thread());

// B12: Subagent inheritance (new_subagent)
if let Some(parent_ext) = parent_thread.read(cx).extension.clone() {
    thread.extension = parent_ext.borrow().for_subagent();
}
```

### 6.5 What the prototype reveals

1. **`Rc<RefCell<>>` noise**: every access is `self.extension.as_ref().map(|e| e.borrow().xxx())`.
   Compare with the grouped-struct: `self.kask.xxx()`. The trait version
   is ~2x more verbose at each call site.

2. **`build_turn_record` returns `Option<Option<...>>`**: the outer `Option`
   is "extension exists?", the inner is "thread is curator?". This is
   awkward — the grouped-struct version returns `ThreadTurnRecord` directly
   (the memory_port check is in the caller, not the extension).

3. **`on_context_injection` uses `Pin<Box<dyn Future>>`**: this is the
   least-bad way to make an async trait method dyn-compatible, but it
   allocates per call. The grouped-struct version calls the injector
   directly (no allocation).

4. **`for_subagent` returns `Option<Rc<RefCell<dyn ...>>>`**: the subagent
   inheritance is a clone-and-wrap. The grouped-struct version is
   `KaskThreadState::inherit_from(&parent.kask)` — a struct construction.

5. **The trait doesn't actually decouple from kask**: the `KaskThreadExtension`
   impl is kask-specific code that lives in the `agent` crate (which is
   zed-kask-side, not `kask/`-side). Moving it to `kask/` would require
   the trait to be in a shared crate, adding a new dependency edge. The
   grouped-struct lives in the same crate with the same dependency profile.

**Verdict:** the prototype validates that the hook trait is *feasible*
but confirms it is *not worth the complexity* for a single-implementation
case. The grouped-struct approach (§3.2) achieves the same separation
with less code, no allocation, and no `RefCell` noise.

## 7. Recommendation

**Adopt the grouped-struct approach (§3.2) with the 5-phase migration
path (§5).** The hook trait prototype (§6) confirms the approach is
feasible but over-engineered for this codebase.

The grouped-struct approach:
- Reduces 9 kask-specific fields to 1 (`kask: KaskThreadState`)
- Extracts turn-loop behaviors into testable methods
- Keeps upstream `Thread::new_internal` / `from_db` clean (one line: `kask: KaskThreadState::new()`)
- Is incremental (each field moves independently, build stays green)
- Is consistent with the existing `Rc<RefCell<ToolRetryTracker>>` pattern
- Adds `// zed-kask:` markers at every call site (fixing the under-marking)

The hook trait approach is documented here for posterity but should not
be adopted unless a second `ThreadExtension` implementation is needed
(which would make the trait non-speculative).