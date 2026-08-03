---
name: swarm-intelligence
visibility: public
description: "Convergent swarm-composition process for agent swarms. Senses swarm state against the Onto4MAT multi-agent teaming ontology and the swarm backend's APIs; orients via Ashby's requisite variety and PSO cognitive/social balance; decides composition adjustments isomorphic to PSO velocity tuning, ACO pheromone deposition, and Reynolds separation/alignment/cohesion; acts via gated swarm_delegate/swarm_delegate_local calls with a budget gate; checks spend against the algedonic channel; converges via a Cauchy criterion on the swarm-state distance metric. Mode-aware (v2 §15): abw mode fetches from ABW REST and delegates via swarm_delegate; local mode reads the local agent registry + ledger and delegates via swarm_delegate_local. Anchored to Reynolds flocking, Kennedy-Eberhart PSO, Dorigo ACO, Onto4MAT, W3C SSN/SOSA, Thagard coherence, and Ashby requisite variety. Composes pragmatic-cybernetics, kata-improvement, essentialist. Emits reg.swarm.* spans. Any userpod may invoke this skill."
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
Check: Phase 6  — CHECK             → Re-measure, compute swarm-state distance d, emit next_focus + algedonic + blame_count (C5)
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

| Component | What | Enforcement point |
|-----------|------|--------------------|
| **C0** task-success `s` | Deterministic evaluator verdict → fourth axis of `d` | CHECK template + manifest `task_success` input |
| **C1** second-order monitor | Reasoning-loop + sensor-truth-divergence detection over the iteration log | `swarm.second_order_monitor` compute step (9) |
| **C2** Go See cadence | Scheduled human check every N convergences + event trigger | `cadence_every` param in the monitor; SENSE surfaces `go_see` |
| **C3** failed-edit memory | Anti-loop set; the FILTER drops moves matching prior failed signatures | `swarm.filter_proposed_moves` compute step (4) |
| **C4** latency `T_q` | End-to-end delegation latency measurement | `LocalDelegateResult.latency_ms` |
| **C5** fault attribution | Deterministic priority rule over the delegate trace; blame aggregation | ORIENT template + CHECK `blame_count` |
| **C6** reconfigure_agent | Re-prompt a blamed agent in place (Modify-Block / MASS prompt axis) | `swarm_reconfigure_local_agent` tool + DECIDE move type |
| **C7** influence-weighted rejection | Reject re-hire of agent types measured to degrade the swarm | `swarm.filter_proposed_moves` compute step (4) |
| **C8** task-gated alignment | Task-conditional edge relevance in SENSE (OFA-MAS TAGSE port) | SENSE template `alignment` definition |

## Composed Skills

| Skill | Role | When Invoked |
|-------|------|-------------|
| `pragmatic-cybernetics` | 5-property loop assessment + Ashby variety + VSM | ORIENT, when deficit is a loop-break |
| `kata-improvement` | Cauchy convergence pattern (`kata.convergence_check`) | CONVERGE (compute step) |
| `essentialist` | Deletion-test proposed phases | Design-time (applied to the skill itself) |

## Registry

Registry is authoritative — when this SKILL.md disagrees with registry
templates, the registry wins.

- Template manifest: `kask/registry/templates/swarm-intelligence/manifest.yaml`
- Templates: `kask/registry/templates/swarm-intelligence/swarm-{sense,orient,decide,act,check}.j2`
- Reference: `kask/registry/templates/swarm-intelligence/swarm-patterns.yaml`
- Process manifest: `kask/registry/manifests/swarm-intelligence.yaml`
