---
name: company-research-flash
description: "Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Sequential 23-step process: SCOUT alpha score → INTEL business context + earnings listening + pragmatic-semantics certainty classification → FORENSIC pre-screen → CRITICAL FACTOR Bull/Base/Bear → FORENSIC full audit → VALUATION 8-step (batch of 4 MCP tool calls + LLM synthesis) → COMMUNICATION ENTER gate + CASCADE note → KATA PDCA (2 MCP tool calls + cross-skill reuse) + metacognition calibration gap measurement → LENS five-framework audit (2 MCP tool calls + LLM synthesis) → convergence check → loop on PARTIAL → forecast persist. Early-exit gates (DROP / HALT / BLOCK) gate downstream steps. Converges on LENS verdict consistency."
---

# Company Research — Flash Pipeline

Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Sequential 23-step flowdef producing a flash note / initiation report. MCP tool calls (forecast_list, research_search, web_search, company_transcript, scenario_build, dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation, market_check_resolutions, market_calibration, market_match, evaluate_evidence, forecast_persist) are called directly; templates do LLM synthesis over their outputs.

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
2. Reason over market_match and evaluate_evidence outputs (step 18 mcp_batch) plus all prior pipeline outputs.
3. Emit overall_verdict (CONSISTENT / PARTIAL / INCONSISTENT — the convergence signal), key_tensions, pm_memo (200 words). Never blocks publication.

### kata-calibration-measure

1. Close the open kata loop — step 16 (kata-improvement-step1-direction) sets the direction but never measures the gap.
2. Measure the analyst's calibration gap using the market_calibration Brier score and resolved_outcomes (step 15 mcp_batch).
3. Emit calibration_gap (0.0 calibrated → 1.0 maximum gap). No prediction recorded = 1.0 (broken feedback loop, not neutral).
4. LENS consumes calibration_gap as a 6th axis alongside the existing five frameworks.

## Convergence

The flash pipeline converges on LENS verdict consistency: CONSISTENT = 0.0 (fully converged), PARTIAL = 0.5 (re-enter VALUATION synthesis with LENS tensions injected), INCONSISTENT = 1.0 (escalate). max_iterations: 3 bounds the loop.

## Cross-Skill Composition

- Step 6 reuses the full `listening` skill as a sub-flowdef (MAIA v3 earnings-call listening, no-fabrication invariant).
- Step 4 reuses `pragmatic-semantics/semantics-classify-statement` (via `company-research/intel-semantic-classify` adapter) — classifies intel items by IS/OUGHT, declarative/probabilistic/subjunctive before downstream steps consume them.
- Step 16 reuses `kata-improvement/improvement-step1-direction` (Toyota Improvement Kata step 1).
- Step 17 reuses `metacognition/meta-experiment` (via `company-research/kata-calibration-measure` adapter) — closes the open kata loop by measuring the calibration gap using the market_calibration Brier score.

## Registry Templates

All templates live in the shared `kask/registry/templates/company-research/` crate (used by both the flash and deep pipelines):

| Template | Type | Purpose |
|----------|------|---------|
| `scout-alpha-score.j2` | WordAct | Agent 01 SCOUT. Computes the firm-specific alpha score (coverage gap × 0.30 + market cap fit × 0.20 + sector relevance × 0.25 + valuation anomaly × 0.25, plus EM GDP / Bessembinder / low-coverage bonuses up to +25) and applies the 11-criterion excellence universe (S1–S11) where `in_excellence_universe` is true. Emits `decision` (MUST_COVER / REVIEW_ZONE / DROP), `alpha_score`, `horizon_tag`, `downstream_mode` (valentine / gunn / dual). DROP is a terminal early-exit gate. |
| `intel-mosaic.j2` | WordAct | Agent 02 INTEL (DEEPEN). Business-context 8-step + information mosaic. Consumes `research_search` and `web_search` MCP tool outputs (bound via input_mapping from prior direct tool calls) and the `listening/apply-template` earnings-call verdict (cross-skill step 3). Emits `mosaic_clear` (false = MNPI HALT terminal gate), `business_model`, `news_items`, `hypotheses` (PENDING / VALIDATED / UNRESOLVABLE lifecycle), `data_gaps`. Per .rules: failed MCP tools surface as `data_gaps` entries, never collapse to None. |
| `forensic-pre-screen.j2` | WordAct | Agent 04 FORENSIC (pre-screen). Quick risk pre-screen across accounting red flags, governance, going-concern signals. Emits `severity` (SEV-1 minor → SEV-5 fraud/restatement), `recommendation` (CLEAR+adj / CONDITIONAL / BLOCK), `eps_haircut`, `dr_add_bps`. BLOCK is a terminal early-exit gate. FORENSIC cannot be skipped (EFRA-AI invariant). |
| `critical-factor.j2` | WordAct | Agent 03 CRITICAL FACTOR. Identifies the 3–5 critical factors that drive the business and constructs Bull / Base / Bear scenarios with EPS impact. Consumes `scenario_build` MCP tool output (bound via input_mapping) for structured scenario generation. Emits `factors` (empty = DROP terminal gate), `scenarios` (bull/base/bear with probabilities and EPS impact), `eps_impact_pct`. Cross-references `superforecasting/stage_3_probability_estimate` methodology for granular (0.35) vs round (0.50) probabilities. |
| `forensic-full.j2` | WordAct | Agent 04 FORENSIC (full). Full audit: accruals quality, governance (board independence, COB/CEO separation), management profile (owner- operator, capital allocation track record). Consumes `company_transcript` MCP tool output for management quotes. Emits `severity`, `recommendation` (BLOCK terminal gate), `management_quality`, `governance_score`, `accruals_score`. FORENSIC cannot be skipped. |
| `valuation-8step.j2` | WordAct | Agent 05 VALUATION (DEEPEN). 8-step price target engine. Synthesizes over four direct MCP tool outputs bound via input_mapping: `dcf_valuation` (7a), `comparable_analysis` (7b), `expectations_gap` (7c), `scenario_impact_valuation` (7d). Emits `pt_12m`, `rr_ratio`, `rating` (BUY/HOLD/UNDERPERFORM), `FaVeS` (variant expectations score), `confidence`, `data_gaps` (names any failed MCP tool with LLM-derived fallback estimate + confidence penalty per EFRA-AI L1/L2 fallback hierarchy). RR < 2:1 + UNDERPERFORM = DROP terminal gate. |
| `communication-enter.j2` | WordAct | Agent 06 COMMUNICATION. ENTER gate (Edge / New / Timely / Examples / Revealing — 5/5 = PUBLISH, 4/5 = ALERT, ≤3/5 = DROP) and CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data). Emits `publication_possible`, `enter_score`, `cascade_note`, `final_confidence`. Confidence < 0.50 = NO_PUBLISH (EFRA-AI invariant). |
| `lens-five-frameworks.j2` | KnowAct | Agent 09 LENS. Consistency auditor. Applies the firm's five intellectual frameworks: Lens 1 The Loop (economic potential, technological capability, variant expectations, valuation anchor Value = Profits / (r − g), target return > 12%, max P/E < 25×), Lens 2 Superforecasting (granular probabilities, inside/outside view balance, clashing forces, observable invalidation — cross-references `market_cmp` outside view), Lens 3 Dunning-Kruger (process_confidence vs final_confidence gap, overconfidence risk flag), Lens 4 Hidden Champions (Simon 8 characteristics), Lens 5 Kauffman / Adjacent Possible (ergodic vs nonergodic, new niches, Darwinian preadaptations). Emits `overall_verdict` (CONSISTENT / PARTIAL / INCONSISTENT), `key_tensions`, `pm_memo` (200 words). Never blocks publication. |
| `company-8part.j2` | WordAct | Agent 13 COMPANY (DEEPEN). Deep 8-part company analysis: Self-View, Business Franchise, Management Skill (CEO long-term + CFO working capital scorecards), Financial Profile (signposts + 3-stage valuation), Invisible Layer, Turd Blossom, Value Gorilla Elevator Pitch, Investment Thesis Statement. Consumes `company_transcript`, `dcf_valuation`, `comparable_analysis`, `web_search`, `fetch` MCP tool outputs (bound via input_mapping from prior direct tool calls). Emits `CompanyBoard` with all 8 sections, `data_gaps`. |
| `falstaffian-competitive-rotation.j2` | KnowAct | v0.36.0 addition. Rotates the competitive framing of the Company Board before GORILLA scores it. Applies Falstaffian semantic rotation shapes (predicate hollow, subject expansion, object inversion, direction reversal) to expose framing errors in the analyst narrative. Emits rotated_board with competitor-complement analysis, market creator vs participant classification (Wardley evolution axis), framing errors detected, and rotated competitive position. Anchored to MAIA "Falstaff: Give Me Life", "Competition: Readings vs Reality", "Company Analysis", "Thinking Like an Owner". Cross-references metacognition/falstaffian-perspective-engine shapes and decision tree. |
| `gorilla-4dim.j2` | WordAct | Agent 10 GORILLA. Value Gorilla 4-dimension framework with fixed methodology weights (Obvious Problem 25% / Invisible Gorilla 30% / Combinatorial Solution 25% / Choke Point 20%). Weights are fixed by firm methodology — NOT user-tunable, so mcda was rejected (essentialist Surface gate: adds ceremony for fixed weights). Scoring is a `lisp_eval` call, not an mcda call. In v0.36.0, GORILLA consumes the ROTATED board (from falstaffian-competitive- rotation), not the raw Company Board — the rotation corrects framing errors before scoring. Emits `gorilla_score`, `verdict` (GORILLA ≥75 / SMALL_ANIMAL 50-74 / PEDESTRIAN <50), per-dimension scores. |
| `economic-trajectory.j2` | KnowAct | v0.36.0 addition. Economically-anchored imagination scaffold. Identifies the falling-cost trajectory in the subject's industry (McAfee dematerialization), the design constraint being removed (MAIA bottleneck framework), the Coasean firm-boundary shifts (Kauffman economic web), the Kauffman adjacent possible nodes (never-before-born goods and services, Darwinian preadaptations), and convergence vectors (Diamandis). Emits economic_trajectory with falling_cost, constraint_being_removed, coasean_shifts, adjacent_possible_nodes, convergence_vectors, implications_for_ subject, trajectory_velocity. IMAGINE consumes this as the anchor for its 5/10/20Y scenarios. Anchored to MAIA "Focus and Imagination", "More From Less", "Kauffman Readings", "The Future Is Faster", "Bottlenecks and Critical Mass", "Time Horizons". |
| `imagine-longrange.j2` | WordAct | Agent 11 IMAGINE. Projects the business at 5, 10, 20 years and walks it back analytically. Digital Transformation Stages (MODEL / SHADOW / TWIN / SOURCE), Growth Driver Classification (innovation / demographic / both / neither). In v0.36.0, scenarios are ANCHORED on the economic trajectory probe (falling cost, constraint removal, adjacent possible) and CHALLENGED by the Falstaffian rotations (rotated competitive framing, framing errors detected). Consumes `scenario_build` MCP tool output and the `economic_trajectory` probe. Emits `ImagineBoard` with digital stage, growth driver, 3 scenarios (each with trajectory_anchor and falstaffian_challenge), 3–5 falsifiable predictions (tagged by horizon, each with trajectory_ basis), what's not on the page (anchored on adjacent possible), what's not in the price (anchored on trajectory implications), trajectory_anchoring, falstaffian_challenge. |
| `thesis-three-pillars.j2` | KnowAct | Agent 12 THESIS. Synthesizes all prior research into a formal investment thesis covering the three pillars: Business Franchise (moat strength, value creation, durability), Management Quality (capital allocation, leadership), Valuation (3-stage: consensus → normalization → terminal). Quality gate verdict `investment_grade` / `needs_work` / `incomplete` is the deep pipeline convergence signal. Per .rules (LLM-improves-against-LLM-scored-target trap): the quality gate uses `goal-analysis` semantic evaluation, not self-assessment — wired as a cross-skill `render_template` call, not inside this template. |
| `intel-semantic-classify.j2` | KnowAct | v0.36.0 cross-skill adapter. Adapts pragmatic-semantics/ semantics-classify-statement to the INTEL mosaic. Classifies every news_item and hypothesis by ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force, and provenance — BEFORE downstream steps consume the intel. Prevents certainty-level drift: a management quote treated as an ontological fact, a scenario treated as a forecast. Emits semantic_tags and certainty_drift_risk that downstream templates (forensic, critical- factor, valuation) consume via intel_bundle.semantic_tags. |
| `gorilla-capability-reason.j2` | KnowAct | v0.36.0 cross-skill adapter. Adapts capabilities-reasoner/ capability-reason to the GORILLA 4-dim framework. Types each GORILLA dimension (Obvious Problem, Invisible Gorilla, Combinatorial Solution, Choke Point) against a capability registry with floor, ceiling, and maturity-gate limits. The GORILLA score (0–100) is the elicited potential; the capability assessment determines whether that score is credible against the company's observed behavior and maturity. Emits capability_assessments, floor_violations, ceiling_violations, maturity_blocks. A maturity block on a dimension means the `lisp_eval` scoring call should treat that dimension's score as nil, not as the elicited value. |
| `thesis-essentialist.j2` | KnowAct | v0.36.0 cross-skill adapter. Adapts essentialist/essentialist-flow to the three-pillar investment thesis. Runs a single pass of the 3-gate protocol (Exist, Surface, Contract) on the thesis to enforce parsimony — does each pillar earn its place? Is the thesis at the right abstraction level? Can it be stated more tersely? Mode is autonomous (no human in the loop during the pipeline). The elimination_report feeds the goal-analysis quality gate as additional evidence — it does not block the thesis directly. |
| `kata-calibration-measure.j2` | KnowAct | v0.36.0 cross-skill adapter. Adapts metacognition/meta-experiment to close the flash pipeline's open kata loop. Flash step 20 (kata- improvement-step1-direction) sets the direction but never measures the gap. This step measures the analyst's calibration gap using the market_calibration Brier score (step 19) and resolved_outcomes (step 18), then re-measures the current condition. Emits calibration_gap (0.0 calibrated → 1.0 maximum gap) that LENS (step 23) consumes as a 6th axis alongside the existing five frameworks. |
| `wardley-anchor.j2` | KnowAct | v0.36.0 cross-skill adapter. Compresses wardley-mapper's 6-step cascade (inventory → classify → map → movement → recommendations → present) into a single LLM call over the rotated Company Board. Emits wardley_map with components, evolution classifications, movements, commoditization candidates, choke_points, and invisible_gorillas. Feeds GORILLA's Invisible Gorilla and Choke Point dimensions (step 5) and ECONOMIC TRAJECTORY's falling-cost anchor (step 9). The full wardley-mapper skill is available for standalone use — this adapter exists to ground the deep pipeline's strategic analysis without adding a 6-step sub-cascade. |
| `wardley-anchor.j2` | KnowAct | v0.36.0 cross-skill adapter. Compresses wardley-mapper's 6-step cascade (inventory → classify → map → movement → recommendations → present) into a single LLM call over the rotated Company Board. Emits wardley_map with components, evolution classifications, movements, commoditization candidates, choke_points, and invisible_gorillas. Feeds GORILLA's Invisible Gorilla and Choke Point dimensions (step 5) and ECONOMIC TRAJECTORY's falling-cost anchor (step 9). The full wardley-mapper skill is available for standalone use — this adapter exists to ground the deep pipeline's strategic analysis without adding a 6-step sub-cascade. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

The deep-only templates (`company-8part.j2`, `falstaffian-competitive-rotation.j2`, `wardley-anchor.j2`, `gorilla-4dim.j2`, `gorilla-capability-reason.j2`, `economic-trajectory.j2`, `imagine-longrange.j2`, `thesis-three-pillars.j2`, `thesis-essentialist.j2`) are documented in the `company-research-deep` SKILL.md.

## MCP Tool Integration

All MCP tool calls are called directly (deterministic, governed, testable). See `kask/docs/explanation/skill-mcp-integration.md` for the two invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing LENS verdict surfaces as 1.0 (worst case), not silently converged.
- The THESIS quality gate in the deep pipeline uses `goal-analysis/judge` (semantic evaluation), not self-assessment — to avoid the LLM-improves-against-LLM-scored-target trap.
