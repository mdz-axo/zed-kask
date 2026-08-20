---
name: scenario-builder
description: "Scenario planning methodology following Schwartz's framework. Refines focal questions, maps key forces, generates divergent 2x2 scenario narratives, runs a quality gate, and derives implications with early-warning indicators."
---

# Scenario Builder

Scenario planning methodology following Schwartz's framework. Refines focal questions, maps key forces and macro-level driving forces, generates divergent 2x2 scenario narratives, runs independent quality gate, and derives implications with early-warning indicators.


## When to Use

- When refining a focal question to ensure it is decision-relevant, time-bounded, and scope-bounded for strategic scenario analysis.
- When identifying and clustering micro-level key forces (market dynamics, competitor actions, regulatory changes) affecting a focal question.
- When mapping macro-level driving forces across STEEP domains (society, technology, economy, environment, politics) to identify critical uncertainties.
- When generating divergent 2x2 scenario narratives by selecting two critical uncertainty axes and writing detailed, plausible, and challenging quadrant stories.
- When evaluating generated scenarios through an independent quality gate to assess divergence, consistency, and coverage without self-assessment bias.
- When computing a normalized convergence metric for scenario-planning cycles using independent quality gate outputs and heuristic divergence checks.
- When deriving actionable per-scenario implications, robust strategies, contingent strategies, and measurable early-warning indicators.

## Instructions

### scenario_calibration (step 1, execute — no template)

1. Step 1 is an `execute` step (no template): fetches the prior calibration curve from the scenarios MCP via `scenario_calibration` (hkask-mcp-scenarios). Returns `overall_brier`, `overconfidence_score`, and per-bin calibration data from resolved forecasts. On the first invocation with no resolved forecasts, returns a valid result with guidance noting insufficient data — not a failure.

### focal-question

1. Refine the focal question to ensure it is decision-relevant and tied to a real strategic choice.
2. Anchor the question to the explicit time boundary of the planning horizon.
3. Bound the scope of the question, explicitly defining what is in and out of scope.
4. Summarize the current state relevant to the refined question.
5. Preserve the strategic intent of the original question while making it actionable.

### market_match (step 3, execute — no template)

1. Step 3 is an `execute` step (no template): fetches prediction-market candidates for the focal question via `market_match` (hkask-mcp-prediction-markets). Deterministic market anchor for the key-forces step — market-implied probabilities pre-weight STEEP forces by likelihood. An empty result is a valid signal that no relevant market exists, not an error.

### key-forces

1. Enumerate key forces in the microenvironment, including market demand, regulatory regime, technology maturity, competitor behavior, and resource availability.
2. List all relevant forces exhaustively before clustering.
3. Cluster related forces into thematic groups.
4. Assess each force for its impact (high/medium/low) on the focal question.
5. Assess each force for its predictability (high/medium/low) trajectory.

### driving-forces

1. Map driving forces across the five macro STEEP domains: society, technology, economy, environment, and politics.
2. Plot each driving force on a 2D importance × uncertainty matrix.
3. Select the two most critical uncertainties from the high-importance, high-uncertainty quadrant as scenario axes.
4. Verify that the two selected axes are independent and not causally linked.
5. Rate each force on importance (1-5) and uncertainty (1-5).

### axes-and-narratives

1. Define the two axes from the provided critical uncertainties.
2. Write a detailed narrative (300+ words) for each of the 4 quadrants.
3. Start each narrative from the present, chain events causally, and reach the planning horizon.
4. Ensure scenarios are plausible, divergent, consistent, and challenging.
5. Give each scenario a vivid, memorable name that captures its essence.

### scenario-quality-gate

1. Evaluate the scenario set across three independent dimensions: divergence, consistency, and coverage.
2. Assess divergence by checking for structural differences, axis coverage, naming distinctiveness, and assumption challenges.
3. Assess consistency by verifying internal logic, causal chains, and alignment between narrative and end state.
4. Assess coverage by checking axis span, blind spots, and boundary cases.
5. Score each dimension on a 0–1 scale, justifying scores with specific evidence from the narratives.
6. Set the parametric variation flag to true if any two end states differ only in degree, not kind.
7. Determine gate_pass as true only if there are exactly four scenarios and all three scores are ≥ 0.60.

### implications-indicators

1. Identify per-scenario implications, opportunities, and risks for the focal question.
2. Derive robust strategies that perform well across all four scenarios, explaining the mechanism for each.
3. Classify each robust and contingent strategy's constraint force as a prohibition, guardrail, or guideline.
4. Define contingent strategies triggered by specific scenarios, ensuring each has a clear, observable trigger indicator.
5. Identify measurable early indicators (tripwires) and map them to the scenarios they signal.
6. Include the observation method for each early indicator.
7. Formulate a brief plan for ongoing monitoring of early indicators.

### scenario_build (step 9, execute — no template)

1. Step 9 is an `execute` step (no template): persists the generated scenarios to the scenarios MCP via `scenario_build` (hkask-mcp-scenarios) for later comparison against actual outcomes. Closes the persistence half of the calibration loop — the scenarios are now in the forecast store where `scenario_score` can later Brier-score them.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `axes-and-narratives.j2` | KnowAct | Generate divergent scenario narratives by selecting two critical uncertainty axes from identified driving forces. Produces four quadrant scenarios with internally consistent narratives. |
| `driving-forces.j2` | KnowAct | Map macro-level driving forces (STEEP: society, technology, economy, environment, politics) against an importance-uncertainty matrix to identify critical uncertainties. |
| `focal-question.j2` | KnowAct | Refine and bound the focal question with decision relevance, time horizon, and scope boundaries. Produces a current state summary. |
| `implications-indicators.j2` | KnowAct | Derive per-scenario implications and identify early-warning indicators. Produces robust strategies (for all scenarios) and contingent strategies (for specific unfoldings). |
| `key-forces.j2` | KnowAct | Identify micro-level forces: proximate factors, market dynamics, competitor actions, demand shifts, and regulatory changes. Clusters identified forces into thematic groups. |
| `scenario-quality-gate.j2` | KnowAct | Independent quality gate that evaluates scenario divergence, consistency, and coverage without self-assessment bias. Receives scenarios from the narrative generator and produces calibrated 0–1 scores plus a gate_pass determination with actionable fix notes. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- `axes-and-narratives.j2`: Public.
- `driving-forces.j2`: Public.
- `focal-question.j2`: Public.
- `implications-indicators.j2`: Public.
- `key-forces.j2`: Public.
- `scenario-quality-gate.j2`: Public.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
