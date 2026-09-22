# hkask-mcp-kata-kanban

Kata-Kanban workflow coordination MCP server — task management with WIP limits, authenticated self-claim, kata prompts, and Regulation observability.

## Tools

### Board management
| Tool | Description |
|------|-------------|
| `kanban_board_create` | Create a new kanban board with optional custom columns |
| `kanban_board_list` | List all kanban boards owned by the caller |
| `kanban_board_update` | Rename a kanban board (owner only); the name is the board's addressing key |
| `kanban_board_delete` | Delete a kanban board and all its tasks |
| `kanban_board_export` | Export a kanban board as mermaid kanban markdown |
| `kanban_board_import` | Import current-format mermaid kanban markdown as one atomic board aggregate; every section requires explicit `%% kanban column status: <wire-status>` metadata |

### Task CRUD
| Tool | Description |
|------|-------------|
| `kanban_task_create` | Create a new task on a kanban board; `advances` cites the goal criteria the task serves (validated against the goal, captured as documentation) |
| `kanban_task_update` | Update editable fields on a task (title, description, criteria, priority, labels, `advances` citations); only the task owner can edit |
| `kanban_task_list` | List tasks on a kanban board, optionally filtered by status |
| `kanban_task_move` | Move a task to a new column (status transition) |
| `kanban_task_assign` | Assign a task to an agent with consent proof (P1 compliance) |
| `kanban_task_verify` | Verify a task against its acceptance criteria |
| `kanban_task_reopen` | Reopen a completed task (Done → InProgress) |

### Goals (functional target conditions)

Native goal-setting and verification for the four-moves interaction loop
(`kask/docs/architecture/functional-interaction-spec.md`). A goal is the
kata target condition: the user's functional requirement in the user's
words, with observable criteria and a Brier-scored intake prediction.

**Goals persist through resolution until curator-memory acknowledgment**
(operator ruling 2026-09-16, extending the 2026-09-09 persistence ruling):
the goal store is the same DB-backed HMemStore that persists boards and tasks,
so the Brier closure (`kanban_goal_score`) survives server restarts as a
retryable outbox entry. The production turn-ingestion path stores the score as
a first-class goal h_mem in curator memory, then invokes
`kanban_goal_memory_acknowledge` to prune the retained row. Failed ingestion or
acknowledgment leaves the resolved goal retryable; conflicting outcomes are
rejected. Every goal event uses the same shared `curator:goal:{goal_id}` copy;
there is no curator-perspective duplicate.

| Tool | Description |
|------|-------------|
| `kanban_goal_create` | Create a functional goal with 1–4 observable criteria and an optional intake prediction |
| `kanban_goal_judge` | Record a done/continue/blocked verdict with confidence and a result for every criterion (history preserved) |
| `kanban_goal_score` | Resolve a goal (achieved/not-achieved) and Brier-score the intake prediction; retain the resolved row until memory acknowledgment |
| `kanban_goal_memory_acknowledge` | Internal lifecycle operation: confirm the scored outcome reached curator memory, then prune the retained row |
| `kanban_goal_list` | List the caller's goals, including resolved goals awaiting memory acknowledgment, newest first |

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
| `kanban_task_spawn` | Spawn a subagent for task execution with delegated skills |

### Contract management
| Tool | Description |
|------|-------------|
| `contract_propose_expect` | Create kanban tasks for contracts missing `expect:` annotations |

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_KANBAN_DB` | Kanban database file (defaults to `{kask_data_dir}/mcp/kata-kanban/kanban.db`) |
| `HKASK_DB_PASSPHRASE` | SQLCipher encryption passphrase |

## Regulation Spans

All tools emit `reg.tool.*` spans through the MCP framework. Kanban service operations additionally emit `reg.kanban` spans from `KanbanService`.

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-kata-kanban
```
