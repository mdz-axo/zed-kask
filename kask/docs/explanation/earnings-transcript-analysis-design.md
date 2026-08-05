# Earnings-Call Transcript Analysis — Design Exploration

Date: 2026-08-05. Status: design only, no implementation. Two epistemic phases were kept
strictly separate: Phase A states only what was verified against live documentation or the
codebase (IS); Phase B states normative design choices, each carrying a `depends on` clause
tied to a Phase A finding (OUGHT). Unverified items are marked UNVERIFIED, not guessed.

---

## Phase A — Data availability (IS)

### A1. EODHD transcript endpoint: **does not exist** (verified absence)

- The full doc index at `https://eodhd.com/financial-apis/` lists ~45 APIs; no
  earnings-call transcript API appears. The earnings-adjacent product is the Calendar API
  (`/api/calendar/earnings` — dates/EPS estimates, not transcripts).
- The docs sitemap (`/financial-apis/post-sitemap.xml`, 76 URLs) contains no "transcript" URL.
- `https://eodhd.com/financial-apis/earnings-call-transcripts-api` → HTTP 404.
- Consequence: **EODHD is excluded from Phase B.** (Caveat: an EODHD Marketplace
  third-party listing is theoretically possible — UNVERIFIED either way.)

### A2. FMP transcript endpoint: **exists** (verified via archived docs)

- `GET https://financialmodelingprep.com/api/v3/earning_call_transcript/{symbol}?year={YYYY}&quarter={1-4}`
  — documented at `https://site.financialmodelingprep.com/developer/docs/earning-call-transcript-api`
  (verified via Wayback capture 2025-11-20; the live docs host returns HTTP 403 to
  non-browser fetchers, UNVERIFIED whether current docs differ).
- The v3 endpoint is flagged **legacy**; a newer `/stable/` replacement exists but its
  exact path is UNVERIFIED. The codebase already uses `FMP_BASE_URL =
  "https://financialmodelingprep.com/stable"` (`providers.rs:27`), so the current path
  must be confirmed before implementation (likely `/stable/earning-call-transcript` —
  UNVERIFIED).
- Response fields: `[symbol, quarter, year, date, content]` — a single `content` text
  blob. **Full text: verified. Speaker segmentation: UNVERIFIED (docs show none).
  Timestamps: no (none documented).**

### A3. History depth

- The endpoint is keyed by explicit `year`+`quarter`, so there is no documented cap on
  the retrievable window; the earliest retrievable quarter is UNVERIFIED. Practically,
  the last 20 quarters (5y) are retrievable by iteration. FMP's "15+ years" claim
  appears in marketing but was not verified on a docs page.
- "Annual transcripts" are not a distinct artifact: the fiscal-year-end call is the Q4
  (or fiscal-Q4) transcript. Retrievable via the same endpoint; fiscal-quarter mapping
  for non-calendar fiscal years is UNVERIFIED.

### A4. Plan gating

- UNVERIFIED. The archived page shows generic plan boilerplate; whether transcripts are
  gated above the plan the `HKASK_FMP_API_KEY` key is on must be checked with a live
  probe (`GET /stable/earning-call-transcript?symbol=AAPL` with the real key) before any
  slice that depends on it.

### A5. Codebase state (IS, from repo inspection)

- Companies server: `kask/mcp-servers/hkask-mcp-companies/` — 40+ tools; providers FMP
  (`/stable`) + EODHD with per-endpoint routing/fallback (`src/providers.rs`). No
  transcript tool exists. No earnings-call-transcript code exists anywhere in the repo.
- Corpus server: `kask/mcp-servers/hkask-mcp-corpus/` — `corpus_chunk` does
  **token-granularity** chunking only (`tools/document.rs:174` →
  `hkask_memory::SemanticMemory::chunk_text`); **no speaker-turn parsing, no
  prepared-remarks/Q&A section detection, no diarization anywhere**. The only
  transcript-specific code is YouTube transcript *fetch* for corpus discovery
  (`src/corpus/discover/search.rs:107-260`). `corpus_tag_chunks` can post-hoc annotate
  chunks with 5W1H/ontology tags.
- **MAIA guidebook notes: not present in the repo.** `find -iname '*maia*'` returns zero
  files. MAIA exists only as in-code methodology fragments:
  - Gross-margin stability via coefficient of variation, score `1/(1+CV)`
    (`companies/src/analysis.rs:11-23`).
  - Working-capital spread = DPO − DSO; positive spread ⇒ market power, with signal
    labels `strong_market_power`/`moderate_market_power`/`neutral`/`supplier_dominated`
    (`analysis.rs:25-42`).
  - Moat = Wide (stable margins AND positive spread) / Narrow (either) / None; needs ≥3
    periods (`analysis.rs:54-68`).
  - CEO capital-allocation rule: good = decreasing capital with steady/improving returns,
    or increasing capital with improving returns; bad = increasing capital with
    decreasing returns (`analysis.rs:287-318`).
  - CFO working-capital consistency: "the level is structural; consistency is management
    skill" (`tools/analysis.rs:236-243`).
  - Event-tree model: binomial yes/no events with deadlines, conditional probability
    tables, joint-table marginalization (`docs/explanation/forecasting-and-scenarios.md:74-80`).
  - Three-level certainty tier (`hkask-forecast/src/hkask_forecast.rs:158`); moat→fade
    horizon (`economic_profit.rs:73-77`).
  A richer MAIA guidebook, if it exists, lives outside this repo (operator's private
  notes). This is a documented gap, not a fabrication point.

---

## Phase B — Design (OUGHT)

Every recommendation cites its Phase A dependency.

### (a) Transcript-analysis tool design

One new tool on the **companies** server, plus reuse of corpus tools — no new server.

```
earnings_transcript(symbol, year?, quarter?, quarters_back=1, mode=fetch|analyze)
  → { transcripts: [{symbol, year, quarter, date, content, source_endpoint}],
      segments?: [{section: prepared_remarks|qa, speaker?, text}],   // analyze mode
      coverage: {requested_quarters, retrieved_quarters, missing: [..]} }
```

- Fetch: FMP `/stable/...` (or legacy v3) `earning_call_transcript` per (year, quarter);
  `quarters_back=20` iterates the last 20 quarters; `coverage` reports gaps honestly.
  *Depends on A2 (FMP-only source), A3 (iteration for 20 quarters), A4 (plan probe).*
- Analyze mode: rule-based segmentation of `content` into `prepared_remarks` vs `qa` and
  per-speaker turns via the stable textual conventions in transcripts ("Operator:",
  "Question-and-Answer Session", "Executives:", "Analysts:" headers).
  *Depends on A2 (content is a single unstructured blob — segmentation must be built
  in-repo; corpus cannot do it, per A5).* Because A2 leaves speaker formatting
  UNVERIFIED, segmentation must ship with a fixture-driven test over one real FMP
  `content` blob, and must degrade to a single `full_text` segment when no conventions
  match — never fabricate speaker labels.
- Storage: transcripts cached via `corpus_cache`; analysis outputs attached to the
  company via existing `note_add`/`file_attach` portfolio tools. *Depends on A5 (those
  tools already exist — no new storage surface).*
- Errors: per-quarter fetch failures are collected into `coverage.missing`, not
  propagated as whole-tool failure; the tool fails only when zero quarters succeed.
  Idiomatic-Rust note (verdict c includes a Rust tool, so this applies): request type
  `#[derive(JsonSchema)]` with `hkask_mcp_server::AnyJsonValue` for any free-form field;
  `anyhow::Result` with `?` propagation; FMP non-2xx mapped per-variant via a
  `map_fmp_error` fn (per the repo's per-variant error-classification rule), no
  `unwrap()`; fiscal-quarter arithmetic returns `Option` rather than panicking on
  quarter boundaries.

### (b) Listening template spec

A YAML/JSON schema versioned in-repo (`kask/mcp-servers/hkask-mcp-companies/listening_template.yaml`
or a skill asset). Each factor is anchored to the MAIA fragments found in A5 plus the
horizon doctrine supplied by the operator (below) — no invented factors.

**The horizon doctrine (operator-supplied MAIA assumption, 2026-08-05) — this is the
set-point of the whole template and is hard-coded, not optional:**

> MAIA assumes the short term is efficiently covered and fairly well known — no
> inferential advantage exists there. The advantage lives in longer-term trends in
> growth, profitability, and investment: events and plans in the **12–36 month**
> window are essential; events in the next **3–9 months** are not key; and the edge is
> not 5+ years out either — it is being longer-term than most investors, not maximal.
> Consequence for listening: a guidance change matters **iff** (i) it is a change in
> long-term guidance, or (ii) it is a short-term change that signals/indicates a
> long-term change in growth or profitability. Everything else discussed on the call
> is noise unless it carries 12–36-month significance.

This is implemented as a **global stance block** that every section's extraction and
verdict passes through, plus a dedicated classification of every extracted claim by
horizon. The template refuses to emit a verdict on a claim whose horizon class is
`short_term_only` with no long-term signal linkage — such claims are recorded as
`ignored_short_term` (kept for audit, excluded from verdicts), which is the concrete
mechanism that prevents the analysis from being dragged into the efficiently-priced
window.

```yaml
version: 2  # v1 → v2: horizon doctrine hard-coded (operator-supplied MAIA assumption)
source_of_factors:
  - maia-in-code-fragments      # see A5; guidebook NOT in repo
  - operator-supplied horizon doctrine (2026-08-05)  # 12–36mo window; 3–9mo not key; not 5y+
stance:                          # GLOBAL FILTER — applies to every section below
  inferential_advantage_window_months: [12, 36]
  efficiently_priced_window_months: [3, 9]
  far_horizon_months: 60         # beyond ~5y is also NOT the edge — treat as speculative
  claim_horizon_classes:         # every extracted claim is classified before use
    - short_term_only            # <12mo effect, no long-term linkage → recorded, excluded from verdicts
    - short_term_signal          # <12mo event that indicates a 12–36mo change → admissible WITH the linkage stated
    - long_term                  # 12–36mo effect → admissible, primary material
    - speculative_far            # >~48–60mo → admissible only as low-weight context (confidence cap 1)
  admissibility_rule: >
    A claim enters a section verdict only if horizon_class is long_term, or
    short_term_signal with an explicit stated linkage to growth/profitability/
    investment in the 12–36 month window. short_term_only claims are logged under
    ignored_short_term. speculative_far claims cap confidence at level 1.
sections:
  - id: margin_trajectory          # MAIA: gross-margin stability (analysis.rs:11-23)
    listen_for:
      - management commentary on gross-margin direction, pricing, input costs
      - quantified margin guidance vs prior-quarter guidance
    extract: { claims: [verbatim_quote + speaker + section], numbers: [margin figures] }
    maps_to_tool: key_metrics        # quantitative confirmation after the call
  - id: working_capital_power      # MAIA: DPO−DSO spread (analysis.rs:25-42)
    listen_for:
      - receivables/payables/inventory commentary, customer payment terms, supplier pressure
    extract: { claims: [...], signals: [customer_concentration, term_changes] }
    maps_to_tool: working_capital_cycle
  - id: moat_evidence              # MAIA: moat classification (analysis.rs:54-68)
    listen_for:
      - pricing power statements, churn/retention, competitive-response language in Q&A
    extract: { claims: [...], analyst_challenges: [...] }   # Q&A pushback is the sensor
    maps_to_tool: moat_check
  - id: capital_allocation         # MAIA: CEO rule (analysis.rs:287-318)
    listen_for:
      - capex/M&A/buyback/dividend announcements and their stated return expectations
    extract: { claims: [...], numbers: [capex, buyback, acquisition spend] }
    maps_to_tool: management_scorecard
  - id: guidance_vs_expectations   # event-tree prior input (forecasting-and-scenarios.md)
    # HORIZON DOCTRINE APPLIED IN FULL: a guidance change matters iff (a) it is a
    # long-term guidance change, or (b) it is a short-term change that signals a
    # long-term improvement/deterioration in growth or profitability. A bare
    # next-quarter raise/cut with no stated long-term linkage is recorded as
    # ignored_short_term and must NOT move any forecast_record prior.
    listen_for:
      - changes to multi-year / long-range guidance (revenue CAGR, margin targets,
        capital-intensity plans) — primary signal
      - short-term guidance changes WITH management's stated or clearly implied
        linkage to long-term trajectory (e.g. "pulls forward capacity", "reflects a
        durable demand shift", "one-time supply constraint") — admissible as signal
      - short-term guidance changes with NO long-term linkage — record and ignore
      - new quantified commitments with deadlines in the 12–36mo window
    extract:
      commitments: [{statement, deadline?, quantitative?, horizon_months_estimate}]
      guidance_changes:
        [{statement, direction: raised|lowered|withdrawn|initiated,
          horizon_class: long_term|short_term_signal|short_term_only,
          long_term_linkage: verbatim_quote_or_null}]   # null linkage + short_term_only ⇒ inadmissible
    maps_to_tool: expectations_gap, forecast_record
  - id: management_consistency     # MAIA: consistency-is-skill (analysis.rs:236-243)
    listen_for:
      - this-quarter statements vs prior-quarter transcript claims (needs quarters_back≥2)
    extract: { contradictions: [{prior_quote, current_quote}], tone_shift: enum }
    maps_to_tool: (cross-transcript diff — new, see slice 4)
output:
  per_section: { verdict: corroborates|neutral|contradicts, evidence: [quotes], confidence: 1|2|3 }
  # confidence uses the MAIA three-level certainty tier (hkask_forecast.rs:158)
  horizon_summary:               # doctrine-mandated top-level shape
    long_term_findings: [{claim, section, horizon_class, evidence}]
    ignored_short_term: [{claim, reason: no_long_term_linkage}]
    speculative_far: [{claim, confidence_capped_at: 1}]
  # INVARIANT: no verdict or forecast input may be derived from ignored_short_term
  # entries. Golden-file test asserts every forecast_record-affecting claim has
  # horizon_class long_term or short_term_signal with non-null linkage.
```

Every extracted claim must carry a verbatim quote + location; the template output never
contains a verdict without evidence. *Depends on A5 (factor list is exactly the MAIA
fragments in the repo; if the operator's external guidebook is later supplied, the
template gains sections — the schema must tolerate extension).*

### (c) Verdict: **a bit of each** — thin MCP tool (fetch + mechanical segmentation) on the
companies server; the listening template and its evaluation live in a **skill**.

Rationale: what belongs in the MCP tool is everything deterministic and provider-shaped
(auth, endpoint iteration, coverage accounting, rule-based segmentation) — the
governance-membrane pattern the other servers already follow. What belongs in the skill
is everything judgment-shaped: the listening template is a *semantic evaluation
procedure* over text, identical in kind to how `scenario_extract`/`structured-extraction`
skills delegate interpretation to the model; baking it into Rust would freeze a
judgment artifact into code and require a server release to change a listening factor.

**Falsifiable refuting evidence** — the verdict flips if any of these are observed:
1. Segmentation of real FMP `content` blobs turns out to be >90% of the analysis value
   (i.e., sections are already cleanly delimited and model judgment adds little) →
   collapse the skill into the tool.
2. The template changes less than once per quarter in practice → code-freezing it into
   the tool is cheap; merge.
3. The skill is invoked with no companies-server fetch (users paste transcripts) → the
   tool is unnecessary; template-only skill.
4. FMP ships pre-segmented speaker turns (A2 UNVERIFIED item resolves to "yes") → delete
   the tool's segmentation slice entirely.

### (d) Verifiable vertical slices

Each slice is end-to-end testable; acceptance criteria are stated, not implied.

1. **Plan-gating probe** (no code): with the real `HKASK_FMP_API_KEY`, call the current
   FMP transcript endpoint for AAPL 2023Q1..2025Q4. Accept: record which path works,
   HTTP status per quarter, whether `content` has speaker markers. Resolves A2/A3/A4
   UNVERIFIED items. *Blocks slices 2–4.*
2. **Fetch tool**: `earnings_transcript(mode=fetch)` for one (year, quarter).
   Accept: returns `content` + date for a known quarter; `coverage.missing` populated
   for a quarter with no call (e.g. pre-IPO); error classification test per repo rules.
3. **Window fetch**: `quarters_back=20`. Accept: 20-quarter retrieval for a long-listed
   ticker (e.g. MSFT) with explicit coverage report; gaps reported, not invented.
4. **Segmentation**: fixture test over a real captured blob from slice 1.
   Accept: prepared_remarks/QA split and speaker turns extracted when markers present;
   single `full_text` segment + `segmentation: degraded` flag when absent. No fabricated
   speakers.
5. **Listening-template skill**: skill that takes a transcript (tool output or pasted
   text), applies the §(b) template, emits per-section verdicts with evidence quotes.
   Accept: golden-file test — run on the slice-1 fixture, every verdict cites ≥1
   verbatim quote; a fabricated quote fails the test (substring check against source);
   **horizon-filter test** — the fixture must contain at least one bare short-term
   guidance change, and the output must place it in `ignored_short_term` with no
   verdict influence; a verdict derived from it fails the test.
6. **Corpus integration**: transcript cached via `corpus_cache`, analysis note attached
   via `note_add`. Accept: round-trip queryable through existing corpus tools; no new
   storage code.

---

## Skill-role accounting (as assigned in the task)

- **pragmatic-semantics** — enforced the IS/OUGHT split; the UNVERIFIED markers in
  Phase A are its output. Fabricated claims flagged: none remain in this doc; the FMP
  "15+ years" marketing claim was explicitly demoted to UNVERIFIED.
- **pragmatic-cybernetics** — the loop: **sensor** = listening template applied to the
  transcript; **set-point** = MAIA key factors (A5 fragments) **plus the horizon
  doctrine** — and the doctrine is what makes the set-point *discriminating*: without
  it the sensor amplifies whatever the market already prices (3–9mo noise), i.e. zero
  inferential variety; the stance block is the variety attenuator that filters the
  efficiently-covered window before it reaches the actuator; **actuator** = corpus MCP
  cache/tag + portfolio `note_add` (writes the analysis where decisions read it);
  **corrective action** = updated `forecast_record` / `expectations_gap` inputs when the
  transcript contradicts the prior. **Closure diagnosis**: the loop terminates per
  quarter (one pass over one transcript, fixed template, no iteration) — it is a
  sense→act loop, not an optimizing loop; the outer PDCA (compare extracted commitments
  against next quarter's outcomes via `forecast_record` + Brier) is where calibration
  lives. Open risk: without the outer loop wired, transcript analysis produces
  uncorrected verdicts — the `guidance_vs_expectations` section's `forecast_record`
  mapping is the closure mechanism and should be non-optional.
- **essentialist** — deletion tests: (i) *listening template as a file* vs inline prompt:
  deleting it would scatter the same factors across every ad-hoc prompt and break the
  golden-file test (slice 5) — it survives; (ii) *standalone transcript tool* vs
  composing corpus tools: `corpus_chunk` cannot segment and nothing fetches FMP
  transcripts — the tool survives but is cut to fetch+segment only (analysis deleted
  from it, moved to the skill); (iii) a new server: deleted — companies server already
  owns FMP credentials and routing.
- **falsifiability** — each design hypothesis carries its refuter: template-wrong
  evidence = a MAIA-anchored section whose extracted claims never change any
  `forecast_record`/decision over 4 consecutive quarters (delete the section);
  **horizon-doctrine-wrong evidence** = over ≥8 quarters, `ignored_short_term` claims
  that were excluded would have produced better-calibrated `forecast_record` priors
  than the admissible long-term claims (measurable via Brier comparison of the two
  populations — if the short-term population beats the long-term one, the doctrine's
  core empirical premise fails in this domain and the stance block must be revised,
  not silently loosened); template-unnecessary evidence = refuter #2 in §(c);
  tool-unnecessary = refuter #3.
- **grill-me** — stress-test of §(c): *Recall* — the split is deterministic-fetch vs
  judgment-eval; *Mechanism* — MCP tool = governed provider call, skill = model-mediated
  procedure; *Rationale* — change-frequency and governance arguments above; *Edge* —
  pasted-transcript usage (refuter #3), FMP pre-segmentation (refuter #4), template
  template churn (refuter #2); *Edge (added)* — the horizon doctrine as a global
  filter: if the doctrine were a per-section hint instead of a hard-coded stance, the
  model would drift to recency-salient short-term items under time pressure; making it
  a schema-level admissibility rule with an `ignored_short_term` audit channel is what
  makes the doctrine enforceable and testable rather than aspirational; *Synthesis* —
  the verdict is a composition rule, not a taxonomy:
  the membrane hosts what must be governed and stable; the skill hosts what must evolve.
  The verdict survived with refuters attached rather than weakened.
- **sequential-inquiry** — drove Phase A→B ordering: probe providers → probe codebase →
  fix factor source (guidebook absent → in-code fragments) → design. Iterates stabilized
  after one refinement (template factors re-anchored from "guidebook" to code fragments
  once absence was established); a second pass changed no design element → stop.
- **idiomatic-rust** — applied to §(a) (verdict includes a Rust tool): type-driven
  request/response, per-variant error mapping, `?` propagation, no panics, `AnyJsonValue`
  for free-form JSON per repo schema rules.
- **lean-prover** — **not applicable**, as anticipated: no formal contract obligation
  arose; the design's guarantees (coverage honesty, evidence-cited verdicts) are test
  obligations, not proof obligations.

## Deferred to user verification (not assumed)

1. Live FMP probe for plan gating, current `/stable/` path, and real `content` shape
   (slice 1).
2. Whether the external MAIA guidebook exists outside this repo; if supplied, template
   §(b) gains sections (schema is extension-tolerant).
3. Corpus breakdown capability: **verified negative** — `corpus_chunk` is token-based
   only; no speaker/section segmentation exists. The design builds segmentation in the
   companies tool rather than around the corpus server.

## Convergence statement

All four deliverables produced; every Phase B recommendation carries a `depends on`
clause; the §(c) verdict carries four stated refuting observations. Refuting evidence
for the whole design: slice-1 probe showing FMP transcripts unavailable on the current
plan (design collapses to template-only skill over pasted transcripts), or the external
MAIA guidebook surfacing with materially different factors (template §(b) re-derived,
tool unchanged).
