---
name: metacognition
core: true
description: "Metacognitive Improvement Kata: ground the current and task-specific target conditions, predict and test one calibration, measure the gap, and keep measured evidence separate from human closure and optional forecast resolution."
audience: curator
---

# Metacognition

Master self-reflection skill following the Toyota Improvement Kata (Rother 2010).
The skill runs actual PDCA: grasp the current condition, establish a target
condition, make a prediction, run an experiment, measure the gap, and compare observation with expectation. Evaluate convergence via the hypotenuse
of object-space (Dublin Core) and process-space (PKO) gaps; an optional forecast requires a separate prospective declaration and frozen resolver.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Report measured gaps as worker evidence; the host administers assessment and any optional forecast. Do not infer human acceptance from a smaller gap.

## Reference models

- Improvement Kata — Rother, *Toyota Kata* (2010); `onto_anchor` → derived `improvement_kata`.
- PDCA — Shewhart (1939), Deming; `onto_anchor` → derived `pdca_cycle`.
- Brier score — Brier (1950); `onto_anchor` → derived `brier_score`.
- Metacognition — David Dunning (Kruger & Dunning 1999; Dunning 2011); `onto_anchor` → derived `metacognition` (operator ruling 2026-09-25). Dunning's double curse is why this skill never grades its own prediction: the knowledge needed to close the gap is the knowledge needed to see it, so evidence checks and human acceptance stay separate from worker self-assessment. This is a methodological adaptation, not proof of calibration. Sources: the published literature (Kruger & Dunning 1999; Dunning 2011). Historical provenance, now unverifiable: the capabilities-reasoner replica corpus ("The Trouble of Not Knowing What You Don't Know" — the skill was deleted 2026-09-24, its corpus runs 2026-09-29) and the curator's `dunning-video-catalog` (not reachable through the curator recall path as of 2026-09-30).

## D/P labelling

Grasp-current, establish-target, predict, and the experiment's calibration
choice are P — the practitioner's judgment, critiqued by the measured gap and
external evidence and human judgment. The inquiry experiment's machinery is P likewise — the
engine's thought generation, the delegation dispatch, the skill-match loop,
and the dead-letter handling are judgment, critiqued by the delegates'
returned analyses and the engine's `hypothesis_verified` gate. The gap
arithmetic is D: the guarded normalization, the hypotenuse, the reduction,
and the convergence checks run in `lisp_eval`
over cited receipts — an uncited model status leaves the after-gap
unmeasured, not zero. The goal-tool calls are D recordings of specification and worker progress; their semantic content is P. Native human closure and blinded assessment are distinct. The host supplies native-approved bounded file snapshots frozen before inference; fetched evidence IDs may support or contradict a criterion, while unavailable evidence is unknown without a wasted model call. File bytes do not authenticate producer claims.

## When to Use

- When an agent needs to reflect on its own metacognitive state and identify what it knows and doesn't know.
- When an agent needs to establish a measurable target condition for its meta-knowledge.
- When an agent needs to make a calibrated prediction about which intervention will improve its understanding.
- When an agent needs to run an experiment (apply a calibration) and measure whether it closed the gap.

## When NOT to Use

- Coaching a human or agent through the kata — use `kata-improvement`'s Coaching Kata section (the five coach questions); this skill is the practitioner's own reflection loop.
- Executing a specific improvement — `kata-improvement` owns the act; this skill measures the gap and compares expectation with observation.
- Forecast-calibration tracking in the prediction-market domain — `superforecasting` owns that loop (its Market-prior calibration check and stage 6).

## Instructions

### Initial and target condition

- **Initial condition:** the user's specific problem, observed artifacts and procedure receipts, grounded claims versus assumptions, and any available prior eligible forecast history. An absent source is unknown, not zero gap.
- **Target condition:** task-specific observable evidence that the user's problem was addressed *and* the required object/process items are complete and grounded. The session can report a measured gap closure; human closure remains separate; ordinary work requires no probability score.

### Step 0 — Read prior calibration (execute)

1. Read current-schema `metacognition:` goals from the array returned by `kanban_goal_list` as evidence history: original criteria, worker progress, human closure and assessment limitations separately. Only prospectively eligible resolved forecasts may contribute to a declared comparable cohort. Do not reconstruct forecasts or human confirmations from obsolete data. Forecast-market cohorts are a different reference class.
2. The resolved predictions feed the grasp-current step. "No resolved metacognition goals" is an expected empty state: record calibration as unavailable and continue. Other failures remain visible, and the Kata cycle proceeds without calibration context.

### meta-grasp-current (Kata Step 1: Grasp Current Condition)

1. Measure the agent's actual metacognitive state — don't assume, measure.
2. Identify obstacles (typed, severity-rated), surface assumptions, count grounded claims, and enumerate `options` (compute the option count deterministically via `lisp_eval` `(length options)`).
3. Produce current_artifacts (Dublin Core) and current_procedure (PKO) with a checkable source/receipt for every claimed `exists`, `grounded`, or `complete` state. Uncited or ambiguous states are `unmeasured`, not a zero contribution to the gap.
4. On refinement cycles, compare to the previous grasp and note what changed.

### meta-establish-target (Kata Step 2: Establish Target Condition)

1. Declare the user's task-specific observable outcome and how it will be checked, then the metacognitive state needed to reach it. Completing the generic Kata steps is not itself the user's outcome.
2. Produce target_artifacts and target_procedure that the gap computation measures toward. If the user task has no observable success evidence, ask for it before predicting an improvement.
3. The target should be one step beyond the current knowledge threshold — challenging but achievable. Require `target_outcome.success_evidence` and `falsifier` tied to the user's task. Do not proceed from a generic list of Kata artifacts/steps.
4. Reconcile every current `exists`/`grounded`/`complete` status with its cited receipt, then call `lisp_eval` over the four structured collections themselves — never model-count the required/missing rows. The form requires unique nonblank ids/names, at least one required artifact and step, valid booleans on target rows, current measured statuses in the closed vocabularies, and nonblank `evidence_ref` for every current state counted as present/grounded/complete:
   - form: `(begin (define blank (lambda (v) (or (is_null v) (and (stringp v) (= (length v) 0))))) (define get (lambda (k row) (assoc k row))) (define ids (lambda (rows key) (if (is_null rows) (list) (cons (get key (car rows)) (ids (cdr rows) key))))) (define unique (lambda (xs) (if (is_null xs) true (and (stringp (car xs)) (> (length (car xs)) 0) (not (member (car xs) (cdr xs))) (unique (cdr xs)))))) (define find (lambda (id rows key) (if (is_null rows) nil (if (and (stringp (get key (car rows))) (string= id (get key (car rows)))) (car rows) (find id (cdr rows) key))))) (define boolp (lambda (x) (or (eq x true) (eq x false)))) (define target-art-ok (lambda (rows) (if (is_null rows) true (and (boolp (get "required" (car rows))) (boolp (get "must_be_grounded" (car rows))) (target-art-ok (cdr rows)))))) (define target-step-ok (lambda (rows) (if (is_null rows) true (and (boolp (get "required" (car rows))) (target-step-ok (cdr rows)))))) (define current-art-ok (lambda (rows) (if (is_null rows) true (let ((r (car rows))) (and (or (eq (get "exists" r) true) (eq (get "exists" r) false) (string= (get "exists" r) "unmeasured")) (or (eq (get "grounded" r) true) (eq (get "grounded" r) false) (string= (get "grounded" r) "unmeasured")) (if (or (eq (get "exists" r) true) (eq (get "grounded" r) true)) (not (blank (get "evidence_ref" r))) true) (current-art-ok (cdr rows))))))) (define current-step-ok (lambda (rows) (if (is_null rows) true (let ((r (car rows)) (s (get "status" (car rows)))) (and (stringp s) (or (string= s "complete") (string= s "in_progress") (string= s "not_started") (string= s "unmeasured")) (if (string= s "complete") (not (blank (get "evidence_ref" r))) true) (current-step-ok (cdr rows))))))) (define count-required-art (lambda (rows) (if (is_null rows) 0 (+ (if (eq (get "required" (car rows)) true) 1 0) (count-required-art (cdr rows)))))) (define count-required-step (lambda (rows) (if (is_null rows) 0 (+ (if (eq (get "required" (car rows)) true) 1 0) (count-required-step (cdr rows)))))) (define count-missing-art (lambda (rows current) (if (is_null rows) 0 (let ((r (car rows))) (if (eq (get "required" r) true) (let ((c (find (get "id" r) current "id"))) (+ (if (and (not (is_null c)) (eq (get "exists" c) true) (if (eq (get "must_be_grounded" r) true) (eq (get "grounded" c) true) true)) 0 1) (count-missing-art (cdr rows) current))) (count-missing-art (cdr rows) current)))))) (define count-incomplete (lambda (rows current) (if (is_null rows) 0 (let ((r (car rows))) (if (eq (get "required" r) true) (let ((c (find (get "name" r) current "name"))) (+ (if (and (not (is_null c)) (string= (get "status" c) "complete")) 0 1) (count-incomplete (cdr rows) current))) (count-incomplete (cdr rows) current)))))) (let ((ra (count-required-art target_artifacts)) (rs (count-required-step target_procedure)) (ma (count-missing-art target_artifacts current_artifacts)) (is (count-incomplete target_procedure current_procedure))) (if (and (unique (ids target_artifacts "id")) (unique (ids current_artifacts "id")) (unique (ids target_procedure "name")) (unique (ids current_procedure "name")) (target-art-ok target_artifacts) (target-step-ok target_procedure) (current-art-ok current_artifacts) (current-step-ok current_procedure) (> ra 0) (> rs 0)) (list (/ ma ra) (/ is rs) ra ma rs is) (quote unmeasured))))`
   - env: `{ "current_artifacts": <Step 1 rows [{id, exists, grounded, evidence_ref}]>, "current_procedure": <Step 1 rows [{name, status, evidence_ref}]>, "target_artifacts": <Step 2 rows [{id, required, must_be_grounded}]>, "target_procedure": <Step 2 rows [{name, required, completion_condition}]> }`
   - returns `[object_gap, process_gap, required_artifacts, missing_artifacts, required_steps, incomplete_steps]` or `unmeasured`. On `unmeasured`, ask for evidence and do not call `meta-predict`. Otherwise compute `gap_before` with `(sqrt (+ (* og og) (* pg pg)))`, using the first two outputs. If zero, check the task outcome before declaring the target met; never invent a prediction about closing an already zero gap.

### meta-predict (Kata Step 3: Make a Prediction)

1. Predict which calibration will close the gap and by how much.
2. Carry a confidence in [0,1] — how sure is the agent that this prediction is correct?
3. Record the falsifiable expectation and threshold before the experiment. If a goal is needed, create the confirmed target with observable criteria and actual source; omit `forecast` for routine work. An optional probability is eligible only with the complete typed prospective declaration in the canonical pact; do not fabricate one to score this exercise.

### meta-experiment (Kata Step 4: Experiment / Do)

1. Apply the predicted calibration — Falstaffian perspective rotation (the five shapes — predicate hollow, subject expansion, object inversion, direction reversal, value redefinition — defined in `metacognition/falstaffian-rotation`; apply the rotation inline to the assessment), ellipsis analysis (render `metacognition/ellipsis-analysis` with `text` = the context text, `expectations`, `biases`, `assumptions`, and `domain`), strategy adjustment, or an **inquiry experiment** for a problem that needs branching, revision and deep-dive delegation (below).
2. Re-measure the current condition after the experiment (the experiment changed the system).
3. Produce new current_artifacts and current_procedure for the gap computation. Carry Step 2's measured `gap_before` and Step 3's predeclared reduction threshold unchanged; never recompute the baseline from post-experiment data.

### Inquiry experiment (branching chain of thought with delegation)

Use when the gap is in understanding a problem that needs several dependent reasoning steps, not in calibrating a known one. It runs inside Step 4; the Kata steps around it are unchanged.

1. Render `metacognition/inquiry-engine` with `problem`, `domain`, `constraints`, `max_thoughts` (≤ 20; 12 is the default) and the prior cycle's `prior_chain`, `prior_delegation_results` and `prior_skill_match_results`. It generates, branches, revises and verifies thoughts and emits `delegation_requests` and `skill_match_queries` (total thoughts used is computed deterministically as `(length thought_chain)`).
2. For each `delegation_requests` entry, route deterministically via `lisp_eval` gate **`MC-1`** to the canonical skill (`hypothesis-framer`, `mcda`, `diagnose`, or `falsifiability`):
   - form: `(if (and (<= (length delegation_requests) 3) (or (string= requested_skill "hypothesis-framer") (string= requested_skill "mcda") (string= requested_skill "diagnose") (string= requested_skill "falsifiability"))) requested_skill "route_to_skill_discovery")`
   - env: `{ "delegation_requests": <the delegation_requests array>, "requested_skill": <entry.skill> }`
   Invoke the matched canonical skill (`hypothesis-framer`, `mcda`, `diagnose`, or `falsifiability`) via the `skill` tool (not a local shadow template). When `MC-1` returns `"route_to_skill_discovery"` or for `skill_match_queries`, render `skill-discovery/skill-discovery-route` with `task_description`, `task_context`, the actual installed `skill_catalog`, `max_recommendations: 3`, and its required `epistemic_state: {}` when no independently supported confidence/type was supplied. An empty object disables the boost; never invent 0.5 or an uncertainty type. Do this for at most three follow-up skills.
3. Feed the delegation and skill-match results back into the next engine render. The engine's `final_answer` and `hypothesis_verified` become the post-experiment current condition.

### Convergence (Steps 5-9: Check + Act)

Status classification (`exists`/`grounded`/`complete`) is P — model-proposed and accepted only with a cited receipt. Counts and all arithmetic over the accepted structured rows are D (`lisp_eval`); no model-produced scalar count enters the gap.

1. (P→D handoff) Recheck the cited artifact/step receipts against the *same* target after the experiment and use Step 2's guarded `lisp_eval` normalization. An uncited model status leaves the after-gap `unmeasured`, not zero.
2. (D) Compute the hypotenuse with `lisp_eval`, form `(sqrt (+ (* og og) (* pg pg)))`, env `{ "og": <object gap>, "pg": <process gap> }`. Never estimate the gap in prose.
3. (D) The reduction is `(- gap_before gap_after)` via `lisp_eval`; `gap_before` is the Step 3 value, unchanged.
4. Judge the recorded goal with the measured gap (`kanban_goal_judge`). Keep the measured worker result separate from native human closure and independent assessment; an unmeasured result is not zero error.
5. Check the predeclared gap-reduction threshold and the user's task-specific success evidence separately. The threshold check is `(>= (- gap_before gap_after) (* gap_before expected_gap_reduction))` via `lisp_eval`, env `{ "gap_before": <the Step 3 value>, "gap_after": <the measured after-gap>, "expected_gap_reduction": <the predeclared fraction in (0, 1] — 0 is forbidden: a zero reduction passes the threshold vacuously on any non-regression }`. A zero after-gap with observed target-outcome evidence meets the local target; with missing evidence report `unmeasured`. Two nearly unchanged measured iterations while the target still fails indicate a plateau, not success. Do not convert a session gap or completed task into a human outcome; optional forecasts resolve only under their frozen source rule. Re-enter grasp-current only on a named obstacle and new observation; stop after three cycles with remaining gaps visible.

## Registry Templates

| Template | Purpose |
|----------|---------|
- `meta-grasp-current.j2` | Measure the agent's actual metacognitive state right now. Identify obstacles, surface assumptions, enumerate grounded claims and options. Produces stable-id `current_artifacts` and stable-name `current_procedure` row arrays for deterministic gap computation. |
| `meta-establish-target.j2` | Declare the target metacognitive state — what sufficient meta-knowledge looks like for this goal. Produces target_artifacts and target_procedure for gap computation. |
| `meta-predict.j2` | Predict a task-specific calibration and gap reduction, with an optional declared forecast; closure and assessment remain separate. |
| `meta-experiment.j2` | Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment. Re-measure the current condition after the experiment. Produces new current_artifacts and current_procedure. |
| `inquiry-engine.j2` | Inquiry experiment: branching, revising chain of thought that emits deep-dive delegation requests (routed via `MC-1` to canonical skills) and skill-match queries. |
| `ellipsis-analysis.j2` | Apply Bloom's five-step method to detect gaps in context, classify them as ellipsis (deliberate) or leak (unintentional), and find what is not inferno (Calvino: the exceptional, supported element worth preserving). Used by the experiment step for ellipsis perspective. |

To render a template, call the `render_template` tool with the template ref (e.g., `metacognition/meta-grasp-current`) and a context object with the required variables. Context keys are **top-level** (not nested under `variables`). Pass the prior step's result as the named condition (`current_condition` = Step 1 result, `target_condition` = Step 2 result, `prediction` = Step 3 result), not as `current_grasp` or `predicted_calibration`; `gap_before` is the measured Step 2 value (>0), carried unchanged.

Copyable `render_template` context blocks (variable names from each template's `[inference] contract`): prior evidence is empty/null unless source-backed records exist; never fill examples with invented human outcomes. Eligible forecast history must carry its declaration, resolver, timing, cohort and loss convention, not just confidence/outcome pairs.

`metacognition/meta-grasp-current`:
```json
{
  "goal": "<the user's goal>",
  "session_history": "<prior session turns cited as evidence>",
  "current_state": { "<regulation counter or bot status>": "<observed value>" },
  "prior_outcomes": [],
  "prev_grasp": { "<previous iteration's measured field>": "<measured value>" },
  "prior_calibration": null
}
```

`metacognition/meta-establish-target`:
```json
{
  "goal": "<the user's goal>",
  "current_condition": { "<Step 1 grasp result field>": "<measured value>" }
}
```

`metacognition/meta-predict`:
```json
{
  "goal": "<the user's goal>",
  "current_condition": { "<Step 1 grasp result field>": "<measured value>" },
  "target_condition": { "<Step 2 target field>": "<declared value>" },
  "gap_before": 0.71,
  "prev_prediction": { "<previous prediction field>": "<predicted value>" }
}
```

`metacognition/meta-experiment`:
```json
{
  "goal": "<the user's task-specific goal>",
  "current_condition": { "current_artifacts": [{ "id": "artifact-1", "exists": true, "grounded": true, "evidence_ref": "<receipt>" }], "current_procedure": [{ "name": "step-1", "status": "complete", "evidence_ref": "<receipt>" }] },
  "prediction": { "<Step 3 prediction field>": "<predicted value>" },
  "perspectives": ["falstaffian", "ellipsis"],
  "context_text": "<about 2000 characters of context for the ellipsis analysis>",
  "prev_experiment": { "<previous experiment field>": "<result value>" }
}
```

`metacognition/ellipsis-analysis`:
```json
{
  "text": "<the context text to analyze>",
  "expectations": ["<baseline expectation to check>"],
  "biases": ["<known cognitive bias>"],
  "assumptions": ["<assumption to question>"],
  "domain": "<the domain>"
}
```

`metacognition/inquiry-engine`:
```json
{
  "problem": "<the problem to reason through>",
  "domain": "<the domain>",
  "constraints": ["<constraint>"],
  "max_thoughts": 12,
  "prior_chain": [{ "thoughtNumber": 1, "thought": "<prior thought>" }],
  "prior_delegation_results": { "<delegated skill>": { "invoked": true, "result": "<insight>" } },
  "prior_skill_match_results": { "<matched skill>": "<insight>" }
}
```

A `render_template` call renders the prompt; it does not execute the inference step or produce that step's result. Use the measured result of each step when supplying the next step's condition.

## Constraints

- The gap arithmetic is D only over source-reconciled rows — an uncited model status leaves the after-gap `unmeasured`, never zero. Feed the structured normalization form the four declared row arrays verbatim; it derives required/missing counts and rejects duplicate/blank identities, invalid statuses, zero required targets and measured-complete rows without evidence. The hypotenuse and reduction run in `lisp_eval`; the convergence threshold check is `(>= (- gap_before gap_after) (* gap_before expected_gap_reduction))`.
- Never author a human outcome. Request native human closure separately; optional forecast scoring is deterministic under its frozen resolver, not a self-grade.
- `gap_before` is carried unchanged from Step 2 through the experiment and the convergence check — never recomputed from post-experiment data.
- A zero after-gap with observed target-outcome evidence meets the local target; with missing evidence report `unmeasured`, not success.
- Two nearly unchanged measured iterations while the target still fails indicate a plateau, not success — report it honestly.
- Re-enter grasp-current only on a named obstacle and a new observation; stop after three cycles with remaining gaps visible.
- The inquiry experiment's `epistemic_state` must be an empty object `{}` when no independently supported confidence/type was supplied — never invent 0.5 or an uncertainty type.
- Forecast-market calibration (`scenario_calibration`) is a different reference class — it is not this skill's calibration and must not be read as such.
- Evaluate convergence after each full iteration using the criteria described above.
- Execute the four Kata steps (grasp, target, predict, experiment), then evaluate the measured gap against the predeclared threshold; native human closure and eligible forecast resolution remain separate.

## Regression case

Run the structured normalization form through the live tool with: (i) two
required artifacts, one present-and-grounded and one absent, plus two required
steps, one complete and one in-progress → `[0.5, 0.5, 2, 1, 2, 1]`; (ii) the same
rows with a duplicate id, a missing evidence_ref on a measured-complete state,
or zero required target rows → `unmeasured`. Then run the hypotenuse (env `{og:
0.5, pg: 0.5}` → `0.7071067811865476`), the reduction (env `{gap_before:
0.7071067811865476, gap_after: 0.3}` → `0.4071067811865476`), and the convergence
threshold. Verify a zero after-gap with no task-outcome evidence stays
`unmeasured` at the semantic target check. All forms are executed through the
live tool at audit and use time, never anchored in code.
