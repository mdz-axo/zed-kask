# Agent Loop Improvements — Task Checklist

## Phase 1 — Foundation & high-risk plumbing

- [ ] **T1 (S1): Deferred tool-result plumbing**
  - Add `deferred_tool_results: Vec<DeferredToolResult>` field + `DeferredToolResult` struct to `Thread`
  - Drain due deferred results at top of each `run_turn_internal` iteration (after compaction, before `build_completion_request`)
  - AC: deferred result enqueued in iteration N appears as tool-result message in iteration N+1 with correct `tool_use_id`
  - AC: with no deferred results, behavior is byte-identical to before (no extra messages, no digest change)
  - AC: `test_deferred_tool_result_appears_in_next_request` passes; `./script/clippy` clean
  - Files: `crates/agent/src/thread.rs`
  - Scope: M

- [ ] **T2a (S2): `SubagentHandle::send_streaming` trait method**
  - Add `send_streaming` to `SubagentHandle` trait with default impl delegating to `send` (I2)
  - Implement on `NativeSubagentHandle`: returns progress stream + final result future
  - AC: default impl preserves blocking behavior for any impl that doesn't override
  - AC: `NativeSubagentHandle::send_streaming` returns a stream of progress events + a final `Result<String>`
  - Files: `crates/agent/src/thread.rs` (trait), `crates/agent/src/agent.rs` (impl)
  - Scope: M

- [ ] **T2b (S2): Non-blocking `spawn_agent` tool**
  - Rewrite `SpawnAgentTool::run` to return immediate placeholder result + enqueue `DeferredToolResult`
  - Stream progress via `ToolCallEventStream::update_fields` while subagent runs
  - AC: parent's tool-result slot freed after `spawn_agent`; parent receives `StopReason::ToolUse` and can continue
  - AC: subagent final output appears as tool result in parent's next request, keyed by original `tool_use_id`
  - AC: cancelling parent cancels all running subagents (existing `running_subagents` path still works)
  - AC: `test_non_blocking_subagent_streams_then_delivers` passes; existing subagent tests pass; `./script/clippy` clean
  - Files: `crates/agent/src/tools/spawn_agent_tool.rs`, `crates/agent/src/thread.rs`
  - Scope: M

**Checkpoint 1**: `./script/clippy` clean; `cargo test -p agent --features test-support` passes; manual smoke: spawn two subagents in one parent turn, observe parallel execution.

## Phase 2 — Conditional rules

- [ ] **T3 (S5): Frontmatter parsing in `prompt_store`**
  - Add `RuleFrontmatter { globs: Vec<String>, always_apply: bool }` + `Option<RuleFrontmatter>` on `RulesFileContext`
  - Parse YAML frontmatter in `load_worktree_rules_file`; strip from `text`
  - AC: frontmattered file parses into correct `globs` + `always_apply`; `text` excludes frontmatter
  - AC: no-frontmatter file parses with `always_apply: true`, `globs: vec![]`, `text` unchanged (I2)
  - AC: rendered prompt for `alwaysApply: true` frontmattered file is byte-identical to same file without frontmatter
  - Files: `crates/prompt_store/src/prompts.rs`, `crates/agent/src/agent.rs`
  - Scope: S

- [ ] **T4 (S6): Conditional-rules scoping**
  - Filter conditional rules (`always_apply: false` + non-empty `globs`) in `build_project_context` by open-files + mentioned-paths
  - Add project-event subscription for open-file changes (mirror existing rules-file-change subscription)
  - AC: `**/*.rs`-scoped rule included iff a `.rs` file is open or a `.rs` path is in latest user message
  - AC: opening/closing a matching file mid-session changes the digest (rule appears/disappears)
  - AC: `alwaysApply: true` rules always included (I2)
  - AC: `test_conditional_rule_scoped_to_open_file` passes; existing rules tests pass; `./script/clippy` clean
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`
  - Scope: M

**Checkpoint 2**: `./script/clippy` clean; `cargo test -p prompt_store` + `cargo test -p agent --features test-support rules` pass; manual smoke: add `**/*.rs`-scoped rule, open/close `.rs` file, observe rule appear/disappear in prompt (digest change in telemetry).

## Phase 3 — Static-context memory

- [ ] **T5 (S3): Static-context memory block**
  - Add `inject_static_context` to `ContextInjector` (default returns empty — I2)
  - Add `static_context: Option<SharedString>` to `Thread`; call once on first turn, cache
  - Add `static_context: Option<SharedString>` to `SystemPromptTemplate` + `.hbs`; render after project context
  - Include `static_context` in `system_prompt_digest` (I1)
  - AC: with `ContextInjector` set + non-empty static context, prompt contains static block exactly once after project context
  - AC: digest changes when static context changes (`test_system_prompt_digest_includes_static_context`)
  - AC: with no `ContextInjector`, prompt byte-identical to before (extend `test_system_prompt_digest_stability`)
  - AC: `./script/clippy` clean
  - Files: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`, `crates/agent/src/templates/system_prompt.hbs`
  - Scope: M

**Checkpoint 3**: `./script/clippy` clean; `cargo test -p agent --features test-support system_prompt` passes; manual smoke: set `ContextInjector` returning static context, confirm it appears once + digest changes on change.

## Phase 4 — Context-aware tool router

- [ ] **T6 (S7): `ToolRouter` trait + heuristic scorer**
  - Create `crates/agent/src/tool_router.rs` (no `mod.rs`): `ToolRouter` trait, `ToolSelectionContext`, `HeuristicToolRouter`
  - Register module in `tools.rs` or `lib.rs`
  - Heuristic scores: `.rs`/`.ts` open ⇒ `grep`/`read_file`/`edit_file`/`diagnostics` ≥ 0.5; URL in msg ⇒ `fetch`/`web_search` ≥ 0.5; "terminal"/"run" in msg ⇒ `terminal` ≥ 0.5; baseline 0.1
  - Return tools scoring ≥ 0.30
  - AC: `select_tools` returns `grep`+`read_file` for open `.rs` file, no URL
  - AC: returns `fetch`+`web_search` for URL in message, no open code file
  - AC: `test_heuristic_tool_router_scores` passes; `./script/clippy` clean
  - Files: `crates/agent/src/tool_router.rs` (new), `crates/agent/src/tools.rs` or `lib.rs`
  - Scope: M

- [ ] **T7 (S4): Wire `ToolRouter` into `enabled_tools`**
  - Add `static TOOL_ROUTER: OnceLock<Option<Arc<dyn ToolRouter>>>` in `agent.rs` (mirror `CONTEXT_INJECTOR`)
  - In `Thread::enabled_tools`, after profile/feature-flag filter, if router set: build `ToolSelectionContext`, retain only router-selected tools
  - Empty router result ⇒ fail-open (no filtering) to avoid starving the model
  - Filtered set feeds `render_system_prompt` `available_tools` (digest reflects it — I1)
  - AC: router selecting only `grep`+`read_file` ⇒ next request `tools` array contains exactly those (+ MCP tools passed through)
  - AC: no router set ⇒ `enabled_tools` returns same set as before (I2)
  - AC: digest changes when router selects different set (`test_digest_reflects_tool_router_selection`)
  - AC: `./script/clippy` clean
  - Files: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`
  - Scope: M

**Checkpoint 4**: `./script/clippy` clean; `cargo test -p agent --features test-support tool_router` passes; manual smoke: with router set, confirm tool set narrows by open files; with no router, confirm no regression.
