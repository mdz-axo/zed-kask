# Swarm capability removal plan — 2026-10-28

**Goal (operator order):** remove the `hkask-mcp-swarm` MCP server, its related
skills, and every package/function that exists specifically for the local swarm
agents (A2A protocol surface, the local agent registry, the child-side tool-invoke
IPC path, the swarm memory store). This finishes the arc the ABW removal
(2026-10-22) started: no swarm capability remains in zed-kask.

**What survives (explicitly):** the kata-kanban task-delegation flow itself —
`kanban_task_spawn` executes via zed-native worktree agent threads
(`WorktreeSpawnPort` → `AgentPanelSiblingHost::create_sibling_thread`), not via
the swarm runtime. The delegation-grant machinery (`delegation_grants.rs`,
`kask.mcp.delegated_tools`), the `CreateWorktreeThread` IPC method, and
`hkask-tool-invoker` stay — kanban and the panels are their live consumers.

## Decisions (technical, presented for functional veto)

1. **The curated local-agent registry is removed entirely** — including
   kata-kanban's reuse of it. `kanban_task_spawn` always builds the in-memory
   task-agent card (the current fallback path; behavior-identical for every
   spawn that had no skill-covering expert card). The registry had no authoring
   surface after the panel dies, one consumer, and is "specifically included
   for the local swarm agents" per the order. `HKASK_LOCAL_AGENTS_DIR` and the
   `kask.swarm` settings section die with it.
2. **`kanban_task_delegate_result` + the `Task.delegate_result` /
   `deterministic_verdict` / `swarm_id` fields are removed** — census verified
   there is no writer anywhere in the tree (the writer died with the in-server
   swarm executor; the worktree flow reports via task comments). A read-only
   tool that can only ever return `has_result: false` is the compatibility
   state the 2026-09-09 no-dead-surface ruling forbids. Kanban tool count goes
   27 → 26.
3. **The child→parent tool-invoke IPC path is removed with its only consumer**
   (`InferenceMethod::ToolInvoke` + `ToolDefinition` dispatch arms,
   `InferenceIpcClient::invoke_tool`/`tool_definition`,
   `InferencePort::invoke_tool`, `host_skill_tools.rs`, `HostSkillToolPort`,
   the H7 IPC test module, the `skill_tx` channel). This path existed for
   MCP-server child agent loops; the swarm server's local delegate was the only
   one. `create_worktree_thread` (the other consumer of grants) is untouched.
4. **`agent_card` is removed from the experimentation `LAYERS` closed set** —
   its artifact substrate (local agent cards) no longer exists. Old experiment
   rows keep reading (layer is a stored string); `population_query` with the
   removed filter value fails `UnknownLayer`.
5. **The panel persona is renamed** `swarm-panel` → `kask-panel` (const
   `SWARM_PANEL_CALL_CAP` → `KASK_PANEL_CALL_CAP`, seed + pin updated) — an
   accounting identity for panel tool calls, kept aligned after the swarm panel
   dies.

## Removal inventory

- `kask/mcp-servers/hkask-mcp-swarm/` — 31 source files, 16,637 LOC (incl.
  a2a.rs/a2a_http.rs/a2a_tools.rs, local registry/runtime/swarms, port
  registry, schema validation, thread store, task board, agent stats).
- `crates/swarm_panel/` — 13 files, 5,593 LOC; `crates/hkask-swarm-widget/`
  (519 LOC, renders ```swarm_delegate_results fenced blocks) + its viz-core
  factory arm and D18 admitted fence language.
- `a2a` (external `a2a-lf` 0.3) workspace dep — swarm-only consumer.
  `tiny_http` stays (upstream uses it).
- kata-kanban: swarm dep + registry field/param + `build_task_agent_card`'s
  `LocalAgentCard` + `swarm_id`/`memory_scope` spawn params + `delegate_result`
  fields/tool (Decision 2) + `HKASK_LOCAL_AGENTS_DIR` read.
- kask_bridge: swarm `BuiltinMcpServer` entry, `emit_swarm_env`,
  `KaskSwarmSettings` (+ content + settings_ui page), `swarm_memory` managed-DB
  row + resolver, `HKASK_MCP_SERVER_IDS` emission (swarm-only consumer),
  host-skill-tool surface (Decision 3), swarm references in tests.
- curator server: `curator_memory_backlink_embeddings` "swarm" store arm.
- experimentation server: `agent_card` layer + doc-comment surfaces.
- zed-side: swarm panel init/menu/status-bar/`set_tool_invoker` call (re-pointed
  to `hkask_tool_invoker`), `PanelToolInvoker` retyped to the leaf trait, cap
  seed rename, `test_action_namespaces` pin, system-prompt viz-block mention.
- Skills: `swarm-intelligence` (481 LOC + 6 templates), `local-research-swarm`
  (102 LOC + 2 templates) — deleted, names appended to the retired record
  (check-retired-record.sh gate). Surviving skills with dying-tool references
  reworked: `kanban-task-management`, `algedonic-review`, `create-skill`,
  `kata-improvement`, `self-improvement` (+ incidental mentions in
  `mcp-tool-review`, `skill-maintenance`, `therapy`, `superforecasting`,
  `experimental-design`, `eureka-cycle`).
- Docs: `kask/docs/reference/mcp-servers/swarm.md`, `diataxis/swarm_system/*`,
  `diagrams/swarm.md` deleted; ~30 referencing docs updated; registry counts
  reconciled.
- DIVERGENCE.md: D33 retired; D1 (F3), D2, D3, D6, D18, D28, D45, D54, D80,
  D86 prose updated; workspace-members and `marketplace_ui_common` lines.
- Scripts: `mcp-servers.txt`, `check-docs-citation-anchors.sh`,
  `check-mcp-tool-tests.sh` (comment), `check-structural-traps.sh`,
  `check-unsafe-forbid.sh` (scan dirs), `check-uninstall-paths.sh` (fixture
  binary name).

## Phases

1. **kata-kanban decoupling** — sever the swarm dep first so the tree never
   holds a broken edge. Registry removal, spawn simplification, field/tool
   removal, all test constructors, tool-count pin 27 → 26.
2. **kask_bridge + hkask-inference + hkask-types decoupling** — IPC tool path
   removal, settings/env/identity swarm surfaces, test constructors.
3. **Delete server + zed-side surface** — crates, workspace wiring, panel
   registration, widget fence, settings UI, curator/experimentation servers,
   scripts.
4. **Skills, templates, docs, DIVERGENCE.md** — retire skills + retired record,
   rework referencing skills, docs reconciliation.
5. **Sweep + verify** — full-repo identifier sweep, `./script/clippy` over the
   touched scope, `cargo check -p zed`, kata/bridge test suites, check scripts.

## Out of scope / operator follow-ups

- Dormant on-disk data (`mcp/swarm/*` DBs, curated agent cards under the hKask
  data dir) — operator-owned; deletion is an operator decision (the
  training-server precedent: deleted at operator direction, never
  auto-deleted).
- `.rules` updates (Agent typing system section, swarm mentions in MCP
  patterns) — proposed in the change report, not edited inline.