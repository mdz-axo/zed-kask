---
name: direct-model-edit
description: "Direct weight-space model editing: check a run against a named vendor-card row, route blocker vs capability gap, localize a candidate direction by contrast, test it by intervention (random-direction control, strength series), edit under a surface budget, and attribute the measured change from paired answers. Anchored on MIT CSAIL's 'Sparks of Weight-Space Programming' (2026-11-02), whose edit of about 0.017% of Qwen3.5-0.8B's weights lifted MATH-500 from 7.9% to 48.1%."
---

# Direct Model Edit

A frontier model can read and write another network's raw weights directly — no gradient descent, no training loop. The anchor case: MIT CSAIL researchers gave GPT-6 Astra ("Astra") a Python kernel over Qwen3.5-0.8B's weights, practice problems from the GSM8K and MATH training sets, and a budget of forward passes — no gradients, no test-set access. The model's "math bug" was not math: 85 of its first 96 practice answers ran to the 32,768-token limit. Astra compared internals across progressing and looping answers, localized a direction written by the MLPs of layers 15 and 19 (two vectors, one per layer — see the post-publication record below), and edited 64 output columns per layer — 131,072 weights addressed, about 0.017% of the language model. Token-limit hits fell from 93% to 33%; MATH-500 rose 7.9% → 48.1%, GSM8K 13.8% → 67.1%, IFEval 21.0% → 62.3%. The authors' own framing, scoped, is this skill's core law: in that one language-model pair, editing weights "could remove an obstacle that hid an ability, but it could not add an ability the model lacked" — an edit to a small surface (about 0.017% of the language model's weights, applied at strength 6 — see the post-publication record below) only unblocked answers the model could already produce. The evidence is one control: Gemma 3 270M, which rarely loops, stayed flat within ~1 point — a different model, about a third the size, run non-thinking with an 8,192-token limit and graded leniently (strict grading: 2.8 → 2.6–3.8, per Achintya K J's audit cited below). This skill covers that edit class; constructive edits are outside this skill, not shown impossible (the same post's Experiment 4 wrote a new controller into a control policy: 0 of 256 → 1,024 of 1,024 successes, 3.5% of its weights). The anchor's Python kernel is a fact about the published method, never a tooling prescription here — the harness-language constraint below is absolute.

The edit itself, read through Ousterhout's lens, is a deep module: an interface of a direction per layer and two layer indices, hiding about 752 million weights of behavior behind it. Small surface, large consequence — that shape is the discipline this skill enforces.

Post-publication record (cited so this skill's numbers carry both their corroboration and their corrections): the released edit was independently confirmed — byte-checked tensors, a 51% MATH-500 re-run (Achintya K J, 'Does "Sparks of Weight-Space Programming" hold up?', 2026-11-04, achintyakj.tech/blog/weight-space-audit) — while Toledo's two-decider byte analysis certified the edit as released and corrected the source's wording: 130,188 entries changed, not 131,072 (that is the addressed count; 884 round back to their original values); 0.015% of the release / 0.017% of the language model, with 0.016% true only over a round 0.8B; and the operation is W = W₀ − 6·u(uᵀW₀) — the direction's component reversed and amplified five-fold, not a plain subtraction (Carlos Toledo, 'Weight-Space Programming: the edit decided from its bytes', carlostoledo.co/reports/weight-space-programming.html, run 2026-11-03). Achintya K J's audit adds three points: the "one direction" is two — the layer-15 and layer-19 vectors differ (absolute cosine 0.849); the direction was built from four pairs of short texts Astra wrote (a productive continuation against a hand-written loop), not from captured answers; and the Gemma control's scores use lenient grading (strict: 2.8 → 2.6–3.8). Toledo's pre-registered card-settings runs confirmed what this skill's config gates encode: the unedited model scored 7.2% at the post's settings, 22.3% at the card's thinking text settings, 44.0% at the card's non-thinking text settings — 'mostly a settings effect' is the pre-registered reading. This skill teaches the source's published method and reports its numbers as published; the corrections above are part of the anchor's honest record, not a change of method.

## Initial and target condition

- **Initial condition:** a target model (open weights, small enough to edit as a GGUF with the Rust harness), its vendor model card saved as fetched, the `dme-harness` and its practice problems, a run config file that names the vendor-card row it follows (`card_row`), a baseline probe rows file with its `summary`, and a reported behavioral bug.
- **Target condition:** the DME-0 and DME-1 verdicts are recorded; if blocker-routed — a candidate direction with its DME-2c and DME-2d verdicts, a preservation baseline that passes DME-2b, an applied edit whose surface passes DME-2, a DME-3 result (verdict, notes, paired counts, operation class) on the target set and on the preservation set, a non-failing sibling control with its DME-3b verdict, and a report carrying the edit specs and receipts, every gate env and result, seeds, and logs.

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
| 1 Ground — baseline rows and counts | D | `dme-harness probe` and `dme-harness summary` |
| 1 Ground — choosing the card row and quoting it | P | the baseline template; critiqued by DME-0, which finds the quote in the saved card and reads the sampling values from it |
| 1 Ground — settings gate | D | `dme-measure.jq` then `lisp_eval` DME-0 |
| 1 Ground — failure-mode reading | P | the operator; critiqued by DME-1's route arithmetic |
| 2 Route | D | `lisp_eval` DME-1 over the measured answer classes; DME-1S over the zero-shot/few-shot presence probe |
| 3 Localize | P | the contrast captures; critiqued by Step 3b's intervention gates |
| 3b Intervene | D | `dme-measure.jq` then `lisp_eval` DME-2c (random-direction control) and DME-2d (strength series) |
| 4 Plan | P | the operator critiques the surface choice; DME-2 and DME-2b are the D gates on it |
| 5 Apply + re-measure | D (re-measurement) | the harness's probe rows and edit receipts; the comparability checks inside DME-3; DME-3b (sibling verdict) |
| 6 Attribute | D | `dme-measure.jq` then `lisp_eval` DME-3 over the paired answers and the receipts |
| 7 Converge | D | `lisp_eval` DME-4 over recorded open-findings counts |
| 8 Report | P | the operator; the logs are the evidence |

Hybrid steps label at sub-step granularity; `render_template` is D (deterministic render) feeding P (model consumption). A route verdict the model asserts but DME-1 does not compute from measured counts is not a route.

## Instructions

### How the measured gates are called (DME-0, DME-2b, DME-2c, DME-2d, DME-3)

These five gates read what the harness wrote. Their files are in `kask/crates/dme-harness/gates/`: `dme-measure.jq` measures, and `dme-0.lisp`, `dme-2b.lisp`, `dme-2c.lisp`, `dme-2d.lisp`, `dme-3.lisp` decide.

1. For every probe rows file, run `dme-harness summary --rows <rows.jsonl> --out <rows.summary.json>` in `terminal`.
2. Run the gate's `jq` command (given at each step) in `terminal`, redirected to a file, e.g. `> dme-3.env.json`. It prints one JSON object: the complete `lisp_eval` env. `dme-measure.jq` counts answer classes, pairs two runs by problem id (and compares seed, gold answer and prompt length per pair), compares the run facts the harness recorded, and reads the edit receipts. It holds no threshold and returns no verdict.
3. `read_file` the gate's `.lisp` file and the env file; call `lisp_eval` with the gate text as `form`, the env file's content as `env`, and `max_steps: 2000000`.

Copy the env file's content unchanged. Never type a count, a ratio or a flag into it. What the form can and cannot catch: it rejects an env whose numbers do not add up (class counts against the answer total, the paired table against both runs' class counts, non-integer or negative counts, strings where numbers belong, an env built for another gate), and it is built to return a verdict rather than raise on a malformed `in` (an env with no `in` key raises `unbound symbol: in`; treat any evaluator error as a refusal and report it). It cannot tell a consistently altered env from a measured one — `lisp_eval` takes its env as an argument, so the object passes through the model. The env file is therefore part of the evidence: keep it with the report so the operator can re-run the same `jq` command and compare. (Closing this gap needs the harness to run both halves itself; see Constraints.)

A change is **established** when both hold: an exact two-sided sign test over the answers that changed class gives p < 0.05 (a continuity-corrected chi-square above 100 such answers), and the net change is more than 1.0 point of all answers — the same band DME-3b calls flat. "Not established" is not "absent": at 96 answers a net change of five answers (5.2 points) is not established. Both numbers are this skill's own thresholds.

Refusals shared by these gates, each naming what is missing: `rows-missing`, `rows-malformed`, `rows-repeat-a-problem`, `rows-disagree-with-harness-summary` (class counts, row count or model-file hash), `run-facts-mixed-within-a-run`, `run-facts-incomplete`, `model-file-hash-missing-or-mixed`, `measurement-missing`, `measurement-inconsistent`, `inputs-not-an-object`, `env-built-for-another-gate`; and, where two runs are compared, `runs-differ-beyond-the-model-file` (with the differing keys), `edited-rows-carry-the-base-model-file`, `edit-receipts-missing`, `receipts-do-not-link-base-to-edited`, `collateral-change-unrecorded`, `collateral-change-reported`, `rows-not-paired` (a problem missing from one run, or a pair whose seed, gold answer or prompt length differs). A refusal about one run is wrapped with the run's name: `["base-run", …]`, `["edited-run", …]`, `["targeted-arm", …]`, `["control-arm", …]`, `["control-to-targeted", …]`, `["preservation-run", …]`, `["target-run", …]`, `["series-arm", n, …]`.

### Step 1 — Ground: baseline audit and the settings gate

1. Measure, do not assume. Before any weight is read, run `dme-harness probe` on practice problems under the current config, then `dme-harness summary` on the rows file. The summary's `counts` are the answer classes: `correct_complete`, `wrong_complete`, `hit_limit` (answers that ran to the token limit without finishing).
2. Save the vendor model card as fetched, with nothing added — no notes, no commentary, no config descriptions in the same file. Read its recommended sampling rows — a card can have several; the Qwen3.5-0.8B card has four (thinking or non-thinking, text or vision-language) — and every warning about failure modes.
3. Call `render_template` to render the baseline audit prompt:

```
template: direct-model-edit/baseline
variables:
  model_name: <target model name>
  model_card_notes: <the saved vendor card text>
  run_config: <the run config file's content>
  baseline_counts: <the summary's counts object>
  benchmark_name: <the practice benchmark family>
```

4. Copy the template's `card_row` object into the run config file under the key `card_row`, and probe again if the rows were written before the key was there. The harness embeds the config file in every row (`config_file`), so the row a run claims to follow is recorded with the run. `card_row` holds: `row` (the card's name for the row), `source_quote` (the card's line for that row, copied character for character), `enable_thinking` and `default_enable_thinking` (the row's mode and the card's default mode), and optionally `cap` and `penalty_last_n` when the card states them. It holds no sampling value: DME-0 reads temperature, top_p, top_k, min_p, presence_penalty and repetition_penalty out of the quote itself (`name=value` or `name: value`). Which row: the one for the mode the deployment will use.
5. Run the settings gate (DME-0) as described under "How the measured gates are called":

```
jq -n -c -L kask/crates/dme-harness/gates --slurpfile rows <rows.jsonl> --slurpfile sum <rows.summary.json> --rawfile card <card.md> 'include "dme-measure"; gate_dme0($rows; $sum[0]; $card)'
```

   - `lisp_eval` with `dme-0.lisp` returns `[verdict, row, moved, silent, notes]`. Compared: the mode the config asked for; the six sampling values the server echoed back for the run (`server_generation_settings`) against the values in the quote; the row's `cap` and `penalty_last_n` against `card_row`. Numbers match within 1e-6 relative.
   - `settings-match-card-row` → the quote states all six sampling values and none differs: proceed. `silent` lists what was not compared (`cap`, `penalty_last_n`, the mode, when `card_row` leaves them out); `notes` may hold `mode-is-not-the-card-default` or `card-default-mode-not-stated`. Carry both lists into the report.
   - `settings-match-on-stated-settings-only` → nothing stated differs, but the quote leaves some sampling values out (they are in `silent`). This is weaker than a match: quote the card's full row, or report the settings that were not compared.
   - `settings-differ-from-card-row` → `moved` names what differs: re-run the baseline at the row's settings and re-gate before Step 2, once. If the second gate still reports a difference, stop and report the differing fields; do not re-run again. A bug that disappears at the row's settings is a settings bug; one that disappears only by switching mode is a mode decision for the operator. The anchor's own trap: its run (thinking, temperature 0.6, top_p 0.95, presence penalty 1.5, cap 32,768) differs from the non-thinking vision-language row in `enable_thinking`, `temperature` and `top_p`; from the non-thinking text row in those three and `presence_penalty`; from the thinking text row (temperature 1.0, with the 81,920-token limit the card suggests for competition math) in `temperature` and `cap` — and there Toledo measured 75.0% of answers still at the token cap. The source published the trap but never ran this discipline; the card-settings re-measurement was done post-publication, independently (Toledo: the unedited model scores 7.2% at the post's settings, 22.3% at the card's thinking text settings and 44.0% at its non-thinking text settings).
   - Refusals specific to this gate: `card-row-missing`, `card-row-not-named-or-unquoted`, `card-row-quote-not-found-in-card`, `card-quote-states-no-sampling-setting`, `card-row-value-ambiguous-or-mistyped` (the quote gives two values for one setting, or a `card_row` field has the wrong type), `run-setting-missing-or-mistyped`, `run-settings-missing`.
   - What this gate does not check: that the saved card is the vendor's text (it trusts the file), that the quote is the line for the row named, or the mode the server actually applied (the harness writes `enable_thinking_applied: true` on every row, `probe.rs:301`; the mode compared is the one the config asked for).

### Step 2 — Route: blocker or capability gap

Call `lisp_eval` with the route gate (DME-1) over the measured counts — it derives the shares itself, never hand-assert them:
- form: `(cond ((= answers_total 0) (quote empty-baseline)) (t (let ((nonfinish_share (/ answers_hit_limit answers_total)) (wrong_complete_share (/ answers_wrong_complete answers_total))) (cond ((>= nonfinish_share 0.5) (quote blocker-removal-route)) ((>= wrong_complete_share 0.5) (quote capability-gap-outside-this-skill)) (t (quote collect-more-evidence))))))`
- env: `{ "answers_total": <count>, "answers_hit_limit": <count>, "answers_wrong_complete": <count> }`
- `blocker-removal-route` → continue to Step 3.
- `capability-gap-outside-this-skill` → stop; report the route verdict. In the anchor's one model pair, this skill's edit class removed an obstacle and did not add an ability; the Gemma control is that single case, with the confounds named in the introduction, not a general counter-example.
- `collect-more-evidence` or `empty-baseline` → enlarge the practice set, re-measure, re-run the gate. Enlarge at most twice; if the gate still returns either verdict, stop and report the route as undetermined, with no weight edit.

Sibling gate (DME-1S) — the skill-presence probe. When the route question is whether the model possesses the skill the failure suppresses — before any localization spend — probe task conformance on practice problems twice, zero-shot and few-shot, and gate the verdict with `lisp_eval`:
- form: `(cond ((= conformance_sample 0) (quote collect-more-evidence)) ((< few_shot_conformance 0.5) (quote capability-gap-outside-this-skill)) ((> (- few_shot_conformance zero_shot_conformance) 0.1) (quote skill-unlock-route)) (t (quote skill-present-not-suppressed)))`
- env: `{ "zero_shot_conformance": <share of conforming answers, zero-shot>, "few_shot_conformance": <share of conforming answers, few-shot>, "conformance_sample": <practice answers probed per arm> }`
- The probe ADJUDICATES AT THE CARD ROW the run config names (Step 1: the row for the mode the deployment will use) — never only at trap-style configs. Config-dependence is a recorded live finding, not a hypothesis: the anchor's own case reproduced the loop at the trap config (6 of 7 answers hit-limit) and did NOT survive the card config (0 of 10 hit-limit — at card settings DME-1 itself routes that same baseline to `capability-gap-outside-this-skill`); receipts at ~/Documents/zk-data/direct-model-edit-archive/logs/stage1-summary.json (the archived stage-1 instrument's historical record; the Rust replication re-measures). A presence probe run at trap-style configs measures the trap, not the skill. The probe's design — zero-shot vs few-shot conformance as a skill-presence test — is this skill's original instrument: the anchor ran no presence probe; the probe is grounded in the stage-1 receipts above and the anchor's published config facts, with its thresholds disclosed as Evidence-tier operationalizations in Constraints.
- `skill-unlock-route` → the skill is present but suppressed: continue to Step 3. `capability-gap-outside-this-skill` → stop, as DME-1's same verdict. `skill-present-not-suppressed` → the probed failure is not suppressing a skill: re-examine the diagnosis before spending localization. `collect-more-evidence` → enlarge the probe sample and re-run DME-1S, at most twice; after that, report skill presence as undetermined and do not spend localization.

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

3. Following the template's schema, isolate the candidate direction: the internal pattern that separates failing from progressing trajectories at named sites. The anchor's signature: a direction written by the MLPs of two layers (two vectors, absolute cosine 0.849), appearing before repetition.
4. Evidence bar (P, critiqued by the operator): present in failing trajectories, absent or late in progressing ones, written at specific named sites — not a wholesale layer. A direction that clears this bar is a candidate, not a cause: the contrast is correlational. The "immediately before the failure" timing criterion is this skill's own operationalization; that wording was not found in the extracted post text (the anchor's logs repository was not searched).

### Step 3b — Intervene: is the candidate direction specific and graded?

The anchor supported "targeted" by intervention: a strength series (1, 2, 3, 4, 6), layer-isolation tests, and a norm-matched random direction that gained 1.6 points. Two gates ask the same two questions here. Each arm is a probe run over the same problems, seeds, config and server build as the baseline, on a model file made by `dme-harness edit`.

Harness prerequisite, not yet met (see Constraints): the edit receipt must record the direction as `{source, dim, sha256, mean}`. Until it does, both gates return `direction-not-recorded-in-receipt` and the direction stays a candidate — report it as one.

1. Random-direction control (DME-2c). Make the targeted edit (direction from a file) and a control edit with the same layers, tensor, trigger and `alpha` and the direction `{"source": "seeded"}`. Probe both.

```
jq -n -c -L kask/crates/dme-harness/gates --slurpfile b <base.jsonl> --slurpfile bs <base.summary.json> --slurpfile t <targeted.jsonl> --slurpfile ts <targeted.summary.json> --slurpfile tr <targeted.receipts.json> --slurpfile c <control.jsonl> --slurpfile cs <control.summary.json> --slurpfile cr <control.receipts.json> 'include "dme-measure"; gate_dme2c($b; $bs[0]; $t; $ts[0]; $tr; $c; $cs[0]; $cr)'
```

   - `lisp_eval` with `dme-2c.lisp` returns `[verdict, notes, counts]`; `counts` holds `answers`, `targeted_net`, `control_net`, `targeted_over_control_net`, `targeted_truncation_drop`, `control_truncation_drop`.
   - `direction-specific` needs all five: the targeted net gain is established; the control's net change is under 0.25 of the targeted one (operator ruling 2026-11-11 — this skill's own threshold); the control's drop in truncated answers is under 0.25 of the targeted drop (compared only when the targeted's own drop is positive — a targeted gain that does not reduce truncation has no truncation channel to compare; its control's truncation churn is caught, if at all, by the control's own net change and the paired comparison); the targeted run beats the control run when the two are paired directly; and the control's own change is not established (operator ruling 2026-11-12: a random direction that moves the score by an established amount fails the specificity claim whatever the ratios, superseding the 2026-11-11 ratio-only reading).
   - `control-change-is-itself-established` → the control's own change is established — the random direction moved the score by more than noise (e.g. +99 answers of 1,000 against a targeted +400, a difference the 0.25 ratio does not catch): the specific-direction claim is not supported whatever the ratios. Report the control's counts beside the verdict and keep the direction a candidate. A control inside noise can still fail the ratio checks; a control that moves by an established amount fails here even when the ratios pass.
   - `targeted-gain-not-established`, `control-change-at-least-a-quarter-of-targeted`, `control-unblocks-at-least-a-quarter-as-much` (the random direction ends truncation about as well, whatever the answers), `specificity-not-established` → the direction remains a candidate; say which.
   - Refusals specific to this gate: `control-rows-missing`, `control-edit-not-comparable` (a different site, strength, sign or number of edits, a control that is not seeded, or a size that was not recorded), `control-not-norm-matched` (some layer's edit size, `edit_delta_l2`, differs by more than 1%), `control-direction-not-centred` (the seeded vector's components sum to more than 4 in absolute value; a centred random unit vector's sum is near 1), `direction-not-recorded-in-receipt`.
2. Strength series (DME-2d). Two to twelve edits of the same direction at the same site with strictly increasing `alpha`, each probed. Pass one `{rows, summary, receipts}` object per arm, weakest first:

```
jq -n -c -L kask/crates/dme-harness/gates --slurpfile b <base.jsonl> --slurpfile bs <base.summary.json> --slurpfile r1 <arm1.jsonl> --slurpfile s1 <arm1.summary.json> --slurpfile c1 <arm1.receipts.json> --slurpfile r2 <arm2.jsonl> --slurpfile s2 <arm2.summary.json> --slurpfile c2 <arm2.receipts.json> 'include "dme-measure"; gate_dme2d($b; $bs[0]; [{rows: $r1, summary: $s1[0], receipts: $c1}, {rows: $r2, summary: $s2[0], receipts: $c2}])'
```

   - `lisp_eval` with `dme-2d.lisp` returns `[verdict, steps, against_best, against_base]`. Each list holds 1, 0 or -1 per arm — an established gain, no established change, an established loss — against the arm before it (the first against the baseline), against the best of the baseline and all earlier arms, and against the baseline.
   - `dose-response-graded` → at least two steps are established gains, no arm is an established loss against its predecessor or against the best earlier point, and the strongest arm is an established gain over the baseline.
   - `dose-response-reverses` → some arm is an established loss against its predecessor or against the best earlier point (the archive's own series: 7, then 9, then 0 correct of 24; or a series that climbs and then slides back in steps too small to register one at a time).
   - `dose-response-not-graded` → anything else, including one jump followed by a plateau and a series that only recovers ground it lost.
   - Refusals specific to this gate: `strength-series-too-short`, `strength-series-too-long`, and `["series-arm", n, reason]` with `series-arm-malformed` (an arm whose receipts carry different strengths), `series-arms-not-comparable`, `strengths-not-strictly-increasing`, `edit-size-not-proportional-to-strength` or a shared refusal.
3. Layer isolation is not gated. Record whether it was run and what it showed.

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
3. Do not label the operation. The harness applies `W ← W − alpha · direction · kᵀ` with an absolute `alpha` (`edit.rs:13`); it has no relative strength, so none is gated. What the edit did is read after the fact from the edit receipts by DME-3 (Step 6): the sign of `alpha` times the trigger's recorded weight range gives `uniform-minus-direction`, `uniform-plus-direction` or `mixed-sign` (a dense trigger whose weights have both signs subtracts on some columns and adds on others). The source calls its edit a subtraction ("much like removing a behavior by deleting a single direction" — the source's own footnote for that phrase is Arditi et al., 'Refusal in Language Models Is Mediated by a Single Direction', arXiv:2406.11717, 2024; the wider published editing lineage the source situates itself in: Meng et al., 'Locating and Editing Factual Associations in GPT' (ROME), arXiv:2202.05262, 2022, and Elhage et al., 'A Mathematical Framework for Transformer Circuits', 2021), yet it was applied at strength 6 — the component along the direction reversed and amplified five-fold, per the post-publication record above — and the DME-2 cap counts entries addressed, not the size of the change. So every edit, whatever its class, needs a measured preservation baseline before it is applied: a probe run of the unedited model, under the same config and on the same model file as the target run, on problems disjoint from the target set. This requirement is this skill's discipline, not the source's. Run the preservation gate (DME-2b):

```
jq -n -c -L kask/crates/dme-harness/gates --slurpfile p <preservation.jsonl> --slurpfile ps <preservation.summary.json> --slurpfile t <target.jsonl> --slurpfile ts <target.summary.json> 'include "dme-measure"; gate_dme2b($p; $ps[0]; $t; $ts[0])'
```

   - `lisp_eval` with `dme-2b.lisp` returns `["preservation-baseline-measured", {answers, correct, detectable_loss_exceeds_answers}]` → proceed. The set must have at least 60 answers and at least 6 correct ones (and more than 1% correct): the sign test cannot establish even a pure loss below six changed answers, so a ten-point loss that is entirely a loss needs at least sixty answers, and offsetting gains push the requirement higher — at sixty answers, seven lost against one gained is a ten-point net loss the test cannot establish. The refusal symbol `preservation-set-too-small-to-show-a-10-point-loss` carries the exactly-a-tenth reading: a set under sixty answers cannot hold the six answers a ten-percent loss needs — the floor is a margin, not the detection limit (at fifty-nine answers a pure loss of six answers is 10.2 points and would be established; the floor refuses the set anyway). `detectable_loss_exceeds_answers` is the net loss, in answers, above which DME-3 can call a loss established on this set (the larger of 5 answers and 1.0 point); a smaller loss will read as `net-change-not-established`, so report the paired counts, not only the verdict.
   - After Step 5, run DME-3 with the preservation rows before and after the edit: `net-loss-established` there is collateral damage.
   - Refusals specific to this gate: `preservation-baseline-missing` (a plan in words is not a baseline), `preservation-and-target-runs-differ` (with the differing keys), `preservation-and-target-on-different-model-files`, `preservation-set-overlaps-target-set`, `preservation-baseline-cannot-show-a-loss`, `preservation-set-too-small-to-show-a-10-point-loss`. A refusal here is answered once: re-measure the preservation baseline — a larger set, one that can show a loss — and re-gate; a second refusal stops the run with no edit applied.

### Step 5 — Apply, re-measure, verify

1. Apply the edit with `dme-harness edit --spec <spec>` and keep every receipt (`<output>.edit-receipt.json`). A receipts file is the receipts that lead from the base file to that arm, concatenated in order (`cat first.edit-receipt.json second.edit-receipt.json > arm.receipts.json`); for one invocation it is the receipt itself. Probe the edited file on the SAME problems with the SAME config, cap and seed base, against the same server build.
2. There is no separate identity gate. Every gate that compares two runs (DME-2c, DME-2d, DME-3) first checks that nothing the harness recorded differs besides the model file — the embedded config, cap, context size, engine build, harness version, penalty window and the server's echoed generation settings — that the receipts lead from the base file's hash to the edited file's, that no receipt reports changed weights outside the edited surface, and that every pair of answers is the same problem with the same seed, gold answer and prompt length. `runs-differ-beyond-the-model-file` names the keys: a change rode along with the edit and the comparison verifies nothing ("a verification under different settings verifies nothing" — the verify template's own constraint); re-run before Step 6. Equal recorded facts do not prove equal conditions: what the harness does not record (GPU placement, threads, server flags) can still differ, and run-to-run noise at identical recorded facts has not been measured — the one archived pair of unedited runs that differ in engine build shows 10 rescues and 11 regressions of 84.
3. Call `render_template`:

```
template: direct-model-edit/verify
variables:
  model_name: <target>
  edit_summary: <layers, tensor, trigger kind, alpha and weights_touched, from the receipts>
  gate_results: { dme3: <DME-3 result on the target set>, dme3_preservation: <DME-3 result on the preservation set>, dme2c: <result or null>, dme2d: <result or null> }
  control_model_result: { control_name, control_nonfinish_rate, control_delta }
```

4. The template tabulates; it computes nothing. Every count in its table is copied from a gate result.
5. Run the sibling control: a non-failing model edited under the same recipe, then gate its verdict (DME-3b) with `lisp_eval`:
   - form: `(cond ((<= (abs control_delta) flat_band) (quote sibling-flat)) (t (quote sibling-moved-general-booster-suspect)))`
   - env: `{ "control_delta": <the sibling's before/after benchmark delta>, "flat_band": 1.0 }` — the band is Evidence-tier: the anchor's Gemma control stayed flat within about one point (6.8 → 7.0), a gain Astra itself reported as not established.
   - `sibling-moved-general-booster-suspect` → name the general-booster suspicion and the collateral risk out loud; the specificity claim is not established. `sibling-flat` → the control corroborates specificity.

### Step 6 — Attribute the gain

Run the attribution gate (DME-3) on the target set, and again on the preservation set:

```
jq -n -c -L kask/crates/dme-harness/gates --slurpfile b <base.jsonl> --slurpfile bs <base.summary.json> --slurpfile e <edited.jsonl> --slurpfile es <edited.summary.json> --slurpfile r <receipts.json> 'include "dme-measure"; gate_dme3($b; $bs[0]; $e; $es[0]; $r)'
```

- `lisp_eval` with `dme-3.lisp` returns `[verdict, notes, counts, operation]`. `counts` holds `answers`, `truncated_to_correct`, `finished_wrong_to_correct`, `correct_to_truncated`, `correct_to_finished_wrong`, `truncated_to_finished_wrong`, `truncated_before`, `truncated_after`. `operation` is the class read from the receipts (Step 4): `uniform-minus-direction`, `uniform-plus-direction` or `mixed-sign`. Report it; an edit described as a removal that comes back `uniform-plus-direction` or `mixed-sign` was not one.
- The gate pairs every problem before and after. Rescues are answers that became correct; regressions are answers that stopped being correct; the net change is rescues minus regressions. It asks three questions, each "is a change established?": over all answers (net), over answers that finished before the edit (wrong-to-correct against correct-to-wrong), and over answers gained from truncation against answers lost to it.
- `net-change-not-established` → there is no gain to attribute; say exactly that (one rescue of 96 answers; ten rescues against eleven regressions; twenty rescues of 2,000).
- `net-loss-established` → the edit made things worse.
- `mostly-unblocking` → the net gain and the truncation gain are established, and the net change among answers that already finished is within 1.0 point of all answers: the headline is a completion fix, not a capability gain (the anchor: 576 of its 579 rescues were originally truncated outputs; the authors wrote that this "is not the same as making a model better at math").
- `unblocking-with-unresolved-change-on-finished-answers` → the truncation gain is established, and finished answers moved by more than 1.0 point without that change being established either way. Do not call this "mostly unblocking": report the finished-answer counts and collect more answers.
- `unblocking-with-loss-on-finished-answers` → the truncation gain is established, with an established loss among answers that used to finish correctly. Report both.
- `gain-includes-finished-answers` → answers the model already finished got better too; scrutinize before claiming capability.
- `net-gain-source-unresolved` → a net gain is established but the truncation gain is not, on its own.
- `notes` may hold `finished-answer-gain-untestable` or `finished-answer-loss-untestable` (the baseline has too few finished-wrong, or finished-correct, answers for a change there to be detectable — with either note, `mostly-unblocking` says nothing about finished answers) , `correct-answers-newly-truncated` (more than 1.0 point of answers that were correct now run to the limit) and `finished-answers-changed-both-ways` (more than 1.0 point of finished answers were gained and more than 1.0 point lost: the edit, or run-to-run noise, is moving finished answers even where the net is small). Report the notes with the label.
- Refusals specific to this gate: `edit-strength-not-a-nonzero-number`, `edit-trigger-weights-not-recorded`, `too-many-edit-receipts` (more than 24).
- This paired accounting is this skill's operationalization of the source's rescue counts; the source reports the counts and performs no test.

### Step 7 — Converge

After each verify pass, count what the oracle still reports open — remaining hit-limit share, regressed benchmarks, un-reproduced claims. Record the count and check convergence with `lisp_eval` (DME-4) over the recorded counts, oldest first:
- form: `(and (>= (length xs) 3) (= (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs)) (= (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs)))`
- env: `{ "xs": [<open-findings counts, oldest first>] }`
- Converged when the last three counts are equal (two counts can never satisfy the form). Maximum 5 iterations, then the remaining instability is reported, not iterated. Non-smooth gains are expected: the anchor's jump landed at the 50-forward-pass budget, barely visible at 10 or 25.

### Step 8 — Report

The report carries: the DME-0 result with its silent settings and notes, the DME-1 route verdict, baseline and post-edit answer classes, the DME-2c and DME-2d results with their notes (or the statement that the direction is an untested candidate), the DME-2b result, the DME-3 result — verdict, notes, paired counts and operation class — on the target set and on the preservation set, the sibling control, the edit specs and receipts, every gate env file as `jq` wrote it, every forward-pass budget point tried, seeds, and logs. Logs or it did not happen — the anchor released its logs, though not exhaustively: the logs' own README lists omissions, and the independent audit found earlier trial runs of earlier prompt versions left out, their number undisclosed. If any MCP tool call fails along the way, call `curator_report_skill_use_issue` with `skill_name: "direct-model-edit"`, the tool name, and the error, then continue with the best available information.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `baseline.j2` | Ground a direct weight-edit task: read the baseline answer classes, choose the vendor-card row the run follows and quote it verbatim, before any weight is touched. |
| `localize.j2` | Localize a diagnosed failure mode to weights: contrast internal activations across progressing and failing trajectories to isolate a candidate direction, its layers and columns, and plan the intervention runs that test it. |
| `edit-plan.j2` | Plan the minimal weight edit: choose the smallest parameter surface for the candidate direction and name the preservation set whose baseline is measured before the edit, under an explicit budget of edited weights and forward passes. |
| `verify.j2` | Tabulate the verification of an applied weight edit from the gate results: answer-class changes, the attribution verdict and operation class, the preservation verdict and the sibling control. |

To render a template, call the `render_template` tool with the ref (e.g. `direct-model-edit/baseline`) and a context object with the required variables.

Template context variables (from each template's `[inference]` contract):
- `baseline.j2`: `model_name`, `model_card_notes`, `run_config`, `baseline_counts`, `benchmark_name`
- `localize.j2`: `model_name`, `failure_mode`, `trajectory_contrast`, `localization_notes`
- `edit-plan.j2`: `model_name`, `selected_site`, `total_weights`, `edited_weights`, `forward_pass_budget`
- `verify.j2`: `model_name`, `edit_summary`, `gate_results`, `control_model_result`

## Constraints

- The harness language boundary is absolute (operator ruling 2026-11-06, extending the 2026-10-13 zero-Python rule beyond the repo tree): the harness is Rust + bash — no Python anywhere an agent authors — not in the repo, not in a replication workspace under the data dirs, not "temporarily". Ecosystem convenience is not an exception path: if the target architecture or capture machinery is unavailable in Rust on this machine (candle/burn architecture support, the ROCm backend), that is a surfaced capability gap routed to the operator under this skill's own routing discipline — never a silent language substitution. A Python harness is this skill's refusal state, not its Step 1.
- Measure before you diagnose: the anchor's "math bug" was 93% non-terminating answers (the reasoning-loop phenomenon the anchor itself cites: Pipis et al., 'Wait, Wait, Wait... Why Do Reasoning Models Loop?', arXiv:2512.12895, 2025); completed answers were already right almost every time.
- The settings gate precedes every weight read. The run config names the vendor-card row it follows and quotes the card's line for it; DME-0 finds that quote in the saved card, reads the six sampling values from it, and compares them with what the server echoed for the run. The anchor ran in the exact mode the card warns about, at a temperature-and-penalty combination associated with looping.
- The measured gates take their inputs from files the harness wrote, through `dme-measure.jq`. No count, ratio or flag is typed into a gate, and each env file is kept with the report. The gate files, the measuring module and their declared cases live in `kask/crates/dme-harness/gates/` and are checked together by `cargo test -p hkask-lisp --test dme_gates`, which needs `jq` on the path.
- Harness gaps, each surfaced to the operator and never a reason to hand-fill a gate: (1) the edit receipt does not record the direction (`kask/crates/dme-harness/src/edit.rs:113`, built at `edit.rs:466`) — DME-2c and DME-2d need `direction: {source, dim, sha256, mean}` there; (2) the seeded direction's components are never negative (`edit.rs:290-304`), so it is not a centred random control; (3) the harness has no mode that runs `dme-measure.jq`'s measurements and a gate together, so a gate env passes through the model on its way to `lisp_eval` (`crates/agent/src/tools/lisp_eval_tool.rs` takes `env` as an argument and has no file input); (4) `enable_thinking_applied` is written as `true` on every row (`probe.rs:301`), so the applied mode is not recorded. Until (1) and (2) change, DME-2c and DME-2d refuse and a direction stays a candidate.
- This skill's edit class removes obstacles; in the anchor's one model pair it did not install a capability — the Gemma control is one confounded case (a different model, about a third the size, non-thinking, 8,192-token limit, lenient grading), not a general law. Constructive edits are outside this skill, not shown impossible. Route capability gaps out.
- Localize by contrast (failing vs progressing trajectories), never by intuition. Contrast makes a direction a candidate; only intervention — a norm-matched random-direction control (DME-2c) and a strength series (DME-2d) — supports calling it specific.
- Cap the edit surface at 0.1% of total weights (DME-2); the anchor's 0.017% of the language model is the reference point, not the limit. The harness enforces this cap mechanically — the pre-emit gate refuses over-cap manifests by name and the edit leg hard-refuses on the parsed GGUF's real param_count (commit 376b4025); the lisp_eval form stays as the decision-time discipline.
- Provenance at commissioning (operator ruling 2026-11-07, the corrective phase's ground rule): every edit geometry, trigger design, localization method, and measurement construct named in a commissioning carries its provenance line — the literature anchor resolved through the research server with recorded provenance, or the explicit novel/exploratory label. Novel is legitimate when explicit; unanchored-by-omission is the drift. A commissioning without provenance lines is malformed: challenge it at intake, before any leg runs.
- Report the whole budget curve, not just the best point — gains can be non-smooth across forward-pass budgets.
- Every headline change carries its DME-3 result — verdict, notes, paired counts and operation class; no edit is applied without a measured preservation baseline (DME-2b), and the preservation set gets its own DME-3 result after the edit.
- The sibling control is not optional: a report without it is an unverified claim of specificity.
- Logs or it did not happen: the full exchange, the edit diff, the budgets tried, the seeds.
- The gate apparatus — DME-0's quoted-row comparison, DME-1S's probe design, DME-2's cap, DME-2b's preservation baseline, DME-2c's control comparison, DME-2d's series reading, DME-3's paired attribution, DME-3b's band, DME-4's convergence rule, and their constants (the 0.1% cap, the 0.5 routing thresholds, DME-1S's 0.5/0.1 presence thresholds, the 1e-6 settings tolerance, the sign test at 0.05 with its 1.0-point floor, the 1.0-point band on finished answers, the 0.25 control ratios, the 1% edit-size match, the centring bound of 4, the 60-answer preservation floor, DME-3b's flat band, the convergence bound) — is this skill's Evidence-tier operationalization of the anchor's single published case: original generalization machinery the source does not contain, labeled as such — operationalizations, not prohibitions. The inline forms are parse-pinned by `check_skill_lisp_forms`; the measured gates and the measuring module are case-pinned together by `cargo test -p hkask-lisp --test dme_gates`, which also checks the forms' floating-point sign test against exact integer arithmetic for every pair of counts up to 100 changed answers. The machine-checked Lean treatment is `kask/lean/dme_gates.lean` (Lean 4 v4.34.0, core only), which proves the gates' decision rules as modeled there (the pin's header states the modeling boundary) — DME-0: the moved list names exactly the settings the card side states and the run differs on, in order, a setting the card does not state is never reported as moved, and a full match requires every sampling setting stated and none moved; DME-2's cap in exact Nat arithmetic: the anchor instance passes at the measured 752,393,024 weights, shrinking the surface preserves passing, uniform scaling preserves the verdict; DME-3: a net change within 1.0 point is never established, the verdict is `mostly-unblocking` iff the net gain and the truncation gain are established, the finished-answer change is not established and lies within the band, and the anchor's published counts satisfy it; DME-4's index arithmetic decides the last-three-equal spec exactly — and `kask/lean/dme_gates_negative_control.lean`, stated over the pin's own definitions, must fail to check.
- The minimal-surface/maximal-behavior shape of a good edit is Ousterhout-derived (the same lens as the `deep-module` skill, applied to a weight patch instead of a code module).

## Regression case

Inline forms are parse-verified by `check_skill_lisp_forms`; their semantics are executed at use time with `lisp_eval`. The measured gates are pinned by file: `kask/crates/dme-harness/gates/dme-gate-cases.gen.jq` builds every declared case with `dme-measure.jq` itself (more than 200 cases; the archive-derived ones from `dme-gate-fixture.json`), and `cargo test -p hkask-lisp --test dme_gates` evaluates them. Expected values:

- DME-0 (case A2): the anchor's run against the card's "Non-thinking, VL tasks" line → `["settings-differ-from-card-row", "Non-thinking, VL tasks", ["enable_thinking", "temperature", "top_p"], ["penalty_last_n"], ["mode-is-not-the-card-default"]]`. Case A5: equal except `repetition_penalty` 1.1 → moved `["repetition_penalty"]`. Case A1: the server's echo 0.699999988079071 against the card's 0.7 → `settings-match-card-row`. Case A22: a `card_row` that quotes a note describing the run itself → `card-quote-states-no-sampling-setting`. Case A33: counts that are fractional but still sum to the answer total → `measurement-inconsistent` (the integrality check is what catches them).
- DME-1, the anchor's baseline: `{ "answers_total": 96, "answers_hit_limit": 85, "answers_wrong_complete": 4 }` → 85/96 ≈ 0.886 ≥ 0.5 → `blocker-removal-route`. A Gemma-shaped baseline: `{ "answers_total": 100, "answers_hit_limit": 8, "answers_wrong_complete": 60 }` → 0.08 < 0.5, 0.6 ≥ 0.5 → `capability-gap-outside-this-skill`.
- DME-1S, presence probe: `{ "zero_shot_conformance": 0.08, "few_shot_conformance": 0.75, "conformance_sample": 96 }` → `skill-unlock-route`; `{ "zero_shot_conformance": 0.10, "few_shot_conformance": 0.15, "conformance_sample": 96 }` → `capability-gap-outside-this-skill`; `{ "zero_shot_conformance": 0.55, "few_shot_conformance": 0.60, "conformance_sample": 96 }` → `skill-present-not-suppressed`; `{ "conformance_sample": 0 }` → `collect-more-evidence`.
- DME-2, the anchor's surface: `{ "total_weights": 752393024, "edited_weights": 131072 }` → budget = 752,393.024; 131,072 ≤ 752,393.024 → `[true, 752393.024]` (0.0174% of the measured Qwen3.5-0.8B BF16 param count passes the 0.1% cap).
- DME-2b (case C19): 96 preservation answers, 60 correct → `["preservation-baseline-measured", {"answers": 96, "correct": 60, "detectable_loss_exceeds_answers": 5}]`. Case C6b: 24 answers → `preservation-set-too-small-to-show-a-10-point-loss`. Case C2: a plan in words and no rows → `preservation-baseline-missing`.
- DME-2c (case D4): 256 answers, targeted +102, control +4 → `["direction-specific", [], {"answers": 256, "targeted_net": 102, "control_net": 4, "targeted_over_control_net": 98, "targeted_truncation_drop": 102, "control_truncation_drop": 4}]`. Case D3: 1,000 answers, targeted +402, control +350 → `control-change-at-least-a-quarter-of-targeted`. Case D18: 1,000 answers, targeted +400, control +99 (established) → `["control-change-is-itself-established", [], {"answers": 1000, "targeted_net": 400, "control_net": 99, "targeted_over_control_net": 301, "targeted_truncation_drop": 400, "control_truncation_drop": 99}]` (operator ruling 2026-11-12). Case D24: control +6 of 96 → `control-change-is-itself-established`; case D25: control +5 of 96 → `direction-specific` (the six-changed-answer floor of the sign test). Case D23: a targeted gain with no truncation drop beside a control that reclassifies 100 truncations → `direction-specific` (the truncation channel is compared only when the targeted has a drop). Case D19: a control that ends truncation as often as the targeted edit but gets the answers wrong → `control-unblocks-at-least-a-quarter-as-much`. Case G4: receipts as the harness writes them today → `direction-not-recorded-in-receipt`.
- DME-2d (case E2): 256 answers at strengths 1, 2, 3 with +32, +63, +102 → `["dose-response-graded", [1, 1, 1], [1, 1, 1], [1, 1, 1]]`. Case E4: 7, then 9, then 0 correct of 24 → `["dose-response-reverses", [0, -1], [0, -1], [0, -1]]`. Case E15: every arm below the baseline → `dose-response-reverses`.
- DME-3 (case F5), the anchor's counts: 1,152 answers, 579 rescues (576 from truncated answers, 3 from finished-wrong ones), 5 regressions → `["mostly-unblocking", [], {"answers": 1152, "truncated_to_correct": 576, "finished_wrong_to_correct": 3, "correct_to_truncated": 0, "correct_to_finished_wrong": 5, "truncated_to_finished_wrong": 0, "truncated_before": 1000, "truncated_after": 424}, "uniform-minus-direction"]` (the sample size and baseline split are constructed to fit the post's published counts). Case F1: one rescue of 96 → `net-change-not-established`. Case F2, two archived unedited runs: ten rescues, eleven regressions → `net-change-not-established` with the note `finished-answers-changed-both-ways`. Case F24: 6 rescues from truncation, 60 gained and 42 lost among finished answers, of 500 → `unblocking-with-unresolved-change-on-finished-answers`. Case C1: `alpha` −6 → operation `uniform-plus-direction`. Case F34: 25 edit receipts → `too-many-edit-receipts`. Cases F35/F36: 100 changed answers → `net-change-not-established` (the exact binomial branch: a 20-answer net of 200 is not established) and 101 changed → established (the chi-square branch). Case B1b: the full archived pair, older rows and all → `["base-run","rows-disagree-with-harness-summary"]` — the older archived rows predate the recorded facts the gates read, so the pair is inadmissible before the runs-differ check is reached.
- DME-3b, sibling verdict (band 1.0, Evidence-tier): `{ "control_delta": 0.2, "flat_band": 1.0 }` → `sibling-flat` (the anchor's recorded 6.8 → 7.0 control); `{ "control_delta": 5.0, "flat_band": 1.0 }` → `sibling-moved-general-booster-suspect`.
- DME-4, converged: `{ "xs": [61, 12, 12, 12] }` → `true` (last three equal); unstable: `{ "xs": [61, 12, 12, 4] }` → `false`; too few counts: `{ "xs": [12, 12] }` → `false`.
