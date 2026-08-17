# Classifier model selection — working criteria

**OUTCOME (2026-08-17):** operator selected `z-ai/glm-5.2`. Default switched in
`kask/crates/hkask-inference/src/model_constants.rs:23` to
`OpenRouter/z-ai/glm-5.2`. Rationale: GLM-5.2 is the accuracy leader on the real
label-space eval (39/47), the operator knows the GLM family, GLM 5.3 is expected
to drop price (~75–80%, operator-provided, revisit trigger not current fact), and
throughput can be optimized over time. See `kask/docs/review/eval-results-2026-08-17.tsv`.

---

(Draft criteria retained below for the record.)

## Goal
Pick the classifier model for hKask (resolves `model: ""` → `HKASK_CLASSIFIER_MODEL`
→ `DEFAULT_CLASSIFIER_MODEL`, `kask/crates/hkask-inference/src/model_constants.rs:23`).
Method: 4 screening gates → equal-weight average rank over latency / token-speed /
accuracy-on-real-label-spaces → price as reporting/tie-break. Operator cost ceiling
applies (largest observed delta ~+1041% GLM-5.2 is unacceptable).

## Screening gates (catalog)

- **(a) Temperature** — model accepts `temperature` (catalog `supported_parameters`).
- **(b) Structured output** — model supports `structured_outputs`.
- **(c) Non-thinking callable (CORRECTED)** — passes if EITHER:
  - non-reasoning (no thinking mode → nothing to disable), OR
  - reasoning-capable AND accepts `reasoning.enabled=false` AND has a non-thinking
    mode (endpoint does not return "reasoning is mandatory").
  - Live-checked via `kask/scripts/gate-c-check.sh`, NOT inferred from catalog.
- **(d) Updated within 180 days (CHANGED from "created")** — uses the model's last
  revision date, NOT original creation date. Threshold: 180 days before today.
  Source: the `created` timestamp of the canonical/snapshot id corresponds to its
  revision publish date (the `canonical_slug` embeds it, e.g. `-20260423`).

## Hard filters (operator-directed)

- **Latest version only.** Per model family, use the most recent revision (e.g.
  DeepSeek V4 Flash → latest snapshot, DeepSeek V4 Pro → latest snapshot), not stale
  dated snapshots and not superseded versions.
- **No anchoring on operator-named models** for the *derivation* of the pool — the
  candidate set is derived from screening. (Operator-named models are still screened
  through the same gates; they don't bypass them.)
- **NOTE:** the >50B-parameter and catalog-disclosure filters were REMOVED by the
  operator on 2026-08-17. Parameter count is no longer a screen criterion.

## Candidate models the operator has named / asked to consider
- DeepSeek V4 Flash (latest version)
- DeepSeek V4 Pro (latest version)
- DeepSeek R1 (most recent) — "supposed to be a strong classifier"

## Open conflicts the operator needs to resolve
1. **DeepSeek R1** — fails gate (c) (`reasoning.mandatory = true`; no non-thinking
   mode) and gate (d) (latest revision `deepseek-r1-0528` = 2025-05-28, >180 days).
   Per the screen as now defined, R1 is OUT. (Operator may still name it explicitly to
   run it outside the screen.)
2. **DeepSeek V4 Pro (0813)** — gates a/b/d PASS; gate (c) live-PASS. (The
   parameter-count disclosure concern is moot now that the param filter is removed.)

## What does NOT happen until this is CONFIRMED
- No eval runs.
- No gate-c live checks beyond what's already done.
- The benchmark scripts are fixed and ready (`check-classifier-models.sh`,
  `run-classifier-eval.sh`) but are not executed against OpenRouter.

## Status of supporting work (already done, not dependent on the pool)
- Benchmark scripts fixed + concurrent (bash + curl + jq only):
  - `kask/scripts/check-classifier-models.sh` — bounded `xargs -P` (CONCURRENCY env,
    default 8), per-case files for deterministic ordering, running-count progress
    (`S04 [SCORED 5/50] OK ...`), per-call timing (own clock per case, correct under
    parallelism), scored-only per-task denominators (section/17, dimension/20,
    failure/10), case-id-aware in-context exclusion.
  - `kask/scripts/run-classifier-eval.sh` — live progress to stderr (foreground +
    backgrounded), per-model `[i/N]`, summary never left silently empty (FAIL row on
    error), comment-stripping list reader.
- Validated locally with a stubbed curl (no network, no spend): concurrency, ordering,
  running counter, and timing all correct.
- One real 3-model validation run completed (deepseek-v4-flash, ling-2.6-1t,
  granite-4.1-8b): ling-2.6-1t reproduced sequential baseline exactly (35/47); the
  ±2–4 accuracy drift on others is OpenRouter routing nondeterminism, not a
  concurrency artifact.
- Eval set: `kask/docs/review/eval_set.json` (50 cases, 3 real label spaces).
- Prior 6-model baseline preserved in the existing report
  (`kask/docs/review/classifier-model-review.md`) — not yet rewritten pending the
  finalized pool + a clean full run.

## Operator: mark CONFIRMED (or edit above) before any run proceeds.