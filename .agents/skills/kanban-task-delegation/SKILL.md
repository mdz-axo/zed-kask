---
name: kanban-task-delegation
visibility: public
description: >
  Delegate kanban tasks to sub-userpods. Configure spawn parameters
  (delegation level, skills, memory scope, gas budget, timeout) and
  execute the task. The spawned agent works on the task, producing
  deliverables and appending progress notes to the comment thread.
  Ongoing coordination is handled by kanban-task-management. Ontology:
  PKO — task execution = pko:StepExecution, delegation = pko:StepExecution.
---

# Kanban Task Delegation

Delegate kanban tasks to sub-userpods (subagents). Handles the initial
handoff: configure spawn parameters, then execute the task. The spawned
agent works on the task and reports back through the comment thread.

## When to Use

- After `kanban-board-builder` has created tasks on the board
- When a task needs subagent execution with skill cascades and gas budgeting
- When configuring `kanban_task_spawn` parameters for a specific task

## When NOT to Use

- For board creation (use `kanban-board-builder`)
- For ongoing board monitoring (use `kanban-task-management`)
- For task decomposition (use `kanban-task-decomposition`)

## Pipeline

```
Configure Spawn → Execute Task
```

1. **Configure Spawn** — Determine delegation level, skills, memory scope, tool servers, gas budget, timeout
2. **Execute Task** — The spawned agent works on the task, producing deliverables and progress notes

## MCP Tools

| Tool | When |
|------|------|
| `kanban_task_spawn` | Call after configure-spawn with the produced config |
| `kanban_task_comment` | Call to post progress notes from the spawned agent |
| `kanban_task_add_deliverable` | Call to attach deliverable links to the task |
| `kanban_task_delegate_result` | Call to read the structured delegation result |

All tools are on the `hkask-mcp-kata-kanban` server.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `configure-spawn.j2` | `KnowAct` | Propose minimum-capable spawn configuration |
| `execute-task.j2` | `KnowAct` | Execute task and report status/deliverables/blockers |

## Constraints

- Gas cap: 12,000 per invocation.
- Process manifest: `kask/registry/manifests/kanban-task-delegation.yaml`
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.