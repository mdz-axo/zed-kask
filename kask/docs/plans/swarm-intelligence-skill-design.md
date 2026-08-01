---
title: "swarm-intelligence Skill — Design Document"
audience: [zed-kask integrators, hKask architects, skill authors]
last_updated: 2026-08-01
version: "0.1.0"
status: "Design converged; scaffold pending grill-me round"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# `swarm-intelligence` Skill — Design Document

> **One-line frame:** A registry skill that informs the **composition,
> configuration, and operation** of Agent Bestiary World (ABW) agent swarms —
> grounded in ABW's actual swarm semantics (workspaces, hired agents, compound
> agents, `execute_agent`/`delegate_to_agent`, Thagard coherence, credit gas)
> and the classical swarm-intelligence literature (Reynolds, Kennedy-Eberhart,
> Dorigo, Bonabeau), steered toward a target condition via an embedded PDCA
> loop with a deterministic convergence criterion.

This document is the research-grounded design for the `swarm-intelligence`
skill. It is the companion to `abw-swarm-intelligence.md` (the *integration*
plan for the `swarm_panel` crate + `hkask-mcp-swarm` MCP server). Where the
integration plan answers "how is the ABW surface wired into zed-kask?", this
document answers "how does an operator *decide* what swarm to compose, how to
configure it, and how to steer it toward a target condition?" — and encodes
that decision process as a registry skill.

---

## 0. Research Phase — Sources and Anchors

Three bounded source passes were performed. Every phase in the proposed
manifest (§3) traces to at least one anchor here; the traceability table is
in §5.

### Pass 1 — Local library (`~/Clones/Library`)

Path verified: the user's "Clones/Librayr" was a typo for `~/Clones/Library`.

| Source | Anchor extracted | How it shapes the skill |
|---|---|---|
| `Evolutionary_Swarm_Intelligence__Jagdish_Chand_Bansal.pdf` (Bansal, Singh, Pal 2019, Springer SCI v779) | PSO velocity update: `v ← ωv + c₁r₁(pbest−x) + c₂r₂(gbest−x)`. Cognitive (c₁) vs social (c₂) scaling. Inertia weight ω as exploration/exploitation balance. ACO pheromone deposition/evaporation as stigmergy. **Failure mode: premature convergence / diversity collapse.** | The **Orient** phase classifies a swarm's current cognitive/social balance and diversity. The **Decide** phase proposes composition adjustments isomorphic to tuning c₁/c₂/ω: add diverse agents (raise c₁), add a coordinator (raise c₂), narrow the search (lower ω). |
| `cross_domain_swarm_intelligence.pdf` (Labra 2026, draft) | **Onto4MAT** multi-agent teaming ontology (Kiesel et al. 2022): `Team { hasAlignmentWithTeam, hasCohesionWithTeam, hasTeamSeparation, hasInfluence }`, `Formation { Arc, Echelon, Line, V, Wedge }`, `Agent { hasSpeed, hasHeading, hasEnergy, hasDistanceToGoal }`. **Reynolds (1987) flocking**: separation / alignment / cohesion as three local rules sufficient for emergent flocking. **Scale-invariance**: normalized swarm metrics are domain-independent. **W3C SSN/SOSA** sensor ontology for observable properties. | The **Sense** phase measures the swarm against Onto4MAT team properties (alignment, cohesion, separation) and Reynolds rules — these are the *measurable* swarm-state variables. Scale-invariance lets the same metrics apply to a 3-agent and a 30-agent swarm. |
| `Complex_Adaptive_Systems_-_John_H_Miller.pdf` (Miller & Page) | CAS: emergence from local rules, feedback, heterogeneous adaptive agents, nonlinear aggregation. "Roving agent" models. Schelling segregation as emergent. | Substrate ontology: a swarm is a CAS — its behavior is emergent, not centrally commanded. The skill steers *via the rules and composition*, not by dictating outputs (consistent with ABW's no-delegation-chains invariant). |
| `Evolution_of_cooperation_-_Axelrod.pdf` (Axelrod 1984) | Iterated Prisoner's Dilemma tournament: **TIT FOR TAT** wins (start cooperative, mirror opponent). Cooperation emerges among egoists *without central authority* when there is a shadow of the future. Noise degrades pure TFT; generosity (GTFT) restores it. | The **Decide** phase's inter-agent coordination policy: when agents in a swarm have conflicting objectives, the skill recommends reciprocity-based coordination (TFT/GTFT) over command, and flags when the "shadow of the future" is too short for cooperation to emerge (one-shot dispatches). |
| `Why_Humans_Cooperate_Henrich.pdf` (Henrich) | Cultural group selection, norm psychology, cooperative dilemmas. | Secondary anchor for the **Orient** phase's "is this swarm a cooperation problem or a division-of-labor problem?" classification. |
| `Patterns_of_Distributed_Systems_Joshi.pdf` | Distributed-systems patterns (heartbeat, lease, saga, etc.). | The **Act** phase's configuration recommendations reference these for cross-process swarm coordination (when agents run on separate ABW workspaces). |

### Pass 2 — Local archive (`~/Clones/hkask-archive`)

| Source | Anchor extracted | How it shapes the skill |
|---|---|---|
| `2026-05-22-documentation-refresh/STANDING_ENSEMBLE_SESSION.md` | **DEPRECATED.** The "Standing Ensemble Session" (Curator + 7 bots, Curator-led, "no swarm consensus") was the *ensemble* model. The user has confirmed the ensemble concept is **deprecated** and ABW is the current agentic-swarm/orchestration framework. | The skill must **not** assume Curator-led centralized orchestration. ABW swarms are workspace-scoped, agent-hired, with compound-agent orchestration — a genuinely different control architecture. This is recorded as an explicit design decision (§2, D1). |
| `2026-06-24 - all/research/bug-hunting-as-autopoietic-skill-unified.md` | Maturana & Varela autopoiesis; Luhmann autopoietic communication; second-order cybernetics (the system observes its own process). Operational closure: next charter depends on prior findings. | The PDCA loop is **autopoietic at the heuristic layer**: each iteration's composition recommendation is a function of the prior iteration's measured swarm state and prior heuristics, not external directives. This is the convergence-loop rationale (§4). |

### Pass 3 — ABW integration plan (the primary source)

| Source | Anchor extracted | How it shapes the skill |
|---|---|---|
| `kask/docs/plans/abw-swarm-intelligence.md` §0 (verified live 2026-08-01) | **ABW swarm semantics:** a "swarm" = an ABW **workspace** with hired agents. Compound agents orchestrate via `execute_agent` (text consult, 1 LLM call) and `delegate_to_agent` (full tool access, 1 cr + tokens, **delegation chains forbidden** — delegates lose `delegate_to_agent`/`execute_agent`). Gas: hire 5 cr, @mention 1 cr + tokens, delegation 1 cr + tokens. Compound agents declare `dependencies { required, optional }` and auto-hire their team. Current compound agents: `social_media_studio`, `cohere_and_coordinate`, `intention_coordinator`. **Thagard coherence scoring** on workspaces. Agent cards carry `capabilities { executor, mcp_tools[], model, temperature }`, `dependencies`, `execution_stats`, `dreaming`, `embedding`, `accepts`, `produces`. | This is the **domain ontology** the skill operates on. The Sense phase reads workspace/agent state via the `hkask-mcp-swarm` tools; the Decide phase recommends hires/fires/delegations against this exact vocabulary; the Act phase emits `swarm_update_swarm` / `swarm_delegate` calls. The skill does not invent a parallel vocabulary. |
| `abw-swarm-intelligence.md` §4 (Cybernetic Analysis) | The feedback loop the swarm panel must close: operator intent → dispatch → ABW execute → credits consumed → state + cost/consent fed back. **5-property assessment**: polarity (healthy iff cost feeds back), delay (risk high), gain (risk unbounded — N×M cost amplification), closure (broken unless cost + curator-data-sharing feed back), fidelity (unknown). **Ashby variety check**: 2 of 7 disturbances (402 payment, credit pre-flight) had no existing mechanism. **VSM mapping**: S1 ops, S2 anti-oscillation (missing), S3 resource allocation (missing — critical), S4 spec drift, S5 policy, algedonic S1→S5. | The skill's PDCA loop **is** the closure of this feedback loop. The convergence criterion (§4) is the deterministic signal that the loop has closed: the swarm's measured state has stabilized against the target condition *and* the cost/consent algedonic channel has reported no unhandled deviation. The skill reuses pragmatic-cybernetics' 5-property + Ashby + VSM vocabulary rather than reinventing it. |
| `abw-swarm-intelligence.md` §3.6 (cost/consent gate) | `DispatchIntent { swarm_id, task, estimated_credits, curator_involved, data_shared }` → `GateDecision { Proceed { credits_authorized } | Abort { reason } }`. Consent gate must *actually block* dispatch. | The **Act** phase never emits a dispatch without a `DispatchIntent` whose `estimated_credits` is reconciled against `/api/wallet`. The skill treats an un-gated dispatch as a broken feedback loop (reinforcing, not corrective) — directly from the `.rules` "unwrap_or(0) on regulation-loop sense inputs" trap. |

### Pass 3b — GitHub open-source patterns (survey, not cloned)

A survey of canonical open-source swarm-algorithm implementations was
initiated but **canceled by the user** before results returned. The classical
patterns (PSO velocity update, ACO pheromone cycle, Reynolds boids) were
already extracted from the Bansal book and the cross-domain paper in Pass 1,
so the design does not depend on the GitHub survey. Where a GitHub anchor
would strengthen traceability, the design cites the canonical *paper* (Kennedy
& Eberhart 1995 for PSO; Dorigo 1992 for ACO; Reynolds 1987 for boids) rather
than a specific repo. **Open uncertainty U1** (§6) records that repo-level
pattern extraction was not completed.

---

## 1. Q1 — Target Condition (what does "good swarm composition" look like, measurably?)

**Target condition (TC):** A swarm is well-composed for a task when, across
the PDCA iterations, three measurable conditions hold simultaneously:

1. **Requisite variety (Ashby):** the swarm's response repertoire covers the
   disturbance set the task presents. Operationalized via Onto4MAT: the set
   of `accepts[]`/`produces[]` type-pairs across hired agents spans the task's
   required input→output transformations. **Metric:** `variety_coverage =
   |required_transforms ∩ covered_transforms| / |required_transforms|`,
   computed deterministically from agent cards (no LLM judgment). Target: ≥ 0.9.

2. **Coherence without premature convergence (Thagard + PSO):** the swarm's
   Thagard coherence score is non-decreasing across iterations *and* the
   agent-diversity (Onto4MAT `hasTeamSeparation` analog: distinct
   `agent_type`/`model`/`temperature` tuples) has not collapsed below a floor.
   **Metric:** `diversity = |distinct (agent_type, model) tuples| /
   |agents|`, with floor 0.25 (≥¼ of agents are non-identical). Premature
   convergence is the canonical PSO/ACO failure mode (Bansal 2019).

3. **Closed feedback loop (cybernetics):** every dispatch's `estimated_credits`
   was reconciled against actual spend (`/api/wallet/transactions`), and every
   `curator_involved` dispatch's `data_shared` was acknowledged by the
   operator. **Metric:** `loop_closure = (reconciled_dispatches /
   total_dispatches) ∧ (acknowledged_curator_dispatches /
   curator_dispatches)`, both ratios = 1.0. A single un-reconciled dispatch
   fails the condition — this is the `.rules` "unwrap_or(0)" trap applied as a
   target, not just a guard.

The target condition is **not** "the swarm produced the right answer" (that is
LLM-judged and out of scope for a composition skill). It is "the swarm is
*structurally* capable of the task, *diverse* enough to avoid premature
convergence, and *governed* by a closed cost/consent loop." These are
deterministic, queryable, and traceable to the research anchors.

---

## 2. Q2 — Domain Decomposition (manifest + .j2 template phases, PDCA shape)

The PDCA shape emerges from the cybernetic feedback loop in the integration
plan §4.1, not from a generic template:

```
SENSE  (Check)  → Measure current swarm state against Onto4MAT + wallet
ORIENT (Plan)   → Classify the gap: variety deficit | coherence deficit | loop-break
DECIDE (Plan)   → Propose composition adjustment isomorphic to PSO/ACO/Reynolds tuning
ACT    (Do)     → Emit gated swarm_update_swarm / swarm_delegate with DispatchIntent
CHECK  (Check)  → Reconcile actual spend + curator data-sharing; compute convergence metric
LOOP   (Act)    → Re-enter SENSE with prior_iteration if not converged
```

This is **sense → orient → decide → act** (the Regulation OODA shape already
used by `pragmatic-cybernetics`), with an explicit **check** that closes the
loop deterministically and a **loop** that feeds the prior iteration forward.
It is *not* the gradient-hunter's Prior→Map→Detect→Hypothesize→Report shape
(that's spatial-gradient analysis) and *not* the bug-hunt's
Charter→Probe→Oracle→Taxonomize→Report shape (that's exploratory testing). The
shape is cybernetic because the domain is a feedback loop.

### Phase templates (6 executable + 1 reference)

| # | Phase | Template | Type | Ontology anchor | Research source |
|---|---|---|---|---|---|
| 1 | SENSE | `swarm-sense.j2` | KnowAct | Onto4MAT Team/Agent properties; SOSA Observation; ABW `/api/workspaces/{id}` + `/api/wallet` | Labra 2026 (Onto4MAT, Reynolds); ABW plan §0,§4.1 |
| 2 | ORIENT | `swarm-orient.j2` | KnowAct | Ashby requisite variety; PSO cognitive/social balance; Thagard coherence | Bansal 2019 (PSO); ABW plan §4.2; Axelrod (cooperation vs division-of-labor) |
| 3 | DECIDE | `swarm-decide.j2` | KnowAct | PSO velocity update (c₁/c₂/ω); ACO pheromone (deposition/evaporation); Reynolds separation/alignment/cohesion; ABW `dependencies { required, optional }` | Bansal 2019; Labra 2026 (Reynolds); ABW plan §0 (compound agents) |
| 4 | ACT | `swarm-act.j2` | KnowAct | ABW `swarm_update_swarm`/`swarm_delegate`; `DispatchIntent`/`GateDecision`; OCAP consent gate | ABW plan §3.3, §3.6 |
| 5 | CHECK | `swarm-check.j2` | KnowAct | Cybernetic 5-property closure; algedonic channel; `.rules` unwrap_or(0) trap | ABW plan §4.1; pragmatic-cybernetics skill |
| 6 | CONVERGE | (compute step, no .j2) | compute | Cauchy criterion on the convergence metric | kata-improvement skill (convergence pattern) |
| 7 | REF | `swarm-patterns.yaml` | RenderAct | PSO/ACO/Reynolds/Onto4MAT pattern catalog | Bansal 2019; Labra 2026; Reynolds 1987 |

The convergence step (6) is a `compute` action with `compute_ref:
kata.convergence_check`, identical in shape to gradient-hunter's step 6 and
kata-improvement's step 10 — the deterministic Cauchy criterion on a
hypotenuse history. The hypotenuse is the **swarm-state distance metric**
emitted by the CHECK phase (§4).

### Why these phases and not others (essentialist deletion test, applied to the design)

- **No separate "Report" phase.** The CHECK phase *is* the report — it emits
  the convergence metric and the next-iteration focus. A separate Report
  phase would restate CHECK. (gradient-hunt has a Report phase because its
  CHECK is a pure numeric convergence and its Report carries the structured
  gradient findings; here the findings *are* the convergence payload.)
- **No separate "Hypothesize" phase.** There is no counterfactual reasoning to
  discriminate — the swarm state is directly observable via ABW API. A
  Hypothesize phase would be speculative generality (the `.rules`
  "Trait-with-one-impl" trap generalized to phases).
- **No "Curator" phase.** Curator (Xaman Ek) involvement is a *parameter* of
  the ACT phase (`curator_involved` in `DispatchIntent`), not a phase. ABW
  treats it as a per-dispatch consent flag (plan §3.7); the skill mirrors
  that, not a separate orchestration stage.

---

## 3. Manifest Shape (proposed)

```yaml
manifest:
  id: swarm-intelligence
  category: skill
  name: Swarm Intelligence
  description: >
    Convergent swarm-composition process for Agent Bestiary World (ABW) agent
    swarms. Senses current swarm state against the Onto4MAT multi-agent
    teaming ontology (alignment, cohesion, separation) and the ABW workspace/
    wallet APIs; orients by classifying the gap (variety deficit, coherence
    deficit, loop-break) via Ashby's requisite variety and PSO cognitive/
    social balance; decides composition adjustments isomorphic to PSO
    velocity tuning (c₁/c₂/ω), ACO pheromone deposition/evaporation, and
    Reynolds separation/alignment/cohesion; acts via gated
    swarm_update_swarm/swarm_delegate calls with a DispatchIntent consent
    gate; checks by reconciling actual spend and curator data-sharing against
    the algedonic channel; and converges via a Cauchy criterion on the
    swarm-state distance metric. Anchored to Reynolds (1987) flocking,
    Kennedy-Eberhart PSO, Dorigo ACO, Onto4MAT (Kiesel et al. 2022), W3C
    SSN/SOSA, Thagard coherence, Ashby requisite variety, and the ABW
    integration plan (zed-kask 2026-08-01). Composes pragmatic-cybernetics,
    kata-improvement, and essentialist. Emits reg.swarm.* spans.
  functional_role: flowdef
  version: 0.1.0
  editor: curator-or-human-admin
  visibility: Public

convergence:
  convergence_mode: "cauchy"
  cauchy_epsilon: 0.03
  cauchy_window: 3
  max_iterations: 10
  min_iterations: 2
  on_not_reached: escalate
  target_artifacts_field: "current_artifacts"

gas:
  cap: 120000          # higher than gradient-hunter (100k): ABW calls are spend
  cost_per_iteration: 150
  alert_threshold: 0.8
  hard_limit: true

rjoule:
  cap: 3               # DECIDE and ACT use select (composition proposals)
  alert_threshold: 0.8
  hard_limit: true

inputs:
  - name: swarm_id
    type: string
    required: true
    description: >
      The ABW workspace id (= swarm) to compose/steer. Resolved via
      hkask-mcp-swarm's swarm_get_swarm.
  - name: task
    type: string
    required: true
    description: >
      The target task the swarm must be composed for. Drives the
      required_transforms set in the variety check.
  - name: target_condition
    type: object
    required: false
    description: >
      Operator-specified target. Defaults to the §1 TC (variety ≥ 0.9,
      diversity ≥ 0.25, loop_closure = 1.0). Each field overrides a default.
  - name: prior_iteration
    type: object
    required: false
    description: >
      Output of a previous swarm-intelligence iteration on the same swarm.
      Closes the feedback loop: SENSE consumes prior_iteration.next_focus.

# Fusion: omitted — defers to operator's global kask.fusion settings.
# Recommended: mode "best-of-n" with skills: [pragmatic-cybernetics].
# Model names are not hardcoded (per .rules "Manifests must not hardcode
# model names in the fusion block").

steps:
  - ordinal: 1
    action: select
    description: >
      SENSE: Measure current swarm state. Fetch the workspace roster
      (agents[], their accepts[]/produces[]/capabilities/dependencies) and
      the wallet balance/transactions via hkask-mcp-swarm. Compute Onto4MAT
      team properties: alignment (fraction of agents whose produces[] matches
      another agent's accepts[] — the delegation graph density), cohesion
      (fraction of required dependencies satisfied by hired agents), and
      separation (distinct (agent_type, model, temperature) tuples / agent
      count). Record required_transforms derived from the task. Consumes
      prior_iteration.next_focus when present.
    renderer: minijinja
    template_ref: swarm-intelligence/swarm-sense
    gas_cap: 4096
    timeout_seconds: 120   # ABW API latency
    input_mapping:
      swarm_id: "{{ swarm_id }}"
      task: "{{ task }}"
      target_condition: "{{ target_condition | default({}) }}"
      prior_iteration: "{{ prior_iteration | default({}) }}"

  - ordinal: 2
    action: select
    description: >
      ORIENT: Classify the gap between sensed state and target condition into
      one of three deficit classes (or "on-target"): (a) variety deficit —
      required_transforms not covered by hired agents' accepts/produces;
      (b) coherence deficit — Thagard coherence flat or declining, OR
      diversity below floor (premature convergence, the PSO/ACO failure
      mode); (c) loop-break — un-reconciled dispatches or un-acknowledged
      curator data-sharing. Delegates to pragmatic-cybernetics for the
      5-property assessment (polarity, delay, gain, closure, fidelity) when
      the deficit is a loop-break. Classifies the coordination problem
      (Axelrod): is this a cooperation problem (repeated interaction, shadow
      of the future) or a division-of-labor problem (one-shot, complementary
      capabilities)?
    renderer: minijinja
    template_ref: swarm-intelligence/swarm-orient
    gas_cap: 4096
    timeout_seconds: 90
    input_mapping:
      swarm_state: "{{ step_1_result }}"
      task: "{{ task }}"
      target_condition: "{{ target_condition | default({}) }}"

  - ordinal: 3
    action: select
    description: >
      DECIDE: Propose composition adjustments isomorphic to swarm-algorithm
      tuning, mapped onto ABW's hire/fire/delegate vocabulary:
        - Variety deficit → hire agents whose accepts/produces cover the
          missing transforms (ACO pheromone deposition: strengthen the path
          to the uncovered region).
        - Coherence deficit + low diversity → hire a diverse agent
          (raise PSO c₁ cognitive / lower ω inertia — broaden the search);
          fire a redundant duplicate (evaporate the over-deposited pheromone).
        - Coherence deficit + high diversity but low alignment → hire/assign
          a coordinator compound agent (raise PSO c₂ social — pull toward
          gbest; Reynolds alignment rule).
        - Loop-break → do NOT change composition; emit a DispatchIntent
          reconciliation repair (re-query /api/wallet/transactions for the
          un-reconciled dispatch) and a curator-consent backfill.
      Recommendations reference the ABW compound-agent dependencies model
      (required/optional) and the no-delegation-chains invariant.
    renderer: minijinja
    template_ref: swarm-intelligence/swarm-decide
    gas_cap: 6144
    timeout_seconds: 90
    input_mapping:
      swarm_state: "{{ step_1_result }}"
      orientation: "{{ step_2_result }}"
      task: "{{ task }}"

  - ordinal: 4
    action: select
    description: >
      ACT: Emit the gated composition change. For each proposed hire/fire/
      delegate, construct a DispatchIntent { swarm_id, task,
      estimated_credits (from ABW dependencies cost estimate), curator_involved
      (default false per §3.7), data_shared (derived from the agents'
      accepts[] and the task) }. The consent gate is mandatory: no
      swarm_update_swarm or swarm_delegate call is emitted without a signed
      credits_authorized token. If the operator Aborts, the phase records
      the abort reason and emits no spend. This phase enforces the .rules
      "Advertised invariants need enforcement points" trap: the consent gate
      must actually block, not just warn.
    renderer: minijinja
    template_ref: swarm-intelligence/swarm-act
    gas_cap: 4096
    timeout_seconds: 120
    input_mapping:
      decisions: "{{ step_3_result }}"
      swarm_id: "{{ swarm_id }}"
      task: "{{ task }}"

  - ordinal: 5
    action: select
    description: >
      CHECK: Re-measure the swarm state post-Act (re-fetch workspace + wallet).
      Compute the convergence metric: a composite swarm-state distance
      d = sqrt( (1 - variety_coverage)² + max(0, diversity_floor - diversity)²
                + (1 - loop_closure)² ).
      d = 0 means the target condition is met on all three axes. Emit
      next_focus: if variety is the largest term, focus next iteration on
      hiring; if diversity, on diversification; if loop_closure, on
      reconciliation. Also emit the algedonic signal: any 402 / un-acknowledged
      curator dispatch is surfaced as a loop-break regardless of d.
    renderer: minijinja
    template_ref: swarm-intelligence/swarm-check
    gas_cap: 4096
    timeout_seconds: 120
    input_mapping:
      pre_act_state: "{{ step_1_result }}"
      act_result: "{{ step_4_result }}"
      swarm_id: "{{ swarm_id }}"
      target_condition: "{{ target_condition | default({}) }}"

  - ordinal: 6
    action: compute
    compute_ref: kata.convergence_check
    description: "CHECK CONVERGENCE: Cauchy criterion on the swarm-state distance d. Deterministic."
    input_mapping:
      hypotenuse: 0.0
      hypotenuse_epsilon: "{{ _convergence.hypotenuse_epsilon | default(0.05) }}"
      cauchy_epsilon: "{{ _convergence.cauchy_epsilon | default(0.03) }}"
      cauchy_window: "{{ _convergence.cauchy_window | default(3) }}"
      brier_history: "{{ _convergence.brier_history | default([]) }}"
      hypotenuse_history: "{{ _convergence.hypotenuse_history | default([]) }}"
      brier_threshold: "{{ _convergence.brier_threshold | default(0.15) }}"
      brier_window: "{{ _convergence.brier_window | default(3) }}"
      mode: "cauchy"

  - ordinal: 7
    action: loop
    description: >
      Re-enter the swarm-intelligence cycle if convergence is not met. Routes
      step_6_result.next_focus back to step 1 as prior_iteration.next_focus,
      along with the prior CHECK's lessons_learned, closing the feedback loop.
    input_mapping:
      loop_target: "{{ 1 }}"
      prior_iteration:
        lessons_learned: "{{ step_5_result.lessons_learned | default([]) }}"
        next_focus: "{{ step_6_result.next_focus | default('') }}"
      kata_hypotenuse: "{{ step_6_result.convergence_metric | default(1.0) }}"

error_handling:
  on_gas_exceeded: abort
  on_timeout: retry
  max_retries: 1
  retry_backoff_seconds: 2     # ABW rate-limit friendly
  on_validation_failure: abort

ocap:
  required_capabilities:
    - resource: template
      action: render
      template_id: swarm-intelligence/swarm-sense
      gas_budget: 4096
    - resource: template
      action: render
      template_id: swarm-intelligence/swarm-orient
      gas_budget: 4096
    - resource: template
      action: render
      template_id: swarm-intelligence/swarm-decide
      gas_budget: 6144
    - resource: template
      action: render
      template_id: swarm-intelligence/swarm-act
      gas_budget: 4096
    - resource: template
      action: render
      template_id: swarm-intelligence/swarm-check
      gas_budget: 4096
    - resource: manifest
      action: execute
      template_id: swarm-intelligence
      gas_budget: 1000
  delegation_chain_required: true
  signature_algorithm: ed25519
  capability_expiry_seconds: 3600
  template_scoped: true

ledger:
  emit_spans: true
  span_namespace: reg.swarm
  telemetry_namespace: hkask.template.swarm-intelligence
  variety_monitoring: true
  algedonic_threshold: 100
  escalation_target: Curator
```

---

## 4. Q3 — Convergence Criterion (deterministic, not LLM-judged)

**Convergence metric:** the swarm-state distance

```
d = sqrt( (1 - variety_coverage)²
        + max(0, diversity_floor - diversity)²
        + (1 - loop_closure)² )
```

where:
- `variety_coverage = |required_transforms ∩ covered_transforms| / |required_transforms|`
  — computed deterministically from the ABW agent cards' `accepts[]`/`produces[]`
  and the task's `required_transforms`. No LLM judgment.
- `diversity = |distinct (agent_type, model) tuples| / |agents|` — computed
  deterministically from the workspace roster. `diversity_floor = 0.25`.
- `loop_closure = (reconciled_dispatches / total_dispatches) ∧
  (acknowledged_curator_dispatches / curator_dispatches)` — computed
  deterministically from `/api/wallet/transactions` and the consent-gate log.

**Convergence criterion (Cauchy):** the sequence `d_1, d_2, …, d_n` has
converged when, for `cauchy_window = 3` consecutive iterations,
`|d_i − d_{i−1}| < cauchy_epsilon = 0.03`. This is the *exact* criterion used
by `kata-improvement` (step 10), `gradient-hunter` (step 6), and
`pragmatic-cybernetics` (step 4) — implemented by `compute_ref:
kata.convergence_check` with `mode: "cauchy"`. It is deterministic: the
`compute` action runs no LLM, and the hypotenuse history is the sequence of
`d` values pushed via `kata_hypotenuse` in the loop step.

**Why Cauchy and not "d < ε"?** A single sub-ε reading can be a lucky
configuration; the Cauchy criterion requires the *sequence* to stabilize,
which is the correct semantics for "the swarm composition has stopped needing
to change." This matches the kata-improvement rationale verbatim.

**Algedonic override:** regardless of `d`, if the CHECK phase detects a 402
(payment required) or an un-acknowledged curator dispatch, the loop does *not*
claim convergence — it escalates (`on_not_reached: escalate`). This is the
`.rules` "unwrap_or(0) on regulation-loop sense inputs" trap enforced as a
convergence invariant: a broken algedonic channel is never read as "no
deviation."

---

## 5. Traceability Table (design decision → research source)

| Design decision | Research source | Phase |
|---|---|---|
| PDCA shape = sense→orient→decide→act→check→loop | ABW plan §4.1 (the feedback loop the swarm panel must close); pragmatic-cybernetics skill (OODA/Regulation shape) | all |
| Swarm state measured via Onto4MAT alignment/cohesion/separation | Labra 2026 `cross_domain_swarm_intelligence.pdf` (Onto4MAT, Kiesel et al. 2022; Reynolds 1987) | SENSE |
| `accepts[]`/`produces[]` type-pairs as the variety substrate | ABW plan §0 (verified agent card schema) | SENSE, ORIENT |
| Variety deficit class via Ashby requisite variety | ABW plan §4.2 (Ashby variety check); pragmatic-cybernetics skill | ORIENT |
| Coherence deficit + diversity floor as premature-convergence detection | Bansal 2019 (PSO/ACO failure mode: "unable to manage a proper balance between exploration and exploitation"); Thagard coherence (ABW plan §0) | ORIENT |
| Loop-break deficit class via cybernetic 5-property + algedonic | ABW plan §4.1 (5-property assessment); `.rules` "unwrap_or(0)" trap | ORIENT, CHECK |
| Cooperation vs division-of-labor classification | Axelrod 1984 (cooperation without central authority); Henrich (cooperative dilemmas) | ORIENT |
| Hire-to-cover = ACO pheromone deposition | Bansal 2019 (ACO: "As more ants find the path, the concentration of pheromone gets stronger") | DECIDE |
| Diversify = raise PSO c₁ / lower ω | Bansal 2019 (PSO velocity update: c₁ cognitive, ω inertia; "cognitive scaling parameter c₁ regulates the maximum step size in the direction of the personal best") | DECIDE |
| Add coordinator = raise PSO c₂ / Reynolds alignment | Bansal 2019 (c₂ social); Labra 2026 (Reynolds alignment rule) | DECIDE |
| Fire redundant = ACO pheromone evaporation | Bansal 2019 (ACO: "pheromone evaporates with time and hence the longer paths will have more evaporation") | DECIDE |
| No-delegation-chains invariant respected | ABW plan §0 ("delegation chains forbidden — delegates receive all workspace tools except delegate_to_agent/execute_agent") | DECIDE, ACT |
| `DispatchIntent` + `GateDecision` consent gate | ABW plan §3.6 (cost/consent gate is the critical new build) | ACT |
| `curator_consent_default: false` | ABW plan §3.7 (Private/opt-in is the default) | ACT |
| Convergence metric `d` is deterministic (no LLM judgment) | kata-improvement skill (Cauchy convergence pattern); `.rules` "Advertised invariants need enforcement points" | CHECK, CONVERGE |
| Algedonic override on 402 / un-acknowledged curator | `.rules` "unwrap_or(0) on regulation-loop sense inputs is a broken feedback loop"; ABW plan §4.1 (closure broken unless cost + curator feed back) | CHECK, CONVERGE |
| Autopoietic loop (next iteration depends on prior findings) | `bug-hunting-as-autopoietic-skill-unified.md` (Maturana & Varela; Luhmann) | LOOP |
| Ensemble model NOT assumed (ABW is the current framework) | User correction (2026-08-01): ensemble deprecated, ABW is the agentic-swarm framework; corroborated by `personas-r7.md` ("No swarms per spec" referred to the *ensemble* bots, not ABW) | all (D1) |
| No separate Report/Hypothesize/Curator phases | essentialist skill (deletion test); `.rules` "Trait-with-one-impl is speculative generality" generalized to phases | §2 |

---

## 6. Open Uncertainties (listed explicitly, not resolved by speculation)

- **U1 — GitHub repo-level pattern extraction not completed.** The
  sub-agent survey was canceled before results returned. The design cites
  canonical *papers* (Kennedy-Eberhart 1995, Dorigo 1992, Reynolds 1987) for
  PSO/ACO/boids; repo-level implementation patterns (e.g., specific
  stagnation-detection heuristics, niching strategies) were not extracted.
  If the skill's DECIDE phase needs a richer tuning palette than
  c₁/c₂/ω + pheromone + Reynolds, a follow-up GitHub survey pass is needed.

- **U2 — ABW Thagard coherence score is referenced but its exact computation
  is unverified.** The integration plan §0 mentions "Thagard coherence
  scoring" on workspaces but the score is not exposed in the verified API
  surface (the `/api/workspaces/{id}` roster does not show a coherence field
  in the recon). The ORIENT phase assumes a coherence signal is available;
  if it is not, the phase must fall back to a proxy (delegation graph density
  from `accepts[]`/`produces[]` overlap). This needs an authenticated probe
  of `/api/workspaces/{id}` with a populated workspace.

- **U3 — ABW streaming granularity unknown.** The integration plan §0 lists
  "Streaming granularity — unknown" as a blocking question. If swarm
  dispatch is asynchronous with a pollable run id (unverified), the SENSE and
  CHECK phases' timeout budgets (120s) may be too short for long compound
  runs. The design assumes synchronous-enough state reads; an async ABW
  would require a poll loop in SENSE/CHECK.

- **U4 — `required_transforms` derivation from the task is the one LLM-judged
  step.** The variety metric is deterministic *given* `required_transforms`,
  but deriving `required_transforms` from the natural-language `task` input
  is an LLM judgment inside the SENSE template. This is bounded (the SENSE
  template emits a structured transform set that CHECK then measures
  against), but it is the one place the loop's *inputs* are not fully
  deterministic. The convergence *criterion* remains deterministic; the
  *input* is not. Recorded honestly rather than hidden.

- **U5 — ABW compound-agent `dependencies` auto-hire interaction with the
  consent gate.** ABW compound agents "auto-hire their team" (plan §0). It
  is unverified whether auto-hire bypasses the per-dispatch consent gate or
  whether the gate intercepts the auto-hire. The ACT phase assumes the gate
  intercepts all spend including auto-hire; if ABW auto-hires server-side
  before the gate sees it, the loop-closure metric would need to account for
  "spend that occurred without a DispatchIntent" as a loop-break by
  definition.

- **U6 — Cross-workspace swarms are out of scope (v1).** The integration
  plan §1 non-goal: "Cross-workspace swarm coordination. v1 is
  single-workspace." The skill inherits this scope. A multi-workspace
  extension would need a federation layer in SENSE (Axelrod/Henrich
  cooperation anchors become load-bearing here) and is left as v2.

---

## 7. Grill-Me Readiness (pre-check before the formal round)

The acceptance criterion requires the design to survive one grill-me round
without `rewrite_needed`. Before invoking grill-me, the design self-checks
against the five grill-me levels:

- **Recall:** Can I state the target condition? Yes — §1, three measurable
  axes, deterministic metrics.
- **Mechanism:** Can I explain *why* the PDCA shape is sense→orient→decide→
  act→check and not something else? Yes — §2: it is the cybernetic feedback
  loop from the integration plan §4.1, not a generic template.
- **Rationale:** Why Cauchy and not "d < ε"? §4: a single sub-ε reading can
  be lucky; the sequence-stabilization semantics match "composition has
  stopped needing to change."
- **Edge cases:** What happens on a 402 mid-loop? §4 algedonic override:
  escalate, do not claim convergence. What if `required_transforms` is
  empty (trivial task)? variety_coverage is undefined (0/0) — the SENSE
  template must define it as 1.0 (vacuously satisfied) and flag it. **This
  is a gap to fix in the .j2 template, recorded here.**
- **Synthesis:** How does this differ from gradient-hunter and bug-hunt?
  §2: different ontology (Onto4MAT + ABW, not spin-glass or Beizer), different
  shape (cybernetic OODA, not spatial-gradient or exploratory-testing).

The formal grill-me round is the next step (see §8).

---

## 7b. Grill-Me Round (executed 2026-08-01)

The grill-me skill was run on this design across all five levels. The round
was conducted as a rigorous self-interrogation probing the design's weakest
points. Result: **survived without `rewrite_needed`**. One Partial (edge
cases) produced two template-level fixes, recorded below.

| Level | Question (abbreviated) | Answer source | Verdict |
|---|---|---|---|
| 1 Recall | State the target condition and its three axes. | §1 | Solid |
| 2 Mechanism | Why sense→orient→decide→act→check and not gradient-hunt's Prior→Map→Detect→Hypothesize→Report? | §2: the domain is a cybernetic feedback loop (ABW plan §4.1), not spatial-gradient analysis; the shape emerges from the ontology. | Solid |
| 3 Rationale | Why Cauchy and not "d < ε"? Why is the algedonic override necessary? | §4: single sub-ε reading can be lucky; Cauchy requires sequence stabilization. Override enforces the `.rules` "unwrap_or(0)" trap — a 402 read as "no deviation" is a reinforcing loop. | Solid |
| 4 Edge Cases | What happens when `required_transforms` is empty (0/0)? When `|agents| = 0` (diversity 0/0)? When `total_dispatches = 0` (loop_closure 0/0)? | §7 caught only the first; the other two were unhandled. | **Partial** |
| 5 Synthesis | How does this differ from just running pragmatic-cybernetics on the swarm panel? Isn't the cybernetic analysis already in plan §4? | pragmatic-cybernetics is diagnostic (analyzes loop properties); swarm-intelligence is compositional (adds the PSO/ACO/Reynolds tuning palette + Onto4MAT substrate) and *iterative* (repeatable PDCA vs one-time design analysis). Delegates to pragmatic-cybernetics in ORIENT, doesn't duplicate it. | Solid |

### Fixes from the Partial (Level 4)

The design caught one of three 0/0 edge cases. The other two are metric-
definition gaps, not design-shape gaps — fixed here, not deferred:

- **Empty `required_transforms`** (trivial task): `variety_coverage` is 0/0.
  Define as **1.0** (vacuously satisfied — no required transform is
  uncovered) and emit a `trivial_task` flag so ORIENT can decide whether the
  swarm is over-provisioned for a trivial task.
- **Empty swarm** (`|agents| = 0`): `diversity` is 0/0. Define as **0.0**
  (an empty swarm has zero diversity — it fails the floor by definition) so
  the loop correctly drives the first hire. `variety_coverage` is also 0/0
  here when `required_transforms` is non-empty → define as **0.0** (nothing
  is covered).
- **Zero dispatches** (`total_dispatches = 0`): `loop_closure` is 0/0.
  Define as **1.0** (vacuously — no dispatch is un-reconciled) so a freshly
  composed swarm that has not yet dispatched is not falsely flagged as a
  loop-break. The algedonic override still fires on a 402 from a *failed*
  dispatch attempt.

These three vacuous-truth defaults are **metric definitions**, to be encoded
in the SENSE and CHECK templates during scaffolding. They do not change the
PDCA shape or the convergence criterion.

## 8. Next Steps

1. **Grill-me round.** Invoke the `grill-me` skill on this design document
   with the five-level escalation. Target: survive without `rewrite_needed`.
   If a level surfaces a gap, fix the design and re-run.
2. **Scaffold the registry crate.** If grill-me passes, invoke
   `create-skill` (which delegates to `skill-maintenance-build`) to scaffold:
   - `kask/registry/templates/swarm-intelligence/manifest.yaml`
   - `kask/registry/templates/swarm-intelligence/swarm-{sense,orient,decide,act,check}.j2`
   - `kask/registry/templates/swarm-intelligence/swarm-patterns.yaml`
   - `kask/registry/manifests/swarm-intelligence.yaml`
   - `.agents/skills/swarm-intelligence/SKILL.md`
3. **Validate.** `skill-maintenance-validate` against R1-R12, Z1-Z8, X1-X4,
   E1-E10.
4. **Fix U4 edge case** (empty `required_transforms` → vacuous satisfaction)
   in the SENSE template during scaffolding.
