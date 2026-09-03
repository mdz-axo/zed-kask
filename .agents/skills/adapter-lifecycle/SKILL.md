---
name: adapter-lifecycle
description: "Run the full fine-tuning loop for an agent or skill: measure current performance with the rollout harness, bridge verdict-labeled rollouts into training datasets, validate the LoRA config against the math-contract gates, submit and track the training job, evaluate the adapter against the baseline, and retrain from curated feedback. Reifies verifier-gated Foundation Model improvement over the training and swarm MCP servers."
---

# Adapter Lifecycle

Improve an agent's or skill's model behavior end to end: measure, build
a dataset from real verdict-labeled rollouts, train under the
math-contract gates, evaluate against the baseline, and iterate. The
loop is verifier-gated — every stage produces a deterministic verdict,
and no adapter ships without beating its baseline.

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
  contains/regex/exit_code/file_exists-style checks to stamp verdicts.
- The operator has not accepted a PEFT configuration — the
  `lora-training` skill's gate output is the accepted config; do not
  substitute your own.

## Instructions

### Phase 1 — Measure (the rollout harness)

1. Define the task set: 3-10 representative tasks, each with a
   deterministic evaluator (contains / not_contains / regex /
   exit_code / file_exists) and a credits_authorized budget.
2. Call `swarm_eval_agent_local` (swarm server) with the agent name,
   the task set, and repeats (2-3 for a first measurement). Read the
   per-task pass rates and standard error. This is the BASELINE —
   record it (it is also recorded as model_request + verdict events in
   the event store).

### Phase 2 — Build the dataset

3. Call `training_bridge_rollouts` (training server) with
   mode "sft" (or "dpo" when both passed and failed rollouts exist for
   the same tasks — preference pairs are the stronger signal). It
   emits ChatML JSONL from the retained request/response bodies.
   Check `skipped_no_bodies` — rollouts whose bodies were stripped
   cannot be bridged; note the count.
4. Call `training_ingest_dataset` with the emitted dataset path to
   normalize and cache it. Read the format detection and sample count.

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
   adapter from the HuggingFace manifest. Read the A/B comparison it
   emits (train vs baseline loss).
8. Call `training_evaluate` with the adapter id, a held-out test
   dataset, and the method matching your evaluator semantics
   (exact_match / contains / semantic / benchmark). Note: evaluation
   routes through the named model — the adapter must be deployed for
   the evaluation to measure the adapter, not the base model. If it
   is not deployed, say so and treat the A/B loss as the only signal.

### Phase 5 — Verdict and retrain

9. Convergence gate — call `lisp_eval` with:
   - form: `(and (eq loss_improved 1) (>= pass_rate baseline_pass_rate))`
   - env: `{ "loss_improved": <1 if the A/B loss improved>,
            "pass_rate": <Phase 4 eval pass rate>,
            "baseline_pass_rate": <Phase 1 baseline> }`
   If false, do NOT promote the adapter. Diagnose: re-run
   `swarm_eval_agent_local` on the failure cases, curate the failing
   exchanges into a feedback file, and re-enter Phase 3 with
   `training_submit` passing feedback_path (retrain mode merges the
   feedback, deduplicates by question, and increments the adapter
   version).
10. Persist the verdict — call `memory_insert` (curator server) with
    entity = the agent/skill name, attribute = "adapter_verdict",
    value = { adapter_id, baseline_pass_rate, pass_rate, promoted },
    and the evidence h_mem id from the eval. The training server does
    not persist A/B verdicts itself — this step is what closes the
    loop.

## Constraints

- Never submit with unresolved refuse-severity gate findings.
- Never promote an adapter that did not beat its baseline on BOTH the
  loss and the eval pass rate.
- Budgets are real: each rollout and training job spends credits/GPU —
  the operator confirms before Phase 3 submission.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "adapter-lifecycle", the tool name, and the error;
  continue with the best available information.
