---
name: direct-model-edit
description: "Direct weight-space model editing: ground the baseline behavior and sampling config, route blocker vs capability gap, localize the failure to a causal direction in weight space, subtract it under an explicit surface budget, and verify with honest gain attribution. Anchored on MIT CSAIL's 'Sparks of Weight-Space Programming' (Takehi, Teoh, Zhang, Khattab; 2026-11-02 blog post, public logs), whose frontier-model edit lifted Qwen3.5-0.8B MATH-500 from 7.9% to 48.1% by removing one looping direction from 131,072 weights (~0.016%)."
---

# Direct Model Edit

A frontier model can read and write another network's raw weights directly — no gradient descent, no training loop. The anchor case: MIT CSAIL researchers gave GPT-6 Astra ("Astra") a Python kernel over Qwen3.5-0.8B's weights, practice problems from the GSM8K and MATH training sets, and a budget of forward passes — no gradients, no test-set access. The model's "math bug" was not math: 85 of its first 96 practice answers ran to the 32,768-token limit. Astra compared internals across progressing and looping answers, localized a single direction written by the MLPs of layers 15 and 19, and subtracted it from 64 output columns per layer — 131,072 weights, about 0.016% of the model. Token-limit hits fell from 93% to 33%; MATH-500 rose 7.9% → 48.1%, GSM8K 13.8% → 67.1%, IFEval 21.0% → 62.3%. The authors' own framing, scoped, is this skill's core law: in that one language-model pair, editing weights "could remove an obstacle that hid an ability, but it could not add an ability the model lacked" — an edit to a small surface (about 0.017% of the language model's weights, applied at strength 6 — see the post-publication record below) only unblocked answers the model could already produce. The evidence is one control: Gemma 3 270M, which rarely loops, stayed flat within ~1 point — a different model, about a third the size, run non-thinking with an 8,192-token limit and graded leniently (strict grading: 2.8 → 2.6–3.8, per Achintya K J's audit cited below). This skill covers that edit class; constructive edits are outside this skill, not shown impossible (the same post's Experiment 4 wrote a new controller into a control policy: 0 of 256 → 1,024 of 1,024 successes, 3.5% of its weights). The anchor's Python kernel is a fact about the published method, never a tooling prescription here — the harness-language constraint below is absolute.

The edit itself, read through Ousterhout's lens, is a deep module: an interface of one direction and two layer indices, hiding 800 million weights of behavior behind it. Small surface, large consequence — that shape is the discipline this skill enforces.

Post-publication record (cited so this skill's numbers carry both their corroboration and their corrections): the released edit was independently confirmed — byte-checked tensors, a 51% MATH-500 re-run (Achintya K J, 'Does "Sparks of Weight-Space Programming" hold up?', 2026-11-04, achintyakj.tech/blog/weight-space-audit) — while Toledo's two-decider byte analysis certified the edit as released and corrected the source's wording: 130,188 entries changed, not 131,072 (that is the addressed count; 884 round back to their original values); 0.015% of the release / 0.017% of the language model, with 0.016% true only over a round 0.8B; and the operation is W = W₀ − 6·u(uᵀW₀) — the direction's component reversed and amplified five-fold, not a plain subtraction (Carlos Toledo, 'Weight-Space Programming: the edit decided from its bytes', carlostoledo.co/reports/weight-space-programming.html, run 2026-11-03). Achintya K J's audit adds three points: the "one direction" is two — the layer-15 and layer-19 vectors differ (absolute cosine 0.849); each was built from four pairs of short texts Astra wrote (a productive continuation against a hand-written loop), not from captured answers; and the Gemma control's scores use lenient grading (strict: 2.8 → 2.6–3.8). Toledo's pre-registered card-settings runs confirmed what this skill's config gates encode: the unedited model scored 7.2% at the post's settings, 22.3% at the card's thinking text settings, 44.0% at the card's non-thinking text settings — 'mostly a settings effect' is the pre-registered reading. This skill teaches the source's published method and reports its numbers as published; the corrections above are part of the anchor's honest record, not a change of method.

## Initial and target condition

- **Initial condition:** a target model (open weights, small enough to hold resident in a Rust harness), its vendor model card, a reproducible Rust + bash harness with practice problems, a measured baseline split into answer classes (correct-complete / wrong-complete / hit-limit), the sampling configuration in force (mode, temperature, top-p, top-k, presence penalty, token limit), and a reported behavioral bug.
- **Target condition:** the DME-0 config gate and DME-1 route verdict are recorded; if blocker-routed — an applied edit whose surface passes the DME-2 budget and whose preservation plan passes DME-2b, a verification table carrying benchmark deltas AND completion-rate deltas side by side, a DME-3 attribution label on every headline gain, a non-failing sibling control run under the same recipe with its DME-3b verdict, and a report carrying the full edit diff, every forward-pass budget point tried, seeds, and logs.

## When to Use

- A small open-weights model exhibits a reproducible behavioral failure — repetition loops, stuck local minima, a specific trigger it mishandles — and you want to fix the weights directly, without training.
- You are auditing weights for a hidden failure mode before or instead of benchmarking.
- You must attribute an existing "model got better" claim honestly — separating answers unblocked from failure loops out of answers the model could not produce before.

## When NOT to Use

- Capability gaps — the model finishes its answers and they are wrong. The anchor's own Gemma control: wrong arithmetic and invalid algebra have "no single component to switch off"; edits stayed within ~1 point and Astra itself reported the gain as "not established". Route capability gaps out of this skill, to training or distillation: this skill's edit class has only been shown to unblock, in one model pair with one control (the core law's scope, above).
- Closed-weight models with no weights interface — there is nothing to read or edit.
- A config delta the Ground step has not examined — run the DME-0 gate first. A bug that disappears at the vendor card's recommended settings for the same mode is a settings bug, not a weight bug; a bug that disappears only when the mode is switched (thinking to non-thinking) is a mode decision for the operator, not proof the weights are sound.
- Prompt or harness bugs — if the fix belongs in the harness, it does not belong in the weights.

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Ground — baseline counts | D (counts measured by the harness) | the harness's eval output |
| 1 Ground — config gate | D | `lisp_eval` DME-0 |
| 1 Ground — failure-mode reading | P | the operator; critiqued by DME-1's route arithmetic |
| 2 Route | D | `lisp_eval` DME-1 over the measured answer classes; DME-1S over the zero-shot/few-shot presence probe (adjudicated at the card config) |
| 3 Localize | P | the contrast captures; critiqued by the re-appearance of the direction before failures |
| 4 Plan | P | the operator critiques the surface choice; DME-2 and DME-2b are the D gates on it |
| 5 Apply + re-measure | D (re-measurement) | the harness's eval output; DME-0 (config identity, second application) and DME-3b (sibling verdict) gate it |
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

4. Following the template's output schema, produce the baseline observation: classify the failed answers (non-terminating vs wrong-but-complete) and record the card's recommended six-field configuration (`card_config`) for the DME-0 gate — never diff the fields yourself. The anchor's own trap: it ran in thinking mode — the exact condition the Qwen card warns loops — at temperature 0.6 with presence penalty 1.5.
5. Call `lisp_eval` with the config gate (DME-0) over all six card-config fields:
   - form: `(let ((diffs (append (if (string= (assoc! "mode" run_config) (assoc! "mode" card_config)) () (list "mode")) (if (= (assoc! "temperature" run_config) (assoc! "temperature" card_config)) () (list "temperature")) (if (= (assoc! "top_p" run_config) (assoc! "top_p" card_config)) () (list "top_p")) (if (= (assoc! "top_k" run_config) (assoc! "top_k" card_config)) () (list "top_k")) (if (= (assoc! "presence_penalty" run_config) (assoc! "presence_penalty" card_config)) () (list "presence_penalty")) (if (= (assoc! "token_limit" run_config) (assoc! "token_limit" card_config)) () (list "token_limit"))))) (cond ((> (length diffs) 0) (list (quote config-differs-rerun-at-card-config) diffs)) (t (quote config-matches-proceed))))`
   - env: `{ "run_config": { mode, temperature, top_p, top_k, presence_penalty, token_limit } as in force, "card_config": { the card's recommended value for the same six fields, from the template's output } }` — both objects carry all six fields; a missing field fails the form's `assoc!` loudly, never silently.
   - `config-differs-rerun-at-card-config` → the emitted differing-field list names exactly what moved: re-run the baseline at the card's config and re-gate before Step 2, once. If the second gate still reports a difference, stop and report the differing fields; do not re-run again. A bug that disappears at the vendor's recommended settings for the same mode is a settings bug; one that disappears only by switching mode is a mode decision for the operator. Name the card row compared: the Qwen3.5-0.8B card recommends four sampling rows (thinking or non-thinking, text or vision-language). Against the vision-language non-thinking row (temperature 0.7, top_p 0.8, presence penalty 1.5 — the Regression case's `card_config`) the anchor's case differs in mode, temperature AND top_p — a two-field gate misses the third. Against the text non-thinking row (temperature 1.0, top_p 1.0, presence penalty 2.0) it differs in presence penalty as well. Against the text thinking row (temperature 1.0, with the 81,920-token limit the card suggests for competition math) it differs only in temperature and token limit, and there Toledo measured 75.0% of answers still at the token cap. The diff-and-rerun protocol itself is this skill's operationalization of that trap — the source published the trap but never ran this discipline; the card-config re-measurement was done post-publication, independently (Toledo: the unedited model scores 7.2% at the post's settings but 22.3% at the card's thinking text settings and 44.0% at its non-thinking text settings).

### Step 2 — Route: blocker or capability gap

Call `lisp_eval` with the route gate (DME-1) over the measured counts — it derives the shares itself, never hand-assert them:
- form: `(cond ((= answers_total 0) (quote empty-baseline)) (t (let ((nonfinish_share (/ answers_hit_limit answers_total)) (wrong_complete_share (/ answers_wrong_complete answers_total))) (cond ((>= nonfinish_share 0.5) (quote blocker-removal-route)) ((>= wrong_complete_share 0.5) (quote capability-gap-no-weight-edit)) (t (quote collect-more-evidence))))))`
- env: `{ "answers_total": <count>, "answers_hit_limit": <count>, "answers_wrong_complete": <count> }`
- `blocker-removal-route` → continue to Step 3.
- `capability-gap-no-weight-edit` → stop; report the route verdict. In the anchor's one model pair, this skill's edit class removed an obstacle and did not add an ability; the Gemma control is that single case, with the confounds named in the introduction, not a general counter-example.
- `collect-more-evidence` or `empty-baseline` → enlarge the practice set, re-measure, re-run the gate. Enlarge at most twice; if the gate still returns either verdict, stop and report the route as undetermined, with no weight edit.

Sibling gate (DME-1S) — the skill-presence probe. When the route question is whether the model possesses the skill the failure suppresses — before any localization spend — probe task conformance on practice problems twice, zero-shot and few-shot, and gate the verdict with `lisp_eval`:
- form: `(cond ((= conformance_sample 0) (quote collect-more-evidence)) ((< few_shot_conformance 0.5) (quote capability-gap-no-weight-edit)) ((> (- few_shot_conformance zero_shot_conformance) 0.1) (quote skill-unlock-route)) (t (quote skill-present-not-suppressed)))`
- env: `{ "zero_shot_conformance": <share of conforming answers, zero-shot>, "few_shot_conformance": <share of conforming answers, few-shot>, "conformance_sample": <practice answers probed per arm> }`
- The probe ADJUDICATES AT THE CARD CONFIG — the model's default mode — never only at trap-style configs. Config-dependence is a recorded live finding, not a hypothesis: the anchor's own case reproduced the loop at the trap config (6 of 7 answers hit-limit) and did NOT survive the card config (0 of 10 hit-limit — at card settings DME-1 itself routes that same baseline to `capability-gap-no-weight-edit`); receipts at ~/Documents/zk-data/direct-model-edit-archive/logs/stage1-summary.json (the archived stage-1 instrument's historical record; the Rust replication re-measures). A presence probe run at trap-style configs measures the trap, not the skill. The probe's design — zero-shot vs few-shot conformance as a skill-presence test — is this skill's original instrument: the anchor ran no presence probe; the probe is grounded in the stage-1 receipts above and the anchor's published config facts, with its thresholds disclosed as Evidence-tier operationalizations in Constraints.
- `skill-unlock-route` → the skill is present but suppressed: continue to Step 3. `capability-gap-no-weight-edit` → stop, as DME-1's same verdict. `skill-present-not-suppressed` → the probed failure is not suppressing a skill: re-examine the diagnosis before spending localization. `collect-more-evidence` → enlarge the probe sample and re-run DME-1S, at most twice; after that, report skill presence as undetermined and do not spend localization.

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
4. Evidence bar (P, critiqued by the operator): present in failing trajectories, absent or late in progressing ones, written at specific named sites — not a wholesale layer. A direction that clears this bar is a candidate, not yet a cause: the contrast is correlational. The anchor supported "targeted" by intervention — a strength series (1, 2, 3, 4, 6), layer-isolation tests, and a norm-matched random direction that gained 1.6 points. This skill has no gate for that intervention evidence yet (commissioned on Algedonic review card e16da528); until it does, record which of the three were run and report the direction as a candidate where none were. The "immediately before the failure" timing criterion is this skill's own operationalization; that wording was not found in the post.

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
   - Returns `[within_budget, budget]`. A false first element → shrink the surface (fewer columns, fewer layers, rank-1 only) or stop. Shrink and re-check at most twice; a surface still over budget after the second shrink stops the run, reporting the last surface and the budget. The anchor passed at 131,072 of a measured 752,393,024 (0.0174% — the Qwen3.5-0.8B BF16 param count via gguf-info; the ~800M figure was an estimate) — six times under this skill's 0.1% cap. The cap is also MECHANICAL in the harness (corrective phase 2026-11-07, commit 376b4025): convert states the projection on every manifest (`projected_weights_touched`, `projected_edited_share` from the config's required `total_weights`), the pre-emit lisp gate refuses any share above 0.001, and `edit::run` hard-refuses against the parsed GGUF's real param_count — a typed DME-2 refusal from the harness is a finding to answer with a smaller surface, never a bypass attempt.
3. The edit operation is subtractive by default: remove the direction from the site's output columns ("much like removing a behavior by deleting a single direction" — the source's own footnote for that phrase is Arditi et al., 'Refusal in Language Models Is Mediated by a Single Direction', arXiv:2406.11717, 2024; the wider published editing lineage the source situates itself in: Meng et al., 'Locating and Editing Factual Associations in GPT' (ROME), arXiv:2202.05262, 2022, and Elhage et al., 'A Mathematical Framework for Transformer Circuits', 2021). Additive edits need an explicit preservation plan naming the new behavior they risk — a requirement that is this skill's discipline, not the source's (the anchor's edit was described as subtraction but applied at strength 6 — the component along the direction reversed and amplified five-fold, per the post-publication record above; this gate does not read strength and passes any operation labelled "subtract", and the DME-2 cap above counts entries addressed, not the size of the change; grafting appears in the source only as future speculation) — enforce it with the preservation gate (DME-2b) in `lisp_eval`:
   - form: `(cond ((string= operation "subtract") (quote subtractive-default-ok)) ((string= risked_new_behavior "") (quote additive-edit-missing-preservation-plan)) (t (quote additive-edit-has-preservation-plan)))`
   - env: `{ "operation": <edit_spec.operation>, "risked_new_behavior": <preservation_plan.risked_new_behavior — blank when the plan names none> }`
   - `additive-edit-missing-preservation-plan` → the plan is not a plan: name the risked new behavior in the template's output and re-gate before applying anything, once; a plan that still names none is not applied — stop and report. `subtractive-default-ok` / `additive-edit-has-preservation-plan` → proceed.

### Step 5 — Apply, re-measure, verify

1. Apply the edit in the Rust harness's resident session. Re-run the SAME practice benchmark under the SAME config.
2. Assert before/after config identity with the DME-0 form's second application — the same form and the same six fields as Step 1, the two configs as its objects:
   - form: `(let ((diffs (append (if (string= (assoc! "mode" run_config) (assoc! "mode" card_config)) () (list "mode")) (if (= (assoc! "temperature" run_config) (assoc! "temperature" card_config)) () (list "temperature")) (if (= (assoc! "top_p" run_config) (assoc! "top_p" card_config)) () (list "top_p")) (if (= (assoc! "top_k" run_config) (assoc! "top_k" card_config)) () (list "top_k")) (if (= (assoc! "presence_penalty" run_config) (assoc! "presence_penalty" card_config)) () (list "presence_penalty")) (if (= (assoc! "token_limit" run_config) (assoc! "token_limit" card_config)) () (list "token_limit"))))) (cond ((> (length diffs) 0) (list (quote config-differs-rerun-at-card-config) diffs)) (t (quote config-matches-proceed))))`
   - env: `{ "run_config": <the pre-edit six-field config in force>, "card_config": <the post-edit six-field config in force> }`
   - `config-matches-proceed` → the only intervention is the edit. `config-differs-rerun-at-card-config` with any field named → a config change rode along with the edit and this verification verifies nothing ("a verification under different settings verifies nothing" — the verify template's own constraint): re-run the post-edit measurement under the pre-edit config before Step 6.
3. Call `render_template`:

```
template: direct-model-edit/verify
variables:
  model_name: <target>
  edit_summary: { layers, columns, operation, edited_weights }
  post_edit_counts: { answers_total, answers_correct_complete, answers_wrong_complete, answers_hit_limit }
  control_model_result: { control_name, control_nonfinish_rate, control_delta }
```

4. Re-measure the classes and record completion rates beside scores — the attribution gate needs both.
5. Run the sibling control: a non-failing model edited under the same recipe, then gate its verdict (DME-3b) with `lisp_eval`:
   - form: `(cond ((<= (abs control_delta) flat_band) (quote sibling-flat)) (t (quote sibling-moved-general-booster-suspect)))`
   - env: `{ "control_delta": <the sibling's before/after benchmark delta>, "flat_band": 1.0 }` — the band is Evidence-tier: the anchor's Gemma control stayed flat within about one point (6.8 → 7.0), a gain Astra itself reported as not established.
   - `sibling-moved-general-booster-suspect` → name the general-booster suspicion and the collateral risk out loud; the specificity claim is not established. `sibling-flat` → the control corroborates specificity.

### Step 6 — Attribute the gain

Call `lisp_eval` (DME-3):
- form: `(let ((delta_score (- new_score base_score)) (unblocked_gain (* 100 (* (- base_nonfinish new_nonfinish) complete_correct_rate)))) (cond ((<= delta_score 0) (quote score-declined-or-flat)) ((>= (/ unblocked_gain delta_score) 0.5) (quote mostly-unblocking)) (t (quote mixed-or-genuine-gain))))`
- env: `{ "base_nonfinish": <hit-limit share pre-edit>, "new_nonfinish": <hit-limit share post-edit>, "complete_correct_rate": <correctness among completed answers post-edit>, "base_score": <benchmark score pre-edit>, "new_score": <benchmark score post-edit> }` — delta_score is derived inside the form from the measured scores; measured counts in, no hand arithmetic feeding a D gate. The formula itself — unblocked_gain = 100 × Δnonfinish × complete_correct_rate against the measured delta — is this skill's operationalization of the source's qualitative rescue-accounting (576 of its 579 rescues were originally truncated outputs; accuracy on originally finished outputs unchanged); the source performs no such arithmetic.
- `mostly-unblocking` → the headline is a completion fix, not a capability gain — say exactly that (the anchor: its 6x "math" jump was loops removed, 93% → 33%; the authors wrote that this "is not the same as making a model better at math").
- `mixed-or-genuine-gain` → completions do not explain the gain; scrutinize before claiming capability.

### Step 7 — Converge

After each verify pass, count what the oracle still reports open — remaining hit-limit share, regressed benchmarks, un-reproduced claims. Record the count and check convergence with `lisp_eval` (DME-4) over the recorded counts, oldest first:
- form: `(and (>= (length xs) 3) (= (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs)) (= (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs)))`
- env: `{ "xs": [<open-findings counts, oldest first>] }`
- Converged when the last three counts are equal (two counts can never satisfy the form). Maximum 5 iterations, then the remaining instability is reported, not iterated. Non-smooth gains are expected: the anchor's jump landed at the 50-forward-pass budget, barely visible at 10 or 25.

### Step 8 — Report

The report carries: the DME-0 config-gate result, the DME-1 route verdict, baseline and post-edit answer classes, benchmark deltas beside completion deltas, the DME-3 attribution label, the sibling control, the edit diff (layers, columns, operation), every forward-pass budget point tried, seeds, and logs. Logs or it did not happen — the anchor released its logs, though not exhaustively: the logs' own README lists omissions, and the independent audit found earlier trial runs of earlier prompt versions left out, their number undisclosed. If any MCP tool call fails along the way, call `curator_report_skill_use_issue` with `skill_name: "direct-model-edit"`, the tool name, and the error, then continue with the best available information.

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

- The harness language boundary is absolute (operator ruling 2026-11-06, extending the 2026-10-13 zero-Python rule beyond the repo tree): the harness is Rust + bash — no Python anywhere an agent authors — not in the repo, not in a replication workspace under the data dirs, not "temporarily". Ecosystem convenience is not an exception path: if the target architecture or capture machinery is unavailable in Rust on this machine (candle/burn architecture support, the ROCm backend), that is a surfaced capability gap routed to the operator under this skill's own routing discipline — never a silent language substitution. A Python harness is this skill's refusal state, not its Step 1.
- Measure before you diagnose: the anchor's "math bug" was 93% non-terminating answers (the reasoning-loop phenomenon the anchor itself cites: Pipis et al., 'Wait, Wait, Wait... Why Do Reasoning Models Loop?', arXiv:2512.12895, 2025); completed answers were already right almost every time.
- The config gate precedes every weight read. Pin the sampling settings — mode, temperature, top-p, top-k, presence penalty, token limit — and diff them against the vendor card. The anchor ran in the exact mode the card warns about, at a temperature-and-penalty combination associated with looping.
- This skill's edit class removes obstacles; in the anchor's one model pair it did not install a capability — the Gemma control is one confounded case (a different model, about a third the size, non-thinking, 8,192-token limit, lenient grading), not a general law. Constructive edits are outside this skill, not shown impossible. Route capability gaps out.
- Localize by contrast (failing vs progressing trajectories), never by intuition; the candidate direction is the one that reliably appears immediately before the failure. Contrast makes it a candidate; only intervention (a strength series, a norm-matched random-direction control) supports calling it causal.
- Cap the edit surface at 0.1% of total weights (DME-2); the anchor's 0.016% is the reference point, not the limit. The harness enforces this cap mechanically — the pre-emit gate refuses over-cap manifests by name and the edit leg hard-refuses on the parsed GGUF's real param_count (commit 376b4025); the lisp_eval form stays as the decision-time discipline.
- Provenance at commissioning (operator ruling 2026-11-07, the corrective phase's ground rule): every edit geometry, trigger design, localization method, and measurement construct named in a commissioning carries its provenance line — the literature anchor resolved through the research server with recorded provenance, or the explicit novel/exploratory label. Novel is legitimate when explicit; unanchored-by-omission is the drift. A commissioning without provenance lines is malformed: challenge it at intake, before any leg runs.
- Report the whole budget curve, not just the best point — gains can be non-smooth across forward-pass budgets.
- Every headline gain carries its DME-3 attribution label; every verification table carries completion-rate deltas beside benchmark deltas.
- The sibling control is not optional: a report without it is an unverified claim of specificity.
- Logs or it did not happen: the full exchange, the edit diff, the budgets tried, the seeds.
- The gate apparatus — DME-0's diff-and-rerun protocol, DME-1S's probe design, DME-2's cap, DME-2b's preservation rule, DME-3's attribution formula, DME-3b's band, DME-4's convergence rule, and their constants (the 0.1% cap, the 0.5 routing thresholds, DME-1S's 0.5/0.1 presence thresholds, DME-3b's flat band, the convergence bound) — is this skill's Evidence-tier operationalization of the anchor's single published case: original generalization machinery the source does not contain, labeled as such — operationalizations, not prohibitions. It shipped as deterministic `lisp_eval` gates with parse-pinned regression forms, and the generalization layer's machine-checked Lean treatment now exists: `kask/lean/dme_gates.lean` (Lean 4 v4.34.0, core only, checked by `lean_check` against the pinned Lake toolchain) proves the gates' properties as modeled there (the pin's header states the modeling boundary) — DME-0's diff names exactly the differing fields, order-preserving, with `.matches` iff all six fields are equal and no empty-diff false alarm; DME-2's cap in exact Nat arithmetic: the anchor regression instance passes, shrinking the surface preserves passing, uniform scaling preserves the verdict; DME-4's index arithmetic decides the last-three-equal spec exactly, with the stability and instability corollaries — and `kask/lean/dme_gates_negative_control.lean` must fail to check (decide refutes all three closed false claims, one per gate).
- The minimal-surface/maximal-behavior shape of a good edit is Ousterhout-derived (the same lens as the `deep-module` skill, applied to a weight patch instead of a code module).

## Regression case

Forms parse-verified by `check_skill_lisp_forms`; semantics executed at use time by the executor with `lisp_eval` — expected values:

- DME-0, config differs (the anchor's own case, all six fields): `{ "run_config": { "mode": "thinking", "temperature": 0.6, "top_p": 0.95, "top_k": 20, "presence_penalty": 1.5, "token_limit": 32768 }, "card_config": { "mode": "non-thinking", "temperature": 0.7, "top_p": 0.8, "top_k": 20, "presence_penalty": 1.5, "token_limit": 32768 } }` → `["config-differs-rerun-at-card-config", ["mode", "temperature", "top_p"]]` — the three-field diff a two-field gate missed. The card's own non-thinking settings on both sides → `config-matches-proceed`.
- DME-1, the anchor's baseline: `{ "answers_total": 96, "answers_hit_limit": 85, "answers_wrong_complete": 4 }` → 85/96 ≈ 0.886 ≥ 0.5 → `blocker-removal-route`. A Gemma-shaped baseline: `{ "answers_total": 100, "answers_hit_limit": 8, "answers_wrong_complete": 60 }` → 0.08 < 0.5, 0.6 ≥ 0.5 → `capability-gap-no-weight-edit`.
- DME-1S, presence probe (adjudicated at the card config): `{ "zero_shot_conformance": 0.08, "few_shot_conformance": 0.75, "conformance_sample": 96 }` → `skill-unlock-route`; `{ "zero_shot_conformance": 0.10, "few_shot_conformance": 0.15, "conformance_sample": 96 }` → `capability-gap-no-weight-edit`; `{ "zero_shot_conformance": 0.55, "few_shot_conformance": 0.60, "conformance_sample": 96 }` → `skill-present-not-suppressed`; `{ "conformance_sample": 0 }` → `collect-more-evidence`.
- DME-2, the anchor's surface: `{ "total_weights": 752393024, "edited_weights": 131072 }` → budget = 752,393.024; 131,072 ≤ 752,393.024 → `[true, 752393.024]` (0.0174% of the measured Qwen3.5-0.8B BF16 param count passes the 0.1% cap).
- DME-2b, preservation gate: `{ "operation": "subtract", "risked_new_behavior": "" }` → `subtractive-default-ok`; `{ "operation": "add", "risked_new_behavior": "" }` → `additive-edit-missing-preservation-plan`; `{ "operation": "add", "risked_new_behavior": "over-release: the grafted direction fires on benign prompts" }` → `additive-edit-has-preservation-plan`.
- DME-3, the anchor's attribution (delta derived in-form): `{ "base_nonfinish": 0.93, "new_nonfinish": 0.33, "complete_correct_rate": 0.72, "base_score": 7.9, "new_score": 48.1 }` → delta_score = 48.1 − 7.9 = 40.2 in-form; unblocked_gain = 100 × 0.60 × 0.72 = 43.2; 43.2 / 40.2 ≈ 1.07 ≥ 0.5 → `mostly-unblocking`.
- DME-3b, sibling verdict (band 1.0, Evidence-tier): `{ "control_delta": 0.2, "flat_band": 1.0 }` → `sibling-flat` (the anchor's recorded 6.8 → 7.0 control); `{ "control_delta": 5.0, "flat_band": 1.0 }` → `sibling-moved-general-booster-suspect`.
- DME-4, converged: `{ "xs": [61, 12, 12, 12] }` → `true` (last three equal); unstable: `{ "xs": [61, 12, 12, 4] }` → `false`; too few counts: `{ "xs": [12, 12] }` → `false`.