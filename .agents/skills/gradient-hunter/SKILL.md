---
name: gradient-hunter
visibility: public
description: "Find steep gradients between populated and unpopulated regions of a codebase, telemetry, or test field and investigate the reason. The signal is in the gradient shape and its cause, not in the absence itself."
---

# Gradient Hunter

Find steep gradients between populated and unpopulated regions of a field, and investigate *why* the gradient exists. The signal is in the gradient shape and its cause — not in the absence itself. A desert is only meaningful relative to an expected oasis; a missing test is only interesting if neighboring code has tests; a silent hook is only a bug if sibling hooks log warnings.

## Substrate ontology: non-ergodicity and information storage

Grounded in Parisi's spin glass theory (Parisi 1979; Sherrington-Kirkpatrick 1975; Nobel 2021). Non-ergodic systems that store work, information, or memory have **rugged energy landscapes with metastable valleys separated by barriers**. The energy gradient is non-monotonic, so following the local gradient gets you stuck in a local minimum, not the global optimum.

A "desert" is therefore not necessarily an absence — it is often a **metastable valley** the system has relaxed into and cannot leave without crossing an energy barrier. The system stores information *by being trapped*. A gradient between two metastable states is a gradient between two memories.

The seven surface ontologies (below, in `gradient-shapes.yaml`) describe the *shape* of gradients at boundaries. The spin glass substrate explains *why there are boundaries at all*. Before classifying a gradient's shape, consider whether the desert is a valley the system is trapped in — if so, "add the missing artifact" will not work; the system will relax back. The intervention must inject enough energy to cross the barrier (a deferred task, a startup signal, a reconfiguration that reshapes the landscape itself).

## When to Use

- Audit a crate for missing tests, but only where neighboring code has tests (test-coverage gradient)
- Audit a subsystem for missing telemetry, but only where sibling subsystems emit spans (telemetry cliff)
- Audit a config for missing failure signals, but only where sibling configs have them (silent-failure asymmetry)
- Find `// zed-kask:` comments without paired tests, but only where other deviations have tests
- Find hooks wired inconsistently with their siblings (hook-signal gradient)
- Find span namespaces declared but never emitted, but only where sibling namespaces are emitted
- Diagnose whether an agentic orchestrator's strategy ensemble is being shifted by the task (allosteric coupling check)
- Any "dog that didn't bark" investigation where you expect data/activity on both sides of a region

Do NOT use for:
- Pure absence checking with no prior ("does this crate have any tests?" — that's a checklist, not gradient analysis)
- Positive-space threat hunting (use `bug-hunt`)
- Reasoning-context ellipses (use `metacognition`)
- Reproducing a known symptom (use `diagnose` — deserts are asymptomatic until you have a prior)

## PDCA Loop

```
Plan:   Phase 1 — Prior       → Build expected-field model (sibling/convention/principle)
Do:     Phase 2 — Map         → Measure actual field with prior's granularity
Do:     Phase 3 — Detect      → Classify shape, scale, domain, fractal recurrence
Check:  Phase 4 — Hypothesize → Generate reason hypotheses (Rubin + spin glass + allostery)
Act:    Phase 5 — Report      → Prioritized gradient report with lessons + pattern signatures
Check:  Phase 6 — Converge    → Composite metric + next_prior_focus for loop closure
Act:    Phase 7 — Loop        → If not converged, re-enter at Phase 1 with refined prior
```

Feedback loop closure: convergence emits `next_prior_focus` (consumed by next iteration's Prior); Report emits `lessons_learned` and `pattern_signatures` (consumed by next iteration's Prior and Detect).

## Improvement Measure

**Field**: `step_6_result.convergence_metric`. **Threshold**: 0.25. **Max iterations**: 3.

| Score | Meaning |
|-------|---------|
| 0.00 | Fully stabilized — all gradients re-confirmed, full field coverage |
| 0.25 | Converged at threshold — adequate for action, minor unexplored surface |
| 0.50 | Not converged — many new gradients or significant unexplored surface |
| 1.00 | First iteration or no meaningful mapping performed |

Composite of two sub-metrics (weighted 0.5/0.5):

1. **process_stabilization_metric** (0.0–1.0): gradient overlap across iterations. High overlap = stabilization (low metric). First iteration = 1.0.
2. **field_coverage_estimate** (0.0–1.0): fraction of expected field mapped. Honest estimate — false precision is worse than honest ignorance.

## Composed Skills

| Skill | Role | When Invoked |
|-------|------|-------------|
| `graph-audit` (code mode) | Field topology extraction | Map phase — when hunting topology gradients (orphan nodes, missing edges, disconnected components) |
| `pragmatic-cybernetics` | Prior modeling | Prior phase — when no sibling or convention prior is available; models expected field via variety engineering |
| `falsifiability` | Counterfactual discrimination | Hypothesize phase — when discriminating between reason hypotheses |
| `metacognition` | Prior perspective rotation | Hypothesize phase — different priors surface different gradients; if a gradient disappears under a different prior, it was a prior artifact |

### Composition Protocol

1. **Prior first** — always start with gradient-prior; implicit priors produce implicit, unfalsifiable gradients.
2. **Map with matching granularity** — granularity mismatch produces false gradients.
3. **Detect with fractal check** — the fractal recurrence check is mandatory, not optional.
4. **Hypothesize with multiple reasons** — at least 2-3 hypotheses per gradient from different reason classes. Do not collapse to the first match.
5. **Delegate when needed** — topology gradients → `graph-audit`; no sibling/convention prior → `pragmatic-cybernetics`; non-obvious discrimination → `falsifiability`; prior may be wrong → `metacognition`.
6. **Report with feedback** — emit `lessons_learned` and `pattern_signatures` for the next iteration.
7. **Converge honestly** — false precision is worse than honest uncertainty.

## Shape and Reason Taxonomy

The eight gradient shapes (sharp cliff, roof edge, wombling boundary, regression discontinuity, topological hole, oracle gap, frustrated landscape, allosteric population shift) and the seven reason classes (intentional boundary / MCAR, explainable gap / MAR, forgotten wire / MNAR, stale refactor / MNAR drift, scope creep / MNAR missing abstraction, metastable trap / spin glass, broken allosteric coupling / allostery) are defined authoritatively in `gradient-shapes.yaml` in the registry crate, including ontology anchors, fractal recurrence across scales/domains, and priority ordering.

Key non-obvious rules the taxonomy encodes:

- Shapes are fractal — the same shape recurs at pixel, function, subsystem, and organization scales. Recognize the shape regardless of domain.
- Each shape implies a different reason family and a different intervention. A sharp cliff is fixed by adding the wire; a frustrated landscape is fixed by reshaping the landscape; an allosteric population shift is fixed by restoring the distal coupling, not by adding the missing conformation. Conflating them produces wrong interventions.
- The metastable trap reason class must be considered for every gradient. The default assumption that "the artifact is missing" is an ergodic-system assumption; non-ergodic systems trap.
- The broken allosteric coupling reason class must be considered for every gradient in an orchestrator context. The coupling is broken, not the strategy.
- Priority: broken allosteric coupling > metastable trap > MNAR > MAR > MCAR, then fractal recurrence, then magnitude, then populated-side criticality.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `gradient-prior.j2` | `KnowAct` | Build prior model of expected field (sibling/convention/principle). Delegates to pragmatic-cybernetics. |
| `gradient-map.j2` | `KnowAct` | Map actual field with prior's granularity. Delegates to graph-audit for topology. |
| `gradient-detect.j2` | `KnowAct` | Detect gradients and classify shape, scale, domain, fractal recurrence. References gradient-shapes.yaml. |
| `gradient-hypothesize.j2` | `KnowAct` | Generate reason hypotheses via Rubin + spin glass + allostery taxonomy. Delegates to falsifiability and metacognition. |
| `gradient-report.j2` | `KnowAct` | Compile structured gradient report with priority ranking, lessons_learned, pattern_signatures. |
| `gradient-shapes.yaml` | `RenderAct` | Reference: eight gradient shape ontology with fractal recurrence, reason classes, and priority ordering. |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- Energy caps: prior (4096), map (4096), detect (6144), hypothesize (6144), report (4096).
- Gas cap: 100,000 per invocation. Maximum 3 iterations.
- The prior must be explicit before gradient detection.
- The fractal recurrence check is mandatory.
- Do not collapse the eight ontologies into one — each shape implies a different intervention.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
