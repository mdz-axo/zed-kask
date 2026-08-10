# P1: Worktree/Terminal Model for `kanban_task_spawn`

**Status:** DECISION REQUIRED — three options documented below with trade-offs.
**Date:** 2026-08-09
**Scope:** `kask/mcp-servers/hkask-mcp-kata-kanban/` + the kanban widget's spawn surface.

## Problem

`kanban_task_spawn` delegates task execution to a local swarm agent via
`LazyLocalSwarmRuntime::delegate()`. The spawned agent runs **in-memory in the
MCP server process** — no git worktree isolation, no terminal session, no
filesystem boundary. The agent operates on the same working tree as the user.

The old system (deleted in `de6abf453f` — "fold service crates into MCP server
binaries") had:

1. **`/kanban spawn <task>`** REPL command (`kask/crates/hkask-repl/src/handlers/kanban.rs`)
   that spawned a userpod (separate process) to execute a task.
2. **Terminal WebSocket route** (`kask/crates/hkask-api/src/routes/terminal.rs`)
   that gave each WebID its own browser-based terminal session (`kask repl
--webid <webid>` with piped stdio).
3. **`SpawnSpec` with `capability_tokens`** (OCAP token specs like
   `"tool:kanban:execute"`) for capability-scoped delegation — removed as dead
   theater (`.rules` "Manifest `ocap:` is declared config, not a security gate").

The current `SpawnSpec` (`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/spawn.rs`)
retains `delegation_level`, `delegated_skills`, `memory_scope`, `tool_servers`,
`gas_budget`, `timeout_seconds`, `registries`, `artifacts` — but no worktree or
terminal fields.

## Current spawn path

```
kanban_task_spawn (MCP tool)
  → SpawnSpec::new(tid).with_level(...).with_skills(...)
  → KanbanService::spawn_task(tid, spec, webid)  // records spec on task
  → LazyLocalSwarmRuntime::get_or_init()
  → runtime.delegate(&agent, &task_text, credits, ceiling)
    → in-memory agent execution (ledger-funded inference + guard + skill cascade)
  → KanbanService::task_record_delegation(tid, result, verdict, webid)
  → KanbanService::task_comment(tid, webid, result_note)
  → KanbanService::task_move(tid, InProgress, webid)
```

The agent runs in the same process, same filesystem, same working tree. There is
no isolation boundary between the spawned agent's file mutations and the user's
working tree.

## Options

### Option A: Editor-side worktree (via `create_thread_tool`)

**How:** The `kanban_task_spawn` MCP tool calls back into the editor via the
`ToolPort` dispatch path to invoke `create_thread` with `use_new_worktree: true`.
The spawned agent thread runs in a new git worktree, isolated from the user's
working tree.

**Pros:**

- Reuses the existing `create_thread_tool` worktree infrastructure
  (`git_ui_core::worktree_service::create_worktree_workspace`).
- The spawned agent has a full editor context (terminal, file panel, etc.).
- The user can see the spawned agent's work in a separate tab.

**Cons:**

- Crosses the MCP↔editor boundary: the MCP server would need a new port to call
  `create_thread` (currently `ToolPort` dispatches MCP tools, not editor actions).
- The MCP server is process-global (app-level), but `create_thread` is
  workspace-scoped — the MCP server doesn't know which workspace to spawn in.
- Latency: a round-trip through the editor for each spawn.
- The `LazyLocalSwarmRuntime` path (in-memory agent) would be bypassed — the
  spawn would go through the editor's agent thread system instead.

**Complexity:** High. Requires a new `WorktreeSpawnPort` in `kask_bridge` +
wiring in `main.rs` + the MCP server calling it.

### Option B: Headless process spawn (like the old REPL handler)

**How:** The `kanban_task_spawn` MCP tool spawns a `kask` subprocess in a new
git worktree: `kask agent run --task <id> --worktree <path>`. The subprocess
runs a headless agent (no GPUI) that executes the task and writes results back
to the kanban board via the MCP server's SQLite store.

**Pros:**

- Mirrors the old `/kanban spawn` REPL pattern (separate process per task).
- True process isolation (separate PID, separate filesystem scope via worktree).
- No editor dependency — works in headless/server mode.
- The `kask` binary already exists; adding an `agent run --task` subcommand is
  incremental.

**Cons:**

- Requires a headless agent mode in the `kask` binary (the current `kask repl`
  is interactive; a `kask agent run` mode would need to be built).
- The spawned process needs its own inference + tool dispatch wiring (can't
  reuse the editor's `McpRuntime`).
- Worktree lifecycle management: who creates the worktree, who cleans it up?

**Complexity:** Medium. Requires `kask agent run --task` subcommand + worktree
creation + result writeback.

### Option C: No worktree isolation (status quo, document the trade-off)

**How:** Keep the current in-memory `LazyLocalSwarmRuntime::delegate()` path.
Document that spawned agents run in the same process/working-tree as the user,
and that worktree isolation is a future enhancement.

**Pros:**

- Zero implementation cost.
- The in-memory path is already tested and working.
- The gas accountant feedback loop (P2) works because the kata engine and the
  kanban service share the same SQLite store.

**Cons:**

- No isolation: a spawned agent's file mutations touch the user's working tree.
- No terminal session: the user can't see the spawned agent's terminal output
  (only the `delegate_result` written to the task).

**Complexity:** Zero.

## Decision: Option A — IMPLEMENTED (2026-08-10)

**Option A (editor-side worktree via `create_thread_tool`) was implemented.**

The IPC bridge now supports `InferenceMethod::CreateWorktreeThread`. The
`WorktreeSpawner` trait in `kask_bridge` is implemented by
`AgentPanelWorktreeSpawner` in `main.rs`, which calls
`AgentPanelSiblingHost::create_sibling_thread` with `use_new_worktree: true`.
The spawner is process-global (Mutex-based, re-settable) — set when an
`AgentPanel` is created, cleared when it's dropped.

`kanban_task_spawn` tries the worktree spawn first via the
`WorktreeSpawnPort` trait. On success, it records a comment on the task,
advances to `InProgress`, and returns. On failure (no IPC socket, no active
workspace, spawn error), it falls back to the in-memory
`LazyLocalSwarmRuntime::delegate()` path — the fallback is seamless.

### Known limitations

1. **`SiblingThreadInfo` doesn't expose thread id or worktree path.** The
   `WorktreeThreadInfo` response carries only a `message` string. The MCP
   server can't reference the created thread by id — the agent in the worktree
   must call `kanban_task_delegate_result` when done.

2. **No terminal visibility.** The user sees the spawned agent's work in a
   separate workspace tab (the editor creates it), but the MCP server only
   gets a confirmation message — no streaming output.

3. **The `kanban_task_spawn` doc comment still references the old decision
   doc.** It should be updated to reflect the implemented Option A path.
