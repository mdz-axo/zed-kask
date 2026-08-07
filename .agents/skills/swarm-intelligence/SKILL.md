---
name: swarm-intelligence
visibility: public
description: "Convergent swarm-composition process. Senses swarm state against Onto4MAT and the swarm backend APIs; orients via Ashby requisite variety and PSO cognitive/social balance; decides composition adjustments isomorphic to PSO velocity tuning, ACO pheromone deposition, and Reynolds flocking; acts via gated swarm_delegate/swarm_delegate_local calls with a budget gate; checks spend against the algedonic channel; converges via Cauchy criterion on the swarm-state distance metric. Mode-aware: abw mode fetches from ABW REST and delegates via swarm_delegate; local mode reads the local agent registry + ledger and delegates via swarm_execute_plan_local (batch) or swarm_delegate_local (single). Composes pragmatic-cybernetics, kata-improvement, essentialist. Emits reg.swarm.* spans. Any userpod may invoke this skill."
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

- Dispatching a single ABW agent without composition intent (use the
  `swarm_panel` UI directly)
- General multi-agent coordination theory outside ABW (the skill's vocabulary
  is ABW-specific: workspaces, hired agents, compound agents, dependencies)
- Curator (Xaman Ek) session management (that is a per-dispatch parameter of
  the ACT phase, not a separate skill)

## PDCA Loop

```
Check: Phase 1  — SENSE            → Measure current swarm state against Onto4MAT + backend workspace/wallet
Plan:  Phase 2  — ORIENT           → Classify the gap + deterministic fault attribution (C5)
Plan:  Phase 3  — DECIDE           → Propose composition adjustments isomorphic to PSO/ACO/Reynolds tuning
Det:   Phase 4  — FILTER           → Deterministically enforce C3 failed-edit + C7 influence guards (no LLM)
Do:    Phase 5  — ACT              → Emit gated swarm_hire / swarm_delegate / swarm_delegate_local
Check: Phase 6  — CHECK             → Re-measure, compute swarm-state distance d, emit next_focus + algedonic
Check: Phase 7  — CONVERGE (check)  → Cauchy criterion on d (deterministic, no LLM judgment)
Check: Phase 8  — CONVERGE (accum) → Deterministic accumulator: iteration_log, failed_edits, influence_scores (C1/C3/C7)
Check: Phase 9  — CONVERGE (monitor)→ Second-order monitor: reasoning-loop + sensor-truth-divergence + Go See cadence (C1/C2)
Act:   Phase 10 — LOOP              → Re-enter SENSE with prior_iteration + threaded accumulators if not converged
```

The shape is cybernetic (sense → orient → decide → filter → act → check → converge),
not the gradient-hunter's Prior→Map→Detect→Hypothesize→Report (spatial-gradient
analysis) or the bug-hunt's Charter→Probe→Oracle→Taxonomize→Report
(exploratory testing). The shape emerges from the domain: a swarm is a
feedback loop, so the skill is a feedback loop. The deterministic compute steps
(FILTER, CONVERGE) enforce the cybernetic plan's guards without an LLM — an
LLM template cannot reliably maintain a running set/sum across LOOP iterations.

## Target condition (measurable)

A swarm is well-composed for a task when three conditions hold simultaneously:

1. **Requisite variety** (Ashby): `variety_coverage >= 0.9` — the hired
   agents' `accepts[]`/`produces[]` cover the task's required transforms.
2. **Coherence without premature convergence**: `diversity >= 0.25` (≥¼ of
   agents are non-identical) and Thagard coherence non-decreasing.
3. **Closed feedback loop**: `loop_closure = 1.0` — every dispatch's
   `estimated_credits` reconciled against `/api/wallet/transactions` and
   every `curator_involved` dispatch's `data_shared` acknowledged.

## Convergence criterion (deterministic)

Cauchy criterion on the swarm-state distance
`d = sqrt( (1 - variety_coverage)² + max(0, diversity_floor - diversity)² + (1 - loop_closure)² )`.
When the caller supplies a deterministic `task_success` verdict (component C0),
`d` gains a fourth axis `(1 - s)²` (s = task_success.score, or pass→1.0 /
fail→0.0) — a healthy swarm that fails the task must NOT converge. When
`task_success` is null (open tasks with no oracle), `d` uses the three
swarm-health axes only; the human Go See loop (C2) covers the task-success gap,
never an LLM judge.
The sequence `d_1, d_2, …` has converged when `|d_i − d_{i−1}| < 0.03` for 3
consecutive iterations. **Algedonic override:** a 402 or un-acknowledged
curator dispatch escalates regardless of `d` — a broken algedonic channel is
never read as "no deviation" (the `.rules` "unwrap_or(0)" trap enforced as a
convergence invariant).

## Cybernetic Swarm Plan components

The skill integrates the Cybernetic Swarm Plan's deterministic components
(C0–C8). The accumulators and guards are `compute` primitives (no LLM) — an
LLM template cannot reliably maintain a running set/sum across LOOP
iterations, so the enforcement points live in the deterministic math layer.

| Component                           | What                                                                                                                                                                                                            | Enforcement point                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **C0** task-success `s`             | Deterministic evaluator verdict → fourth axis of `d`                                                                                                                                                            | CHECK template + manifest `task_success` input                                                                                                                                                                                                                                                                                                  |
| **C1** second-order monitor         | Reasoning-loop + sensor-truth-divergence detection over the iteration log                                                                                                                                       | `swarm.second_order_monitor` compute step (9)                                                                                                                                                                                                                                                                                                   |
| **C2** Go See cadence               | Scheduled human check every N convergences + event trigger                                                                                                                                                      | `cadence_every` param in the monitor; SENSE surfaces `go_see`                                                                                                                                                                                                                                                                                   |
| **C3** failed-edit memory           | Anti-loop set; the FILTER drops moves matching prior failed signatures                                                                                                                                          | `swarm.filter_proposed_moves` compute step (4)                                                                                                                                                                                                                                                                                                  |
| **C4** latency `T_q`                | End-to-end delegation latency measurement → ORIENT surfaces latency outliers → DECIDE reconfigures slow agents                                                                                                  | `LocalDelegateResult.latency_ms` → ORIENT `latency_outliers` → DECIDE `reconfigure_agent` (regulated, audit 2026-08-03; previously sensed but not acted on)                                                                                                                                                                                     |
| **C5** fault attribution            | Deterministic priority rule over the delegate trace — per-delegation `task_success` (highest fidelity) → whole-task terminal failure → binary `tool_calls[].ok`/`executed_skills[].ok`; fault-count aggregation | ORIENT template (rules 1-6) + `swarm.converge_accumulate` `fault_count`. `task_success` is the Loop B fidelity fix (audit 2026-08-03); `llm_judged` provenance is downgraded (Gap S3). Fires only when `delegate_results` execution telemetry is supplied (the planning cascade emits intents, not executed results). See Steering modes below. |
| **C6** reconfigure_agent            | Re-prompt a blamed agent in place (Modify-Block / MASS prompt axis)                                                                                                                                             | `swarm_reconfigure_local_agent` tool + DECIDE move type. Active only when C5 has fault telemetry (steering mode).                                                                                                                                                                                                                               |
| **C7** influence-weighted rejection | Reject re-hire of agent types measured to degrade the swarm                                                                                                                                                     | `swarm.filter_proposed_moves` compute step (4)                                                                                                                                                                                                                                                                                                  |
| **C8** task-gated alignment         | Task-conditional edge relevance in SENSE (OFA-MAS TAGSE port)                                                                                                                                                   | SENSE template `alignment` definition                                                                                                                                                                                                                                                                                                           |

## Composed Skills

| Skill                   | Role                                                  | When Invoked                              |
| ----------------------- | ----------------------------------------------------- | ----------------------------------------- |
| `pragmatic-cybernetics` | 5-property loop assessment + Ashby variety + VSM      | ORIENT, when deficit is a loop-break      |
| `kata-improvement`      | Cauchy convergence pattern (`kata.convergence_check`) | CONVERGE (compute step)                   |
| `essentialist`          | Deletion-test proposed phases                         | Design-time (applied to the skill itself) |

## Steering modes (the execution boundary)

The swarm-intelligence cascade is a **planning loop** — it composes/steers the
swarm and emits a plan (`emitted_calls`), but it does not execute delegations
itself (no `action: execute` step). The `steering_mode` context input governs
how the output is handled and closes the feedback loop:

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
`swarm_execute_plan_local` with the emitted_calls as delegations (each with
an optional deterministic evaluator), which runs all delegations, evaluates
results, and returns the collected `LocalDelegateResult` array with
`task_success` verdicts stamped. The Curator re-invokes swarm-intelligence
with `delegate_results` set to that array — closing the feedback loop without
a new FlowDef execution surface (the Curator's normal tool-call turn IS the
execution).

### Cloud swarms — Xaman Ek

Xaman Ek has steering **built in** (cloud-side). Delegate to it via
`swarm_xaman` with the plan as the message; Xaman Ek executes the plan and
steers the ABW swarm. The zed-kask side calls `swarm_xaman` with a steering-
style message (session_type `composition_design`); the execution + result
capture is Xaman Ek's built-in capability, then `delegate_results` flow back.

### Local swarms — the Kask Curator (continued)

Locally, the Kask Curator steers using the `swarm-intelligence` skill itself
(the cascade plans, the Curator executes), OR the focused **swarm-steering**
skill (a narrower skill that codifies just the execute-and-feed-back loop:
call `swarm_execute_plan_local` with the plan, collect the returned
`LocalDelegateResult` array with `task_success` verdicts, re-invoke with
`delegate_results`).

### The `delegate_results` contract (C5/C6 activation)

`delegate_results` is an array of `swarm_execute_plan_local` results
(`LocalDelegateResult`-shaped): `agent_id`, `response`, `model`, `tokens_used`,
`cost`, `balance`, `latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`),
`executed_skills[]` (each `{skill, ok, error?}`), `task_success` (optional
deterministic verdict). ORIENT attributes fault from
`delegate_results[].task_success.pass` (highest fidelity, when present) and
`delegate_results[].tool_calls[].ok` / `executed_skills[].ok`; `fault_count`
accumulates; C6 reconfigures the most-blamed agent. Absent `delegate_results`,
C5/C6 are inert (the planning cascade has no execution telemetry).

## Known limitations (audit 2026-08-03)

The [Swarm Cybernetics/Semantics Audit](../../../kask/docs/audits/swarm-cybernetics-semantics-audit.md)
found structural gaps; the two High-severity ones are now mitigated in the
registry + code (2026-08-03), with one residual:

- **Loop B fidelity — MITIGATED.** C5/C6 fault attribution now reads a
  per-delegation `task_success` (deterministic verdict the executor stamps on
  each `LocalDelegateResult`) as its highest-fidelity signal, with the binary
  `tool_calls[].ok` as a fallback. `LocalDelegateResult` carries an optional
  `task_success: Option<TaskSuccessVerdict>`; `llm_judged` provenance is
  downgraded by ORIENT (Gap S3). **Residual:** open tasks with no oracle still
  rely on the Go See loop (C2) — the cascade cannot detect a healthy-but-wrong
  agent without a deterministic evaluator.
- **C4 latency — REGULATED.** `LocalDelegateResult.latency_ms` now flows through
  ORIENT (`latency_outliers`) into DECIDE, which proposes `reconfigure_agent`
  for outlier agents. The sense-without-act sub-loop is closed.
- **Loop A closure — OPEN (by design).** The default execution mode is
  `advisory`, where the operator must feed `delegate_results` back or the
  planning loop stays open. The `swarm-steering` skill (steering mode) makes
  closure structural; it is not yet the default.

Full per-property evidence and the VSM/Ashby analysis are in the audit.

## Registry

Registry is authoritative — when this SKILL.md disagrees with registry
templates, the registry wins.

- Template manifest: `kask/registry/templates/swarm-intelligence/manifest.yaml`
- Templates: `kask/registry/templates/swarm-intelligence/swarm-{sense,orient,decide,act,check}.j2`
- Reference: `kask/registry/templates/swarm-intelligence/swarm-patterns.yaml`
- Process manifest: `kask/registry/manifests/swarm-intelligence.yaml` (10 steps:
  SENSE, ORIENT, DECIDE, FILTER, ACT, CHECK, convergence_check,
  converge_accumulate, second_order_monitor, LOOP)
- Deterministic compute primitives: `swarm.converge_accumulate`,
  `swarm.second_order_monitor`, `swarm.filter_proposed_moves` (in
  `hkask-templates/src/compute.rs`)
- MCP tool surface (50 tools — both sets always registered in either mode;
  `kask.swarm.mode` selects the substrate, not the surface; pinned by
  `tool_surface_is_exactly_50_registered_tools`):
  - **ABW tools (27)**: `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`,
    `swarm_list_apps`, `swarm_ontology_templates`, `swarm_execute_agent`,
    `swarm_hire_cost`, `swarm_request_consent`, `swarm_authorize_session`,
    `swarm_hire`, `swarm_delegate`, `swarm_delegate_and_wait`, `swarm_fanout`,
    `swarm_run_status`, `swarm_generate_prompt`, `swarm_generate_ontology`,
    `swarm_create_agent`, `swarm_create_swarm`, `swarm_xaman`, `swarm_create_app`,
    `swarm_fire` (roster removal, verified live), `swarm_delete_agent`,
    `swarm_delete_swarm`, `swarm_search_knowledge`, `swarm_publish_checks`,
    `swarm_publish_agent`, `swarm_fork_agent`.
  - **Local tools (25)**: `swarm_fund_local`, `swarm_balance_local`,
    `swarm_local_history`, `swarm_delegate_local`, `swarm_fanout_local`,
    `swarm_pipeline_local`, `swarm_a2a_send` (A2A protocol message, in-process),
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
    [Local Knowledge Tools design](../../../kask/docs/plans/local-swarm-knowledge-tools.md)),
    `swarm_ai_assist` (authoring aid), `swarm_evaluate_local` (deterministic
    task-success evaluator — stamps a `TaskSuccessVerdict` with
    `provenance: Deterministic`), `swarm_execute_plan_local` (batch plan
    execution — runs delegations, evaluates results, returns collected array
    with verdicts stamped; closes the loop in one call).
- Spend-mutating ABW tools (`swarm_hire`, `swarm_delegate`,
  `swarm_delegate_and_wait`, `swarm_fanout`, `swarm_create_swarm`,
  `swarm_xaman`) are consent-gated via `swarm_request_consent` (single-use,
  action+target+credits-scoped, TTL-bounded) or `swarm_authorize_session`
  (headless). In local mode there is no consent token — the ledger balance +
  the per-dispatch ceiling (`HKASK_ABW_MAX_CREDITS`, default 50) is the gate.
  See the [Swarm Systems Reference](../../../kask/docs/diataxis/swarm_system/reference.md)
  for the full tool/contract table and the token model.
