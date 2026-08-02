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
Check: Phase 1 — SENSE     → Measure current swarm state against Onto4MAT + ABW workspace/wallet
Plan:  Phase 2 — ORIENT    → Classify the gap: variety deficit | coherence deficit | loop-break
Plan:  Phase 3 — DECIDE    → Propose composition adjustments isomorphic to PSO/ACO/Reynolds tuning
Do:    Phase 4 — ACT       → Emit gated swarm_hire / swarm_delegate with a consent token
Check: Phase 5 — CHECK     → Re-measure, compute swarm-state distance d, emit next_focus + algedonic
Check: Phase 6 — CONVERGE  → Cauchy criterion on d (deterministic, no LLM judgment)
Act:   Phase 7 — LOOP      → Re-enter SENSE with prior_iteration if not converged
```

The shape is cybernetic (sense → orient → decide → act → check), not the
gradient-hunter's Prior→Map→Detect→Hypothesize→Report (spatial-gradient
analysis) or the bug-hunt's Charter→Probe→Oracle→Taxonomize→Report
(exploratory testing). The shape emerges from the domain: a swarm is a
feedback loop, so the skill is a feedback loop.

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
The sequence `d_1, d_2, …` has converged when `|d_i − d_{i−1}| < 0.03` for 3
consecutive iterations. **Algedonic override:** a 402 or un-acknowledged
curator dispatch escalates regardless of `d` — a broken algedonic channel is
never read as "no deviation" (the `.rules` "unwrap_or(0)" trap enforced as a
convergence invariant).

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
