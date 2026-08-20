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

### A2. FMP transcript endpoint: **exists** (verified live 2026-08-05 with the operator's key)

- **Working endpoint (verified):** `GET https://financialmodelingprep.com/stable/earning-call-transcript?symbol={SYM}&year={YYYY}&quarter={1-4}`
  — returns HTTP 200 with the operator's key. The legacy v3 path
  (`/api/v3/earning_call_transcript/{symbol}`) returns **403 "Legacy Endpoint … only
  available for legacy users … prior August 31, 2025"** — dead for this subscription.
- **Plan gating: RESOLVED** — the operator's current plan serves transcripts (200 on
  `/stable/`). No upgrade needed.
- **Response fields (verified):** `[symbol, period, year, date, content]` — `period` is
  `"Q1".."Q4"`, `content` is a single text blob (~45–51k chars for AAPL).
- **Shape (verified on AAPL 2023Q1, 45,583 chars):** speaker markers PRESENT —
  `Name:` at line starts (13 distinct labels: `Timothy Cook` ×14, `Operator` ×12,
  `Luca Maestri` ×6, analysts by name); `Question-and-Answer` section marker PRESENT;
  `Operator:` PRESENT; **timestamps ABSENT**. Segmentation into prepared-remarks/Q&A +
  per-speaker turns is therefore feasible by rule-based parsing (design §(a) segment
  mode is viable as specified).
- **Caveat (verified oddity):** AAPL 2023Q1 returns `"date": "2012-03-19"` — the
  `date` field is unreliable (likely first-call date); use (year, period) as the
  temporal key, not `date`.
- **History depth (verified):** 200 with full content at 2010Q3 (50,927B), 2015Q2,
  2020Q4; **empty array `[]` at 2005Q2 and 2000Q2** — the floor is somewhere between
  2005 and 2010 for AAPL. 20 quarters (5y) and 10 fiscal-year-ends are comfortably
  retrievable; the "15+ years" marketing claim is plausible but the exact floor is
  per-symbol and UNVERIFIED beyond these probes.

### A3. History depth (verified live 2026-08-05)

- Verified retrievable: 2010Q3, 2015Q2, 2020Q4 (full content). Empty (`[]`) at 2005Q2
  and 2000Q2 for AAPL — floor between 2005–2010, per-symbol. The endpoint is keyed by
  explicit `year`+`quarter` with no documented cap; 20 quarters (5y) and 10
  fiscal-year-ends are retrievable by iteration.
- "Annual transcripts" are not a distinct artifact: the fiscal-year-end call is the Q4
  (or fiscal-Q4) transcript. Retrievable via the same endpoint; fiscal-quarter mapping
  for non-calendar fiscal years is UNVERIFIED (AAPL's Q1-2023 = fiscal Q1 ending Dec
  2022 — the `period`/`year` labeling follows FMP's own fiscal mapping).

### A4. Plan gating (RESOLVED 2026-08-05)

- The operator's current plan serves `/stable/earning-call-transcript` (HTTP 200).
  Legacy v3 is 403 for this subscription. No plan change needed for the design.

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
- **MAIA guidebook: FOUND (outside the repo)** — `/home/mdz-axolotl/Clones/Library/MAIA-Substack/posts/`
  (55 HTML posts). Extracted 2026-08-05; substance relevant to transcript analysis:
  - **Horizon post** (`137682783.time-horizons-expected-events-and`): sell-side consensus
    "probably actually fairly accurate for the next 12-18 months" — use consensus for the
    near term, don't compete there; 3-stage value model (1–2y consensus / 3–5y
    normalization / 5y terminal multiple, 15% target return); horizon bands **Tactical
    12–18mo / Strategic 3–5y / Long-term 7–10y**; edge quote: "We are not predicting the
    future – but if we can more quickly see what has happened and rapidly discard things
    that are not possible – well, that's how one gets an analytical advantage that isn't
    based on luck." Certainty levels: **Proximate ≥67% / Probable 33–66% / Possible <32%**.
  - **Company template** (`137738813.company-template`): BUSINESS / MANAGEMENT /
    VALUATION / RISKS / FUTURE; "how does the company take one dollar and turn it into
    two?"; CEO/CFO scorecards; "What expectations are in the current price?"; "Why do we
    think it's invisible, neglected or misunderstood?"; FUTURE = "what we are watching
    for in the next 18 months" + how expected events relate to expectations.
  - **Company-analysis method** (`137881063.company-analysis`): start with how the
    company describes itself (filings/presentations first, NOT sell-side); the "hole" =
    "the gap between the expectations of the market and the potential of the business";
    thesis arc: market expects X, we expect Y, Y > X; position-building signal = seeing
    "what isn't on the page yet."
  - **Financial signposts** (`137464131.financial-signposts`): market power = gross-margin
    **stability through a cycle** ("Market power is not the ability to increase gross
    margins") + DPO−DSO negative working capital; CEO long-term capital allocation
    ("the no-no is increasing capital and decreasing returns"); CFO working-capital
    consistency ("stable working capital accounts in the face of shifting market
    environments requires exceptional discipline and skill"); caveat: watch for
    accounting games (Schilit/Mulford/Comiskey).
  - **Owner mindset** (`137670567.thinking-like-an-owner`): owners worry about growth
    and profitability, not trading patterns; "part of the job is to ignore a lot of
    noise"; if you understand the business you can predict management's reasoning —
    unexplained actions mean you are still learning the business.
  - Grep-verified absences across all 55 posts: "earnings call", "transcript" (finance
    sense), "12–36", "inferential advantage", "moat" — zero hits.
  - **Horizon model RESOLVED (operator clarification, 2026-08-05):** the 12–36mo window
    is the **seam/transition between the guidebook's tactical (12–18mo) and strategic
    (3–5y) bands** — the leverage zone where strategic plans must begin appearing as
    observable intermediate events/checkpoints. The model is a **flow, not states**:
    tactical events are waypoints on the path to strategic goals; the analyst's job at
    the seam is to identify the key intermediate events evidencing movement toward (or
    away from) long-term plans. Consensus-efficiency out to 12–18mo and the 12–36mo
    edge are compatible: the edge is not predicting near-term numbers but *recognizing
    which events are checkpoints on the strategic path* (matches the guidebook edge
    quote). 5y+ is where terminal-multiple math lives, not the event edge — consistent
    with both sources.
- MAIA in-code fragments (consistent with the guidebook extraction above):
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


---

## Phase B — Design (OUGHT)

Every recommendation cites its Phase A dependency.

### (a) Transcript-analysis tool design

> **Generalized 2026-08-05:** this tool is now the `earnings` mode of the general
> **`company_transcript`** tool (modes: `earnings` / `corpus` / `combined`) — see
> `company-corpus-design.md` §B1. The fetch/segment mechanics below are the
> `earnings`-mode spec, unchanged. The corpus pipeline below is the shared §B3
> ingestion path from that doc.

One new tool on the **companies** server, plus reuse of corpus tools — no new server.

```
earnings_transcript(symbol, year?, quarter?, quarters_back=1, mode=fetch|segment)
  → { transcripts: [{symbol, year, quarter, date, content, source_endpoint}],
      segments?: [{section: prepared_remarks|qa, speaker?, text}],   // segment mode
      coverage: {requested_quarters, retrieved_quarters, missing: [..]} }
```

- Fetch: FMP `/stable/...` (or legacy v3) `earning_call_transcript` per (year, quarter);
  `quarters_back=20` iterates the last 20 quarters; `coverage` reports gaps honestly.
  *Depends on A2 (FMP-only source), A3 (iteration for 20 quarters), A4 (plan probe).*
- Segment mode: rule-based segmentation of `content` into `prepared_remarks` vs `qa` and
  per-speaker turns via the stable textual conventions in transcripts ("Operator:",
  "Question-and-Answer Session", "Executives:", "Analysts:" headers).
  *Depends on A2 (content is a single unstructured blob — segmentation must be built
  in-repo; corpus cannot do it, per A5).* Because A2 leaves speaker formatting
  UNVERIFIED, segmentation must ship with a fixture-driven test over one real FMP
  `content` blob, and must degrade to a single `full_text` segment when no conventions
  match — never fabricate speaker labels.
- Storage/RAG: transcripts flow through the **corpus server pipeline** (see §(b)′
  corpus anchoring below), not a bespoke store: `corpus_chunk` → `corpus_tag_chunks`
  (FIBO/PKO ontology anchors) → `corpus_embed` → `corpus_extract_triples` (h_mem
  knowledge graph) → centroid grouping → `corpus_query` RAG for the listening
  template. *Depends on A5 (all five tools verified present with the needed
  contracts: raw-text chunking with entity_ref_prefix, ontology tagging, triple
  extraction into the memory DB, RAG query).*
- Errors: per-quarter fetch failures are collected into `coverage.missing`, not
  propagated as whole-tool failure; the tool fails only when zero quarters succeed.
  Idiomatic-Rust note (verdict c includes a Rust tool, so this applies): request type
  `#[derive(JsonSchema)]` with `hkask_mcp_server::AnyJsonValue` for any free-form field;
  `anyhow::Result` with `?` propagation; FMP non-2xx mapped per-variant via a
  `map_fmp_error` fn (per the repo's per-variant error-classification rule), no
  `unwrap()`; fiscal-quarter arithmetic returns `Option` rather than panicking on
  quarter boundaries.

### (b) Listening template spec

A YAML/JSON schema versioned in-repo (a `SKILL.md` + `.j2`
templates under `.agents/skills/listening/`). Each factor is anchored to the guidebook extraction + in-code
fragments in A5 plus the operator's seam clarification — no invented factors.

**The horizon model — unified (operator clarification, 2026-08-05):**

> The key horizon is the **seam between tactical and strategic** — 12 to 36 months
> out. That is the leverage point where you should see things happening that move
> toward the strategic long-term plans: not mere short-term events, and not the
> long-term plans themselves (not yet accomplished). The analyst's job is to identify
> the **key intermediate events or checkpoints** on the way to the company achieving
> its strategic goals. The guidebook's bands (tactical 12–18mo / strategic 3–5y /
> long-term 7–10y) and the 12–36mo focus are one model read as a **flow**: the seam
> is where the strategic becomes observable.

This is implemented as a **global stance block** that every section's extraction and
verdict passes through. The template refuses to emit a verdict on a claim whose
horizon class is `short_term_only` with no strategic-path linkage — such claims are
recorded as `ignored_short_term` (kept for audit, excluded from verdicts). **The
linkage, not the calendar date, is the admissibility bar**: a 6-month event that is a
nameable checkpoint on the path to a stated 3-year goal IS primary material; a
14-month event with no strategic linkage is not. The central output is the
**checkpoint map** (see output schema).

```yaml
version: 3  # v1: fragments only; v2: operator horizon overlay; v3: guidebook found +
            # seam model unification (flow, not states) + guidebook-native sections
source_of_factors:
  - maia-guidebook: /home/mdz-axolotl/Clones/Library/MAIA-Substack (extracted 2026-08-05)
  - maia-in-code-fragments      # see A5 (margin CV, DPO−DSO, moat, CEO/CFO scorecards)
  - operator seam clarification (2026-08-05)  # 12–36mo = tactical→strategic transition zone
template_sections:               # guidebook company-template structure the listening feeds
  # BUSINESS / MANAGEMENT / VALUATION / RISKS / FUTURE (137738813.company-template)
  # FUTURE = "what we are watching for in the next 18 months" — the checkpoint map
  # (output below) is the transcript-derived feed into exactly this section.
process_stance:                  # from 137881063.company-analysis — transcript is primary source
  anchor: "how the company describes itself"   # filings/calls first, NOT sell-side — a transcript
                                               # IS the company's self-description: highest-authority input
stance:                          # GLOBAL FILTER — applies to every section below
  model: process_flow            # events are waypoints on a path, not independent states
  guidebook_bands: {tactical_months: [12, 18], strategic_years: [3, 5], long_term_years: [7, 10]}
  seam_window_months: [12, 36]   # the leverage zone — primary listening target
  consensus_efficient_months: [0, 18]   # guidebook: use consensus here, don't compete
  far_horizon_months: 60         # terminal-multiple math lives here, not events
  claim_horizon_classes:         # every extracted claim is classified before use
    - short_term_only            # no strategic-path linkage → ignored_short_term (any horizon)
    - seam_checkpoint            # event WITH named linkage to a strategic goal → PRIMARY
    - tactical_event             # 12–18mo dated expected event → guidebook FUTURE feed
    - strategic_context          # 3–10y plan/goal statement → the anchor checkpoints link TO
    - speculative_far            # >5y event claims → no granularity (guidebook); context only
  admissibility_rule: >
    A claim enters a section verdict only if it is a seam_checkpoint (strategic-goal
    linkage named), a tactical_event (dated, with feasibility/scaling basis), or a
    strategic_context claim that anchors checkpoint linkages. short_term_only claims
    are logged under ignored_short_term. The linkage, not the calendar date, is the
    bar: a near-term event that is a nameable checkpoint on the path to a stated
    strategic goal IS primary material.
  certainty_levels:              # guidebook verbatim (137682783)
    proximate: ">=67% — already started to happen, could stop"
    probable: "33–66% — all elements exist for it to happen"
    possible: "<32% — could happen but unlikely"
sections:
  - id: margin_trajectory          # MAIA: gross-margin STABILITY through a cycle, not expansion
                                   # (137464131: "Market power is not the ability to increase gross margins")
    listen_for:
      - management commentary on gross-margin direction, pricing, input costs
      - quantified margin guidance vs prior-quarter guidance
      - ACCOUNTING-GAMES CHECK (137464131 caveat): margin changes achieved via cost
        reclassification, capitalization shifts, one-time "adjustments"
    extract: { claims: [verbatim_quote + speaker + section], numbers: [margin figures] }
    maps_to_tool: key_metrics        # quantitative confirmation after the call
  - id: working_capital_power      # MAIA: DPO−DSO spread (analysis.rs:25-42)
    listen_for:
      - receivables/payables/inventory commentary, customer payment terms, supplier pressure
    extract: { claims: [...], signals: [customer_concentration, term_changes] }
    maps_to_tool: working_capital_cycle
  - id: moat_evidence              # guidebook vocabulary: "market power" / "competitive
                                   # advantage" / "edge" — "moat" does not appear in the corpus
    listen_for:
      - pricing power statements, churn/retention, competitive-response language in Q&A
      - "how it takes one dollar and turns it into two" — unit-economics explanations
      - evidence of ability to dictate price AND payment timing/terms (the two-sided test)
    extract: { claims: [...], analyst_challenges: [...] }   # Q&A pushback is the sensor
    maps_to_tool: moat_check
  - id: capital_allocation         # MAIA CEO rule (137464131): good = decreasing capital
                                   # w/ steady/improving returns OR increasing capital w/
                                   # improving returns; "the no-no is increasing capital
                                   # and decreasing returns"
    listen_for:
      - capex/M&A/buyback/dividend announcements AND their stated return expectations
      - capital being drained from underperformers / added to outperformers (the actual test)
    extract: { claims: [...], numbers: [capex, buyback, acquisition spend] }
    maps_to_tool: management_scorecard
  - id: expectations_gap_update    # THE core MAIA frame (137881063): "the hole is the gap
                                   # between the expectations of the market and the
                                   # potential of the business"; thesis arc: market expects
                                   # X, we expect Y, Y > X
    listen_for:
      - anything that changes what Y should be (our long-term growth/profitability
        expectation) — NOT what changes next quarter's consensus
      - management revealing plans/capabilities "not on the page yet" (137881063:
        the position-building signal is seeing what isn't on the page yet)
      - evidence the market misunderstands/neglects something confirmed or denied on the call
    extract: { y_updates: [{claim, direction: raises_y|lowers_y, evidence}],
               x_signals: [{consensus_assumption_challenged, evidence}] }
    maps_to_tool: expectations_gap
  - id: guidance_vs_expectations   # event-tree prior input (forecasting-and-scenarios.md)
    # SEAM MODEL APPLIED: a guidance change matters iff (a) it states or moves a
    # strategic (3–5y) goal, or (b) it is a checkpoint — a shorter-horizon change
    # whose position on the path to a stated strategic goal can be named (e.g.
    # capacity coming online that the 3y margin plan depends on). Guidebook
    # grounding: consensus is treated as fairly accurate for 12–18mo, so
    # consensus-shaped short-term guidance carries no edge by MAIA's own premise.
    # A bare next-quarter raise/cut with no nameable strategic-path linkage is
    # recorded as ignored_short_term and must NOT move any forecast_record prior.
    listen_for:
      - strategic goal statements and their movement (multi-year revenue/margin/
        capital-intensity targets) — the anchors
      - INTERMEDIATE EVENTS / CHECKPOINTS: dated milestones in the 12–36mo seam that
        the strategic plan depends on (capacity, product launches, regulatory
        approvals, market entry, cost-program completion) — the primary quarry
      - short-term guidance changes WITH a nameable checkpoint linkage — admissible
      - short-term guidance changes with NO nameable linkage — record and ignore
    extract:
      strategic_goals: [{statement, horizon_years, quantitative?, evidence}]
      checkpoints: [{event, deadline_or_window, strategic_goal_link, certainty:
                     proximate|probable|possible, basis: feasibility|scaling, evidence}]
      guidance_changes:
        [{statement, direction: raised|lowered|withdrawn|initiated,
          horizon_class: seam_checkpoint|tactical_event|strategic_context|short_term_only,
          strategic_linkage: verbatim_quote_or_null}]   # null + short_term_only ⇒ inadmissible
    maps_to_tool: expectations_gap, forecast_record
  - id: management_consistency     # MAIA CFO rule (137464131): "stable working capital
                                   # accounts in the face of shifting market environments
                                   # requires exceptional discipline and skill"; plus
                                   # 137670567: if you understand the business you can
                                   # predict management's reasoning — unexplained behavior
                                   # = you are still learning the business
    listen_for:
      - this-quarter statements vs prior-quarter transcript claims (needs quarters_back≥2)
      - CHECKPOINT DRIFT: previously stated checkpoints moved, dropped, or silently
        missed across quarters — the flow-model failure signal (a checkpoint that
        slips twice is evidence the strategic path itself is breaking, not the quarter)
      - actions whose rationale is opaque given the stated strategy (analytical gap signal)
    extract: { contradictions: [{prior_quote, current_quote}], tone_shift: enum,
               checkpoint_drift: [{checkpoint, first_stated, slipped_to, times_moved}],
               unexplained_actions: [{action, why_opaque}] }
    maps_to_tool: (cross-transcript diff — new, see slice 4)
output:
  per_section: { verdict: corroborates|neutral|contradicts, evidence: [quotes],
                 certainty: proximate|probable|possible }
  # ONE certainty vocabulary everywhere: the guidebook tier (verbatim, 137682783),
  # matching hkask_forecast.rs:158 certainty_tier (proximate ≥67% / probable 33–66% /
  # possible <33%). The earlier 1|2|3 scale is removed — two scales would drift.
  horizon_summary:               # seam-model top-level shape — THE central output is the
                                 # checkpoint map: events placed on the tactical→strategic path
    checkpoint_map: [{checkpoint, deadline_or_window, strategic_goal_link, certainty,
                      status: on_track|slipped|new|dropped, evidence}]
    # deadline_or_window is the single date field (mirrors the extraction schema);
    # a derived months-out value, if needed, is computed from it — never stored twice.
    strategic_goals: [{goal, horizon_years, moved_this_call: none|raised|lowered|new|withdrawn}]
    ignored_short_term: [{claim, reason: no_strategic_path_linkage}]
    speculative_far: [{claim, role: context_only}]
  # INVARIANT: no verdict or forecast input may be derived from ignored_short_term
  # entries. Golden-file test asserts every forecast_record-affecting claim is a
  # seam_checkpoint, tactical_event, or strategic_context with its linkage named.
```

Every extracted claim must carry a verbatim quote + location; the template output never
contains a verdict without evidence. *Depends on A5 (factor list = guidebook extraction
+ in-code fragments + operator seam clarification, all recorded in A5; the schema is
extension-tolerant if further guidebook posts surface new factors).*

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

1. **Plan-gating probe** (no code): **COMPLETE 2026-08-05.** With the real
   `HKASK_FMP_API_KEY`, called the current FMP transcript endpoint for AAPL
   2023Q1..2025Q4. Result: `/stable/earning-call-transcript` returns 200;
   legacy v3 is 403; speaker markers + Q&A section markers present; timestamps
   absent; `date` field unreliable (AAPL 2023Q1 → `date: "2012-03-19"`).
   Resolves A2/A3/A4 UNVERIFIED items.

2. **Fetch tool**: **COMPLETE.** `company_transcript(mode=earnings)` for one
   (year, quarter). Returns `content` + date for a known quarter;
   `coverage.missing` populated for a quarter with no call; per-variant error
   mapping (`classify_fmp_status`); no `unwrap()`. Temporal key is
   `(year, quarter)`, not `date`.

3. **Window fetch**: **COMPLETE.** `quarters_back=N` iteration with explicit
   coverage report. Per-quarter failures collected into `coverage.missing`,
   not propagated. Tested with 8-quarter windows + proptests.

4. **Segmentation**: **FOLDED INTO SLICE 6.** The corpus pipeline's
   `corpus_chunk` + `corpus_tag_chunks` subsumes segmentation — chunks are
   tagged with PKO `Step`/`Procedure` for sections and `dc:contributor` for
   speakers. No standalone segmentation module was built (the initial
   over-built module was deleted in the essentialist audit). The design's
   §A5/Deferred #3 confirms `corpus_chunk` is token-based only, so the tagging
   step is where section/speaker attribution happens.

5. **Listening skill**: **COMPLETE.** The `listening` skill applies the v3
   template using a **retrieve-cite-verify** process: the transcript is
   pre-split into numbered chunks, the model searches the chunks and cites
   what it found (chunk_id + quote + char_start), and a post-processing step
   (`verify_citation`) checks each cited substring is present in the
   referenced chunk. The no-fabrication invariant is process-embedded, not
   instruction-embedded. Golden-file tests assert the fixture contains the
   required material (short-term guidance change, strategic checkpoint) and
   the template enforces the retrieve-cite-verify design. The RAG variant
   (`apply-template-rag.j2`) extends this to cross-document citation over
   the company KG.

6. **Corpus integration**: **COMPLETE.** The `entity_ref_prefix` is carried on
   each `TranscriptRecord` (wired into the tool output, not a standalone
   function). The pipeline sequence is `corpus_chunk` → `corpus_tag_chunks`
   → `corpus_embed` → `corpus_extract_triples` → centroid grouping →
   `corpus_query` (see `company-corpus-design.md` §B3). Negative accept probed:
   `corpus_cache` writes via `std::fs::write` with no size limit — full-length
   FMP blobs (~51k chars) cache without truncation.

---

## Skill-role accounting (as assigned in the task)

- **pragmatic-semantics** — enforced the IS/OUGHT split; the UNVERIFIED markers in
  Phase A are its output. Fabricated claims flagged: none remain in this doc; the FMP
  "15+ years" marketing claim was explicitly demoted to UNVERIFIED.
- **pragmatic-cybernetics** — the loop: **sensor** = listening template applied to the
  transcript; **set-point** = MAIA key factors + the seam model — and the seam framing
  makes the loop *anticipatory* rather than reactive: the checkpoint map is a
  feedforward element (expected waypoints with deadlines) against which each call is
  measured, so deviation detection (checkpoint slipped/dropped) is automatic rather
  than re-derived each quarter; the stance block remains the variety attenuator that
  filters the consensus-covered window before it reaches the actuator; **actuator** = corpus MCP
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
  `forecast_record`/decision over 4 consecutive quarters (delete the section) — note
  the two windows differ deliberately: section-deletion is cheap to test (4 quarters),
  the seam premise is a calibration claim needing ≥8 to Brier-compare meaningfully;
  **seam-model-wrong evidence** = over ≥8 quarters, `ignored_short_term` claims that
  were excluded would have produced better-calibrated `forecast_record` priors than
  the admissible checkpoint/strategic claims (Brier comparison of the two populations
  — if the short-term population beats the seam population, the seam premise fails in
  this domain and the stance block must be revised, not silently loosened);
  **checkpoint-map-empty evidence** = if across 4 consecutive quarters for a covered
  company no seam_checkpoint can be extracted with a named strategic linkage, either
  the company does not manage via articulated plans (map legitimately sparse) or the
  extraction is under-sensitive — adjudicate with a human read of the same transcripts
  before concluding which; template-unnecessary evidence = refuter #2 in §(c);
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
  fix factor source → design. Three refinement passes ran: (1) factors anchored to
  in-code fragments when the guidebook appeared absent; (2) operator horizon overlay
  hard-coded as a stance block; (3) guidebook located and extracted, operator seam
  clarification unified the two horizon models into the flow/checkpoint model (v3).
  A fourth pass over the v3 diff changed no design element → stop.
- **idiomatic-rust** — applied to §(a) (verdict includes a Rust tool): type-driven
  request/response, per-variant error mapping, `?` propagation, no panics, `AnyJsonValue`
  for free-form JSON per repo schema rules.
- **lean-prover** — **not applicable**, as anticipated: no formal contract obligation
  arose; the design's guarantees (coverage honesty, evidence-cited verdicts) are test
  obligations, not proof obligations.

## Deferred to user verification (not assumed)

1. Live FMP probe for plan gating, current `/stable/` path, and real `content` shape
   (slice 1).
2. Whether additional MAIA guidebook posts beyond the extracted set contain further
   transcript-relevant factors (the 55-post corpus was searched; priority files fully
   extracted; remaining posts were grep-covered but not all read end-to-end).
3. Corpus breakdown capability: **verified negative** — `corpus_chunk` is token-based
   only; no speaker/section segmentation exists. The design builds segmentation in the
   companies tool rather than around the corpus server.

## Convergence statement

All four deliverables produced; every Phase B recommendation carries a `depends on`
clause; the §(c) verdict carries four stated refuting observations. Refuting evidence
for the whole design: slice-1 probe showing FMP transcripts unavailable on the current
plan (design collapses to template-only skill over pasted transcripts), or the external
MAIA guidebook revision surfacing materially different factors (template §(b) re-derived,
tool unchanged — the guidebook extraction of 2026-08-05 is the current anchor).
