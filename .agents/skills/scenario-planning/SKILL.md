---
name: scenario-planning
description: "Run a complete scenario-planning project: Schwartz framing, forces and divergent 2x2 narratives with an independent quality gate and early-warning indicators, then Tetlock quantification and Bayesian propagation, Brier-scored resolution, calibration tracking, and Chermack assessment, over the scenarios MCP server."
---

# Scenario Planning

A complete scenario-planning project: frame the focal question
conversationally (Schwartz), quantify the event tree (Tetlock), update
on evidence, score resolutions, and assess the project itself (Chermack).
The scenarios server implements all three methodologies as one pipeline;
this skill is the operating procedure for that pipeline.

## Reference models

- Schwartz, *The Art of the Long View* (1991) — focal question, driving forces, 2x2 narratives, indicators.
- Tetlock & Gardner, *Superforecasting* (2015) — `onto_anchor` → derived `superforecasting`; Brier (1950) → derived `brier_score`.
- Chermack, *Scenario Planning in Organizations* (2011) — project assessment.
- `onto_anchor` → derived `scenario_planning` (Schwartz 1991; Chermack 2011; operator ruling 2026-09-25).

## Initial and target condition

- **Initial condition (T1):** the `scenario_triage` classification, the FramingDocument, and — when prior projects exist — `scenario_calibration` (resolved count, Brier, bias) passed as `prior_calibration`; on a first run it is null.
- **Target condition (T2):** the Convergence gate below passes and the Chermack assessment is reported.

**D/P labelling.** Every `scenario_*` tool call is D (the server is the oracle: it rejects bad probabilities, conditional lengths and cycles, and computes marginals, Bayes, synthesis and Brier). Framing answers are the operator's (human decision). Brainstorm events, conditionals, forces and narratives are P, critiqued by the separate `scenario-quality-gate` render, `scenario_cross_validate` (divergence > 0.15 → `grill-me`), and, at resolution, Brier.

## When to Use

- The operator faces a decision under uncertainty with a time horizon
  (a strategy, an investment, a policy).
- A forecasting question deserves the full treatment: framing,
  dependencies, calibrated probabilities, and later scoring.
- Re-running an existing scenario project with new evidence.

## When NOT to Use

- A single quick probability estimate — use the `superforecasting`
  skill instead (it is the Tetlock-only fast path).
- A financial 2x2 valuation matrix for a company — use the companies
  server's `scenario_analysis` (the Schwartz 2x2 for valuation lives
  there; this skill is the general event-tree pipeline).
- The question has no deadline or no resolution criteria — refine it
  first (`scenario_triage` will classify it cloudlike).

## Instructions

### Phase 1 — Frame (Schwartz: the focal question)

1. Call `scenario_triage` with the question. If it classifies cloudlike,
   work with the operator to sharpen it before proceeding; if clocklike,
   a base rate may suffice — say so and stop unless the operator wants
   the full project anyway.
2. Call `scenario_frame` with the subject. Run the 7-turn framing
   conversation it prescribes WITH the operator — you are the coach,
   not an interviewer. Do not answer for the operator.
3. Call `scenario_frame_document` with the collected answers to produce
   the typed FramingDocument.

### Phase 2 — Diverge and structure (Schwartz: brainstorm)

4. Call `scenario_brainstorm` with the frame. Run its 4-round protocol
   (DIVERGE with the personas, GROUND in facts and base rates, LINK
   causal chains, PRUNE to the final tree).
5. If research is needed, run web searches, then call `scenario_build`
   with the research text to get the extraction scaffold, and extract
   candidate events against it. Refine the candidates into `ScenarioEvent`
   objects (yes/no questions, deadlines, dependency edges with
   conditionals).
6. Call `scenario_quantify` with the events. It returns marginals, the
   joint probability, and a sensitivity ranking. Emit the `graph` viz
   block it describes so the operator sees the tree.

### Phase 2b — Divergent 2x2 narratives (Schwartz: axes, stories, indicators)

Run when the uncertainty is structural (the operator needs distinct
futures to plan against, not one probability). It extends the framing into
the Schwartz narrative set; the event tree from Phase 2 remains the
quantified backbone.

7. Render `scenario-planning/key-forces` with the refined focal question,
   planning horizon, domain and any `market_match` candidates as
   `market_context`; then `scenario-planning/driving-forces` (STEEP
   importance × uncertainty) to select two independent critical
   uncertainties as axes. `scenario-planning/focal-question` refines the
   question first when the framing document left it unbounded.
8. Render `scenario-planning/axes-and-narratives` for the four quadrant
   narratives, then `scenario-planning/scenario-quality-gate` — a separate
   render that scores divergence, consistency and coverage (0–1). A failing
   gate revises the narratives its fix notes name and re-runs once (max 2
   cycles); a second failure ships the scenarios with the fix notes shown.
9. Render `scenario-planning/implications-indicators` for robust and
   contingent strategies and observable early-warning indicators. Carry the
   indicator count into Phase 5's `scenario_assess`
   (`has_early_warning_indicators`).

### Phase 3 — Quantify and update (Tetlock)

10. For each event needing calibration, call `scenario_calibrate` with
   its Fermi sub-questions, base rate, and reference class. When ≥5
   resolved forecasts exist in the store it applies the learned
   overconfidence bias automatically — read the calibration-adjusted
   probability it returns.
11. On new evidence for a single event, call `scenario_update` (Bayes)
   and then `scenario_propagate` with the full event list and the
   event's new prior to recompute descendants and the joint. The
   propagation journal is the audit record — report the deltas.
12. When multiple independent perspectives exist, collect them and call
   `scenario_synthesize` (dragonfly-eye, inverse-Brier weighting).
13. Call `scenario_cross_validate` comparing your estimate against
    the server-computed one. If divergence exceeds 0.15, activate the
    `grill-me` skill on the diverging sub-questions before proceeding.
    Bound: one grill-me pass per diverging sub-question set;
    divergence > 0.15 that persists after one pass is recorded with
    both estimates and flagged for operator adjudication.

### Phase 4 — Resolve and learn (the Brier loop)

14. When event deadlines pass, call `scenario_score` with the events
    and their outcomes. This is the ONLY step that writes the forecast
    journal — persistence happens here, not at build time. Report the
    Brier score and its interpretation.
15. Call `scenario_calibration` to compute the calibration curve over
    resolved forecasts. Report bias direction (too high / too low).

### Phase 5 — Assess (Chermack)

16. Call `scenario_assess` with the project metrics (perspective count,
    disagreement, event count, dependency ratio, strategies generated
    and implemented, learning events, early-warning indicators). Report
    the per-phase scores, gaps, strengths, and recommendations.

### Convergence

17. Gate — call `lisp_eval` with:
    - form: `(and (> resolved_forecasts 0) (eq unresolved_critical 0))`
    - env: `{ "resolved_forecasts": <count from scenario_score>,
              "unresolved_critical": <events past deadline without outcomes> }`
    A project is complete when every event with a passed deadline has a
    recorded outcome and the assessment is reported. Calibration signal
    is only claimed at ≥10 resolved forecasts — below that, say the
    curve is thin.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `focal-question.j2` | Refine and bound the focal question with decision relevance, time horizon, and scope boundaries. |
| `key-forces.j2` | Identify and cluster micro-level forces, rated for impact and predictability. |
| `driving-forces.j2` | Map STEEP driving forces on importance × uncertainty and select two independent critical uncertainties. |
| `axes-and-narratives.j2` | Four divergent quadrant narratives on the two axes. |
| `scenario-quality-gate.j2` | Separate gate scoring divergence, consistency and coverage (0–1) with fix notes. |
| `implications-indicators.j2` | Robust and contingent strategies with observable early-warning indicators. |

To render a template, call `render_template` with the ref (e.g. `scenario-planning/key-forces`) and these inputs:
- `focal-question.j2`: `focal_question`, `planning_horizon`, `domain`, `prior_calibration`
- `key-forces.j2`: `refined_question`, `planning_horizon`, `domain`, `market_context`
- `driving-forces.j2`: `refined_question`, `key_forces`
- `axes-and-narratives.j2`: `refined_question`, `critical_uncertainties`, `planning_horizon`
- `scenario-quality-gate.j2`: `scenarios`, `refined_question`
- `implications-indicators.j2`: `refined_question`, `scenarios`

## Constraints

- `scenario_build` does NOT persist anything — do not tell the operator
  scenarios are "saved for later scoring". Only `scenario_score` writes
  the journal.
- Probabilities outside [0,1], conditionals whose length is not
  2^parents, and cycles are rejected by the server — fix the input,
  never work around the rejection.
- Withhold is honest: if a market-derived base rate is low-reliability,
  the bridge withholds it. Report withheld inputs as unknowns.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "scenario-planning", the tool name, and the error;
  continue with the best available information.
