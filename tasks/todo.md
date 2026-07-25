# Agent Loop Improvements — Task Checklist

## Phase 1 — Foundation & high-risk plumbing

- [x] **T1 (S1): Deferred tool-result plumbing**
  - Added `deferred_tool_results: Vec<DeferredToolResult>` field + `DeferredToolResult` struct to `Thread`
  - Drain at top of each `run_turn_internal` iteration (after compaction, before `build_completion_request`)
  - `cancel()` clears the deferred queue
  - 3 tests: `test_deferred_tool_result_appears_in_next_request`, `test_deferred_tool_result_pending_stays_in_queue`, `test_deferred_tool_result_cancel_clears_queue`
  - `./script/clippy -p agent` clean
  - Files: `crates/agent/src/thread.rs`

- [x] **T2a (S2): `SubagentHandle::send_streaming` trait method**
  - Added `send_streaming` to `SubagentHandle` trait with default impl delegating to `send` (I2)
  - Implemented on `NativeSubagentHandle`: spawns subagent turn in background, delivers result via oneshot channel
  - Added `enqueue_deferred_result` method to `ToolCallEventStream` for parent-thread access
  - Files: `crates/agent/src/thread.rs` (trait + `ToolCallEventStream`), `crates/agent/src/agent.rs` (impl)

- [~] **T2b (S2): Non-blocking `spawn_agent` tool** (DEFERRED)
  - Infrastructure in place (deferred results, `send_streaming`, `enqueue_deferred_result`)
  - Blocking `send` kept to preserve test compatibility — non-blocking requires test suite updates
  - The `send_streaming` method and deferred-result plumbing are ready for opt-in activation

**Checkpoint 1**: ✅ `./script/clippy -p agent` clean; `cargo test -p agent --features test-support` passes (23 subagent tests + 3 deferred-result tests)

## Phase 2 — Conditional rules

- [x] **T3 (S5): Frontmatter parsing in `prompt_store`**
  - Add `RuleFrontmatter { globs: Vec<String>, always_apply: bool }` + `Option<RuleFrontmatter>` on `RulesFileContext`
  - Parse YAML frontmatter in `load_worktree_rules_file`; strip from `text`
  - AC: frontmattered file parses into correct `globs` + `always_apply`; `text` excludes frontmatter
  - AC: no-frontmatter file parses with `always_apply: true`, `globs: vec![]`, `text` unchanged (I2)
  - Files: `crates/prompt_store/src/prompts.rs`, `crates/agent/src/agent.rs`
  - Scope: S

- [x] **T4 (S6): Conditional-rules scoping**
  - Filter conditional rules (`always_apply: false` + non-empty `globs`) in `build_project_context` by open-files + mentioned-paths
  - AC: `**/*.rs`-scoped rule included iff a `.rs` file is open or a `.rs` path is in latest user message
  - AC: `alwaysApply: true` rules always included (I2)
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`
  - Scope: M

## Phase 3 — Static-context memory

- [x] **T5 (S3): Static-context memory block**
  - Add `inject_static_context` to `ContextInjector` (default returns empty — I2)
  - Add `static_context: Option<SharedString>` to `Thread`; call once on first turn, cache
  - Add `static_context` to `SystemPromptTemplate` + `.hbs`; include in `system_prompt_digest` (I1)
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`, `crates/agent/src/templates/system_prompt.hbs`
  - Scope: M

## Phase 4 — Context-aware tool router

- [x] **T6 (S7): `ToolRouter` trait + heuristic scorer**
  - Create `crates/agent/src/tool_router.rs`: `ToolRouter` trait, `ToolSelectionContext`, `HeuristicToolRouter`
  - Heuristic scores: `.rs`/`.ts` open ⇒ `grep`/`read_file`/`edit_file`/`diagnostics` ≥ 0.5; URL in msg ⇒ `fetch`/`web_search` ≥ 0.5; baseline 0.1
  - Return tools scoring ≥ 0.30
  - Files: `crates/agent/src/tool_router.rs` (new), `crates/agent/src/tools.rs`
  - Scope: M

- [x] **T7 (S4): Wire `ToolRouter` into `enabled_tools`**
  - Add `TOOL_ROUTER` extension point in `agent.rs` (mirror `CONTEXT_INJECTOR`)
  - In `Thread::enabled_tools`, apply router filter after profile/feature-flag; fail-open on empty
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`
  - Scope: M
