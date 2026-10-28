---
name: swarm-intelligence
core: true
description: "Convergent swarm-composition process. Senses swarm state, orients via Ashby requisite variety and PSO balance, decides composition adjustments (PSO velocity, ACO pheromone, Reynolds flocking), and acts via shared-thread local delegation across named local swarms and local swarm agents."
---

# Swarm Intelligence

Inform the composition, configuration, and operation of local agent swarms
— how swarms are composed, how they are run, and how their
behavior is steered toward a target condition via an embedded PDCA loop.

## Substrate: local swarm semantics

A local swarm is a named roster of local agent cards
(`agents/local/curated/<id>/agent_card.json`) with a mission and a durable
ordered thread (`swarm_thread_local`). Local agents run on the host
session's model (a card may pin one; unpinned cards resolve the default at
execution time — never assume a cheaper or smaller model). Cards declare
`accepts[]`/`produces[]` port labels (typing references, not free strings —
see `port_registry.rs`), optional `dependencies { required, optional }`, an
optional `workflow_template`, and optional `evaluators` (deterministic
contains/not_contains/regex contracts). There is no credit ledger, no
consent token, and no spend gate — dispatch is bounded instead by the
roster membership, the tool's 10-entry batch cap, and the card's
`mcp_tools` allowlist (filtered against the governed server set).

## Surface ontologies

- **Onto4MAT** — Hepworth, Baxter & Abbass (2022, arXiv:2203.12955): `Team { hasAlignmentWithTeam,
hasCohesionWithTeam, hasTeamSeparation, hasInfluence }`, `Agent { hasSpeed,
hasHeading, hasEnergy, hasDistanceToGoal }`, `Formation`. The measurable
  substrate for swarm state.
- **Reynolds (1987) flocking**: separation / alignment / cohesion — three
  local rules sufficient for emergent flocking.
- **PSO** (Kennedy & Eberhart 1995): `v ← ωv + c1·r1·(pbest−x) + c2·r2·(gbest−x)`.
  c1 cognitive, c2 social, ω inertia. The tuning palette for composition.
- **ACO** (Dorigo 1992): pheromone deposition / evaporation (stigmergy).
- **W3C SSN/SOSA**: observable properties for swarm-state sensing.
- **Ashby requisite variety**; **cybernetic 5-property loop assessment**.

## D/P labelling

SENSE, ORIENT and DECIDE judgments (deficit classification, composition moves,
tuning choices) are P — critiqued by the Go See loop (human observation) and
by the observed receipts in CHECK. The pinned arithmetic is D: the `SWM-1`
variety_coverage, diversity, cohesion, separation, and per_agent_metrics
computations in SENSE, the `SWM-2` deficit-classification, latency-outlier
(> max(2x median, 1000)), and C5 fault-attribution priority rule in ORIENT, the `SWM-3`
local steering preflight check, the convergence distance and target gate, the
receipt-coverage check, the loop_closure ratio, the failed-signature comparison,
and the C7 influence guard all run in `lisp_eval` over sensed state and actual
receipts — the model never supplies the convergence score.
- `SWM-1` (`lisp_eval` SENSE metrics — the caller runs it after the SENSE render's Step 2): `(begin (define inter (lambda (a b) (if (is_null a) 0 (if (member (car a) b) (+ 1 (inter (cdr a) b)) (inter (cdr a) b))))) (define jac (lambda (x y) (let ((i (inter x y)) (u (- (+ (length x) (length y)) (inter x y)))) (if (= u 0) 0.0 (/ i u))))) (define pair-rows (lambda (a bs) (if (is_null bs) (list) (let ((j (jac (nth 1 a) (nth 1 (car bs))))) (if (>= j 0.75) (cons (list (nth 0 a) (nth 0 (car bs)) j) (pair-rows a (cdr bs))) (pair-rows a (cdr bs))))))) (define all-pairs (lambda (as) (if (is_null as) (list) (append (pair-rows (car as) (cdr as)) (all-pairs (cdr as)))))) (let ((vc (if (= req_count 0) 1.0 (if (= agent_count 0) 0.0 (/ cov_count req_count)))) (div (if (= agent_count 0) 0.0 (/ distinct_type_model_count agent_count))) (coh (if (= dep_count 0) 1.0 (/ sat_dep_count dep_count))) (sep (if (= agent_count 0) 0.0 (/ distinct_tuple_count agent_count)))) (list vc div coh sep (= req_count 0) (all-pairs ports_by_agent))))`, env `{ "req_count": <|required_transforms|>, "cov_count": <|required_transforms ∩ covered_transforms|>, "agent_count": <|agents|>, "distinct_type_model_count": <|distinct (agent_type, model)|>, "dep_count": <|required dependencies|>, "sat_dep_count": <|satisfied dependencies|>, "distinct_tuple_count": <|distinct (agent_type, model, temperature)|>, "ports_by_agent": <|per-agent port labels [[agent_id, [accepts ∪ produces]], ...] in roster order|> }` — returns `(variety_coverage diversity cohesion separation trivial_task redundancy_pairs)`; each `redundancy_pairs` entry is `[agent_a, agent_b, jaccard]` for an unordered pair with `jaccard >= 0.75`, in roster order (the C7 substitutability signal).
- `SWM-2` (`lisp_eval` ORIENT deficit cross-check, latency outliers, and C5 fault attribution — the caller runs it after the ORIENT render): `(begin (define excluded (lambda (r) (or (not (nth 1 r)) (string= (nth 2 r) "invalid") (string= (nth 3 r) "eval")))) (define adm (lambda (rs) (if (is_null rs) (list) (if (excluded (car rs)) (adm (cdr rs)) (cons (car rs) (adm (cdr rs))))))) (define first-trusted-fail (lambda (rs) (if (is_null rs) (list) (if (nth 4 (car rs)) (car rs) (first-trusted-fail (cdr rs)))))) (define first-exec-fail (lambda (rs) (if (is_null rs) (list) (if (nth 5 (car rs)) (car rs) (first-exec-fail (cdr rs)))))) (define last-of (lambda (rs) (if (is_null (cdr rs)) (car rs) (last-of (cdr rs))))) (define outliers (lambda (xs thr) (if (is_null xs) (list) (if (> (nth 1 (car xs)) thr) (cons (car xs) (outliers (cdr xs) thr)) (outliers (cdr xs) thr))))) (let ((deficit (cond ((< lc 1.0) "loop_break") ((< vc 0.9) "variety_deficit") ((or (< div 0.25) (not coherence_non_decreasing)) "coherence_deficit") (t "on_target"))) (a (adm delegate_records))) (let ((r1 (first-trusted-fail a)) (r3 (first-exec-fail a))) (list deficit (outliers agent_latencies (max (* 2 median_latency) 1000)) (cond ((not (is_null r1)) (list (nth 0 r1) 1)) ((and whole_task_failed (> (length a) 0)) (list (nth 0 (last-of a)) 2)) ((not (is_null r3)) (list (nth 0 r3) 3)) (t (list)))))))`, env `{ "vc": <variety_coverage>, "div": <diversity>, "lc": <loop_closure>, "coherence_non_decreasing": <bool>, "agent_latencies": <[[agent_id, latency_ms], ...]>, "median_latency": <median latency_ms>, "delegate_records": <[[agent_id, bind_matched, input_contract_check, route_reason, trusted_task_failed, exec_failed], ...] in delegate_results array order — trusted_task_failed only for provenance "deterministic_evaluator"/"operator" with pass = false (the Gap S3 guard: llm_judged is never trusted); exec_failed for an explicit per-entry error or any tool_calls[].ok = false>, "whole_task_failed": <bool — the whole-task task_success input is present with trusted provenance and pass = false> }` — returns `(deficit latency_outliers fault)`: `latency_outliers` flags entries with `latency_ms > max(2 * median_ms, 1000)` (C4); `fault` is `(list)` (no attribution — C5/C6 inert) or `[agent_id, rule]` by the 4-rule priority over the admissible entries (1: earliest trusted per-delegation `task_success.pass = false`; 2: terminal admissible agent on whole-task failure; 3: earliest explicit error / `tool_calls[].ok = false`; 4: array-order tie-break). An entry is inadmissible — routing-attributable, never C6 blame — when `bind_matched = false`, `input_contract_check = "invalid"`, or `dispatch.route_reason = "eval"`.
ACT renders
intents, never executed receipts; execution comes only from the live local
dispatch tools, and a rendered plan is
not a swarm state change. A task-success target requires an actual
deterministic or operator verdict — with no task oracle, report swarm-health
on target, task unassessed.

## When to Use

- Compose a new local swarm for a task (which agents to add, what dependencies
  to satisfy)
- Author a new agent when no existing card (or a near-match) covers a
  required transform — agent creation + authoring is a first-class `author_agent`
  move (generate prompt + ontology + create), not an implicit side-effect of roster add
- Diagnose an existing swarm that is under-performing (variety deficit,
  coherence deficit, or a broken feedback loop)
- Steer a swarm toward a target condition across iterations (the PDCA loop)
- Detect premature convergence (the swarm has collapsed onto a single agent
  type and lost exploration ability — the canonical PSO/ACO failure mode)
- Reconcile delegated dispatches against observed receipts (close the
  feedback loop)

Do NOT use for:

- Dispatching a single local agent without composition intent (use
  `swarm_delegate_local` directly)
- General multi-agent coordination theory outside the local substrate (the
  skill's vocabulary is substrate-specific: named swarms, rosters, port
  labels, dependency declarations)
- Panel form authoring assistance outside the ACT/DECIDE author_agent flow
  (the swarm panel's own `swarm_ai_assist` tool covers standalone form help)

## When NOT to Use

- Single-agent tasks — delegate directly; the SENSE→ORIENT→DECIDE→ACT→CHECK→CONVERGE loop buys nothing without a swarm to regulate.
- Executing a plan without its receipt check — in steering mode, run the "Steering a local swarm" loop below (directive → one `swarm_execute_plan_local` call → receipt check → feed back); never replay delegations or feed back an unverified array.
- Inventing dispatch authority — local dispatch is bounded by roster membership and the card's declared allowlists; a plan may not dispatch a non-member or a tool outside the governed set.

## Instructions

```
Check: Phase 1  — SENSE            → Measure current swarm state against Onto4MAT + the local roster. Optionally pass `kanban_board_id` when a kanban board is this swarm's durable coordination surface — SENSE grounds its sensing in the board's persisted task state via `kanban_task_list`. Pass the already-read roster as `fetched_swarm_state` when the live-process roster read has run — SENSE then does not re-fetch.
Plan:  Phase 2  — ORIENT           → Classify the gap + deterministic fault attribution (C5). When a `swarm_id` is available, call `swarm_task_board` for durable task progress. Detect reasoning loops from the iteration log (C1), not from fields absent in local results.
Plan:  Phase 3  — DECIDE           → Propose composition adjustments isomorphic to PSO/ACO/Reynolds tuning
Det:   Phase 4  — FILTER           → Deterministically enforce C3 failed-edit + C7 influence guards (no LLM)
Do:    Phase 5  — ACT              → Emit shared-thread local-swarm action intents
Check: Phase 6  — CHECK             → Re-measure, compute swarm-state distance d, emit next_focus + algedonic. Pass the pre-Act sensed state as `pre_act_state` and the post-Act re-read as `fetched_post_act_state` (CHECK re-reads through them rather than re-fetching).
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
with the partial fields and the selectors:

```json
{
  "surface": "agent|swarm",
  "action": "suggest|validate",
  "name": "<string> | null",
  "agent_type": "<string> | null",
  "description": "<string> | null",
  "system_prompt": "<string> | null",
  "mission": "<string> | null",
  "agents": "<string> | null"
}
```

`suggest` returns field completions;
`validate` returns a verdict over the supplied fields. It is read-only. "Local"
is an execution location, not a model tier — leave the card model unset to
inherit Settings → Kask → Models; never invent a cheaper or smaller local
model. The panel's own `swarm_ai_assist` tool runs the deterministic local
contract checks in code and its own inline prompt; it does not render this
template.

## Target condition (measurable)

A swarm is well-composed for a task when three conditions hold simultaneously:

1. **Requisite variety** (Ashby): `variety_coverage >= 0.9` — the hired
   agents' `accepts[]`/`produces[]` cover the task's required transforms.
2. **Coherence without premature convergence**: `diversity >= 0.25` (≥¼ of
   agents are non-identical) and Thagard coherence non-decreasing.
3. **Closed feedback loop**: `loop_closure = 1.0` — local delegation attempts
   have observed results or surfaced errors (every admitted delegation has
   one actual receipt; no local credit ledger exists).

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
- target-plus-stability form: `(and (= (length ds) 4) (< (abs (- (nth 1 ds) (nth 0 ds))) 0.03) (< (abs (- (nth 2 ds) (nth 1 ds))) 0.03) (< (abs (- (nth 3 ds) (nth 2 ds))) 0.03) (>= vc 0.9) (>= div 0.25) (= lc 1) coherence_non_decreasing no_algedonic_alert (or (not task_success_required) (and (numberp s) (= s 1))))`. Bind `ds` to four observed distances, `vc`, `div`, `lc`, `s` to the latest observed inputs; `task_success_required` comes from the user's target, and the coherence/algedonic flags must have observed evidence. Any returned per-entry error or tool-level error forces `no_algedonic_alert = false` (and a tool-level error leaves `lc` unmeasured). An unavailable flag is not `true`. Fewer than four measurements cannot converge. **Algedonic override:** a tool-level dispatch error or an un-reconciled emitted dispatch escalates regardless of `d` — a broken feedback channel is
never read as "no deviation" (the `.rules` "unwrap_or(0)" trap enforced as a
convergence invariant).

## Cybernetic Swarm Plan components

The skill integrates the Cybernetic Swarm Plan's components (C0-C8).
Maintain accumulators and guards across iterations in your reasoning — running
sets/sums must be tracked consistently across loop iterations.

| Component                           | What                                                                                                                                                                                                            | Enforcement point                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **C0** task-success `s`             | Deterministic evaluator verdict → fourth axis of `d`                                                                                                                                                            | CHECK inputs from an observed evaluator/operator `task_success` receipt                                                                                                                                                                                                                                                                                                  |
| **C1** second-order monitor | Inspect actual `reasoning_steps` when enabled and iteration/Go See observations; missing traces stay unavailable | Human/Curator review of recorded evidence; no registered second-order monitor tool. |
| **C2** Go See cadence | Human review after a task failure, missing oracle or operator-chosen cadence | Surface a Go See request; no automatic `cadence_every` monitor is registered. |
| **C3** failed-edit memory | Prior failed signatures block an identical repeat | Caller compares proposed signatures to recorded failures with `lisp_eval` before ACT; an absent history is not proof of safety. Pinned form: `(not (member proposed_signature failed_signatures))`, env `{ "proposed_signature": <the proposed move's signature: decision_action + "|" + swarm_state_signature, where swarm_state_signature is deficit_class + "|" + sorted(roster agent_types) — the same key scheme failed_edits records>, "failed_signatures": <derived from failed_edits: each entry's decision_action + "|" + its swarm_state_signature> }` — `false` blocks the move; an empty history returns `true` (not blocked by C3) but is not proof of safety. |
| **C4** latency `T_q`                | End-to-end delegation latency measurement → ORIENT surfaces latency outliers → DECIDE reconfigures slow agents                                                                                                  | `LocalDelegateResult.latency_ms` → the `SWM-2` gate's `latency_outliers` (ORIENT phase) → DECIDE `reconfigure_agent` (regulated, audit 2026-08-03; previously sensed but not acted on)                                                                                                                                                                                     |
| **C5** fault attribution            | Deterministic priority rule over the delegate trace — per-delegation `task_success` (highest fidelity) → whole-task terminal failure → `tool_calls[].ok`; fault-count aggregation | the caller's `SWM-2` gate (rules 1–3 plus array-order tie-break, with the routing-attributability exclusions) plus fault counts reconciled from actual receipts or `swarm_task_board`; no accumulator tool. `task_success` is the Loop B fidelity fix (audit 2026-08-03); `llm_judged` provenance is downgraded (Gap S3). Fires only when `delegate_results` execution telemetry is supplied (the planning process emits intents, not executed results). See Steering modes below. |
| **C6** reconfigure_agent            | Re-prompt a blamed agent in place (Modify-Block / MASS prompt axis)                                                                                                                                             | `swarm_reconfigure_local_agent` tool + DECIDE move type. Active only when C5 has fault telemetry (steering mode).                                                                                                                                                                                                                               |
| **C7** influence-weighted rejection | Reject re-hire only when observed outcomes establish negative influence | Caller checks measured influence with `lisp_eval` before ACT. Pinned form: `(or (not (assoc agent_type influence_scores)) (> (assoc agent_type influence_scores) 0))`, env `{ "agent_type": <the proposed hire's agent_type>, "influence_scores": <the measured influence scores keyed by agent_type> }` — `false` rejects the hire (measured negative influence); an absent score returns `true` (unknown history is not deterministic rejection). |
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
- **steering**: the **Kask Curator** (or the human operator) executes
  the plan and feeds results back autonomously (Option B).
  ACT emits a `steering_directive` the Curator acts on.

### The Kask Curator — the local executor

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

### Steering a local swarm — directive and receipt (PDCA)

PKO's specification/execution split: the plan is the Procedure; the
delegation array and its results are the StepExecutions. The Curator (or the
human) steers by running the loop below on the plan this skill emitted — the
execute-and-feed-back step (formerly the separate swarm-steering skill,
folded in 2026-09-25): it sequences and collects, it never re-plans — DECIDE
owns composition.

- **Initial condition:** the emitted plan, an existing local `swarm_id`, the
  current roster, its ordered delegation entries (up to the tool's 10-entry
  cap), and any deterministic evaluators. A missing result is not a failed
  result.
- **Target condition:** each admitted delegation has one actual tool result
  or explicit per-entry error, in input order; only the returned `results`
  array becomes `delegate_results`.

1. **Preflight (`SWM-3`, D):** take the delegation subset of `emitted_calls` from `swarm-act.j2`'s `steering_directive` and validate 1–10 entries, roster membership, and evaluator shape deterministically with `lisp_eval` (`SWM-3` — local steering preflight; replaces the deleted `swarm-steer-direct.j2` wrapper template):
   - form: `(begin (define all-members (lambda (ds r) (if (is_null ds) t (and (member (assoc "agent_name" (car ds)) r) (all-members (cdr ds) r))))) (define valid-evals (lambda (ds) (if (is_null ds) t (let ((ev (assoc "evaluator" (car ds)))) (and (or (is_null ev) (and (member (assoc "evaluator" ev) (list "contains" "not_contains" "regex")) (> (length (assoc "spec" ev)) 0))) (valid-evals (cdr ds))))))) (if (= (length delegations) 0) "empty" (if (and (<= (length delegations) 10) (all-members delegations roster) (valid-evals delegations)) "ready" "invalid")))`
   - env: `{ "delegations": <delegate subset of emitted_calls>, "roster": <member agent ids in swarm_id> }`
   `"empty"` → make no call; `"invalid"` → stop and fix preflight before calling the tool.
2. **Do (D):** when `SWM-3` returns `"ready"`, call `swarm_execute_plan_local` once with `swarm_id` and `delegations` from `swarm-act.j2`'s `steering_directive`. If
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
`latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`), `task_success`
(optional evaluator verdict — a contains/regex response match describes the
response, not whether the task was accomplished), and the contract/telemetry
fields when applicable (`bind_matched`, `rollout_id`, `reasoning_steps`,
`input_contract_check`, `output_contract_check`, `memory`), plus `dispatch` — the dispatch-provenance stamp
(`{route_reason, question_source}`, fermi `stamp_invocation` port: which
surface dispatched and where the task came from; carries no quality
judgment). The caller's `SWM-2` gate (ORIENT phase) attributes fault from
`delegate_results[].task_success.pass` (highest fidelity, when present) and
`delegate_results[].tool_calls[].ok`; `fault_count` accumulates; C6
reconfigures the most-blamed agent — **but only on agent-attributable
blame**. A failed result whose `bind_matched` is `false` (or whose
`input_contract_check` is `invalid`) is routing-attributable: the wrong
question was sent to that agent, and the fix is the composition (the plan
entry, the roster, the router), not the agent's prompt. The `dispatch`
stamp names the surface that sent it (`plan`, `fanout`, `pipeline`,
`workflow`, `direct`, `thread`, `a2a`, `eval`) so a routing-attributable
pattern is traceable to its dispatching surface. Absent `delegate_results`,
C5/C6 are inert. Results stamped `route_reason: "eval"` are rollout-harness
measurements — they feed agent stats, never C6 blame.
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

The 2026-08-03 Swarm Cybernetics/Semantics Audit found structural gaps; the
two High-severity ones are now mitigated in the
registry + code (2026-08-03), with one residual:

- **Loop B fidelity — MITIGATED.** C5/C6 fault attribution now reads a
  per-delegation `task_success` (the deterministic evaluator's response-match
  verdict stamped on each `LocalDelegateResult`) as its highest-fidelity signal, with the binary
  `tool_calls[].ok` as a fallback. `LocalDelegateResult` carries an optional
  `task_success: Option<TaskSuccessVerdict>`; `llm_judged` provenance is
  downgraded by ORIENT (Gap S3). **Residual:** open tasks with no oracle still
  rely on the Go See loop (C2) — the process cannot detect a healthy-but-wrong
  agent without a deterministic evaluator.
- **C4 latency — REGULATED.** `LocalDelegateResult.latency_ms` now flows through
  ORIENT (`latency_outliers`) into DECIDE, which proposes `reconfigure_agent`
  for outlier agents. The sense-without-act sub-loop is closed.
- **Loop A closure requires execution evidence.** Advisory is the safe default: the plan is the output and the operator may execute it. When the caller explicitly chooses `steering`, the Curator/human calls the local execution tool once, reconciles its returned `results` to the plan, and feeds that array into the next iteration. A rendered directive or former manifest description is not an execution receipt. A tool-level error leaves execution state unknown; do not automatically replay.

Full per-property evidence and the VSM/Ashby analysis were recorded in the
2026-08-03 audit (the audit document has since been retired; this section is
the surviving record).

## Registry

- Templates: `kask/registry/templates/swarm-intelligence/swarm-{sense,orient,decide,act,check,compose-guide}.j2`
- Live process: read the selected swarm's roster → SENSE → ORIENT → DECIDE → check proposed moves against observed failed signatures and actual roster membership/allowlists → ACT emits intents → execute only in explicitly selected steering mode → CHECK re-reads state and receipts → compute receipt coverage, distance and target gate with `lisp_eval` → carry observed iteration history into a bounded next cycle. A rendered phase alone does not call an MCP tool.
- No registered `swarm.*` filter, accumulator or second-order monitor primitives exist. Record failed edits and task-board/receipt fault evidence explicitly; `lisp_eval` handles only the pinned arithmetic and exact-signature checks, while Go See is human observation. Do not report a nonexistent compute step as executed.
- MCP tool surface — 37 local tools (the server is local-only; consult current registration rather than a fixed count):
  - **Dispatch**: `swarm_delegate_local` (standalone),
    `swarm_delegate_in_thread_local` (member-scoped shared thread),
    `swarm_fanout_local` (parallel batch, capped at 10),
    `swarm_pipeline_local` (sequential with `{prev_output}` substitution),
    `swarm_execute_plan_local` (ordered plan + optional evaluators +
    task-board progress), `swarm_evaluate_local` (deterministic
    task-success evaluator — stamps a `TaskSuccessVerdict` with
    `provenance: Deterministic`), `swarm_eval_suite_local`,
    `swarm_eval_agent_local` (rollout harness), `swarm_task_board`.
  - **Thread and shared memory**: `swarm_thread_local` (read a swarm's
    ordered durable turns), `swarm_list_local_threads`,
    `swarm_recall_local` (recall prior response passages from shared
    local swarm memory by semantic similarity).
  - **Agent registry**: `swarm_list_local_agents`,
    `swarm_get_local_agent`, `swarm_create_local_agent`,
    `swarm_reconfigure_local_agent` (C6), `swarm_remove_local`.
  - **Swarm lifecycle**: `swarm_create_local_swarm`,
    `swarm_list_local_swarms`, `swarm_get_local_swarm`,
    `swarm_update_local_swarm`, `swarm_delete_local_swarm`,
    `swarm_clone_local_swarm`, `swarm_add_agent_local`,
    `swarm_remove_agent_local` (named swarms/rosters, explicit lifecycle).
  - **Knowledge and fleet shape**: `swarm_search_knowledge_local`,
    `swarm_generate_prompt_local`, `swarm_generate_ontology_local`
    (search/generate over the operator's `hkask-memory` + local
    inference; see
    [Local Knowledge Tools design](../../../kask/docs/diataxis/swarm_system/reference.md)),
    `swarm_observed_seams_local` (the observed delegation topology —
    edges recorded at dispatch, compared against declared ports),
    `swarm_fleet_digest_local` (the fleet's shape: categories and
    counts, never individual agents), `swarm_who_answers_local`
    (which agents accept a label, with the bespoke/cohort/universal
    reading).
  - **Workflow and assist**: `swarm_workflow_check_local`,
    `swarm_run_workflow_local` (execute a declared workflow_template
    end-to-end), `swarm_ai_assist` (authoring aid).
  - **A2A**: `swarm_a2a_send` (A2A protocol message, in-process),
    `swarm_a2a_card` (A2A Agent Card discovery),
    `swarm_a2a_broadcast` (message every member of a local swarm).

    **Fleet facts are registry-first — never from memory** (fermi's
    `biotech_analyst` incident: a navigator answered three specific claims
    about an agent's model ladder, all false, all one lookup from being
    checked, because its sources were prose). Per-agent facts (model, ports,
    stats) come from `swarm_get_local_agent`; the fleet's shape comes from
    `swarm_fleet_digest_local`; who answers an ask comes from
    `swarm_who_answers_local`. Do not state them from memory. If a tool
    reports a different fleet size than a digest you hold, the digest is
    stale — say so rather than answering from it.
- Local dispatch has no consent token, credit budget, or funding gate —
  authority is roster membership, the card's declared allowlists, and the
  governed tool surface. See the
  [Swarm Systems Reference](../../../kask/docs/diataxis/swarm_system/reference.md)
  for the full tool/contract table.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `swarm-sense.j2` | Measure the current swarm state. Read the member roster and local agent registry (`swarm_get_local_swarm` / `swarm_list_local_agents`). Optionally probe an agent's prefix-scoped knowledge via `swarm_search_knowledge_local` when the roster's accepts/produces is ambiguous about whether an agent covers a required transform. Compute the alignment team property (task-gated delegation-graph density from produces/accepts overlap); variety_coverage, diversity, cohesion, separation, and redundancy_pairs route through the caller's `SWM-1` gate. Derive required_transforms and covered_transforms from the task and roster. Consumes prior_iteration.next_focus when present (feedback loop closure). Vacuous-truth defaults (enforced in the `SWM-1` gate): empty required_transforms → variety_coverage 1.0 + trivial_task flag; empty swarm → diversity 0.0, variety_coverage 0.0. |
| `swarm-orient.j2` | Classify the gap between sensed state and target condition into one of three deficit classes (or on-target): (a) variety deficit — required_transforms not covered; (b) coherence deficit — Thagard coherence flat/declining OR diversity below floor (premature convergence, the PSO/ACO failure mode); (c) loop-break — un-reconciled dispatches or un-fed-back receipts. Operates on the state shape from SENSE, not the data source. Delegates to pragmatic-cybernetics for the 5-property assessment (polarity, delay, gain, closure, fidelity) when the deficit is a loop-break. Classifies the coordination problem (Axelrod): cooperation (repeated, shadow of the future) vs division-of-labor (one-shot, complementary capabilities). |
| `swarm-decide.j2` | Propose measured-deficit responses using PSO/ACO/Reynolds as analogies, not executed numerical algorithms. Local composition uses named-swarm roster add/remove (`swarm_add_agent_local` / `swarm_remove_agent_local`) without deleting saved cards; an author or remove move that must delete a card is explicit (`swarm_create_local_agent` / `swarm_remove_local`). The one-level delegation invariant still applies. |
| `swarm-act.j2` | Prepare local no-credit action intents (`decisions`, `swarm_id`, `task`, `steering_mode`) and emit `steering_directive` for local steering (`swarm_execute_plan_local`, gated by `SWM-3`). Rendering emits a plan, never an executed-call receipt; the invoking Curator/human runs the selected live tools and retains their results. |
| `swarm-check.j2` | Re-read post-Act state and observed receipts (`pre_act_state`, `act_result`, `swarm_id`, `target_condition`, `task_success`, `fetched_post_act_state`); local `loop_closure` covers both successful and explicit error entries in order, while a dispatch error remains an independent alert. Supply measured axes for the caller's `lisp_eval` distance/target gate. Zero actual delegations are vacuously receipted, but unexecuted plans are not. |
| `swarm-compose-guide.j2` | Reference/authoring template for agent and swarm composition. Given a composition request (surface: agent|swarm, action: suggest|validate, plus partial fields), it produces either suggested completions for unfilled fields (action=suggest) or a validation verdict over the supplied fields (action=validate). Encodes the canonical field definitions (name, agent_type, description, system_prompt, mission, agents) and the local-registry constraints (lowercase_with_underscores ids, port-registry-backed accepts/produces, filesystem cards without credits). Mirrors the inline prompt used by the `swarm_ai_assist` MCP tool — this template is the single source of truth the panel and the process share. Consumed by the DECIDE phase when it proposes an `author_agent` move (no existing card covers a required transform); also available for standalone invocation from the swarm panel. |

To render a template, call the `render_template` tool with the template ref and a context object naming every contract input field. The deterministic gates (`SWM-1` after SENSE, `SWM-2` after ORIENT, `SWM-3` before local steering) run in the caller's `lisp_eval` — never in a rendered template.

`swarm-intelligence/swarm-sense` (Phase 1 — SENSE):
```json
{
  "swarm_id": "<string>",
  "task": "<string>",
  "target_condition": { "variety_coverage": 0.9, "diversity": 0.25, "loop_closure": 1.0 } | null,
  "prior_iteration": { "next_focus": "<string>", "lessons_learned": [] } | null,
  "kanban_board_id": "<string> | null",
  "fetched_swarm_state": { "<the pre-fetched roster read — required object>" }
}
```

`swarm-intelligence/swarm-orient` (Phase 2 — ORIENT):
```json
{
  "swarm_state": { "<the SENSE output with the caller's SWM-1 gate metrics merged>" },
  "task": "<string>",
  "target_condition": { "variety_coverage": 0.9, "diversity": 0.25, "loop_closure": 1.0 } | null,
  "delegate_results": [ { "<LocalDelegateResult entries — observed receipts only>" } ] | null
}
```

`swarm-intelligence/swarm-decide` (Phase 3 — DECIDE):
```json
{
  "swarm_state": { "<the SENSE output with the SWM-1 gate metrics merged>" },
  "orientation": { "<the ORIENT output with the caller's SWM-2 gate outputs (latency_outliers, agent_at_fault) merged>" },
  "task": "<string>",
  "failed_edits": [ { "decision_action": "<string>", "swarm_state_signature": "<string>", "d_delta": 0.0 } ],
  "influence_scores": { "<agent_type>": 0.0 },
  "second_order": { "recommendation": "go_see|diversify_action" }
}
```

`swarm-intelligence/swarm-act` (Phase 5 — ACT):
```json
{
  "decisions": { "<the DECIDE output>" },
  "swarm_id": "<string>",
  "task": "<string>",
  "steering_mode": "advisory|steering"
}
```

`swarm-intelligence/swarm-check` (Phase 6 — CHECK):
```json
{
  "pre_act_state": { "<the SENSE output with the SWM-1 gate metrics merged>" },
  "act_result": { "<the ACT output>" },
  "swarm_id": "<string>",
  "target_condition": { "variety_coverage": 0.9, "diversity": 0.25, "loop_closure": 1.0 } | null,
  "task_success": { "pass": true, "score": 1.0, "provenance": "deterministic_evaluator" } | null,
  "fetched_post_act_state": { "<the post-Act roster re-read — required object>" }
}
```

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass):

- Target-plus-stability gate, four distances stepping ≤0.03, vc 0.95,
  div 0.3, lc 1, coherence non-decreasing, no algedonic alert, task
  success 1 → `true`.
- One 0.05 jump in the distance sequence → `false`.
- Distance form, vc 0.8 / div 0.2 / lc 0.9 / s null →
  `0.22912878474779194` — the PSO balance reading.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Local member delegation carries `swarm_id`, not a credit budget — there is no credit ledger and no consent gate; authority is roster membership and the card's declared allowlists.
- Task-success verdicts are deterministic (`swarm_evaluate_local` or a configured evaluator) or null — never LLM-judged; `llm_judged` provenance is downgraded by ORIENT.
- Algedonic override: a dispatch error or un-reconciled emitted dispatch escalates regardless of the convergence distance `d` — a broken feedback loop is never read as "no deviation".
- Convergence requires |d_i − d_{i−1}| < 0.03 for 3 consecutive iterations; a healthy swarm that fails the task must NOT converge.
- Open tasks with no oracle: `task_success` stays null and the human Go See loop (C2) covers the gap — never an LLM judge.
- Clean up (storage Cleanup rule). When a steering session ends, remove local cards this session created by an `author_agent` move that the operator did not ask to keep (`swarm_remove_local`), and delete a local swarm the session created (`swarm_delete_local_swarm`); its thread stays readable. A swarm or card the operator supplied, or asked to keep, is untouched and named in the report. Deleting an operator-supplied card is never automatic cleanup.
