# hkask-mcp-kata-kanban

Kata-Kanban workflow coordination MCP server — task management with WIP limits, authenticated self-claim, kata prompts, and Regulation observability.

## Tools

### Board management
| Tool | Description |
|------|-------------|
| `kanban_board_create` | Create a new kanban board with optional custom columns; `done_requires_verification` gates Done behind `kanban_task_verify` (stamped by convention for the reserved "Algedonic review" name) |
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
| `kanban_task_move` | Move a task to a new column (status transition); on verification-gated boards (e.g. the Algedonic review board), Done is reachable only via `kanban_task_verify` |
| `kanban_task_assign` | Claim an unassigned task as the authenticated caller (no arbitrary-agent assignment; the caller's identity is the assignee) |
| `kanban_task_verify` | Record verification evidence for a Review task — the user's confirmation text is the pass signal (criteria guide work but do not gate completion; only the operator's relayed confirmation moves Review to Done) |
| `kanban_task_reopen` | Reopen a completed task (Done → InProgress) |

Task mutations transform the current stored task inside the existing
`HMemStore::update_value_atomic` transaction, rather than replacing an earlier
snapshot. Independent changes (including comments, status, deliverables,
assignment, editable fields, verification, reopening and spawn configuration)
preserve one another. Task authorization and lifecycle checks use the current
value; a deleted task returns not-found instead of being recreated. Conflicting
scalar edits follow commit order, and transitions invalidated by an earlier
commit are rejected. Task/index identity is unchanged.

WIP admission runs inside the task transaction: the sibling-task count is read
through the same `IMMEDIATE` transaction that commits the move
(`update_value_atomic_with_reads`), so two concurrent movers into a full column
cannot both pass the count. An indexed task row that is missing or fails to
decode is surfaced — listings report it (`undecodable_tasks`), and the WIP
admission refuses the move naming the row, because an unreadable row can only
undercount and undercounting over-admits. Goal-citation lookups remain outside
the task transaction (a nested pool acquisition would deadlock the connection
the transaction already holds).

### Goals

Goals are verification-primary canonical records: original explicit criteria,
stable criterion IDs, a specification digest, and append-only histories. Work
closure (accomplished/stop) and human outcome (achieved/not achieved/unknown)
are distinct from worker progress and independent criterion assessment.

| Tool | Description |
|------|-------------|
| `kanban_goal_create` | Declare 1–4 explicit criteria and their source; optionally freeze a prospective forecast |
| `kanban_goal_judge` | Append worker progress with exact criterion coverage; done requires every criterion passed |
| `kanban_goal_get` | Get the complete caller-owned specification and lifecycle history |
| `kanban_goal_list` | Return an array of complete caller-owned current-schema goals, including closed records; invalid current records fail visibly |

Human closure and corrections are **native-only**; worker actor strings never
grant human authority. The service owns assessment claim/expiry/conditional
finalization behind one operation, rather than exposing a worker subprotocol.
Exact consented local-file contents or failure receipts are frozen before a
fresh blinded assessment. No fetched evidence means Unknown without inference;
support/contradiction must cite fetched digest-matched IDs. Failures retry
without reopening human closure.

Forecasts are optional and prospectively frozen. Only human acceptance resolves
them; there are no scheduled file-digest or arbitrary machine resolvers. Original
closure time must fall inside the declared horizon/deadline window. Corrections
relabel that event without moving its time. Unknown is unscored; baseline loss
zero has undefined skill. Human and assessor labels remain separate.

Recall has one current memory per goal, atomically published with its vector
using existing semantic write/revision facilities. A current revision and last
indexed revision replace full-snapshot logs and per-event receipts. Publication
and acknowledgment are protected by the canonical revision transaction; failed
acknowledgment retries after restart without appending another memory record.
Delivery never deletes canonical history.

Only schema version 3 is accepted. `goal_list(owner)` returns `Vec<Goal>` after
filtering caller ownership before decoding; the host scan returns owner-grouped
vectors. Invalid current records and query failures remain visible errors, not
omitted rows or a separate rejection channel. Obsolete goal rows and projections
are discarded by an explicit one-time reset, with no runtime migration, archive,
or backward compatibility contract. No reset execution is claimed here.
See [the exact ownership boundary](GOAL_INTERFACE.md).

### Communication
| Tool | Description |
|------|-------------|
| `kanban_task_comment` | Add a comment to a task (feedback thread for subagent↔agent communication) |
| `kanban_task_comments_since` | Fetch task comments starting from an index (for incremental memory ingestion) |
| `kanban_task_add_deliverable` | Attach a deliverable (file path or URL) to a task as work output |

### Kata prompts
| Tool | Description |
|------|-------------|
| `kanban_task_kata_prompt` | Generate a task-scoped prompt; `stage` selects `coaching`, `improvement`, or `practice` (`sub_problem` required for `practice`) |
| `kanban_task_kata_record` | Record durable kata state (target condition, focus obstacle, last experiment with prediction and outcome) — the prompts read it back, so a new session continues the routine instead of restarting from zero |

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

All tools emit `reg.tool.*` spans through the MCP framework. Domain spans:
`reg.kata` (kata prompt generation and kata-state recording,
`src/kanban/service_impl/kata.rs`) and `hkask.kanban` (board/task lifecycle
mutations — `board_created`, `board_renamed`, `board_imported`,
`task_created`, `task_moved`, `task_assigned`, `task_verified`,
`src/kanban/service_impl/service.rs`).

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-kata-kanban
```
