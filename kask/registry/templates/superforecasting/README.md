# Superforecasting Pipeline

**Location:** `registry/manifests/superforecasting.yaml`  
**Templates:** `registry/templates/superforecasting/`  
**Version:** 0.31.0

## Overview

This pipeline implements Philip Tetlock's Fermi-ization methodology from the Good Judgment Project. It provides a structured, multi-stage approach to producing well-calibrated probabilistic forecasts.

## Pipeline Stages

| Stage | Template | Purpose | Energy Cap |
|-------|----------|---------|------------|
| 0 | `stage_0_triage.j2` | Classify question difficulty (Goldilocks zone) | 2,048 |
| 1 | `stage_1_fermi_decompose.j2` | Decompose into tractable sub-questions | 4,096 |
| 2 | `stage_2_outside_view.j2` | Establish base rates from reference classes | 4,096 |
| 3 | `falsifiability-hypothesize` → `falsifiability-counterfactual` → `stage_3_probability_estimate.j2` | Generate causal hypotheses + counterfactual necessary-conditions (delegated to falsifiability), then estimate probabilities and adjust from the outside-view anchor | 4,096 |
| 4 | `stage_4_evidence_update.j2` | Bayesian belief revision | 4,096 |
| 5 | `stage_5_synthesis.j2` | Dragonfly eye aggregation of perspectives | 4,096 |
| 6 | `stage_6_calibration.j2` | Assign precise, calibrated probability | 4,096 |
| 7 | `stage_7_record.j2` | Record forecast for tracking/audit | 2,048 |
| 8 | `forecast-quality-gate.j2` | Independent quality gate (calibration, confidence, evidence, record) | 3,072 |
| 9 | `superforecasting-convergence-check.j2` | Convergence metric + materiality guard | 2,048 |

**Total Energy Budget:** 25,000 tokens

## Theoretical Foundation

Based on Tetlock's **Ten Commandments for Aspiring Superforecasters**:

1. **Triage** (Commandment 1) — Focus on questions where effort pays off
2. **Fermi-ization** (Commandment 2) — Decompose intractable problems
3. **Outside/Inside View** (Commandment 3) — Anchor on base rates, adjust for specifics
4. **Evidence Updating** (Commandment 4) — Bayesian belief revision
5. **Causal Synthesis** (Commandment 5) — Dragonfly eye perspective aggregation
6. **Precision Calibration** (Commandments 6-7) — Use full probability scale
7. **Error Tracking** (Commandment 8) — Prepare for post-mortem analysis

## Deterministic Primitives (Rust Conformance Contract)

The natural-language pipeline above is backed by a small set of deterministic
primitives in the `hkask-forecast` crate (`crates/hkask-forecast/src/lib.rs`) —
the canonical pure-math core of the Tetlock methodology. The skill's LLM stages
consume these formulas implicitly; the MCP servers (`hkask-mcp-scenarios`,
`hkask-mcp-companies`) consume them explicitly via `hkask_forecast::*`.

This table is the conformance contract: each skill stage is mapped to the
`hkask-forecast` function that implements its deterministic core, or marked
"natural-language only" when no pure-math core exists. The contract is
mechanically verified by `scripts/check-forecast-conformance.sh` in CI.

| Stage | `hkask-forecast` function | Notes |
|-------|---------------------------|-------|
| 0 Triage | — | Natural-language only. A deterministic heuristic (`triage_question`) lives in `hkask-mcp-scenarios` for tooling, but skill stage 0 is LLM judgment. |
| 1 Fermi decomposition | `calibrate_from_fermi` | Confidence-weighted average of `FermiQuestion` estimates. |
| 2 Outside view | `outside_view_adjustment` | Shrinkage estimator blending base rate with inside estimate. |
| 3 Inside view (probability estimate) | — | Natural-language only. Hypothesis generation + counterfactual analysis are delegated to the `falsifiability` skill; probability estimation is LLM reasoning against the anchor. |
| 4 Evidence update | `bayesian_update` | `posterior = prior × likelihood / evidence_base_rate`, clamped to [0.01, 0.99]. |
| 5 Synthesis (MCDA) | — | Natural-language only. Dragonfly-eye MCDA aggregation is LLM reasoning. |
| 6 Calibration | — | Natural-language only (forward-looking single-forecast calibration). Backward-looking 10-bin calibration tracking is `compute_calibration_curve` in `hkask-mcp-scenarios`. |
| 7 Record | — | Natural-language only (forecast record structure). Persistent journal storage is `ForecastStore` in `hkask-mcp-scenarios`. |
| Quality gate | — | Natural-language only (independent rubric evaluation). |
| Convergence check | — | Natural-language only (materiality guard + weighted-penalty rubric). |
| Brier scoring (cross-cutting) | `brier_score`, `brier_score_multi`, `brier_interpretation` | Used by stage 7 record feedback and the MCP servers' outcome tracking. |
| Calibration feedback (cross-cutting) | `apply_calibration_adjustment` | Closes the Tetlock learning loop: consumes a calibration curve's overconfidence bias (from `compute_calibration_curve` in `hkask-mcp-scenarios`) to adjust the next forecast's prior toward 0.5. The first operational bridge between recorded outcomes and future forecasts. |

**Layering rule:** `hkask-forecast` holds pure-math primitives only — no domain
types, no NLP, no I/O. Domain-shaped logic (`WeightedScenario`,
`ForecastOutcome`, `ForecastStore`, event-tree marginalization, `FermiDefaults`
env loading) stays in the MCP servers where it is consumed. The skill operates on
natural language and does not call Rust directly, but its stage descriptions
must stay consistent with the formulas the primitives implement.

## Usage

### Invoking the Pipeline

```yaml
# Example pipeline invocation
manifest_id: superforecasting
input:
  forecasting_question: "Will [specific outcome] occur by [date]?"
  domain: "geopolitics"  # optional
  time_horizon: "6 months"  # optional
  resolution_criteria: "How the outcome will be judged"
  expiration_date: "2026-12-31"
```

### Stage Outputs

Each stage produces structured JSON output that feeds into subsequent stages:

```json
// Stage 0: Triage
{
  "difficulty_level": "goldilocks",
  "goldilocks_zone": true,
  "proceed_recommendation": true,
  "rationale": "..."
}

// Stage 1: Fermi Decomposition
{
  "sub_questions": ["...", "..."],
  "assumptions": [...],
  "knowns": [...],
  "unknowns": [...]
}

// Stage 2: Outside View
{
  "reference_classes": [...],
  "base_rates": [...],
  "starting_probability": 0.35
}

// Stage 6: Final Calibration
{
  "final_probability": 0.42,
  "confidence_level": "medium",
  "precision_justification": "...",
  "defensible_range": {"lower": 0.35, "upper": 0.50}
}
```

## Regulation Integration

The pipeline emits Regulation spans for monitoring:
- `hkask.template.select` — Pipeline selection
- `hkask.template.render` — Template execution at each stage
- `hkask.template.outcome` — Forecast recorded

**Variety Counters:**
- `hypothesis_count` — Number of causal hypotheses generated
- `reference_class_count` — Number of reference classes identified
- `evidence_item_count` — Number of evidence items evaluated

**Algedonic Alert:** Triggered if variety deficit >100 (escalates to Curator)

## OCAP Requirements

The pipeline requires the following capabilities:
- Template render permissions for all 8 stages
- Manifest execution permission
- Regulation emission permission
- Memory storage permission (for forecast recording)

All capabilities are template-scoped and expire after 3600 seconds.

## Error Handling

| Error Type | Behavior |
|------------|----------|
| Energy exceeded | Abort |
| Timeout | Retry (max 2, 2s backoff) |
| Validation failure | Abort |
| Capability denied | Escalate to Curator |

## Audit Trail

All pipeline executions are logged with:
- Input question and parameters
- Output from each stage
- Energy costs per stage
- Regulation event references
- Final forecast record

## Testing the Pipeline

1. **Unit tests:** Test each template independently with mock inputs
2. **Integration tests:** Run full pipeline on historical questions with known outcomes
3. **Calibration tests:** Compare predicted probabilities to actual outcomes over time

,## Fusion Mode

This skill supports **fusion mode** — multi-model deliberation where each
inference call is sent to a panel of models in parallel, then a **judge**
processes the responses. The judge is either an LLM model operating in one
of five deliberation modes (synthesis, best-of-n, critique, deliberation, pi),
or the **algo / no-judge** path (`judge: algo`) — a deterministic JSON merge
with zero LLM judge calls. This is logically equivalent to ensemble mode:
instead of running the entire pipeline N times independently, each step
benefits from N model perspectives with per-step synthesis (LLM judge) or
per-step merge (algo / no-judge).

,### Enabling Fusion Mode

Fusion is configured per-manifest via the `fusion:` block in the flow manifest.
Each skill declares its own judge, panel, mode, and skill anchors:

```yaml
fusion:
  judge: deepseek-v4-pro
  panel:
    - Kimi2.7
    - Qwen3.7 Max
    - GLM5.2
    - Minimax3
  mode: synthesis  # or critique, deliberation, best-of-n, pi
  skills:
    - superforecasting  # anchor judge on Tetlock methodology
  max_rounds: 5
```

When the `fusion:` block is present, all analysis steps route through this
fusion config (the panel models in parallel, judge synthesizes). The global
env vars (`HKASK_FUSION_*`) are NOT needed when per-manifest config is present.

### Per-Step Fusion Control

The quality gate (step 9) and convergence check (step 10) have `fusion: false`
set explicitly — these are deterministic rubric evaluations that should use
single-model inference for reproducibility. All other steps inherit the
manifest-level `fusion: true` and will route through the panel when fusion
is globally enabled.

### Why Fusion ≈ Ensemble

Naive ensemble mode would run the entire 10-step pipeline N times with
different models, then synthesize the final forecast. Fusion mode achieves
the same goal more efficiently:

1. **Earlier error catching**: each step's output is validated by multiple
   models before propagating to the next step (vs. end-of-pipeline synthesis)
2. **Judge anchoring**: the judge can be anchored on the superforecasting
   methodology via `HKASK_FUSION_SKILLS=superforecasting`, ensuring synthesis
   follows Tetlock's principles at each step
3. **Lower cost**: only inference calls are multiplied (not flow orchestration)

### Future: Per-Manifest Fusion Config

Currently, fusion uses the global config (env vars). Per-manifest fusion config
(custom judge/panel per skill) requires extending `FusionConfig` into the
manifest schema and routing it through the `InferencePort` trait — noted as a
future enhancement.

## Future Enhancements

- [x] Iterative loop (return to earlier stages on new evidence) — step 11 restarts at Fermi decomposition (step 2), carrying forward the prior iteration's calibrated probability for the materiality guard
- [x] Independent quality gate (step 9) — evaluates calibration realism, confidence justification, evidence trail, and record completeness without self-assessment bias
- [ ] Ensemble mode (multiple parallel pipeline runs) — Note: distinct from hKask ensemble module (deferred 2026-06-14)
- [ ] Human-in-the-loop checkpoints
- [ ] Automatic reference class lookup from knowledge base
- [ ] Brier score tracking and feedback
- [x] MCDA-style weighted aggregation in stage 5 (synthesis) — causal models scored against evidence alignment, reference class stability, causal mechanism clarity, and model confidence criteria, with compensation masking detection. Embedded in the synthesis template rather than delegated via template_ref to avoid flow step ordinal shifts.
- [ ] Sub-question independence validation in stage 1 (Fermi) — hypothesis-framer interface mismatch: FINER/PICO evaluates research question quality, not Fermi sub-question independence. A lightweight independence check embedded in the Fermi template is a better fit than cross-skill delegation.

## References

- Tetlock, P. & Gardner, D. (2015). *Superforecasting: The Art and Science of Prediction*
- Good Judgment Project: https://goodjudgment.com/
- Fermi-ization methodology: https://goodjudgment.com/superforecasters-toolbox-fermi-ization-in-forecasting/
- Ten Commandments: https://goodjudgment.com/philip-tetlocks-10-commandments-of-superforecasting/
