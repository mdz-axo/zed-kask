---
name: company-research-flash
description: "Equity research flash pipeline (EFRA-AI conversion): SCOUT → source collection → INTEL + LISTEN + semantic classification → FORENSIC → CRITICAL FACTOR → VALUATION → provisional COMMUNICATION → KATA + LENS → mandatory independent grounding verification → PERSIST + CONDENSE. Preserves DROP/HALT/BLOCK gates; publication requires completed factual verification and the ENTER/confidence gates. Bounded correction loop."
---

# Company Research — Flash Pipeline

Equity research flash pipeline converted from EFRA-AI (Replicant-Partners). Governed process producing a flash note / initiation report, with mandatory independent grounding verification before publication. MCP tool calls (forecast_list, company_research_search, web_search, company_transcript, scenario_build, dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation, market_check_resolutions, market_calibration, market_match, evaluate_evidence, forecast_persist) are called directly; templates do LLM synthesis over their outputs.

## When to Use

- When you need a flash note or initiation report for an equity ticker, following the Valentine × Gunn Dual-Mode Framework.
- When you want the full EFRA-AI main pipeline (SCOUT → INTEL → FORENSIC × 2 → CRITICAL FACTOR → VALUATION → COMMUNICATION → KATA → LENS) as a single governed process.
- When you want deterministic, testable MCP tool calls (DCF, comparables, expectations gap, scenario-weighted PT) rather than LLM-mediated tool use.
- When you want the forecast-to-outcome calibration loop (market_check_resolutions → KATA PDCA "check") that EFRA-AI's KATA agent describes but cannot close.
- When you want the LENS five-framework consistency audit (The Loop, Superforecasting, Dunning-Kruger, Hidden Champions, Kauffman) as a convergence signal.

## When NOT to Use

- Deep initiation with the full 8-part company analysis — use `company-research-deep`.
- No companies/prediction-markets MCP access — the pipeline's substrate is those tools' outputs; without them the early-exit gates fire and nothing downstream runs.
- Post-thesis monitoring — a published flash note's lifecycle is PERSIST/CONDENSE; monitoring belongs to the portfolio-review loop.

## Instructions

Execution order: collection setup and SCOUT → shared verification at candidate commitment → remaining collection → INTEL →
LISTEN (`listening` over the company's own retained narrative) → semantic classification →
FORENSIC pre-screen → CRITICAL FACTOR → FORENSIC full → VALUATION → provisional
COMMUNICATION → KATA/calibration → LENS (and bounded revisions) →
PREPARE-SUMMARY + draft Tier 1 scan → verify-before-publish → PERSIST/CONDENSE. Render each named synthesis template
with the actual preceding outputs. Keep `listening_view` in `intel_bundle`
and its original documents in `source_outputs`; neither is interchangeable.
DROP/HALT/BLOCK remain terminal and cannot be reopened by a passing fact_score.

### collect-evidence

1. Resolve the company with `resolve_symbol` and read `forecast_list` / relevant `report_load` outputs as prior analysis, not primary evidence. Begin a source ledger with `begin_research_run`; pass `run_id` to `web_search` / `web_extract` / `web_find_similar`. Preserve ledger failures as `data_gaps`, not as silent fallback. SCOUT's terminal DROP still avoids unnecessary deep collection.
2. For companies that pass SCOUT, before INTEL or valuation start with how the company sees itself: its own filings, reports and presentations, not sell-side or media reports. Freeze a small sourced factual candidate note in `drafts/*.md`, list its load-bearing claims in `key-claims.json`, call `company_research_packet_build(run_folder)` on the candidate snapshot, then run the SAME independent `company-research/verification-handoff` gate described in verify-before-publish using the builder's `packet_sha256` (not the publication decision). At this candidate stage an `incomplete` result is recorded as `data_gaps` and does not stop INTEL; a `needs_work` result (a false or unsupported load-bearing claim) is repaired first. The publication gate in verify-before-publish still blocks. Later changes to the artifact, evidence, forecast or as-of require re-execution, not reuse of this receipt. For companies that pass the early gate, collect business context with `company_research_search`, `web_search`, `company_transcript` and the financial tools required below. Retrieve the original page or filing with `web_extract` for load-bearing claims and quotes; a search snippet or generated answer is only a lead. Retain provider warnings, entity identity, period, currency, units and audit status. A 10-Q is unaudited interim data, not an audited annual statement.
3. Collect evidence for meaning, not form. Keep a document only if it bears on the business, management, valuation, risks or thesis, the growth and profitability an owner weighs. Boilerplate, meaning text with form but no bearing on the company's economics (routine third-party ownership threshold notices, legal notices, contact pages, results about other entities), is not evidence: do not retrieve, list, classify or count it, and its absence is never a gap or `not_checked`. An agent that spends the run processing boilerplate instead of the company's economics has failed the skill; its output is discarded and not retried. For PDF or DOCX originals, retain the full binary, URL, SHA-256, full deterministic extraction and log; follow the PDF-specific OCR and `pdftotext` rules in `company-research/verification-handoff` when applicable. Converted DOCX text does not attest original identity or table fidelity by itself. Retain actual responses as `source_outputs`: `{tool_name, description, output_key, output, source_kind, url, retrieved_at, period, unit}`. Use a unique `output_key` (logged in `pipeline_tool_log` as `{tool_name, output_key, status: "ok"}`, the shape the source-check form reads) also present in `pipeline_tool_log`; unknown metadata is null. `source_kind` is `original` (retrieved passages/provider data), `derived` (computed results), or `synthesis` (generated prose). Keep source content separate from model-written summaries and record failed calls with their errors. Retain each full valuation, signpost and quote-bearing tool result as `output_key.txt` in the run folder and cite `[output_key]`; opt the file into `sources.json` instead of hand-condensing `tool-outputs.json`. Retain model stage outputs as separate stage files with `source_kind: synthesis`, not primary evidence. Pass the source records to `company-research/intel-mosaic` for synthesis and keep them through downstream stages.
4. Build `congruence_rules` using `{quantity, primary_source, cross_check_sources, tolerance, resolution}` for material financial comparisons: original financial statements for historical accounting inputs, `stock_quote` for the quoted market snapshot, declared derivations for calculated values. Align periods/units and justify tolerances from source precision. Build applicable `leak_rules` using `{block, rule_type, pattern}` for financial, valuation and paying-customer assertions. Neither source reputation nor tool transport substitutes for the checks.
5. Use the existing `companies-mcp/research-runs/{run_folder}` public-artifacts run (`run_folder` = `{YYYY-MM-DD}-{ticker-slug}`). Write `sources.json` with `issuer_identifier`, `as_of_date`, and nonempty `sources`: each entry has unique `output_key`, `url`, run-relative `origin_path` and `text_file`, `method`, `period`, and optional `tool_name`, `source_kind`, `description`, `retrieved_at`, `unit`. `origin_path` is required for `text_extraction`/`pdftotext`; keep the full original and deterministic text. Use `method: tool_response` for retained computed results (`source_kind: derived`), and `source_kind: synthesis` for model stage outputs. Keep `key-claims.json` and the exact `drafts/*.md` for each snapshot; declare optional `historical_findings`, `disclosure_inventory`, `congruence_rules`, `leak_rules` JSON file paths in `sources.json` when present, plus `research_run_id` when known. Call `company_research_packet_build(run_folder)` at early and changed draft snapshots: it builds `packet.json`, binds retained texts and returns `packet_sha256` and Tier 1/2 mechanical results. Never build or hash `packet.json` manually. The independent verifier reruns `company_verification_packet_check(run_folder, expected_sha256=packet_sha256)` and checks original identity; a builder result is not approval. Do not store private unpublished text in this plaintext route; without authorized private storage report `not_checked`. Never replace originals with research-ledger excerpts. Before release, read `get_research_run`, disclose recording limitations and annotate only checks performed.

### listen

After INTEL collection, read the company once through the `listening` skill's long-term horizon so downstream stages work from cited strategic evidence rather than raw documents, short-term noise or hype.

1. Split the company's own narrative originals into verbatim passages of at most ~800 words at paragraph or speaker-turn boundaries: current and prior earnings-call transcripts plus the annual report's business and MD&A sections when retained. Each passage is `{source, entity_ref, text}` with `source` = its `source_outputs.output_key`. Exclude financial statement tables, notes and boilerplate. A missing prior call is a `data_gaps` entry.
2. Follow the `listening` skill process with `listening/apply-template-rag` (`corpus_passages`, `kg_triples: []` unless a company graph exists, `company_symbol`, `focus_query` = the note's scope). Verify every quote with `lisp_eval` `string-contains` against its own passage; drop quotes that fail twice.
3. **Materiality exemption.** LISTEN filters strategic narrative, not materiality. Audited statements, risk factors, contingencies, commitments and related-party disclosures bypass it and go to `source_outputs` and `disclosure_inventory` unchanged.
4. Put `listening_view` in `intel_bundle` (keep the originals in `source_outputs`). INTEL hypotheses, FORENSIC management profile, CRITICAL FACTOR and COMMUNICATION catalysts consume it instead of re-reading the raw narrative; consult an original only to check a specific quote or fill a named gap. No input comes from `ignored_short_term`; promotional claims with no dated checkpoint, delivered result or audited support stay context.

### scout-alpha-score

1. Score each component 0–1, then compute the alpha score via `lisp_eval` — fixed weights; add the EM GDP / Bessembinder / low-coverage bonuses (up to +25) to the weighted base:
   - form: "(+ (* coverage_gap 0.30) (* market_cap_fit 0.20) (* sector_relevance 0.25) (* valuation_anomaly 0.25))"
   - env: `{ "coverage_gap": <0–1>, "market_cap_fit": <0–1>, "sector_relevance": <0–1>, "valuation_anomaly": <0–1> }`
2. Apply the 11-criterion excellence universe (S1–S11) if `in_excellence_universe` is true.
3. Emit decision (MUST_COVER / REVIEW_ZONE / DROP — DROP is terminal), alpha_score, horizon_tag, downstream_mode.

### intel-mosaic

1. Synthesize company_research_search and web_search MCP tool outputs into a business context 8-step (identity, geography, business model, competitive position, customers, management, risks, catalysts).
2. Build the information mosaic with source-tier classification and horizon tagging.
3. Form 3–5 testable hypotheses with PENDING / VALIDATED / UNRESOLVABLE lifecycle.
4. Emit mosaic_clear (false = MNPI HALT terminal gate), business_model, news_items, hypotheses, data_gaps.
5. **Intent-routed search:** Call `web_search` with the `intent` hint (news, academic, semantic, freshness, general, transcript) and no explicit `provider` — the tool scores the configured providers against (query, intent) and queries the top recommendation as a single-provider call, surfacing the ranking in `provider_recommendations` and the choice in `selected_provider`. This prevents the arxiv-only fallback that occurs when no provider is explicitly selected and the credential map is stale.

### intel-semantic-classify

1. Render `company-research/intel-semantic-classify` with `ticker`, `intel_bundle`, and retained `source_outputs`. Classify every news_item and hypothesis by ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force, and provenance. Generated summaries are inference even when returned by a tool; semantic tags do not replace mechanical verification.
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
2. Consume company_transcript MCP tool output for verbatim management quotes. If `company_transcript` is unavailable, search for `"{company} earnings call transcript Q{N} {year}"` via `web_search(provider="serpapi")` — SerpAPI supports YouTube transcript extraction. Fall back to `web_search(provider="tavily")` for written transcript sources. Never substitute Wikipedia-sourced paraphrases for verbatim management quotes without flagging a `data_gap`.
3. Emit recommendation (BLOCK terminal gate), management_quality, governance_score, accruals_score.

### valuation-8step

1. Synthesize over four MCP tool outputs (dcf_valuation, comparable_analysis, expectations_gap, scenario_impact_valuation).
2. Produce pt_12m as a weighted blend of the tool outputs via `lisp_eval` — one term per tool output, weights chosen by judgment and stated in the rationale, normalized by the weight sum:
   - form: "(/ (+ (* dcf w_dcf) (* comps w_comps) (* scenario_pt w_siv)) (+ w_dcf w_comps w_siv))"
   - env: `{ "dcf": <DCF fair value>, "comps": <comparables fair value>, "scenario_pt": <scenario-weighted PT>, "w_dcf": <weight>, "w_comps": <weight>, "w_siv": <weight> }`
3. Compute rr_ratio, rating, and the DROP gate via `lisp_eval` (rr = upside to PT / downside to bear-case PT):
   - rr form: "(/ (- pt_12m market_price) (- market_price bear_case_pt))"
   - rating form: "(let ((rr (/ (- pt_12m market_price) (- market_price bear_case_pt)))) (cond ((>= rr 2) 'BUY) ((>= rr 1) 'HOLD) (t 'UNDERPERFORM)))"
   - DROP gate form: "(if (and (< rr 2) (member rating (list \"UNDERPERFORM\"))) 'DROP 'PROCEED)"
   - env: `{ "pt_12m": <blended target>, "market_price": <current price>, "bear_case_pt": <bear-case target>, "rating": <the emitted rating string> }`
4. Compute FaVeS (variant expectations score — where your thesis differs from the market).
5. Emit data_gaps for any failed MCP tool with LLM-derived fallback estimate + confidence penalty (L1/L2 fallback hierarchy).
6. RR < 2:1 + UNDERPERFORM = DROP terminal gate.

### communication-enter

1. Score the ENTER gate (Edge / New / Timely / Examples / Revealing — each true/false), then dispatch via `lisp_eval`:
   - form: "(let ((n (+ (if edge 1 0) (if new 1 0) (if timely 1 0) (if examples 1 0) (if revealing 1 0)))) (cond ((= n 5) 'PUBLISH) ((= n 4) 'ALERT) (t 'DROP)))"
   - env: `{ "edge": <bool>, "new": <bool>, "timely": <bool>, "examples": <bool>, "revealing": <bool> }`
2. Draft the CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data): 300–500 words for PUBLISH eligibility, or 200–300 for ALERT.
3. Compute final_confidence (blend of VALUATION confidence, FORENSIC severity, FaVeS).
4. Confidence < 0.50 = NO_PUBLISH. publication_possible = false is a terminal DROP (KATA and LENS skip). A true value and ENTER's PUBLISH/ALERT are provisional eligibility, not release permission: verify-before-publish must still pass.

### lens-five-frameworks

1. Apply the five intellectual frameworks: The Loop (economic potential, variant expectations, valuation anchor), Superforecasting (granular probabilities, outside view via market_match, certainty-level drift via semantic_tags), Dunning-Kruger (process_confidence vs final_confidence gap, calibration_gap from kata-calibration-measure), Hidden Champions (Simon 8 characteristics), Kauffman (ergodic vs nonergodic, adjacent possible).
2. Reason over market_match and evaluate_evidence outputs (step 18 mcp_batch) plus all prior pipeline outputs.
3. Emit overall_verdict (CONSISTENT / PARTIAL / INCONSISTENT — the convergence signal), key_tensions, pm_memo (200 words). Never blocks publication.

### kata-calibration-measure

1. Close the open kata loop — step 20 (kata-improvement-step1-direction) sets the direction but never measures the gap.
2. (D) Measure the analyst's own calibration, not the market's: read the analyst's resolved price-target forecasts for this symbol with `forecast_list` (stated `forecast_probability`, outcome in band 1/0), then compute with `lisp_eval`:
   - form: `(begin (define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l)))))) (define mean (lambda (l) (/ (sum l) (length l)))) (cond ((is_null ps) (list 1.0 "no_prediction")) ((< (length ps) 5) (list nil "undetermined")) (t (let ((d (- (mean ps) (mean os)))) (list (abs d) (if (> d 0) "overconfident" (if (< d 0) "underconfident" "calibrated")))))))`
   - env: `{ "ps": <stated probabilities>, "os": <outcomes 1/0> }`
   Pass the result as `calibration_gap_measured` when rendering `company-research/kata-calibration-measure`. The `market_calibration` bucket Brier is market context, not the analyst's calibration.
3. Emit calibration_gap (0.0 calibrated → 1.0 maximum gap) and its direction. No prediction recorded = 1.0 (broken feedback loop, not neutral); fewer than 5 resolved forecasts = null (undetermined), not a number.
4. LENS consumes calibration_gap as a 6th axis alongside the existing five frameworks.

### Two-tier verification (applies to both gates)

Verification is a quick pass over everything plus a full pass over what the note stands on. It does not re-derive every fact.

- **Tier 1: all cited claims, mechanical (seconds).** `company_verification_packet_check` returns `tier1_claim_scan`. It matches every figure and quoted phrase on a cited line (`[output_key]`) against that source's full text, tolerating rounding and units. It lists the items it could not match and counts lines without a resolvable retained citation; figures cited to opted-in `output_key.txt` tool results ARE checkable against their full retained text. A match confirms the tool output, not its assumptions or original-source provenance. Tier 1 does not judge context, period, sign or materiality.
- **Formatting for Tier 1:**
  - End each figure line with its citation.
  - Put tool-output figures on their own cited line.
  - Cite model outputs (scores, forecasts) to the stage file that produced them, e.g. `[stages-06-gorilla]`, never to a primary source.
- **Tier 2: key claims, independent full review.** The author lists 5–25 `key_claims` in the packet as `{id, claim, role, output_key, quote}`:
  - `claim`: exact text from the note.
  - `quote`: verbatim source evidence.
  - `role`: one of `thesis_pillar`, `gorilla_evidence`, `valuation_input`, `material_disclosure`, `forecast_checkpoint`, `risk`.
  - Include every claim the rating, price target, catalysts or ENTER score rests on, plus material disclosures.
  - The tool pre-checks each quote verbatim (`tier2_key_claims`). The independent verifier judges support, period, units, context, contrary evidence and materiality.
- **Promotion (selection guard).** The verifier adds every Tier 1 unmatched item, and any unlisted load-bearing claim, to Tier 2 and reports what it promoted.
- **Source identity is established once.** The first gate re-downloads and hashes each original. Later gates re-hash retained files against the earlier receipt in `historical_findings`, and re-download only a new or changed source.
- **Gate.**
  - A key claim that is unsupported, contradicted or misleading blocks publication.
  - A Tier 1 mismatch on a non-key line must be corrected or labelled, but does not block on its own.
  - Disclosure coverage is judged by meaning in Tier 2.
  - `fact_score` covers Tier 2 key claims only (`claims_checked` = key claims checked). `tier1_match_rate` is reported separately.
  - After required corrections, a re-check reviews only the changed lines and carries the prior Tier 2 result forward.

### prepare-summary

After KATA/LENS and any consistency-driven revision, assemble the full CASCADE report draft with all included stage outputs, financial tables and LENS memo; draft the condensed summary now if requested (not after publication). Cite figure lines to their source keys, and keep tool-output figures on their own cited lines. Before freezing, save the entire provisional report and any summary in `drafts/*.md`, update `key-claims.json` and `sources.json` for the retained source files, then call `company_research_packet_build(run_folder)`. It assembles the full `target_text` and returns `packet_sha256`, `tier1_claim_scan` and `tier2_key_claims`. Inspect `tier1_claim_scan` for every unmatched figure or quote and `not_checkable` lines; repair source/text mismatches in the drafts and rerun the builder on the changed files until no unexplained mismatch remains. Label non-key uncheckable lines; correct load-bearing mismatches before a clean freeze; if unresolved within the shared iteration bound, mark the draft incomplete and carry them as blocking open findings into independent Tier 2, never erase them to make the scan look clean. Failure to access the packet or run the scan is `incomplete`, not a pass. Keep the draft-scan receipt in the manifest's `historical_findings` file. This author-side mechanical pass does not establish source identity, context or publication approval; rerun the builder on the corrected exact draft(s) for the frozen independent late gate and use that snapshot's returned `packet_sha256`.

### verify-before-publish

1. After prepare-summary, freeze the exact full report and any requested condensed summary as `target_text`; checking only the headline or conclusion is insufficient. Do not compose a new summary after the draft scan.
2. Ensure `sources.json`, `key-claims.json`, `drafts/*.md` and declared optional inventory/findings/rules files cover the exact frozen target; call `company_research_packet_build(run_folder)` after all corrections. Render the SAME `company-research/verification-handoff` used at candidate commitment with `run_folder`, its returned `packet_sha256`, frozen `original_forecast` and current `working_forecast` (both null when no forecast). Call `spawn_agent` with label `verify-flash-report` and the rendered handoff. The independent verifier reruns `company_verification_packet_check(run_folder, expected_sha256=packet_sha256)` and performs the remaining factual checks. Do not inherit an early/pre-freeze digest or a mechanical-only builder verdict as publication approval. The verifier must load `grounding-verify`, run original-source mechanical checks and independently check the originals' identity and bounded discovery coverage; it must not substitute a generated answer or training knowledge. Delegation/file-access failure is `incomplete`, not permission to self-approve.
3. Consume the canonical `fact_score`, `fact_score_breakdown`, `verified_claims`, `hallucination_findings`, `source_conflicts`, `source_review`, `forecast_integrity`, `data_gaps`, `confidence_adjustment`, `confidence_band`, `decoupling`, and `verification_scope_limitations`. Independently rerun Tier 1 on the frozen drafts, promote unmatched/load-bearing omissions to Tier 2, and perform full Tier 2 key-claim review; the prepare-summary scan is only diagnostic. Check full target coverage and executed mechanical results; require the verifier's `company_verification_packet_check` rerun on the builder's pinned digest rather than an inline excerpt check. Its `[coverage_status, known_omission]` pair tests listed disclosures only; preserve both findings, and bind `material_failure=true` when the second is `material_omission`. `source_review_status` remains `not_checked` until the independent verifier checks original download/hash identity, bounded discovery coverage and materiality; never copy a packet's `mechanical_review[0]` into this field as approval. An exhausted check is `not_checked`, not a reason to substitute excerpts. `not_checked` blocks verified publication and yields an incomplete draft. Bind raw original and working forecast snapshots; then execute the shared gate. Never accept a report author's `checked` field as an oracle. `claims_checked` comes from `fact_score_breakdown.claims_checked`; missing fields are not zero/default success.
4. Gate `incomplete` (including nil, zero claims, omitted sections, unperformed independent source review or in-thread verification): stop publication and return a labelled incomplete draft with the blocker. A missing load-bearing forecast snapshot or OCR-only load-bearing quote cannot silently become a verified factual claim. Gate `needs_work` (score < 0.60 or material failure): re-enter the earliest affected collection/synthesis stage, correct the claim or source and regenerate dependent outputs, including VALUATION/COMMUNICATION/LENS when affected, then re-verify. Preserve earlier records unchanged, keyed by iteration and stage. Do not remove material uncertainty just to improve a score. All retries share the existing maximum of three pipeline iterations; no new retry budget per gate.
5. Only `passed` may reach the final publication decision. Apply the final verifier's `confidence_adjustment` once to COMMUNICATION's unadjusted confidence using `lisp_eval`; floor at zero. Reapply confidence ≥ 0.50 and the original ENTER eligibility. Preserve `confidence_band` separately: a score is not a probability of truth. Scores 0.60–0.79 carry the canonical -0.10 penalty; no score overrides a rejected/unsupported load-bearing factual claim, unresolved load-bearing conflict, or high/critical finding.
6. Publication is permitted only if ENTER eligibility is true, adjusted confidence ≥ 0.50, and the verification gate is `passed`. Execute the following `lisp_eval` form with `enter_eligible` from COMMUNICATION's `publication_possible`, its original `unadjusted_confidence`, and the current verifier's `confidence_adjustment` and `verification_gate`. It returns `[adjusted_confidence, publish]`; missing inputs produce `[nil, false]` and an `incomplete` data gap. Validate numeric types/ranges before evaluation; any evaluation error also blocks publication.

   ```lisp
   (if (or (is_null enter_eligible) (is_null unadjusted_confidence) (is_null confidence_adjustment) (is_null verification_gate)) (list nil false) (let ((adjusted (+ unadjusted_confidence confidence_adjustment))) (list (max 0 adjusted) (and enter_eligible (>= adjusted 0.50) (member verification_gate (list "passed"))))))
   ```

7. LENS consistency cannot override a failed grounding gate. Record the verification report alongside the research report. Any factual change after this check, including condensing, requires verification of the edited deliverable before release; otherwise label it unverified.

### persist-report

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
2. If a condensed flash note was drafted in prepare-summary, export that exact frozen text to `~/Documents/zk-data/companies-mcp/reports/{ticker}-flash-summary-{date}.md`; do not compose new prose here.
3. Both files are markdown in `~/Documents/zk-data/companies-mcp/reports/`. Any factual change from the late-gate target requires re-verification before release; otherwise label the edited deliverable unverified.

## Convergence

The consistency loop remains LENS-driven: CONSISTENT = 0.0, PARTIAL = 0.5 (re-enter VALUATION with tensions), INCONSISTENT = 1.0 (escalate). LENS remains advisory to publication, but is not a factuality check. Publication separately requires verify-before-publish `passed`, ENTER eligibility and adjusted confidence ≥ 0.50. Factual failures re-enter collection or the affected synthesis stage, not merely valuation weighting. `incomplete` stops publication and names the failed measurement. max_iterations: 3 bounds ALL correction cycles together; exhaustion returns a labelled needs_work/incomplete draft, never a publishable note.

## Cross-Skill Composition

- LISTEN reuses the full `listening` skill (`listening/apply-template-rag`, no-fabrication invariant) over the company's own narrative documents; material disclosures are exempt from its horizon filter.
- Step 4 reuses `pragmatic-semantics/semantics-classify-statement` (via `company-research/intel-semantic-classify` adapter) — classifies intel items by IS/OUGHT, declarative/probabilistic/subjunctive before downstream steps consume them.
- Step 20 reuses `kata-improvement/improvement-step1-direction` (Toyota Improvement Kata step 1).
- The kata-calibration-measure adapter reuses `metacognition/meta-experiment` — closes the open kata loop by measuring the calibration gap using the market_calibration Brier score.
- Mandatory verify-before-publish composes `grounding-verify` through `company-research/verification-handoff` and `spawn_agent`. The flash skill owns collection, correction and publication; the verifier checks the supplied evidence without rewriting the report.

## Registry Templates

All templates live in the shared `kask/registry/templates/company-research/` crate (used by both the flash and deep pipelines):

| Template | Purpose |
|----------|---------|
| `verification-handoff.j2` | Prepare a decoupled grounding-verify handoff for a complete company report and retained source outputs, preserving source provenance and surfacing missing evidence. |
| `scout-alpha-score.j2` | Agent 01 SCOUT. Computes the firm-specific alpha score (coverage gap × 0.30 + market cap fit × 0.20 + sector relevance × 0.25 + valuation anomaly × 0.25, plus EM GDP / Bessembinder / low-coverage bonuses up to +25) and applies the 11-criterion excellence universe (S1–S11) where `in_excellence_universe` is true. Emits `decision` (MUST_COVER / REVIEW_ZONE / DROP), `alpha_score`, `horizon_tag`, `downstream_mode` (valentine / gunn / dual). DROP is a terminal early-exit gate. |
| `intel-mosaic.j2` | Agent 02 INTEL (DEEPEN). Business-context 8-step + information mosaic. Consumes `company_research_search` and `web_search` MCP tool outputs (passed via the render_template context from prior direct tool calls) and the `listening/apply-template` earnings-call verdict (cross-skill step 3). Emits `mosaic_clear` (false = MNPI HALT terminal gate), `business_model`, `news_items`, `hypotheses` (PENDING / VALIDATED / UNRESOLVABLE lifecycle), `data_gaps`. Per .rules: failed MCP tools surface as `data_gaps` entries, never collapse to None. |
| `forensic-pre-screen.j2` | Agent 04 FORENSIC (pre-screen). Quick risk pre-screen across accounting red flags, governance, going-concern signals. Emits `severity` (SEV-1 minor → SEV-5 fraud/restatement), `recommendation` (CLEAR+adj / CONDITIONAL / BLOCK), `eps_haircut`, `dr_add_bps`. BLOCK is a terminal early-exit gate. FORENSIC cannot be skipped (EFRA-AI invariant). |
| `critical-factor.j2` | Agent 03 CRITICAL FACTOR. Identifies the 3–5 critical factors that drive the business and constructs Bull / Base / Bear scenarios with EPS impact. Consumes `scenario_build` MCP tool output (passed via the render_template context) for structured scenario generation. Emits `factors` (empty = DROP terminal gate), `scenarios` (bull/base/bear with probabilities and EPS impact), `eps_impact_pct`. Cross-references `superforecasting/stage_3_probability_estimate` methodology for granular (0.35) vs round (0.50) probabilities. |
| `forensic-full.j2` | Agent 04 FORENSIC (full). Full audit: accruals quality, governance (board independence, COB/CEO separation), management profile (owner-operator, capital allocation track record). Consumes `company_transcript` MCP tool output for management quotes. Emits `severity`, `recommendation` (BLOCK terminal gate), `management_quality`, `governance_score`, `accruals_score`. FORENSIC cannot be skipped. |
| `valuation-8step.j2` | Agent 05 VALUATION (DEEPEN). 8-step price target engine. Synthesizes over four direct MCP tool outputs passed via the render_template context: `dcf_valuation` (7a), `comparable_analysis` (7b), `expectations_gap` (7c), `scenario_impact_valuation` (7d). Emits `pt_12m`, `rr_ratio`, `rating` (BUY/HOLD/UNDERPERFORM), `FaVeS` (variant expectations score), `confidence`, `data_gaps` (names any failed MCP tool with LLM-derived fallback estimate + confidence penalty per EFRA-AI L1/L2 fallback hierarchy). RR < 2:1 + UNDERPERFORM = DROP terminal gate. |
| `communication-enter.j2` | Agent 06 COMMUNICATION. ENTER gate (Edge / New / Timely / Examples / Revealing — 5/5 = PUBLISH, 4/5 = ALERT, ≤3/5 = DROP) and CASCADE-format research note (Conclusion → Action → Scenarios → Catalysts → Data). Emits `publication_possible`, `enter_score`, `cascade_note`, `final_confidence`. Confidence < 0.50 = NO_PUBLISH (EFRA-AI invariant). |
| `lens-five-frameworks.j2` | Agent 09 LENS. Consistency auditor. Applies the firm's five intellectual frameworks: Lens 1 The Loop (economic potential, technological capability, variant expectations, valuation anchor Value = Profits / (r − g), target return > 12%, max P/E < 25×), Lens 2 Superforecasting (granular probabilities, inside/outside view balance, clashing forces, observable invalidation — cross-references `market_cmp_index` outside view), Lens 3 Dunning-Kruger (process_confidence vs final_confidence gap, overconfidence risk flag), Lens 4 Hidden Champions (Simon 8 characteristics), Lens 5 Kauffman / Adjacent Possible (ergodic vs nonergodic, new niches, Darwinian preadaptations). Emits `overall_verdict` (CONSISTENT / PARTIAL / INCONSISTENT), `key_tensions`, `pm_memo` (200 words). Never blocks publication. |
| `company-8part.j2` | Agent 13 COMPANY (DEEPEN). Deep 8-part company analysis: Self-View, Business Franchise, Management Skill (CEO long-term + CFO working capital scorecards), Financial Profile (signposts + 3-stage valuation), Invisible Layer, Falstaffian Inversion, Value Gorilla Elevator Pitch, Investment Thesis Statement. Consumes `company_transcript`, `dcf_valuation`, `comparable_analysis`, `web_search`, `fetch` MCP tool outputs (passed via the render_template context from prior direct tool calls). Consumes retained `source_outputs`; emits `CompanyBoard` with all 8 sections, `claim_sources`, and `data_gaps`. |
| `falstaffian-competitive-rotation.j2` | Rotates the competitive framing of the Company Board before GORILLA scores it. Applies Falstaffian semantic rotation shapes (predicate hollow, subject expansion, object inversion, direction reversal) to expose framing errors in the analyst narrative. Emits rotated_board with competitor-complement analysis, market creator vs participant classification (Wardley evolution axis), framing errors detected, and rotated competitive position. Anchored to MAIA "Falstaff: Give Me Life", "Competition: Readings vs Reality", "Company Analysis", "Thinking Like an Owner". Cross-references metacognition/falstaffian-perspective-engine shapes and decision tree. |
| `gorilla-4dim.j2` | Agent 10 GORILLA. Value Gorilla 4-dimension framework with fixed methodology weights (Obvious Problem 25% / Invisible Gorilla 30% / Combinatorial Solution 25% / Choke Point 20%). Weights are fixed by firm methodology — NOT user-tunable, so mcda was rejected (essentialist Surface gate: adds ceremony for fixed weights). Scoring is a `lisp_eval` call, not an mcda call. GORILLA consumes the ROTATED board (from falstaffian-competitive- rotation), not the raw Company Board — the rotation corrects framing errors before scoring. Emits `gorilla_score`, `verdict` (GORILLA ≥75 / SMALL_ANIMAL 50-74 / PEDESTRIAN <50), per-dimension scores. |
| `economic-trajectory.j2` | Economically-anchored imagination scaffold. Identifies the falling-cost trajectory in the subject's industry (McAfee dematerialization), the design constraint being removed (MAIA bottleneck framework), the Coasean firm-boundary shifts (Kauffman economic web), the Kauffman adjacent possible nodes (never-before-born goods and services, Darwinian preadaptations), and convergence vectors (Diamandis). Emits economic_trajectory with falling_cost, constraint_being_removed, coasean_shifts, adjacent_possible_nodes, convergence_vectors, implications_for_ subject, trajectory_velocity. IMAGINE consumes this as the anchor for its 5/10Y scenarios. Anchored to MAIA "Focus and Imagination", "More From Less", "Kauffman Readings", "The Future Is Faster", "Bottlenecks and Critical Mass", "Time Horizons". |
| `imagine-longrange.j2` | Agent 11 IMAGINE. Projects the business at 5 and 10 years and walks it back analytically. Digital Transformation Stages (MODEL / SHADOW / TWIN / SOURCE), Growth Driver Classification (innovation / demographic / both / neither). Scenarios are ANCHORED on the economic trajectory probe (falling cost, constraint removal, adjacent possible) and CHALLENGED by the Falstaffian rotations (rotated competitive framing, framing errors detected). Consumes `scenario_build` MCP tool output and the `economic_trajectory` probe. Emits `ImagineBoard` with digital stage, growth driver, 3 scenarios (each with trajectory_anchor and falstaffian_challenge), 3–5 falsifiable predictions (tagged by horizon, each with trajectory_basis), what's not on the page (anchored on adjacent possible), what's not in the price (anchored on trajectory implications), trajectory_anchoring, falstaffian_challenge. |
| `thesis-three-pillars.j2` | Agent 12 THESIS. Synthesizes all prior research into a formal investment thesis covering the three pillars: Business Franchise (moat strength, value creation, durability), Management Quality (capital allocation, leadership), Valuation (3-stage: consensus → normalization → terminal). States whether the accumulated evidence supports a thesis, contradicts it, or is insufficient, with an explicit price conclusion at the as-of price. Emits no quality verdict — an investment-grade judgment is a separate outcome the deep pipeline never claims. |
| `intel-semantic-classify.j2` | Cross-skill adapter. Adapts pragmatic-semantics/ semantics-classify-statement to the INTEL mosaic. Classifies every news_item and hypothesis by ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force, and provenance — BEFORE downstream steps consume the intel. Prevents certainty-level drift: a management quote treated as an ontological fact, a scenario treated as a forecast. Emits semantic_tags and certainty_drift_risk that downstream templates (forensic, critical- factor, valuation) consume via intel_bundle.semantic_tags. |
| `gorilla-capability-reason.j2` | Tests the GORILLA 4-dim scores against capability floor, ceiling and maturity-gate limits. Types each GORILLA dimension (Obvious Problem, Invisible Gorilla, Combinatorial Solution, Choke Point) against a capability registry with floor, ceiling, and maturity-gate limits. The GORILLA score (0–100) is the elicited potential; the capability assessment determines whether that score is credible against the company's observed behavior and maturity. Emits capability_assessments, floor_violations, ceiling_violations, maturity_blocks. A maturity block contributes 0 for that dimension in the fixed-weight `lisp_eval` calculation while preserving the block and source gaps. |
| `kata-calibration-measure.j2` | Cross-skill adapter. Adapts metacognition/meta-experiment to close the flash pipeline's open kata loop. Flash step 20 (kata-improvement-step1-direction) sets the direction but never measures the gap. This step measures the analyst's calibration gap using the market_calibration Brier score (step 19) and resolved_outcomes (step 18), then re-measures the current condition. Emits calibration_gap (0.0 calibrated → 1.0 maximum gap) that LENS (step 23) consumes as a 6th axis alongside the existing five frameworks. |
| `wardley-anchor.j2` | Cross-skill adapter. Compresses wardley-mapper's 6-step process (inventory → classify → map → movement → recommendations → present) into a single LLM call over the rotated Company Board. Emits wardley_map with components, evolution classifications, movements, commoditization candidates, choke_points, and invisible_gorillas. Feeds GORILLA's Invisible Gorilla and Choke Point dimensions (step 5) and ECONOMIC TRAJECTORY's falling-cost anchor (step 9). The full wardley-mapper skill is available for standalone use — this adapter exists to ground the deep pipeline's strategic analysis without adding a 6-step sub-process. |

To render a template, call the `render_template` tool with the template ref (e.g., `company-research/scout-alpha-score`) and a context object with the required variables.

The deep-only templates (`company-8part.j2`, `evidence-review.j2`, `falstaffian-competitive-rotation.j2`, `wardley-anchor.j2`, `gorilla-4dim.j2`, `gorilla-capability-reason.j2`, `economic-trajectory.j2`, `imagine-longrange.j2`, `thesis-three-pillars.j2`) are documented in the `company-research-deep` SKILL.md.

## MCP Tool Integration

All MCP tool calls are called directly (deterministic, governed, testable). See `kask/docs/architecture/skills-and-composition.md` Part II for the invocation patterns. Failed MCP tools surface as `data_gaps` entries in the consuming template — never collapse to None (per .rules).

## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- MCP tool failures must not collapse to None. Templates emit `data_gaps` entries naming the failed tool.
- No `unwrap_or(0)` on regulation signals. Missing LENS verdict surfaces as 1.0 (worst case), not silently converged.
- Reports are written as markdown files to `~/Documents/zk-data/companies-mcp/reports/` via `terminal` (see persist-report — never the source tree or the hidden internal data dir).
- Rendering verification-handoff is not executing verification. A missing, nil, zero-claim, partial-coverage or in-thread verification result cannot authorize publication.
- A high fact_score never overrides an unresolved material finding. LENS/ENTER are not substitutes for grounding; apply the current final verification adjustment once and retain its independently derived confidence band.
- Historical findings remain append-only and outside the current score denominators. Corrections require a new verification record and regenerate affected downstream outputs; max_iterations: 3 is shared across all re-entry paths.
