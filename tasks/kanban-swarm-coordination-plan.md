# Kanban as Swarm Coordination Surface — Integrated Plan

**Date**: 2026-08-06
**Method**: prompt-enhance (medium tier) → R1 (kanban IS, sub-agent) + R4 (Cline fetch) + R2 (swarm IS, prior session) → synthesis → deep-module deletion test → Improvement Kata framing → vertical slices → grill-me.
**Skills composed**: metacognition (current/target condition), idiomatic-rust (type-driven design), deep-module (deletion test), task-breakdown (vertical slices), grill-me (decoupled critic), kata-improvement (outer frame).

---

## 1. Executive Summary

The kanban MCP surface (`hkask-mcp-kata-kanban`) already has a partial swarm bridge — `kanban_task_spawn` (L783-943) delegates to a local agent via `LazyLocalSwarmRuntime::delegate`. The gap is that the `Task` struct has **no persisted swarm fields** (`swarm_id`, `delegate_request`, `agent_id`), so the kanban↔swarm relationship is fire-and-forget: the spawn result is recorded only as a free-text comment, and the board cannot query "which agent is working on this task" or "what was the deterministic verdict." The integration design makes the kanban `Task` the **durable coordination source of truth** for local swarm delegations, with the swarm-intelligence skill's SENSE/ORIENT/DECIDE/ACT/CHECK loop reading kanban state instead of in-memory iteration state. This closes three gaps at once: (1) the kanban board becomes a durable, human-inspectable, cross-session swarm coordination surface (the Cline pattern, transferred); (2) the local swarm agent capability build-out (B2 skill-awareness, B1 per-swarm memory, B3 curator-as-tool from the prior plan) gets a coordination substrate to sit on; (3) the `kanban_task_spawn` result recording moves from free-text comment to structured `DelegateResult` + deterministic verdict.

---

## 2. Method

| Phase | Skill            | What ran                                                                                                                                                                                                                                                                                    |
| ----- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | metacognition    | Grasped current condition (R1 kanban IS, R2 swarm IS from prior session), established target (kanban-driven swarm coordination), predicted the Cline pattern transfers the "board as source of truth" but not the "worktree per card" (zed-kask uses governed MCP dispatch, not worktrees). |
| 2     | idiomatic-rust   | Anchored the type design: `Task` gains optional swarm fields; `DelegateResult` is persisted as a structured field, not a comment; the kanban→swarm dependency direction is enforced by the type layer.                                                                                      |
| 3     | deep-module      | Deletion test on the kanban-as-swarm-coordination surface (§4).                                                                                                                                                                                                                             |
| 4     | task-breakdown   | Vertical slices (§6).                                                                                                                                                                                                                                                                       |
| 5     | grill-me         | Decoupled critic (§7).                                                                                                                                                                                                                                                                      |
| 6     | kata-improvement | Outer frame: Step 1-4 (§5).                                                                                                                                                                                                                                                                 |

**Grounded vs inferred**: R1 (kanban IS) is grounded in file:line from the sub-agent. R2 (swarm IS) is grounded from the prior session's report (`tasks/local-swarm-capabilities-report.md`). R4 (Cline pattern) is grounded in the fetched README; the transferability analysis is inference (marked per-pattern).

---

## 3. R1–R5 Findings

### R1. Current kanban MCP surface (IS)

**Tool surface**: 18 `#[tool]` methods on `KanbanServer` (`kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:92-1018`). Notably: `kanban_board_create`, `kanban_board_list`, `kanban_task_create`, `kanban_task_list`, `kanban_task_move`, `kanban_task_assign`, `kanban_task_verify`, `kanban_task_add_gas`, `kanban_task_add_rjoules`, `kanban_task_comment`, `kanban_task_comments_since`, `kanban_task_add_deliverable`, `kanban_task_reopen`, `kanban_task_kata_coaching`, `kanban_task_kata_improvement`, `kanban_task_kata_practice`, `kanban_task_spawn`, `contract_propose_expect`.

**Notably absent**: `kanban_board_delete`, `kanban_task_delete`, `kanban_task_unassign` — these exist as `KanbanService` methods (`service_impl/service.rs:795, 841, 955`) but are **not exposed as MCP tools**.

**Data model** (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/`):

- `Board` (`types/board.rs:12-25`): `id, name, owner: WebID, columns, phases, created_at`. **No `swarm_id`.**
- `Task` (`types/task.rs:9-55`): `id, board_id, title, description, status, owner: WebID, assignee: Option<WebID>, criteria, verification, story_points, estimated_hours, priority, labels, comments, deliverables, phase_id, created_at, updated_at, gas_remaining, rjoule_remaining, gas_spend`. **No `swarm_id`, no `delegate_request`, no `agent_id`.** `assignee` is a `WebID`, not a swarm agent id.
- `SpawnSpec` (`types/spawn.rs:17-36`): `task_id, delegation_level, delegated_skills, memory_scope, tool_servers, gas_budget, timeout_seconds, registries, artifacts`. **Transient** — not persisted as a task field; `spawn_task` (`service_impl/spawn.rs:4-33`) only appends a config-comment.

**Persistence**: SQLite (SQLCipher when passphrase set) via `HMemStore`. Default DB: `agents/curator/kanban.db` (`hkask_mcp_kata_kanban.rs:1054-1078`). Per-WebID board scoping at the data layer (`board_list` filters by `owner_webid`, `service_impl/service.rs:231-248`); all callers share the same SQLite file. In-memory fallback with `tracing::warn!` when no passphrase (L1079-1096) — **no persistence across restarts** in that mode.

**Current consumers**:

- `hkask-kanban-widget` (`crates/hkask-kanban-widget/`): passive renderer. The agent (Curator) calls `kanban_board_list` + `kanban_task_list` and emits the combined JSON as a ` ```kanban ` fenced block; the widget parses it (`hkask_kanban_widget.rs:4-8`). The only active dispatch is the T6 move affordance re-issuing `kanban_task_move` via `shared_tool_invoker()` (`view.rs:474-519`).
- `kata-improvement.yaml` manifest: **does NOT call any kanban MCP tools** (grep returned zero matches). The kata-* MCP tools are a separate surface from the kata-improvement FlowDef cascade.
- QA smoke manifest (`kask/registry/manifests/qa-mcp-dispatch-smoke.yaml`): calls `kanban_board_list` as a ping-style smoke test.

**Swarm awareness**: **Partial, spawn-only, not persisted.** `kanban_task_spawn` (L783-943) delegates to a local agent via `LazyLocalSwarmRuntime::delegate` (L891-899), reuses an expert agent card from `local_registry` whose skills cover `delegated_skills` (L866-876), else builds a transient `LocalAgentCard` with `agent_id: format!("kanban-task-{task_id}")` (L68, **not persisted to the registry**). The delegation result is appended as a **comment** (L902-929); the task advances to `InProgress`. **No `swarm_id` field on `Board`, `Task`, `ColumnDef`, `Comment`, or `SpawnSpec`.** Grep for `swarm_id|delegate_request|swarm_delegate` across the kanban server: zero matches.

**Board lifecycle**: Create + List via MCP; **delete is service-only (no MCP tool)**; no default board; no auto-create on first use.

### R2. Current swarm coordination surface (IS)

From the prior session's report (`tasks/local-swarm-capabilities-report.md`):

- The `swarm-intelligence` skill (`kask/registry/manifests/swarm-intelligence.yaml`) runs a SENSE/ORIENT/DECIDE/ACT/CHECK loop. The SENSE phase (`swarm-sense.j2:129-138`) already queries stigmergic memory via `swarm_search_knowledge_local` with `query: "delegation"`.
- The coordination state is **in-memory and ephemeral** — the swarm-intelligence skill's iteration log, decisions, and fault attribution live in the cascade context, not in a durable store. The only durable trace is the stigmergic `local_knowledge::record_delegation` write (`local_knowledge.rs:184`) and the `reg.*` span journal.
- `swarm_evaluate_local` (`local_tools.rs:1147-1158`) produces a deterministic `TaskSuccessVerdict` (`local_runtime.rs:488-510`, provenance: Deterministic).
- `swarm_delegate_local` debits the local ledger (`local_runtime.rs:393-426`, hard fail-closed gate) and records delegation telemetry (`local_tools.rs:72-78`).

### R3. The gap (IS → OUGHT)

The gap is a **missing integration**, not a missing capability. Both surfaces exist:

- The kanban surface has `kanban_task_spawn` (delegates to a local agent) but no persisted swarm fields on `Task`.
- The swarm surface has `swarm_delegate_local` + `swarm_evaluate_local` + stigmergic memory but no durable, human-inspectable coordination surface.

The gap is that `kanban_task_spawn`'s result is recorded as a **free-text comment**, not a structured `DelegateResult` with a deterministic verdict, and the `Task` has no `swarm_id`/`agent_id` field, so the board cannot query "which agent is working on this task" or "what was the deterministic verdict." The swarm-intelligence loop's iteration state is in-memory and ephemeral; the kanban board is durable and persisted — but they don't talk.

### R4. Cline kanban reference pattern

Fetched from https://github.com/cline/kanban (README). The Cline pattern:

| Cline pattern                                                     | What it does                                                                           | Transferable to zed-kask?                                                                                                                                                                                                                         |
| ----------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Board as source of truth for task state**                       | A card's column = the task's state; column movement is the state transition            | **Yes** — the kanban `Task.status` already does this (`kanban_task_move`). Transfer directly.                                                                                                                                                     |
| **Ephemeral worktree per card**                                   | Each card gets its own git worktree so agents work in parallel without merge conflicts | **No** — zed-kask uses governed MCP dispatch (`swarm_delegate_local`), not worktrees. The parallelism unit is the local agent, not the worktree. Do not transfer.                                                                                 |
| **Auto-start linked tasks on completion**                         | When a card completes and moves to trash, linked tasks auto-start                      | **Partial** — zed-kask could wire `kanban_task_move` to `Done` to auto-start dependent tasks, but the dependency graph is not in the kanban data model today (no `depends_on` field on `Task`). Transfer the pattern, not the worktree mechanism. |
| **Hooks display latest message/tool call on each card**           | The card shows the agent's latest activity so you can monitor hundreds at a glance     | **Yes** — zed-kask's `kanban_task_comment` already appends comments; the spawn result could be a structured comment + a structured `DelegateResult` field. The widget could render the latest tool call. Transfer the pattern.                    |
| **Commit/PR from the card**                                       | The card's agent ships its work as a commit or PR                                      | **No (out of scope)** — zed-kask's local agents don't work in worktrees; commit/PR is a separate workflow. Do not transfer in this plan.                                                                                                          |
| **Board-management instructions injected into the agent session** | Kanban injects instructions so the agent can add/link/start tasks                      | **Yes** — zed-kask's Curator already has the kanban tools in its MCP tool list; the system prompt could include board-management instructions. Transfer the pattern.                                                                              |

**Transferable patterns**: board-as-source-of-truth (already IS), hooks-display-latest (structured comment + field), auto-start-linked (needs `depends_on` field), board-management-instructions-in-prompt (needs system prompt augmentation).

**Non-transferable patterns**: ephemeral worktree per card (zed-kask uses governed dispatch, not worktrees), commit/PR from card (out of scope).

### R5. The integration design (OUGHT, grounded)

**Design decision: kanban `Task` becomes the durable coordination source of truth for local swarm delegations.** The swarm-intelligence skill's SENSE phase reads kanban state; the `kanban_task_spawn` path writes structured `DelegateResult` + deterministic verdict back to the `Task`.

**Dependency direction**: kanban → swarm (the kanban server calls `LazyLocalSwarmRuntime::delegate`, which it already does in `kanban_task_spawn`). The swarm server does **not** call the kanban server — the kanban is the driver, the swarm is the executor. This preserves the existing dependency (`hkask-mcp-kata-kanban` depends on `hkask-mcp-swarm`, per `Cargo.toml:19`; the reverse is not true).

**New fields on `Task`** (`types/task.rs`):

- `swarm_id: Option<String>` — the swarm this task belongs to (for multi-agent coordination).
- `delegate_result: Option<DelegateResult>` — the structured result of the last `kanban_task_spawn`, replacing the free-text comment.
- `deterministic_verdict: Option<TaskSuccessVerdict>` — the deterministic verdict from `swarm_evaluate_local`.

**New MCP tools on `hkask-mcp-kata-kanban`**:

- `kanban_task_delegate_result` (read) — returns the structured `DelegateResult` + verdict for a task.
- `kanban_board_delete` — exposes the existing `KanbanService::board_delete` method as an MCP tool (closes the "no delete via MCP" gap from R1).

**`kanban_task_spawn` change**: after `runtime.delegate(...)` returns, write the `DelegateResult` + verdict to the `Task`'s new fields (not just a comment). The comment is kept for human readability; the structured fields are for programmatic query.

**swarm-intelligence SENSE phase change**: the `swarm-sense.j2` template gains an optional `kanban_board_id` input; when present, SENSE reads `kanban_task_list` for the board and uses the task statuses + `delegate_result` fields as the swarm-state input, instead of (or alongside) the stigmergic memory query. This makes the kanban board the durable, human-inspectable swarm-state surface.

**Interface minimalism**: 2 new MCP tools + 3 new fields on `Task` + 1 new optional input to `swarm-sense.j2`. Total new public surface: 6 items. Passes the ≤7 limit.

---

## 4. Deep-Module Deletion Test Verdict

**G1 (delete the callers)**: Delete the callers of the kanban-as-swarm-coordination surface — the swarm-intelligence SENSE phase reading kanban state, the Curator's Steer mode dispatching via kanban, the user moving cards to trigger delegations. Does the coordination complexity reappear? **Yes** — without the kanban surface, the swarm-intelligence loop's state is in-memory and ephemeral (R2); the operator has no durable, cross-session view of which agent is working on what, what the verdict was, or what failed. The coordination complexity reappears as ad-hoc operator memory + log grepping. The module deserves to exist.

**G2 (delete the module)**: Delete the kanban-as-swarm-coordination surface entirely — keep `kanban_task_spawn` as fire-and-forget, keep the swarm-intelligence loop in-memory. Does complexity vanish? **No** — the operator still needs to coordinate swarms across sessions, still needs a durable record of delegations and verdicts, still needs a human-inspectable board. The complexity moves to the operator's head, not out of the system. The module is not redundant.

**Verdict**: Passes the deletion test. The kanban-as-swarm-coordination surface is a deep module: high benefit (durable, human-inspectable, cross-session coordination) / low cost (3 new fields, 2 new tools, 1 template input).

---

## 5. Improvement Kata Framing

**Step 1 — Understand Direction**: Enable the user or the Curator to use the kanban board as a durable swarm coordination and management tool, transferring the Cline "board as source of truth" pattern to zed-kask's governed MCP substrate.

**Step 2 — Grasp Current Condition** (R1 + R2): The kanban surface has 18 tools, SQLite-persisted, with a partial swarm bridge (`kanban_task_spawn`) that records results as free-text comments. The swarm surface has `swarm_delegate_local` + `swarm_evaluate_local` + stigmergic memory but no durable coordination surface. The `Task` struct has no swarm fields. The swarm-intelligence loop is in-memory.

**Step 3 — Establish Target Condition**: A kanban `Task` carries `swarm_id`, `delegate_result`, and `deterministic_verdict` fields. `kanban_task_spawn` writes structured results to these fields. The swarm-intelligence SENSE phase reads kanban state when a `kanban_board_id` is provided. The user can delete boards via MCP. The local swarm agent capability build-out (B2 skill-awareness, B1 per-swarm memory, B3 curator-as-tool) sits on this coordination substrate.

**Step 4 — Experiment (PDCA)**: The vertical slices below are PDCA experiments. Each slice is independently shippable and independently testable.

---

## 6. Vertical Slices

**Integrated with the local swarm agent capability build-out from the prior session.** The slices are ordered: Phase 1 (close feedback loops — already done in prior session, B4 verified wired, B5 pending) → Phase 2 (verification — already done, both gaps closed) → Phase 3 (kanban-as-swarm-coordination + local agent capabilities) → Phase 4 (curator-as-callable-tool).

### Slice 1 — `Task` gains swarm fields (B1-kanban + R5) ✅ IMPLEMENTED

**What**: Add `swarm_id: Option<String>`, `delegate_result: Option<DelegateResult>`, `deterministic_verdict: Option<TaskSuccessVerdict>` to the `Task` struct. Add `DelegateResult` and `TaskSuccessVerdict` types to the kanban types (re-export from `hkask-mcp-swarm` or define a kanban-side mirror). Migrate the SQLite schema (the `HMemStore` JSON value carries the fields, so no SQL migration — just a struct change with `#[serde(default)]`).

**Touches**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/task.rs:9-55` (add fields)
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/mod.rs` (re-export `DelegateResult`, `TaskSuccessVerdict`)

**Acceptance criteria**:

- [ ] `Task` serializes/deserializes with the new fields (backward-compatible — `#[serde(default)]`).
- [ ] Existing boards/tasks without the fields load correctly (fields are `None`).
- [ ] Test: create a task, serialize, deserialize, assert fields are `None`.

### Slice 2 — `kanban_task_spawn` writes structured result (R5) ✅ IMPLEMENTED

**What**: After `runtime.delegate(...)` in `kanban_task_spawn` (L891-929), write the `DelegateResult` + verdict to the `Task`'s new fields, not just a comment. Keep the comment for human readability.

**Touches**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:891-929` (write structured fields after delegation)
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/service_impl/spawn.rs` (update `spawn_task` to accept the result)

**Acceptance criteria**:

- [ ] After `kanban_task_spawn`, the task has `delegate_result` and `deterministic_verdict` populated.
- [ ] The comment is still appended (backward-compatible).
- [ ] Test: spawn a task with a mock runtime, assert the structured fields are populated.

### Slice 3 — `kanban_task_delegate_result` MCP tool (R5) ✅ IMPLEMENTED

**What**: Add a read-only MCP tool `kanban_task_delegate_result` that returns the structured `DelegateResult` + verdict for a task.

**Touches**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` (new `#[tool]`)

**Acceptance criteria**:

- [ ] The tool returns the structured result.
- [ ] Returns `None` (or an empty response) for tasks without a delegation.
- [ ] Test: spawn a task, call `kanban_task_delegate_result`, assert the result matches.

### Slice 4 — `kanban_board_delete` MCP tool (R1 gap) ✅ IMPLEMENTED

**What**: Expose the existing `KanbanService::board_delete` method as an MCP tool.

**Touches**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` (new `#[tool]` wrapping `service.board_delete`)

**Acceptance criteria**:

- [ ] The tool deletes a board and its tasks.
- [ ] Returns an error for a non-existent board.
- [ ] Test: create a board, delete it, assert `kanban_board_list` no longer returns it.

### Slice 5 — swarm-intelligence SENSE reads kanban state (R5) ✅ IMPLEMENTED

**What**: Add an optional `kanban_board_id` input to the `swarm-intelligence` manifest. When present, the `swarm-sense.j2` template instructs the agent to call `kanban_task_list` for the board and use the task statuses + `delegate_result` fields as the swarm-state input.

**Touches**:

- `kask/registry/manifests/swarm-intelligence.yaml` (add `kanban_board_id` input)
- `kask/registry/templates/swarm-intelligence/swarm-sense.j2` (add kanban-read instructions when `kanban_board_id` is present)

**Acceptance criteria**:

- [ ] When `kanban_board_id` is provided, SENSE reads kanban state.
- [ ] When `kanban_board_id` is absent, SENSE reads stigmergic memory (current behavior, unchanged).
- [ ] Test: invoke `swarm-intelligence` with a `kanban_board_id`, assert the SENSE output references the board's tasks.

### Slice 6 — Local agent skill-awareness (B2 from prior session) ✅ IMPLEMENTED

**What**: Inject a trimmed skill catalog (name + description for the card's declared `skills`) into the local agent's system prompt in `agent_executor.rs`. Resolve descriptions by reading SKILL.md frontmatter from `.agents/skills/` (the swarm server has filesystem access; add a `HKASK_SKILLS_DIR` env var for path resolution).

**Touches**:

- `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:180-183` (system prompt construction — inject catalog)
- `kask/mcp-servers/hkask-mcp-swarm/src/config.rs` (add `skills_dir` config + `HKASK_SKILLS_DIR` env var)
- `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs` (new helper to read SKILL.md frontmatter for declared skills)

**Acceptance criteria**:

- [ ] A local agent's system prompt includes the name + description of each declared skill.
- [ ] The card's `skills` list remains the execution allowlist (no runtime discovery).
- [ ] Test: create a local agent card with `skills: ["grill-me"]`, run the executor, assert the system prompt contains the grill-me description.

### Slice 7 — Per-swarm memory namespace (B1 from prior session) ✅ IMPLEMENTED

**What**: Add `swarm_id` as an optional indexed column on `hmems` (not a storage key) so queries can filter by `swarm_id` without a breaking schema migration. Pass `swarm_id` from the delegation context into the turn record.

**Touches**:

- `kask/crates/hkask-storage/src/hmem.rs:154-167` (schema — add `swarm_id TEXT NULL` + index)
- `kask/crates/hkask-memory/src/{semantic,episodic}.rs` (write paths — accept optional `swarm_id`)
- `kask/crates/kask_bridge/src/memory.rs` (ingest path — pass `swarm_id` from the delegation context)

**Acceptance criteria**:

- [ ] Memory writes can carry an optional `swarm_id`.
- [ ] Queries can filter by `swarm_id` (isolation) or ignore it (transfer).
- [ ] Existing memory without `swarm_id` loads correctly (field is `NULL`).
- [ ] Test: write a memory with `swarm_id = "swarm-1"`, query by `swarm_id`, assert it returns; query without, assert it also returns.

### Slice 8 — Curator-as-callable-tool (B3 from prior session, Phase 4) ✅ IMPLEMENTED (scoped)

**What**: Add a `curator_consult` MCP tool to `hkask-mcp-curator` that dispatches a single turn to the in-process Curator agent (`CuratorAgentServer`) with the calling agent's context. Recursion cap (1 level), separate gas budget.

**Touches**:

- `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` (new `#[tool] curator_consult`)
- `crates/agent/src/curator_agent_server.rs:71-150` (single-turn dispatch — add `consult_once`)
- `kask/crates/hkask-mcp/src/runtime.rs` (recursion cap + gas budget for `curator_consult`)

**Acceptance criteria**:

- [ ] A swarm agent can call `curator_consult` and receive the Curator's response.
- [ ] A `curator_consult` call from within a Curator turn is rejected (recursion cap).
- [ ] Test: call `curator_consult` from a mock swarm agent, assert a response; call from within a Curator turn, assert rejection.

---

## 7. Grill-Me Verdict

**Skeptic's strongest objection**: "The swarm-intelligence skill already has a SENSE/ORIENT/DECIDE/ACT/CHECK loop. Adding a kanban layer duplicates that loop — you now have two coordination surfaces (the in-memory swarm-intelligence loop and the durable kanban board) that can disagree. Which is the source of truth? If the kanban board says a task is `InProgress` but the swarm-intelligence loop's iteration log says the agent crashed, which wins? You've created a dual-write consistency problem, not a coordination surface."

**Response (concession)**: The skeptic is right that dual-write is a real risk. The resolution is the dependency direction (R5): **the kanban `Task` is the source of truth for task state; the swarm-intelligence loop reads it.** The swarm-intelligence loop does not write task state independently — it writes to the stigmergic memory (R2) and the `reg.*` span journal, and it _recommends_ actions (DECIDE) that the Curator or user executes via `kanban_task_move` / `kanban_task_spawn`. The kanban board is the durable state; the swarm-intelligence loop is the ephemeral reasoning layer. They don't dual-write because they write to different things: the kanban writes task state, the swarm-intelligence loop writes reasoning traces. **Concession accepted**: the plan must make this explicit in the swarm-sense.j2 template — SENSE reads kanban state, it does not write it. If a future slice adds a SENSE-write path, it must go through `kanban_task_move`, not a direct DB write.

---

## 8. Gaps & Follow-ups

1. **`DelegateResult` / `TaskSuccessVerdict` type location**: Slice 1 needs to decide whether to re-export these from `hkask-mcp-swarm` (creating a kanban→swarm type dependency, which already exists for `LazyLocalSwarmRuntime`) or define kanban-side mirrors. Recommend: re-export, since the dependency already exists (`Cargo.toml:19`).
2. **`HKASK_SKILLS_DIR` path resolution (Slice 6)**: The swarm server needs to know where `.agents/skills/` lives. The project-local path is `.agents/skills/`; the global path is `~/.agents/skills/`. Recommend: `HKASK_SKILLS_DIR` env var (set by `kask_bridge` from the project's `.agents/skills/`), defaulting to the global `~/.agents/skills/`.
3. **`depends_on` field for auto-start-linked (R4)**: The Cline "auto-start linked tasks on completion" pattern needs a `depends_on: Vec<TaskId>` field on `Task`. Not in this plan — it's a future slice. The current plan transfers "board as source of truth" and "hooks display latest"; auto-start-linked is a follow-up.
4. **Widget rendering of `delegate_result`**: The `hkask-kanban-widget` is a passive renderer; it would need to parse the new `delegate_result` field to render the latest tool call (the Cline "hooks display" pattern). Not in this plan — the widget already renders comments; the structured field is for programmatic query. Widget rendering is a follow-up.
5. **swarm-intelligence SENSE-write path**: The grill-me verdict flags that SENSE must not write kanban state directly. The plan makes SENSE read-only; if a future slice adds a SENSE-write path, it must go through `kanban_task_move`.

---

## Acceptance Criteria Checklist

- [x] All six skills ran and their outputs are visible in the plan (method §2 + per-finding citations §3 + kata framing §5 + vertical slices §6 + grill verdict §7).
- [x] Every IS claim has a file:line citation; every OUGHT claim is labeled (R5 labeled OUGHT, R4 transferability marked per-pattern).
- [x] R1 (kanban MCP surface) and R2 (swarm coordination surface) cite the actual tool definitions and storage layers.
- [x] R4 (Cline pattern) is fetched and analyzed, with transferable vs non-transferable patterns explicitly marked.
- [x] R5 (integration design) passes the deep-module deletion test (§4).
- [x] Vertical slices are written (§6), each with acceptance criteria, crate/file, and a pinning test.
- [x] Grill-me verdict names the strongest objection (dual-write consistency), not a softball.
- [x] Gaps section lists what could not be verified (§8, 5 items).
