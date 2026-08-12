---
name: kanban-board-builder
visibility: public
description: >
  Materialize a kanban board from a decomposition plan or direct task
  specification. Creates the board via kanban_board_create, creates tasks
  via kanban_task_create with criteria and gas budgets, optionally spawns
  subagents via kanban_task_spawn, and emits a kanban block for widget
  rendering. Bridges kanban-task-decomposition (Plan) and
  kanban-task-management (Check) to the actual kata-kanban MCP tools (Do).
  Ontology: PKO (Procedural Knowledge Ontology) — board = pko:Procedure,
  task = pko:Step, execution = pko:StepExecution, verification =
  pko:StepVerification. Convergent PDCA: Plan → Execute → Verify → Render.
---

# Kanban Board Builder

Materialize a kanban board from a decomposition plan or direct task
specification. This is the "Act" in the kanban PDCA lifecycle:

```
kanban-task-decomposition (Plan) → kanban-board-builder (Do) → kanban-task-management (Check/Act)
```

The existing kanban-task-decomposition skill produces structured JSON
describing tasks (INVEST-compliant, with acceptance criteria, estimates,
and recomposition strategy). The kanban-task-management skill monitors
and coordinates the board once tasks are active. But neither calls the
actual MCP tools — that's this skill's role.

## When to Use

- After running `kanban-task-decomposition` — consume its `board_tasks` output
- When you have a direct task specification and want to create a board
- When you need to create a board, tasks, and spawn subagents in one pipeline

## When NOT to Use

- For task decomposition (use `kanban-task-decomposition` or `task-breakdown`)
- For ongoing board management (use `kanban-task-management`)
- For subagent spawn configuration planning (use `kanban-task-delegation`)

## PDCA Shape (emergent from PKO)

The shape follows PKO's specification/execution separation:

1. **Plan** — Map decomposed tasks to MCP tool calls. Resolve gas budgets
   from effort estimates, criteria from acceptance criteria, initial
   column from task status. (`pko:Procedure` specification)
2. **Do** — Execute: `kanban_board_create` + `kanban_task_create` for each
   task + optional `kanban_task_spawn` for delegated tasks.
   (`pko:ProcedureExecution` + `pko:StepExecution`)
3. **Check** — Verify: `kanban_board_list` + `kanban_task_list` against
   the plan. (`pko:StepVerification`)
4. **Act** — Render: emit ` ```kanban ` block for KanbanWidget rendering.
   The provenance field makes move chips clickable.

## MCP Tools Used

| Tool | Purpose |
|------|---------|
| `kanban_board_create` | Create the board with columns |
| `kanban_task_create` | Create each task with criteria + gas budget |
| `kanban_task_spawn` | Spawn subagent for delegated tasks |
| `kanban_board_list` | Verify board exists |
| `kanban_task_list` | Verify tasks + collect data for rendering |

All tools are on the `hkask-mcp-kata-kanban` server.

## Inputs

- `decomposed_tasks` (required): Array of tasks from decomposition or direct spec
- `project_name` (optional): Board name
- `swarm_id` (optional): Link spawned tasks to a swarm
- `spawn_config` (optional): Delegation configuration for subagent spawning
- `custom_columns` (optional): Custom column definitions with WIP limits

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `plan-creation.j2` | `KnowAct` | Map decomposed tasks to MCP tool calls |
| `execute-creation.j2` | `KnowAct` | Execute board + task creation + optional spawn |
| `verify-and-render.j2` | `KnowAct` | Verify board state + emit kanban block |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility.
- Gas cap: 20,000 per invocation. Maximum 5 iterations.
- The kanban block provenance must use server `"hkask-mcp-kata-kanban"` (from `kanban_wire::KANBAN_SERVER_NAME`).
- Task gas budgets default to estimated_hours × 1000 (1 hour ≈ 1000 gas units), minimum 2000.
- All tasks start in `backlog` status unless a dependency requires `ready`.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.