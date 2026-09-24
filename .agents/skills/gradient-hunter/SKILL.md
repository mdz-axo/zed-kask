---
name: gradient-hunter
description: "Establish a task-relevant expectation before a substantive inquiry; investigate surprises or vague predictions with discriminating probes, and diagnose measured gradients in code, telemetry, or tests."
---

# Gradient Hunter

Investigate a disagreement between a grounded expectation and reality, then revise the expectation using the next observation that can distinguish explanations. When neighboring observations actually show a boundary, investigate *why* that gradient exists. A desert is meaningful relative to an expected oasis; a missing test is interesting if neighboring code has tests. A surprising observation is not automatically a gradient.

## Substrate ontology: non-ergodicity and information storage

Grounded in Parisi's spin glass theory (Parisi 1979; Sherrington-Kirkpatrick 1975; Nobel 2021). Non-ergodic systems that store work, information, or memory have **rugged energy landscapes with metastable valleys separated by barriers**. The energy gradient is non-monotonic, so following the local gradient gets you stuck in a local minimum, not the global optimum.

A "desert" is therefore not necessarily an absence — it is often a **metastable valley** the system has relaxed into and cannot leave without crossing an energy barrier. The system stores information *by being trapped*. A gradient between two metastable states is a gradient between two memories.

The seven surface ontologies (below, in `gradient-shapes.yaml`) describe the *shape* of gradients at boundaries. The spin glass substrate explains *why there are boundaries at all*. Before classifying a gradient's shape, consider whether the desert is a valley the system is trapped in — if so, "add the missing artifact" will not work; the system will relax back. The intervention must inject enough energy to cross the barrier (a deferred task, a startup signal, a reconfiguration that reshapes the landscape itself).

## When to Use

- **At intake for a nontrivial, uncertain inquiry:** before the first substantive evidence probe, state one task-relevant expected observation, its source and what would falsify it; if no grounded expectation exists, say `unknown` and use a scoping probe. Skip this overhead for trivial mechanical tasks. Do not wait for an anomaly to start forming expectations.
- **Self-trigger during other work** when a tool result, code path, test outcome, or missing answer conflicts with the declared expectation, or when a consequential expectation is too vague to predict what an observation would show. Say what was expected and why before selecting another probe; do not wait for an explicit audit request.
- Audit a crate for missing tests, but only where neighboring code has tests (test-coverage gradient)
- Audit a subsystem for missing telemetry, but only where sibling subsystems emit spans (telemetry cliff)
- Audit a config for missing failure signals, but only where sibling configs have them (silent-failure asymmetry)
- Find `// zed-kask:` comments without paired tests, but only where other deviations have tests
- Find hooks wired inconsistently with their siblings (hook-signal gradient)
- Find span namespaces declared but never emitted, but only where sibling namespaces are emitted
- Diagnose whether an agentic orchestrator's strategy ensemble is being shifted by the task (allosteric coupling check)
- Any "dog that didn't bark" investigation where a source or field is rich in adjacent evidence but an expected measurement, subgroup, or explanation is missing; test that expected slot, not just the amount of evidence present.

Do NOT use for:
- Pure absence checking with no prior or testable expectation ("does this crate have any tests?" — that's a checklist, not gradient analysis)
- Positive-space threat hunting (use `bug-hunt`)
- Reasoning-context ellipses (use `metacognition`)
- Reproducing a known symptom (use `diagnose` — deserts are asymptomatic until you have a prior)

## When NOT to Use

- Absent features someone should simply build — the gradient is the *reason* for an absence, not a backlog; a known missing feature needs a plan, not an investigation.
- Performance profiling — use `diagnose`; a slow path is a measurement question, not an information-field question.

## Instructions

```
Plan:   Phase 1 — Prior       → Build expected-field model (sibling/convention/principle)
Do:     Phase 2 — Map         → Measure actual field with prior's granularity
Do:     Phase 3 — Detect      → Classify shape, scale, domain, fractal recurrence
Check:  Phase 4 — Hypothesize → Generate reason hypotheses (Rubin + spin glass + allostery)
Act:    Phase 5 — Report      → Prioritized gradient report with lessons + pattern signatures
Check:  Phase 6 — Converge    → gradient-map stability gate (`lisp_eval`: `(and (eq new_gradient_shapes 0) (eq top_k_stable 1))` — no new gradient shapes vs the prior map and the prioritized top-K unchanged)
Act:    Phase 7 — Loop        → If not converged, re-enter at Phase 1 with refined prior (bound: max 2 prior refinements; then emit the report with lessons_learned)
```

Feedback loop closure: convergence emits `next_prior_focus` (consumed by next iteration's Prior); Report emits `lessons_learned` and `pattern_signatures` (consumed by next iteration's Prior and Detect).

### Expectation-led inquiry when no gradient has yet been measured

Start this route at intake for a substantive uncertain inquiry by selecting a question whose possible answer matters to the active user goal, stating a falsifiable expectation **before** the first evidence probe. A routine check that meets the expectation needs no investigation; a contradiction or a consequentially vague expectation opens the next-probe loop. Use this route **instead of forcing a shape or reason class** when a single observation contradicts an expectation, or the expectation cannot make a discriminating prediction. It also supplies the next probe for the ordinary gradient route. A surprise nominates an inquiry; it does not authorize spending the user's time on it. If the operator has already dismissed this line of inquiry, apply step 3 immediately, before any more tool calls.

1. **Commit a checkable expectation.** At task intake, select one uncertainty whose resolution changes the active goal; before inspecting the target, state the expected observation or evidence slot, source (sibling, empirical comparator, convention, principle, model), scope, comparable measure, and what result would contradict it. Render `gradient-hunter/gradient-prior` when an expected field must be constructed. If the discrepancy was noticed first, label the reconstructed expectation `retrospective`; never claim a before-the-fact prediction. If no grounded expectation can be formed, record `unknown` and, only when the task relevance gate passes, select a probe to establish one, not a surprise score.
2. **Sense and compare.** Fetch direct evidence with `grep`/`read_file` or the relevant read-only tool; render `gradient-hunter/gradient-map` when mapping a field. Record observation, source and comparability. In a rich field, inventory which expected slots are actually covered: distinguish `observed`, `explicitly not measured`, `not reported in inspected scope`, and `not retrieved`. Use `metacognition/ellipsis-analysis` for source-text gaps: only evidence of deliberate omission supports `ellipsis`, only evidence of accidental loss supports `leak`; otherwise mark intent unknown. A paper's acknowledged missing variable is evidence about that study set, not proof that no one has studied it. An unobserved region is `unmeasured`, not empty. A mismatch in units, scope, measurement or authorization is a measurement problem, not yet evidence the world violates the prior.
3. **Gate relevance before inquiry.** Compare the discrepancy with the active user goal: what decision, capability assessment, or user experience could resolving it change? Is the expectation actually uncertain and potentially learnable, or merely surprising to this agent? Use explicit operator feedback as the priority signal. If the operator calls it uninteresting or no consequence for the active goal can be named, stop this branch and return to the goal; do not ask for permission to continue chasing it. If it exposes a mandatory correctness or safety risk, surface that risk once with evidence rather than silently burying it, but do not extend an unrelated investigation without authorization.
4. **Choose a question, not a spectacle.** Only after the relevance gate passes, render `gradient-hunter/expectation-inquiry` with `active_goal` from the user's stated outcome, `operator_feedback` (explicit text or null), `expectation`, `observation`, and `probe_results: []`. Do not bury the goal in an optional nested field or invent it when absent. Form at least two live explanations (including a bad prior or bad measurement), each with a differing prediction for an authorized next probe. Pick the smallest probe whose possible outcomes discriminate them; for a suspected omission, check methods, supplementary data, or a comparable source that could contain the missing slot before asserting absence. State expected outcomes **before** invoking it. Repeated unexplained noise without learnable structure is not a reason to revisit indefinitely.
5. **Execute and update.** Run the selected probe with its owner tool, carry its result and provenance into a second rendering of `expectation-inquiry` with the same `active_goal` and updated `probe_results`, and mark each explanation retained/eliminated/undetermined. Update the expected field only where supported; check one independent neighboring or held-out observation when available. If repeated comparable neighbors reveal a boundary, resume the ordinary Detect → Hypothesize → Report route. If not, call it an `expectation_mismatch` or unresolved expectation, not a spatial gradient. Do not reuse the finance-specific `expectations_gap` ontology identity for a general discrepancy.
6. **Bound and report.** At most two discriminating probes per question. Stop on operator-declared irrelevance, decisive discrimination, insufficient evidence, unavailable oracle, denied authority, or budget exhaustion. Report the original expectation (prospective or retrospective), mismatch or vagueness, competing predictions, probe result, revised expectation or explicit unknown, and the observation that would change the verdict. Improvement means fewer *independently contradicted* expectations on subsequent cases, not a higher count of surprises; without a subsequent case the benefit is `unverified`. Re-enter at step 1 only if the result changes the prior and another authorized probe remains.

## Improvement Measure

**Field**: the result of step 6's `convergence_metric`. **Threshold**: 0.25. **Max iterations**: 3.

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
| `grep + manual code analysis | Field topology extraction | Map phase — when hunting topology gradients (orphan nodes, missing edges, disconnected components) |
| `pragmatic-cybernetics` | Prior modeling | Prior phase — when no sibling or convention prior is available; models expected field via variety engineering |
| `falsifiability` | Counterfactual discrimination | Hypothesize phase — when discriminating between reason hypotheses |
| `metacognition` | Prior perspective rotation and source-scoped ellipsis analysis | Hypothesize phase — different priors surface different gradients; when an expected detail is missing amid rich data, test whether it is actually unmeasured, merely unretrieved, or of unknown intent |

### Composition Protocol

1. **Prior first** — name the source and falsifier; render `gradient-prior` for a field prior, or use the expectation-led route above for a single mismatch (mark retrospectively constructed priors).
2. **Map with matching granularity** — granularity mismatch produces false gradients.
3. **Detect with fractal check** — required for a measured gradient; do not impose a gradient shape or fractal recurrence on an isolated expectation mismatch.
4. **Hypothesize with multiple reasons** — at least 2-3 hypotheses per gradient from different reason classes. Do not collapse to the first match.
5. **Delegate when needed** — topology gradients → `grep` + manual analysis; no sibling/convention prior → `pragmatic-cybernetics`; non-obvious discrimination → `falsifiability`; prior may be wrong → `metacognition`.
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

| Template | Purpose |
|----------|---------|
| `expectation-inquiry.j2` | Require an explicit user goal, then gate a sourced expectation mismatch before selecting a discriminating next probe; on re-render reconcile actual evidence or stop on operator-declared irrelevance. No gradient shape is inferred from one observation. |
| `gradient-prior.j2` | Build a prior model of the expected field. Without a prior, you can only detect absences, not gradients. The prior comes from one of three sources in order of preference: sibling prior (a populated region structurally similar to the target), convention prior (a documented convention like a .rules trap), or principle prior (a design principle). Records source, scope, and confidence. Delegates to pragmatic-cybernetics for variety engineering when no sibling or convention prior is available. |
| `gradient-map.j2` | Measure the actual field in the target region with the same granularity as the prior. The field is whatever is being hunted: test presence, span emission, log statements, error-handling branches, paired comments, manifest entries. Delegates to grep + manual code analysis for topology extraction (call graph, dependency graph, span emission sites) when hunting topology gradients (orphan nodes, missing edges, disconnected components). |
| `gradient-detect.j2` | Compare prior to actual. Classify each gradient by its shape using the eight ontological anchors (sharp cliff, roof edge, wombling boundary, regression discontinuity, topological hole, oracle gap, frustrated landscape, allosteric population shift). Record location, shape, scale, domain, fractal recurrence (does this shape appear at other scales or in other domains?), populated side, desert side, magnitude. The fractal recurrence check is mandatory — a shape that recurs at multiple scales/domains is the system's characteristic pattern. References gradient-shapes.yaml for the shape taxonomy. |
| `gradient-hypothesize.j2` | For each gradient, generate multiple hypotheses for why it exists. Use the seven-class reason taxonomy: intentional boundary (MCAR), explainable gap (MAR), forgotten wire (MNAR), stale refactor (MNAR drift), scope creep (MNAR missing abstraction), metastable trap (spin glass non-ergodic), broken allosteric coupling (ensemble redistribution). Delegates to falsifiability for counterfactual hypothesis discrimination ("if this desert were intentional, what else would be true?") and to metacognition for prior perspective rotation (different priors surface different gradients). |
| `gradient-report.j2` | Compile detected gradients into a structured report. Each gradient entry includes location, shape, ontology anchor, scale, domain, fractal recurrence, prior, populated side, desert side, magnitude, reason hypotheses, recommended reason, and action. Prioritizes by reason class (broken allosteric coupling > metastable trap > MNAR > MAR > MCAR), then fractal recurrence, then magnitude, then populated-side criticality. Emits lessons_learned and pattern_signatures for the next iteration's prior (feedback loop closure). |
| `gradient-shapes.yaml` | Reference: eight gradient shapes drawn from seven academic domains (image processing, spatial statistics, causal inference, computational topology, software engineering, statistical physics, biochemistry/ biophysics). Each shape includes ontology, anchor concept, meaning, reason family, and fractal recurrence across scales. Includes the seven-class reason taxonomy (Rubin MCAR/MAR/MNAR + spin glass metastable trap + allostery broken coupling) and priority ordering. Substrate ontology (spin glass) explains why gradients exist at all. |

To render a template, call the `render_template` tool with the template ref (e.g., `gradient-hunter/gradient-prior`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `gradient-prior.j2`: `target_region`, `field_type`, `prior_iteration`
- `expectation-inquiry.j2`: `active_goal` (required string, the user's words), `operator_feedback` (explicit string or null), `expectation`, `observation`, `probe_results` (empty array on first pass; carry actual results on re-render)


## Constraints

- All flow templates are prompt templates with `Public` visibility. Reference documents are rendering templates.
- The prior must be explicit before gradient detection. A reconstructed prior is labeled retrospective, not scored as a pre-registered forecast.
- Do not claim a spatial gradient without measured neighbors and an explicit comparison rule; do not claim learnability from prediction error alone. Do not promote 'not reported in this source' to 'absent from the field' or infer an author's intention from silence.
- Operator-declared irrelevance stops an agent-initiated inquiry even if the discrepancy surprises the agent; user priority is not inferred from error magnitude.
- The intake expectation is a one-question checkpoint for consequential uncertainty, not an open-ended research license; when evidence agrees, return to the task rather than looking for a more entertaining anomaly.
- The fractal recurrence check is mandatory.
- Do not collapse the eight ontologies into one — each shape implies a different intervention.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
