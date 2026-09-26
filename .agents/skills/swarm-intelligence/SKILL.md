---
name: swarm-intelligence
core: true
description: "Convergent swarm-composition process. Senses swarm state, orients via Ashby requisite variety and PSO balance, decides composition adjustments (PSO velocity, ACO pheromone, Reynolds flocking), and acts via shared-thread local delegation or consent-gated ABW calls."
---

# Swarm Intelligence

Inform the composition, configuration, and operation of Agent Bestiary World
(ABW) agent swarms — how swarms are composed, how they are run, and how their
behavior is steered toward a target condition via an embedded PDCA loop.

## Substrate: ABW swarm semantics

A swarm is an ABW **workspace** with hired agents (not the deprecated
"ensemble" model). Compound agents orchestrate via `execute_agent` (text
consult, one LLM call) and `delegate_to_agent` (full tool access, 1 cr +
tokens, **delegation chains forbidden** — delegates lose
`delegate_to_agent`/`execute_agent`). Gas (verified live 2026-08-02): own-
agent hire via `/add` = flat 2 cr; third-party `/hire` = flat 5 cr base
(quote `total` = base + required + optional); @mention 1 cr + tokens,
delegation 1 cr + tokens. Compound agents declare
`dependencies { required,
optional }` and auto-hire their team. Thagard coherence scoring on
workspaces. (Verified live 2026-08-01.)

## Surface ontologies

- **Onto4MAT** (Kiesel et al. 2022): `Team { hasAlignmentWithTeam,
hasCohesionWithTeam, hasTeamSeparation, hasInfluence }`, `Agent { hasSpeed,
hasHeading, hasEnergy, hasDistanceToGoal }`, `Formation`. The measurable
  substrate for swarm state.
- **Reynolds (1987) flocking**: separation / alignment / cohesion — three
  local rules sufficient for emergent flocking. Scale-invariant (Labra 2026).
- **PSO** (Kennedy & Eberhart 1995): `v ← ωv + c1·r1·(pbest−x) + c2·r2·(gbest−x)`.
  c1 cognitive, c2 social, ω inertia. The tuning palette for composition.
- **ACO** (Dorigo 1992): pheromone deposition / evaporation (stigmergy).
- **W3C SSN/SOSA**: observable properties for swarm-state sensing.
- **Ashby requisite variety**; **cybernetic 5-property loop assessment**.

## When to Use

- Compose a new ABW swarm for a task (which agents to hire, what dependencies
  to satisfy)
- Author a new agent when no catalogue agent (or forkable near-match) covers a
  required transform — agent creation + authoring is a first-class `author_agent`
  move (generate prompt + ontology + create), not an implicit side-effect of hire
- Diagnose an existing swarm that is under-performing (variety deficit,
  coherence deficit, or a broken cost/consent feedback loop)
- Steer a swarm toward a target condition across iterations (the PDCA loop)
- Detect premature convergence (the swarm has collapsed onto a single agent
  type and lost exploration ability — the canonical PSO/ACO failure mode)
- Reconcile swarm spend against the ABW wallet (close the algedonic channel)

Do NOT use for:

- Dispatching a single ABW agent without composition intent (use the swarm
  panel directly)
- General multi-agent coordination theory outside ABW (the skill's vocabulary
  is ABW-specific: workspaces, hired agents, compound agents, dependencies)
- Curator (Xaman Ek) session management (that is a per-dispatch parameter of
  the ACT phase, not a separate skill)

## When NOT to Use

- Single-agent tasks — delegate directly; the SENSE→ORIENT→DECIDE→ACT→CHECK→CONVERGE loop buys nothing without a swarm to regulate.
- Executing a plan without its receipt check — in steering mode, run the "Steering a local swarm" loop below (directive → one `swarm_execute_plan_local` call → receipt check → feed back); never replay delegations or feed back an unverified array.
- Unbudgeted ABW spend calls — cloud delegations require `credits_authorized` and consent; local delegations have neither.

## Instructions

```
Check: Phase 1  — SENSE            → Measure current swarm state against Onto4MAT + backend workspace/wallet
Plan:  Phase 2  — ORIENT           → Classify the gap + deterministic fault attribution (C5). When a `swarm_id` is available, call `swarm_task_board` for durable task progress. Detect reasoning loops from the iteration log (C1), not from fields absent in local results.
Plan:  Phase 3  — DECIDE           → Propose composition adjustments isomorphic to PSO/ACO/Reynolds tuning
Det:   Phase 4  — FILTER           → Deterministically enforce C3 failed-edit + C7 influence guards (no LLM)
Do:    Phase 5  — ACT              → Emit consent-gated ABW calls or shared-thread local-swarm calls
Check: Phase 6  — CHECK             → Re-measure, compute swarm-state distance d, emit next_focus + algedonic
Check: Phase 7  — CONVERGE (check)  → evaluate d (swarm-state distance)
Check: Phase 8  — CONVERGE (accum) → Deterministic accumulator: iteration_log, failed_edits, influence_scores (C1/C3/C7)
Check: Phase 9  — CONVERGE (monitor)→ Second-order monitor: reasoning-loop + sensor-truth-divergence + Go See cadence (C1/C2)
Act:   Phase 10 — LOOP              → Re-enter SENSE with prior_iteration + threaded accumulators if not converged
```

The shape is cybernetic (sense → orient → decide → filter → act → check → converge),
not the gradient-hunter's Prior→Map→Detect→Hypothesize→Report (spatial-gradient
analysis) or the bug-hunt's Charter→Probe→Oracle→Taxonomize→Report
(exploratory testing). The shape emerges from the domain: a swarm is a
feedback loop, so the skill is a feedback loop. Maintain the cybernetic
plan's guards (FILTER, CONVERGE) across iterations — track running sets/sums
consistently in your reasoning across loop iterations.

### Authoring aid (single pass, outside the loop)

When the operator is filling the swarm panel's agent or swarm form, or DECIDE
proposes an `author_agent` move, render `swarm-intelligence/swarm-compose-guide`
with the partial fields and the `surface` (agent|swarm), `mode` (abw|local) and
`action` (suggest|validate) selectors. `suggest` returns field completions;
`validate` returns a verdict over the supplied fields. It is read-only: no
ledger debit, no consent. "Local" is an execution location, not a model tier —
leave the card model unset to inherit Settings → Kask → Models; never invent a
cheaper or smaller local model. The panel's own `swarm_ai_assist` tool runs
the deterministic ABW contract checks in code and its own inline prompt; it
does not render this template.

## Target condition (measurable)

A swarm is well-composed for a task when three conditions hold simultaneously:

1. **Requisite variety** (Ashby): `variety_coverage >= 0.9` — the hired
   agents' `accepts[]`/`produces[]` cover the task's required transforms.
2. **Coherence without premature convergence**: `diversity >= 0.25` (≥¼ of
   agents are non-identical) and Thagard coherence non-decreasing.
3. **Closed feedback loop**: `loop_closure = 1.0` — ABW spend reconciles
   against wallet transactions and curator consent; local delegation attempts
   have observed results or surfaced errors (no local credit ledger).

## Convergence criterion

Stability criterion on the swarm-state distance
`d = sqrt( (1 - variety_coverage)² + max(0, diversity_floor - diversity)² + (1 - loop_closure)² )`.
When the caller supplies a deterministic `task_success` verdict (component C0),
`d` gains a fourth axis `(1 - s)²` (s = task_success.score, or pass→1.0 /
fail→0.0) — a healthy swarm that fails the task must NOT converge. When
`task_success` is null (open tasks with no oracle), `d` uses the three
swarm-health axes only; the human Go See loop (C2) covers the task-success gap,
never an LLM judge.
Stability alone is a plateau, not success: four equal off-target distances cannot converge. The inputs (`variety_coverage`, `diversity`, `loop_closure`) are sensed/assessed in CHECK (P for judged components, critiqued by Go See); the distance and gate arithmetic are D via `lisp_eval`. A task-success target requires an actual deterministic or operator verdict; with no task oracle, report *swarm-health on target, task unassessed*, never user-task success:
- distance form: `(let ((base (+ (* (- 1 vc) (- 1 vc)) (let ((g (max 0 (- 0.25 div)))) (* g g)) (* (- 1 lc) (- 1 lc))))) (sqrt (if (is_null s) base (+ base (* (- 1 s) (- 1 s))))))`, env `{ "vc": <variety_coverage>, "div": <diversity>, "lc": <loop_closure>, "s": <task_success score, or null> }`
- target-plus-stability form: `(and (= (length ds) 4) (< (abs (- (nth 1 ds) (nth 0 ds))) 0.03) (< (abs (- (nth 2 ds) (nth 1 ds))) 0.03) (< (abs (- (nth 3 ds) (nth 2 ds))) 0.03) (>= vc 0.9) (>= div 0.25) (= lc 1) coherence_non_decreasing no_algedonic_alert (or (not task_success_required) (and (numberp s) (= s 1))))`. Bind `ds` to four observed distances, `vc`, `div`, `lc`, `s` to the latest observed inputs; `task_success_required` comes from the user's target, and the coherence/algedonic flags must have observed evidence. An unavailable flag is not `true`. Fewer than four measurements cannot converge. **Algedonic override:** a 402 or un-acknowledged
curator dispatch escalates regardless of `d` — a broken algedonic channel is
never read as "no deviation" (the `.rules` "unwrap_or(0)" trap enforced as a
convergence invariant).

## Cybernetic Swarm Plan components

The skill integrates the Cybernetic Swarm Plan's components (C0-C8).
Maintain accumulators and guards across iterations in your reasoning — running
sets/sums must be tracked consistently across loop iterations.

| Component                           | What                                                                                                                                                                                                            | Enforcement point                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **C0** task-success `s`             | Deterministic evaluator verdict → fourth axis of `d`                                                                                                                                                            | CHECK inputs from an observed evaluator/operator `task_success` receipt                                                                                                                                                                                                                                                                                                  |
| **C1** second-order monitor         | Reasoning-loop + sensor-truth-divergence detection over the iteration log; local delegation results do not carry reasoning steps. | `swarm.second_order_monitor` `lisp_eval` call (step 9)                                                                                                                                                                                                                                                                                          |
| **C2** Go See cadence               | Scheduled human check every N convergences + event trigger                                                                                                                                                      | `cadence_every` param in the monitor; SENSE surfaces `go_see`                                                                                                                                                                                                                                                                                   |
| **C3** failed-edit memory           | Anti-loop set; the FILTER drops moves matching prior failed signatures                                                                                                                                          | `swarm.filter_proposed_moves` `lisp_eval` call (step 4)                                                                                                                                                                                                                                                                                         |
| **C4** latency `T_q`                | End-to-end delegation latency measurement → ORIENT surfaces latency outliers → DECIDE reconfigures slow agents                                                                                                  | `LocalDelegateResult.latency_ms` → ORIENT `latency_outliers` → DECIDE `reconfigure_agent` (regulated, audit 2026-08-03; previously sensed but not acted on)                                                                                                                                                                                     |
| **C5** fault attribution            | Deterministic priority rule over the delegate trace — per-delegation `task_success` (highest fidelity) → whole-task terminal failure → `tool_calls[].ok`; fault-count aggregation | ORIENT template (rules 1-6) + `swarm.converge_accumulate` `fault_count`. `task_success` is the Loop B fidelity fix (audit 2026-08-03); `llm_judged` provenance is downgraded (Gap S3). Fires only when `delegate_results` execution telemetry is supplied (the planning process emits intents, not executed results). See Steering modes below. |
| **C6** reconfigure_agent            | Re-prompt a blamed agent in place (Modify-Block / MASS prompt axis)                                                                                                                                             | `swarm_reconfigure_local_agent` tool + DECIDE move type. Active only when C5 has fault telemetry (steering mode).                                                                                                                                                                                                                               |
| **C7** influence-weighted rejection | Reject re-hire of agent types measured to degrade the swarm                                                                                                                                                     | `swarm.filter_proposed_moves` `lisp_eval` call (step 4)                                                                                                                                                                                                                                                                                         |
| **C8** task-gated alignment         | Task-conditional edge relevance in SENSE (OFA-MAS TAGSE port)                                                                                                                                                   | SENSE template `alignment` definition                                                                                                                                                                                                                                                                                                           |

## Composed Skills

| Skill                   | Role                                                  | When Invoked                              |
| ----------------------- | ----------------------------------------------------- | ----------------------------------------- |
| `pragmatic-cybernetics` | 5-property loop assessment + Ashby variety + VSM      | ORIENT, when deficit is a loop-break      |
| `kata-improvement`      | Stability-check convergence pattern (the pinned stability form in Convergence criterion) | CONVERGE (`lisp_eval` call)               |
| `essentialist`          | Deletion-test proposed phases                         | Design-time (applied to the skill itself) |

## Steering modes (the execution boundary)

The swarm-intelligence process is a **planning loop** — it composes/steers the
swarm and emits a plan (`emitted_calls`), but it does not execute delegations
itself (no direct tool execution). The `steering_mode` context input governs
how the output is handled. A plan alone never closes the feedback loop:

- **advisory** (default): the plan IS the final output. The operator executes
  the emitted_calls manually and feeds `delegate_results` back on the next
  invocation (Option A — operator-in-the-loop).
- **steering**: the **Kask Curator** (local swarms) or **Xaman Ek** (cloud
  swarms) executes the plan and feeds results back autonomously (Option B).
  ACT emits a `steering_directive` the Curator acts on.

### Local swarms — the Kask Curator

The Kask Curator (`Agent::Curator`, `CURATOR_AGENT_ID`) is the in-process agent
that runs zed-kask — it has governed tool access (the MCP servers via
`McpRuntime`), its own sovereign memory, and the regulation/metacognition
loops. In steering mode, the Curator executes the plan by calling
`swarm_execute_plan_local` with only the delegation subset of `emitted_calls` (each with
an optional deterministic evaluator), which runs all delegations, evaluates
results, and returns a `results` array of successful `LocalDelegateResult`
entries (with evaluator verdicts) or error entries for failed attempts. The Curator re-invokes swarm-intelligence
with `delegate_results` set to that array — closing the feedback loop without
a new skill execution surface (the Curator's normal tool-call turn IS the
execution).

### Cloud swarms — Xaman Ek

Xaman Ek has steering **built in** (cloud-side). Delegate to it via
`swarm_xaman` with the plan as the message; Xaman Ek executes the plan and
steers the ABW swarm. The zed-kask side calls `swarm_xaman` with a steering-
style message (session_type `composition_design`); the execution + result
capture is Xaman Ek's built-in capability, then `delegate_results` flow back.

### Local swarms — the Kask Curator (continued)

Locally, the Kask Curator (or a human) steers by running the loop below on
the plan this skill emitted. It is this skill's execute-and-feed-back step
(formerly the separate `swarm-steering` skill, folded in 2026-09-25): it
sequences and collects, it never re-plans — DECIDE owns composition.

### Steering a local swarm — directive and receipt (PDCA)

PKO's specification/execution split: the plan is the Procedure; the
delegation array and its results are the StepExecutions.

- **Initial condition:** the emitted plan, an existing local `swarm_id`, the
  current roster, its ordered delegation entries (up to the tool's 10-entry
  cap), and any deterministic evaluators. A missing result is not a failed
  result.
- **Target condition:** each admitted delegation has one actual tool result
  or explicit per-entry error, in input order; only the returned `results`
  array becomes `delegate_results`.

1. **Plan (P):** take the delegation subset of `emitted_calls` and render
   `swarm-intelligence/swarm-steer-direct` with `task`, `swarm_id` and the
   unmodified plan. No delegations → report `empty` and make no call.
   Otherwise check 1–10 entries, roster membership and evaluator shape before
   labelling the directive `ready`. Rendering a directive is not executing it.
2. **Do (D):** call `swarm_execute_plan_local` once with that directive. If
   the tool errors instead of returning a complete `results` array, report
   the execution state unknown (earlier calls may have run); do not retry or
   fabricate receipts.
3. **Check (D):** derive `expected_names` from the submitted delegations and
   `observed_names`, in order, from each returned `agent_id` (success) or
   `agent_name` (error entry), then call `lisp_eval`:
   `(begin (define same-names (lambda (a b) (if (= (length a) 0) (= (length b) 0) (if (= (length b) 0) nil (and (string= (car a) (car b)) (same-names (cdr a) (cdr b))))))) (and (> (length expected_names) 0) (<= (length expected_names) 10) (same-names expected_names observed_names)))`.
   Keep per-entry `{agent_name, ok:false, error}` objects as errors. The
   tool's `succeeded` counts dispatches, not passed tasks; a
   `task_success.pass=false` is still a returned execution, and an absent
   `task_success` is unscored.
4. **Act:** a malformed *pre-execution* directive is corrected once and only
   the read-only preflight repeats. After execution, a count/order mismatch
   or tool error blocks feedback and needs investigation — never replay
   delegations. On a complete matched array, feed exactly `results` back as
   `delegate_results` on the next iteration.

Stamp a deterministic `task_success` per result when an evaluator exists;
leave it null for open tasks (the Go See loop covers them) — never
LLM-judge. In advisory mode, leave the plan to the operator without claiming
execution.

### The `delegate_results` contract (C5/C6 activation)

`delegate_results` is the `results` array from `swarm_execute_plan_local`.
Successful entries are `LocalDelegateResult`-shaped: `agent_id`, `response`, `model`, `tokens_used`,
`latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`), and `task_success`
(optional deterministic verdict). ORIENT attributes fault from
`delegate_results[].task_success.pass` (highest fidelity, when present) and
`delegate_results[].tool_calls[].ok`; `fault_count` accumulates; C6
reconfigures the most-blamed agent. Absent `delegate_results`, C5/C6 are inert.
Do not infer reasoning traces or skill executions from this result shape.

### The task board (durable task progress)

When `swarm_execute_plan_local` is called with a `swarm_id`, task progress
(status, attempt count, fail count, last result summary) is persisted to the
swarm's task board (`<swarm_dir>/<swarm_id>/task_board.json`). The
`swarm_task_board` MCP tool queries this board. ORIENT calls `swarm_task_board`
when a `swarm_id` is available to see durable task progress across PDCA
iterations ("task 3 failed twice, task 5 succeeded") without re-deriving it
from `delegate_results`. The task board's `all_complete()` / `all_terminal()`
flags are NOT convergence signals — convergence is the swarm-state distance
`d` (see Convergence criterion). The board is an ORIENT input for fault
attribution context, not a DECIDE input for convergence.

## Known limitations (audit 2026-08-03)

The [Swarm Cybernetics/Semantics Audit](../../../kask/docs/audits/swarm-cybernetics-semantics-audit.md)
found structural gaps; the two High-severity ones are now mitigated in the
registry + code (2026-08-03), with one residual:

- **Loop B fidelity — MITIGATED.** C5/C6 fault attribution now reads a
  per-delegation `task_success` (deterministic verdict stamped on
  each `LocalDelegateResult`) as its highest-fidelity signal, with the binary
  `tool_calls[].ok` as a fallback. `LocalDelegateResult` carries an optional
  `task_success: Option<TaskSuccessVerdict>`; `llm_judged` provenance is
  downgraded by ORIENT (Gap S3). **Residual:** open tasks with no oracle still
  rely on the Go See loop (C2) — the process cannot detect a healthy-but-wrong
  agent without a deterministic evaluator.
- **C4 latency — REGULATED.** `LocalDelegateResult.latency_ms` now flows through
  ORIENT (`latency_outliers`) into DECIDE, which proposes `reconfigure_agent`
  for outlier agents. The sense-without-act sub-loop is closed.
- **Loop A closure requires execution evidence.** Advisory is the safe default: the plan is the output and the operator may execute it. When the caller explicitly chooses `steering`, the Curator/human calls the local execution tool once, reconciles its returned `results` to the plan, and feeds that array into the next iteration. A rendered directive or former manifest description is not an execution receipt. A tool-level error leaves execution state unknown; do not automatically replay.

Full per-property evidence and the VSM/Ashby analysis are in the audit.

## Registry

This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

- Templates: `kask/registry/templates/swarm-intelligence/swarm-{sense,orient,decide,act,check,compose-guide}.j2`
- Reference: `kask/registry/templates/swarm-intelligence/swarm-patterns.yaml` (Rendering template — PSO/ACO/Reynolds/Onto4MAT tuning palette; not sent to the LLM)
- Process (15 steps:
  swarm_get_swarm + swarm_get_local_swarm (call directly) → SENSE → ORIENT → DECIDE →
  FILTER (swarm.filter_proposed_moves) → ACT → re-measure (call directly ×2) → CHECK →
  converge_accumulate → second_order_monitor → `lisp_eval` convergence signal → re-enter the cycle)

- Deterministic compute primitives: `swarm.converge_accumulate`,
  `swarm.second_order_monitor`, `swarm.filter_proposed_moves` (in
  the swarm compute primitives)
- MCP tool surface (consult current registration rather than a fixed count):
  - **ABW tools**: `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`,
    `swarm_list_apps`, `swarm_ontology_templates`, `swarm_execute_agent`,
    `swarm_hire_cost`, `swarm_request_consent`, `swarm_authorize_session`,
    `swarm_hire`, `swarm_delegate`, `swarm_delegate_and_wait`, `swarm_fanout`,
    `swarm_run_status`, `swarm_generate_prompt`, `swarm_generate_ontology`,
    `swarm_create_agent`, `swarm_create_swarm`, `swarm_xaman`, `swarm_create_app`,
    `swarm_fire` (roster removal, verified live), `swarm_delete_agent`,
    `swarm_delete_swarm`, `swarm_search_knowledge`, `swarm_publish_checks`,
    `swarm_publish_agent`, `swarm_fork_agent`.
  - **Local tools**: `swarm_delegate_local` (standalone),
    `swarm_delegate_in_thread_local` (member-scoped shared thread),
    `swarm_fanout_local`, `swarm_pipeline_local`,
    `swarm_execute_plan_local`, `swarm_a2a_send` (A2A protocol message, in-process),
    `swarm_a2a_card` (A2A Agent Card discovery), `swarm_list_local_agents`,
    `swarm_clone_to_local`, `swarm_push_to_cloud`, `swarm_remove_local`,
    `swarm_create_local_agent`, `swarm_reconfigure_local_agent` (C6),
    `swarm_create_local_swarm`, `swarm_list_local_swarms`,
    `swarm_get_local_swarm`, `swarm_delete_local_swarm`, `swarm_add_agent_local`,
    `swarm_remove_agent_local` (local-swarm lifecycle — local mode has explicit
    named swarms/rosters, not just an ephemeral session),
    `swarm_search_knowledge_local`, `swarm_generate_prompt_local`,
    `swarm_generate_ontology_local` (local knowledge analogs — search/generate
    over the operator's `hkask-memory` + local inference; no ABW; see
    [Local Knowledge Tools design](../../../kask/docs/diataxis/swarm_system/reference.md),
    `swarm_ai_assist` (authoring aid), `swarm_evaluate_local` (deterministic
    task-success evaluator — stamps a `TaskSuccessVerdict` with
    `provenance: Deterministic`), `swarm_eval_suite_local`,
    `swarm_eval_agent_local` (rollout harness), `swarm_task_board`,
    `swarm_workflow_check_local`, `swarm_run_workflow_local` (execute a
    declared workflow_template end-to-end), `swarm_observed_seams_local`
    (the observed delegation topology — edges recorded at dispatch, compared
    against declared ports), `swarm_fleet_digest_local` (the fleet's shape:
    categories and counts, never individual agents), `swarm_who_answers_local`
    (which agents accept a label, with the bespoke/cohort/universal reading),
    `swarm_select_agent_local` (rank candidates for a slot by measured stats).

    **Fleet facts are registry-first — never from memory** (fermi's
    `biotech_analyst` incident: a navigator answered three specific claims
    about an agent's model ladder, all false, all one lookup from being
    checked, because its sources were prose). Per-agent facts (model, ports,
    stats) come from `swarm_get_local_agent`; the fleet's shape comes from
    `swarm_fleet_digest_local`; who answers an ask comes from
    `swarm_who_answers_local`. Do not state them from memory. If a tool
    reports a different fleet size than a digest you hold, the digest is
    stale — say so rather than answering from it.
- Spend-mutating ABW tools (`swarm_hire`, `swarm_delegate`,
  `swarm_delegate_and_wait`, `swarm_fanout`, `swarm_execute_agent`,
  `swarm_create_swarm`,
  `swarm_xaman`) are consent-gated via `swarm_request_consent` (single-use,
  action+target+credits-scoped, TTL-bounded) or `swarm_authorize_session`
  (headless). In local mode there is no consent token, credit budget, or funding gate.
  See the [Swarm Systems Reference](../../../kask/docs/diataxis/swarm_system/reference.md)
  for the full tool/contract table and the token model.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `swarm-sense.j2` | Measure the current swarm state. In `abw` mode, fetch the ABW workspace roster and wallet; in `local` mode, read the member roster and local agent registry. Optionally probe an agent's consolidated knowledge graph via `swarm_search_knowledge` (fermi v0.10.26 — the embedder was broken platform-wide for 6 weeks and is now fixed) when the roster's accepts/produces is ambiguous about whether an agent covers a required transform. Compute Onto4MAT team properties: alignment (delegation-graph density from produces/accepts overlap), cohesion (fraction of required dependencies satisfied), separation (distinct (agent_type, model, temperature) tuples / agent count). Derive required_transforms from the task. Consumes prior_iteration.next_focus when present (feedback loop closure). Vacuous-truth defaults: empty required_transforms → variety_coverage 1.0 + trivial_task flag; empty swarm → diversity 0.0, variety 0.0. |
| `swarm-orient.j2` | Classify the gap between sensed state and target condition into one of three deficit classes (or on-target): (a) variety deficit — required_transforms not covered; (b) coherence deficit — Thagard coherence flat/declining OR diversity below floor (premature convergence, the PSO/ACO failure mode); (c) loop-break — un-reconciled dispatches or un-acknowledged curator data-sharing. Mode-agnostic (v2 §15): operates on the state shape from SENSE, not the data source. Delegates to pragmatic-cybernetics for the 5-property assessment (polarity, delay, gain, closure, fidelity) when the deficit is a loop-break. Classifies the coordination problem (Axelrod): cooperation (repeated, shadow of the future) vs division-of-labor (one-shot, complementary capabilities). |
| `swarm-decide.j2` | Propose measured-deficit responses using PSO/ACO/Reynolds as analogies, not executed numerical algorithms. Local composition uses named-swarm roster add/remove without deleting saved cards; ABW removal uses `swarm_fire` under the workspace contract. The one-level delegation invariant still applies. |
| `swarm-act.j2` | Prepare ABW consent-gated or local no-credit action intents. Rendering emits a plan, never an executed-call receipt; the invoking Curator/human runs the selected live tools and retains their results. |
| `swarm-check.j2` | Re-read post-Act state and observed receipts; local `loop_closure` covers both successful and explicit error entries in order, while a dispatch error remains an independent alert. Supply measured axes for the caller's `lisp_eval` distance/target gate. Zero actual delegations are vacuously receipted, but unexecuted plans are not. |
| `swarm-steer-direct.j2` | Steering a local swarm, step 1: from the plan and local swarm id, check member agents and produce one scoped `swarm_execute_plan_local` call with optional deterministic evaluators, the `LocalDelegateResult` collection shape, and the feedback instruction. |
| `swarm-compose-guide.j2` | Reference/authoring template for agent and swarm composition. Given a composition request (surface: agent|swarm, mode: abw|local, action: suggest|validate, plus partial fields), it produces either suggested completions for unfilled fields (action=suggest) or a validation verdict over the supplied fields (action=validate). Encodes the canonical field definitions (name, agent_type, description, system_prompt, mission, agents) and the ABW/local backend considerations (credit-cost consent-gated catalogue vs local filesystem registry without credits). Mirrors the inline prompt used by the `swarm_ai_assist` MCP tool — this template is the single source of truth the panel and the process share. Consumed by the DECIDE phase when it proposes an `author_agent` move (no catalogue agent covers a required transform); also available for standalone invocation from the swarm panel. |
| `swarm-patterns.yaml` | Reference: the swarm-algorithm tuning palette mapped onto ABW's hire/fire/delegate vocabulary (fire via `swarm_fire` — redundancy moves fire duplicates via `swarm_fire`, not flag-for-manual-pruning). PSO velocity terms (c1 cognitive, c2 social, omega inertia) → ABW composition moves. ACO pheromone deposition/evaporation → hire/fire-redundant. Reynolds separation/alignment/cohesion → diversity/coordination/coherence. Onto4MAT team properties as the measurable substrate. Includes the three deficit classes and their tuning responses, plus the canonical failure mode (premature convergence / diversity collapse) and its detection signal. |

To render a template, call the `render_template` tool with the template ref (e.g., `swarm-intelligence/swarm-sense`) and a context object with the required variables.

## Constraints

- Every ABW spend delegation carries an explicit `credits_authorized` and consent. Local member delegation carries `swarm_id`, not a credit budget.
- Task-success verdicts are deterministic (`swarm_evaluate_local` or a configured evaluator) or null — never LLM-judged; `llm_judged` provenance is downgraded by ORIENT.
- Algedonic override: a 402 or un-acknowledged curator dispatch escalates regardless of the convergence distance `d` — a broken algedonic channel is never read as "no deviation".
- Convergence requires |d_i − d_{i−1}| < 0.03 for 3 consecutive iterations; a healthy swarm that fails the task must NOT converge.
- Open tasks with no oracle: `task_success` stays null and the human Go See loop (C2) covers the gap — never an LLM judge.
