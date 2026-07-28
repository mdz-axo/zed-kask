---
name: gradient-hunter
visibility: public
description: Find steep gradients between populated and unpopulated regions of a codebase/telemetry/test field and investigate the reason for the gradient. The signal is in the gradient shape and its cause, not in the absence itself. Anchored to a substrate ontology (Parisi spin glass theory: non-ergodicity, frustration, metastable valleys) and seven surface gradient-shape ontologies (wombling, regression discontinuity, Rubin MCAR/MAR/MNAR, persistent homology, edge detection, oracle gap, Monod allostery with agentic-orchestrator corollary). Decomposed into phased templates: Prior → Map → Detect → Hypothesize → Report → Convergence. Composes graph-audit, pragmatic-cybernetics, falsifiability, metacognition. Emits reg.gradient.* spans. Any userpod may invoke this skill.
---

# Gradient Hunter

Find steep gradients between populated and unpopulated regions of a field, and investigate *why* the gradient exists. The signal is in the gradient shape and its cause — not in the absence itself.

A desert is only meaningful relative to an expected oasis. A missing test is only interesting if neighboring code has tests. A silent hook is only a bug if sibling hooks log warnings. The signal lives in the *gradient* — the boundary between populated and unpopulated — and in the *reason* for the gradient.

## Substrate ontology: non-ergodicity and information storage

The deep insight that grounds this skill comes from Parisi's spin glass theory (Parisi 1979; Sherrington-Kirkpatrick 1975; Nobel 2021). All non-ergodic systems that store work, information, or memory — including living systems and the materials that compute — are composed of, or leverage, spin glass properties: **rugged energy landscapes with metastable valleys separated by barriers**. The energy gradient is non-monotonic (unlike the smooth monotonic landscape entropy predicts for ergodic systems), so following the local gradient gets you stuck in a local minimum, not the global optimum.

This means a "desert" is not necessarily an absence. It is often a **metastable valley** the system has relaxed into and cannot leave without crossing an energy barrier. The system stores information *by being trapped*. A gradient between two metastable states is a gradient between two memories.

The seven surface ontologies below describe the *shape* of gradients at boundaries. The spin glass substrate explains *why there are boundaries at all*: the system is non-ergodic, and its metastable states are its memory. Before classifying a gradient's shape, consider whether the desert is a valley the system is trapped in — in which case "add the missing artifact" will not work; the system will relax back. The intervention must inject enough energy to cross the barrier (a deferred task, a startup signal, a reconfiguration that reshapes the landscape itself).

## When to Use

- Audit a crate for missing tests, but only where neighboring code has tests (test-coverage gradient)
- Audit a subsystem for missing telemetry, but only where sibling subsystems emit spans (telemetry cliff)
- Audit a config for missing failure signals, but only where sibling configs have them (silent-failure asymmetry)
- Find `// zed-kask:` comments without paired tests, but only where other deviations have tests (comment-test pairing gradient)
- Find hooks wired inconsistently with their siblings (hook-signal gradient)
- Find span namespaces declared but never emitted, but only where sibling namespaces are emitted (span-emission hole)
- Diagnose whether an agentic orchestrator's strategy ensemble is being shifted by the task (allosteric coupling check)
- Any "dog that didn't bark" investigation where you expect data/activity on both sides of a region

Do NOT use for:
- Pure absence checking with no prior ("does this crate have any tests?" — that's a checklist, not gradient analysis)
- Positive-space threat hunting (use `bug-hunt`)
- Reasoning-context ellipses (use `metacognition`)
- Reproducing a known symptom (use `diagnose` — deserts are asymptomatic until you have a prior)

## PDCA Loop

The skill follows a **Plan → Do → Check → Act** cycle with feedback loop closure:

```
Plan:   Phase 1 — Prior       → Build expected-field model (sibling/convention/principle)
Do:     Phase 2 — Map         → Measure actual field with prior's granularity
Do:     Phase 3 — Detect      → Classify shape, scale, domain, fractal recurrence
Check:  Phase 4 — Hypothesize → Generate reason hypotheses (Rubin + spin glass + allostery)
Act:    Phase 5 — Report      → Prioritized gradient report with lessons + pattern signatures
Check:  Phase 6 — Converge    → Composite metric + next_prior_focus for loop closure
Act:    Phase 7 — Loop        → If not converged, re-enter at Phase 1 with refined prior
```

Feedback loop closure: the convergence check emits `next_prior_focus`, which the next iteration's Prior phase consumes as `prior_iteration.next_prior_focus`. The Report phase emits `lessons_learned` and `pattern_signatures`, which the next iteration's Prior and Detect phases consume.

## Improvement Measure

### Convergence Metric: Composite Stabilization + Coverage

**Field**: `step_6_result.convergence_metric`

| Score | Meaning |
|-------|---------|
| 0.00 | Fully stabilized — all gradients re-confirmed, full field coverage |
| 0.25 | Converged at threshold — adequate for action, minor unexplored surface |
| 0.50 | Not converged — many new gradients or significant unexplored surface |
| 1.00 | First iteration or no meaningful mapping performed |

**Threshold**: 0.25. **Max iterations**: 3.

**Scoring breakdown** (composite of two sub-metrics, weighted 0.5/0.5):

1. **process_stabilization_metric** (0.0–1.0): gradient overlap across iterations. High overlap = stabilization (low metric). Low overlap = new gradients (high metric). First iteration = 1.0.
2. **field_coverage_estimate** (0.0–1.0): fraction of expected field mapped. Considers prior's expected_field elements measured, scales surveyed, domains surveyed, fractal recurrence patterns suggesting unexplored surface. Honest estimate — false precision is worse than honest ignorance.

## Composed Skills

Gradient-hunter composes other skills for deeper analysis:

| Skill | Role | When Invoked |
|-------|------|-------------|
| `graph-audit` (code mode) | Field topology extraction | Map phase — when hunting topology gradients (orphan nodes, missing edges, disconnected components) |
| `pragmatic-cybernetics` | Prior modeling | Prior phase — when no sibling or convention prior is available; models expected field via variety engineering |
| `falsifiability` | Counterfactual discrimination | Hypothesize phase — when discriminating between reason hypotheses ("if this desert were intentional, what else would be true?") |
| `metacognition` | Prior perspective rotation | Hypothesize phase — different priors surface different gradients; if a gradient disappears under a different prior, it was a prior artifact |

### Composition Protocol

1. **Prior first** — Always start with gradient-prior to build the expected-field model.
2. **Map with matching granularity** — Use gradient-map with the prior's granularity. Granularity mismatch produces false gradients.
3. **Detect with fractal check** — The fractal recurrence check is mandatory, not optional.
4. **Hypothesize with multiple reasons** — Generate at least 2-3 hypotheses per gradient from different reason classes. Do not collapse to the first match.
5. **Delegate when needed** — If the assessment reveals:
   - Topology gradients → delegate to `graph-audit` (code mode)
   - No sibling/convention prior → delegate to `pragmatic-cybernetics`
   - Non-obvious hypothesis discrimination → delegate to `falsifiability`
   - Prior may be wrong → delegate to `metacognition` for perspective rotation
6. **Report with feedback** — Emit lessons_learned and pattern_signatures for the next iteration.
7. **Converge honestly** — Use the composite metric. False precision is worse than honest uncertainty.

## Gradient Shape Taxonomy (ontological anchors)

The shapes below are drawn from seven academic domains. Each domain formalizes a different kind of gradient that recurs across scales and fields — the same shape appears in image pixels, in code coverage, in spatial epidemiology, in spin glass energy landscapes, in protein conformational ensembles. The gradient-hunter's job is to recognize the shape regardless of the domain it appears in.

Full reference: `gradient-shapes.yaml` in the registry crate.

| Shape | Ontology | Anchor | Reason family |
|---|---|---|---|
| **Sharp cliff** | Image processing | Step edge (Canny) | Forgotten wire, missing abstraction |
| **Roof edge** | Image processing | Discontinuity in derivative | Scope creep, gradual neglect |
| **Wombling boundary** | Spatial statistics | Womble 1951; Banerjee & Gelfand 2006 | Intentional boundary or forgotten seam |
| **Regression discontinuity** | Causal inference | RDD (Thistlethwaite & Campbell 1960) | Treatment effect at threshold |
| **Topological hole** | Computational topology | Persistent homology (Edelsbrunner; Carlsson 2009) | Stale refactor, forgotten migration |
| **Oracle gap** | Software engineering | Oracle gap (Petrova et al. ISSRE 2023) | Weak oracle, superficial tests |
| **Frustrated landscape** | Statistical physics | Spin glass (Parisi 1979; Nobel 2021) — SUBSTRATE | System trapped in metastable valley |
| **Allosteric population shift** | Biochemistry | Allostery (Monod-Wyman-Changeux 1965) | Broken ensemble coupling; orchestrator corollary |

### Fractal recurrence across scales and domains

These shapes are not domain-specific. They recur fractally:

- A **sharp cliff** at the pixel scale (Canny edge) is the same shape as a **test-coverage cliff** at the function scale, which is the same shape as a **telemetry cliff** at the subsystem scale, which is the same shape as a **governance boundary** at the organization scale.
- A **topological hole** in a point cloud (persistent homology b₁ loop) is the same shape as a **missing-test hole** in a code graph, which is the same shape as a **coverage gap** in a sensor network.
- A **frustrated landscape** at the spin-glass scale (Parisi valleys) is the same shape as a **deferred-task trap** at the hook-wiring scale, which is the same shape as a **stalemate** in a dependency graph.
- An **allosteric population shift** at the protein scale (T→R ensemble redistribution) is the same shape as a **skill-cascade strategy shift** at the orchestrator scale, which is the same shape as a **tool-router ensemble shift** at the MCP scale. The orchestrator is a polymorphic catalyst; the task is the effector; the population shift is the orchestration.

## Reason Taxonomy

Seven reason classes adapted from Rubin 1976 + spin glass + allostery:

| Reason class | Analog | Meaning | Action |
|---|---|---|---|
| **Intentional boundary** | MCAR (ignorable) | Gradient is by design. | Document. Verify intentional. |
| **Explainable gap** | MAR (ignorable with covariates) | Desert exists because of observed covariate. | Fix covariate or document scope. |
| **Forgotten wire** | MNAR (non-ignorable) | Missing artifact should depend on itself. | Fix. Missing wire is the cause. |
| **Stale refactor** | MNAR (drift) | Refactor moved populated side, left desert behind. | Fix. Update desert side. |
| **Scope creep** | MNAR (missing abstraction) | Populated side has structure desert side lacks. | Fix. Extract abstraction. |
| **Metastable trap** | Spin glass (non-ergodic) | Desert is a valley system relaxed into; cannot leave without crossing barrier. | Inject energy at barrier. Reshape landscape. |
| **Broken allosteric coupling** | Allostery (ensemble redistribution) | Desert is a conformation/strategy that should be populated but isn't. Effector isn't shifting ensemble. | Restore coupling at distal site. For orchestrators: wire task to strategy ensemble. |

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `gradient-prior.j2` | `KnowAct` | Build prior model of expected field (sibling/convention/principle). Delegates to pragmatic-cybernetics. |
| `gradient-map.j2` | `KnowAct` | Map actual field with prior's granularity. Delegates to graph-audit for topology. |
| `gradient-detect.j2` | `KnowAct` | Detect gradients and classify shape, scale, domain, fractal recurrence. References gradient-shapes.yaml. |
| `gradient-hypothesize.j2` | `KnowAct` | Generate reason hypotheses via Rubin + spin glass + allostery taxonomy. Delegates to falsifiability and metacognition. |
| `gradient-report.j2` | `KnowAct` | Compile structured gradient report with priority ranking, lessons_learned, pattern_signatures. |
| `gradient-convergence-check.j2` | `KnowAct` | Compute composite convergence metric (stabilization + coverage) with next_prior_focus for loop closure. |
| `gradient-shapes.yaml` | `RenderAct` | Reference: eight gradient shape ontology with fractal recurrence, reason classes, and priority ordering. |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- Energy caps: prior (4096), map (4096), detect (6144), hypothesize (6144), report (4096), convergence-check (2048).
- Gas cap: 100,000 per invocation. Maximum 3 iterations.
- The prior must be explicit before gradient detection. Implicit priors produce implicit gradients — unrepeatable and unfalsifiable.
- The fractal recurrence check is mandatory, not optional. A gradient classified without checking whether its shape recurs at other scales or domains is incomplete.
- The metastable trap reason class must be considered for every gradient. The default assumption that "the artifact is missing" is an ergodic-system assumption. Non-ergodic systems trap.
- The broken allosteric coupling reason class must be considered for every gradient in an orchestrator context. The default assumption that "the strategy is missing" misses the point — the coupling is broken, not the strategy.
- Do not collapse the eight ontologies into one. Each shape implies a different reason family and a different intervention. A sharp cliff is fixed by adding the wire; a frustrated landscape is fixed by reshaping the landscape; an allosteric population shift is fixed by restoring the distal coupling, not by adding the missing conformation. Conflating them produces wrong interventions.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
