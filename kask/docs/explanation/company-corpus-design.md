---
title: Company Corpus — Design (discovery → ontology-anchored KG → RAG for MAIA analysis)
last_updated: 2026-08-06
status: implemented (slices 0-7 complete, see §B6)
depends_on:
  - kask/docs/explanation/earnings-transcript-analysis-design.md (Phase A provider findings, seam model, listening template)
  - kask/docs/explanation/ontology-anchored-embedding.md (tag→embed pipeline)
  - kask/docs/explanation/companies-mcp.md (provider routing, 42-tool surface)
verified_against:
  - kask/mcp-servers/hkask-mcp-corpus/src/tools/gather/mod.rs (corpus_discover pattern)
  - kask/mcp-servers/hkask-mcp-corpus/src/corpus/discover/search.rs:107-260 (YouTube transcript fetch via SerpAPI)
  - kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs:610-640 (ChunkRequest contract)
  - kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs:433-448 (TagChunksRequest contract)
  - kask/mcp-servers/hkask-mcp-corpus/src/tools/semantic/mod.rs:626-641 (ExtractTriplesRequest contract)
  - kask/mcp-servers/hkask-mcp-companies/src/tools/*.rs (42 #[tool] fns, verified 2026-08-05)
  - kask/crates/kask_bridge/src/settings.rs:451-458 (prediction_markets settings pattern for per-company config)
---

# Company Corpus — Design

Two epistemic modes are kept separate, as in the earnings-transcript design: **IS** =
verified against code or provider docs (marked UNVERIFIED where not); **OUGHT** =
normative design, each recommendation carrying a `depends on` clause.

## Phase A — What exists (IS)

### A1. The discovery pattern already exists (author-shaped)

`corpus_discover` (`tools/gather/mod.rs:81`) discovers an **academic author's** body of
work: multi-source search (Semantic Scholar, arXiv, web, **YouTube transcripts via
SerpAPI** — `include_transcripts` flag at L51, fetch at `corpus/discover/search.rs:242`),
content extraction, `corpus.yaml` generation for `corpus_build_persona`. Supports
`agentic` (fully automated) and `curated` (human-in-the-loop) modes.

**Key finding:** the *machinery* for company discovery exists; the *shape* is
author-specific (Semantic Scholar/arXiv sources, persona output). Generalizing to
companies is a **new tool + a source-tier manifest**, reusing the search/fetch/extract
plumbing. Verified: no company-discovery tool exists today (grep across corpus server).

### A2. The pipeline tools exist with the needed contracts (verified)

| Stage | Tool | Contract verified |
|---|---|---|
| Chunk | `corpus_chunk` | `ChunkRequest`: raw `text` OR `path` OR `input_dir`; `entity_ref_prefix`; multi-tier (document.rs:610-640) |
| Tag | `corpus_tag_chunks` | `TagChunksRequest`: `chunks_jsonl` → 5W1H + Dublin Core + PKO + FIBO/GOLEM (ops.rs:433-448) |
| Embed | `corpus_embed` | ontology-anchored vectors, INSTRUCTOR-style tag prepending (semantic/mod.rs:422) |
| Triples | `corpus_extract_triples` | `ExtractTriplesRequest`: `chunks_jsonl` + optional `tagged_jsonl` + `db_path` → RDF h_mems (semantic/mod.rs:626-641) |
| Centroids | persona pipeline | `EmbeddingStore` centroid retrieval; `centroid_distance` (persona/mod.rs:76-96) |
| RAG | `corpus_query` | in-memory vector index, top-k + optional LLM answer (storage.rs:73-100) |

### A3. Provider findings (updated with live probes 2026-08-05)

- **FMP**: `/stable/earning-call-transcript` verified live with the operator's key
  (HTTP 200; legacy v3 is 403). Speaker markers + Q&A section markers PRESENT in
  `content`; timestamps absent; history floor between 2005–2010 for AAPL. Plan gating
  RESOLVED (current plan works). See earnings-transcript-analysis-design.md A2–A4.
- **EODHD**: no transcript endpoint (verified absence); excluded.
- **SerpAPI**: `youtube_video_transcript` engine verified live. On a real corporate
  keynote (MSFT Build 2025, `ceV3RsG946s`): 1004 segments with `start_ms`/`end_ms`
  timestamps, `snippet` text, ~96k chars; **speaker labels are generic
  (`SPEAKER 1..7`) not named**; `available_transcripts` + `chapters` present.
  `youtube` search engine returns channel names (channel-allowlist filtering is
  feasible at discovery time). **Corporate transcript quality: verified usable** —
  timestamped, chunked, but speaker attribution is NOT named (contrast with FMP's
  named speakers); tier-2 speaker-level analysis must rely on FMP earnings calls,
  not YouTube.

### A4. Source-authority doctrine (guidebook, verified)

`137881063.company-analysis` (MAIA-Substack): "Company research should always start
with how the company sees themselves. Reading company filings and reports and listening
to presentations… the most useful anchor in analyzing a company is how it describes
itself. It is important not to begin with sell side or media reports." This is the
set-point for the source-tier manifest (§B2).

---

## Phase B — Design (OUGHT)

### B1. The generalized transcript tool: `company_transcript`

The earnings-transcript tool generalizes to a **company transcript tool** with three
modes. It stays on the **companies server** (it owns FMP credentials/routing; verdict
(c) of the earnings design is unchanged — fetch+segment is deterministic/provider-shaped).

```
company_transcript(symbol, mode=earnings|corpus|combined, …)
```

| Mode | What it does | Source |
|---|---|---|
| `earnings` | Current earnings-transcript behavior: fetch+segment earnings calls (FMP), coverage-honest | FMP `earning_call_transcript` |
| `corpus` | Fetch **non-earnings** company transcripts: investor-day presentations, executive keynotes/interviews (YouTube via SerpAPI, channel-allowlisted), any other transcript-shaped tier-1/2 source in the company manifest | SerpAPI YouTube + manifest-listed sources |
| `combined` | Both, merged into one transcript set for the pipeline | both |

- `earnings` mode is the existing design (fetch + rule-based prepared-remarks/Q&A +
  speaker segmentation, degrade-to-full-text, never fabricate speakers).
  *Depends on earnings-design A2 (FMP), A5 (corpus can't segment).*
- `corpus` mode is new: it does NOT segment (presentations/interviews lack the stable
  Operator/Q&A conventions of earnings calls); it fetches + normalizes to
  `{symbol, source_tier, kind, title, date, url, content}` and hands off to the
  pipeline. *Depends on A3 (SerpAPI verified; corporate-channel allowlist UNVERIFIED —
  see slice 1).*
- Output is pipeline-ready: a JSONL of transcript records keyed by
  `transcript:{symbol}:{kind}:{date}` entity refs, so §B3 ingestion is mode-agnostic.
  *Depends on A2 (corpus_chunk accepts raw text + entity_ref_prefix).*

**Essentialist check (why one tool, three modes, not three tools):** the modes share
fetch/coverage/normalize logic and differ only in source + whether segmentation
applies; three tools would triplicate the fetch/coverage/error-mapping surface. One
tool with a mode enum is the deeper module. A fourth mode (`sector`, cross-company)
is deliberately NOT included — no consumer exists yet; add when the scenarios server
needs it (no speculative generality, per `.rules`).

### B2. The company corpus tool: `corpus_discover_company`

A new corpus-server tool generalizing `corpus_discover` from author → company. It
discovers a company's document corpus from an **approved-source manifest** and emits a
`corpus.yaml` for ingestion.

```
corpus_discover_company(symbol, mode=agentic|curated, manifest_path?, max_docs?, tiers?)
  → { manifest: resolved_source_manifest,
      discovered: [{tier, kind, title, url, date, fetch_status}],
      corpus_yaml: path,
      coverage: {by_tier: {tier_1: n, tier_2: n, tier_3: n}, excluded: [...]} }
```

**The approved-source manifest** is the trust policy and the design's enforcement
point. Per-company, versioned (repo `kask/registry/company-sources/{symbol}.yaml` or
portfolio data dir), diff-able, auditable:

```yaml
company: { symbol: MSFT, name: Microsoft, cik: "0000789019",
           ir_base: "https://www.microsoft.com/en-us/Investor" }
source_tiers:
  tier_1_self_description:          # highest authority — the company on itself (MAIA anchor)
    - { kind: sec_filings, forms: [10-K, 10-Q, 8-K, DEF-14A], via: sec_edgar }
    - { kind: ir_documents, subpaths: [earnings, annual-reports, investor-day] }
    - { kind: earnings_transcript, via: companies_mcp }      # company_transcript earnings mode
  tier_2_executive_voice:           # executives speaking, unmediated
    - { kind: youtube, via: serpapi,
        queries: ["{ceo_name} keynote", "{ceo_name} interview", "{company} investor day"],
        channels_allowlist: ["Microsoft", "Microsoft Investor Relations"] }
    - { kind: company_transcript, via: companies_mcp }       # company_transcript corpus mode
  tier_3_external:                  # admissible context, lower authority — opt-in only
    - { kind: conference_talks, events_allowlist: [...] }
    # sell-side research and news media EXCLUDED by default (MAIA: do not begin there)
provenance_rule: >
  Every document carries source_tier + dc:source + retrieval date. tier_3 may inform
  context but can never be cited as the company's own position in a template verdict.
  Generated content (corpus_generate_qa output) is never tier-1/2 evidence.
exclusion_rule: >
  A tier_2 youtube result whose channel is not on channels_allowlist, or whose
  transcript fails the no-fabrication citation check, is excluded (logged in
  coverage.excluded), never silently downgraded to tier_3.
```

*Depends on A1 (discover pattern), A3 (SerpAPI), A4 (tier ordering = MAIA doctrine).*
The tier system is the mechanical enforcement of "start with how the company describes
itself" — tier-1 is processed first and weighted highest; sell-side/media is off by
default, which is the doctrine made non-optional.

### B3. The ingestion pipeline (mode-agnostic)

Discovery output (any tier, any transcript mode) flows through the **same**
ontology-anchored pipeline defined in the earnings design §(b)′ — no parallel silo:

```
corpus_discover_company / company_transcript
  → corpus_chunk(text, entity_ref_prefix="{company}:{kind}:{date}")   # per-document
  → corpus_tag_chunks        # 5W1H + Dublin Core + PKO + FIBO (FIBO = financial anchor)
  → corpus_embed             # ontology-anchored vectors
  → corpus_extract_triples   # h_mems in the memory DB — the company knowledge graph
  → centroid grouping        # per (company, theme) — reuses persona centroid machinery
  → corpus_query / KG traversal   # RAG surface
```

The company KG is therefore **cross-document**: an earnings-call checkpoint, the 10-K
risk factor it references, and the investor-day slide that announced it are linked by
shared entity refs and FIBO/PKO concepts. *Depends on A2 (all tools verified).*

### B4. What the company corpus enables (the payoff for MAIA analysis)

These are the concrete upgrades over transcript-only analysis; each names its
KG/RAG mechanism:

1. **Cross-source verdict citation** — the listening template's "verbatim quote"
   invariant extends from "the call" to "the company's tier-1/2 corpus": a margin
   claim on the Q4 call can be checked against the 10-K's stated pricing model.
   Mechanism: `corpus_query` across the whole company KG, tier-filtered.
2. **Checkpoint ↔ filing linkage** — a seam checkpoint (PKO Step) extracted from a call
   is linked to the strategic plan (investor-day deck) that defined it and the 10-K/10-Q
   that reports progress. Mechanism: shared entity refs + `corpus_extract_triples`
   cross-document triples. This is the checkpoint map gaining its evidentiary depth.
3. Executive-language drift — CEO/CFO prepared-remarks centroids across quarters
   AND across document kinds (call vs keynote vs annual letter); displacement =
   strategy/messaging shift signal for `management_consistency`. Mechanism: centroid
   grouping per (executive, period). *Depends on speaker segmentation — VERIFIED for
   FMP earnings calls (named speakers), NOT available for YouTube (generic SPEAKER n
   labels): executive-level centroids are earnings-call-only unless YouTube speakers
   are manually attributed in curated mode.*
4. **Self-description baseline** — the MAIA "how the company describes itself" anchor
   becomes a queryable artifact: the company's own tier-1 corpus is the baseline
   against which external claims (tier-3) and market expectations (the X in
   `expectations_gap`) are measured.

### B5. Falsifiability

- **Source-tier-wrong evidence**: over ≥4 companies, tier-1-anchored verdicts do NOT
  out-calibrate tier-3-informed ones (Brier on the downstream `forecast_record`
  population) → the tier ordering is doctrine, not evidence; revisit.
- **Tier-2-noise evidence**: >30% of allowlisted YouTube transcripts fail the
  citation/no-fabrication check → tier-2 via SerpAPI is not viable at corporate
  quality; replace with manual URL curation (curated mode only) and drop agentic
  tier-2 fetch. *(Probe note 2026-08-05: transcript text quality is high — timestamped,
  clean snippets — but speaker labels are generic, so the citation check must not
  require named speakers for tier-2; the failure mode to watch is wrong-video or
  auto-caption garbage, not speaker naming.)*
- **Cross-document-linkage-empty evidence**: if, across a covered company, no
  cross-document triples survive extraction (calls never link to filings/decks), the
  KG adds no value over per-document RAG → drop §B3 cross-document aspiration, keep
  per-document pipeline.
- **Corpus-mode-unnecessary evidence**: if non-earnings transcripts (investor days,
  keynotes) never change a verdict or checkpoint that earnings calls alone produce
  (4-quarter comparison), `corpus`/`combined` modes collapse back to `earnings`-only.

### B6. Verifiable vertical slices

Each is end-to-end testable; slice 0 gates the rest (unchanged from earnings design).

0. **FMP + SerpAPI probes** (no code): **COMPLETE 2026-08-05.** FMP:
   `/stable/earning-call-transcript` live (200), legacy v3 dead (403), speaker+Q&A
   markers present, timestamps absent, floor 2005–2010, plan OK. SerpAPI:
   `youtube_video_transcript` live, timestamped segments, generic `SPEAKER n` labels
   (not named), channel names available at search time for allowlisting. All A3
   UNVERIFIED items resolved.
1. **Source manifest**: hand-author one manifest (a covered company, e.g. MSFT);
   validate against a manifest schema; the tier/provenance/exclusion rules parse.
   Accept: schema rejects a manifest with tier_3 sell-side enabled-by-default.
2. **`company_transcript` earnings mode**: the existing earnings slices 2–5 (fetch,
   window, segmentation, template-skill golden tests) — unchanged.
3. **`company_transcript` corpus mode**: fetch + normalize one allowlisted investor-day
   transcript. Accept: pipeline-ready JSONL record with correct tier/kind/provenance;
   a non-allowlisted channel is excluded and logged, never silently kept.
4. **`corpus_discover_company`**: agentic discovery against the slice-1 manifest.
   Accept: tier-1 SEC filings discovered (Edgar), tier-2 allowlisted videos discovered,
   `coverage.excluded` populated for any non-allowlisted/low-quality hit; `corpus.yaml`
   generated.
5. **Pipeline ingestion**: one full document (10-K) + one earnings transcript through
   §B3. Accept: FIBO/PKO tags present; h_mems reference the entity prefix; a
   `corpus_query` returns the right document; centroids computed per theme.
6. **Cross-document linkage (the payoff slice)**: a checkpoint from an earnings call
   linked to its investor-day source. Accept: a KG traversal from checkpoint →
   strategic-goal → source document returns all three; listening-template golden test
   re-run against the full company KG reproduces verdicts with cross-source citations.
7. **Curated mode**: human-in-the-loop discovery. Accept: agentic proposals presented,
   human accept/reject recorded, rejected sources excluded from `corpus.yaml`.

### B7. What is deliberately NOT built (essentialist)

- **No new server** — discovery lives on corpus, transcript fetch on companies; both
  own their credentials already.
- **No sector/cross-company mode** — no consumer yet.
- **No sell-side/news ingestion** — excluded by MAIA doctrine (A4), not merely unbuilt.
- **No real-time/streaming discovery** — batch discovery + re-run is sufficient;
  streaming is a later question if coverage staleness proves to matter.
- **`corpus_generate_qa` on company docs** — deferred per the earnings design's
  augmentation policy (training data only, separate namespace, never evidence).

---

## Skill-role accounting (perspectives applied to this design)

- **pragmatic-semantics** — IS/OUGHT split enforced; SerpAPI corporate quality and FMP
  plan gating marked UNVERIFIED, not assumed. The tier ordering is labeled OUGHT
  (doctrine-derived), with its falsifier in §B5.
- **pragmatic-cybernetics** — the company corpus is the sensor's **long-term memory**:
  centroids + KG give the loop a compressed per-company state, so deviation detection
  (checkpoint drift, tone shift, guidance movement) is comparison against state, not
  re-reading. The tier manifest is a **variety attenuator** at the sense boundary —
  it filters the high-noise external channel before it reaches the KG.
- **essentialist** — deletion tests in §B7; the load-bearing one: the source manifest
  survives deletion test (inline source lists would scatter trust policy across
  prompts; the manifest is the auditable enforcement point).
- **falsifiability** — §B5; each architectural bet (tiers, tier-2 viability, cross-doc
  linkage, corpus mode) carries a measurable refuter.
- **grill-me** — self-challenge on "one tool three modes": *edge* — earnings
  segmentation is mode-specific; does one tool hide that? Resolved: mode enum makes the
  difference explicit (`corpus` mode never segments), and shared fetch/coverage is the
  deep part. *Synthesis* — the split holds: deterministic fetch in the tool, judgment
  in the skill, trust policy in the manifest.
- **idiomatic-rust** — applies to `company_transcript` + `corpus_discover_company`
  (both Rust MCP tools): type-driven mode enum, per-variant error mapping
  (`map_fmp_error` / `map_serp_error`), `?` propagation, no `unwrap()`, `AnyJsonValue`
  for free-form fields, coverage collected not panicked.
- **sequential-inquiry** — converged: author-discovery generalization → manifest as
  trust policy → pipeline reuse → payoff enumeration; a further pass changed no
  element → stop.
- **lean-prover** — not applicable (test obligations, not proof obligations).

## Convergence statement

Design complete pending slice-0 probes. Every Phase B recommendation carries a
`depends on` clause; every architectural bet carries a falsifier (§B5). Refuting
evidence for the whole design: SerpAPI cannot deliver corporate transcripts at
citation quality (tier-2 collapses to curated-only), or cross-document KG linkage
proves empty in practice (§B3 aspiration downgrades to per-document RAG).
