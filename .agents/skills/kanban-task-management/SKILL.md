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
- **Delegate**: Configure one spawn for a task, call `kanban_task_spawn`, then read its persisted `kanban_task_delegate_result`. Comments and deliverable links are derived from that observed result; no second execution prompt runs.
- **Operate**: Monitor the board, coordinate agents through comment threads,
  track deliverables, move tasks through columns, verify completion, and
  escalate to the human operator. The agent calls `kanban_task_list`,
  `kanban_task_move`, `kanban_task_verify`, `kanban_task_comment`, etc.

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

## Instructions

The first step (`triage.j2`) examines the available inputs and determines
which phase to run:

| Input present | Phase | Steps |
|---|---|---|
| `project_description` | decompose | gather-context → decompose-tasks → review-tasks → populate-board |
| `task_to_delegate` | delegate | configure-spawn → `kanban_task_spawn` → `kanban_task_delegate_result` |
| `board_id` | operate | monitor-board → coordinate-agents → track-deliverables → move-tasks → verify-completion → escalate |

Templates for non-active phases are not rendered. Before Decompose, confirm the operator's functional goal, 2–4 observable criteria and any existing goal identity; an inferred goal is only a question. When confirmed and no goal exists, call `kanban_goal_create` once with the operator's `goal_text`, criteria and a stable idempotency key; retain its `goal_id` and verbatim criterion text for `kanban_task_create.advances`. If the user has not confirmed the target, stop before board creation. The triage result names only the active phase.

### Reference models and labels

Anderson, *Kanban: Successful Evolutionary Change* (2010) — visualize work, limit WIP, manage flow; Wake, "INVEST in Good Stories" (2003) for task shape. Board state and every transition are D (the kanban tools are the oracle; `kanban_task_list` re-reads confirm a move took). Decomposition, delegation briefs and the judgment that Review evidence is sufficient are P, critiqued by `kanban_task_verify` evidence and by the operator, who owns acceptance.

### Delegate result handoff

After `kanban_task_spawn`, read `kanban_task_delegate_result` for the *same* task ID. If it is unavailable or failed, leave the task open and surface the missing receipt; do not render another execution prompt. Use the persisted response and any verified paths to add a deliverable or post a progress/blocker comment. A delegate's `task_success` verdict is evidence for Review, not operator-confirmed task completion. No second task record or model-produced execution report is created.

### Operate-phase sweep loop

One bounded PDCA applies to the active phase: Plan from the initial and target conditions above; Do only the admitted board/spawn/transition operations; Check by re-reading real tool state and the cited goal criteria; Act once on a named gap (complete a missing board write or return failed work for correction), or stop with the gap and operator decision needed. Do not replay a spawn after an uncertain tool error. For the operate phase specifically, close with a board sweep:

1. Re-run `kanban_task_list` — the Check signal is two lists: tasks in
   Review without verification evidence, and InProgress tasks with no
   activity since the last sweep (staleness threshold per the operator).
2. Act per finding: call `kanban_task_verify` only after the operator confirms a Review task's observed pass evidence; its nonempty `evidence` is the pass signal and moves that task to Done. Comment on a failed/unsupported submission without calling verify, and return it for rework. Comment to unblock stalled tasks.
3. Re-list to confirm the transitions took.
4. Reconcile the IDs in `unverified_review` and `stalled_in_progress` against the *newly re-listed board* before calling `lisp_eval` with `(and (= (length unverified_review) 0) (= (length stalled_in_progress) 0))`. This gate checks sweep obstacles only; it does not prove the functional goal was achieved. On a mismatch or a surviving blocker, stop after this one sweep and escalate with the actual task IDs.

## MCP Tools

| Tool | Phase | When |
|------|-------|------|
| `kanban_goal_create` | decompose | Persist the operator-confirmed functional target before board creation |
| `kanban_board_create` | decompose | Post-step: create the board |
| `kanban_task_create` | decompose | Create each accepted task with exact goal-criterion `advances` citations |
| `kanban_task_list` | decompose, operate | Post-step: verify / pre-step: fetch |
| `kanban_board_list` | operate | Pre-step: fetch board state |
| `kanban_task_spawn` | delegate | Post-step: spawn subagent once (tool may require enabling in this session) |
| `kanban_task_delegate_result` | delegate | Post-step: read structured result |
| `kanban_task_comment` | delegate, operate | Post-step: post progress notes / coordinator replies |
| `kanban_task_add_deliverable` | delegate, operate | Post-step: record deliverable links |
| `kanban_task_move` | operate | Post-step: execute status transitions |
| `kanban_task_verify` | operate | Operator-confirmed Review pass only; nonempty evidence moves task to Done |
| `kanban_task_reopen` | operate | Post-step: reopen for rework |
| `kanban_task_comments_since` | operate | Pre-step: read incremental updates |
| `kanban_goal_judge` | operate | Record an evidence-grounded criterion result without substituting for operator confirmation |
| `kanban_goal_score` | operate | Only after the operator supplies achieved/not-achieved ground truth |

All tools are on the `hkask-mcp-kata-kanban` server.

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
ceiling. The recorded evidence string carries the tier prefix
(`[oracle: demonstrated] ...`), so a board reader sees what class of
verification closed the task. "Tests pass" without the pasted output
is asserted; the pasted command and exit code are ran-and-pasted.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `triage.j2` | Triage step. Examines available inputs (project_description, task_to_delegate, board_id) and determines which phase to run: decompose, delegate, or operate. |
| `gather-context.j2` | Extract structured project context: project name, goals, constraints, resources, and target task size. Phase: decompose. |
| `decompose-tasks.j2` | Decompose a project into INVEST-compliant tasks with vertical slicing, dependencies, recomposition strategy, and acceptance criteria. Phase: decompose. |
| `review-tasks.j2` | Review decomposed tasks for INVEST compliance, completeness, and recomposition viability. Phase: decompose. |
| `populate-board.j2` | Convert accepted tasks into board-ready format. Includes post-step instructions for the agent to call kanban_board_create and kanban_task_create. Phase: decompose. |
| `configure-spawn.j2` | Configure spawn parameters: delegation level, skills, memory scope, timeout. Includes post-step instructions for the agent to call kanban_task_spawn. Phase: delegate. |

| `monitor-board.j2` | Monitor board state, identify blockers, flag overdue tasks. Includes pre-step instructions for the agent to fetch board data via kanban_board_list and kanban_task_list. Phase: operate. |
| `coordinate-agents.j2` | Read active-task comment threads and prepare actionable replies. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |
| `track-deliverables.j2` | Assess deliverables for completeness. Includes post-step instructions for the agent to call kanban_task_move and kanban_task_add_deliverable. Phase: operate. |
| `move-tasks.j2` | Recommend status transitions based on evidence. Includes post-step instructions for the agent to call kanban_task_move. Phase: operate. |
| `verify-completion.j2` | Assess evidence criterion by criterion; comment and return failed work without a pass signal, and call `kanban_task_verify` only after operator-confirmed Review success. |
| `escalate.j2` | Convert unresolved issues into human-operator-ready escalations. Includes post-step instructions for the agent to call kanban_task_comment. Phase: operate. |

To render a template, call the `render_template` tool with the template ref (e.g., `kanban-task-management/triage`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `coordinate-agents.j2`: `triage_phase`,`board_name` `active_tasks`
- `escalate.j2`: `triage_phase`,`board_name` `escalation_candidates`


## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
