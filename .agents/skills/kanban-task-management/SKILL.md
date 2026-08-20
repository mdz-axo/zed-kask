---
name: kanban-task-management
core: true
description: "Unified kanban task management across the full task lifecycle. Decompose projects into INVEST-compliant tasks, delegate to subagents with spawn config and rJoule budgeting, monitor boards, coordinate agents, verify completion, and escalate."
---

# Kanban Task Management

Unified kanban task management across the full task lifecycle. Three
phases triaged at runtime based on operator inputs:

```
Decompose → Delegate → Operate
```

- **Decompose**: Break a project description into INVEST-compliant kanban
  tasks with acceptance criteria, dependencies, and recomposition strategy.
  The agent then calls `kanban_board_create` + `kanban_task_create`.
- **Delegate**: Configure spawn parameters for a task and execute it via a
  subagent. The agent then calls `kanban_task_spawn` + `kanban_task_comment`.
- **Operate**: Monitor the board, coordinate agents through comment threads,
  track deliverables, move tasks through columns, verify completion, and
  escalate to the human operator. The agent calls `kanban_task_list`,
  `kanban_task_move`, `kanban_task_verify`, `kanban_task_comment`, etc.

## When to Use

- Decompose: When you have a project description and need to break it into
  board-ready tasks. Pass `project_description` as a skill input.
- Delegate: When you have a task that needs subagent execution. Pass
  `task_to_delegate` as a skill input.
- Operate: When you have a board with active tasks that need monitoring,
  coordination, or verification. Pass `board_id` as a skill input.

## When NOT to Use

- For convergent planning with dependency graphs (use `task-breakdown`)
- For TDD execution of vertical slices (use `tdd`)

## Phase Selection (Triage)

The first step (`triage.j2`) examines the available inputs and determines
which phase to run:

| Input present | Phase | Steps |
|---|---|---|
| `project_description` | decompose | gather-context → decompose-tasks → review-tasks → populate-board |
| `task_to_delegate` | delegate | configure-spawn → execute-task |
| `board_id` | operate | monitor-board → coordinate-agents → track-deliverables → move-tasks → verify-completion → escalate |

Templates for non-active phases receive the triage phase and produce a
skip result without meaningful work. This is necessary because skill execution processes all phases in order.

## MCP Tools

| Tool | Phase | When |
|------|-------|------|
| `kanban_board_create` | decompose | Post-process: create the board |
| `kanban_task_create` | decompose | Post-process: create each task |
| `kanban_task_list` | decompose, operate | Post-process: verify / pre-step: fetch |
| `kanban_board_list` | operate | Pre-process: fetch board state |
| `kanban_task_spawn` | delegate | Post-process: spawn subagent |
| `kanban_task_delegate_result` | delegate | Post-process: read structured result |
| `kanban_task_comment` | delegate, operate | Post-process: post progress notes / coordinator replies |
| `kanban_task_add_deliverable` | delegate, operate | Post-process: record deliverable links |
| `kanban_task_move` | operate | Post-process: execute status transitions |
| `kanban_task_verify` | operate | Post-process: record verification evidence |
| `kanban_task_reopen` | operate | Post-process: reopen for rework |
| `kanban_task_comments_since` | operate | Pre-process: read incremental updates |

All tools are on the `hkask-mcp-kata-kanban` server.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `triage.j2` | Triage step. Examines available inputs (project_description, task_to_delegate, board_id) and determines which phase to run: decompose, delegate, or operate. |
| `gather-context.j2` | Extract structured project context: project name, goals, constraints, resources, and target task size. Phase: decompose. |
| `decompose-tasks.j2` | Decompose a project into INVEST-compliant tasks with vertical slicing, dependencies, recomposition strategy, and acceptance criteria. Phase: decompose. |
| `review-tasks.j2` | Review decomposed tasks for INVEST compliance, completeness, and recomposition viability. Phase: decompose. |
| `populate-board.j2` | Convert accepted tasks into board-ready format. Includes post-step instructions for the agent to call kanban_board_create and kanban_task_create. Phase: decompose. |
| `configure-spawn.j2` | Configure spawn parameters: delegation level, skills, memory scope, rJoule budget, timeout. Includes post-step instructions for the agent to call kanban_task_spawn. Phase: delegate. |
| `execute-task.j2` | Execute a delegated task within its approved configuration. Includes post-step instructions for the agent to call kanban_task_comment and kanban_task_add_deliverable. Phase: delegate. |
| `monitor-board.j2` | Monitor board state, identify blockers, flag overdue tasks. Includes pre-step instructions for the agent to fetch board data via kanban_board_list and kanban_task_list. Phase: operate. |
| `coordinate-agents.j2` | Read active-task comment threads and prepare actionable replies. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |
| `track-deliverables.j2` | Assess deliverables for completeness. Includes post-step instructions for the agent to call kanban_task_move and kanban_task_add_deliverable. Phase: operate. |
| `move-tasks.j2` | Recommend status transitions based on evidence. Includes post-step instructions for the agent to call kanban_task_move. Phase: operate. |
| `verify-completion.j2` | Evaluate deliverables against acceptance criteria. Includes post-step instructions for the agent to call kanban_task_verify and kanban_task_move. Phase: operate. |
| `escalate.j2` | Convert unresolved issues into human-operator-ready escalations. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- Process manifest: `kask/registry/manifests/kanban-task-management.yaml`
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.