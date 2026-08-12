---
name: kanban-task-management
visibility: public
description: >
  Manage the kanban board as agents execute delegated tasks. Monitor board
  state, coordinate between agents through comment threads, track
  deliverables, move tasks through columns based on progress, verify
  completion against acceptance criteria, and escalate when agents are
  blocked. This is the ongoing coordination skill — the board operator.
  Ontology: PKO — monitoring = pko:ProcedureExecution, verification =
  pko:StepVerification, transitions = pko:ChangeOfStatus.
---

# Kanban Task Management

The ongoing management of the kanban board as agents work. This is the
"board operator" skill — it monitors board state, coordinates between
agents, tracks deliverables, manages comment threads, verifies
completion, moves tasks through columns, and handles escalations.

## When to Use

- After tasks are on the board and agents are working
- When monitoring board state and identifying blockers
- When verifying task completion against acceptance criteria
- When coordinating between agents through comment threads

## When NOT to Use

- For task decomposition (use `kanban-task-decomposition`)
- For board creation (use `kanban-board-builder`)
- For initial spawn configuration (use `kanban-task-delegation`)

## Pipeline

```
Monitor → Coordinate → Track Deliverables → Move Tasks → Verify → Escalate
```

1. **Monitor** — Assess board state, identify blockers, flag overdue tasks
2. **Coordinate** — Read comment threads, respond to questions, resolve blockers
3. **Track Deliverables** — Assess submitted work for relevance and completeness
4. **Move Tasks** — Recommend status transitions based on evidence, enforce WIP limits
5. **Verify** — Evaluate deliverables against acceptance criteria, produce pass/fail
6. **Escalate** — Convert unresolved issues into human-operator-ready escalations

## MCP Tools

| Tool | When |
|------|------|
| `kanban_board_list` | Call to get board list for monitoring |
| `kanban_task_list` | Call to get tasks for monitoring and coordination |
| `kanban_task_move` | Call for each transition produced by move-tasks |
| `kanban_task_verify` | Call to record verification evidence from verify-completion |
| `kanban_task_comment` | Call to post coordinator replies and escalation records |
| `kanban_task_add_deliverable` | Call to record validated deliverables |
| `kanban_task_comments_since` | Call to read incremental comment updates |
| `kanban_task_reopen` | Call when verification fails and task needs rework |

All tools are on the `hkask-mcp-kata-kanban` server.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `monitor-board.j2` | `KnowAct` | Assess board state, identify blockers |
| `coordinate-agents.j2` | `KnowAct` | Prepare comment-thread replies and resolutions |
| `track-deliverables.j2` | `KnowAct` | Assess deliverables for relevance and completeness |
| `move-tasks.j2` | `KnowAct` | Recommend valid status transitions with WIP enforcement |
| `verify-completion.j2` | `KnowAct` | Evaluate completion against acceptance criteria |
| `escalate.j2` | `KnowAct` | Prepare human-decision escalation records |

## Constraints

- Gas cap: 50,000 per invocation.
- Process manifest: `kask/registry/manifests/kanban-task-management.yaml`
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.