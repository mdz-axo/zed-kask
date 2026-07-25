# Agent Loop Improvements — Task Checklist

## Phase 1 — Foundation & high-risk plumbing

- [x] **T1: Deferred tool-result plumbing**
  - `DeferredToolResult` with `owning_message_ix` + `tool_name`, `Option<Pin<Box<oneshot::Receiver>>>`
  - Drain at top of `run_turn_internal` after compaction, before `build_completion_request`
  - Inject into original agent message (not synthetic orphan) — API-compliant `tool_use` + `tool_result` pairing
  - `cancel()` clears the queue; `end_turn` path waits for pending deferred results (closure fix)
  - 3 tests
  - Files: `crates/agent/src/thread.rs`

- [x] **T2a: `SubagentHandle::send_streaming` trait method**
  - Async trait method with default impl delegating to `send` (I2)
  - `NativeSubagentHandle` override spawns background turn, delivers via oneshot
  - `ToolCallEventStream::enqueue_deferred_result` bridges to parent thread with `owning_message_ix` + `tool_name`
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`

- [~] **T2b: Non-blocking `spawn_agent` tool** (DEFERRED)
  - Infrastructure complete; blocking `send` kept for test compatibility
  - Activation requires updating the subagent test suite's timing assumptions

## Phase 2 — Conditional rules

- [x] **T3: Frontmatter parsing**
  - `RuleFrontmatter { globs, always_apply }` with manual `Default` impl (always_apply: true)
  - `parse_rules_frontmatter()` handles empty frontmatter (`---\n---`), no closing fence, malformed YAML
  - 6 tests
  - Files: `crates/prompt_store/src/prompts.rs`, `crates/agent/src/agent.rs`

- [x] **T4: Conditional-rules scoping**
  - `filter_conditional_rules()` in `render_system_prompt` — clones `ProjectContext`, filters by open-file + mentioned paths
  - Relative glob matching via `globset` with worktree-prefix stripping
  - `has_rules` recomputed after filtering
  - Cached via `CachedFilteredContext` + `filter_inputs_digest` — avoids per-render clone when inputs stable
  - Invalid globs logged via `log::warn!`
  - 4 tests
  - Files: `crates/agent/src/thread.rs`

## Phase 3 — Static-context memory

- [x] **T5: Static-context memory block**
  - `inject_static_context` is async (returns `Pin<Box<Future>>`) — no `block_on` deadlock risk
  - Loaded in `run_turn_internal` (async context) before `build_completion_request`, cached on `Thread`
  - `BridgeContextInjector` overrides with high-confidence memory recall
  - Rendered in system prompt after project context, included in digest
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`, `crates/agent/src/templates/system_prompt.hbs`, `kask/crates/kask_bridge/src/context_injector.rs`

## Phase 4 — Context-aware tool router

- [x] **T6: Lazy tool router**
  - `LazyToolRouter` — only activates on explicit tool requests, complex messages (≥40 words or decomposition signals), or code-file + edit signals
  - Scores all tools (including MCP) by keyword overlap with descriptions
  - Returns `Option<Vec<SharedString>>` — `None` = fail-open, `Some(vec)` = filter
  - 8 tests
  - Files: `crates/agent/src/tool_router.rs` (new)

- [x] **T7: Wire `ToolRouter` into `enabled_tools` + composition root**
  - `TOOL_ROUTER` extension point in `agent.rs` — returns `None` by default (I2)
  - Applied in `Thread::enabled_tools` after profile/feature-flag filter
  - Wired in `crates/zed/src/main.rs` composition root: `set_tool_router(Some(Arc::new(LazyToolRouter::new())))`
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`, `crates/zed/src/main.rs`

## Adversarial review fixes

- [x] **C1**: `tool_router()` returns `None` by default (I2 compliance)
- [x] **C2**: `BridgeContextInjector` implements `inject_static_context`
- [x] **B1**: Deferred result injected into original agent message (not synthetic orphan)
- [x] **B2**: Deferred results survive replay (injected into original message's `tool_results`)
- [x] **B3**: Correct `scoped_tool_call_id` using original `owning_message_ix`
- [x] **B4**: Relative globs match via worktree-prefix stripping
- [x] **B5**: Router returns `Option<Vec>` to distinguish "not activated" from "no matches"
- [x] **M4**: `tool_name` stored on `DeferredToolResult`, not hardcoded
- [x] **M6**: Empty frontmatter `---\n---` recognized
- [x] **1.1**: `inject_static_context` is async — no `block_on` deadlock
- [x] **1.2**: Clone-and-replace pattern instead of `Arc::get_mut` (no silent data loss)
- [x] **1.4**: `CachedFilteredContext` avoids per-render `ProjectContext` clone
- [x] **2.1**: `end_turn` path checks for pending deferred results (closure fix)
- [x] **3.3**: `filter_conditional_rules` doc clarifies I2 scope
- [x] **3.4**: `drain_completed_deferred_results` doc updated (noop waker, not `now_or_never`)

## Remaining (not blocking)

- T2b: Non-blocking `spawn_agent` — requires test suite timing updates
- 1.5: Redundant `opened_buffers` traversal (twice per turn) — low priority
- 2.4: Static context has no mid-session refresh — by design ("loaded once per session")
