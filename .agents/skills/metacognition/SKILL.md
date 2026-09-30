---
name: metacognition
core: true
description: "Metacognitive Improvement Kata: ground the current and task-specific target conditions, predict and test one calibration, measure the gap, and leave Brier scoring to the operator-resolved goal."
---

# Metacognition

Master self-reflection skill following the Toyota Improvement Kata (Rother 2010).
The skill runs actual PDCA: grasp the current condition, establish a target
condition, make a prediction, run an experiment, measure the gap, and score
the prediction. Evaluate convergence via the hypotenuse
of object-space (Dublin Core) and process-space (PKO) gaps; the Brier score
arrives only after the operator resolves the recorded prediction.

## Reference models

- Improvement Kata — Rother, *Toyota Kata* (2010); `onto_anchor` → derived `improvement_kata`.
- PDCA — Shewhart (1939), Deming; `onto_anchor` → derived `pdca_cycle`.
- Brier score — Brier (1950); `onto_anchor` → derived `brier_score`.
- Metacognition — David Dunning (Kruger & Dunning 1999; Dunning 2011); `onto_anchor` → derived `metacognition` (operator ruling 2026-09-25). Dunning's double curse is why this skill never grades its own prediction: the knowledge needed to close the gap is the knowledge needed to see it, so the check comes from outside the self-assessment — the operator's `kanban_goal_score` and its Brier score. Sources: the published literature (Kruger & Dunning 1999; Dunning 2011). Historical provenance, now unverifiable: the capabilities-reasoner replica corpus ("The Trouble of Not Knowing What You Don't Know" — the skill was deleted 2026-09-24, its corpus runs 2026-09-29) and the curator's `dunning-video-catalog` (not reachable through the curator recall path as of 2026-09-30).

## D/P labelling

Grasp-current, establish-target, predict, and the experiment's calibration
choice are P — the practitioner's judgment, critiqued by the measured gap and
the operator's score. The inquiry experiment's machinery is P likewise — the
engine's thought generation, the delegation dispatch, the skill-match loop,
and the dead-letter handling are judgment, critiqued by the delegates'
returned analyses and the engine's `hypothesis_verified` gate. The gap
arithmetic is D: the guarded normalization, the hypotenuse, the reduction,
and the convergence checks run in `lisp_eval`
over cited receipts — an uncited model status leaves the after-gap
unmeasured, not zero. The goal-tool calls are D recordings —
`kanban_goal_list` (Step 0's prior-calibration read), `kanban_goal_create`
(the prediction record), `kanban_goal_judge` (the verdict record); the
prediction and verdict CONTENT they carry is P. The
Brier score is the operator's alone — this skill never scores its own
prediction (the Dunning double curse: the knowledge needed to close the gap
is the knowledge needed to see it).

## When to Use

- When an agent needs to reflect on its own metacognitive state and identify what it knows and doesn't know.
- When an agent needs to establish a measurable target condition for its meta-knowledge.
- When an agent needs to make a calibrated prediction about which intervention will improve its understanding.
- When an agent needs to run an experiment (apply a calibration) and measure whether it closed the gap.

## When NOT to Use

- Coaching a human or agent through the kata — use `kata-improvement`'s Coaching Kata section (the five coach questions); this skill is the practitioner's own reflection loop.
- Executing a specific improvement — `kata-improvement` owns the act; this skill measures the gap and scores the prediction.
- Forecast-calibration tracking in the prediction-market domain — `superforecasting` owns that loop (its Market-prior calibration check and stage 6).

## Instructions

### Initial and target condition

- **Initial condition:** the user's specific problem, observed artifacts and procedure receipts, grounded claims versus assumptions, and any available prior operator-resolved calibration. An absent source is unknown, not zero gap.
- **Target condition:** task-specific observable evidence that the user's problem was addressed *and* the required object/process items are complete and grounded. The session can report a measured gap closure; confidence calibration remains pending until the operator scores the goal.

### Step 0 — Read prior calibration (execute)

1. Read this skill's own prior calibration with `kanban_goal_list`: resolved goals whose `goal_text` begins `metacognition:` carry the intake prediction and the operator's scored outcome (Brier-scored by `kanban_goal_score`). Forecast-market calibration (`scenario_calibration`) is a different reference class and is not this skill's calibration.
2. The resolved predictions feed the grasp-current step. "No resolved metacognition goals" is an expected empty state: record calibration as unavailable and continue. Other failures remain visible, and the Kata cycle proceeds without calibration context.

### meta-grasp-current (Kata Step 1: Grasp Current Condition)

1. Measure the agent's actual metacognitive state — don't assume, measure.
2. Identify obstacles (typed, severity-rated), surface assumptions, count grounded claims.
3. Produce current_artifacts (Dublin Core) and current_procedure (PKO) with a checkable source/receipt for every claimed `exists`, `grounded`, or `complete` state. Uncited or ambiguous states are `unmeasured`, not a zero contribution to the gap.
4. On refinement cycles, compare to the previous grasp and note what changed.

### meta-establish-target (Kata Step 2: Establish Target Condition)

1. Declare the user's task-specific observable outcome and how it will be checked, then the metacognitive state needed to reach it. Completing the generic Kata steps is not itself the user's outcome.
2. Produce target_artifacts and target_procedure that the gap computation measures toward. If the user task has no observable success evidence, ask for it before predicting an improvement.
3. The target should be one step beyond the current knowledge threshold — challenging but achievable. Require `target_outcome.success_evidence` and `falsifier` tied to the user's task. Do not proceed from a generic list of Kata artifacts/steps.
4. Reconcile every current `exists`/`grounded`/`complete` status with its cited receipt, then count required and missing target artifacts/steps. Call `lisp_eval` with `(if (and (> required_artifacts 0) (> required_steps 0) (<= 0 missing_artifacts) (<= missing_artifacts required_artifacts) (<= 0 incomplete_steps) (<= incomplete_steps required_steps)) (list (/ missing_artifacts required_artifacts) (/ incomplete_steps required_steps)) (quote unmeasured))`. On `unmeasured`, ask for evidence and do not call `meta-predict`. Otherwise compute `gap_before` with `(sqrt (+ (* og og) (* pg pg)))`, using those two normalized outputs. If zero, check the task outcome before declaring the target met; never invent a prediction about closing an already zero gap.

### meta-predict (Kata Step 3: Make a Prediction)

1. Predict which calibration will close the gap and by how much.
2. Carry a confidence in [0,1] — how sure is the agent that this prediction is correct?
3. Record the prediction before the experiment: `kanban_goal_create` with `goal_text` in the user's task terms (prefixed `metacognition:` for recall), observable criteria for the predeclared gap reduction and the task-specific `target_outcome` evidence, and confidence as `prediction`. The operator scores it later (`kanban_goal_score`); this session never scores its own prediction.

### meta-experiment (Kata Step 4: Experiment / Do)

1. Apply the predicted calibration — Falstaffian perspective rotation (the five shapes — predicate hollow, subject expansion, object inversion, direction reversal, value redefinition — defined in `company-research/falstaffian-competitive-rotation`; apply the rotation inline to the assessment), ellipsis analysis (render `metacognition/ellipsis-analysis` with `text` = the context text, `expectations`, `biases`, `assumptions`, and `domain`), strategy adjustment, or an **inquiry experiment** for a problem that needs branching, revision and deep-dive delegation (below).
2. Re-measure the current condition after the experiment (the experiment changed the system).
3. Produce new current_artifacts and current_procedure for the gap computation. Carry Step 2's measured `gap_before` and Step 3's predeclared reduction threshold unchanged; never recompute the baseline from post-experiment data.

### Inquiry experiment (branching chain of thought with delegation)

Use when the gap is in understanding a problem that needs several dependent reasoning steps, not in calibrating a known one. It runs inside Step 4; the Kata steps around it are unchanged.

1. Render `metacognition/inquiry-engine` with `problem`, `domain`, `constraints`, `max_thoughts` (≤ 20; 12 is the default) and the prior cycle's `prior_chain`, `prior_delegation_results` and `prior_skill_match_results`. It generates, branches, revises and verifies thoughts and emits `delegation_requests` and `skill_match_queries`.
2. For each `delegation_requests` entry, render the matching delegation template: `metacognition/inquiry-delegate-hypothesis-framer` (question framing, FINER + PICO), `metacognition/inquiry-delegate-mcda` (choice among alternatives), `metacognition/inquiry-delegate-diagnose` (a bug or regression), or `metacognition/inquiry-delegate-falsifiability` (a counterfactual or an untestable claim). Render the matching delegate with the FULL `delegation_requests` array — each delegate filters to its own skill's entries and returns an array under `result` when multiple match. For `skill_match_queries`, render `skill-discovery/skill-discovery-route` with `task_description`, `task_context`, the actual installed `skill_catalog`, `max_recommendations: 3`, and its required `epistemic_state: {}` when no independently supported confidence/type was supplied. An empty object disables the boost; never invent 0.5 or an uncertainty type. Do this for at most three follow-up skills.
3. Feed the delegation and skill-match results back into the next engine render. The engine's `final_answer` and `hypothesis_verified` become the post-experiment current condition.

### Convergence (Steps 5-9: Check + Act)

The counts are probabilistic (P — model-produced, critiqued by the operator when the recorded goal is scored); the arithmetic over them is deterministic (D — `lisp_eval`).

1. (P→D handoff) Recheck the cited artifact/step receipts against the *same* target after the experiment and use Step 2's guarded `lisp_eval` normalization. An uncited model status leaves the after-gap `unmeasured`, not zero.
2. (D) Compute the hypotenuse with `lisp_eval`, form `(sqrt (+ (* og og) (* pg pg)))`, env `{ "og": <object gap>, "pg": <process gap> }`. Never estimate the gap in prose.
3. (D) The reduction is `(- gap_before gap_after)` via `lisp_eval`; `gap_before` is the Step 3 value, unchanged.
4. Judge the recorded goal with the measured gap (`kanban_goal_judge`). The Brier score arrives only when the operator scores the goal; until then calibration is pending, not zero error.
5. Check the predeclared gap-reduction threshold and the user's task-specific success evidence separately. The threshold check is `(>= (- gap_before gap_after) (* gap_before expected_gap_reduction))` via `lisp_eval`, env `{ "gap_before": <the Step 3 value>, "gap_after": <the measured after-gap>, "expected_gap_reduction": <the predeclared fraction in (0, 1] — 0 is forbidden: a zero reduction passes the threshold vacuously on any non-regression }`. A zero after-gap with observed target-outcome evidence meets the local target; with missing evidence report `unmeasured`. Two nearly unchanged measured iterations while the target still fails indicate a plateau, not success. Operator-resolved Brier calibration remains pending until `kanban_goal_score`; do not convert a session gap into a scored outcome. Re-enter grasp-current only on a named obstacle and new observation; stop after three cycles with remaining gaps visible.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `meta-grasp-current.j2` | Measure the agent's actual metacognitive state right now. Identify obstacles, surface assumptions, count grounded claims, enumerate options. Produces current_artifacts and current_procedure for gap computation. |
| `meta-establish-target.j2` | Declare the target metacognitive state — what sufficient meta-knowledge looks like for this goal. Produces target_artifacts and target_procedure for gap computation. |
| `meta-predict.j2` | Predict a task-specific calibration and gap reduction, with confidence in [0,1]; Brier calibration waits for the operator's goal resolution. |
| `meta-experiment.j2` | Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment. Re-measure the current condition after the experiment. Produces new current_artifacts and current_procedure. |
| `inquiry-engine.j2` | Inquiry experiment: branching, revising chain of thought that emits deep-dive delegation requests and skill-match queries. |
| `inquiry-delegate-hypothesis-framer.j2` | Delegation: frame a research question and testable hypothesis (FINER + PICO). |
| `inquiry-delegate-mcda.j2` | Delegation: weigh and rank alternatives with sensitivity. |
| `inquiry-delegate-diagnose.j2` | Delegation: structured root-cause diagnosis of a bug or regression. |
| `inquiry-delegate-falsifiability.j2` | Delegation: eliminative inference for a counterfactual or an untestable claim. |
| `ellipsis-analysis.j2` | Apply Bloom's five-step method to detect gaps in context, classify them as ellipsis (deliberate) or leak (unintentional), and find what is not inferno (Calvino: the exceptional, supported element worth preserving). Used by the experiment step for ellipsis perspective. |

To render a template, call `render_template` with the template ref and these **top-level** context keys (not nested under `variables`). Pass the prior step's result as the named condition, not as `current_grasp` or `predicted_calibration`:

| Template ref | Required context keys | Optional context keys |
|---|---|---|
| `metacognition/meta-grasp-current` | `goal` | `session_history`, `current_state`, `prior_outcomes`, `prev_grasp`, `prior_calibration` |
| `metacognition/meta-establish-target` | `goal`, `current_condition` (Step 1 result) | — |
| `metacognition/meta-predict` | `goal`, `current_condition` (Step 1 result), `target_condition` (Step 2 result), measured `gap_before` (>0) | `prev_prediction` |
| `metacognition/meta-experiment` | `goal`, `current_condition` (Step 1 result), `prediction` (Step 3 result) | `perspectives`, `context_text`, `prev_experiment` |
| `metacognition/ellipsis-analysis` | `text`, `expectations`, `biases`, `assumptions`, `domain` | — |
| `metacognition/inquiry-engine` | `problem`, `domain`, `constraints`, `max_thoughts` | `prior_chain`, `prior_delegation_results`, `prior_skill_match_results` |
| `metacognition/inquiry-delegate-*` | `delegation_requests` | — |

A `render_template` call renders the prompt; it does not execute the inference step or produce that step's result. Use the measured result of each step when supplying the next step's condition.

## Constraints

- The gap arithmetic is D only over cited receipts — an uncited model status leaves the after-gap `unmeasured`, never zero. Feed the normalization form ONLY counts whose exists/grounded/complete statuses each carry a cited receipt; the form guards degenerate counts but cannot detect an uncited status — the citation gate is upstream of the form. The guarded normalization, the hypotenuse, and the reduction run in `lisp_eval`; the convergence threshold check is `(>= (- gap_before gap_after) (* gap_before expected_gap_reduction))`.
- This skill never scores its own prediction (the Dunning double curse: the knowledge needed to close the gap is the knowledge needed to see it). The Brier score arrives only from the operator's `kanban_goal_score`.
- `gap_before` is carried unchanged from Step 2 through the experiment and the convergence check — never recomputed from post-experiment data.
- A zero after-gap with observed target-outcome evidence meets the local target; with missing evidence report `unmeasured`, not success.
- Two nearly unchanged measured iterations while the target still fails indicate a plateau, not success — report it honestly.
- Re-enter grasp-current only on a named obstacle and a new observation; stop after three cycles with remaining gaps visible.
- The inquiry experiment's `epistemic_state` must be an empty object `{}` when no independently supported confidence/type was supplied — never invent 0.5 or an uncertainty type.
- Forecast-market calibration (`scenario_calibration`) is a different reference class — it is not this skill's calibration and must not be read as such.
- Evaluate convergence after each full iteration using the criteria described above.
- Execute the four Kata steps (grasp, target, predict, experiment), then evaluate the measured gap against the predeclared threshold; Brier calibration remains pending until the operator scores the goal.

## Regression case

Run the four `lisp_eval` forms through the live tool with representative
values: the guarded normalization (env `{required_artifacts: 4, required_steps: 3,
missing_artifacts: 1, incomplete_steps: 2}` → `[0.25, 0.6666666666666666]`
— the artifact gap and the process gap); the hypotenuse (env `{og: 0.25,
pg: 0.6666666666666666}` → `0.7120003121097942`); the reduction (env
`{gap_before: 0.7120003121097942, gap_after: 0.3}` → `0.4120003121097942`;
the gap_before chains from the hypotenuse receipt — the pre-repair case
claimed 0.7123124314512559, which is the ROUNDED-pg (0.667) result, not the
stated env's); and the convergence threshold (env `{gap_before:
0.7120003121097942, gap_after: 0.3, expected_gap_reduction: 0.5}` → true:
0.412 ≥ 0.356). Verify the normalization returns
`unmeasured` when required_artifacts or required_steps is 0 (the guard fires
before the division — without it `(/ 1 0)` errors). Verify the hypotenuse
is 0 when both gaps are 0 (target met). Verify the reduction is negative
when gap_after > gap_before (a regression — the threshold check fails it,
which is the correct interpretation). All four forms durably pinned by
`test_metacognition_skill_md_pins_forms`; receipts executed through the
live tool 2026-09-30.
