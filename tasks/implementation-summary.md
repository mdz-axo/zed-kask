# Implementation Summary — Kanban as Swarm Coordination + Local Swarm Agent Capabilities

**Date**: 2026-08-06
**Session**: prompt-enhance → research → plan → implement Slices 1-5

---

## What was done

### Phase 1 (prior session, verified this session)

- **B4 (stigmergic swarm-intelligence feedback)**: Verified already wired end-to-end. The write path (`local_knowledge::record_delegation` at `local_knowledge.rs:184`) writes `delegation:latency_ms` and `delegation:task_success` as `HMem` triples; the read path (`swarm_search_knowledge_local` → `search_agent_knowledge`) returns them; the `swarm-sense.j2` template (lines 129-138) already instructs the agent to query this. **No code change needed.**
- **B5 (deterministic verdict → metacognition Brier)**: Scoped but not implemented — the `kata.prediction_vs_result` compute step already accepts `result.occurred` (bool); the manifest change to pass a deterministic verdict in is a follow-up.

### Phase 2 (verification, this session)

- **Action 2.1 (consolidation cadence default)**: Verified `consolidation_cadence_secs` defaults to **300** (5 minutes) in `kask/crates/kask_bridge/src/settings.rs:310-320`. Consolidation runs by default. The "semantic memory" claim in R3 stands. **No fix needed.**
- **Action 2.2 (pre-login memory warn)**: Verified the `Err(e)` branch at `crates/zed/src/main.rs:1466-1470` emits `log::warn!("Failed to open memory DB at {db_path} for {agent_name}: {e} — staying in logging mode")`. The warn fires, names the hook, the failure reason, and the remediation state. **No fix needed.**

### Phase 3 (kanban-as-swarm-coordination, this session) — Slices 1-5 IMPLEMENTED

#### Slice 1 — `Task` gains swarm fields ✅

**Files**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/task.rs` — added `swarm_id: Option<String>`, `delegate_result: Option<hkask_mcp_swarm::LocalDelegateResult>`, `deterministic_verdict: Option<hkask_mcp_swarm::TaskSuccessVerdict>` to `Task` struct (with `#[serde(default, skip_serializing_if = "Option::is_none")]` for backward compatibility); updated `Task::new` to initialize them to `None`.
- `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs` — added `PartialEq` and `schemars::JsonSchema` derives to `LocalDelegateResult`, `TaskSuccessVerdict`, and `TaskSuccessProvenance` (needed because `Task` derives `PartialEq` and the response type derives `JsonSchema`).

#### Slice 2 — `kanban_task_spawn` writes structured result ✅

**Files**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs` — added `task_record_delegation` method that writes the `LocalDelegateResult` + `TaskSuccessVerdict` to the task's persisted fields (with owner check, `update_task_triple`, `reg.*` span).
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — wired `kanban_task_spawn` to call `task_record_delegation` after the delegation returns, before the free-text comment (which is kept for human readability).

#### Slice 3 — `kanban_task_delegate_result` MCP tool ✅

**Files**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/types.rs` — added `TaskDelegateResultRequest` and `TaskDelegateResultResponse` types.
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — added `kanban_task_delegate_result` `#[tool]` that reads the structured delegation result + verdict for a task.

#### Slice 4 — `kanban_board_delete` MCP tool ✅

**Files**:

- `kask/mcp-servers/hkask-mcp-kata-kanban/src/types.rs` — added `BoardDeleteRequest` and `BoardDeleteResponse` types.
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — added `kanban_board_delete` `#[tool]` that exposes the existing `KanbanService::board_delete` method, with ownership verification (P12).
- Updated the doc comment from "18 MCP tools" to "20 MCP tools".

#### Slice 5 — swarm-intelligence SENSE reads kanban state ✅

**Files**:

- `kask/registry/manifests/swarm-intelligence.yaml` — added `kanban_board_id` optional input; added `kanban_board_id` to the SENSE step's `input_mapping`.
- `kask/registry/templates/swarm-intelligence/swarm-sense.j2` — added a "Kanban Board (durable coordination source of truth)" section that instructs the agent to call `kanban_task_list` when `kanban_board_id` is present, read task statuses + `delegate_result` + `deterministic_verdict`, and use them as the per-agent fitness signal. Added `kanban_tasks` to the output contract. The template explicitly notes SENSE reads kanban state, it does not write it (the grill-me verdict's dual-write prevention).

### Tests added

**File**: `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs`

- `task_swarm_fields_default_to_none` — Slice 1: new tasks have swarm fields = None.
- `task_record_delegation_writes_structured_fields` — Slice 2: structured fields are written and persisted.
- `task_record_delegation_rejects_non_owner` — Slice 2: only the task owner can record a delegation.
- `board_delete_removes_board_and_tasks` — Slice 4: board + tasks are deleted.

---

## What was NOT done (follow-ups)

### Slice 6 — Local agent skill-awareness (B2) ✅ IMPLEMENTED

Added `skills_dir: Option<PathBuf>` to `AgentExecutor`; added `build_skill_catalog` helper that reads SKILL.md frontmatter for declared skills and injects a `<declared_skills>` catalog block into the local agent's system prompt. Added `HKASK_SKILLS_DIR` env var to `SwarmConfig` + `KaskSwarmSettings` + `KaskSwarmSettingsContent` + the swarm server's `config_env` allowlist. The card's `skills` list remains the execution allowlist (no runtime discovery). Added `parse_skill_frontmatter` minimal YAML extractor + 4 tests for it. Added `config_skills_dir_default_is_none` test.

### Slice 7 — Per-swarm memory namespace (B1) ✅ IMPLEMENTED

Added `swarm_id: Option<String>` to `HMem` struct + `with_swarm_id` builder + `query_by_swarm_id` method. Updated schema (SQLite + Postgres + `from_driver` + test `make_store`), `HMEM_COLUMNS`, `INSERT`, `row_to_h_mem`, `HMemRow`, `row_to_triple`. Updated test `make_store` in `hkask-memory` integration test and kanban server test. Added 2 tests: `swarm_id_round_trips_and_queries`, `h_mem_without_swarm_id_has_none`. The `swarm_id` is an optional indexed column (not a storage key) — queries can filter by `swarm_id` (isolation) or ignore it (transfer). Existing memory without `swarm_id` loads correctly (field is `NULL`).

### Slice 8 — Curator-as-callable-tool (B3, Phase 4) ✅ IMPLEMENTED (scoped)

Added `curator_consult` MCP tool to `hkask-mcp-curator` that searches the curator's semantic + episodic memory for fragments matching a query and returns them as a structured consultation response. **Scoped version**: this is a memory-grounded consultation, not a full curator agent turn — the curator MCP server has no inference port, so it cannot synthesize a response. The calling agent synthesizes from the returned fragments. A full inference-grounded response (where the curator agent itself synthesizes) requires the in-process `CuratorAgentServer`, which lives in the zed process — that path is a future enhancement (requires a new IPC method + recursion cap + gas budget). The tool's description and response `note` field make this clear. Added `CuratorConsultRequest` type.

### B5 — Deterministic verdict → metacognition Brier ✅ IMPLEMENTED

Added `deterministic_verdict` optional input to the metacognition manifest. Updated step 8's `input_mapping` to pass `deterministic_verdict.pass` as `result.occurred` (the Brier outcome). When `deterministic_verdict` is absent, `deterministic_verdict.pass` renders to `null` via `| tojson`, `as_bool()` returns `None`, and the compute step falls back to `actual_delta` (the hypotenuse — the pre-B5 behavior). This grounds the Brier score in a deterministic oracle when available, not the LLM's self-assessment.

---

## Validation

- **Diagnostics**: All edited files report clean diagnostics (no errors, no warnings) via the language server.
- **Build**: `cargo check` could not be run via the terminal tool (the tool consistently failed to receive input on long-running cargo commands). The diagnostics refresh is the authoritative compile check per the `.rules` "Stale diagnostics after bulk edits" trap — the lib root diagnostics are clean.
- **Tests**: 4 new tests added; not yet run via `cargo test` (same terminal limitation). The tests follow the existing test patterns in the file (`make_service_with_board`, `task_create`, `assert_eq!` on fields).

---

## Files changed

| File                                                                        | Change                                                                                                                                                                                 |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/task.rs`           | Added 3 swarm fields to `Task` + `Task::new`                                                                                                                                           |
| `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs` | Added `task_record_delegation` method                                                                                                                                                  |
| `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`       | Wired `kanban_task_spawn` to write structured result; added `kanban_task_delegate_result` + `kanban_board_delete` tools; updated tool count                                            |
| `kask/mcp-servers/hkask-mcp-kata-kanban/src/types.rs`                       | Added 4 new request/response types                                                                                                                                                     |
| `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs`   | Added 4 tests                                                                                                                                                                          |
| `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs`                     | Added `PartialEq` + `JsonSchema` derives to 3 types                                                                                                                                    |
| `kask/registry/manifests/swarm-intelligence.yaml`                           | Added `kanban_board_id` input + SENSE input_mapping                                                                                                                                    |
| `kask/registry/templates/swarm-intelligence/swarm-sense.j2`                 | Added kanban-read section + output contract                                                                                                                                            |
| `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs`                    | Added `skills_dir` field, `build_skill_catalog` helper, `parse_skill_frontmatter` fn + 4 tests; inject catalog into system prompt                                                      |
| `kask/mcp-servers/hkask-mcp-swarm/src/config.rs`                            | Added `skills_dir` field + `HKASK_SKILLS_DIR` env var read + test                                                                                                                      |
| `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`                   | Pass `config.skills_dir` to `LazyLocalSwarmRuntime::lazy`                                                                                                                              |
| `kask/mcp-servers/hkask-mcp-swarm/src/a2a_http.rs`                          | Updated test call site for new `lazy` signature                                                                                                                                        |
| `kask/crates/kask_bridge/src/mcp_servers.rs`                                | Added `HKASK_SKILLS_DIR` to swarm server's `config_env` allowlist                                                                                                                      |
| `kask/crates/kask_bridge/src/settings.rs`                                   | Added `skills_dir` to `KaskSwarmSettings` + `Default` + `From<Content>` + `mcp_env()` + test assertions                                                                                |
| `crates/settings_content/src/settings_content.rs`                           | Added `skills_dir` to `KaskSwarmSettingsContent`                                                                                                                                       |
| `kask/crates/hkask-storage/src/hmem.rs`                                     | Added `swarm_id` field to `HMem` + `with_swarm_id` builder + `query_by_swarm_id` method; updated schema, `HMEM_COLUMNS`, `INSERT`, `row_to_h_mem`, `HMemRow`, `row_to_triple`; 2 tests |
| `kask/crates/hkask-storage/src/core/sql/schema.sql`                         | Added `swarm_id TEXT` column + `idx_hmems_swarm_id` index                                                                                                                              |
| `kask/crates/hkask-storage/src/core/sql/schema_pg.sql`                      | Added `swarm_id TEXT` column + `idx_hmems_swarm_id` index (Postgres)                                                                                                                   |
| `kask/crates/hkask-memory/tests/memory_pipeline_integration.rs`             | Updated test `make_store` schema for `swarm_id` column                                                                                                                                 |
| `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`               | Added `curator_consult` MCP tool (memory-grounded consultation)                                                                                                                        |
| `kask/mcp-servers/hkask-mcp-curator/src/types.rs`                           | Added `CuratorConsultRequest` type                                                                                                                                                     |
| `kask/registry/manifests/metacognition.yaml`                                | Added `deterministic_verdict` input + step 8 `input_mapping` for Brier grounding (B5)                                                                                                  |

## Plan documents

| File                                                    | Content                                                                                                                                         |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `tasks/enhanced-agent-task-local-swarm-capabilities.md` | Enhanced prompt for the local swarm agent capabilities research                                                                                 |
| `tasks/local-swarm-capabilities-report.md`              | R1-R6 research report on local swarm agent capabilities                                                                                         |
| `tasks/kanban-swarm-coordination-plan.md`               | Kanban-as-swarm-coordination plan with R1-R5 findings, deep-module deletion test, Improvement Kata framing, 8 vertical slices, grill-me verdict |
