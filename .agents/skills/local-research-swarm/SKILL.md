---
name: local-research-swarm
description: Coordinate a project-sized, source-grounded research effort across a local agent roster using a kanban board for work state and scoped A2A handoffs for adaptive collaboration.
---

# Local Research Swarm

## Anchors and boundary

- **PKO Procedure / StepExecution**: the project plan and task criteria are specifications; an A2A reply is an execution observation, not evidence that the planned result is true. Use `onto_anchor` for task-specific domain terms before classifying them.
- **Kanban Guide (May 2025)**: define visible work items, started/finished policies, control work in progress, actively unblock items, and improve the workflow from observed results. https://kanbanguides.org/the-kanban-guide/
- **SEPIO has-supporting-evidence**: a claim is not grounded merely because an agent or provider cites a URL; inspect the original passage and record the supporting source and any failure to inspect it.

The coordinator is the actor executing this skill. The kanban board is the durable **project** task ledger. `swarm_thread_local` is the ordered member conversation; `swarm_task_board` is separate execution telemetry for `swarm_execute_plan_local`, NOT a mirror of the kanban board. Scoped `swarm_a2a_send` is an in-process message/response, NOT a board mutation or a hosted agent service. The coordinator reconciles the two surfaces after each result.

## When to Use

- A research project has at least two distinguishable transforms (for example, discover original sources and independently challenge claims), and new findings may change the next tasks or who does them.
- Start a new project from a stated research question, or resume an existing `board_id` and `swarm_id`.

## When NOT to Use

- One bounded lookup or a task already served by one research skill: call the research tools directly.
- Pure parallel independent jobs (`swarm_fanout_local`) or a fixed ordered transform (`swarm_pipeline_local`). This skill's reason to exist is evidence-driven reassignment and follow-up.
- Cloud ABW workspaces or Exa Agent. Do not use `swarm_hire`, `swarm_delegate`, or paid research endpoints as implicit fallbacks.

## Instructions

### 1. Define the project and its limits (Plan)

1. Capture the operator's research question, deliverable, deadline if any, allowed source scope, permitted external data sharing and model/provider spend, and completion criteria. If these limits are missing and dispatch could expose private data or spend money, ask before dispatch. `local` describes the orchestrator: a local agent card may select a cloud model and research tools can call external providers. Do not claim free or offline execution.
2. Call `kanban_board_list` and `swarm_list_local_swarms` before creating anything. For resume, require the caller's `board_id` and `swarm_id`, read them with `kanban_task_list` and `swarm_get_local_swarm`, and do not replace them. For new work, call `skill(name="kanban-task-management", task=<full question, deliverable and limits>)` and follow its **decompose** phase: define independently checkable tasks, criteria, source requirements, dependencies and recomposition; create one board with `kanban_board_create`, then `kanban_task_create` for the agreed tasks. The skill tool returns instructions; it does not execute those calls. Record both returned IDs. Prefer the standard Backlog/Ready/InProgress/Review/Done columns. Never call `kanban_task_spawn`: it launches a different subagent path rather than a member of this local swarm.
3. Render `local-research-swarm/charter` with the actual question, limits, board tasks and their criteria. Use its JSON shape to produce the research transforms and source-check plan. Inspect `swarm_list_local_agents`, `swarm_get_local_agent` and `swarm_a2a_card` for candidates, and `swarm_who_answers_local` / `swarm_select_agent_local` for registered ports when helpful. Do not invent a card, tool grant, model, or capability from its name. A worker that must find originals needs declared `research/web_search` and `research/web_extract`; a source checker must be able to inspect the original, either with its own declared research tools or from original passages supplied by the coordinator. If the requisite roles are missing, call `skill(name="swarm-intelligence", task=<local mode, required transforms, roster gap and limits>)` for composition guidance. If approved limits allow it, use `swarm_create_local_agent` for only the missing role, with `accepts=["text"]`, `produces=["text"]`, only verified necessary `mcp_tools`, an explicit evidence/failure system prompt, and `model=""` to inherit configured defaults; otherwise stop with the named gap. Re-read the created card. Do not clone or push to ABW. A missing tool permission is not solved by asking the model to try harder.
4. Call `swarm_create_local_swarm(name=<project>, mission=<question and quality boundary>, agents=<verified member ids>)` only when a suitable swarm does not already exist. Read back `swarm_get_local_swarm` and check every intended member is in its roster. Record `board_id`, `swarm_id`, task-to-agent mappings and source scope in task comments, not in a fabricated kanban assignee: `kanban_task_assign` claims a task for the authenticated caller, not an arbitrary agent. For each task, use `kanban_task_comment` to name its member, dependencies and intended evidence; the coordinator remains responsible for board transitions.

### 2. Pull and hand off bounded work (Do)

1. Call `kanban_task_list(board_id, status=null)` and `swarm_get_local_swarm(swarm_id)` on every sweep. Select at most the number of ready, dependency-clear items allowed by the project's stated WIP control. Move selected tasks to InProgress using `kanban_task_move` with the status accepted by the tool. Do not start blocked dependencies or dispatch to a nonmember.
2. For each selected task call `swarm_a2a_send(agent_name=<roster member>, swarm_id=<swarm_id>, message=<task id, original question, bounded task, expected artifacts, required exact source passages/URLs, allowed tools, limits and what to do on failure>, context_id=<same project context id when available>)`. Save the returned A2A Task/Artifact and the scoped thread record; do not confuse A2A Task IDs with kanban task IDs. Dispatch to a second member for a targeted challenge when the first result changes an assumption or produces a load-bearing claim. The coordinator can send the first member the critic's specific counterexample and request a correction; this is a contingent feedback loop, not a prewritten pipeline. If agents themselves exchange A2A messages, their cards must explicitly allowlist `swarm/swarm_a2a_send`; inspect the shared thread and reconcile any resulting work with the board.
3. On each result, call `kanban_task_comment` with a concise observation, member, tool/source failures, source URLs and next dependency. Attach a durable output with `kanban_task_add_deliverable` when one exists. Do not echo secret tokens or large private source texts onto the board. On timeout, invalid A2A response or tool failure, comment the classification and retain the task InProgress or blocked; never mark it Done. Use `curator_report_skill_use_issue(skill_name="local-research-swarm", tool_name=<failed tool>, step_ordinal=<step>, error=<observed error>, failure_origin=<controlled value>)` for unexpected MCP failures.

### 3. Reconcile evidence and replan (Check → Act)

1. Render `local-research-swarm/reconcile` with current board items, latest A2A response(s), source inspection results and task criteria. Check each load-bearing factual claim against an original retrieved passage with URL, date/period where relevant, and exact quoted support. Use `begin_research_run` with `run_id` on `web_search` / `web_extract` when the coordinator performs retrieval; use `evaluate_evidence` for corroboration where useful. An agent's `tool_calls.ok` or `output.grounding` is not independent verification. If original text is inaccessible, mark `not_checked` or `blocked`; a search hit or model summary is not original evidence. Do not invent failed inspection as negative evidence.
2. Decide from **observed** results: if criteria are supported, move the task to Review and append the exact checked evidence. If a new question or contradiction changes the project, create a kanban follow-up task with `kanban_task_create` (and explicit parent/task link in its description or comment), reprioritize Ready items, and use a targeted `swarm_a2a_send` to a different member. If a worker fails, inspect `swarm_thread_local` and card/tool failures; reassign by board comment and scoped send, or stop/escalate rather than repeating a broken call. Never pretend `swarm_execute_plan_local`'s task board synchronizes with kanban.
3. For Review tasks, follow `skill(name="kanban-task-management", task=<board_id and review evidence>)`'s **operate** phase. Check criteria yourself against the originals before calling `kanban_task_verify`. This API treats any non-empty evidence as a pass and can move a Review item to Done; it does not enforce evidentiary quality. Record `[oracle: demonstrated]` with source URL and exact checked passage (or a genuine command plus output), not the worker's assertion. If checking is impossible, leave in Review with `not_checked` and ask for source access or operator judgment; do not call verify. A finished board is not itself a verified research report.
4. Call `lisp_eval` with form `(and (= open 0) (= unverified 0) (= blocked 0))` and env `{ "open": <count of Backlog/Ready/InProgress tasks>, "unverified": <count of Review tasks without checked original evidence>, "blocked": <count of unresolved gaps> }`. Count from the latest `kanban_task_list` and evidence record, not a model's optimistic prose. The improvement signal is the reduction in unsupported claims and unresolved work between sweeps. If false, Act: re-enter step 2 using new board state, provided there is a justified next discriminating action; otherwise stop and return an incomplete report. Bound to three no-new-evidence sweeps per invocation; do not repeat a failing tool call indefinitely. If true, report task IDs, independently checked citations, retained gaps, model/provider usage if available, and board/swarm IDs for resumption. The operator decides whether the project's functional goal is achieved.

## Registry Templates

| Template | Purpose |
|---|---|
| `charter.j2` | Map the project and real kanban tasks into bounded research transforms, source checks, roster needs, and a WIP policy. |
| `reconcile.j2` | Reconcile observed A2A work with board criteria and original source evidence, selecting contingent follow-up or review without claiming unsupported completion. |

## Constraints

- Kanban is authoritative for project state; the local shared thread carries member messages; neither automatically updates the other. Preserve the actual IDs and status values returned by tools.
- The coordinator must not route to an unverified agent card. Use only registered `accepts`/`produces` ports and real `mcp_tools`. Local cloud inference and research-provider costs still require the operator's limits.
- Record each independent source check separately from each worker claim. A deterministic `contains` evaluator is only a format check, never a truth oracle.
- Do not create accounts, invoke ABW, or enable Exa Agent/Ultra as part of this skill. Do not dispatch provider calls when permission or spending limits are unresolved.
