---
name: kanban-task-decomposition
visibility: public
description: >
  Decompose a project description into discrete, independently verifiable
  :
  kanban tasks following INVEST criteria (Independent, Negotiable, Valuable,
  Estimable, Small, Testable). Prompt-chaining pipeline: gather context,
  decompose with vertical slicing and recomposition strategy, review for
  quality, and populate board-ready task list. The populate-board template
  includes post-cascade instructions for the agent to call kanban_board_create
  and kanban_task_create directly. Ontology: PKO — board = pko:Procedure,
  task = pko:Step.
---

# Kanban Task Decomposition

Decompose a project description into discrete, independently verifiable
kanban tasks. Uses INVEST criteria with vertical slicing — each task
delivers end-to-end user value. Includes recomposition strategy: the
plan for reassembling completed tasks into the final deliverable.

## When to Use

- Breaking a project description into board-ready tasks
- Before `kanban-board-builder` (which materializes the output via MCP tools)
- When you need INVEST-compliant task decomposition with recomposition strategy

## When NOT to Use

- For convergent planning with dependency graphs (use `task-breakdown`)
- For board creation (use `kanban-board-builder`)
- For ongoing board management (use `kanban-task-management`)

## Pipeline (Prompt Chaining)

```
Gather Context → Decompose → Review → Populate Board
```

1. **Gather Context** — Extract project name, goals, constraints, resources, target task size
2. **Decompose** — Break into INVEST-compliant tasks with vertical slicing, dependencies, recomposition strategy
3. **Review** — Check quality, INVEST compliance, completeness, recomposition viability
4. **Populate Board** — Convert accepted tasks into board-ready format

## Output

Produces a `board_tasks` JSON array. The agent then materializes the board
by calling `kanban_board_create` + `kanban_task_create` directly (see the
`populate-board.j2` template's post-cascade instructions).

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `gather-context.j2` | `KnowAct` | Extract structured project context |
| `decompose-tasks.j2` | `KnowAct` | Decompose into INVEST-compliant tasks |
| `review-tasks.j2` | `KnowAct` | Review for quality and completeness |
| `populate-board.j2` | `KnowAct` | Convert accepted tasks to board-ready format |

## Constraints

- Gas cap: 25,000 per invocation.
- Process manifest: `kask/registry/manifests/kanban-task-decomposition.yaml`
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.