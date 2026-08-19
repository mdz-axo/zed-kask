---
name: superforecasting
core: true
visibility: public
description: "Superforecasting pipeline following Tetlock's Good Judgment Project methodology. Eight-stage process: question triage, Fermi decomposition, outside/inside views, Bayesian updating, dragonfly-eye synthesis, calibration, and forecast recording."
---

# Superforecasting

Superforecasting pipeline following Tetlock's Good Judgment Project methodology. Eight-stage process from question triage through Fermi decomposition, outside/inside views, Bayesian evidence updating, dragonfly-eye synthesis, probability calibration, and forecast recording.

## When to Use

- When you need to forecast the likelihood of a future event using a rigorous, structured methodology based on Tetlock's Good Judgment Project.
- When a forecasting question falls in the "Goldilocks zone" (not too easy, not too unpredictable) and warrants full pipeline investment.
- When you need to decompose a complex prediction into tractable sub-questions and establish base rates using the outside view.
- When you need to update a prior probability with new evidence using Bayesian methods and likelihood ratios.
- When you need to synthesize multiple causal models and dissenting views into a single calibrated probability.
- When you need to record a forecast with resolution criteria for later tracking, Brier scoring, and post-mortem analysis.
- When evaluating generated forecasts through an independent quality gate to assess calibration realism, confidence justification, evidence trail, and record completeness without self-assessment bias.
- When LEAP expert-judgment forecasts are available for the forecasting question or a close reference class, and you want to anchor the outside view on FRI's longitudinal expert panel rather than (or in addition to) market-implied probabilities.

## Instructions

### stage_0_triage

1. Evaluate whether a forecasting question is worth investing significant effort in.
2. Classify the question into "clocklike" (easy), "goldilocks" (just right), or "cloudlike" (unpredictable).
3. Assess if there is sufficient publicly available information, if the outcome is determined by analyzable factors, if research would improve accuracy, and if the time horizon is appropriate.
4. Recommend proceeding if the question is in the goldilocks zone.

### stage_1_fermi_decompose

1. Decompose the forecasting question into tractable sub-questions.
2. Unpack the question by asking what it would take for the answer to be yes or no.
3. Separate knowable from unknowable factors and expose assumptions.
4. Generate 3-7 sub-questions that are specific and answerable. Emit both a flat list (`sub_questions`, consumed by the Fermi calibration step) and a conditional probability tree (`sub_question_tree`) encoding how they combine into the outcome.
5. Structure the tree so correlated sub-questions share a common-cause parent (never co-parents of the same child — `marginalize` assumes parent independence). Keep fan-in ≤ 3 and depth ≤ 3.
6. List all assumptions, noting whether they are reasonable and what happens if they are false.
7. Identify established facts (knowns) and uncertain factors requiring estimation (unknowns).

### stage_2_outside_view

1. Establish base rates by identifying relevant reference classes and determining how often similar events occur.
2. Identify reference classes for the main question and sub-questions.
3. Determine the historical frequency, sample size, and data quality for each reference class.
4. Establish a starting probability anchor based on the base rates before considering case-specific details.

### stage_3_probability_estimate (delegated split)

The former single inside-view step is split into three FlowDef steps. Generation and counterfactual analysis are delegated to the `falsifiability` skill; probability estimation stays in superforecasting.

1. **Generate causal hypotheses (delegate to falsifiability).** Invoke `falsifiability/falsifiability-hypothesize` with `admitted_target` = the forecasting question, `domain` = "forecasting", `context` = the sub-questions and outside-view output. Produces 3–7 ranked candidate causal pathways with forced diversity (≥1 primary, ≥1 alternative, ≥1 contamination/false-positive, ≥1 opposing-outcome), each with a Platt-form prediction and a falsifier; discards vibes at generation.
2. **Construct counterfactuals / necessary conditions (delegate to falsifiability).** Invoke `falsifiability/falsifiability-counterfactual` with the generated `hypotheses`, `admitted_target`, `domain`. For each hypothesis construct the minimal do(not X) counterfactual, hold confounders fixed, and derive the testable consequence that distinguishes the counterfactual world from the factual one. Flag irreducible causes.
3. **Estimate probabilities and emit the tree (superforecasting).** Invoke `superforecasting/stage_3_probability_estimate` with the `hypotheses`, `counterfactuals`, `starting_probability` (the outside-view anchor), `outside_view_output`, and the conditional probability tree from stage 1 (`sub_question_tree`, `topological_order`, `outcome_node_id`). For each hypothesis weigh evidence pro/con against its counterfactual's testable consequence, assign an individual probability, and enforce internal consistency. For each tree node estimate a marginal (roots) or a conditional table (dependents) — the combinator (AND/OR/mixture) is encoded structurally in the conditional values, not as a separate field. The `combine_tree_probabilities` compute step (next ordinal) walks the tree via `hkask_forecast::marginalize` and produces `tree_combined_probability` — the exact inside-view posterior fed to stage 4 as the prior. The LLM no longer estimates `combined_probability`; the compute step owns that.

### stage_4_evidence_update

1. Incorporate new evidence and update probabilities using likelihood ratios and Bayesian reasoning.
2. Assess the strength (weak/moderate/strong) and direction (supports/contradicts/neutral) of each piece of evidence.
3. Calculate or estimate the likelihood ratio (P(E|H) / P(E|~H)) for each evidence item.
4. Make many small updates most of the time, and occasional large updates when evidence is very strong.
5. Update the prior probability to the posterior probability based on the accumulated evidence.

### stage_5_synthesis

1. Integrate multiple causal models and perspectives into a "dragonfly eye" view.
2. Identify clashing causal forces pushing toward YES vs. NO.
3. Steelman the strongest opposing arguments, making them as persuasive as possible.
4. Generate 3-5 distinct causal models, each with an implied probability.
5. Apply MCDA-style weighted aggregation: score each model against evidence alignment, reference class stability, causal mechanism clarity, and model confidence criteria. Compute composite scores and detect compensation masking.
6. Synthesize an integrated probability using the MCDA-weighted average of model probabilities.
7. Aggregate the judgments of different models, noting where they agree and diverge.

### stage_6_calibration

1. Assign a precise, well-calibrated probability to the forecasted outcome using the full 0-100% scale.
2. Avoid hedge words and use specific percentages matched to evidence quality.
3. Assess confidence level (low, medium, high) based on evidence quality, model agreement, and reference class stability.
4. Justify the specific probability and precision against the pipeline's evidence trail.
5. Define a defensible range of probabilities that would also be reasonable.

### stage_7_record

1. Create a structured record of the forecast for later tracking, scoring, and post-mortem analysis.
2. Include a unique tracking ID, timestamp, full question text, resolution criteria, probability, and confidence.
3. Summarize the reasoning and list key assumptions made.
4. Define what would count as resolution and what evidence will determine the outcome.
5. Set an expiration date for when the forecast should be evaluated.

### forecast-quality-gate

1. Evaluate the forecast across four independent dimensions: calibration realism, confidence justification, evidence trail, and record completeness.
2. Score each dimension on a 0–1 scale with specific evidence from the calibration and record outputs.
3. Set gate_pass to true only if all four scores are >= 0.60.
4. If gate_pass is false, each failing dimension must have a specific, actionable fix note.
5. You are evaluating, not generating — do not rewrite or improve the forecast.

## LEAP Integration

The superforecasting cascade accepts an optional `expert_prior` array input
alongside `market_context`. Before invoking the cascade, the invoking agent
should:

1. Call `rss_search` on the `fri-leap` and `fri-leap-reports` streams with
   keywords from the forecasting question (e.g. "AGI timelines", "GDP growth",
   "AI risks").
2. If matches are found, parse the LEAP forecast distribution from the
   matched entries into the `expert_prior` array. Each element carries:
   `question`, `median_probability`, `superforecaster_median` (if available),
   `public_median` (if available), `interquartile_range` (if available),
   `time_horizon`, `scenario` ('rapid AI progress' / 'slow AI progress' /
   'unconditional'), `rationale_excerpt` (optional), `source_url`, `wave`.
3. Pass the `expert_prior` array alongside `market_context` when invoking the
   superforecasting skill.

The cascade does not fetch LEAP itself — the invoking agent does, because the
cascade is a single skill invocation and should not reach out to MCP servers
mid-cascade (per the existing `market_context` pattern). If `rss_search`
returns no matches for the forecasting question, `expert_prior` is empty and
the cascade runs without it. Do not fabricate LEAP data.

## EQM Feedback Integration

The superforecasting cascade accepts an optional `overconfidence_bias` number
input that feeds the step-16 compute (`apply_calibration_adjustment`) to
close the Brier feedback loop. Before re-invoking superforecasting on a prior
iteration's forecast, the invoking agent should:

1. Run the `eqm` skill on the prior iteration's rationale (stage_7_record
   output) to score it against the Explanation Quality Markers (Karvetski et
   al. 2026).
2. Extract the `overconfidence_bias` field from the `eqm` skill's feedback
   output. The signal is signed: positive = overconfident (red flags dominate,
   especially `extreme_confidence` and `forecast_rationale_misalign`), negative
   = underconfident (excessive `speculative_terms` without grounding).
3. Pass the `overconfidence_bias` value when re-invoking superforecasting.

The cascade does not run `eqm` itself — the invoking agent does, because the
cascade is a single skill invocation and should not reach out to other skills
mid-cascade (per the existing `market_context` and `expert_prior` pattern). If
`eqm` is not run, `overconfidence_bias` defaults to 0.0 (no adjustment). Do
not fabricate the bias value.

For rationale-level improvement (not just calibration feedback), invoke the
`eqm-improvement` skill on the rationale before re-invoking superforecasting.
`eqm-improvement` reverse-engineers the reasoning patterns the EQM definitions
specify, producing an improved rationale with a higher EQM passage rate while
preserving the forecast probability (alignment invariant).

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `stage_0_triage.j2` | WordAct | Triage a forecasting question to determine difficulty level and whether it falls in the Goldilocks zone warranting full pipeline investment. |
| `stage_1_fermi_decompose.j2` | WordAct | Fermi-decompose the forecasting question into independent, tractable sub-questions. Separate knowns from unknowns and document assumptions. |
| `stage_2_outside_view.j2` | WordAct | Establish base rates by identifying reference classes and determining how often similar events occur. Produces the outside-view starting probability. |
| `stage_3_probability_estimate.j2` | WordAct | Inside-view probability estimation. Takes pre-generated hypotheses (from falsifiability/falsifiability-hypothesize) and their counterfactual necessary-conditions (from falsifiability/falsifiability-counterfactual), weighs evidence pro/con against each counterfactual's testable consequence, assigns individual probabilities, enforces internal consistency, and combines to adjust from the outside-view anchor. Replaces the probability half of the former stage_3_inside_view step. |
| `stage_4_evidence_update.j2` | WordAct | Incorporate new evidence via Bayesian updating with likelihood ratios. Revise the prior probability based on evidence strength. |
| `stage_5_synthesis.j2` | WordAct | Synthesize a dragonfly-eye view by integrating multiple causal models and perspectives. Steel-man dissenting views and produce a synthesized probability. |
| `stage_6_calibration.j2` | WordAct | Calibrate the final probability using the full 0-100% scale. Justify precision against known calibration principles and the pipeline's evidence trail. |
| `stage_7_record.j2` | WordAct | Create a structured forecast record with resolution criteria and expiration date for later tracking, Brier scoring, and post-mortem analysis. |
| `forecast-quality-gate.j2` | KnowAct | Independent quality gate that evaluates forecast calibration realism, confidence justification, evidence trail completeness, and record quality without self-assessment bias. Receives calibration and record outputs and produces calibrated 0–1 scores plus a gate_pass determination with actionable fix notes. |
| `forecast-quality-gate.j2` | KnowAct | Independent quality gate that evaluates forecast calibration realism, confidence justification, evidence trail completeness, and record quality without self-assessment bias. Receives calibration and record outputs and produces calibrated 0–1 scores plus a gate_pass determination with actionable fix notes. |

## Constraints

- rJoule cap: 5 per invocation. Maximum 10 iterations.
- `stage_0_triage.j2`: Public.
- `stage_1_fermi_decompose.j2`: Public.
- `stage_2_outside_view.j2`: Public. (Expert-judgment priors are fetched by the invoking agent from the LEAP RSS feed via `hkask-mcp-research` `rss_search`/`rss_get_entries` before cascade invocation, parallel to `market_context`.)
- `stage_3_probability_estimate.j2`: Public. (Inside-view generation + counterfactual analysis are delegated to `falsifiability/falsifiability-hypothesize` and `falsifiability/falsifiability-counterfactual`.)
- `stage_4_evidence_update.j2`: Public. (Expert-judgment priors are fetched by the invoking agent from the LEAP RSS feed via `hkask-mcp-research` `rss_search`/`rss_get_entries` before cascade invocation, parallel to `market_context`.)
- `stage_5_synthesis.j2`: Public.
- `stage_6_calibration.j2`: Public.
- `stage_7_record.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
