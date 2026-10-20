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

Board/WIP counts and goal-citation lookups remain outside that task transaction
to avoid nested store acquisition; this is not atomic WIP admission across
separate tasks or a transaction spanning task and goal records.

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
| `kanban_goal_list` | List complete caller-owned goals, including delivered and archived records |

Human closure, assessor leases/results, observed forecast outcomes and memory
receipts are **not MCP tools**. Authenticated native host code constructs typed
inputs and calls `KanbanService` directly; a worker-supplied actor string never
grants human authority. See [the exact bridge pact](GOAL_INTERFACE.md).

Closure atomically persists a pending assessment and a revision/digest-bound
projection. Claims preserve the original evidence manifest and a structurally
blinded input (no worker verdict, human label, probability or score). Assessment
failure/expired leases remain visible and retryable after restart. Evidence
strings are untrusted content, not proof of independent ground truth.

Forecasts are optional, frozen at declaration, and use only the declared human
acceptance or machine source/oracle. Horizon is the evaluation-window start;
resolution deadline is its end. Source observations outside that window are
not eligible outcomes. Binary Brier uses `(p-y)^2`, matched baseline loss and
undefined skill when baseline loss is zero; it is not a calibration certificate.
Scoring waits for an assessor attempt, not assessor agreement. Unknown/disputed
cases stay visible; source corrections append superseding score versions. A
human correction corrects the original outcome, not its evaluation time.
Only `forecast_status = resolved` identifies a current score; earlier score
versions remain history when a later correction is unknown or disputed.

The service stores complete projection payloads with stable event IDs, revision
and SHA-256 digest. The host drains pending work independently of model turns.
An exact delivery receipt marks a projection delivered and **never deletes the
canonical goal**. Transport is at-least-once, requiring idempotent effects at
the destination. Retrieval/indexing remains host-owned and may fail separately.

Historical rows migrate to `legacy.raw` with their exact historical data and an
explicit provenance limitation. Old predictions/scores are archived, not
retroactively admitted or reconstructed. Changed requirements declare a new
goal with `supersedes`; they never rewrite an earlier specification.

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

All tools emit `reg.tool.*` spans through the MCP framework. The server's one registered domain span is `reg.kata` (coaching-prompt generation, `src/kanban/service_impl/kata.rs:44`); no `reg.kanban` namespace exists.

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-kata-kanban
```
