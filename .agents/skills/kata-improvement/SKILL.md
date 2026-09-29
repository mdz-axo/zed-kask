---
name: kata-improvement
description: "4-step Improvement Kata templates for scientific capability development: Understand Direction, Grasp Current Condition, Establish Target Condition, Experiment (PDCA). Includes the Coaching Kata (the five coach questions, a separate coach role) and beginner_mode drills for foundational scientific thinking habit-building."
---

# Kata Improvement

4-step Improvement Kata templates for scientific capability development. Step 1: Understand Direction. Step 2: Grasp Current Condition. Step 3: Establish Target Condition. Step 4: Experiment (PDCA). Each step references prior outputs. The cycle closes with before/after measurement. Includes beginner_mode drills (folded from kata-starter): Five Questions, PDCA Cycle, and Observation Drill for foundational scientific thinking habit-building; agents graduate when automaticity > 0.5. The Coaching Kata (folded from `kata-coaching`, 2026-09-28) is the coach role that runs the five questions against a learner's storyboard — the same kata seen from the other chair.


## Reference model

Rother, *Toyota Kata* (2010) and the Lean Enterprise Institute lexicon — `onto_anchor` → derived `improvement_kata`, `pdca_cycle` and `coaching_kata` (operator rulings 2026-09-24). The Coaching Kata's five questions are Rother's, asked by a coach who gives procedural guidance rather than solutions; **the coach is a separate role from the learner** — one session never plays both.

**D/P labelling.** Steps 1–3 and step 4's Plan are P (the practitioner's judgment; critique: the operator, or the Coaching Kata below run by a separate coach). Step 2's measurements are D when taken from a tool or test — name it in `metrics[].method`. Step 4's Check is D: compute `(- metrics_target metric_after)` and `(- metric_after metric_before)` with `lisp_eval` over the observed values, and never from a plan.

## When to Use

- When practicing the Toyota Improvement Kata for scientific capability development.
- When articulating the strategic direction and challenge from the level above.
- When grasping the current condition by gathering facts and data to establish a baseline.
- When establishing a measurable next target condition reachable within this session's bounded experiments.
- When designing rapid PDCA experiments with testable predictions toward the target.
- When coaching a human or agent learner through their kata with the five coach questions (Coaching Kata) — the learner's storyboard is the input, the learner's own words are the output.
- When computing a convergence metric from the step-4 Check forms to evaluate PDCA cycle coherence.
- When an agent needs to build foundational scientific thinking habits through beginner_mode drills (folded from kata-starter): Five Questions, PDCA Cycle on a trivial process, or Observation Drill separating facts (IS) from interpretations (OUGHT).

## Beginner-mode drills

The three drills build the kata's foundational habits before the four-step practice. Selection: render `kata-improvement/beginner-selector` with the learner's practice history; it routes to the appropriate drill (Observation Drill for a first session or >7 days since last practice; the lowest-automaticity drill otherwise). Each drill runs one session, produces its declared output, and self-assesses automaticity on a 0–1 scale (the learner's own honest rating, not a test score). Graduate when automaticity exceeds 0.5 across two consecutive sessions on the same drill; the selector's `< 0.3` threshold routes a struggling learner back to the same drill rather than advancing.

## When NOT to Use

- Executing a specific improvement task — the kata is the practice method for developing capability, not a task executor; run the task through its own skill.
- One-shot problems with no iteration — a single experiment with no target condition to converge toward needs a plan, not a kata.

## Instructions

### improvement-step1-direction

1. Articulate the direction before measuring progress toward it.
2. Answer what the challenge is with specific, measurable statements.
3. Describe what excellent performance looks like in measurable terms.
4. Define how you will know you've improved by stating the metric and measurement plan.
5. Mark the boundary of your current knowledge threshold explicitly.
6. Respond with a JSON object containing `challenge`, `excellent_performance`, `measurement_plan`, and `knowledge_threshold`.

### improvement-step2-current

1. Go and see to gather the facts; do not assume—measure.
2. Collect real data to describe the actual performance now.
3. List every metric describing the current state with method and source.
4. Observe what patterns exist in the data.
5. Redraw the boundary between known and assumed for your knowledge threshold.
6. Record the baseline measurements you commit to measuring against as `metric_before`.
7. Respond with a JSON object containing `current_performance`, `metrics`, `patterns`, `knowledge_threshold`, and `metric_before`.

### improvement-step3-target

1. Declare a specific, measurable target condition reachable within this session's PDCA cycles (at most the skill's iteration bound), beyond your current knowledge threshold. The Lean Enterprise Institute's human practice sets the target about two weeks out; an agent practices the kata within one session (operator ruling 2026-09-24), so the horizon is counted in bounded experiments, not calendar time.
2. Identify every obstacle between current and target conditions to create an Obstacles Parking Lot.
3. Select the ONE most consequential obstacle to address first.
4. Define what you do NOT know about the focus obstacle.
5. Respond with a JSON object containing `target_condition`, `obstacles`, `focus_obstacle`, `knowledge_gap`, and `metrics_target`.

### improvement-step4-experiment

1. Design a PDCA experiment against ONE obstacle.
2. Plan your next step: make it specific, actionable, and one change at a time.
3. Plan your expectation: state your prediction and why (the theory you're testing).
4. Do: define how you will execute (tool, parameter, configuration).
5. Check: define how you will measure and what confirms or refutes your prediction.
6. Act after observing the result: compare the measured outcome to `metric_before`, the prediction, and `metrics_target` via `lisp_eval`:
   - gap-to-target form: `(- metrics_target metric_after)`
   - improvement form: `(- metric_after metric_before)`
   - env: `{ "metrics_target": <the step-3 target>, "metric_after": <the observed post-experiment value>, "metric_before": <the step-2 baseline> }`
   If no post-experiment observation is available, report `pending_check` rather than an improvement. If wrong, revise the theory and re-enter step 2; if correct but below target, select the next obstacle and re-enter step 3. Stop after three experiments or on target attainment; report the remaining gap.
7. Determine how quickly you can go and see the result.
8. Respond with a JSON object containing `obstacle`, `next_experiment`, `prediction`, `measurement_method`, `success_criterion`, `learning_commitment`, `when_to_check`, and (only after observation) `metric_after`, `prediction_result`, and `next_step`. Never fill `metric_after` from a plan.

### Coaching Kata — the five questions (coach role)

- **Learner:** named by `learner_bot` — the operator, or a local agent reached with `swarm_delegate_local`. The learner's own words come back as `learner_answer`. Reading the learner's storyboard is context for the coach's questions, not a substitute for the learner answering.
- **Initial condition:** the learner's current storyboard (step 1–4 outputs above: target, `metric_before`, obstacles, last experiment and its result).
- **Target condition:** after Q5, the learner has stated in their own words a measurable target, a data-grounded actual condition, one focus obstacle, a next step with a testable prediction, and a committed check point — each coach assessment names what the template emits: Q1 `clear`, Q2 `data-driven`, Q3 `focused`, Q4 `testable`, Q5 `tight-loop`.
- **Loop:** each question runs ASK (render without `learner_answer`) then ASSESS (render with it). A non-passing assessment asks one follow-up (max 2 per question); still failing, record the gap and move on — the gap is coaching data, not a reason to answer for the learner.
- **D/P:** the questions are fixed text (D). The learner's answers are the learner's. The coach's assessments are P, critiqued by the next session's observed result: an assessment of `testable` whose prediction could not be checked is a coaching miss to record.

1. **Q1 — target** (`kata-improvement/coaching-q1-target`): ask what the target condition is; the learner states the measurable target, experiment bound and success criteria, never the coach. If vague, ask for specific, measurable, verifiable.
2. **Q2 — actual** (`coaching-q2-actual`): ask what the actual condition is now; probe whether statements rest on measured data or assumptions; challenge interpretations stated as facts by asking what was observed.
3. **Q3 — obstacles** (`coaching-q3-obstacles`): ask what obstacles block the target and which single one is being addressed now; require the learner to justify the priority; if several are listed unprioritized, force the selection of one.
4. **Q4 — experiment** (`coaching-q4-experiment`): ask the next step and the expected result; require exactly what will be done and what is predicted; an action without a prediction is sent back for one.
5. **Q5 — learn** (`coaching-q5-learn`): ask how quickly the learner can go and see; require the check point, the metric, and what would prove the theory wrong.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `beginner-selector.j2` | Select appropriate starter drill based on practice history and automaticity. If no history, start with Observation Drill. If automaticity is low in a specific drill, target that drill. If 7+ days since last practice, restart with Observation Drill. |
| `beginner-five-questions.j2` | Five Questions Drill — exercise asking the 5 coaching questions in order on a trivial process (making toast, brewing coffee). |
| `beginner-pdca-cycle.j2` | PDCA Cycle Drill — practice Plan-Do-Check-Act on a trivial, measurable process. |
| `beginner-observation-drill.j2` | Observation Drill — practice separating observed facts (IS) from interpretations (OUGHT). |
| `coaching-q1-target.j2` | Coaching Kata Q1 — What is the Target Condition? Ground the learner in their goal. |
| `coaching-q2-actual.j2` | Coaching Kata Q2 — What is the Actual Condition now? Ground the learner in data. |
| `coaching-q3-obstacles.j2` | Coaching Kata Q3 — What obstacles? Which ONE now? Force prioritization. |
| `coaching-q4-experiment.j2` | Coaching Kata Q4 — Next step? Expected result? Drive action with a prediction. |
| `coaching-q5-learn.j2` | Coaching Kata Q5 — How quickly can we go and see? Close the feedback loop. |
| `improvement-step1-direction.j2` | Step 1 of the Improvement Kata — understand the strategic direction and challenge from the level above. |
| `improvement-step2-current.j2` | Step 2 of the Improvement Kata — grasp the current condition by gathering facts and data to establish a baseline. |
| `improvement-step3-target.j2` | Step 3 of the Improvement Kata — establish a measurable next target condition bounded in experiments, not calendar time. |
| `improvement-step4-experiment.j2` | Step 4 of the Improvement Kata — define next experiment with testable predictions toward the target. |

To render a template, call the `render_template` tool with the template ref (e.g., `kata-improvement/beginner-selector`) and a context object with the required variables.

Template context variables (from each template's [inference] contract — the 13 templates share a uniform shape):
- All templates: `context` (object), `learner_bot` (string), `previous_steps` (array|null)
- Coaching templates (q1–q5) additionally: `learner_answer` (string|null — present for the ASSESS render, absent for the ASK render)
- `improvement-step1-direction.j2` additionally: `prior_kanban` (object|null), `prior_boards` (object|null)

## Regression case

Run the step-4 Check forms through `lisp_eval` with representative values:
`{ "metrics_target": 10, "metric_after": 7, "metric_before": 5 }` →
gap-to-target `(- metrics_target metric_after)` = 3 and improvement
`(- metric_after metric_before)` = 2. Verify the gap-to-target is 0 when
the target is met (metric_after = metrics_target) and the improvement is
negative when the metric regressed (metric_after < metric_before).
Render one coaching template (e.g., `kata-improvement/coaching-q1-target`)
with a contract-conformant context — the ASK render (no `learner_answer`)
and the ASSESS render (with it) both execute. All receipts through the
live tool 2026-09-29.

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

