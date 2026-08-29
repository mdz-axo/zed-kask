---
name: company-research-deep
description: "Equity research deep pipeline converted from EFRA-AI (Replicant-Partners). Sequential 13-step process following the scientific method: COMPANY deep 8-part analysis (observation) → VERIFY early anchor (foundation grounding) → FALSTAFFIAN competitive perspective rotation (hypothesis framing) → WARDLEY compressed value-chain map (structural analysis) → ECONOMIC TRAJECTORY probe with strategy-literature search (theoretical framework) → GORILLA 4-dimension framework (hypothesis evaluation, fixed weights, `lisp_eval` scoring) + capabilities-reasoner floor/ceiling/maturity assessment (falsification) → IMAGINE 5/10Y scenarios + digital stage (prediction, scenario_build MCP tool call + LLM synthesis) → THESIS three-pillar synthesis (conclusion) + essentialist 3-gate eliminative interrogation (parsimony) → VERIFY late gate (full-pipeline grounding) → PERSIST report to companies MCP server → CONDENSE to ≤5000-word human-readable summary. Converges on THESIS investment_grade verdict."
---

# Company Research — Deep Pipeline

Equity research deep pipeline converted from EFRA-AI (Replicant-Partners). Sequential 16-step process producing a deep company analysis and investment thesis. MCP tool calls (company_transcript, dcf_valuation, comparable_analysis, web_search, scenario_build) are called directly; templates do LLM synthesis over their outputs.

## When to Use

- When you need a deep company analysis and investment thesis for an equity ticker, following the Valentine × Gunn Dual-Mode Framework.
- When you want the full EFRA-AI research pipeline (COMPANY → GORILLA → IMAGINE → THESIS) as a single governed process.
- When you want the 8-part company analysis (Self-View, Business Franchise, Management Skill, Financial Profile, Invisible Layer, Falstaffian Inversion, Value Gorilla Elevator Pitch, Investment Thesis Statement).
- When you want the GORILLA 4-dimension framework (Obvious Problem / Invisible Gorilla / Combinatorial Solution / Choke Point) with fixed methodology weights.
- When you want IMAGINE's 5/10Y scenarios with digital transformation stage classification and falsifiable predictions persisted to the forecast-ledger.
- When you want the THESIS three-pillar synthesis (Business Franchise, Management Quality, Valuation) with a cross-skill goal-analysis quality gate.

## Instructions

### Data Provenance Hierarchy (applies to all steps)

All financial data used in the pipeline must be classified by provenance strength. Stronger provenance overrides weaker provenance when they conflict.

| Level | Source | Legal standing | Use as DCF/valuation input? |
|-------|--------|---------------|---------------------------|
| 1 | Audited financial statements (10-K, 10-Q) | Audited, legal penalties for misrepresentation | **Yes — these are the anchor** |
| 2 | Earnings call transcripts | Regulation FD applies, but forward-looking statements have safe harbor | No — use for qualitative analysis, management assessment, milestone tracking |
| 3 | Press releases / investor presentations | Selective disclosure, not audited | No — use for context, not for financial inputs |
| 4 | Media coverage / analyst reports | Third-party interpretation, may repeat company claims | No — use for incumbent research, market context |
| 5 | Web research / news | Variable quality | No — use for market-reality gate, incumbent verification |

**DCF inputs must come from Level 1 (audited financial statements) only.** If audited data is insufficient for a meaningful DCF — e.g., pre-revenue company with no historical revenue, margins, or working capital data — the correct output is "insufficient audited data for DCF," not substituting analyst estimates, round-number guesses, or transcript claims as inputs. Swapping in non-audited data as DCF inputs pollutes the calculation and produces a number that has the appearance of rigor without the substance.

**Customer base evidence** must distinguish between:
- Revenue from paying customers (Level 1 — from audited financials)
- Customer/partner counts cited in transcripts (Level 2 — management claim, not audited)
- Partnership announcements in press releases (Level 3 — selective disclosure)

A company citing "60+ MNO partners covering 3B+ subscribers" in a transcript (Level 2) does not constitute evidence of paying customers. Only recognized revenue from audited financials (Level 1) verifies that customers pay for the product.

### company-8part

1. Self-View — how the company describes itself (verbatim from fetched IR page).
2. Business Franchise — identity, geography, moat, competitive position.
3. Management Skill — CEO scorecard (long-term) + CFO scorecard (working capital) with verbatim transcript quotes.
4. Financial Profile — signposts + 3-stage valuation (consensus → normalization → terminal).
5. Invisible Layer — what's not on the page, what's not in the price.
6. Falstaffian Inversion — pierce market ceremony and social illusion (honour is "air"); find economic life where the consensus disdains to look (the "spare men"); early shoots of improvement the consensus misses.
7. Strategic Position Summary — what problem exists (verified), what the company claims, what evidence supports or contradicts.
8. Investment Thesis Statement — state whether evidence supports a thesis. If insufficient, say so.
9. Consume company_transcript, dcf_valuation, comparable_analysis, web_search, fetch MCP tool outputs.
10. Emit data_gaps for any failed MCP tool — never collapse to None.
11. **Intent-routed search:** Before calling `web_search`, call `web_recommend_provider` with the query and `intent` hint. Use the recommended provider explicitly via `web_search(provider="...")` rather than defaulting to RRF fusion. This prevents the arxiv-only fallback that occurs when no provider is explicitly selected and the credential map is stale.
12. **Earnings call transcripts:** If `company_transcript` is unavailable, search for `"{company} earnings call transcript Q{N} {year}"` via `web_search(provider="serpapi")` — SerpAPI supports YouTube transcript extraction. Fall back to `web_search(provider="tavily")` for written transcript sources. Never substitute Wikipedia-sourced paraphrases for verbatim management quotes without flagging a `data_gap`.

### verify-early-anchor

1. Verify that the CompanyBoard's factual claims and cited quotes are grounded in the MCP tool outputs and transcript consumed by company-8part. This is the early anchor — it catches foundation hallucinations BEFORE they contaminate downstream steps (Falstaffian rotation, Wardley map, GORILLA, IMAGINE, THESIS).
2. Call `spawn_agent` with a message that invokes the `grounding-verify` skill:
   - label: "verify-early-anchor"
   - message: instruct the agent to run the `grounding-verify` skill on the CompanyBoard output against all step-1 MCP tool outputs. Provide the CompanyBoard text as `target_text`, the MCP tool outputs as `source_outputs`, the pipeline tool call log as `pipeline_tool_log`, and the domain-specific leak rules for equity research as `leak_rules`.
3. The spawned agent is decoupled from the company-8part generator — it has no shared conversation history (self-improvement §9.1). It receives only the stage output and the source outputs.
4. Consume the `fact_score`, `verified_claims`, `data_gaps`, `confidence_adjustment`, and `confidence_band` from the spawned agent's output.
5. If `fact_score < 0.60`: block downstream steps, re-enter company-8part with fact-check gaps injected into the convergence loop. If `fact_score = nil`: emit `data_gap: "fact_score_early_measurement_failed"` + confidence penalty -0.20, proceed with caution. If `0.60 ≤ fact_score < 0.80`: proceed, emit `data_gaps` for failed sub-metrics, apply confidence penalty -0.10. If `fact_score ≥ 0.80`: proceed with no penalty.
6. The `verified_claims` registry from this step is append-only — downstream steps and the late gate can read it but cannot modify it. This prevents the un-stripping trap.

### falstaffian-competitive-rotation

1. Rotate the competitive framing of the Company Board before GORILLA scores it — structural defense against frame capture by analyst narratives.
2. Apply Falstaffian semantic rotation shapes (predicate hollow, subject expansion, object inversion, direction reversal) to expose framing errors.
3. Emit rotated_board with competitor-complement analysis, market creator vs participant classification, framing errors detected, and rotated competitive position.
4. GORILLA consumes the rotated board, not the raw Company Board.

### wardley-anchor

1. Compressed value-chain map of the rotated Company Board — compresses wardley-mapper's 6-step process into a single LLM call.
2. **Inventory two classes of components:**
   - **Internal value-chain components** — what the company builds. Classify each on the Wardley evolution axis (Genesis → Custom → Product → Commodity).
   - **Claimed market categories** — every market or capability the company claims to own, create, or be unique in. For each, search via `web_search` for incumbents who already sell it as a product line. Classify based on **what incumbents do**, not what the company claims. If incumbents sell it as a product line, it is at Product/Commodity stage — flag it as a commoditization red flag.
3. Surface commoditization candidates (including any claimed market found to be at Product/Commodity stage), choke points, and invisible gorillas.
4. GORILLA consumes choke_points, invisible_gorillas, AND commoditization_candidates — a claimed market at Product/Commodity stage caps the Obvious Problem score (the company is entering an existing market, not solving a new problem). ECONOMIC TRAJECTORY consumes commoditization_candidates as the falling-cost anchor.
5. **Conditional full Wardley:** If the compressed choke_point score (from GORILLA, evaluated after this step) is < 60, re-run with the full `wardley-mapper` skill (6-step process: inventory → classify → map → movement → recommendations → present). The full map adds movement analysis (component velocity on the evolution axis) and visual positioning that the compressed adapter cannot produce. This conditional trigger avoids convergence fatigue on clear-cut cases while preserving depth when the strategic position is uncertain.

### economic-trajectory

1. Identify the falling-cost trajectory in the subject's industry, the design constraint being removed, the Coasean firm-boundary shifts, and the Kauffman adjacent possible nodes.
2. Consume the Wardley map's commoditization_candidates as the falling-cost anchor — what's becoming commodity IS the falling-cost trajectory.
3. **Strategy-literature-probe:** Before asserting Coasean shifts or adjacent-possible nodes, search for grounding literature via `web_search(provider="exa")` with semantically-framed queries: `"industrial organization market structure entry barriers {industry}"`, `"case study vertical integration {industry} {strategy}"`, `"platform economics two-sided market {business_model}"`. Exa's neural search finds literature by meaning, not keyword. Consume the results as evidence for the trajectory claims — do not assert theoretical grounding without a source. Emit `literature_sources` with each trajectory claim.
4. Emit economic_trajectory with falling_cost, constraint_being_removed, coasean_shifts, adjacent_possible_nodes, convergence_vectors, implications_for_subject, literature_sources.
5. IMAGINE consumes this as the anchor for its 5/10Y scenarios.

### gorilla-4dim

1. **Market-reality gate.** For each market the company claims to own or have created, search via `web_search` for incumbents who sell it as a product line. Output: `claimed market | incumbent | what they sell | URL`. If incumbents exist, cap Obvious Problem at 40. If no incumbents found after searching, state this explicitly and score normally.
2. **Financial red-flag screen.** Answer two questions using Level 1 (audited financial statement) data only:
   - Is capex producing revenue? (Compare capex trajectory to revenue trajectory from 10-K/10-Q filings.)
   - Is revenue real? (Check whether reported revenue is from paying customers or from contracts/MoUs. Audited revenue is recognized revenue — not backlog, not contracted, not "committed.")
   If audited data is insufficient to answer either question, state "insufficient audited data" and apply a confidence penalty. If either answer is negative based on audited data, apply a confidence penalty to all GORILLA scores.
3. **Falsification before scoring above 70.** Write one paragraph arguing against any dimension scored above 70. If the counter-argument is more convincing than the supporting evidence, reduce the score.
4. Score the 4 dimensions (Obvious Problem, Invisible Gorilla, Combinatorial Solution, Choke Point) against the ROTATED Company Board, the Wardley map, the economic trajectory, the market-reality gate, and the financial red-flag screen.
5. Every score must cite evidence: transcript quote, web source URL, or financial data point. A score without cited evidence is invalid.
6. Call `lisp_eval` to apply fixed weights (25/30/25/20) and compute the verdict (GORILLA ≥75 / SMALL_ANIMAL 50-74 / PEDESTRIAN <50).
7. Do NOT propose alternative weightings.

### gorilla-capability-reason

1. Type each GORILLA dimension against a capability registry with floor, ceiling, and maturity-gate limits.
2. **Execution evidence requirement.** For each dimension scored above 50, cite one prior-quarter guidance item or stated milestone from the Company Board and whether it was delivered. The comparison is: `prior guidance | delivered? | evidence`. If delivery is absent across 2+ dimensions, apply a maturity block — the score is treated as nil, not as the elicited value. This distinguishes companies that see an opportunity and are executing from companies that see an opportunity and are not.
3. **Customer base evidence requirement.** The most valuable asset any business has is its paying customer base (Reichheld: loyal, low-turnover customers drive ROIC by reducing acquisition costs and enabling referral-driven growth). For each dimension scored above 50, cite evidence of paying customers using the Data Provenance Hierarchy:
   - **Paying customers (Level 1 — audited)**: recognized revenue from 10-K/10-Q, customer counts disclosed in audited filings, ARPU calculated from audited revenue ÷ customer count
   - **Non-paying relationships (Level 2-3 — unaudited)**: partnerships, MoUs, letters of intent, "agreements" cited in transcripts or press releases that have not converted to recognized revenue
   If the company has no Level 1 evidence of paying customers but claims a large partner ecosystem (Level 2-3), apply a maturity block on Obvious Problem and Combinatorial Solution. A company that cannot get people to pay for its product has not demonstrated the capability to monetize the opportunity, regardless of opportunity size.
   Cross-reference the `listening` skill output for multi-quarter trajectory: are customer counts growing? Are partners converting to paying customers? Is revenue per customer growing? Persistent partner counts without revenue conversion is a signal that the company sees the opportunity but cannot execute on the most basic business function.
4. Evaluate whether the GORILLA scores are defensible given the company's capability maturity, execution track record, and customer base evidence.
5. Emit floor_violations, ceiling_violations, maturity_blocks. A maturity block means the `lisp_eval` scoring call treats that dimension as nil.

### imagine-longrange

1. Classify the digital transformation stage (MODEL / SHADOW / TWIN / SOURCE).
2. Classify the growth driver (innovation / demographic / both / neither).
3. Construct 5/10Y scenarios using the scenario_build MCP tool output as scaffold, ANCHORED on the economic trajectory and CHALLENGED by the Falstaffian rotations.
4. Emit 3–5 falsifiable predictions tagged by horizon (5Y/10Y) for the forecast-ledger — the IMAGINE kata loop.
5. Emit what's not on the page (anchored on the adjacent possible) and what's not in the price (anchored on the trajectory's implications).

### thesis-three-pillars

1. Synthesize all prior research into a formal investment thesis covering the three pillars: Business Franchise (moat strength, value creation, durability), Management Quality (capital allocation, leadership), Valuation (3-stage: consensus → normalization → terminal).
2. The terminal stage must cross-reference the IMAGINE 10Y scenario.
3. Emit the thesis statement (durable, timeless, covering all three pillars).
4. Do NOT self-evaluate — the investment_grade / needs_work / incomplete verdict comes from the cross-skill goal-analysis/judge step in the process.

### thesis-essentialist

1. Run a single pass of the 3-gate eliminative interrogation (Exist, Surface, Contract) on the thesis.
2. Gate 1 (Exist): delete each pillar — does the thesis collapse? If not, the pillar is decorative.
3. Gate 2 (Surface): count load-bearing claims — ≤ 7 passes, > 7 requires justification.
4. Gate 3 (Contract): trace abstractions — are moat source, durability, and terminal stage genuine content or labels/hedges?
5. The elimination_report feeds the goal-analysis quality gate as additional evidence — it does not block the thesis directly.

### verify-late-gate

1. Verify that the full pipeline report's factual claims (CompanyBoard through THESIS) are grounded in source data. This is the late gate — it catches hallucinations introduced during downstream synthesis (GORILLA, IMAGINE, THESIS) that were not present in the CompanyBoard.
2. Call `spawn_agent` with a message that invokes the `grounding-verify` skill:
   - label: "verify-late-gate"
   - message: instruct the agent to run the `grounding-verify` skill on the full pipeline report (thesis + all prior stage outputs) against all accumulated MCP tool outputs. Provide the thesis text as `target_text`, all MCP tool outputs from the full pipeline as `source_outputs`, the pipeline tool call log as `pipeline_tool_log`, the `verified_claims` registry from the early anchor as `prior_verified_claims`, and the domain-specific leak rules for equity research as `leak_rules`.
3. The spawned agent checks thesis claims against the `verified_claims` registry first (fast path — already-verified claims skip re-verification), then runs the full grounding-verify process on new claims introduced during downstream synthesis.
4. Consume `fact_score_final`, `fact_score_breakdown`, `data_gaps`, `confidence_adjustment`, `confidence_band`, `hallucination_findings`, and `verification_scope_limitations` from the spawned agent's output.
5. `fact_score_final` and `confidence_adjustment` feed the goal-analysis quality gate as additional evidence alongside the essentialist elimination_report.
6. If `fact_score_final < 0.60`: flag `needs_work` with fact-check gaps injected into convergence loop. If `fact_score_final = nil`: surface to quality gate as `incomplete` (cannot evaluate thesis quality without factuality signal). If `0.60 ≤ fact_score_final < 0.80`: proceed to quality gate with confidence penalty -0.10. If `fact_score_final ≥ 0.80`: proceed to quality gate with no penalty.
7. The fact_score does NOT replace the `goal-analysis/judge` semantic evaluation — two independent gates, not one. `fact_score < 0.60` bypasses semantic evaluation (a thesis on hallucinated facts cannot be evaluated for investment quality); `fact_score ≥ 0.60` still requires the semantic quality gate.

### persist-report

1. After the quality gate verdict, write the full report as a **rich markdown file**.
2. Reports are stored in the user-facing artifacts directory: `~/Documents/zk-data/companies-mcp/reports/`. This is separate from the internal data dir (`~/.local/share/zed-kask/`) — reports are user-facing artifacts that should be visible, not buried in a hidden cache directory.
3. Create the reports directory if it does not exist: `mkdir -p ~/Documents/zk-data/companies-mcp/reports` via `terminal`.
4. Write the markdown file to `~/Documents/zk-data/companies-mcp/reports/{ticker}-{date}.md` for single-company analyses, or `~/Documents/zk-data/companies-mcp/reports/{ticker-a}-vs-{ticker-b}-{date}.md` for comparative analyses. Use `write_file` with the full absolute path.
5. The markdown file is the deliverable — full rich markdown with all sections, mermaid diagrams, source notes, and citations.
6. Do NOT write reports to the source tree (`zed-kask/reports/` or similar) — that pollutes the user's code repository.
7. Do NOT write reports to the hidden internal data dir (`~/.local/share/zed-kask/mcp/companies/reports/`) — that buries user-facing output where the user will never find it.

### condense-report

1. The full markdown report written in persist-report IS the deliverable.
2. If it exceeds ~5,000 words, produce a condensed executive summary as a separate markdown file at `~/Documents/zk-data/companies-mcp/reports/{ticker}-summary-{date}.md`.
3. Both files are markdown in `~/Documents/zk-data/companies-mcp/reports/`.

## Convergence

The deep pipeline converges on the THESIS quality gate verdict: investment_grade = 0.0 (fully converged), needs_work = 0.5 (re-enter COMPANY with THESIS gaps injected), incomplete = 1.0 (escalate). max_iterations: 3 bounds the loop.

The fact_score (from verify-late-gate) feeds the quality gate as additional evidence: fact_score < 0.60 triggers needs_work directly (bypasses semantic evaluation — a thesis built on hallucinated facts cannot be evaluated for investment quality). fact_score = nil triggers incomplete (cannot evaluate without factuality signal). fact_score ≥ 0.60 proceeds to semantic evaluation with confidence penalty factored in. The fact_score for pipeline iteration N only counts claims from iteration N — prior iteration failures are reported as historical_findings context, not as current-pipeline failures (cohort scoping).

## Cross-Skill Composition

- Step 3 (FALSTAFFIAN) reuses the falstaffian perspective-engine shapes and decision tree (via `company-research/falstaffian-competitive-rotation`).
- Step 4 (WARDLEY) compresses `wardley-mapper`'s 6-step process via the `company-research/wardley-anchor` adapter. Conditionally upgrades to full `wardley-mapper` when choke_point score < 60.
- Step 5 (ECONOMIC TRAJECTORY) includes a strategy-literature-probe via Exa semantic search for IO/competition economics grounding.
- Step 6 reuses `capabilities-reasoner/capability-reason` (via `company-research/gorilla-capability-reason` adapter) — types each GORILLA dimension against a capability registry with floor/ceiling/maturity-gate limits. Dimensions with maturity blocks are nil'd by the `lisp_eval` scoring call.
- Step 10 reuses `essentialist/essentialist-flow` (via `company-research/thesis-essentialist` adapter) — runs a single pass of the 3-gate eliminative interrogation (Exist, Surface, Contract) on the thesis to enforce parsimony. The elimination_report feeds the goal-analysis quality gate as additional evidence.
- Step 11 (VERIFY-LATE-GATE) invokes `grounding-verify` as a `spawn_agent` call — decoupled from all prior generators. Checks the full pipeline report against all accumulated source outputs and the `verified_claims` registry from the early anchor. The fact_score feeds the `goal-analysis/judge` quality gate as additional evidence.
- The fact_score computation uses `lisp_eval` (deterministic scoring, same pattern as GORILLA's fixed-weight scoring). The provenance lattice and extraction ceiling are adapted from Fermi's `grounding_trust.rs`.
- Step 12 reuses `goal-analysis/judge` (semantic evaluation of the thesis against the three-pillar investment_grade criteria). This avoids the LLM-improves-against-LLM-scored-target trap per .rules — the quality gate is grounded in goal-analysis's semantic evaluator, not LLM self-assessment.
- Steps 12-13 (PERSIST + CONDENSE) write the full markdown report and optional condensed summary to `reports/company-research/` via the built-in `write_file` tool.

## Registry Templates

All templates live in the shared `kask/registry/templates/company-research/` crate (used by both the flash and deep pipelines):

| Template | Purpose |
|----------|---------|
| `scout-alpha-score.j2` | Agent 01 SCOUT. Computes the firm-specific alpha score (coverage gap × 0.30 + market cap fit × 0.20 + sector relevance × 0.25 + valuation anomaly × 0.25, plus EM GDP / Bessembinder / low-coverage bonuses up to +25) and applies the 11-criterion excellence universe (S1–S11) where `in_excellence_universe` is true. Emits `decision` (MUST_COVER / REVIEW_ZONE / DROP), `alpha_score`, `horizon_tag`, `downstream_mode` (valentine / gunn / dual). DROP is a terminal early-exit gate. |
| `intel-mosaic.j2` | Agent 02 INTEL (DEEPEN). Business-context 8-step + information mosaic. Consumes `company_research_search` and `web_search` MCP tool outputs (bound via input_mapping from prior direct tool calls) and the `listening/apply-template` earnings-call verdict (cross-skill step 3). Emits `mosaic_clear` (false = MNPI HALT terminal gate), `business_model`, `news_items`, `hypotheses` (PENDING / VALIDATED / UNRESOLVABLE lifecycle), `data_gaps`. Per .rules: failed MCP tools surface as `data_gaps` entries, never collapse to None. |
| `forensic-pre-screen.j2` | Agent 04 FORENSIC (pre-screen). Quick risk pre-screen across accounting red flags, governance, going-concern signals. Emits `severity` (SEV-1 minor → SEV-5 fraud/restatement), `recommendation` (CLEAR+adj / CONDITIONAL / BLOCK), `eps_haircut`, `dr_add_bps`. BLOCK is a terminal early-exit gate. FORENSIC cannot be skipped (EFRA-AI invariant). |
| `critical-factor.j2` | Agent 03 CRITICAL FACTOR. Identifies the 3–5 critical factors that drive the business and constructs Bull / Base / Bear scenarios with EPS impact. Consumes `scenario_build` MCP tool output (bound via input_mapping) for structured scenario generation. Emits `factors` (empty = DROP terminal gate), `scenarios` (bull/base/bear with probabilities and EPS impact), `eps_impact_pct`. Cross-references `superforecasting/stage_3_probability_estimate` methodology for granular (0.35) vs round (0.50) probabilities. |
| `forensic-full.j2` | Agent 04 FORENSIC (full). Full audit: accruals quality, governance (board independence, COB/CEO separation), management profile (owner- operator, capital allocation track record). Consumes `company_transcript` MCP tool output for management quotes. Emits `severity`, `recommendation` (BLOCK terminal gate), `management_quality`, `governance_score`, `accruals_score`. FORENSIC cannot be skipped. |
| `valuation-8step.j2` | Agent 05 VALUATION (DEEPEN). 8-step price target engine. Synthesizes over four direct MCP tool outputs bound via input_mapping: `dcf_valuation` (7a), `comparable_analysis` (7b), `expectations_gap` (7c), `scenario_impact_valuation` (7d). Emits `pt_12m`, `rr_ratio`, `rating` (BUY/HOLD/UNDERPERFORM), `FaVeS` (variant expectations score), `confidence`, `data_gaps` (names any failed MCP tool with LLM-derived fallback estimate + confidence penalty per EFRA-AI L1/L2 fallback hierarchy). RR < 2:1 + UNDERPERFORM = DROP terminal gate. |
| `communication-enter.j2` | Agent 06 COMMUNICATION. ENTER gate (Edge / New / Timely / Examples / Revealing — 5/5 = PUBLISH, 4/5 = ALERT, ≤3/5 = DROP) and CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data). Emits `publication_possible`, `enter_score`, `cascade_note`, `final_confidence`. Confidence < 0.50 = NO_PUBLISH (EFRA-AI invariant). |
| `lens-five-frameworks.j2` | Agent 09 LENS. Consistency auditor. Applies the firm's five intellectual frameworks: Lens 1 The Loop (economic potential, technological capability, variant expectations, valuation anchor Value = Profits / (r − g), target return > 12%, max P/E < 25×), Lens 2 Superforecasting (granular probabilities, inside/outside view balance, clashing forces, observable invalidation — cross-references `market_cmp` outside view), Lens 3 Dunning-Kruger (process_confidence vs final_confidence gap, overconfidence risk flag), Lens 4 Hidden Champions (Simon 8 characteristics), Lens 5 Kauffman / Adjacent Possible (ergodic vs nonergodic, new niches, Darwinian preadaptations). Emits `overall_verdict` (CONSISTENT / PARTIAL / INCONSISTENT), `key_tensions`, `pm_memo` (200 words). Never blocks publication. |
| `company-8part.j2` | Agent 13 COMPANY (DEEPEN). Deep 8-part company analysis: Self-View, Business Franchise, Management Skill (CEO long-term + CFO working capital scorecards), Financial Profile (signposts + 3-stage valuation), Invisible Layer, Falstaffian Inversion, Value Gorilla Elevator Pitch, Investment Thesis Statement. Consumes `company_transcript`, `dcf_valuation`, `comparable_analysis`, `web_search`, `fetch` MCP tool outputs (bound via input_mapping from prior direct tool calls). Emits `CompanyBoard` with all 8 sections, `data_gaps`. |
| `falstaffian-competitive-rotation.j2` | v0.38.0 addition. Rotates the competitive framing of the Company Board before GORILLA scores it. Applies Falstaffian semantic rotation shapes (predicate hollow, subject expansion, object inversion, direction reversal) to expose framing errors in the analyst narrative. Emits rotated_board with competitor-complement analysis, market creator vs participant classification (Wardley evolution axis), framing errors detected, and rotated competitive position. Anchored to MAIA "Falstaff: Give Me Life", "Competition: Readings vs Reality", "Company Analysis", "Thinking Like an Owner". Cross-references metacognition/falstaffian-perspective-engine shapes and decision tree. |
| `gorilla-4dim.j2` | Agent 10 GORILLA. Value Gorilla 4-dimension framework with fixed methodology weights (Obvious Problem 25% / Invisible Gorilla 30% / Combinatorial Solution 25% / Choke Point 20%). Weights are fixed by firm methodology — NOT user-tunable, so mcda was rejected (essentialist Surface gate: adds ceremony for fixed weights). Scoring is a `lisp_eval` call, not an mcda call. In v0.38.0, GORILLA consumes the ROTATED board (from falstaffian-competitive- rotation), not the raw Company Board — the rotation corrects framing errors before scoring. Emits `gorilla_score`, `verdict` (GORILLA ≥75 / SMALL_ANIMAL 50-74 / PEDESTRIAN <50), per-dimension scores. |
| `economic-trajectory.j2` | v0.38.0 addition. Economically-anchored imagination scaffold. Identifies the falling-cost trajectory in the subject's industry (McAfee dematerialization), the design constraint being removed (MAIA bottleneck framework), the Coasean firm-boundary shifts (Kauffman economic web), the Kauffman adjacent possible nodes (never-before-born goods and services, Darwinian preadaptations), and convergence vectors (Diamandis). Emits economic_trajectory with falling_cost, constraint_being_removed, coasean_shifts, adjacent_possible_nodes, convergence_vectors, implications_for_ subject, trajectory_velocity. IMAGINE consumes this as the anchor for its 5/10Y scenarios. Anchored to MAIA "Focus and Imagination", "More From Less", "Kauffman Readings", "The Future Is Faster", "Bottlenecks and Critical Mass", "Time Horizons". |
| `imagine-longrange.j2` | Agent 11 IMAGINE. Projects the business at 5 and 10 years and walks it back analytically. Digital Transformation Stages (MODEL / SHADOW / TWIN / SOURCE), Growth Driver Classification (innovation / demographic / both / neither). In v0.38.0, scenarios are ANCHORED on the economic trajectory probe (falling cost, constraint removal, adjacent possible) and CHALLENGED by the Falstaffian rotations (rotated competitive framing, framing errors detected). Consumes `scenario_build` MCP tool output and the `economic_trajectory` probe. Emits `ImagineBoard` with digital stage, growth driver, 3 scenarios (each with trajectory_anchor and falstaffian_challenge), 3–5 falsifiable predictions (tagged by horizon, each with trajectory_ basis), what's not on the page (anchored on adjacent possible), what's not in the price (anchored on trajectory implications), trajectory_anchoring, falstaffian_challenge. |
| `thesis-three-pillars.j2` | Agent 12 THESIS. Synthesizes all prior research into a formal investment thesis covering the three pillars: Business Franchise (moat strength, value creation, durability), Management Quality (capital allocation, leadership), Valuation (3-stage: consensus → normalization → terminal). Quality gate verdict `investment_grade` / `needs_work` / `incomplete` is the deep pipeline convergence signal. Per .rules (LLM-improves-against-LLM-scored-target trap): the quality gate uses `goal-analysis` semantic evaluation, not self-assessment — wired as a cross-skill `render_template` call, not inside this template. |
| `intel-semantic-classify.j2` | v0.38.0 cross-skill adapter. Adapts pragmatic-semantics/ semantics-classify-statement to the INTEL mosaic. Classifies every news_item and hypothesis by ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force, and provenance — BEFORE downstream steps consume the intel. Prevents certainty-level drift: a management quote treated as an ontological fact, a scenario treated as a forecast. Emits semantic_tags and certainty_drift_risk that downstream templates (forensic, critical- factor, valuation) consume via intel_bundle.semantic_tags. |
| `gorilla-capability-reason.j2` | v0.38.0 cross-skill adapter. Adapts capabilities-reasoner/ capability-reason to the GORILLA 4-dim framework. Types each GORILLA dimension (Obvious Problem, Invisible Gorilla, Combinatorial Solution, Choke Point) against a capability registry with floor, ceiling, and maturity-gate limits. The GORILLA score (0–100) is the elicited potential; the capability assessment determines whether that score is credible against the company's observed behavior and maturity. Emits capability_assessments, floor_violations, ceiling_violations, maturity_blocks. A maturity block on a dimension means the `lisp_eval` scoring call should treat that dimension's score as nil, not as the elicited value. |
| `thesis-essentialist.j2` | v0.38.0 cross-skill adapter. Adapts essentialist/essentialist-flow to the three-pillar investment thesis. Runs a single pass of the 3-gate protocol (Exist, Surface, Contract) on the thesis to enforce parsimony — does each pillar earn its place? Is the thesis at the right abstraction level? Can it be stated more tersely? Mode is autonomous (no human in the loop during the pipeline). The elimination_report feeds the goal-analysis quality gate as additional evidence — it does not block the thesis directly. |
| `kata-calibration-measure.j2` | v0.38.0 cross-skill adapter. Adapts metacognition/meta-experiment to close the flash pipeline's open kata loop. Flash step 20 (kata- improvement-step1-direction) sets the direction but never measures the gap. This step measures the analyst's calibration gap using the market_calibration Brier score (step 19) and resolved_outcomes (step 18), then re-measures the current condition. Emits calibration_gap (0.0 calibrated → 1.0 maximum gap) that LENS (step 23) consumes as a 6th axis alongside the existing five frameworks. |
| `wardley-anchor.j2` | v0.38.0 cross-skill adapter. Compresses wardley-mapper's 6-step process (inventory → classify → map → movement → recommendations → present) into a single LLM call over the rotated Company Board. Emits wardley_map with components, evolution classifications, movements, commoditization candidates, choke_points, and invisible_gorillas. Feeds GORILLA's Invisible Gorilla and Choke Point dimensions (step 5) and ECONOMIC TRAJECTORY's falling-cost anchor (step 9). The full wardley-mapper skill is available for standalone use — this adapter exists to ground the deep pipeline's strategic analysis without adding a 6-step sub-process. |
| `wardley-anchor.j2` | v0.38.0 cross-skill adapter. Compresses wardley-mapper's 6-step process (inventory → classify → map → movement → recommendations → present) into a single LLM call over the rotated Company Board. Emits wardley_map with components, evolution classifications, movements, commoditization candidates, choke_points, and invisible_gorillas. Feeds GORILLA's Invisible Gorilla and Choke Point dimensions (step 5) and ECONOMIC TRAJECTORY's falling-cost anchor (step 9). The full wardley-mapper skill is available for standalone use — this adapter exists to ground the deep pipeline's strategic analysis without adding a 6-step sub-process. |
| `verify-early-anchor.j2` | Verification step 1. Invokes `grounding-verify` skill via `spawn_agent` to verify CompanyBoard claims against step-1 MCP tool outputs. Extracts claims, assigns provenance tiers on the strength lattice (tool_verified > model_inference > unavailable), mechanically verifies citations via retrieve-cite-verify, scans narrative for leak rules, computes fact_score_early = 0.30*SAR + 0.25*CVR + 0.20*HFR + 0.25*NLR via lisp_eval. Emits verified_claims registry (append-only), fact_score_early, data_gaps, confidence_adjustment, confidence_band. Run as spawn_agent (decoupled from generator). |
| `verify-late-gate.j2` | Verification step 2. Invokes `grounding-verify` skill via `spawn_agent` to verify full pipeline report against all accumulated source outputs. Checks thesis claims against verified_claims registry from early anchor (fast path), runs full verification on new claims from downstream synthesis. Computes fact_score_final = 0.30*SAR + 0.25*CVR + 0.20*HFR + 0.25*NLR. Emits fact_score_final, fact_score_breakdown, data_gaps, confidence_adjustment, confidence_band, hallucination_findings, verification_scope_limitations. fact_score_final feeds goal-analysis quality gate as additional evidence. Run as spawn_agent (decoupled from generator). |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

The flash-only templates (`scout-alpha-score.j2`, `intel-mosaic.j2`, `intel-semantic-classify.j2`, `forensic-pre-screen.j2`, `critical-factor.j2`, `forensic-full.j2`, `valuation-8step.j2`, `communication-enter.j2`, `lens-five-frameworks.j2`, `kata-calibration-measure.j2`) are documented in the `company-research-flash` SKILL.md.

## MCP Tool Integration

All MCP tool calls are called directly (deterministic, governed, testable). See `kask/docs/architecture/skills-and-composition.md` Part II for the invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing THESIS verdict surfaces as 1.0 (worst case), not silently converged.
- The THESIS quality gate uses `goal-analysis/judge` (semantic evaluation), not self-assessment — to avoid the LLM-improves-against-LLM-scored-target trap.
- IMAGINE's falsifiable predictions are persisted to the forecast-ledger (when built — Phase 3) for later Brier scoring.
- Verification steps (verify-early-anchor, verify-late-gate) run as `spawn_agent` calls invoking the `grounding-verify` skill — decoupled from the generator to prevent the self-confirming loop (self-improvement §9.1).
- fact_score sub-metrics use nil-propagation, not zero-fallback. If any sub-metric is nil, fact_score is nil — never `unwrap_or(0)`. A nil fact_score surfaces as `data_gap: "fact_score_measurement_failed"` with confidence penalty -0.20.
- fact_score carries a `claims_checked` count. A fact score of 1.0 with zero claims checked is nil — zero mismatches over zero rows is unknown, not clean.
- The provenance lattice has an extraction ceiling: LLM-synthesized claims are `model_inference` (strength 1), never `tool_verified` (strength 2). Only direct citations verified via mechanical match can be `tool_verified`.
- The `verified_claims` registry from the early anchor is append-only — the late gate adds entries but does not modify existing ones. This prevents the un-stripping trap.
- fact_score for pipeline iteration N only counts claims from iteration N — prior iteration failures are historical context, not current failures (cohort scoping).
- The fact_score feeds the quality gate as additional evidence — it does not replace `goal-analysis/judge`. fact_score < 0.60 triggers `needs_work` directly; fact_score ≥ 0.60 still requires the semantic quality gate. Two independent gates, not one.
- The confidence adjustment is derived from the provenance floor, not from the LLM's self-assessed confidence. If the weakest claim is `model_inference`, the band is `medium` regardless of what the thesis template says.
