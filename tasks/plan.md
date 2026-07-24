# Agent Loop Improvements — Task Breakdown Plan

<!--
DC+BIBO document metadata
Title:        Agent Loop Improvements — Task Breakdown Plan
Creator:      task-breakdown skill (GLM 5.2 agent)
Date:         2026-07-24
Type:         bibo:Document
Description:  Vertical-slice decomposition of four agent REPL/chat loop improvements
              (non-blocking subagents, conditional rules, static-context memory,
              context-aware tool router) for the zed-kask codebase.
-->

## Overview

Four improvements to the agent REPL/chat loop in `crates/agent`, decomposed into
vertical slices that each deliver a testable, end-to-end behavior change while
keeping the system in a working state after every slice. Mode-based tool scoping
(Roo Code modes) is explicitly abandoned; the fourth item replaces it with a
context-aware tool router.

The four improvements, ordered by risk (highest first) and dependency:

1. **Non-blocking subagents** — `spawn_agent` returns immediately with a
   `session_id` and streams progress; the final result is delivered as a
   deferred tool result in a subsequent outer-loop iteration.
2. **Conditional rules** — YAML frontmatter on `AGENTS.md` / `.rules` files
   scopes rules to file globs; only matching rules enter the system prompt.
3. **Static-context memory block** — a project-level memory block loaded once
   per session via the existing `ContextInjector` trait, included in the
   system-prompt digest (not retrieved per-turn).
4. **Context-aware tool router** — a Rust-side `ToolRouter` trait that filters
   the tool set per turn based on context (open files, message content),
   replacing static mode-based scoping.

All four share two invariants that every slice must preserve:

- **I1 — System-prompt digest cache correctness**: any change to system-prompt
  inputs (rules text, static context, available tools) must be reflected in
  the digest computed by `system_prompt_digest` (thread.rs L4862) so cache
  busts are observable and stale prompts are never served.
- **I2 — Upstream Zed compatibility**: all new features are no-ops when the
  extension point is unset. `ContextInjector`, `ToolRouter`, the
  `SubagentHandle::send_streaming` default, and frontmatter parsing
  (frontmatter absent ⇒ `alwaysApply: true`) must all degrade to current
  behavior when not wired.

## Architecture Decisions

### AD-1: Deferred tool results for non-blocking subagents

The inner loop's `tool_results: FuturesUnordered` drains all tool tasks before
re-entering the outer loop (thread.rs L3104–3111). A non-blocking subagent
cannot block this drain. Decision: `spawn_agent` returns an immediate
placeholder `LanguageModelToolResult` ("subagent spawned, session_id=X, will
report when done") that completes the current tool-result slot, and registers a
**deferred tool result** keyed by `tool_use_id` on the `Thread`. The outer loop
checks for due deferred results at the top of each iteration (after compaction,
before `build_completion_request`) and, if any are ready, inserts them as a
synthetic tool-result message so the model sees them in the next request. This
fits the existing architecture without restructuring the inner-loop race.

Rationale: option (a) from the initiative brief is simpler and avoids
introducing a stream-of-results type into the inner loop's `select!`. Progress
events flow through the existing `ToolCallEventStream::update_fields` path
(already used by streaming tool inputs); only the *final* result is deferred.

### AD-2: Frontmatter parsing in `prompt_store`, scoping in `agent`

`RulesFileContext` (prompts.rs L95) gains an `Option<RuleFrontmatter>` field
with `globs: Vec<String>` and `always_apply: bool`. Parsing happens in
`load_worktree_rules_file` (agent.rs L1267) — the frontmatter is stripped from
the `text` field so the system prompt never sees the YAML. Scoping happens in
`build_project_context` (agent.rs L1032): conditional rules are filtered out of
the `WorktreeContext.rules_file` unless a matching file is open or mentioned.
The filtered `ProjectContext` is what feeds `render_system_prompt`, so the
digest (AD/I1) automatically reflects the conditional-rules state — no
separate digest field is needed.

Frontmatter format is Cline-compatible (`globs`, `alwaysApply`). When
frontmatter is absent or `alwaysApply: true`, behavior is unchanged (I2).

### AD-3: Static context via a new `ContextInjector` method, cached on `Thread`

Add `inject_static_context(&self, thread_id: &str) -> Vec<LanguageModelRequestMessage>`
to the `ContextInjector` trait with a default impl returning `Vec::new()` (I2).
Call it once on the first turn of a session (or lazily on first
`render_system_prompt`), cache the result on `Thread` as
`static_context: Option<SharedString>`, and include it in
`SystemPromptTemplate` as a new `static_context: Option<SharedString>` field
rendered after the project context block. The digest must hash this field
(I1). The per-turn `inject_context` continues unchanged for dynamic retrieval.

### AD-4: `ToolRouter` trait with heuristic default, plugged into `enabled_tools`

A new `crates/agent/src/tool_router.rs` file defines `ToolRouter` (trait) and
`ToolSelectionContext` (struct: latest user message, open file paths, mentioned
paths, available tool names). The router returns `Vec<SharedString>` of selected
tool names above a 0.30 threshold. A `HeuristicToolRouter` default impl scores
tools by simple signals (`.rs` open ⇒ boost `grep`/`read_file`/`edit_file`;
URL in message ⇒ boost `fetch`/`web_search`; etc.). The router plugs into
`Thread::enabled_tools` (thread.rs L4222) as a final filter after
profile/feature-flag checks. When no router is wired (upstream Zed), all
enabled tools pass through (I2). The selected tool set feeds
`render_system_prompt`'s `available_tools`, so the digest (I1) reflects it.

## Dependency Graph

```
                      ┌──────────────────────────────────────────┐
                      │ S1: Deferred tool-result plumbing (found.)│
                      └─────────────────┬────────────────────────┘
                                        │
                          ┌─────────────┴─────────────┐
                          ▼                           ▼
                ┌────────────────────┐      ┌──────────────────────┐
                │ S2: Non-blocking   │      │ S5: Frontmatter      │
                │   subagent tool    │      │   parsing (prompt_   │
                │   (end-to-end)     │      │   store)             │
                └────────────────────┘      └──────────┬───────────┘
                                                       │
                                                       ▼
                                          ┌────────────────────────┐
                                          │ S6: Conditional-rules  │
                                          │   scoping (end-to-end) │
                                          └────────────────────────┘
                                                       │
                                          ┌────────────┴───────────┐
                                          ▼                        ▼
                                ┌──────────────────┐     ┌─────────────────────┐
                                │ S3: Static-       │     │ S7: ToolRouter      │
                                │   context memory  │     │   trait + heuristic  │
                                │   (end-to-end)    │     │   (end-to-end)       │
                                └──────────────────┘     └─────────────────────┘
                                                       │
                                                       ▼
                                          ┌────────────────────────┐
                                          │ S4: Wire ToolRouter   │
                                          │   into enabled_tools  │
                                          └────────────────────────┘
```

- **S1** is the foundation for S2 (non-blocking subagents need the deferred
  result mechanism). It is also independently valuable (enables any future
  deferred tool result).
- **S5 → S6**: frontmatter parsing must exist before scoping can filter on it.
- **S3** depends on S1 only loosely (both touch `Thread` fields and the digest)
  but is scheduled after S2 to keep the high-risk subagent work contiguous and
  to avoid interleaving two `Thread` struct changes in the same session.
- **S7 → S4**: the trait must exist before it is wired into `enabled_tools`.
- S6 and S7 are independent and could be parallelized across two sessions if
  needed, but the plan sequences them to keep `Thread` struct edits
  single-threaded.

## Slices (Vertical)

### S1 — Deferred tool-result plumbing (foundation)
- **slice_id**: `agent/deferred-tool-results`
- **feature_path**: `crates/agent/src/thread.rs`
- **Description**: Add a `deferred_tool_results: Vec<DeferredToolResult>` field
  to `Thread` and a check at the top of each `run_turn_internal` iteration that
  drains due deferred results into a synthetic tool-result message before
  `build_completion_request`. No tool uses this yet — the slice delivers the
  mechanism plus a test that injects a deferred result manually and observes it
  appear in the next request.
- **Acceptance criteria**:
  - A deferred result enqueued during iteration N appears as a tool-result
    message in the request built at iteration N+1, with the correct
    `tool_use_id`.
  - With no deferred results enqueued, `run_turn_internal` behavior is
    byte-identical to before (no extra messages, no digest change).
  - New unit test `test_deferred_tool_result_appears_in_next_request` passes;
    `./script/clippy` clean.
- **Verification**: `cargo test -p agent --features test-support deferred_tool_result`
  + `./script/clippy`.
- **Dependencies**: None.
- **Files likely touched**: `crates/agent/src/thread.rs`.
- **Estimated scope**: M.

### S2 — Non-blocking `spawn_agent` tool (end-to-end)
- **slice_id**: `agent/non-blocking-subagent`
- **feature_path**: `crates/agent/src/tools/spawn_agent_tool.rs`,
  `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`
- **Description**: `SpawnAgentTool::run` returns an immediate placeholder
  result ("subagent spawned, session_id=X, will report when done") and enqueues
  a `DeferredToolResult` carrying the real `subagent.send()` task. Progress
  events flow via `ToolCallEventStream::update_fields` while the subagent runs.
  When the subagent completes, the deferred result is marked due and the outer
  loop delivers it. Add `SubagentHandle::send_streaming` with a default impl
  that delegates to `send` (so upstream `NativeSubagentHandle` and any other
  impl keep working — I2). The parent can spawn multiple subagents in parallel
  and continue generating text while they run.
- **Acceptance criteria**:
  - After `spawn_agent` returns, the parent's tool-result slot is freed and the
    parent receives a `StopReason::ToolUse` (not blocked) so it can continue.
  - When the subagent completes, its final output appears as a tool result in
    the parent's next request, keyed by the original `tool_use_id`.
  - Cancelling the parent cancels all running subagents (existing
    `running_subagents` cancellation still works).
  - New test `test_non_blocking_subagent_streams_then_delivers` passes; existing
    subagent tests still pass; `./script/clippy` clean.
- **Verification**: `cargo test -p agent --features test-support subagent` +
  `./script/clippy`.
- **Dependencies**: S1.
- **Files likely touched**: `crates/agent/src/tools/spawn_agent_tool.rs`,
  `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`.
- **Estimated scope**: L → broken into S2a + S2b (see Tasks).

### S3 — Static-context memory block (end-to-end)
- **slice_id**: `agent/static-context-memory`
- **feature_path**: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`,
  `crates/agent/src/templates.rs`, `crates/agent/src/templates/system_prompt.hbs`
- **Description**: Add `inject_static_context` to `ContextInjector` (default
  returns empty). On first turn (or first `render_system_prompt`), call it,
  cache the result on `Thread` as `static_context: Option<SharedString>`, render
  it in the system prompt after the project context, and include it in
  `system_prompt_digest` (I1). The per-turn `inject_context` path is unchanged.
- **Acceptance criteria**:
  - When a `ContextInjector` is set and returns non-empty static context, the
    rendered system prompt contains the static block exactly once, after the
    project context section.
  - The digest changes when the static context changes (new test
    `test_system_prompt_digest_includes_static_context`); the cache busts.
  - When no `ContextInjector` is set, the system prompt is byte-identical to
    before (I2) — verified by extending `test_system_prompt_digest_stability`.
  - `./script/clippy` clean.
- **Verification**: `cargo test -p agent --features test-support system_prompt` +
  `./script/clippy`.
- **Dependencies**: S1 (both touch `Thread` struct + digest; sequence to avoid
  merge conflicts).
- **Files likely touched**: `crates/agent/src/agent.rs`,
  `crates/agent/src/thread.rs`, `crates/agent/src/templates.rs`,
  `crates/agent/src/templates/system_prompt.hbs`.
- **Estimated scope**: M.

### S4 — Wire `ToolRouter` into `enabled_tools`
- **slice_id**: `agent/tool-router-wiring`
- **feature_path**: `crates/agent/src/thread.rs`, `crates/agent/src/agent.rs`
- **Description**: Add a `static TOOL_ROUTER: OnceLock<Option<Arc<dyn ToolRouter>>>`
  extension point in `agent.rs` (mirroring `CONTEXT_INJECTOR`). In
  `Thread::enabled_tools`, after the profile/feature-flag filter, if a router
  is set, build a `ToolSelectionContext` from the current turn (latest user
  message, open file paths from the project, available tool names) and retain
  only the router-selected tools. When no router is set, all enabled tools pass
  through (I2). The filtered set feeds `render_system_prompt`'s
  `available_tools`, so the digest (I1) reflects it.
- **Acceptance criteria**:
  - With a router set that selects only `grep` and `read_file`, the next
    request's `tools` array contains exactly those two (plus any context-server
    tools the router passes through).
  - With no router set, `enabled_tools` returns the same set as before (I2) —
    verified by an existing test asserting no regression.
  - The system-prompt digest changes when the router selects a different tool
    set (new test `test_digest_reflects_tool_router_selection`).
  - `./script/clippy` clean.
- **Verification**: `cargo test -p agent --features test-support tool_router` +
  `./script/clippy`.
- **Dependencies**: S7 (the trait must exist).
- **Files likely touched**: `crates/agent/src/thread.rs`,
  `crates/agent/src/agent.rs`.
- **Estimated scope**: M.

### S5 — Frontmatter parsing (prompt_store)
- **slice_id**: `prompt_store/rule-frontmatter`
- **feature_path**: `crates/prompt_store/src/prompts.rs`,
  `crates/agent/src/agent.rs`
- **Description**: Add `RuleFrontmatter { globs: Vec<String>, always_apply: bool }`
  and an `Option<RuleFrontmatter>` field on `RulesFileContext`. In
  `load_worktree_rules_file`, parse YAML frontmatter (between `---` fences) at
  the top of the rules file using `serde_yaml` (already a workspace dep via
  minijinja/hkask — confirm) or a minimal hand-rolled parser to avoid a new
  dep. Strip the frontmatter from `text` so the system prompt never sees it.
  When frontmatter is absent, `always_apply` defaults to `true` (I2). No
  scoping happens in this slice — all rules still load unconditionally; this
  slice only adds the parsed metadata and ensures the stripped text renders
  identically to before.
- **Acceptance criteria**:
  - A rules file with frontmatter parses into `RuleFrontmatter` with correct
    `globs` and `always_apply`; the `text` field excludes the frontmatter.
  - A rules file without frontmatter parses with `always_apply: true` and
    `globs: vec![]`; `text` is unchanged.
  - The rendered system prompt for a frontmattered-but-`alwaysApply: true` file
    is byte-identical to the same file without frontmatter (I2) — verified by
    extending `test_system_prompt_renders_user_agents_md_before_project_rules`.
  - `./script/clippy` clean.
- **Verification**: `cargo test -p prompt_store` + `cargo test -p agent
  --features test-support rules_frontmatter` + `./script/clippy`.
- **Dependencies**: None.
- **Files likely touched**: `crates/prompt_store/src/prompts.rs`,
  `crates/agent/src/agent.rs`.
- **Estimated scope**: S.

### S6 — Conditional-rules scoping (end-to-end)
- **slice_id**: `agent/conditional-rules-scoping`
- **feature_path**: `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`
- **Description**: In `build_project_context` (agent.rs L1032), after loading
  `WorktreeContext.rules_file`, filter out conditional rules
  (`always_apply: false` with non-empty `globs`) unless a file matching one of
  the globs is open in the editor or mentioned in the current user message.
  Glob matching uses the `glob` crate (confirm workspace dep) or a minimal
  matcher. The open-files set comes from the project's active editor state;
  the mentioned-paths set is extracted from the latest user message via simple
  path detection. The filtered `ProjectContext` feeds `render_system_prompt`,
  so the digest (I1) automatically reflects which conditional rules are active.
  Add a project-event subscription so opening/closing a matching file refreshes
  `project_context` (mirroring the existing rules-file-change subscription in
  `handle_project_event`).
- **Acceptance criteria**:
  - A conditional rule scoped to `**/*.rs` is included in the system prompt iff
    a `.rs` file is open or a `.rs` path is in the latest user message.
  - Opening a matching file mid-session causes the next `render_system_prompt`
    to include the rule (digest changes); closing it causes the rule to drop
    (digest changes again).
  - `alwaysApply: true` rules are always included (I2).
  - New test `test_conditional_rule_scoped_to_open_file` passes; existing rules
    tests pass; `./script/clippy` clean.
- **Verification**: `cargo test -p agent --features test-support conditional_rules`
  + `./script/clippy`.
- **Dependencies**: S5.
- **Files likely touched**: `crates/agent/src/agent.rs`,
  `crates/agent/src/thread.rs`.
- **Estimated scope**: M.

### S7 — `ToolRouter` trait + heuristic scorer (end-to-end)
- **slice_id**: `agent/tool-router-trait`
- **feature_path**: `crates/agent/src/tool_router.rs` (new),
  `crates/agent/src/tools.rs`
- **Description**: New `crates/agent/src/tool_router.rs` (no `mod.rs` —
  project rule) defining `ToolRouter` (trait), `ToolSelectionContext` (struct),
  and `HeuristicToolRouter` (default impl). The heuristic scores each tool
  0.0–1.0: e.g., `.rs`/`.ts` open ⇒ `grep`/`read_file`/`edit_file`/`diagnostics`
  ≥ 0.5; URL in message ⇒ `fetch`/`web_search` ≥ 0.5; "terminal"/"run" in
  message ⇒ `terminal` ≥ 0.5; otherwise 0.1 baseline. Returns tools scoring
  ≥ 0.30. The trait and heuristic are unit-tested in isolation (no `Thread`
  wiring — that is S4). Register the module in `tools.rs` or `lib.rs` per the
  existing module pattern.
- **Acceptance criteria**:
  - `HeuristicToolRouter::select_tools` returns `grep` and `read_file` when the
    context has an open `.rs` file and no URL.
  - It returns `fetch` and `web_search` when the message contains a URL and no
    open code file.
  - It returns all tools (no filtering) when the context is empty (baseline
    0.1 < 0.30 ⇒ empty selection ⇒ caller treats empty as "no filtering" —
    decide in S4; the trait itself returns the filtered set).
  - New test `test_heuristic_tool_router_scores` passes; `./script/clippy`
    clean.
- **Verification**: `cargo test -p agent --features test-support tool_router` +
  `./script/clippy`.
- **Dependencies**: None (independent of S1–S6; can start in parallel with S5).
- **Files likely touched**: `crates/agent/src/tool_router.rs` (new),
  `crates/agent/src/tools.rs` or `crates/agent/src/lib.rs` (module registration).
- **Estimated scope**: M.

## Tasks (flat list, grouped by phase)

### Phase 1 — Foundation & high-risk plumbing
- [ ] **T1 (S1)**: Add `deferred_tool_results` field + drain logic to `Thread`;
  add `DeferredToolResult` struct; add test
  `test_deferred_tool_result_appears_in_next_request`. (M)
- [ ] **T2a (S2)**: Add `SubagentHandle::send_streaming` trait method with
  default impl delegating to `send`; implement on `NativeSubagentHandle`
  returning a progress stream + final result future. (M)
- [ ] **T2b (S2)**: Rewrite `SpawnAgentTool::run` to return immediate
  placeholder + enqueue `DeferredToolResult`; wire progress via
  `ToolCallEventStream::update_fields`; add test
  `test_non_blocking_subagent_streams_then_delivers`. (M)

**Checkpoint 1**: `./script/clippy` clean; `cargo test -p agent --features
test-support` passes (deferred results + subagent tests); manual smoke test:
spawn two subagents in one parent turn and observe parallel execution.

### Phase 2 — Conditional rules
- [ ] **T3 (S5)**: Add `RuleFrontmatter` + `Option<RuleFrontmatter>` on
  `RulesFileContext`; parse frontmatter in `load_worktree_rules_file`; strip
  from `text`; add parsing tests. (S)
- [ ] **T4 (S6)**: Filter conditional rules in `build_project_context` by
  open-files + mentioned-paths; add project-event subscription for open-file
  changes; add `test_conditional_rule_scoped_to_open_file`. (M)

**Checkpoint 2**: `./script/clippy` clean; `cargo test -p prompt_store` and
`cargo test -p agent --features test-support rules` pass; manual smoke test:
add a `**/*.rs`-scoped rule, open a `.rs` file, confirm rule appears in
prompt; close it, confirm rule disappears (digest changes observable via
telemetry).

### Phase 3 — Static-context memory
- [ ] **T5 (S3)**: Add `inject_static_context` to `ContextInjector` (default
  empty); add `static_context: Option<SharedString>` to `Thread`; call once on
  first turn, cache; add `static_context` field to `SystemPromptTemplate` +
  `.hbs`; include in `system_prompt_digest`; add
  `test_system_prompt_digest_includes_static_context`. (M)

**Checkpoint 3**: `./script/clippy` clean; `cargo test -p agent --features
test-support system_prompt` passes; manual smoke test: set a `ContextInjector`
returning static context, confirm it appears once in the prompt and the digest
changes when it changes.

### Phase 4 — Context-aware tool router
- [ ] **T6 (S7)**: Create `crates/agent/src/tool_router.rs` with
  `ToolRouter` trait, `ToolSelectionContext`, `HeuristicToolRouter`; register
  module; add `test_heuristic_tool_router_scores`. (M)
- [ ] **T7 (S4)**: Add `TOOL_ROUTER` extension point in `agent.rs`; in
  `Thread::enabled_tools`, apply router filter after profile/feature-flag;
  build `ToolSelectionContext` from current turn; add
  `test_digest_reflects_tool_router_selection`. (M)

**Checkpoint 4**: `./script/clippy` clean; `cargo test -p agent --features
test-support tool_router` passes; manual smoke test: with a router set,
confirm the tool set narrows based on open files; with no router, confirm no
regression.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Deferred tool results interact badly with compaction (a deferred result arrives mid-compaction) | High — turn corruption | S1 drains deferred results *after* compaction and *before* `build_completion_request`; compaction never sees a pending deferred result. Test this ordering explicitly. |
| Non-blocking subagent progress events flood the `ToolCallEventStream` | Medium — UI jitter / token waste | Throttle progress updates (e.g., 1 event per N tokens or per 500ms); the existing `update_fields` path is already debounced upstream. |
| Frontmatter parsing introduces a new YAML dep | Medium — build weight | Check workspace for `serde_yaml` (used by hkask-templates/minijinja context). If absent, hand-roll a minimal `---`-fence parser (frontmatter is a flat `globs` list + one bool — ~20 lines). |
| Conditional-rules glob matching is slow on large worktrees | Low — startup latency | Globs are matched only against the open-files set (small) + mentioned paths (small), not the full worktree. Cache compiled globs on `RuleFrontmatter`. |
| Static context loaded once goes stale if the underlying memory changes | Medium — stale prompt | Document that static context is session-scoped; provide a `refresh_static_context` method for future use. The digest catches byte changes if the cache is invalidated. |
| Tool router drops a tool the model needed (false negative) | High — agent can't complete task | Default threshold 0.30 is permissive; heuristic gives every tool a 0.1 baseline so only truly irrelevant tools drop. S4 treats an *empty* router result as "no filtering" (fail-open) to avoid starving the model. |
| Two slices touching `Thread` struct concurrently (S1, S3) cause merge conflicts | Low — rebase friction | Plan sequences S1 → S2 → S3; S3 starts only after S2 merges. |
| Digest change from tool router busts the prefix cache too often | Medium — cost | The router runs once per turn (in `refresh_turn_tools`), so within a turn the tool set is stable. Across turns, cache busts only when the open-file/message context changes — acceptable. |

## Open Questions

1. **`serde_yaml` availability**: Is `serde_yaml` a workspace dependency (via
   hkask-templates or minijinja context), or does adding it to `prompt_store`
   pull a new dep? Check `Cargo.lock` / `Cargo.toml` before T3. If absent,
   hand-roll the minimal frontmatter parser.
2. **`glob` crate availability**: Is the `glob` crate already a workspace dep,
   or should conditional-rules matching use `globset` (likely present via
   git/ignore logic)? Check before T4.
3. **Open-files source**: What is the canonical way to query "files open in the
   editor" from within `build_project_context`? The `Project` entity has a
   worktree store, but the active editor state lives elsewhere (likely
   `workspace::Workspace` or `EditorStore`). Need to find the right entry point
   or pass the open-paths set down from the composition root. This is the
   highest open question for S6.
4. **Deferred result ordering vs. steering messages**: The outer loop already
   handles "steering" queued user messages (`end_turn_at_next_boundary`). Does
   a due deferred result take precedence over a steering message, or vice
   versa? Tentative: steering messages end the turn at the next boundary;
   deferred results are delivered *within* the turn. Confirm in S1.
5. **Static context refresh trigger**: Should static context be re-fetched on
   worktree-add/remove events (like rules), or strictly once per session?
   Tentative: once per session (Kilo Code Memory Bank pattern); revisit if
   staleness is observed.
6. **Tool router + context-server tools**: Should the router score context-server
   (MCP) tools, or only built-in tools? Tentative: router sees only built-in
   tool names; MCP tools always pass through. Confirm in S4.
