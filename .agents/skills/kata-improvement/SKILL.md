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

The two starter drills (`kata-improvement/beginner-observation-drill` and `kata-improvement/beginner-pdca-cycle`) build the kata's foundational habits before the four-step practice. Each learner has one drill log, `~/Documents/zk-data/skills/kata-improvement/<learner_bot>/drills.jsonl`; after every drill append one line with `terminal`:

```
jq -n -c --arg drill <template stem> --argjson automaticity <the render's automaticity_self_assess> --argjson day <today> '$ARGS.named' >> <log>
```

where `<today>` is `$(( $(date +%s) / 86400 ))`. Selection and graduation are deterministic (D) via `lisp_eval` gate **`KI-1`** over that log:
- form: `(begin (define num-of (lambda (row key) (if (numberp (assoc key row)) (assoc key row) 0))) (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (if (listp sessions) sessions (list))) (define of (lambda (drill xs) (if (is_null xs) (list) (if (string= (text-of (car xs) "drill") drill) (cons (car xs) (of drill (cdr xs))) (of drill (cdr xs)))))) (define last-of (lambda (xs) (if (is_null (cdr xs)) (car xs) (last-of (cdr xs))))) (define latest (lambda (xs) (if (is_null xs) 0 (num-of (last-of xs) "automaticity")))) (define prior (lambda (xs) (if (is_null xs) 0 (latest (reverse (cdr (reverse xs))))))) (define max-day (lambda (xs acc) (if (is_null xs) acc (max-day (cdr xs) (max acc (num-of (car xs) "day")))))) (define obs (of "beginner-observation-drill" rows)) (define pdca (of "beginner-pdca-cycle" rows)) (define history (length rows)) (define since (if (= history 0) 0 (- today (max-day rows 0)))) (define obs-auto (latest obs)) (define obs-prev (prior obs)) (define pdca-auto (latest pdca)) (define pdca-prev (prior pdca)) (define selection (cond ((or (= history 0) (>= since 7) (< obs-auto 0.3)) "beginner-observation-drill") ((< pdca-auto 0.3) "beginner-pdca-cycle") ((and (> obs-prev 0.5) (> obs-auto 0.5) (> pdca-prev 0.5) (> pdca-auto 0.5)) "graduated") ((<= obs-auto pdca-auto) "beginner-observation-drill") (t "beginner-pdca-cycle"))) (list (list "selection" selection) (list "history_count" history) (list "days_since_last" since) (list "obs_auto" obs-auto) (list "obs_auto_prev" obs-prev) (list "pdca_auto" pdca-auto) (list "pdca_auto_prev" pdca-prev)))`
- env: `{ "sessions": <the log as one JSON array: jq -s . <log>; [] for a new learner>, "today": <today, integer days> }`
- receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
  ```lisp-receipts
  {"gate": "KI-1", "case": "fresh-log", "env": {"sessions": [], "today": 100}, "expected": {"selection":"beginner-observation-drill","history_count":0,"days_since_last":0,"obs_auto":0,"obs_auto_prev":0,"pdca_auto":0,"pdca_auto_prev":0}}
  {"gate": "KI-1", "case": "first-session-no-prior", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.6,"day":99}], "today": 100}, "expected": {"selection":"beginner-pdca-cycle","history_count":1,"days_since_last":1,"obs_auto":0.6,"obs_auto_prev":0,"pdca_auto":0,"pdca_auto_prev":0}}
  {"gate": "KI-1", "case": "documented-four-session-log", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":90},{"drill":"beginner-pdca-cycle","automaticity":0.35,"day":91},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.4,"day":98}], "today": 100}, "expected": {"selection":"beginner-pdca-cycle","history_count":4,"days_since_last":2,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.4,"pdca_auto_prev":0.35}}
  {"gate": "KI-1", "case": "both-drills-strong-twice-graduates", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":95},{"drill":"beginner-pdca-cycle","automaticity":0.55,"day":96},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.6,"day":99}], "today": 100}, "expected": {"selection":"graduated","history_count":4,"days_since_last":1,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.6,"pdca_auto_prev":0.55}}
  {"gate": "KI-1", "case": "exactly-0.5-prior-is-not-above-0.5", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":95},{"drill":"beginner-pdca-cycle","automaticity":0.5,"day":96},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.6,"day":99}], "today": 100}, "expected": {"selection":"beginner-observation-drill","history_count":4,"days_since_last":1,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.6,"pdca_auto_prev":0.5}}
  {"gate": "KI-1", "case": "lower-latest-goes-to-pdca", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":95},{"drill":"beginner-pdca-cycle","automaticity":0.5,"day":96},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.55,"day":99}], "today": 100}, "expected": {"selection":"beginner-pdca-cycle","history_count":4,"days_since_last":1,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.55,"pdca_auto_prev":0.5}}
  {"gate": "KI-1", "case": "seven-days-stale-not-six", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":95},{"drill":"beginner-pdca-cycle","automaticity":0.55,"day":96},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.6,"day":99}], "today": 106}, "expected": {"selection":"beginner-observation-drill","history_count":4,"days_since_last":7,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.6,"pdca_auto_prev":0.55}}
  {"gate": "KI-1", "case": "stale-days-derived-from-the-log", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.55,"day":95},{"drill":"beginner-pdca-cycle","automaticity":0.55,"day":96},{"drill":"beginner-observation-drill","automaticity":0.6,"day":98},{"drill":"beginner-pdca-cycle","automaticity":0.6,"day":99}], "today": 118}, "expected": {"selection":"beginner-observation-drill","history_count":4,"days_since_last":19,"obs_auto":0.6,"obs_auto_prev":0.55,"pdca_auto":0.6,"pdca_auto_prev":0.55}}
  {"gate": "KI-1", "case": "weak-observation-overrides-strong-pdca", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.2,"day":99},{"drill":"beginner-pdca-cycle","automaticity":0.9,"day":99}], "today": 100}, "expected": {"selection":"beginner-observation-drill","history_count":2,"days_since_last":1,"obs_auto":0.2,"obs_auto_prev":0,"pdca_auto":0.9,"pdca_auto_prev":0}}
  {"gate": "KI-1", "case": "at-0.3-floor-not-under", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.3,"day":99},{"drill":"beginner-pdca-cycle","automaticity":0.25,"day":99}], "today": 100}, "expected": {"selection":"beginner-pdca-cycle","history_count":2,"days_since_last":1,"obs_auto":0.3,"obs_auto_prev":0,"pdca_auto":0.25,"pdca_auto_prev":0}}
  {"gate": "KI-1", "case": "under-floor-observation-wins", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.25,"day":99},{"drill":"beginner-pdca-cycle","automaticity":0.2,"day":99}], "today": 100}, "expected": {"selection":"beginner-observation-drill","history_count":2,"days_since_last":1,"obs_auto":0.25,"obs_auto_prev":0,"pdca_auto":0.2,"pdca_auto_prev":0}}
  {"gate": "KI-1", "case": "one-strong-session-each-cannot-graduate", "env": {"sessions": [{"drill":"beginner-observation-drill","automaticity":0.9,"day":99},{"drill":"beginner-pdca-cycle","automaticity":0.9,"day":99}], "today": 100}, "expected": {"selection":"beginner-observation-drill","history_count":2,"days_since_last":1,"obs_auto":0.9,"obs_auto_prev":0,"pdca_auto":0.9,"pdca_auto_prev":0}}
  ```
- returns `selection` — the drill to render, or `graduated` — with the figures it derived from the log (`history_count`, `days_since_last`, latest and prior automaticity per drill; a drill never run reads 0). The rule: no history, seven or more days since the last drill, or observation automaticity under 0.3 → observation drill; PDCA automaticity under 0.3 → PDCA drill; both drills above 0.5 in each of their last two sessions → `graduated`; otherwise the drill with the lower latest automaticity.

Render the selected drill with `{ "context": <object>, "learner_bot": <string>, "previous_steps": <array|null> }`. Each drill runs one session, produces its declared output, and self-assesses automaticity on a 0–1 scale (P — the learner's own honest rating, not a test score); the log line carries that rating as returned. Graduation is `KI-1`'s `graduated`, never the learner's claim.

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
6. Act after observing the result. Check is `lisp_eval` gate **`KI-3`** over the step-2 and step-3 objects as returned, the focus metric's key in them, the observed value, and the session's experiment log (`<run_dir>/experiments.jsonl`, one line per checked experiment: `jq -n -c --arg obstacle <obstacle> --argjson metric_after <value> --arg prediction_result <result> '$ARGS.named' >> <run_dir>/experiments.jsonl`):
   - form: `(begin (define max-experiments 3) (define num-at (lambda (obj key) (if (numberp (assoc key obj)) (assoc key obj) (if (and (listp (assoc key obj)) (numberp (assoc "value" (assoc key obj)))) (assoc "value" (assoc key obj)) nil)))) (define before (num-at metric_before focus_metric)) (define target (num-at metrics_target focus_metric)) (define done (if (listp experiments) (length experiments) 0)) (define observed (numberp metric_after)) (define problems (append (if (numberp before) (list) (list "focus_metric_missing_in_metric_before")) (if (numberp target) (list) (list "focus_metric_missing_in_metrics_target")) (if (or observed (is_null metric_after)) (list) (list "metric_after_not_a_number")))) (define gap (if (and observed (numberp target)) (- target metric_after) nil)) (define improvement (if (and observed (numberp before)) (- metric_after before) nil)) (define attained (and observed (numberp target) (numberp before) (if (>= target before) (>= metric_after target) (<= metric_after target)))) (define verdict (cond ((not (is_null problems)) "invalid") ((not observed) "pending_check") (attained "target_attained") ((>= (+ done 1) max-experiments) "stop_bound") (t "continue"))) (list (list "verdict" verdict) (list "problems" problems) (list "focus_metric" focus_metric) (list "metric_before" before) (list "metrics_target" target) (list "metric_after" metric_after) (list "gap_to_target" gap) (list "improvement" improvement) (list "experiment" (+ done 1))))`
   - env: `{ "metric_before": <step-2 metric_before object as returned>, "metrics_target": <step-3 metrics_target object as returned>, "focus_metric": <the key of the focus metric in both objects>, "metric_after": <the observed number, or null when not yet observed>, "experiments": <jq -s . <run_dir>/experiments.jsonl, [] before the first check> }`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate": "KI-3", "case": "baseline-5-target-10-observed-7", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": 7, "experiments": []}, "expected": {"verdict":"continue","problems":[],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":7,"gap_to_target":3,"improvement":2,"experiment":1}}
     {"gate": "KI-3", "case": "observed-10-attains-target", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": 10, "experiments": []}, "expected": {"verdict":"target_attained","problems":[],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":10,"gap_to_target":0,"improvement":5,"experiment":1}}
     {"gate": "KI-3", "case": "third-experiment-stops", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": 4, "experiments": [{}, {}]}, "expected": {"verdict":"stop_bound","problems":[],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":4,"gap_to_target":6,"improvement":-1,"experiment":3}}
     {"gate": "KI-3", "case": "second-experiment-continues", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": 4, "experiments": [{}]}, "expected": {"verdict":"continue","problems":[],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":4,"gap_to_target":6,"improvement":-1,"experiment":2}}
     {"gate": "KI-3", "case": "unobserved-is-pending", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": null, "experiments": []}, "expected": {"verdict":"pending_check","problems":[],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":null,"gap_to_target":null,"improvement":null,"experiment":1}}
     {"gate": "KI-3", "case": "lower-is-better-at-or-below-target-attained", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "latency_ms", "metric_after": 190, "experiments": []}, "expected": {"verdict":"target_attained","problems":[],"focus_metric":"latency_ms","metric_before":300,"metrics_target":200,"metric_after":190,"gap_to_target":10,"improvement":-110,"experiment":1}}
     {"gate": "KI-3", "case": "lower-is-better-above-target-continues", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "latency_ms", "metric_after": 250, "experiments": []}, "expected": {"verdict":"continue","problems":[],"focus_metric":"latency_ms","metric_before":300,"metrics_target":200,"metric_after":250,"gap_to_target":-50,"improvement":-50,"experiment":1}}
     {"gate": "KI-3", "case": "focus-metric-absent-from-both-names-each-missing", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "throughput", "metric_after": 1, "experiments": []}, "expected": {"verdict":"invalid","problems":["focus_metric_missing_in_metric_before","focus_metric_missing_in_metrics_target"],"focus_metric":"throughput","metric_before":null,"metrics_target":null,"metric_after":1,"gap_to_target":null,"improvement":null,"experiment":1}}
     {"gate": "KI-3", "case": "textual-observation-is-invalid", "env": {"metric_before": {"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}}, "metrics_target": {"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}}, "focus_metric": "pass_rate", "metric_after": "7", "experiments": []}, "expected": {"verdict":"invalid","problems":["metric_after_not_a_number"],"focus_metric":"pass_rate","metric_before":5,"metrics_target":10,"metric_after":"7","gap_to_target":null,"improvement":null,"experiment":1}}
     ```
   - the form reads the focus metric from each object (a bare number, or `{"value": <number>, ...}`), and returns `verdict`, `gap_to_target` (`metrics_target − metric_after`), `improvement` (`metric_after − metric_before`), and `experiment` (this check's 1-based number). `pending_check`: no observation — report it, leave `metric_after` null. `target_attained`: the target is met in the direction the baseline-to-target pair implies (a lower target counts as attained at or below it). `stop_bound`: this is the third experiment — report the remaining gap. `continue`: if the prediction was wrong, revise the theory and re-enter step 2; if right but short of target, select the next obstacle and re-enter step 3. `invalid` names the problem (`focus_metric_missing_in_metric_before`, `focus_metric_missing_in_metrics_target`, `metric_after_not_a_number`) — fix the input, nothing is logged. Append the experiment line only after a verdict other than `pending_check` or `invalid`.
7. Determine how quickly you can go and see the result.
8. Respond with a JSON object containing `obstacle`, `next_experiment`, `prediction`, `measurement_method`, `success_criterion`, `learning_commitment`, `when_to_check`, `skill_match_query` (a capability-gap query for skill-discovery when the experiment surfaces one), and (only after observation) `metric_after`, `prediction_result`, and `next_step`. Never fill `metric_after` from a plan.

### Coaching Kata — the five questions (coach role)

- **Learner:** named by `learner_bot` — the operator, or a delegated agent reached via `kanban_task_spawn`. The learner's own words come back as `learner_answer`. Reading the learner's storyboard is context for the coach's questions, not a substitute for the learner answering.
- **Initial condition:** the learner's current storyboard (step 1–4 outputs above: target, `metric_before`, obstacles, last experiment and its result).
- **Target condition:** after Q5, the learner has stated in their own words a measurable target, a data-grounded actual condition, one focus obstacle, a next step with a testable prediction, and a committed check point — each coach assessment names what the template emits: Q1 `clear`, Q2 `data-driven`, Q3 `focused`, Q4 `testable`, Q5 `tight-loop`.
- **The five questions** are fixed text emitted directly (D), each with its ASSESS template and passing assessment:
  1. `"What is your target condition?"` — `kata-improvement/coaching-q1-target`, `clear` (vs `vague`): a measurable target, experiment bound and success criteria.
  2. `"What is the actual condition now?"` — `kata-improvement/coaching-q2-actual`, `data-driven` (vs `assumption-based`).
  3. `"What obstacles do you see preventing you from reaching the target? Which ONE are you addressing now?"` — `kata-improvement/coaching-q3-obstacles`, `focused` (vs `scattered`).
  4. `"What is your next step? What do you expect?"` — `kata-improvement/coaching-q4-experiment`, `testable` (vs `untestable`).
  5. `"How quickly can we go and see what we learned?"` — `kata-improvement/coaching-q5-learn`, `tight-loop` (vs `open-loop`).
- **Loop, driven by `KI-2` (D):** keep `assessments`, the list of every ASSESS render's result so far as `{ "question": <1..5>, "coach_assessment": <the render's coach_assessment> }` in render order. Run `KI-2` before each coach turn:
  - form: `(begin (define passes (list "clear" "data-driven" "focused" "testable" "tight-loop")) (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define rows (if (listp assessments) assessments (list))) (define q-of (lambda (row) (if (numberp (assoc "question" row)) (assoc "question" row) 0))) (define of (lambda (q xs) (if (is_null xs) (list) (if (= (q-of (car xs)) q) (cons (car xs) (of q (cdr xs))) (of q (cdr xs)))))) (define passed (lambda (q xs) (if (is_null xs) (not t) (if (string= (text-of (car xs) "coach_assessment") (nth (- q 1) passes)) t (passed q (cdr xs)))))) (define malformed (lambda (xs) (if (is_null xs) (list) (if (and (>= (q-of (car xs)) 1) (<= (q-of (car xs)) 5) (> (length (text-of (car xs) "coach_assessment")) 0)) (malformed (cdr xs)) (cons (car xs) (malformed (cdr xs))))))) (define followups (lambda (q) (max 0 (- (length (of q rows)) 1)))) (define status (lambda (q) (cond ((is_null (of q rows)) "unasked") ((passed q (of q rows)) "passed") ((>= (followups q) 2) "gap_recorded") (t "follow_up")))) (define statuses (list (status 1) (status 2) (status 3) (status 4) (status 5))) (define first-open (lambda (q) (if (> q 5) 0 (if (member (status q) (list "passed" "gap_recorded")) (first-open (+ q 1)) q)))) (define open (first-open 1)) (define gaps (lambda (q) (if (> q 5) (list) (if (string= (status q) "gap_recorded") (cons q (gaps (+ q 1))) (gaps (+ q 1)))))) (define verdict (cond ((not (is_null (malformed rows))) "invalid") ((= open 0) (if (is_null (gaps 1)) "complete" "complete_with_gaps")) ((string= (status open) "unasked") "ask") (t "follow_up"))) (list (list "verdict" verdict) (list "question" open) (list "statuses" statuses) (list "gaps" (gaps 1)) (list "followups" (list (followups 1) (followups 2) (followups 3) (followups 4) (followups 5))) (list "malformed" (length (malformed rows)))))`
  - env: `{ "assessments": <the list, [] before Q1> }`
  - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
    ```lisp-receipts
    {"gate": "KI-2", "case": "empty-log-asks-q1", "env": {"assessments": []}, "expected": {"verdict":"ask","question":1,"statuses":["unasked","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "vague-q1-follows-up", "env": {"assessments": [{"question":1,"coach_assessment":"vague"}]}, "expected": {"verdict":"follow_up","question":1,"statuses":["follow_up","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "second-vague-still-follows-up-counting", "env": {"assessments": [{"question":1,"coach_assessment":"vague"},{"question":1,"coach_assessment":"vague"}]}, "expected": {"verdict":"follow_up","question":1,"statuses":["follow_up","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[1,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "third-vague-records-gap-asks-q2", "env": {"assessments": [{"question":1,"coach_assessment":"vague"},{"question":1,"coach_assessment":"vague"},{"question":1,"coach_assessment":"vague"}]}, "expected": {"verdict":"ask","question":2,"statuses":["gap_recorded","unasked","unasked","unasked","unasked"],"gaps":[1],"followups":[2,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "q1-q4-passed-asks-q5", "env": {"assessments": [{"question":1,"coach_assessment":"clear"},{"question":2,"coach_assessment":"data-driven"},{"question":3,"coach_assessment":"scattered"},{"question":3,"coach_assessment":"focused"},{"question":4,"coach_assessment":"testable"}]}, "expected": {"verdict":"ask","question":5,"statuses":["passed","passed","passed","passed","unasked"],"gaps":[],"followups":[0,0,1,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "all-five-passed-complete", "env": {"assessments": [{"question":1,"coach_assessment":"clear"},{"question":2,"coach_assessment":"data-driven"},{"question":3,"coach_assessment":"scattered"},{"question":3,"coach_assessment":"focused"},{"question":4,"coach_assessment":"testable"},{"question":5,"coach_assessment":"tight-loop"}]}, "expected": {"verdict":"complete","question":0,"statuses":["passed","passed","passed","passed","passed"],"gaps":[],"followups":[0,0,1,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "q2-gap-completes-with-gaps", "env": {"assessments": [{"question":1,"coach_assessment":"clear"},{"question":2,"coach_assessment":"assumption-based"},{"question":2,"coach_assessment":"assumption-based"},{"question":2,"coach_assessment":"assumption-based"},{"question":3,"coach_assessment":"focused"},{"question":4,"coach_assessment":"testable"},{"question":5,"coach_assessment":"tight-loop"}]}, "expected": {"verdict":"complete_with_gaps","question":0,"statuses":["passed","gap_recorded","passed","passed","passed"],"gaps":[2],"followups":[0,2,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "passing-word-on-the-wrong-question-is-not-a-pass", "env": {"assessments": [{"question":1,"coach_assessment":"clear"},{"question":2,"coach_assessment":"clear"}]}, "expected": {"verdict":"follow_up","question":2,"statuses":["passed","follow_up","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "mis-cased-word-is-not-a-pass", "env": {"assessments": [{"question":1,"coach_assessment":"Clear"}]}, "expected": {"verdict":"follow_up","question":1,"statuses":["follow_up","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":0}}
    {"gate": "KI-2", "case": "question-6-is-invalid", "env": {"assessments": [{"question":6,"coach_assessment":"clear"}]}, "expected": {"verdict":"invalid","question":1,"statuses":["unasked","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":1}}
    {"gate": "KI-2", "case": "empty-assessment-is-invalid", "env": {"assessments": [{"question":1,"coach_assessment":""}]}, "expected": {"verdict":"invalid","question":1,"statuses":["follow_up","unasked","unasked","unasked","unasked"],"gaps":[],"followups":[0,0,0,0,0],"malformed":1}}
    ```
  - `ask` with `question` n: emit question n's fixed text, wait for `learner_answer`, render question n's template with `{ "context": <object>, "learner_bot": <string>, "previous_steps": <array|null>, "learner_answer": <non-empty string> }`, append its `coach_assessment` to the list. `follow_up` with `question` n: the last assessment of n did not pass and n has had fewer than two follow-ups — ask one follow-up on n (the template's probe), render again, append. A third non-passing assessment of n is `gap_recorded` in `statuses` and the loop moves on — the gap is coaching data, not a reason to answer for the learner. `complete` or `complete_with_gaps` (`gaps` names the questions) ends the kata; `followups` reports the per-question count. `invalid` means an entry is outside 1..5 or has an empty assessment.
  The learner's answers are the learner's. The coach's assessments are P, critiqued by the next session's observed result: an assessment of `testable` whose prediction could not be checked is a coaching miss to record.

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

`bash kask/scripts/check-skill-lisp-forms.sh` evaluates the three authored gates through the `lisp-receipts` blocks beside each form in this file (the receipts carry the cases; the SKILL.md owns them, per the 2026-09-30 ruling): `KI-1` over an empty log (observation drill), a single observation session (PDCA drill, prior automaticity 0), the four-session log whose latest/prior values are 0.6/0.55 and 0.4/0.35 (PDCA drill), both drills above 0.5 twice (graduated), and a stale log nineteen days old (observation drill, days derived from the log); `KI-2` over no assessments (ask 1), a vague Q1 (follow-up), three vague Q1s (gap recorded, ask 2), a run with one Q3 follow-up through Q5 (complete, followups [0,0,1,0,0]), a run with a Q2 gap (complete_with_gaps [2]), a mis-cased assessment (follow-up, not a pass) and a question 6 (invalid); `KI-3` over baseline 5 / target 10 / observed 7 (continue, gap 3, improvement 2), observed 10 (target_attained, gap 0), observed 4 on the third experiment (stop_bound, improvement −1), no observation (pending_check), a lower-is-better metric carried as `{"value": …}` (target_attained), a focus metric absent from both objects and a textual observation (invalid naming each). `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib kata_improvement_` renders `coaching-q1-target` with a non-empty `learner_answer` and checks the ASSESS vocabulary the gate compares against.

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
  question, counted by `KI-2` from the rendered assessments); still
  failing, record the gap and move on — the gap is coaching data, not a
  reason to answer for the learner.
- Drill history, follow-up counts and experiment counts come from the
  logs and the gates; type none of them into an env.
- Step 2's measurements are D only when taken from a named tool or
  test (`metrics[].method`); assumed numbers are P and must be marked
  as assumptions.
- The five coach questions are fixed text (D) — do not rephrase,
  reorder, or add questions.

