---
name: adapter-lifecycle
description: "Run the full fine-tuning loop for an agent or skill: measure current performance with the rollout harness, bridge verdict-labeled rollouts into training datasets, validate the LoRA config against the math-contract gates, submit and track the training job, evaluate the adapter against the baseline, and retrain from curated feedback. Reifies verifier-gated Foundation Model improvement over the training and swarm MCP servers."
---

# Adapter Lifecycle

Improve an agent's or skill's model behavior end to end: measure, build
a dataset from real verdict-labeled rollouts, train under the
math-contract gates, evaluate against the baseline, and iterate. The
loop is verifier-gated: a first adapter needs a deployed, matched
held-out comparison against its baseline and the operator's pre-agreed
acceptance criterion; a retrain can also report previous-adapter loss.

## When to Use

- An agent's task pass rate is measurably poor and the operator wants a
  fine-tuned adapter rather than a prompt fix.
- Verdict-labeled rollouts have accumulated in the swarm event store
  and should become training data.
- A trained adapter needs evaluation against its baseline, or a
  retrain from curated feedback.

## When NOT to Use

- The problem is a prompt, skill-body, or tool-schema issue — fix that
  first (cheaper, faster, no GPU).
- No deterministic evaluator exists for the tasks — the loop requires
  validated contains/not_contains/regex response checks to stamp verdicts.
  These scores are not independent ground truth; use a separately controlled
  acceptance protocol. Shell/file evaluators are not supported.
- The operator has not accepted a PEFT configuration — the
  `lora-training` skill's gate output is the accepted config; do not
  substitute your own.

## Instructions

### Phase 1 — Measure (the rollout harness)

Model policy: “local swarm” means execution on the hKask substrate, not a local
or smaller LLM. Inherit the platform/curator defaults from Settings → Kask →
Models through the host inference bridge. Do not substitute an unapproved
local/cheaper model to avoid provider cost; if approved routing is unavailable,
surface that blocker. Only an explicit operator choice may override the model.

1. Define the task set: 3-10 representative tasks, each with a
   deterministic response evaluator (contains / not_contains / regex)
   and an explicit experiment scope (tasks/repeats and any approved run
   deadline). Agree with the operator *before training* on the held-out
   acceptance criterion and evaluation method for comparing the baseline
   and candidate on identical inputs; record what outcome counts as
   acceptance rather than inventing a pass-rate threshold afterward.
   Token usage is observed evidence, not a quota. The local harness has
   no credits_authorized or token-budget parameter.
2. Call `swarm_eval_agent_local` (swarm server) with the agent name,
   the task set, and repeats (2-3 for a first measurement). Read the
   per-task pass rates and standard error. This is the BASELINE —
   record it (it is also recorded as model_request + verdict events in
   the event store). Register the acceptance claim in the same step:
   `kanban_goal_create` with the baseline and target pass rates as
   observable criteria and your intake prediction, linking this task
   to the goal via `advances`.

### Phase 2 — Build the dataset

3. Call `training_bridge_rollouts` (training server) with an `output_path`
   and `agent_name`, selecting mode `sft` for passed-rollout ChatML JSONL
   or `preference` for DPO JSONL (`prompt`, `chosen`, `rejected`) when
   passed and failed rollouts from the same harness task have retained
   bodies. `both` emits separate files; choose the dataset compatible
   with the operator-accepted trainer (do not pass `dpo` as a bridge mode).
   Read `sft_path`/`sft_examples` or `preference_path`/`preference_examples`;
   stop if the selected output is empty. Check `skipped_no_bodies` —
   stripped captures cannot be bridged; note the count.
4. Call `training_ingest_dataset` with the selected emitted dataset path
   to normalize and cache it. Read the format detection and sample count.

### Phase 3 — Validate and submit (the gates)

5. Call `training_validate_config` with the base model, the dataset
   path, and the operator-accepted PEFT params (from the lora-training
   skill's G6 gate). Read EVERY finding — refuse/warn severities must
   be resolved or explicitly accepted by the operator before
   submission. The G-D0 profile (format, sample count, token
   estimates) must match the dataset you built.
6. Call `training_submit` with the dataset path, base model, validated
   params, and confirmed: true (only after the operator confirms the
   spend). Record the job id.

### Phase 4 — Track and evaluate

7. Poll `training_status` with the job id until completion. It reports
   pod status, GPU, recent logs, and — on completion — registers the
   adapter from the HuggingFace manifest. `ab_comparison` is available
   only for a retrain with a previous adapter for the same skill and
   both losses present; it compares previous vs new training loss, NOT
   base-model vs candidate loss. First-time adapters have no such loss
   comparison: do not synthesize one or block their held-out verdict on it.
   On retrains, record any `loss_improved` as separate evidence; the
   `auto_promoted` field is not evidence of the held-out evaluation gate.
8. Prepare held-out tasks and expected answers that neither model trained
   on. For a pass-rate comparison, run `training_evaluate` twice on the
   SAME `test_dataset_path`, `method`, `max_examples`, and (if semantic)
   `judge_model`: once with `model` set to the deployed baseline model,
   once with `model` set to the deployed candidate adapter's model name.
   Pass the corresponding `adapter_id` to label each report; this field
   does NOT route inference — `model` does. Use exact_match / contains /
   semantic / benchmark only with the matching ChatML or benchmark dataset;
   semantic is LLM-judged and requires an explicit judge_model. Record
   both `accuracy` values and per-example errors; use the Phase 1 harness
   pass rate as diagnostic context, not as the denominator for this
   different evaluator. Verify both model routes actually serve the intended
   weights (returned model strings are reported, not attested). If the
   candidate is not deployed or identity cannot be verified, do not treat
   an evaluation of the base route as candidate evidence: no promotion.

### Phase 5 — Verdict and retrain

9. Convergence gate — for a FIRST adapter, require verified candidate
   deployment and both matched Phase 4 evaluations. Call `lisp_eval` with
   the operator-approved acceptance predicate over the *measured* baseline
   and candidate results; include deployment in the predicate. For example,
   ONLY if the operator agreed that the candidate must strictly beat the
   baseline on accuracy:
   - form: `(and (= deployed 1) (> pass_rate baseline_pass_rate))`
   - env: `{ "deployed": <1 only if the candidate route serves the adapter>,
            "pass_rate": <candidate Phase 4 accuracy>,
            "baseline_pass_rate": <baseline Phase 4 accuracy> }`
   If the approved criterion differs, encode that exact criterion instead;
   do not use the example as a default policy.
   **Noise floor (always, in addition to the operator's criterion).** A
   difference inside sampling noise is not evidence of improvement,
   whatever margin the operator chose. From each `training_evaluate`
   report's `correct` and example count, compute with `lisp_eval`:
   - form: `(let ((pc (/ kc nc)) (pb (/ kb nb))) (let ((se (sqrt (+ (/ (* pc (- 1 pc)) nc) (/ (* pb (- 1 pb)) nb))))) (cond ((or (< nc 10) (< nb 10)) (list "undetermined" "fewer than 10 held-out examples")) ((= se 0) (list (if (> pc pb) "beyond_noise" "within_noise") 0)) (t (list (if (> (- pc pb) (* 2 se)) "beyond_noise" "within_noise") (- pc pb) (* 2 se))))))`
   - env: `{ "kc": <candidate correct>, "nc": <candidate examples>, "kb": <baseline correct>, "nb": <baseline examples> }`
   Accept only when the operator's criterion holds AND the result is
   `beyond_noise` (the gain exceeds twice the combined standard error of
   the two proportions). `within_noise` is reported as no demonstrated
   improvement, with the difference and the noise band; `undetermined`
   (fewer than 10 examples per side) blocks acceptance and asks for a
   larger held-out set. Example: 42/50 vs 36/50 is a 12-point gain inside
   a 16-point noise band — not demonstrated. The practical margin — how
   large a real gain justifies deployment — remains the operator's. A first adapter can receive
   an acceptance verdict with NO `ab_comparison`: its loss is not an input.
   For a RETRAIN, apply the same deployed, matched held-out gate against
   the agreed baseline (base model or prior deployed adapter, fixed before
   evaluation). Record `ab_comparison` only if present. If the operator's
   pre-agreed retrain criterion also requires improved loss vs the prior
   adapter, include its evidenced `loss_improved` in `lisp_eval`; if that
   comparison is absent, mark that criterion unresolved, not false or
   satisfied. Counterexamples to an unconditional loss gate: a first
   adapter with no `ab_comparison` but verified deployment and a passing
   operator-approved held-out comparison can be accepted; a retrain with
   `loss_improved: true` and `auto_promoted: true` but a failed matched
   held-out criterion cannot. Never treat loss improvement or
   `auto_promoted` alone as a pass-rate verdict. Missing deployment,
   route verification, matched evaluation, or an approved criterion means
   no acceptance/promotion;
   report the gap rather than fabricate inputs or compare Phase 1 rates.
   If a measured gate fails, diagnose the failures, curate the exchanges
   into a feedback file, and re-enter Phase 3 with `training_submit`
   passing feedback_path and skill_name (retrain mode merges feedback,
   deduplicates by question, and increments the adapter version). Obtain
   operator confirmation for each new submission. Bound: max 2 retrain
   cycles per adapter version; a third failure escalates to the operator.
10. File the measurements for review — the session that trained the
    adapter does not record its acceptance (operator ruling 2026-09-24:
    evaluation is separated from execution). Write
    { adapter_id, baseline_pass_rate, pass_rate, evaluation_method,
    model_routes, evidence_gaps, harness logs } via `terminal` to
    `~/Documents/zk-data/curator/proposals/{agent-or-skill}/{date}-adapter-{adapter_id}.json`.
    Use null pass rates when unmeasured. The operator accepts or rejects
    the adapter in `algedonic-review`'s gemba walk; promotion follows only
    an accepted proposal.
11. Judge the registered goal — call `kanban_goal_judge` against the
    goal's criteria with a verdict and per-criterion results from the
    measured pass rates; mark unmeasured criteria as unresolved rather
    than guessing. When the operator confirms the outcome,
    `kanban_goal_score` Brier-scores the intake prediction — the
    scored acceptance record for this adapter change.

## Constraints

- Never submit with unresolved refuse-severity gate findings.
- Do not record acceptance or recommend promotion without verified
  deployment, matched held-out baseline/candidate evaluation, and the
  operator's pre-agreed criterion being met. Do not require retrain-only
  loss comparison for a first adapter or treat it as sufficient for a
  retrain. The skill files measurements as a proposal; it does not record
  a verdict, deploy, or promote.
- Rollouts and training jobs consume provider resources; do not invent a
  spend quota or promote on the basis of an unverified cost estimate.
  Present the available estimate and obtain operator confirmation before
  each Phase 3 submission.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "adapter-lifecycle", the tool name, and the error;
  continue with the best available information.
