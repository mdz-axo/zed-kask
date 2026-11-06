---
name: direct-model-edit
description: "Direct weight-space model editing: ground the baseline behavior and sampling config, route blocker vs capability gap, localize the failure to a causal direction in weight space, subtract it under an explicit surface budget, and verify with honest gain attribution. Anchored on MIT CSAIL's 'Sparks of Weight-Space Programming' (Takehi, Teoh, Zhang, Khattab; 2026-11-02 blog post, public logs), whose frontier-model edit lifted Qwen3.5-0.8B MATH-500 from 7.9% to 48.1% by removing one looping direction from 131,072 weights (~0.016%)."
---

# Direct Model Edit

A frontier model can read and write another network's raw weights directly — no gradient descent, no training loop. The anchor case: MIT CSAIL researchers gave GPT-6 Astra ("Astra") a Python kernel over Qwen3.5-0.8B's weights, practice problems from the GSM8K and MATH training sets, and a budget of forward passes — no gradients, no test-set access. The model's "math bug" was not math: 85 of its first 96 practice answers ran to the 32,768-token limit. Astra compared internals across progressing and looping answers, localized a single direction written by the MLPs of layers 15 and 19, and subtracted it from 64 output columns per layer — 131,072 weights, about 0.016% of the model. Token-limit hits fell from 93% to 33%; MATH-500 rose 7.9% → 48.1%, GSM8K 13.8% → 67.1%, IFEval 21.0% → 62.3%. The authors' own framing is this skill's core law: the edit "removed an obstacle that hid an ability" — it "could not add an ability the model lacked" (their Gemma 3 270M control, which rarely loops, stayed flat within ~1 point).

The edit itself, read through Ousterhout's lens, is a deep module: an interface of one direction and two layer indices, hiding 800 million weights of behavior behind it. Small surface, large consequence — that shape is the discipline this skill enforces.

## Initial and target condition

- **Initial condition:** a target model (open weights, small enough to hold in a Python session), its vendor model card, a reproducible harness with practice problems, a measured baseline split into answer classes (correct-complete / wrong-complete / hit-limit), the sampling configuration in force (mode, temperature, top-p, top-k, presence penalty, token limit), and a reported behavioral bug.
- **Target condition:** the DME-0 config gate and DME-1 route verdict are recorded; if blocker-routed — an applied edit whose surface passes the DME-2 budget, a verification table carrying benchmark deltas AND completion-rate deltas side by side, a DME-3 attribution label on every headline gain, a non-failing sibling control run under the same recipe, and a report carrying the full edit diff, every forward-pass budget point tried, seeds, and logs.

## When to Use

- A small open-weights model exhibits a reproducible behavioral failure — repetition loops, stuck local minima, a specific trigger it mishandles — and you want to fix the weights directly, without training.
- You are auditing weights for a hidden failure mode before or instead of benchmarking.
- You must attribute an existing "model got better" claim honestly — separating answers unblocked from failure loops out of answers the model could not produce before.

## When NOT to Use

- Capability gaps — the model finishes its answers and they are wrong. The anchor's own Gemma control: wrong arithmetic and invalid algebra have "no single component to switch off"; edits stayed within ~1 point and the authors called the gain "not established". Route capability gaps to training or distillation, not weight surgery.
- Closed-weight models with no weights interface — there is nothing to read or edit.
- A config delta the Ground step has not examined — run the DME-0 gate first; a bug that disappears at the vendor card's recommended settings is a settings bug, never a weight bug.
- Prompt or harness bugs — if the fix belongs in the harness, it does not belong in the weights.

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Ground — baseline counts | D (counts measured by the harness) | the harness's eval output |
| 1 Ground — config gate | D | `lisp_eval` DME-0 |
| 1 Ground — failure-mode reading | P | the operator; critiqued by DME-1's route arithmetic |
| 2 Route | D | `lisp_eval` DME-1 over the measured answer classes |
| 3 Localize | P | the contrast captures; critiqued by the re-appearance of the direction before failures |
| 4 Plan | P | the operator critiques the surface choice; DME-2 is the D gate on it |
| 5 Apply + re-measure | D (re-measurement) | the harness's eval output |
| 6 Attribute | D | `lisp_eval` DME-3 over before/after completion rates |
| 7 Converge | D | `lisp_eval` DME-4 over recorded open-findings counts |
| 8 Report | P | the operator; the logs are the evidence |

Hybrid steps label at sub-step granularity; `render_template` is D (deterministic render) feeding P (model consumption). A route verdict the model asserts but DME-1 does not compute from measured counts is not a route.

## Instructions

### Step 1 — Ground: baseline audit and the temperature gate

1. Measure, do not assume. Before any weight is read, run the target model on practice problems under the current harness and count the answer classes: `answers_total`, `answers_correct_complete`, `answers_wrong_complete`, `answers_hit_limit` (answers that ran to the token limit without finishing).
2. Read the vendor model card. Record its recommended sampling configuration — default mode (thinking / non-thinking), temperature, top-p, top-k, presence penalty, output token limit — and every warning about failure modes.
3. Call `render_template` to render the baseline audit prompt:

```
template: direct-model-edit/baseline
variables:
  model_name: <target model name>
  model_card_notes: <the vendor card's mode and sampling warnings, verbatim>
  run_config: { mode, temperature, top_p, top_k, presence_penalty, token_limit } as in force
  baseline_counts: { answers_total, answers_correct_complete, answers_wrong_complete, answers_hit_limit }
  benchmark_name: <the practice benchmark family>
```

4. Following the template's output schema, produce the baseline observation: classify the failed answers (non-terminating vs wrong-but-complete) and diff every config field against the card. The anchor's own trap: it ran in thinking mode — the exact condition the Qwen card warns loops — at temperature 0.6 with presence penalty 1.5.
5. Call `lisp_eval` with the config gate (DME-0):
   - form: `(cond ((and (string= run_mode card_default_mode) (= run_temp card_temp)) (quote config-matches-proceed)) (t (quote config-differs-rerun-at-card-config)))`
   - env: `{ "run_mode": <mode in force>, "card_default_mode": <card default mode>, "run_temp": <temperature in force>, "card_temp": <card recommended temperature> }`
   - `config-differs-rerun-at-card-config` → re-run the baseline at the card's config and re-gate before Step 2. A bug that disappears at the vendor's recommended settings was never a weight bug.

### Step 2 — Route: blocker or capability gap

Call `lisp_eval` with the route gate (DME-1) over the measured counts — it derives the shares itself, never hand-assert them:
- form: `(cond ((= answers_total 0) (quote empty-baseline)) (t (let ((nonfinish_share (/ answers_hit_limit answers_total)) (wrong_complete_share (/ answers_wrong_complete answers_total))) (cond ((>= nonfinish_share 0.5) (quote blocker-removal-route)) ((>= wrong_complete_share 0.5) (quote capability-gap-no-weight-edit)) (t (quote collect-more-evidence))))))`
- env: `{ "answers_total": <count>, "answers_hit_limit": <count>, "answers_wrong_complete": <count> }`
- `blocker-removal-route` → continue to Step 3.
- `capability-gap-no-weight-edit` → stop; report the route verdict. Weight edits remove obstacles; they do not install capabilities (the anchor's Gemma control is the standing counter-example).
- `collect-more-evidence` or `empty-baseline` → enlarge the practice set, re-measure, re-run the gate.

### Step 3 — Localize: contrastive

1. Capture internals for both trajectory classes: answers that progress normally, and answers that fall into the diagnosed failure. Same model, same prompt family, differing outcome.
2. Call `render_template`:

```
template: direct-model-edit/localize
variables:
  model_name: <target>
  failure_mode: <the classified failure from Step 1>
  trajectory_contrast: { progressing: <captures>, failing: <captures>, layers: <layer count> }
  localization_notes: <which sites were compared and how>
```

3. Following the template's schema, isolate the candidate direction: the internal pattern that consistently appears immediately before the failure begins. The anchor's signature: one direction, written by the MLPs of two layers, appearing right before repetition.
4. Evidence bar (P, critiqued by the operator): present in failing trajectories, absent or late in progressing ones, written at specific named sites — not a wholesale layer.

### Step 4 — Plan the minimal edit

1. Call `render_template`:

```
template: direct-model-edit/edit-plan
variables:
  model_name: <target>
  selected_site: { layers: [...], module: <mlp|attention>, columns: <n>, direction_source: <capture set> }
  total_weights: <count>
  edited_weights: <proposed surface size, weights>
  forward_pass_budget: <n>
```

2. Check the surface budget with `lisp_eval` (DME-2):
   - form: `(let ((budget (* total_weights 0.001))) (list (<= edited_weights budget) budget))`
   - env: `{ "total_weights": <count>, "edited_weights": <count> }`
   - Returns `[within_budget, budget]`. A false first element → shrink the surface (fewer columns, fewer layers, rank-1 only) or stop. The anchor passed at 131,072 of ~800,000,000 (0.016%) — six times under this skill's 0.1% cap.
3. The edit operation is subtractive by default: remove the direction from the site's output columns ("much like removing a behavior by deleting a single direction"). Additive edits need an explicit preservation plan naming the new behavior they risk.

### Step 5 — Apply, re-measure, verify

1. Apply the edit in the weights session. Re-run the SAME practice benchmark under the SAME config.
2. Call `render_template`:

```
template: direct-model-edit/verify
variables:
  model_name: <target>
  edit_summary: { layers, columns, operation, edited_weights }
  post_edit_counts: { answers_total, answers_correct_complete, answers_wrong_complete, answers_hit_limit }
  control_model_result: { control_name, control_nonfinish_rate, control_delta }
```

3. Re-measure the classes and record completion rates beside scores — the attribution gate needs both.
4. Run the sibling control: a non-failing model edited under the same recipe. It should show approximately no gain; if it gains, name the general-booster suspicion and the collateral risk.

### Step 6 — Attribute the gain

Call `lisp_eval` (DME-3):
- form: `(let ((unblocked_gain (* 100 (* (- base_nonfinish new_nonfinish) complete_correct_rate)))) (cond ((<= delta_score 0) (quote score-declined-or-flat)) ((>= (/ unblocked_gain delta_score) 0.5) (quote mostly-unblocking)) (t (quote mixed-or-genuine-gain))))`
- env: `{ "base_nonfinish": <hit-limit share pre-edit>, "new_nonfinish": <hit-limit share post-edit>, "complete_correct_rate": <correctness among completed answers post-edit>, "delta_score": <benchmark points gained> }`
- `mostly-unblocking` → the headline is a completion fix, not a capability gain — say exactly that (the anchor: its 6x "math" jump was loops removed, 93% → 33%; the authors wrote that this "is not the same as making a model better at math").
- `mixed-or-genuine-gain` → completions do not explain the gain; scrutinize before claiming capability.

### Step 7 — Converge

After each verify pass, count what the oracle still reports open — remaining hit-limit share, regressed benchmarks, un-reproduced claims. Record the count and check convergence with `lisp_eval` (DME-4) over the recorded counts, oldest first:
- form: `(and (>= (length xs) 3) (= (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs)) (= (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs)))`
- env: `{ "xs": [<open-findings counts, oldest first>] }`
- Converged when the last three counts are equal (two counts can never satisfy the form). Maximum 5 iterations, then the remaining instability is reported, not iterated. Non-smooth gains are expected: the anchor's jump landed at the 50-forward-pass budget, barely visible at 10 or 25.

### Step 8 — Report

The report carries: the DME-0 config-gate result, the DME-1 route verdict, baseline and post-edit answer classes, benchmark deltas beside completion deltas, the DME-3 attribution label, the sibling control, the edit diff (layers, columns, operation), every forward-pass budget point tried, seeds, and logs. Logs or it did not happen — the anchor published every exchange. If any MCP tool call fails along the way, call `curator_report_skill_use_issue` with `skill_name: "direct-model-edit"`, the tool name, and the error, then continue with the best available information.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `baseline.j2` | Ground a direct weight-edit task: audit the target model's baseline behavior and sampling configuration against its vendor model card before any weight is touched. |
| `localize.j2` | Localize a diagnosed failure mode to weights: contrast internal activations across progressing and failing trajectories to isolate the direction, layer, and columns that causally precede the failure. |
| `edit-plan.j2` | Plan the minimal weight edit: choose the smallest parameter surface that removes the localized failure direction while preserving every other capability, under an explicit budget of edited weights and forward passes. |
| `verify.j2` | Verify an applied weight edit and attribute its gains: re-measure benchmarks and completion rates, separate unblocked answers from genuine capability gains, and check a non-failing sibling control. |

To render a template, call the `render_template` tool with the ref (e.g. `direct-model-edit/baseline`) and a context object with the required variables.

Template context variables (from each template's `[inference]` contract):
- `baseline.j2`: `model_name`, `model_card_notes`, `run_config`, `baseline_counts`, `benchmark_name`
- `localize.j2`: `model_name`, `failure_mode`, `trajectory_contrast`, `localization_notes`
- `edit-plan.j2`: `model_name`, `selected_site`, `total_weights`, `edited_weights`, `forward_pass_budget`
- `verify.j2`: `model_name`, `edit_summary`, `post_edit_counts`, `control_model_result`

## Constraints

- Measure before you diagnose: the anchor's "math bug" was 93% non-terminating answers; completed answers were already right almost every time.
- The config gate precedes every weight read. Pin the sampling settings — mode, temperature, top-p, top-k, presence penalty, token limit — and diff them against the vendor card. The anchor ran in the exact mode the card warns about, at a temperature-and-penalty combination associated with looping.
- Weight edits remove obstacles; they do not install capabilities — the Gemma control is the standing counter-example. Route capability gaps out.
- Localize by contrast (failing vs progressing trajectories), never by intuition; the causal direction is the one that reliably appears immediately before the failure.
- Cap the edit surface at 0.1% of total weights (DME-2); the anchor's 0.016% is the reference point, not the limit.
- Report the whole budget curve, not just the best point — gains can be non-smooth across forward-pass budgets.
- Every headline gain carries its DME-3 attribution label; every verification table carries completion-rate deltas beside benchmark deltas.
- The sibling control is not optional: a report without it is an unverified claim of specificity.
- Logs or it did not happen: the full exchange, the edit diff, the budgets tried, the seeds.
- The 0.1% cap, the 0.5 routing thresholds, and the convergence bound are Evidence-tier operationalizations of the anchor's single published case, not prohibitions.
- The minimal-surface/maximal-behavior shape of a good edit is Ousterhout-derived (the same lens as the `deep-module` skill, applied to a weight patch instead of a code module).

## Regression case

Forms parse-verified by `check_skill_lisp_forms`; semantics executed at use time by the executor with `lisp_eval` — expected values:

- DME-0, config differs (the anchor's own case): `{ "run_mode": "thinking", "card_default_mode": "non-thinking", "run_temp": 0.6, "card_temp": 0.7 }` → `config-differs-rerun-at-card-config`. Matching mode and temperature → `config-matches-proceed`.
- DME-1, the anchor's baseline: `{ "answers_total": 96, "answers_hit_limit": 85, "answers_wrong_complete": 4 }` → 85/96 ≈ 0.886 ≥ 0.5 → `blocker-removal-route`. A Gemma-shaped baseline: `{ "answers_total": 100, "answers_hit_limit": 8, "answers_wrong_complete": 60 }` → 0.08 < 0.5, 0.6 ≥ 0.5 → `capability-gap-no-weight-edit`.
- DME-2, the anchor's surface: `{ "total_weights": 800000000, "edited_weights": 131072 }` → budget = 800,000; 131,072 ≤ 800,000 → `[true, 800000]` (0.016% passes the 0.1% cap).
- DME-3, the anchor's attribution: `{ "base_nonfinish": 0.93, "new_nonfinish": 0.33, "complete_correct_rate": 0.72, "delta_score": 40.2 }` → unblocked_gain = 100 × 0.60 × 0.72 = 43.2; 43.2 / 40.2 ≈ 1.07 ≥ 0.5 → `mostly-unblocking`.
- DME-4, converged: `{ "xs": [61, 12, 12, 12] }` → `true` (last three equal); unstable: `{ "xs": [61, 12, 12, 4] }` → `false`; too few counts: `{ "xs": [12, 12] }` → `false`.