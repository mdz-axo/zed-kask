# Agent Loop Improvements — Task Checklist

## Phase 1 — Foundation & high-risk plumbing

- [x] **T1: Deferred tool-result plumbing**
  - `DeferredToolResult` with `owning_message_ix` + `tool_name`, `Option<Pin<Box<oneshot::Receiver>>>`
  - Drain at top of `run_turn_internal` after compaction, before `build_completion_request`
  - Inject into original agent message via clone-and-replace (not `Arc::get_mut`)
  - `cancel()` clears the queue
  - 3 tests
  - Files: `crates/agent/src/thread.rs`

- [x] **T2a: `SubagentHandle::send_streaming` trait method**
  - Async trait method with default impl delegating to `send` (I2)
  - `NativeSubagentHandle` override spawns background turn, delivers via oneshot
  - `ToolCallEventStream::enqueue_deferred_result` with `owning_message_ix` from stream
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`

- [~] **T2b: Non-blocking `spawn_agent` tool** (DEFERRED — GPUI test scheduler incompatibility)
  - Infrastructure complete and tested in isolation
  - Blocking `send` kept: the GPUI test scheduler's `run_until_parked` cannot handle
    a foreground task waiting on a background task (causes "Parking forbidden")
  - Non-blocking activation requires either a test scheduler change or a
    `cx.notify()`-based watcher task pattern

## Phase 2 — Conditional rules

- [x] **T3: Frontmatter parsing**
  - `RuleFrontmatter` with manual `Default` (always_apply: true)
  - Handles empty frontmatter, no closing fence, malformed YAML
  - 6 tests
  - Files: `crates/prompt_store/src/prompts.rs`, `crates/agent/src/agent.rs`

- [x] **T4: Conditional-rules scoping**
  - `filter_conditional_rules()` in `render_system_prompt`
  - Relative glob matching via worktree-prefix stripping + `globset`
  - `CachedFilteredContext` + `filter_inputs_digest` avoids per-render clone
  - Invalid globs logged
  - 4 tests
  - Files: `crates/agent/src/thread.rs`

## Phase 3 — Static-context memory

- [x] **T5: Static-context memory block**
  - `inject_static_context` is async (`Pin<Box<Future>>`) — no `block_on` deadlock
  - Loaded in `run_turn_internal` (async), cached on `Thread`
  - `BridgeContextInjector` overrides with high-confidence memory recall
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`, `crates/agent/src/templates/system_prompt.hbs`, `kask/crates/kask_bridge/src/context_injector.rs`

## Phase 4 — Context-aware tool router

- [x] **T6: Lazy tool router**
  - `LazyToolRouter` — activates on explicit tool requests, complex messages, or code-file + edit signals
  - Scores all tools (including MCP) by keyword overlap with descriptions
  - Returns `Option<Vec>` — `None` = fail-open, `Some(vec)` = filter
  - 8 tests
  - Files: `crates/agent/src/tool_router.rs`

- [x] **T7: Wire `ToolRouter` into `enabled_tools` + composition root**
  - `TOOL_ROUTER` extension point — `None` by default (I2)
  - Wired in `crates/zed/src/main.rs`: `set_tool_router(Some(Arc::new(LazyToolRouter::new())))`
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`, `crates/zed/src/main.rs`

## Adversarial review fixes

- [x] C1: `tool_router()` returns `None` by default (I2)
- [x] C2: `BridgeContextInjector` implements `inject_static_context`
- [x] B1: Deferred result injected into original agent message
- [x] B2: Deferred results survive replay
- [x] B3: Correct `scoped_tool_call_id` using original `owning_message_ix`
- [x] B4: Relative globs match via worktree-prefix stripping
- [x] B5: Router returns `Option<Vec>` to distinguish states
- [x] M4: `tool_name` stored on `DeferredToolResult`
- [x] M6: Empty frontmatter `---\n---` recognized
- [x] 1.1: `inject_static_context` is async — no `block_on`
- [x] 1.2: Clone-and-replace instead of `Arc::get_mut`
- [x] 1.4: `CachedFilteredContext` avoids per-render clone
- [x] 3.3: `filter_conditional_rules` doc clarifies I2 scope
- [x] 3.4: `drain_completed_deferred_results` doc updated

## .rules additions

- [x] No `block_on` on foreground thread
- [x] Mutating `Arc<Message>` in `Thread.messages` (clone-and-replace, not `get_mut`)
- [x] Deferred results and the turn loop (no busy-spin/timer in `end_turn`)

## Remaining (non-blocking, documented)

- T2b: Non-blocking `spawn_agent` — requires GPUI test scheduler change or watcher-task pattern
- 1.5: Redundant `opened_buffers` traversal (twice per turn) — low impact, `CachedFilteredContext` mitigates
- 2.4: Static context has no mid-session refresh — by design
