---
name: company-research-deep
visibility: public
description: "Equity research deep pipeline converted from EFRA-AI (Replicant-Partners). Sequential flowdef: COMPANY deep 8-part analysis (5 native MCP tool calls + LLM synthesis) → GORILLA 4-dimension framework (fixed weights, lisp.eval scoring) → IMAGINE 5/10/20Y scenarios + digital stage (scenario_build MCP tool call + LLM synthesis) → THESIS three-pillar synthesis (cross-skill goal-analysis quality gate) → convergence check → loop on needs_work. MCP tool calls are native action: execute steps; templates do LLM synthesis over their outputs. Converges on THESIS investment_grade verdict."
---

# Company Research — Deep Pipeline

Equity research deep pipeline converted from EFRA-AI (Replicant-Partners). Sequential flowdef producing a deep company analysis and investment thesis. MCP tool calls (company_transcript, dcf_valuation, comparable_analysis, web_search, fetch, scenario_build) are native `action: execute` flowdef steps; templates do LLM synthesis over their outputs.

## When to Use

- When you need a deep company analysis and investment thesis for an equity ticker, following the Valentine × Gunn Dual-Mode Framework.
- When you want the full EFRA-AI research pipeline (COMPANY → GORILLA → IMAGINE → THESIS) as a single governed cascade.
- When you want the 8-part company analysis (Self-View, Business Franchise, Management Skill, Financial Profile, Invisible Layer, Turd Blossom, Value Gorilla Elevator Pitch, Investment Thesis Statement).
- When you want the GORILLA 4-dimension framework (Obvious Problem / Invisible Gorilla / Combinatorial Solution / Choke Point) with fixed methodology weights.
- When you want IMAGINE's 5/10/20Y scenarios with digital transformation stage classification and falsifiable predictions persisted to the forecast-ledger.
- When you want the THESIS three-pillar synthesis (Business Franchise, Management Quality, Valuation) with a cross-skill goal-analysis quality gate.

## Instructions

### company-8part

1. Self-View — how the company describes itself (verbatim from fetched IR page).
2. Business Franchise — identity, geography, moat, competitive position.
3. Management Skill — CEO scorecard (long-term) + CFO scorecard (working capital) with verbatim transcript quotes.
4. Financial Profile — signposts + 3-stage valuation (consensus → normalization → terminal).
5. Invisible Layer — what's not on the page, what's not in the price.
6. Turd Blossom — is the market pricing it like a turd? Early shoots of improvement?
7. Value Gorilla Elevator Pitch — economic opportunity + exploitation + why the market doubts it.
8. Investment Thesis Statement — durable, timeless, covering all three pillars.
9. Consume company_transcript, dcf_valuation, comparable_analysis, web_search, fetch MCP tool outputs.
10. Emit data_gaps for any failed MCP tool — never collapse to None.

### gorilla-4dim

1. Assess the 4 dimensions (Obvious Problem, Invisible Gorilla, Combinatorial Solution, Choke Point) against the COMPANY analysis.
2. Score each dimension 0–100 based on evidence.
3. The lisp.eval compute step in the flowdef applies the fixed weights (25/30/25/20) and computes the verdict (GORILLA ≥75 / SMALL_ANIMAL 50-74 / PEDESTRIAN <50).
4. Do NOT propose alternative weightings — the weights are fixed by firm methodology.

### imagine-longrange

1. Classify the digital transformation stage (MODEL / SHADOW / TWIN / SOURCE).
2. Classify the growth driver (innovation / demographic / both / neither).
3. Construct 5/10/20Y scenarios using the scenario_build MCP tool output as scaffold.
4. Emit 3–5 falsifiable predictions tagged by horizon (5Y/10Y/20Y) for the forecast-ledger — the IMAGINE kata loop.
5. Emit what's not on the page and what's not in the price.

### thesis-three-pillars

1. Synthesize all prior research into a formal investment thesis covering the three pillars: Business Franchise (moat strength, value creation, durability), Management Quality (capital allocation, leadership), Valuation (3-stage: consensus → normalization → terminal).
2. The terminal stage must cross-reference the IMAGINE 20Y scenario.
3. Emit the thesis statement (durable, timeless, covering all three pillars).
4. Do NOT self-evaluate — the investment_grade / needs_work / incomplete verdict comes from the cross-skill goal-analysis/judge step in the flowdef.

## Convergence

The deep pipeline converges on the THESIS quality gate verdict: investment_grade = 0.0 (fully converged), needs_work = 0.5 (re-enter COMPANY with THESIS gaps injected), incomplete = 1.0 (escalate). max_iterations: 3 bounds the loop.

## Cross-Skill Composition

- Step 5 reuses `goal-analysis/judge` (semantic evaluation of the thesis against the three-pillar investment_grade criteria). This avoids the LLM-improves-against-LLM-scored-target trap per .rules — the quality gate is grounded in goal-analysis's semantic evaluator, not LLM self-assessment.

## MCP Tool Integration

All MCP tool calls are native `action: execute` steps (deterministic, governed, testable). See `kask/docs/explanation/skill-mcp-integration.md` for the two invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Constraints

- The registry crate (`kask/registry/templates/company-research/`) is the canonical source of truth. This SKILL.md is a derived companion.
- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing THESIS verdict surfaces as 1.0 (worst case), not silently converged.
- The THESIS quality gate uses `goal-analysis/judge` (semantic evaluation), not self-assessment — to avoid the LLM-improves-against-LLM-scored-target trap.
- IMAGINE's falsifiable predictions are persisted to the forecast-ledger (when built — Phase 3) for later Brier scoring.
