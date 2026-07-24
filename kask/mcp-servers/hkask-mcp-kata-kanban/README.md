# hkask-mcp-kata-kanban

Kata-Kanban workflow coordination MCP server — task management with WIP limits, authenticated self-claim, kata prompts, and Regulation observability.

## Tools (18)

### Board management
| Tool | Description |
|------|-------------|
| `kanban_board_create` | Create a new kanban board with optional custom columns |
| `kanban_board_list` | List all kanban boards owned by the caller |

### Task CRUD
| Tool | Description |
|------|-------------|
| `kanban_task_create` | Create a new task on a kanban board |
| `kanban_task_list` | List tasks on a kanban board, optionally filtered by status |
| `kanban_task_move` | Move a task to a new column (status transition) |
| `kanban_task_assign` | Assign a task to an agent with consent proof (P1 compliance) |
| `kanban_task_verify` | Verify a task against its acceptance criteria |
| `kanban_task_reopen` | Reopen a completed task (Done → InProgress) with optional new budgets |

### Budget management
| Tool | Description |
|------|-------------|
| `kanban_task_add_gas` | Add gas/rJoules to a task's remaining budget |
| `kanban_task_add_rjoules` | Add rJoules to a task's inference/API budget (250k ≈ $1 spend) |

### Communication
| Tool | Description |
|------|-------------|
| `kanban_task_comment` | Add a comment to a task (feedback thread for subagent↔agent communication) |
| `kanban_task_comments_since` | Fetch task comments starting from an index (for incremental memory ingestion) |
| `kanban_task_add_deliverable` | Attach a deliverable (file path or URL) to a task as work output |

### Kata prompts
| Tool | Description |
|------|-------------|
| `kanban_task_kata_coaching` | Generate a Coaching Kata prompt (5-question dialogue) for a task |
| `kanban_task_kata_improvement` | Generate an Improvement Kata prompt (PDCA cycle) for a task |
| `kanban_task_kata_practice` | Generate a Starter Kata observation drill prompt for a task sub-problem |

### Agent spawning
| Tool | Description |
|------|-------------|
| `kanban_task_spawn` | Spawn a subagent for task execution with delegated skills and budgets |

### Contract management
| Tool | Description |
|------|-------------|
| `contract_propose_expect` | Create kanban tasks for contracts missing `expect:` annotations |

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_KANBAN_DB` | Per-agent kanban database file (defaults to `agents/{userpod}/kanban.db`) |
| `HKASK_DB_PASSPHRASE` | SQLCipher encryption passphrase |

## Regulation Spans

All tools emit `reg.tool.*` spans through the MCP framework. Kanban board/task operations additionally emit `reg.kanban` spans from `KanbanService`. Kata operations emit `reg.kata` spans when routed through `KataEngine`.

## Quick Start

```bash
# The server starts automatically with kask
kask chat
# Or standalone:
hkask-mcp-kata-kanban
```
