---
name: company-research-flash
visibility: public
description: "Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Sequential 29-step flowdef: SCOUT alpha score → INTEL business context + earnings listening + pragmatic-semantics certainty classification → FORENSIC pre-screen → CRITICAL FACTOR Bull/Base/Bear → FORENSIC full audit → VALUATION 8-step (4 native MCP tool calls + LLM synthesis) → COMMUNICATION ENTER gate + CASCADE note → KATA PDCA (2 native MCP tool calls + cross-skill reuse) + metacognition calibration gap measurement → LENS five-framework audit (2 native MCP tool calls + LLM synthesis) → convergence check → loop on PARTIAL. MCP tool calls are native action: execute steps; templates do LLM synthesis over their outputs. Early-exit gates (DROP / HALT / BLOCK) are condition: on downstream steps. Converges on LENS verdict consistency."
---

# Company Research — Flash Pipeline

Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Sequential 14-step flowdef producing a flash note / initiation report. MCP tool calls (dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation, market_check_resolutions, market_calibration, market_match, evaluate_evidence, company_transcript, scenario_build, research_search, web_search) are native `action: execute` flowdef steps; templates do LLM synthesis over their outputs.

## When to Use

- When you need a flash note or initiation report for an equity ticker, following the Valentine × Gunn Dual-Mode Framework.
- When you want the full EFRA-AI main pipeline (SCOUT → INTEL → FORENSIC × 2 → CRITICAL FACTOR → VALUATION → COMMUNICATION → KATA → LENS) as a single governed cascade.
- When you want deterministic, testable MCP tool calls (DCF, comparables, expectations gap, scenario-weighted PT) rather than LLM-mediated tool use.
- When you want the forecast-to-outcome calibration loop (market_check_resolutions → KATA PDCA "check") that EFRA-AI's KATA agent describes but cannot close.
- When you want the LENS five-framework consistency audit (The Loop, Superforecasting, Dunning-Kruger, Hidden Champions, Kauffman) as a convergence signal.

## Instructions

### scout-alpha-score

1. Compute the alpha score (coverage gap × 0.30 + market cap fit × 0.20 + sector relevance × 0.25 + valuation anomaly × 0.25, + EM GDP / Bessembinder / low-coverage bonuses up to +25).
2. Apply the 11-criterion excellence universe (S1–S11) if `in_excellence_universe` is true.
3. Emit decision (MUST_COVER / REVIEW_ZONE / DROP — DROP is terminal), alpha_score, horizon_tag, downstream_mode.

### intel-mosaic

1. Synthesize research_search and web_search MCP tool outputs into a business context 8-step (identity, geography, business model, competitive position, customers, management, risks, catalysts).
2. Build the information mosaic with source-tier classification and horizon tagging.
3. Form 3–5 testable hypotheses with PENDING / VALIDATED / UNRESOLVABLE lifecycle.
4. Emit mosaic_clear (false = MNPI HALT terminal gate), business_model, news_items, hypotheses, data_gaps.

### intel-semantic-classify

1. Classify every news_item and hypothesis by ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force, and provenance.
2. Emit semantic_tags and certainty_drift_risk (low/medium/high).
3. Prevents certainty-level drift — a management quote treated as an ontological fact, a scenario treated as a forecast.

### forensic-pre-screen

1. Quick risk scan across accounting red flags, governance, going-concern, regulatory, management integrity.
2. Assign the single highest-severity finding (SEV-1..5).
3. Emit recommendation (CLEAR+adj / CONDITIONAL / BLOCK — BLOCK is terminal). FORENSIC cannot be skipped.

### critical-factor

1. Identify 3–5 critical factors that drive EPS or the multiple.
2. Construct Bull/Base/Bear scenarios with granular probabilities (not round numbers) and EPS impact.
3. Emit impact_mappings (per-node DCF assumption deltas for scenario_impact_valuation).
4. Empty factors = DROP terminal gate.

### forensic-full

1. Full audit: accruals quality, governance (board independence, COB/CEO separation), management profile (owner-operator, capital allocation track record).
2. Consume company_transcript MCP tool output for verbatim management quotes.
3. Emit recommendation (BLOCK terminal gate), management_quality, governance_score, accruals_score.

### valuation-8step

1. Synthesize over four MCP tool outputs (dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation).
2. Produce pt_12m as a weighted blend of the tool outputs.
3. Compute rr_ratio and rating (BUY/HOLD/UNDERPERFORM).
4. Compute FaVeS (variant expectations score — where your thesis differs from the market).
5. Emit data_gaps for any failed MCP tool with LLM-derived fallback estimate + confidence penalty (L1/L2 fallback hierarchy).
6. RR < 2:1 + UNDERPERFORM = DROP terminal gate.

### communication-enter

1. Score the ENTER gate (Edge / New / Timely / Examples / Revealing — 5/5 = PUBLISH, 4/5 = ALERT, ≤3/5 = DROP).
2. Draft the CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data, 300–500 words).
3. Compute final_confidence (blend of VALUATION confidence, FORENSIC severity, FaVeS).
4. Confidence < 0.50 = NO_PUBLISH. publication_possible = false is a terminal DROP (KATA and LENS skip).

### lens-five-frameworks

1. Apply the five intellectual frameworks: The Loop (economic potential, variant expectations, valuation anchor), Superforecasting (granular probabilities, outside view via market_match, certainty-level drift via semantic_tags), Dunning-Kruger (process_confidence vs final_confidence gap, calibration_gap from kata-calibration-measure), Hidden Champions (Simon 8 characteristics), Kauffman (ergodic vs nonergodic, adjacent possible).
2. Reason over market_match (step 10) and evaluate_evidence (step 11) outputs plus all prior pipeline outputs.
3. Emit overall_verdict (CONSISTENT / PARTIAL / INCONSISTENT — the convergence signal), key_tensions, pm_memo (200 words). Never blocks publication.

### kata-calibration-measure

1. Close the open kata loop — step 22 (kata-improvement-step1-direction) sets the direction but never measures the gap.
2. Measure the analyst's calibration gap using the market_calibration Brier score (step 20) and resolved_outcomes (step 19).
3. Emit calibration_gap (0.0 calibrated → 1.0 maximum gap). No prediction recorded = 1.0 (broken feedback loop, not neutral).
4. LENS consumes calibration_gap as a 6th axis alongside the existing five frameworks.

## Convergence

The flash pipeline converges on LENS verdict consistency: CONSISTENT = 0.0 (fully converged), PARTIAL = 0.5 (re-enter VALUATION synthesis with LENS tensions injected), INCONSISTENT = 1.0 (escalate). max_iterations: 3 bounds the loop.

## Cross-Skill Composition

- Step 3b reuses `listening/apply-template` (MAIA v3 earnings-call listening, no-fabrication invariant).
- Step 5 reuses `pragmatic-semantics/semantics-classify-statement` (via `company-research/intel-semantic-classify` adapter) — classifies intel items by IS/OUGHT, declarative/probabilistic/subjunctive before downstream steps consume them.
- Step 9c reuses `kata-improvement/improvement-step1-direction` (Toyota Improvement Kata step 1).
- Step 21 reuses `metacognition/meta-experiment` (via `company-research/kata-calibration-measure` adapter) — closes the open kata loop by measuring the calibration gap using the market_calibration Brier score.

## MCP Tool Integration

All MCP tool calls are native `action: execute` steps (deterministic, governed, testable). See `kask/docs/explanation/skill-mcp-integration.md` for the two invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Constraints

- The registry crate (`kask/registry/templates/company-research/`) is the canonical source of truth. This SKILL.md is a derived companion.
- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing LENS verdict surfaces as 1.0 (worst case), not silently converged.
- The THESIS quality gate in the deep pipeline uses `goal-analysis/judge` (semantic evaluation), not self-assessment — to avoid the LLM-improves-against-LLM-scored-target trap.
