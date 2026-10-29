---
name: kanban-task-management
core: true
description: "Unified kanban task management across the full task lifecycle. Decompose projects into INVEST-compliant tasks, delegate to subagents with spawn configuration, monitor boards, coordinate agents, verify completion, and escalate."
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
- **Delegate**: Configure one spawn for a task, call `kanban_task_spawn`, then read the spawn receipt and the delegate's report from the task's comment thread (`kanban_task_comments_since`). Comments and deliverable links are derived from that observed report; no second execution prompt runs.
- **Operate**: Monitor the board, coordinate agents through comment threads,
  track deliverables, move tasks through columns, verify completion, and
  escalate to the human operator. The agent calls `kanban_task_list`,
  `kanban_task_move`, `kanban_task_verify`, `kanban_task_comment`, etc.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Bind tasks to the original goal criteria. Board/task completion is worker progress, never authority to close a goal as achieved.

## Initial and target condition

- **Initial condition:** the operator's confirmed functional goal and criteria (or an explicit gap awaiting confirmation), current board/task state, and the source task/delegation evidence. A project description alone does not authorize inferred goals.
- **Target condition:** in Decompose, one board whose created tasks cite the exact goal criteria they advance; in Delegate, one observed persisted result or explicit execution failure; in Operate, a re-read board with each transition supported by real evidence and no unresolved Review/staleness finding silently counted as done. Goal achievement remains the operator's judgment, not a count of empty task lists.

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

## D/P labelling

Task decomposition, delegation configuration, and escalation judgment are P —
critiqued by the verification evidence tiers and the operator. The board
operations are D (the kanban tools are the durable ledger — a task's state
comes from the board, never from the manager's memory), and the sweep gate is
D (`lisp_eval` over `unverified_review` and `stalled_in_progress` reconciled
against the newly re-listed board — it checks sweep obstacles only, never
proving the functional goal). A done task without its evidence tier recorded
is unverified, not complete.

## Instructions

Step 0 (deterministic triage and preflight, **`KTM-1`** via `lisp_eval`) examines the available inputs, verifies goal-criterion `advances` readiness for decomposition, and checks column WIP limits (`count(column_tasks) < wip_limit`):
- form: `(list (cond (has_project_description "decompose") (has_task_to_delegate "delegate") (has_board_id "operate") (t "missing_input")) (and goal_confirmed (> advances_count 0)) (< (length column_tasks) wip_limit))`
- env: `{ "has_project_description": <bool>, "has_task_to_delegate": <bool>, "has_board_id": <bool>, "goal_confirmed": <bool>, "advances_count": <int>, "column_tasks": <array of tasks in target column>, "wip_limit": <column WIP limit> }`

| Input present | Phase | Steps |
|---|---|---|
| `project_description` | decompose | `decompose-tasks` → `review-tasks` → `kanban_board_create` + `kanban_task_create` |
| `task_to_delegate` | delegate | `configure-spawn` → `kanban_task_spawn` → `kanban_task_comments_since` |
| `board_id` | operate | `kanban_board_list` + `kanban_task_list` → `coordinate-agents` → `track-deliverables` → `kanban_task_move` → `verify-completion` → `escalate` |

Templates for non-active phases are not rendered. Before Decompose, confirm the operator's functional goal, 2–4 observable criteria and any existing goal identity; an inferred goal is only a question. When confirmed and no goal exists, call `kanban_goal_create` once with the operator's `goal_text`, criteria, actual `source` and a stable idempotency key; omit routine `forecast` and retain the returned full goal's `id` and verbatim criterion text for `kanban_task_create.advances` (verified by `KTM-1`). Render `kanban-task-management/decompose-tasks` with `project_description`, `target_task_hours`, `goals`, `constraints`, and `resources`; it emits `project_name`, `tasks`, and `recomposition`. Then render `kanban-task-management/review-tasks` with `tasks` and `project_name`, and create the board (`kanban_board_create`) and accepted tasks (`kanban_task_create`) directly via MCP tools. If the user has not confirmed the target, stop before board creation. For Delegate, render `kanban-task-management/configure-spawn` with `task` and `available_skills`.

### Reference models and labels

Anderson, *Kanban: Successful Evolutionary Change* (2010) — visualize work, limit WIP, manage flow (`onto_anchor` → derived `kanban`, `work_in_progress_limit`); Wake, "INVEST in Good Stories" (2003) for task shape (`onto_anchor` → derived `invest_criteria`). Board state, phase triage/WIP check (`KTM-1`), oracle-tier reduction (`KTM-2`), and every transition are D (the kanban tools are the oracle; `kanban_task_list` re-reads confirm a move took). Decomposition, delegation briefs and the judgment that Review evidence is sufficient are P, critiqued by `kanban_task_verify` evidence and by the operator, who owns acceptance.

### Delegate result handoff

After `kanban_task_spawn`, the spawned worktree agent executes in its own session and reports through the task's comment thread — read the spawn receipt and the delegate's report with `kanban_task_comments_since` for the *same* task ID. If no report has landed, leave the task open and surface the missing receipt; do not render another execution prompt. Use the reported response and any verified paths to add a deliverable or post a progress/blocker comment. No second task record or model-produced execution report is created.

### Operate-phase sweep loop

One bounded PDCA applies to the active phase: Plan from the initial and target conditions above; Do only the admitted board/spawn/transition operations; Check by re-reading real tool state and the cited goal criteria; Act once on a named gap (complete a missing board write or return failed work for correction), or stop with the gap and operator decision needed. Do not replay a spawn after an uncertain tool error. For the operate phase specifically, close with a board sweep:

1. Re-run `kanban_task_list` — the Check signal is two lists: tasks in
   Review without verification evidence, and InProgress tasks with no
   activity since the last sweep (staleness threshold per the operator).
2. Act per finding: render `kanban-task-management/track-deliverables` with `task` and `deliverables` to assess deliverable readiness, and render `kanban-task-management/verify-completion` with `task_id`, `task_title`, `criteria`, `deliverables`, and `evidence` to classify per-criterion oracle tiers, then reduce the oracle-tier lattice deterministically via **`KTM-2`** (`lisp_eval`). Call `kanban_task_verify` only after `KTM-2` passes and the operator confirms a Review task's observed pass evidence; its nonempty `evidence` is the pass signal and moves that task to Done. Comment on a failed/unsupported submission without calling verify, and return it for rework — render `kanban-task-management/coordinate-agents` with `board_name` and `active_tasks` (the re-listed board's active tasks) to prepare the replies. Comment to unblock stalled tasks. Escalations render `kanban-task-management/escalate` with `board_name` and `escalation_candidates` (from coordinate-agents' output).
3. Re-list to confirm the transitions took.
4. Reconcile the IDs in `unverified_review` and `stalled_in_progress` against the *newly re-listed board* before calling `lisp_eval` with `(and (= (length unverified_review) 0) (= (length stalled_in_progress) 0))`. This gate checks sweep obstacles only; it does not prove the functional goal was achieved. On a mismatch or a surviving blocker, stop after this one sweep and escalate with the actual task IDs.
5. Clean up (storage Cleanup rule). When every task on the board is Done and its deliverables are recorded outside the board, ask the operator to confirm closure, then call `kanban_board_delete(board_id)` and re-run `kanban_board_list` to confirm. A board with any open, Review or blocked task is kept and named in the report with its open task IDs; never delete it to make a sweep look clean.

## MCP Tools

| Tool | Phase | When |
|------|-------|------|
| `kanban_goal_create` | decompose | Persist the operator-confirmed functional target before board creation |
| `kanban_board_create` | decompose | Post-step: create the board |
| `kanban_board_delete` | operate | Cleanup: delete an all-Done board after operator closure |
| `kanban_task_create` | decompose | Create each accepted task with exact goal-criterion `advances` citations |
| `kanban_task_list` | decompose, operate | Post-step: verify / pre-step: fetch |
| `kanban_board_list` | operate | Pre-step: fetch board state |
| `kanban_task_spawn` | delegate | Post-step: spawn subagent once (tool may require enabling in this session) |
| `kanban_task_comments_since` | delegate, operate | Post-step: read the spawn receipt and the delegate's report |
| `kanban_task_comment` | delegate, operate | Post-step: post progress notes / coordinator replies |
| `kanban_task_add_deliverable` | delegate, operate | Post-step: record deliverable links |
| `kanban_task_move` | operate | Post-step: execute status transitions |
| `kanban_task_verify` | operate | Operator-confirmed Review pass only; nonempty evidence moves task to Done |
| `kanban_task_reopen` | operate | Post-step: reopen for rework |
| `kanban_task_comments_since` | operate | Pre-step: read incremental updates |
| `kanban_goal_judge` | operate | Record an evidence-grounded criterion result without substituting for operator confirmation |
| `kanban_goal_close` | operate | Native host ActionChoice only; submit goal identity and evidence, never an outcome |
| `kanban_goal_correct_outcome` | operate | Separate native human correction |

Board/task and create/judge/get/list goal tools are on `hkask-mcp-kata-kanban`; human closure/correction tools are native host tools, not MCP routes.

## Verification Evidence Tiers

`kanban_task_verify` evidence is tiered by oracle class — the same
provenance lattice as grounding-verify's provenance tiers and the
program-manager's definition of done. The verify-completion evaluator
classifies each criterion's evidence:

- `ran-and-pasted` — a command and its observed output, pasted verbatim
- `demonstrated` — the behavior exercised, its observed effect shown
- `asserted` — the worker's claim, without shown oracle output

A criterion is satisfied only by `ran-and-pasted` or `demonstrated`
evidence; `asserted` does not satisfy — the floor tier is never the
ceiling. Reduce the per-criterion oracle classes deterministically via `lisp_eval` gate **`KTM-2`**:
- form: `(and (> criteria_count 0) (= criteria_count (+ ran_and_pasted_count demonstrated_count)) (= asserted_count 0) (if (> demonstrated_count 0) "demonstrated" "ran-and-pasted"))`
- env: `{ "criteria_count": <total criteria>, "ran_and_pasted_count": <satisfied with ran-and-pasted>, "demonstrated_count": <satisfied with demonstrated>, "asserted_count": <classified as asserted> }`

When `KTM-2` evaluates to `"ran-and-pasted"` or `"demonstrated"`, the recorded evidence string carries that tier prefix (`[oracle: demonstrated] ...`), so a board reader sees what class of verification closed the task; when `false` (any `asserted` or unsatisfied criterion), verification fails. "Tests pass" without the pasted output is asserted; the pasted command and exit code are ran-and-pasted.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `decompose-tasks.j2` | Decompose a project into INVEST-compliant tasks with vertical slicing, dependencies, recomposition strategy, and acceptance criteria; emits `project_name`, `tasks`, and `recomposition`. Phase: decompose. |
| `review-tasks.j2` | Review decomposed tasks for INVEST compliance, completeness, and recomposition viability. Phase: decompose. Tasks in `modified` carry specific change recommendations — apply the changes and include the revised tasks in the accepted pool before creating tasks via `kanban_task_create` (a structural change re-enters review). |
| `configure-spawn.j2` | Configure spawn parameters: delegation level, skills, memory scope. Includes post-step instructions for the agent to call kanban_task_spawn. Phase: delegate. |
| `coordinate-agents.j2` | Read active-task comment threads and prepare actionable replies. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |
| `track-deliverables.j2` | Assess deliverables for completeness. Includes post-step instructions for the agent to call kanban_task_move and kanban_task_add_deliverable. Phase: operate. |
| `verify-completion.j2` | Assess evidence criterion by criterion; comment and return failed work without a pass signal, and call `kanban_task_verify` only after `KTM-2` passes and operator-confirmed Review success. |
| `escalate.j2` | Convert unresolved issues into human-operator-ready escalations. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |

To render a template, call the `render_template` tool with the template ref (e.g., `kanban-task-management/decompose-tasks`) and a context object with the required variables — each template's `[inference]` contract is the authoritative input list. The contract audit checks each template's contract-to-body agreement; cross-template producer→consumer seams (one step's output feeding the next step's input) are this skill's own chain to verify at render time.

## Regression case

All receipts executed live through `lisp_eval`, over `KTM-1`, `KTM-2`, and the sweep gate:

- `KTM-1` with `{ "has_project_description": true, "has_task_to_delegate": false, "has_board_id": false, "goal_confirmed": true, "advances_count": 2, "column_tasks": [1, 2], "wip_limit": 3 }` → `["decompose", true, true]`.
- `KTM-2` with `{ "criteria_count": 3, "ran_and_pasted_count": 2, "demonstrated_count": 1, "asserted_count": 0 }` → `"demonstrated"` (and `false` when `asserted_count > 0`).
- Empty unverified-review and stalled-in-progress lists → `true`.
- One unverified review card → `false`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- The board is the durable ledger: a task's state comes from `kanban_task_list` re-reads, never from the manager's memory. Every transition is confirmed by re-listing before it is reported.
- A criterion is satisfied only by `ran-and-pasted` or `demonstrated` evidence; `asserted` never satisfies (the floor tier is never the ceiling). A done task without its evidence tier recorded is unverified, not complete.
- `kanban_task_verify` runs only after the operator confirms a Review task's observed pass evidence; its nonempty `evidence` is the pass signal. A delegate's reported outcome is evidence for Review, never operator-confirmed completion.
- After `kanban_task_spawn`, read the spawn receipt and the delegate's report via `kanban_task_comments_since` for the same task ID; if no report has landed, leave the task open and surface the missing receipt. Never replay a spawn after an uncertain tool error, and never render a second execution prompt.
- The sweep gate `(and (= (length unverified_review) 0) (= (length stalled_in_progress) 0))` checks sweep obstacles only — it never proves the functional goal. Goal achievement is the operator's judgment.
- Board deletion happens only after every task is Done, deliverables are recorded outside the board, and the operator confirms closure; a board with any open task is kept and named. Never delete a board to make a sweep look clean.
- Before Decompose, an inferred goal is only a question; board creation stops until the operator confirms the target. `kanban_goal_create` runs once, with a stable idempotency key.

