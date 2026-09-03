---
name: scenario-planning
description: "Run a complete scenario-planning project over the scenarios MCP server: Schwartz framing and brainstorming, Tetlock quantification and Bayesian propagation, Brier-scored resolution, calibration tracking, and Chermack five-phase assessment. Composes the server's own integrated pipeline (scenario_frame through scenario_assess) end to end."
---

# Scenario Planning

A complete scenario-planning project: frame the focal question
conversationally (Schwartz), quantify the event tree (Tetlock), update
on evidence, score resolutions, and assess the project itself (Chermack).
The scenarios server implements all three methodologies as one pipeline;
this skill is the operating procedure for that pipeline.

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
5. If research is needed, run web searches, then call `scenario_research`
   with the raw research text to extract candidate events. Refine the
   candidates into `ScenarioEvent` objects (yes/no questions, deadlines,
   dependency edges with conditionals).
6. Call `scenario_quantify` with the events. It returns marginals, the
   joint probability, and a sensitivity ranking. Emit the `graph` viz
   block it describes so the operator sees the tree.

### Phase 3 — Quantify and update (Tetlock)

7. For each event needing calibration, call `scenario_calibrate` with
   its Fermi sub-questions, base rate, and reference class. When ≥5
   resolved forecasts exist in the store it applies the learned
   overconfidence bias automatically — read the calibration-adjusted
   probability it returns.
8. On new evidence for a single event, call `scenario_update` (Bayes)
   and then `scenario_propagate` with the full event list and the
   event's new prior to recompute descendants and the joint. The
   propagation journal is the audit record — report the deltas.
9. When multiple independent perspectives exist, collect them and call
   `scenario_synthesize` (dragonfly-eye, inverse-Brier weighting).
10. Call `scenario_cross_validate` comparing your estimate against the
    server-computed one. If divergence exceeds 0.15, activate the
    `grill-me` skill on the diverging sub-questions before proceeding.

### Phase 4 — Resolve and learn (the Brier loop)

11. When event deadlines pass, call `scenario_score` with the events
    and their outcomes. This is the ONLY step that writes the forecast
    journal — persistence happens here, not at build time. Report the
    Brier score and its interpretation.
12. Call `scenario_calibration` to compute the calibration curve over
    resolved forecasts. Report bias direction (too high / too low).

### Phase 5 — Assess (Chermack)

13. Call `scenario_assess` with the project metrics (perspective count,
    disagreement, event count, dependency ratio, strategies generated
    and implemented, learning events, early-warning indicators). Report
    the per-phase scores, gaps, strengths, and recommendations.

### Convergence

14. Gate — call `lisp_eval` with:
    - form: `(and (> resolved_forecasts 0) (eq unresolved_critical 0))`
    - env: `{ "resolved_forecasts": <count from scenario_score>,
              "unresolved_critical": <events past deadline without outcomes> }`
    A project is complete when every event with a passed deadline has a
    recorded outcome and the assessment is reported. Calibration signal
    is only claimed at ≥10 resolved forecasts — below that, say the
    curve is thin.

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
