---
name: kata-improvement
description: "4-step Improvement Kata templates for scientific capability development: Understand Direction, Grasp Current Condition, Establish Target Condition, Experiment (PDCA). Includes the Coaching Kata (the five coach questions, a separate coach role) and beginner_mode drills for foundational scientific thinking habit-building."
audience: platform
---

# Kata Improvement

4-step Improvement Kata templates for scientific capability development. Step 1: Understand Direction. Step 2: Grasp Current Condition. Step 3: Establish Target Condition. Step 4: Experiment (PDCA). Each step references prior outputs. The cycle closes with before/after measurement. Includes beginner_mode drills (folded from kata-starter): Observation Drill and PDCA Cycle for foundational scientific thinking habit-building; agents graduate when automaticity exceeds 0.5 across two consecutive sessions on both drills. The Coaching Kata (folded from kata-coaching, 2026-09-28) is the coach role that runs the five questions against a learner's storyboard — the same kata seen from the other chair.


## Reference model

Rother, *Toyota Kata* (2010) and the Lean Enterprise Institute lexicon — `onto_anchor` → derived `improvement_kata`, `pdca_cycle` and `coaching_kata` (operator rulings 2026-09-24). The Coaching Kata's five questions are Rother's, asked by a coach who gives procedural guidance rather than solutions; **the coach is a separate role from the learner** — one session never plays both.

**D/P labelling.** Steps 1–3 and step 4's Plan are P (the practitioner's judgment; critique: the operator, or the Coaching Kata below run by a separate coach). Step 2's measurements are D when taken from a tool or test — name it in `metrics[].method`. Step 4's Check is D: compute `(- metrics_target metric_after)` and `(- metric_after metric_before)` with `lisp_eval` over the observed values, and never from a plan. The beginner drills' `automaticity_self_assess` is P — the learner's honest self-report, never tool-verified; the selector's routing and the graduation rule consume it as reported. Step 4's `prediction_result` (supported | refuted | pending_check) is P — the practitioner's verdict over the D inputs (the Check forms' outputs and the observed values), critiqued by the next session's observed result; it is never inferred from a plan.

## When to Use

- When practicing the Toyota Improvement Kata for scientific capability development.
- When articulating the strategic direction and challenge from the level above.
- When grasping the current condition by gathering facts and data to establish a baseline.
- When establishing a measurable next target condition reachable within this session's bounded experiments.
- When designing rapid PDCA experiments with testable predictions toward the target.
- When coaching a human or agent learner through their kata with the five coach questions (Coaching Kata) — the learner's storyboard is the input, the learner's own words are the output.
- When computing a convergence metric from the step-4 Check forms to evaluate PDCA cycle coherence.
- When an agent needs to build foundational scientific thinking habits through beginner_mode drills (folded from kata-starter): Observation Drill separating facts (IS) from interpretations (OUGHT), or PDCA Cycle on a trivial process.

## Beginner-mode drills

The two surviving starter drills (`kata-improvement/beginner-observation-drill` and `kata-improvement/beginner-pdca-cycle`) build the kata's foundational habits before the four-step practice. Selection and graduation are deterministic (D) via `lisp_eval` gate **`KI-1`**:
- form: `(cond ((or (= history_count 0) (>= days_since_last 7) (< obs_auto 0.3)) "beginner-observation-drill") ((< pdca_auto 0.3) "beginner-pdca-cycle") ((and (> obs_auto_prev 0.5) (> obs_auto 0.5) (> pdca_auto_prev 0.5) (> pdca_auto 0.5)) "graduated") ((<= obs_auto pdca_auto) "beginner-observation-drill") (t "beginner-pdca-cycle"))`
- env: `{ "history_count": <prior drill sessions>, "days_since_last": <days since last drill>, "obs_auto": <latest observation-drill automaticity>, "obs_auto_prev": <prior observation-drill automaticity>, "pdca_auto": <latest pdca-cycle automaticity>, "pdca_auto_prev": <prior pdca-cycle automaticity> }`

Render the selected drill (`kata-improvement/beginner-observation-drill` or `kata-improvement/beginner-pdca-cycle`) with `{ "context": <object>, "learner_bot": <string>, "previous_steps": <array|null> }`. Each drill runs one session, produces its declared output, and self-assesses automaticity on a 0–1 scale (P — the learner's own honest rating, not a test score). Graduate when automaticity exceeds 0.5 across two consecutive sessions on both drills (`KI-1` returns `"graduated"`).

## When NOT to Use

- Executing a specific improvement task — the kata is the practice method for developing capability, not a task executor; run the task through its own skill.
- One-shot problems with no iteration — a single experiment with no target condition to converge toward needs a plan, not a kata.

## Instructions

### improvement-step1-direction

Render `kata-improvement/improvement-step1-direction` with `context`, `learner_bot`, `previous_steps`, `prior_kanban`, and `prior_boards`:
1. Articulate the direction before measuring progress toward it.
2. Answer what the challenge is with specific, measurable statements.
3. Describe what excellent performance looks like in measurable terms.
4. Define how you will know you've improved by stating the metric and measurement plan.
5. Mark the boundary of your current knowledge threshold explicitly.
6. Respond with a JSON object containing `challenge`, `excellent_performance`, `measurement_plan`, and `knowledge_threshold`.

### improvement-step2-current

Render `kata-improvement/improvement-step2-current` with `context`, `learner_bot`, and `previous_steps`:
1. Go and see to gather the facts; do not assume—measure.
2. Collect real data to describe the actual performance now.
3. List every metric describing the current state with method and source.
4. Observe what patterns exist in the data.
5. Redraw the boundary between known and assumed for your knowledge threshold.
6. Record the baseline measurements you commit to measuring against as `metric_before`.
7. Respond with a JSON object containing `current_performance`, `metrics`, `patterns`, `knowledge_threshold`, and `metric_before`.

### improvement-step3-target

Render `kata-improvement/improvement-step3-target` with `context`, `learner_bot`, and `previous_steps`:
1. Declare a specific, measurable target condition reachable within this session's PDCA cycles (at most the skill's iteration bound), beyond your current knowledge threshold. The Lean Enterprise Institute's human practice sets the target about two weeks out; an agent practices the kata within one session (operator ruling 2026-09-24), so the horizon is counted in bounded experiments, not calendar time.
2. Identify every obstacle between current and target conditions to create an Obstacles Parking Lot.
3. Select the ONE most consequential obstacle to address first.
4. Define what you do NOT know about the focus obstacle.
5. Respond with a JSON object containing `target_condition`, `obstacles`, `focus_obstacle`, `knowledge_gap`, and `metrics_target`.

### improvement-step4-experiment

Render `kata-improvement/improvement-step4-experiment` with `context`, `learner_bot`, and `previous_steps`:
1. Design a PDCA experiment against ONE obstacle.
2. Plan your next step: make it specific, actionable, and one change at a time.
3. Plan your expectation: state your prediction and why (the theory you're testing).
4. Do: define how you will execute (tool, parameter, configuration).
5. Check: define how you will measure and what confirms or refutes your prediction.
6. Act after observing the result: compare the measured outcome to `metric_before`, the prediction, and `metrics_target` via `lisp_eval`:
   - gap-to-target form: `(- metrics_target metric_after)`
   - improvement form: `(- metric_after metric_before)`
   - env: `{ "metrics_target": <the focus metric's scalar target, extracted from the step-3 metrics_target object>, "metric_after": <the observed post-experiment value>, "metric_before": <the focus metric's scalar baseline, extracted from the step-2 metric_before object> }` — the step-2/3 outputs are OBJECTS (multi-metric storyboards); the forms compute over ONE metric, so extract the focus metric's scalar value first and name which metric you extracted.
   If no post-experiment observation is available, report `pending_check` rather than an improvement. If wrong, revise the theory and re-enter step 2; if correct but below target, select the next obstacle and re-enter step 3. Stop after three experiments or on target attainment; report the remaining gap.
7. Determine how quickly you can go and see the result.
8. Respond with a JSON object containing `obstacle`, `next_experiment`, `prediction`, `measurement_method`, `success_criterion`, `learning_commitment`, `when_to_check`, `skill_match_query` (a capability-gap query for skill-discovery when the experiment surfaces one), and (only after observation) `metric_after`, `prediction_result`, and `next_step`. Never fill `metric_after` from a plan.

### Coaching Kata — the five questions (coach role)

- **Learner:** named by `learner_bot` — the operator, or a delegated agent reached via `kanban_task_spawn`. The learner's own words come back as `learner_answer`. Reading the learner's storyboard is context for the coach's questions, not a substitute for the learner answering.
- **Initial condition:** the learner's current storyboard (step 1–4 outputs above: target, `metric_before`, obstacles, last experiment and its result).
- **Target condition:** after Q5, the learner has stated in their own words a measurable target, a data-grounded actual condition, one focus obstacle, a next step with a testable prediction, and a committed check point — each coach assessment names what the template emits: Q1 `clear`, Q2 `data-driven`, Q3 `focused`, Q4 `testable`, Q5 `tight-loop`.
- **Loop:** for each question, emit the fixed coach question directly as D text (no ASK template render), wait for the learner's answer (`learner_answer`), then run Phase ASSESS by rendering the coaching template with `{ "context": <object>, "learner_bot": <string>, "previous_steps": <array|null>, "learner_answer": <non-empty string> }`. A non-passing assessment asks one follow-up (max 2 per question); still failing, record the gap and move on — the gap is coaching data, not a reason to answer for the learner.
- **Coaching gate (`KI-2`, D):** after Q5, verify the follow-up budget (`<= 2` per question) and the five assessment criteria via `lisp_eval`:
  - form: `(and (<= q1_followups 2) (<= q2_followups 2) (<= q3_followups 2) (<= q4_followups 2) (<= q5_followups 2) (string= q1_assessment "clear") (string= q2_assessment "data-driven") (string= q3_assessment "focused") (string= q4_assessment "testable") (string= q5_assessment "tight-loop"))`
  - env: `{ "q1_followups": <0..2>, "q2_followups": <0..2>, "q3_followups": <0..2>, "q4_followups": <0..2>, "q5_followups": <0..2>, "q1_assessment": <string>, "q2_assessment": <string>, "q3_assessment": <string>, "q4_assessment": <string>, "q5_assessment": <string> }`
- **D/P:** the five questions are fixed text emitted directly (D):
  1. `"What is your target condition?"`
  2. `"What is the actual condition now?"`
  3. `"What obstacles do you see preventing you from reaching the target? Which ONE are you addressing now?"`
  4. `"What is your next step? What do you expect?"`
  5. `"How quickly can we go and see what we learned?"`
  The learner's answers are the learner's. The coach's assessments are P, critiqued by the next session's observed result: an assessment of `testable` whose prediction could not be checked is a coaching miss to record.

1. **Q1 — target** (`kata-improvement/coaching-q1-target`): emit `"What is your target condition?"` directly (D); once `learner_answer` is supplied, render `kata-improvement/coaching-q1-target` with `context`, `learner_bot`, `previous_steps`, and `learner_answer` to assess whether the learner stated the measurable target, experiment bound and success criteria (`clear` vs `vague`).
2. **Q2 — actual** (`kata-improvement/coaching-q2-actual`): emit `"What is the actual condition now?"` directly (D); once `learner_answer` is supplied, render `kata-improvement/coaching-q2-actual` with `context`, `learner_bot`, `previous_steps`, and `learner_answer` to assess whether statements rest on measured data or assumptions (`data-driven` vs `assumption-based`).
3. **Q3 — obstacles** (`kata-improvement/coaching-q3-obstacles`): emit `"What obstacles do you see preventing you from reaching the target? Which ONE are you addressing now?"` directly (D); once `learner_answer` is supplied, render `kata-improvement/coaching-q3-obstacles` with `context`, `learner_bot`, `previous_steps`, and `learner_answer` to assess whether a single priority obstacle is chosen (`focused` vs `scattered`).
4. **Q4 — experiment** (`kata-improvement/coaching-q4-experiment`): emit `"What is your next step? What do you expect?"` directly (D); once `learner_answer` is supplied, render `kata-improvement/coaching-q4-experiment` with `context`, `learner_bot`, `previous_steps`, and `learner_answer` to assess whether the action carries a testable prediction (`testable` vs `untestable`).
5. **Q5 — learn** (`kata-improvement/coaching-q5-learn`): emit `"How quickly can we go and see what we learned?"` directly (D); once `learner_answer` is supplied, render `kata-improvement/coaching-q5-learn` with `context`, `learner_bot`, `previous_steps`, and `learner_answer` to assess the committed check point, metric, and falsifier (`tight-loop` vs `open-loop`).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `beginner-pdca-cycle.j2` | PDCA Cycle Drill — practice Plan-Do-Check-Act on a trivial, measurable process. |
| `beginner-observation-drill.j2` | Observation Drill — practice separating observed facts (IS) from interpretations (OUGHT). |
| `coaching-q1-target.j2` | Coaching Kata Q1 (ASSESS) — assess the learner's Target Condition answer (`clear` vs `vague`). |
| `coaching-q2-actual.j2` | Coaching Kata Q2 (ASSESS) — assess the learner's Actual Condition answer (`data-driven` vs `assumption-based`). |
| `coaching-q3-obstacles.j2` | Coaching Kata Q3 (ASSESS) — assess the learner's Obstacles prioritization (`focused` vs `scattered`). |
| `coaching-q4-experiment.j2` | Coaching Kata Q4 (ASSESS) — assess the learner's Next Step and expected result (`testable` vs `untestable`). |
| `coaching-q5-learn.j2` | Coaching Kata Q5 (ASSESS) — assess the learner's committed check point (`tight-loop` vs `open-loop`). |
| `improvement-step1-direction.j2` | Step 1 of the Improvement Kata — understand the strategic direction and challenge from the level above. |
| `improvement-step2-current.j2` | Step 2 of the Improvement Kata — grasp the current condition by gathering facts and data to establish a baseline. |
| `improvement-step3-target.j2` | Step 3 of the Improvement Kata — establish a measurable next target condition bounded in experiments, not calendar time. |
| `improvement-step4-experiment.j2` | Step 4 of the Improvement Kata — define next experiment with testable predictions toward the target. |

To render a template, call the `render_template` tool with the template ref (e.g., `kata-improvement/beginner-observation-drill`) and a context object with the required variables.

Copyable `render_template` context blocks (field names and types from each template's `[inference] contract` — the 11 templates share a uniform base shape with two exceptions):

`kata-improvement/beginner-observation-drill`, `kata-improvement/beginner-pdca-cycle`, `kata-improvement/improvement-step2-current`, `kata-improvement/improvement-step3-target`, `kata-improvement/improvement-step4-experiment` (base shape):
```json
{
  "context": {},
  "learner_bot": "<learner name>",
  "previous_steps": []
}
```

`kata-improvement/coaching-q1-target`, `kata-improvement/coaching-q2-actual`, `kata-improvement/coaching-q3-obstacles`, `kata-improvement/coaching-q4-experiment`, `kata-improvement/coaching-q5-learn` (base shape + `learner_answer` — required for the ASSESS render; the ASK phase emits fixed D question text directly):
```json
{
  "context": {},
  "learner_bot": "<learner name>",
  "previous_steps": [],
  "learner_answer": "<the learner's answer to this coach question>"
}
```

`kata-improvement/improvement-step1-direction` (base shape + `prior_kanban` (`object|null`) + `prior_boards` (`object|null`)):
```json
{
  "context": {},
  "prior_kanban": { "tasks": [ { "title": "<prior task title>", "status": "<status>" } ] },
  "prior_boards": { "boards": [ { "name": "<board name>", "board_id": "<board id>", "column_count": 0 } ] },
  "learner_bot": "<learner name>",
  "previous_steps": []
}
```

## Regression case

Run the step-4 Check forms through `lisp_eval` with representative values:
`{ "metrics_target": 10, "metric_after": 7, "metric_before": 5 }` →
gap-to-target `(- metrics_target metric_after)` = 3 and improvement
`(- metric_after metric_before)` = 2. Verify the gap-to-target is 0 when
the target is met (metric_after = metrics_target) and the improvement is
negative when the metric regressed (metric_after < metric_before).
Run `KI-1` with `{ "history_count": 2, "days_since_last": 2, "obs_auto": 0.6, "obs_auto_prev": 0.55, "pdca_auto": 0.4, "pdca_auto_prev": 0.35 }` → `"beginner-pdca-cycle"`, and `KI-2` with `{ "q1_followups": 1, "q2_followups": 0, "q3_followups": 2, "q4_followups": 1, "q5_followups": 0, "q1_assessment": "clear", "q2_assessment": "data-driven", "q3_assessment": "focused", "q4_assessment": "testable", "q5_assessment": "tight-loop" }` → `true`.
Render `kata-improvement/coaching-q1-target` with a contract-conformant context including a non-empty `learner_answer` (`{ "context": {}, "learner_bot": "agent-a", "previous_steps": [], "learner_answer": "Reach 95% pass rate within 3 experiments, measured by the test runner." }`) to verify the ASSESS render executes.

## Constraints

- The kata is the practice method for developing capability, not a task
  executor; run improvement tasks through their own skills.
- The coach is a separate role from the learner — one session never
  plays both. The coach asks; the learner answers in their own words.
- The target horizon is counted in bounded experiments (at most three
  per session), not calendar time.
- Never fill `metric_after` from a plan — only from an observed
  post-experiment measurement. No observation → `pending_check`, never
  a fabricated improvement.
- A non-passing coaching assessment asks one follow-up (max 2 per
  question); still failing, record the gap and move on — the gap is
  coaching data, not a reason to answer for the learner.
- Step 2's measurements are D only when taken from a named tool or
  test (`metrics[].method`); assumed numbers are P and must be marked
  as assumptions.
- The five coach questions are fixed text (D) — do not rephrase,
  reorder, or add questions.

