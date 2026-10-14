---
name: company-research-flash
description: "Equity research flash pipeline (EFRA-AI conversion): SCOUT → source collection → INTEL + LISTEN + semantic classification → FORENSIC → CRITICAL FACTOR → VALUATION → provisional COMMUNICATION → KATA + LENS → author-side evidence review → PERSIST + CONDENSE. Preserves DROP/HALT/BLOCK gates; publication requires the evidence review performed, the ENTER gate and adjusted confidence ≥ 0.50. Bounded correction loop."
---

# Company Research — Flash Pipeline

Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Governed process producing a flash note / initiation report, with an author-side evidence review before publication. MCP tool calls (forecast_list, company_research_search, web_search, company_transcript, dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation, market_check_resolutions, market_calibration, market_match, evaluate_evidence, forecast_persist) are called directly; templates do LLM synthesis over their outputs.

## When to Use

- When you need a flash note or initiation report for an equity ticker, following the Valentine × Gunn Dual-Mode Framework.
- When you want the full EFRA-AI main pipeline (SCOUT → INTEL → FORENSIC × 2 → CRITICAL FACTOR → VALUATION → COMMUNICATION → KATA → LENS) as a single governed process.
- When you want deterministic, testable MCP tool calls (DCF, comparables, expectations gap, scenario-weighted PT) rather than LLM-mediated tool use.
- When you want the analyst forecast-to-outcome calibration loop (`forecast_persist` → recorded outcomes → `forecast_list` → the KATA PDCA "check" `lisp_eval` form) that EFRA-AI's KATA agent describes but cannot close.
- When you want the LENS five-framework consistency audit (The Loop, Superforecasting, Dunning-Kruger, Hidden Champions, Kauffman) as a convergence signal.

## When NOT to Use

- Deep initiation with the full 8-part company analysis — use `company-research-deep`.
- No companies/prediction-markets MCP access — the pipeline's substrate is those tools' outputs; without them the early-exit gates fire and nothing downstream runs.
- Post-thesis monitoring — a published flash note's lifecycle is PERSIST/CONDENSE; monitoring belongs to the portfolio-review loop.

## Reference models

The pipeline shape (SCOUT → INTEL → FORENSIC → CRITICAL FACTOR → VALUATION → COMMUNICATION → KATA → LENS) is converted from EFRA-AI (Replicant-Partners), a public repository (https://github.com/Replicant-Partners/EFRA-AI); the stage ordering here is the house adaptation of that source. The published anchors sit inside the stages: Mauboussin & Rappaport, *Expectations Investing* (2001) for VALUATION's expectations gap (`expectations_gap`, `reverse_dcf` tools); the MAIA v3 listening template for LISTEN (`onto_anchor` → derived `maia_listening`); Tetlock & Gardner (2015) and Brier (1950) for KATA's calibration measure (`onto_anchor` → derived `superforecasting`, `brier_score`); Simon, *Hidden Champions* (1996) and Kruger & Dunning (1999) for LENS lenses 3–4. LENS lens 1 ("The Loop") and lens 5 (Kauffman's adjacent possible, applied to equities) are the converted firm's own frameworks with no published operationalisation.

## D/P labelling

SCOUT component scoring, INTEL synthesis (including per-item pragmatic-semantics tags and `certainty_drift_risk`), LISTEN reading, FORENSIC and
CRITICAL FACTOR judgment, VALUATION method weighting (`pt_weights`), COMMUNICATION drafting, and the LENS audit are
P — critiqued by the author-side evidence review, the DROP/HALT/BLOCK gate
inputs, and the operator. The gates and computations are D: the alpha
score, the FORENSIC severity lookup (`CRF-1`), the `pt_12m` blend / `rr_ratio` / `rating` / `DROP` gate (`CRF-2`), the ENTER gate
dispatch, the KATA calibration measure, the LENS `overall_verdict` / convergence scalar (`CRF-3`), and the publication gate
(adjusted confidence + the publish boolean) run in `lisp_eval` (fixed forms — publication requires the ENTER
gate's literal output and adjusted confidence ≥ 0.50, never the model's
assessment); the valuation tools (`dcf_valuation`, `comparable_analysis`,
`expectations_gap`, `scenario_impact_valuation`) are server oracles over
model-supplied inputs; quote verification runs `lisp_eval`
`string-contains` against its own passage. The KATA calibration measure
closes deterministically: `forecast_list` over the analyst's resolved
forecasts → the calibration `lisp_eval` form (the `market_calibration`
bucket Brier is market context, never the analyst's calibration).

## Instructions

Execution order: collection setup and SCOUT → candidate note check → remaining collection → INTEL →
LISTEN (`listening` over the company's own narrative) →
FORENSIC pre-screen → CRITICAL FACTOR → FORENSIC full → VALUATION → provisional
COMMUNICATION → KATA/calibration → LENS (and bounded revisions) →
PREPARE-SUMMARY → evidence-review → PERSIST/CONDENSE. Render the templates the steps name —
`company-research/intel-mosaic`, `company-research/forensic-pre-screen`,
`company-research/valuation-8step`, `company-research/lens-five-frameworks` and
`company-research/evidence-review` — with the actual preceding outputs; every other stage runs its
methodology as written in its step. Keep `listening_view` in `intel_bundle`
and its original documents in the working source records; neither is interchangeable.
DROP/HALT/BLOCK remain terminal.

### collect-evidence

1. Resolve the company with `resolve_symbol` and read `forecast_list` / relevant `report_load` outputs as prior analysis, not primary evidence. SCOUT's terminal DROP still avoids unnecessary deep collection.
2. For companies that pass SCOUT, before INTEL or valuation start with how the company sees itself: its own filings, reports and presentations, not sell-side or media reports. Compose a small sourced factual candidate note and check each of its load-bearing claims against its source — right figure, right period, right units — before proceeding. A false or unsupported load-bearing claim is repaired first; a claim that cannot be checked is recorded as a `data_gaps` entry and the note proceeds. For companies that pass the candidate check, collect business context with `company_research_search`, `web_search`, `company_transcript` and the financial tools required below. Retrieve the original page or filing with `web_extract` for load-bearing claims and quotes; a search snippet or generated answer is only a lead. Keep provider warnings, entity identity, period, currency, units and audit status. A 10-Q is unaudited interim data, not an audited annual statement.
3. Collect evidence for meaning, not form. Keep a document only if it bears on the business, management, valuation, risks or thesis, the growth and profitability an owner weighs. Boilerplate, meaning text with form but no bearing on the company's economics (routine third-party ownership threshold notices, legal notices, contact pages, results about other entities), is not evidence: do not retrieve, list, classify or count it, and its absence is never a gap or `not_checked`. An agent that spends the run processing boilerplate instead of the company's economics has failed the skill; its output is discarded and not retried. For each source you use, hold its citation identity: URL, document title, retrieval date, period, units, audit status, and the retrieved text; give it a short citation key the note will cite. Keep source content separate from model-written summaries and record failed retrievals as named gaps. When a valuation, signpost or quote-bearing tool's calculation reaches the note, keep its actual response and distinguish it from audited input; a model-generated stage output is labelled as such, never an original. Pass the source records to `company-research/intel-mosaic` for synthesis and keep them through downstream stages.
4. The note is the only durable artifact: no run folders, no retained-output files, no intermediate evidence files. Sources and stage outputs are working state for this note.

### listen

After INTEL collection, read the company once through the `listening` skill's long-term horizon so downstream stages work from cited strategic evidence rather than raw documents, short-term noise or hype.

1. Split the company's own narrative originals into verbatim passages of at most ~800 words at paragraph or speaker-turn boundaries: current and prior earnings-call transcripts plus the annual report's business and MD&A sections when retained. Each passage is `{source, entity_ref, text}` with `source` = its citation key. Exclude financial statement tables, notes and boilerplate. A missing prior call is a `data_gaps` entry.
2. Follow the `listening` skill process with `listening/apply-template-rag` (`corpus_passages`, `kg_triples: []` unless a company graph exists, `company_symbol`, `focus_query` = the note's scope). Verify every quote with `lisp_eval` `string-contains` against its own passage; drop quotes that fail twice.
3. Put `listening_view` in `intel_bundle` (keep the originals in the working source records). INTEL hypotheses, FORENSIC management profile, CRITICAL FACTOR and COMMUNICATION catalysts consume it instead of re-reading the raw narrative; consult an original only to check a specific quote or fill a named gap. No input comes from `ignored_short_term`; promotional claims with no dated checkpoint, delivered result or audited support stay context.

### scout-alpha-score

1. Score each component 0–1, then compute the alpha score via `lisp_eval` — fixed weights; add the EM GDP / Bessembinder / low-coverage bonuses (up to +25) to the weighted base:
   - form: "(+ (* coverage_gap 0.30) (* market_cap_fit 0.20) (* sector_relevance 0.25) (* valuation_anomaly 0.25))"
   - env: `{ "coverage_gap": <0–1>, "market_cap_fit": <0–1>, "sector_relevance": <0–1>, "valuation_anomaly": <0–1> }`
2. Apply the 11-criterion excellence universe if `in_excellence_universe` is true: S1 trading status active; S2 listed on a qualifying exchange; S3 price above the minimum; S4 working capital adequate; S5 debt serviceable; S6 gross margin stability (3Y); S7 market cap in range; S8 sector relevance; S9 revenue growth positive; S10 no going-concern flag; S11 coverage gap exists. Any S1–S11 failure forces DROP regardless of the alpha score.
3. Emit decision (MUST_COVER / REVIEW_ZONE / DROP — DROP is terminal), alpha_score with its alpha_components (the four weighted component values), horizon_tag, downstream_mode, and research_query — a single focused query for `company_research_search` (e.g. "{ticker} business model competitive position unit economics") that the collection step runs.

### intel-mosaic

1. Render `company-research/intel-mosaic` with `ticker`, `horizon_tag`, `downstream_mode`, `research_search_result`, `web_search_result`, `sources`, and `listening_view`. Synthesize `company_research_search` and `web_search` MCP tool outputs into a business context 8-step (identity, geography, business model, competitive position, customers, management, risks, catalysts).
2. Build the information mosaic with source-tier classification, horizon tagging, and inline per-item pragmatic-semantics tags (`ontological_mode` IS/OUGHT, `epistemic_mode` declarative/probabilistic/subjunctive, `constraint_force`, `provenance`) so downstream stages do not collapse IS/OUGHT/subjunctive distinctions. Generated summaries are `inference` even when returned by a tool.
3. Form 3–5 testable hypotheses with PENDING / VALIDATED / UNRESOLVABLE lifecycle and per-item pragmatic-semantics tags.
4. Emit `mosaic_clear` (false = MNPI HALT terminal gate), `business_model`, `news_items`, `hypotheses`, `certainty_drift_risk` (`low | medium | high`), and `data_gaps`.
5. **Intent-routed search:** Call `web_search` with the `intent` hint (news, academic, semantic, freshness, general, transcript) and no explicit `provider` — the tool scores the configured providers against (query, intent) and queries the top recommendation as a single-provider call, surfacing the ranking in `provider_recommendations` and the choice in `selected_provider`. This prevents the arxiv-only fallback that occurs when no provider is explicitly selected and the credential map is stale.

### forensic-pre-screen

1. Render `company-research/forensic-pre-screen` with `ticker` and `intel_bundle` — a quick risk scan across accounting red flags, governance, going-concern, regulatory, management integrity. The template emits `severity` (`SEV-1`..`SEV-5`), `findings`, and `rationale`.
2. **Gate `CRF-1` (D — `lisp_eval`):** Deterministically map `severity` to `(list recommendation eps_haircut dr_add_bps)` via the 5-row severity lookup:
   - form: `(cond ((string= severity "SEV-5") (list "BLOCK" 30 300)) ((string= severity "SEV-4") (list "CONDITIONAL" 20 200)) ((string= severity "SEV-3") (list "CONDITIONAL" 10 150)) ((string= severity "SEV-2") (list "CLEAR+adj" 5 75)) (t (list "CLEAR+adj" 0 25)))`
   - env: `{ "severity": <"SEV-1".."SEV-5"> }`
   `BLOCK` (`SEV-5`) is a terminal gate — downstream steps do not run. FORENSIC cannot be skipped.

### critical-factor

1. Identify 3–5 critical factors that drive EPS or the multiple.
2. Construct Bull/Base/Bear scenarios with granular probabilities (not round numbers) and EPS impact.
3. Emit impact_mappings (per-node DCF assumption deltas for scenario_impact_valuation) and the scenario tree in the flat format the tool accepts — the Bull/Base/Bear nodes with their ids and marginal probabilities, so `scenario_impact_valuation` has its `scenario_tree` input from this stage.
4. Empty factors = DROP terminal gate.

### forensic-full

1. Full audit: accruals quality, governance (board independence, COB/CEO separation), management profile (owner-operator, capital allocation track record).
2. Consume company_transcript MCP tool output for verbatim management quotes. If `company_transcript` is unavailable, search for `"{company} earnings call transcript Q{N} {year}"` via `web_search(provider="serpapi")` — SerpAPI supports YouTube transcript extraction. Fall back to `web_search(provider="tavily")` for written transcript sources. Never substitute Wikipedia-sourced paraphrases for verbatim management quotes without flagging a `data_gap`.
3. Emit management_quality, governance_score, accruals_score, and the revised `severity` — the full audit may upgrade or downgrade the pre-screen severity on deeper evidence. Re-run `CRF-1` (`lisp_eval`) on the revised `severity` to re-bind `recommendation` (`BLOCK` is terminal), `eps_haircut`, and `dr_add_bps`: the VALUATION stage consumes the re-bound values, never the pre-screen's stale ones.

### valuation-8step

1. Render `company-research/valuation-8step` with `ticker`, `forensic_profile` (`severity`, `recommendation`, `eps_haircut`, `dr_add_bps` from `CRF-1`), `cf_scenarios` (the Bull/Base/Bear outputs — the bear case anchors the risk-reward), `intel_bundle`, `downstream_mode` from SCOUT (the operating mode — valentine / gunn / dual), the four MCP tool outputs (`dcf_result`, `comparables_result`, `expectations_result`, `scenario_pt_result`), and `lens_tensions` from the prior LENS audit when re-entering — address them in this run. Call `dcf_valuation`, `comparable_analysis` and `scenario_impact_valuation` with the FORENSIC-adjusted inputs so the tool outputs the template consumes already reflect the forensic severity: each tool's `discount_rate` carries `dr_add_bps` (bps converted to decimal, e.g. 150bps → +0.015) and the earnings-basis inputs (margins, and growth to the extent it is earnings-driven) are multiplied by (1 − `eps_haircut`/100, e.g. 10% → ×0.90). `expectations_gap` has no adjustment surface (its `growth_estimate` is annotation-only) — it is consumed unadjusted. Tool outputs computed on unadjusted inputs are flagged in `data_gaps`, never silently blended. When the note wants the full scenario path grid (the JSON output caps at 50 paths), call `scenario_impact_valuation` with `presentation: "WorkbookWhatIf"` — the published workbook carries every path as an editable revision and its ```spreadsheet display hint renders inline; the default output is unchanged when the field is omitted. The template emits `pt_weights` (`{w_dcf, w_comps, w_siv}`), `pt_components`, `faves`, `confidence`, `data_gaps`, and `rationale`.
2. **Gate `CRF-2` (D — `lisp_eval`):** From `pt_weights`, `pt_components`, and `bear_case_pt` (from `cf_scenarios`), deterministically compute `pt_12m`, `rr_ratio`, `rating`, and the `DROP` gate in `lisp_eval` (sole producer of those four values):
   - `pt_12m` form: `"(/ (+ (* dcf w_dcf) (* comps w_comps) (* scenario_pt w_siv)) (+ w_dcf w_comps w_siv))"`
   - `rr_ratio`, `rating`, and `DROP` gate form: `"(let ((pt (/ (+ (* dcf w_dcf) (* comps w_comps) (* scenario_pt w_siv)) (+ w_dcf w_comps w_siv)))) (let ((rr (/ (- pt market_price) (- market_price bear_case_pt)))) (let ((rtg (cond ((>= rr 2) \"BUY\") ((>= rr 1) \"HOLD\") (t \"UNDERPERFORM\")))) (list pt rr rtg (if (and (< rr 2) (string= rtg \"UNDERPERFORM\")) \"DROP\" \"PROCEED\")))))"`
   - env: `{ "dcf": <DCF fair value>, "comps": <comparables fair value>, "scenario_pt": <scenario-weighted PT>, "w_dcf": <pt_weights.w_dcf>, "w_comps": <pt_weights.w_comps>, "w_siv": <pt_weights.w_siv>, "market_price": <current price>, "bear_case_pt": <bear-case target> }`
3. Compute FaVeS (variant expectations score — where your thesis differs from the market).
4. Emit `data_gaps` for any failed MCP tool with LLM-derived fallback estimate + confidence penalty (L1/L2 fallback hierarchy).
5. RR < 2:1 + UNDERPERFORM = `DROP` terminal gate.

### communication-enter

1. Score the ENTER gate (Edge / New / Timely / Examples / Revealing — each true/false), then dispatch via `lisp_eval`:
   - form: "(let ((n (+ (if edge 1 0) (if new 1 0) (if timely 1 0) (if examples 1 0) (if revealing 1 0)))) (cond ((= n 5) 'PUBLISH) ((= n 4) 'ALERT) (t 'DROP)))"
   - env: `{ "edge": <bool>, "new": <bool>, "timely": <bool>, "examples": <bool>, "revealing": <bool> }`
2. Draft the CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data): 300–500 words for PUBLISH eligibility, or 200–300 for ALERT.
3. Compute final_confidence (blend of VALUATION confidence, FORENSIC severity, FaVeS).
4. Confidence < 0.50 = NO_PUBLISH. publication_possible = false is a terminal DROP (KATA and LENS skip). A true value and ENTER's PUBLISH/ALERT are provisional eligibility, not release permission: verify-before-publish must still pass.

### lens-five-frameworks

1. Render `company-research/lens-five-frameworks` with `ticker`, `downstream_mode`, `scout`, `intel`, `forensic`, `cf`, `valuation`, `communication`, `kata`, `calibration_gap` (from the KATA calibration step below), `market_outside_view` (from `market_match` and `evaluate_evidence`), and `evidence_audit`. (`intel.certainty_drift_risk` and the per-item tags on `intel.news_items` / `intel.hypotheses` supply the certainty-drift classification directly inside `intel`.) When the note wants the per-artifact evidence matrix rendered inline (confidence, corroborations, published age as a bounded sortable table), call `evaluate_evidence` with `presentation: "InlineTable"` — the published inline table renders from its ```spreadsheet display hint; the default output is unchanged when the field is omitted. The stage applies the five intellectual frameworks: The Loop (economic potential, variant expectations, valuation anchor Value = Profits / (r − g), target return > 12%, max P/E < 25×), Superforecasting (granular probabilities, outside view via market_match, certainty-level drift via `intel.certainty_drift_risk`), Dunning-Kruger (`process_confidence` vs `final_confidence` gap, `calibration_gap`), Hidden Champions (Simon 8 characteristics), Kauffman (ergodic vs nonergodic, adjacent possible).
2. The template emits `fundamental_violation` (boolean), `lens_scores`, `lens_findings`, `key_tensions`, `recommendations`, and `pm_memo` (200 words).
3. **Gate `CRF-3` (D — `lisp_eval`):** Deterministically compute `(list overall_verdict convergence_scalar)` (`CONSISTENT` / `0.0`, `PARTIAL` / `0.5`, `INCONSISTENT` / `1.0`) from `lens_scores` and `fundamental_violation`:
   - form: `(let ((low_count (+ (if (< the_loop 0.6) 1 0) (if (< superforecasting 0.6) 1 0) (if (< dunning_kruger 0.6) 1 0) (if (< hidden_champions 0.6) 1 0) (if (< kauffman 0.6) 1 0)))) (cond ((or fundamental_violation (>= low_count 3)) (list "INCONSISTENT" 1.0)) ((>= low_count 1) (list "PARTIAL" 0.5)) (t (list "CONSISTENT" 0.0))))`
   - env: `{ "the_loop": <0–1>, "superforecasting": <0–1>, "dunning_kruger": <0–1>, "hidden_champions": <0–1>, "kauffman": <0–1>, "fundamental_violation": <bool> }`
   Never blocks publication.

### kata-calibration

1. Set the direction first: render `kata-improvement/improvement-step1-direction` with `task_description`, `context`, and `constraints` for this note's scope (knowledge gaps, untested assumptions, the next PDCA experiment). If the direction step cannot run, its absent prediction is recorded as a calibration signal, never silently skipped.
2. (D) Measure the analyst's own calibration, not the market's: read the analyst's resolved price-target forecasts for this symbol with `forecast_list` (stated `forecast_probability`, outcome in band 1/0), then compute `calibration_gap` (`[gap, direction]`) directly with `lisp_eval`:
   - form: `(begin (define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l)))))) (define mean (lambda (l) (/ (sum l) (length l)))) (cond ((is_null ps) (list 1.0 "no_prediction")) ((< (length ps) 5) (list nil "undetermined")) (t (let ((d (- (mean ps) (mean os)))) (list (abs d) (if (> d 0) "overconfident" (if (< d 0) "underconfident" "calibrated")))))))`
   - env: `{ "ps": <stated probabilities>, "os": <outcomes 1/0> }`
   The `market_calibration` bucket Brier is market context, not the analyst's calibration.
3. Emit `calibration_gap` (`{calibration_gap, calibration_direction}` — 0.0 calibrated → 1.0 maximum gap). No prediction recorded = 1.0 (`no_prediction` — broken feedback loop, not neutral); fewer than 5 resolved forecasts = null (`undetermined`), not a number.
4. LENS consumes `calibration_gap` as a 6th axis alongside the existing five frameworks.

### prepare-summary

After KATA/LENS and any consistency-driven revision, assemble the full CASCADE report draft with all included stage outputs, financial tables and LENS memo; draft the condensed summary now if requested (not after publication). Cite figure lines to their source keys, and keep tool-output figures on their own cited lines. Check every figure and quote against its cited source and correct mismatches; a mismatch you cannot resolve is carried into the evidence review as an open finding, never erased. Compose both texts in full before any checking; the evidence review runs over exactly these composed texts, and persist writes them unchanged.

### verify-before-publish

1. After prepare-summary, freeze the exact full report and any requested condensed summary as the review target; checking only the headline or conclusion is insufficient. Do not compose a new summary after the freeze.
2. Run the evidence review over the composed texts: render `company-research/evidence-review` with `report_draft`, `summary_draft`, `evidence_digest` (the working source records), and `as_of_date`. Extract the load-bearing claims — those the note's rating, price target, catalysts and ENTER score rest on — with the reason each is load-bearing. Incidental phrasing is not checked; the review's cost stays proportional to what matters.
3. Vouch each load-bearing claim against its source: right figure, right period, right units, right context, with the supporting source named. A claim only management has said is labelled uncorroborated and the note must say so. A paraphrased claim with faithful figure, period and units is as good as a quote — the review never rewards excerpt-style writing.
4. Trace the sources for material facts the note should engage but does not; a found omission is added or carried as a visible gap. Where the note's sections disagree (FORENSIC severity vs VALUATION rating, LENS tensions), the note must say so and settle it with evidence — or state plainly what evidence would resolve it.
5. Run the mechanical checks — quoted phrases verbatim against their keyed originals, figures against their sources (`lisp_eval`) — and record them as counts. A mechanical match is a count, never a verdict.
6. Summarize the review in the note itself: the claims checked with their results and provenance, the omissions found, the disagreements settled or deferred, the mechanical counts, and what was NOT checked and why. An unperformed check is a recorded limitation, never a clean result. The note's label states exactly what was checked and what was not — never the word "verified".
7. Publication is permitted only if ENTER eligibility is true, adjusted confidence ≥ 0.50, the review was performed, and no contradicted load-bearing claim remains unresolved (correct it or remove it first). Compute `confidence_adjustment` from the review: 0.0 when every load-bearing claim is supported, -0.10 when any remains uncorroborated. Execute the following `lisp_eval` form with `enter_eligible` from COMMUNICATION's `publication_possible`, its `unadjusted_confidence`, the computed `confidence_adjustment`, `review_performed` (true when the review ran and its summary is in the note) and `unresolved_contradictions` (the count of contradicted load-bearing claims left unresolved). It returns `[adjusted_confidence, publish]`; missing inputs produce `[nil, false]` and an `incomplete` data gap. Validate numeric types/ranges before evaluation; any evaluation error also blocks publication.

   ```lisp
   (if (or (is_null enter_eligible) (is_null unadjusted_confidence) (is_null confidence_adjustment) (is_null review_performed) (is_null unresolved_contradictions)) (list nil false) (let ((adjusted (+ unadjusted_confidence confidence_adjustment))) (list (max 0 adjusted) (and enter_eligible (>= adjusted 0.50) review_performed (= unresolved_contradictions 0)))))
   ```

8. LENS consistency cannot override a failed evidence review. Record the review summary alongside the research report. Any factual change after this check, including condensing, requires the review to run again on the edited deliverable before release; otherwise label it unreviewed.

### persist-report

0. Persist the note's price target with `forecast_persist`, passing the scenario join fields so the recorded forecast carries its scenario provenance (PR-09): `scenario_project_id` (the scenarios server's project id — the subject, by default), `scenario_tree` (the `scenario_quantify` tree snapshot: node ids + marginals + joint), `impact_mappings_ref` (a digest of the per-node DCF deltas the valuation used), and `fused_volatility` (from `scenario_impact_valuation` when a realized volatility was supplied). The join is recorded on the forecast, not left agent-mediated.

1. After verify-before-publish and the final publication decision, write the full report as a **rich markdown file** with its actual status. A blocked/incomplete run may persist a clearly labelled draft, not a published note. Include the canonical verification report, source references, confidence band and scope limitations.
2. Reports are stored in the user-facing artifacts directory: `~/Documents/zk-data/companies-mcp/reports/`. This is separate from the internal data dir (`~/.local/share/zed-kask/`) — reports are user-facing artifacts that should be visible, not buried in a hidden cache directory.
3. Create the reports directory if it does not exist: `mkdir -p ~/Documents/zk-data/companies-mcp/reports` via `terminal`.
4. Write the markdown file to `~/Documents/zk-data/companies-mcp/reports/{ticker}-flash-{date}.md`. Use a quoted `terminal` heredoc for this external artifacts path; project file tools cannot write outside the workspace.
5. The markdown file is the deliverable — full rich markdown with all sections, source notes, and citations.
6. Do NOT write reports to the source tree (`zed-kask/reports/` or similar) — that pollutes the user's code repository.
7. Do NOT write reports to the hidden internal data dir (`~/.local/share/zed-kask/mcp/companies/reports/`) — that buries user-facing output where the user will never find it.
8. Clean up (storage Cleanup rule). A revised flash note replaces the earlier file for the same ticker and date at the same path; delete working drafts and scratch evidence once the note is persisted, keeping only cited evidence, named in the run summary. Never delete another ticker's or another date's report.

### condense-report

1. The full markdown report written in persist-report IS the deliverable.
2. If a condensed flash note was drafted in prepare-summary, export that exact text to `~/Documents/zk-data/companies-mcp/reports/{ticker}-flash-summary-{date}.md`; do not compose new prose here.
3. Both files are markdown in `~/Documents/zk-data/companies-mcp/reports/`. Any factual change from the late-gate target requires re-verification before release; otherwise label the edited deliverable unverified.

## Convergence

The consistency loop remains LENS-driven: CONSISTENT = 0.0, PARTIAL = 0.5 (re-enter VALUATION with tensions), INCONSISTENT = 1.0 (escalate to the operator with the tensions named). LENS remains advisory to publication, but is not a factuality check. Publication separately requires the evidence review performed with no unresolved contradicted load-bearing claim, ENTER eligibility and adjusted confidence ≥ 0.50. Factual failures re-enter collection or the affected synthesis stage, not merely valuation weighting. An unperformed review stops publication and the note is labelled with what was not checked. max_iterations: 3 bounds ALL correction cycles together; exhaustion returns a labelled draft, never a publishable note.

## Cross-Skill Composition

- LISTEN reuses the full `listening` skill (`listening/apply-template-rag`, no-fabrication invariant) over the company's own narrative documents; material disclosures are exempt from its horizon filter.
- `company-research/intel-mosaic` folds pragmatic-semantics item classification (`ontological_mode` IS/OUGHT, `epistemic_mode` declarative/probabilistic/subjunctive, `constraint_force`, `provenance`, and `certainty_drift_risk`) directly into `news_items` and `hypotheses` before downstream steps consume them.
- The KATA direction step reuses `kata-improvement/improvement-step1-direction` (Toyota Improvement Kata step 1), and the deterministic calibration `lisp_eval` form closes the open kata loop by measuring the calibration gap over the analyst's own resolved forecasts (`forecast_list` → `lisp_eval`); the `market_calibration` bucket Brier is market context only.
- Mandatory verify-before-publish runs the author-side evidence review (`company-research/evidence-review`) over the composed texts; its summary goes in the note as the review note. The flash skill owns collection, correction and publication; the review records what was checked and never labels the note verified.

## Registry Templates

All templates live in the shared `kask/registry/templates/company-research/` crate (used by both the flash and deep pipelines):

| Template | Purpose |
|----------|---------|
| `intel-mosaic.j2` | Agent 02 INTEL (DEEPEN). Business-context 8-step, information mosaic, and inline pragmatic-semantics classification (`ontological_mode`, `epistemic_mode`, `constraint_force`, `provenance` on each `news_item` and `hypothesis`, plus `certainty_drift_risk`). Consumes `company_research_search` and `web_search` MCP tool outputs and `listening_view`. Emits `mosaic_clear` (false = MNPI HALT terminal gate), `business_model`, `news_items`, `hypotheses`, `certainty_drift_risk`, `data_gaps`, `rationale`. |
| `forensic-pre-screen.j2` | Agent 04 FORENSIC (pre-screen). Quick risk pre-screen across accounting red flags, governance, going-concern signals. Emits `severity` (`SEV-1`..`SEV-5`), `findings`, and `rationale`; `CRF-1` (`lisp_eval`) deterministically maps `severity` to `recommendation` (`CLEAR+adj` / `CONDITIONAL` / `BLOCK`), `eps_haircut`, and `dr_add_bps`. `BLOCK` is a terminal early-exit gate. FORENSIC cannot be skipped (EFRA-AI invariant). |
| `valuation-8step.j2` | Agent 05 VALUATION (DEEPEN). 8-step price target engine. Consumes `forensic_profile` (severity-scaled haircuts), `cf_scenarios`, `intel_bundle`, `downstream_mode`, four direct MCP tool outputs (`dcf_result`, `comparables_result`, `expectations_result`, `scenario_pt_result`), and `lens_tensions` on LENS-driven re-entry. Emits `pt_weights` (`{w_dcf, w_comps, w_siv}`), `pt_components`, `faves`, `confidence`, `data_gaps`, `rationale`; `CRF-2` (`lisp_eval`) deterministically computes `pt_12m`, `rr_ratio`, `rating` (`BUY`/`HOLD`/`UNDERPERFORM`), and the `DROP` gate. |
| `lens-five-frameworks.j2` | Agent 09 LENS. Consistency auditor over every prior stage output plus `calibration_gap`, `market_outside_view`, and `evidence_audit` (reading `certainty_drift_risk` and per-item tags from `intel`). Applies the firm's five intellectual frameworks: The Loop, Superforecasting, Dunning-Kruger, Hidden Champions, Kauffman / Adjacent Possible. Emits `fundamental_violation`, `lens_scores`, `lens_findings`, `key_tensions`, `recommendations`, `pm_memo`; `CRF-3` (`lisp_eval`) computes `overall_verdict` (`CONSISTENT` / `PARTIAL` / `INCONSISTENT`) and the convergence scalar (`0.0` / `0.5` / `1.0`). Never blocks publication. |
| `evidence-review.j2` | Author-side evidence review of the composed note texts in the audit working-paper discipline: assertion inventory (materiality-scoped), vouching (claim → evidence), tracing (evidence → report), disagreement coverage, mechanical counts recorded as counts. Emits the review record; never a verified label. Used by company-research-flash and company-research-deep. |

To render a template, call the `render_template` tool with the template ref (e.g., `company-research/intel-mosaic`) and a context object with the required variables.

## MCP Tool Integration

All MCP tool calls are called directly (deterministic, governed, testable). See `kask/docs/architecture/skills-and-composition.md` Part II for the invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the KATA calibration form:

- Five predictions at 0.8 vs five outcomes at 0.5 → `[0.3,
  "overconfident"]`.
- No predictions → `[1.0, "no_prediction"]`.
- Fewer than five predictions → `[null, "undetermined"]` — never a
  fabricated verdict.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing LENS verdict surfaces as 1.0 (worst case), not silently converged.
- Reports are written as markdown files to `~/Documents/zk-data/companies-mcp/reports/` via `terminal` (see persist-report — never the source tree or the hidden internal data dir).
- The evidence review is one author-side pass over the composed texts. Its summary is the note's review note; a note without one is labelled "no evidence review performed". Its label never says verified, and a mechanical match is a count, never a verdict.
- A high ENTER score or LENS consistency never overrides an unresolved contradicted load-bearing claim. Corrections regenerate the affected downstream outputs; max_iterations: 3 is shared across all re-entry paths.
