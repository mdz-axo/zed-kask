---
name: kanban-task-management
core: true
visibility: public
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
skip result without meaningful work. This is necessary because the
manifest executor runs all steps in ordinal order.

## MCP Tools

| Tool | Phase | When |
|------|-------|------|
| `kanban_board_create` | decompose | Post-cascade: create the board |
| `kanban_task_create` | decompose | Post-cascade: create each task |
| `kanban_task_list` | decompose, operate | Post-cascade: verify / pre-cascade: fetch |
| `kanban_board_list` | operate | Pre-cascade: fetch board state |
| `kanban_task_spawn` | delegate | Post-cascade: spawn subagent |
| `kanban_task_delegate_result` | delegate | Post-cascade: read structured result |
| `kanban_task_comment` | delegate, operate | Post-cascade: post progress notes / coordinator replies |
| `kanban_task_add_deliverable` | delegate, operate | Post-cascade: record deliverable links |
| `kanban_task_move` | operate | Post-cascade: execute status transitions |
| `kanban_task_verify` | operate | Post-cascade: record verification evidence |
| `kanban_task_reopen` | operate | Post-cascade: reopen for rework |
| `kanban_task_comments_since` | operate | Pre-cascade: read incremental updates |

All tools are on the `hkask-mcp-kata-kanban` server.

## Registry Templates

| Template | Phase | Purpose |
|----------|-------|---------|
| `triage.j2` | all | Determine which phase to run |
| `gather-context.j2` | decompose | Extract structured project context |
| `decompose-tasks.j2` | decompose | Decompose into INVEST-compliant tasks |
| `review-tasks.j2` | decompose | Review for quality and completeness |
| `populate-board.j2` | decompose | Board-ready format + MCP tool-call instructions |
| `configure-spawn.j2` | delegate | Propose spawn configuration |
| `execute-task.j2` | delegate | Execute task and report results |
| `monitor-board.j2` | operate | Assess board state, identify blockers |
| `coordinate-agents.j2` | operate | Prepare comment-thread replies |
| `track-deliverables.j2` | operate | Assess deliverables for completeness |
| `move-tasks.j2` | operate | Recommend status transitions with WIP enforcement |
| `verify-completion.j2` | operate | Evaluate completion against criteria |
| `escalate.j2` | operate | Prepare human-decision escalation records |

## Constraints

- rJoule cap: 5 per invocation. Maximum 10 iterations.
- Process manifest: `kask/registry/manifests/kanban-task-management.yaml`
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.