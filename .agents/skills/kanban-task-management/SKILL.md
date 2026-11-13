---
name: kanban-task-management
core: true
description: "Unified kanban task management across the full task lifecycle. Decompose projects into INVEST-compliant tasks, delegate to subagents with spawn configuration, monitor boards, coordinate agents, verify completion, and escalate."
audience: user
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
- **Delegate**: Configure one spawn for a task, then read the spawn receipt
  and the delegate's report from the task's comment thread
  (`kanban_task_comments_since`). Comments and deliverable links are
  derived from that observed report; no second execution prompt runs.
  Route selection by initiating party: a caller holding the kata-kanban
  server grant (panel-driven, server-origin worktree delegation) uses
  `kanban_task_spawn`; an agent running inside a Zed thread is denied
  that route (crates/agent/src/thread.rs — until per-invocation identity
  is carried end-to-end (P2), native agents delegate through the bound
  `create_thread`/`spawn_agent` paths) and uses the builtin `spawn_agent`.
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
D (`KTM-3`, `lisp_eval` over the newly re-listed rows — it checks sweep
obstacles only, never proving the functional goal). A done task without its evidence tier recorded
is unverified, not complete.

## Instructions

Step 0 (deterministic triage and preflight, **`KTM-1`** via `lisp_eval`) reads the inputs themselves — the request's fields, the goal as the goal tools returned it, the board's columns as `kanban_board_list` returned them and the target column's tasks as `kanban_task_list` returned them — and derives the phase, goal readiness and the WIP state:
- form: `(begin (define nonblank (lambda (v) (and (stringp v) (> (length v) 0)))) (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define phase (cond ((nonblank project_description) "decompose") ((and (listp task_to_delegate) (not (is_null task_to_delegate))) "delegate") ((nonblank board_id) "operate") (t "missing_input"))) (define criteria (if (and (listp goal) (listp (assoc "criteria" goal))) (assoc "criteria" goal) (list))) (define goal-ready (and (listp goal) (nonblank (assoc "id" goal)) (> (length criteria) 0))) (define cols (if (listp board_columns) board_columns (list))) (define column (lambda (cs) (if (is_null cs) (list) (if (string= (text-of (car cs) "status") target_status) (car cs) (column (cdr cs)))))) (define limit (assoc "wip_limit" (column cols))) (define occupancy (if (listp column_tasks) (length column_tasks) 0)) (define wip (cond ((is_null (column cols)) "column_not_found") ((not (numberp limit)) "no_limit") ((< occupancy limit) "under_limit") (t "at_limit"))) (list (list "phase" phase) (list "goal_ready" goal-ready) (list "criteria" (length criteria)) (list "wip" wip) (list "occupancy" occupancy) (list "wip_limit" limit)))`
- env: `{ "project_description": <the request's project description, or "">, "task_to_delegate": <the task object to delegate, or null>, "board_id": <the board id, or "">, "goal": <the goal object from kanban_goal_create/kanban_goal_get, or null>, "board_columns": <the board's columns array from kanban_board_list>, "target_status": <the status the next write targets>, "column_tasks": <kanban_task_list rows for that status> }`
- `phase` is the first input present (`decompose`, `delegate`, `operate`, `missing_input`); `goal_ready` is true only when the goal object carries an `id` and at least one criterion — the `advances` citations come from those criteria verbatim; `wip` is `under_limit`, `at_limit` (do not add to the column), `no_limit` (the column declares none) or `column_not_found`. Nothing in the env is a count or a boolean the model typed.

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

### Native spawn_agent delegation (Zed-thread agents)

When the delegating agent runs inside a Zed thread, delegate via the
builtin `spawn_agent`. The spawned agent sees no conversation history,
so the delegation message is self-contained: the task identity
(`task_id`, board name + id — name for understanding, id for dispatch),
the task title and acceptance criteria verbatim, every file path,
requirement and constraint the worker needs, and the reporting contract.
`spawn_agent` returns only the worker's final message, so the delegator
posts the result to the task itself (`kanban_task_comment`,
`kanban_task_add_deliverable` for verified paths) — one writer per task
per batch. Follow-ups reuse the returned `session_id` with a short direct
message; never a duplicate session. `model` takes an exact
`list_agents_and_models` id and cannot be combined with `session_id`.
Never delegate goal judgment or closure (worker progress only —
GOAL_INTERFACE.md). Shapes that warrant a sub-agent: parallel
independent steps with disjoint write sets, independent
information-gathering, large-log validation runs where only the summary
matters, fresh-perspective reviews — not anything one or two tool calls
accomplish directly.

### Operate-phase sweep loop

One bounded PDCA applies to the active phase: Plan from the initial and target conditions above; Do only the admitted board/spawn/transition operations; Check by re-reading real tool state and the cited goal criteria; Act once on a named gap (complete a missing board write or return failed work for correction), or stop with the gap and operator decision needed. Do not replay a spawn after an uncertain tool error. For the operate phase specifically, close with a board sweep:

1. Re-run `kanban_task_list` — the Check signal is two lists: tasks in
   Review without verification evidence, and InProgress tasks with no
   activity since the last sweep (staleness threshold per the operator).
2. Act per finding: render `kanban-task-management/track-deliverables` with `task` and `deliverables` to assess deliverable readiness, and render `kanban-task-management/verify-completion` with `task_id`, `task_title`, `criteria`, `deliverables`, and `evidence` to classify per-criterion oracle tiers, then reduce the oracle-tier lattice deterministically via **`KTM-2`** (`lisp_eval`). Call `kanban_task_verify` only after `KTM-2` passes and the operator confirms a Review task's observed pass evidence; its nonempty `evidence` is the pass signal and moves that task to Done. Comment on a failed/unsupported submission without calling verify, and return it for rework — render `kanban-task-management/coordinate-agents` with `board_name` and `active_tasks` (the re-listed board's active tasks) to prepare the replies. Comment to unblock stalled tasks. Escalations render `kanban-task-management/escalate` with `board_name` and `escalation_candidates` (from coordinate-agents' output).
3. Re-list to confirm the transitions took.
4. Run the sweep gate **`KTM-3`** over the newly re-listed rows (full view — the compact view omits `activity`) and the stale list computed from them. Record `sweep_started_at` with `date -u +%FT%TZ` before step 1; the stale list is `jq -c --arg since <sweep_started_at> -f stale.jq` over `{"tasks": <the re-listed rows>}` with `stale.jq` saved verbatim from:

   ```jq
   [.tasks[] | select(.status == "in_progress" and .activity != null and .activity.at < $since) | .task_id]
   ```

   - form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (if (listp tasks) tasks (list))) (define unread (not (or (eq tasks (list)) (and (listp tasks) (not (is_null tasks)))))) (define in (lambda (status rs) (if (is_null rs) (list) (if (string= (text-of (car rs) "status") status) (cons (car rs) (in status (cdr rs))) (in status (cdr rs)))))) (define silent (lambda (rs) (if (is_null rs) (list) (if (is_null (assoc "activity" (car rs))) (cons (text-of (car rs) "task_id") (silent (cdr rs))) (silent (cdr rs)))))) (define ids (lambda (rs) (if (is_null rs) (list) (cons (text-of (car rs) "task_id") (ids (cdr rs)))))) (define keep-known (lambda (xs known) (if (is_null xs) (list) (if (member (car xs) known) (cons (car xs) (keep-known (cdr xs) known)) (keep-known (cdr xs) known))))) (define unverified (silent (in "review" rows))) (define stalled (keep-known (if (listp stale_in_progress) stale_in_progress (list)) (ids (in "in_progress" rows)))) (list (list "verdict" (cond (unread "board_unread") ((and (is_null unverified) (is_null stalled)) "sweep_clean") (t "escalate"))) (list "unverified_review" unverified) (list "stalled_in_progress" stalled) (list "cards_read" (length rows))))`
   - env: `{ "tasks": <the re-listed rows of every column, verbatim; null when a read failed>, "stale_in_progress": <the jq output> }`
   - `unverified_review` is every Review card with no activity at all (no evidence comment); `stalled_in_progress` is the stale list, restricted to ids still In Progress on the re-list. `sweep_clean` closes the sweep; `escalate` stops after this one sweep with the actual task IDs; `board_unread` reports the board as unknown, never clean. This gate checks sweep obstacles only; it does not prove the functional goal was achieved.
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
| `kanban_task_spawn` | delegate | Post-step: spawn subagent once — server-grant callers only (native Zed-thread agents use the builtin `spawn_agent`; the tool may require enabling in this session) |
| `spawn_agent` (builtin) | delegate | Native Zed-thread agents only: self-contained delegation message; results return via the delegator's `kanban_task_comment` |
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
ceiling. Reduce the render's per-criterion rows deterministically via `lisp_eval` gate **`KTM-2`**, over the `verify-completion` output as returned:
- form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (if (and (listp verification) (listp (assoc "criteria_results" verification))) (assoc "criteria_results" verification) (list))) (define classes (list "ran-and-pasted" "demonstrated" "asserted")) (define count-class (lambda (c rs) (if (is_null rs) 0 (+ (if (string= (text-of (car rs) "oracle_class") c) 1 0) (count-class c (cdr rs)))))) (define malformed (lambda (rs) (if (is_null rs) 0 (+ (if (member (text-of (car rs) "oracle_class") classes) 0 1) (malformed (cdr rs)))))) (define satisfied-but-asserted (lambda (rs) (if (is_null rs) 0 (+ (if (and (eq (assoc "satisfied" (car rs)) t) (string= (text-of (car rs) "oracle_class") "asserted")) 1 0) (satisfied-but-asserted (cdr rs)))))) (define unsatisfied (lambda (rs) (if (is_null rs) 0 (+ (if (eq (assoc "satisfied" (car rs)) t) 0 1) (unsatisfied (cdr rs)))))) (define n (length rows)) (define ran (count-class "ran-and-pasted" rows)) (define demo (count-class "demonstrated" rows)) (define asserted (count-class "asserted" rows)) (define bad (malformed rows)) (define tier (cond ((= n 0) "no_criteria") ((> bad 0) "malformed_rows") ((> (satisfied-but-asserted rows) 0) "asserted_marked_satisfied") ((> asserted 0) "asserted") ((> (unsatisfied rows) 0) "unsatisfied") ((> demo 0) "demonstrated") (t "ran-and-pasted"))) (define pass (member tier (list "demonstrated" "ran-and-pasted"))) (define claimed (eq (assoc "passed" verification) t)) (list (list "tier" tier) (list "pass" pass) (list "render_claim_agrees" (eq claimed pass)) (list "criteria" n) (list "ran_and_pasted" ran) (list "demonstrated" demo) (list "asserted" asserted) (list "malformed" bad)))`
- env: `{ "verification": <the verify-completion render output, unchanged> }`
- `tier` is `demonstrated` or `ran-and-pasted` (`pass` true) when every row is satisfied by one of those classes; otherwise `asserted`, `unsatisfied`, `asserted_marked_satisfied` (a row the render marked satisfied on asserted evidence — the render contradicts its own rule), `malformed_rows` (an `oracle_class` outside the three) or `no_criteria`, all `pass` false. `render_claim_agrees` is false when the render's `passed` disagrees with its rows; report that beside the tier.

When `pass` is true, the recorded evidence string carries the tier prefix (`[oracle: demonstrated] ...`), so a board reader sees what class of verification closed the task; when false, verification fails and the work returns. "Tests pass" without the pasted output is asserted; the pasted command and exit code are ran-and-pasted.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `decompose-tasks.j2` | Decompose a project into INVEST-compliant tasks with vertical slicing, dependencies, recomposition strategy, and acceptance criteria; emits `project_name`, `tasks`, and `recomposition`. Phase: decompose. |
| `review-tasks.j2` | Review decomposed tasks for INVEST compliance, completeness, and recomposition viability. Phase: decompose. Tasks in `modified` carry specific change recommendations — apply the changes and include the revised tasks in the accepted pool before creating tasks via `kanban_task_create` (a structural change re-enters review). |
| `configure-spawn.j2` | Choose the skills to delegate for one task. Includes post-step instructions for the agent to call kanban_task_spawn. Phase: delegate. |
| `coordinate-agents.j2` | Read active-task comment threads and prepare actionable replies. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |
| `track-deliverables.j2` | Assess deliverables for completeness. Includes post-step instructions for the agent to call kanban_task_move and kanban_task_add_deliverable. Phase: operate. |
| `verify-completion.j2` | Assess evidence criterion by criterion; comment and return failed work without a pass signal, and call `kanban_task_verify` only after `KTM-2` passes and operator-confirmed Review success. |
| `escalate.j2` | Convert unresolved issues into human-operator-ready escalations. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |

To render a template, call the `render_template` tool with the template ref (e.g., `kanban-task-management/decompose-tasks`) and a context object with the required variables — each template's `[inference]` contract is the authoritative input list. The contract audit checks each template's contract-to-body agreement; cross-template producer→consumer seams (one step's output feeding the next step's input) are this skill's own chain to verify at render time.

## Regression case

`bash kask/scripts/check-skill-lisp-forms.sh` evaluates the three authored gates (`scientific_gate_ktm_`): `KTM-1` over each phase input alone and none (`missing_input`), a null task object, a goal with and without criteria, a column at and under its limit, a column with no limit and a status not on the board; `KTM-2` over two ran-and-pasted plus one demonstrated row (`demonstrated`, pass), all ran-and-pasted, an asserted row, an asserted row the render marked satisfied, an unsatisfied ran-and-pasted row, a mis-cased class and no rows; `KTM-3` over a commented and an uncommented Review card with a stale and a fresh In Progress card (`escalate` naming both), an all-clean board, an empty board and a failed read. `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib kanban_task_management_` runs the authored `stale.jq` over a stale, a fresh, a silent and a Review card.

## Constraints

- The board is the durable ledger: a task's state comes from `kanban_task_list` re-reads, never from the manager's memory. Every transition is confirmed by re-listing before it is reported.
- A criterion is satisfied only by `ran-and-pasted` or `demonstrated` evidence; `asserted` never satisfies (the floor tier is never the ceiling). A done task without its evidence tier recorded is unverified, not complete.
- `kanban_task_verify` runs only after the operator confirms a Review task's observed pass evidence; its nonempty `evidence` is the pass signal. A delegate's reported outcome is evidence for Review, never operator-confirmed completion.
- After `kanban_task_spawn`, read the spawn receipt and the delegate's report via `kanban_task_comments_since` for the same task ID; if no report has landed, leave the task open and surface the missing receipt. Never replay a spawn after an uncertain tool error, and never render a second execution prompt.
- The sweep gate `KTM-3` checks sweep obstacles only — it never proves the functional goal. Goal achievement is the operator's judgment.
- Board deletion happens only after every task is Done, deliverables are recorded outside the board, and the operator confirms closure; a board with any open task is kept and named. Never delete a board to make a sweep look clean.
- Before Decompose, an inferred goal is only a question; board creation stops until the operator confirms the target. `kanban_goal_create` runs once, with a stable idempotency key.

