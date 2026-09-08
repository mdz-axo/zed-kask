---
title: "Research Server Capability Adoption Plan (Feynman transfers)"
audience: [developers, architects, agents]
last_updated: 2026-09-07
version: "0.39.0"
status: "Proposed (OUGHT) — none of this is implemented; every IS claim below is grep-verified as of 2026-09-07"
domain: "Inference"
mds_categories: [domain, composition, lifecycle]
---

# Research Server Capability Adoption Plan

Adopts the high- and medium-value capabilities identified in the Feynman
(`advaitpaliwal/feynman`, v0.3.49) architecture analysis into
`kask/mcp-servers/hkask-mcp-research`, in zed-kask idiomatic form. Produced
through the `refactor-architecture` process (explore → candidates → deepen →
audit → strangle → verify), with module designs gated by `essentialist`
(3-gate Exist/Surface/Contract) and `grill-me` (5-level interrogation),
classified by `pragmatic-semantics`, and analyzed by `pragmatic-cybernetics`.
Deterministic invariants are specified as both Rust unit tests (the
enforcement points) and `lisp_eval` expressions (the implementation-time
convergence gates the implementing agent runs).

**Compatibility posture (operator directive, 2026-09-07): no
backward-compatibility requirement.** Every consumer of the surfaces this
plan touches is in-tree — the server is a leaf crate, and the settings
chain, allowlists, tests, and docs all live in this repository. Tool
output shapes are redesigned freely; env vars, settings keys, constants,
and macros are renamed in the same commit that changes their meaning;
tests are rewritten against the new model rather than pinned to old
assertions. The one exclusion is user data: an existing database file is
operator-owned (Magna Carta P1) — the default-filename rename carries an
explicit one-time migration note instead of a compat shim.

## IS baseline (grep-verified 2026-09-07)

| Fact | Location |
|---|---|
| Evidence scoring is pure, deterministic, unit-tested — "not LLM relay — G3 contract" | `hkask_mcp_research.rs:1453-1455`, `:1814` |
| Weights are scattered literals: base 0.3, corroboration ≤0.3, recency 0.2/0.1, content 0.2, capped at 1.0 | `hkask_mcp_research.rs:1840-1845` |
| Corroboration counts DISTINCT source domains; same-domain duplicates do not inflate | `hkask_mcp_research.rs:1825-1840`, tests `:1909-1925` |
| `evaluate_evidence` emits per-artifact confidence + set signals + SEPIO keys; no per-component trail, no stability measure | `hkask_mcp_research.rs:1457-1511` |
| `has_published_date` is `published.is_some()` even when the date is unparseable (age `None`, recency 0) | `hkask_mcp_research.rs:1850`, `:1866-1887` |
| Provider pool: free providers (SemanticScholar, arXiv, RawFetch) always register; key providers register on credential presence; missing key = `permission_denied` naming the env var | `research.rs:50-101`, `docs/reference/mcp-servers/research.md:24-27` |
| One SQLCipher DB (`HKASK_RSS_DB`, default `{data}/mcp/research/rss.db`), opened via canonical 2-tier passphrase chain; DB-unavailable is warned, never silent | `hkask_mcp_research.rs:1659-1740` |
| Settings chain for the DB path: `KaskResearchSettings.rss_db` → `emit_research_env` → `HKASK_RSS_DB` env → server read; allowlisted in `config_env`, pinned by `research_allowlist_matches_actual_reads`; managed-DB layout entry `("research_rss", "HKASK_RSS_DB", mcp_server_db("research", "rss"))` | `kask_bridge/src/settings.rs:316-320` + `:872-874`, `mcp_env.rs:121-124`, `mcp_servers.rs:278-292` + `:1275-1278`, `identity.rs:332-336` |
| Schema DDL constant is `RSS_SCHEMA_DDL` (feeds/subscriptions/entries/entry_states/synthetic_feeds); consumed TWICE — server open + rotation test string-split on the const name | `research/db.rs:26-116`; `hkask_mcp_research.rs:1707`; `hkask-storage/src/rotation.rs:473-480` |
| Degradation contract: every degraded rerank outcome surfaces in `rerank` field; never silent fallback | `docs/reference/mcp-servers/research.md:78-86` |
| Tool surface: 22 tools; `#[tool]` macro + `execute_tool` wrapper pattern | `docs/reference/mcp-servers/research.md:14`, `hkask_mcp_research.rs:1518-1521` |

## What transfers from Feynman (and what does not)

| Feynman element (source) | Disposition |
|---|---|
| `ScoreComponentKey` + `DEFAULT_SCORE_WEIGHTS` as a typed data table (`src/rank/paper-rank.ts:20-35`) | **Adopt** — weights become a const table + exhaustive enum |
| Per-component `ScoreSignal` with rationale (`paper-rank.ts:480-505`) | **Adopt** — per-artifact signal trail in `evaluate_evidence` output |
| `RankSensitivity` ordering stability (`paper-rank.ts:531-561`) | **Adopt** — via weight-profile substitution (see C1) |
| `ScoreCalibrationPreference` profiles (`paper-rank.ts:562-571`) | **Defer** (essentialist cut, see Decisions) |
| `ResearchRun` manifest + `validateResearchRun` (`src/research/contracts.ts`) | **Adopt, strengthened** — server-side ledger, not agent-authored claims (C2) |
| ID normalization chain: `extractArxivId`/`extractPmid`/`extractPmcid`/`normalizeDoi`/`canonicalDoiUrl` (`paper-rank.ts:1450-1590`) | **Adopt** — typed `PaperId` module (C3) |
| OpenAlex Works API provider (`paper-rank.ts`, `science-database-openalex.ts`) | **Adopt** — free provider in the pool (C3) |
| Full-text access resolution + section extraction | **Defer** (essentialist cut) |
| Citation-graph expansion (`--expand-citations`) | **Defer** (essentialist cut) |
| Prompt-protocol workflows, Pi runtime wrapper, workbench, Bio Tools catalog | **Reject** — wrong stack; zed-kask skill infrastructure already covers the mechanism |
| `/watch` standing-watch workflow | **Already covered — build nothing.** The synthetic-feed substrate binds any source URL to an extractor (`css`/`json_path`/`diff_hash`) with cadence hints and content-hash change detection (`db.rs:90-107`, `rss_synthesize` `hkask_mcp_research.rs:969-991`) |
| Lit-review source discovery | **Already covered** — `corpus_gather_author` runs multi-source discovery (Semantic Scholar/arXiv/web, `corpus/src/tools/gather.rs:79`) |
| Session search | **Already covered** — curator memory + corpus RAG |

## Existing-infrastructure audit — the feed substrate vs Feynman's files

Feynman's storage substrate is files-on-disk: markdown artifacts, provenance
sidecars, a lab notebook, and one org-scoped SQLite workbench mirror. Its
`/watch` is prompt+scheduling with no durable observation state, and its
session search indexes sessions, not sources. The research server's
substrate is a standing-observation system (`research/db.rs:21-109`):

| Substrate capability | Location | Architectural implication |
|---|---|---|
| Conditional-GET incremental sync (`etag`, `last_modified`) | `feeds` table; `rss_fetch` (`hkask_mcp_research.rs:706`) | Sources are watched, not one-shot queried |
| FTS5 content index with insert/delete triggers | `entries_fts`, `db.rs:71-88` | Accumulated evidence is searchable in-DB |
| Read/starred work queue | `entry_states`, `db.rs:63-69` | Observation lifecycle is a queue |
| Synthetic feeds: arbitrary source URL + extractor + cadence + content-hash change detection | `db.rs:90-107`, `rss_synthesize` `hkask_mcp_research.rs:969-991` | A generic standing-watch pipeline already exists |
| OPML import/export | `db.rs:541-685` | Source-set portability (Magna Carta P1) |

Consequences for this plan: (1) watch-shaped capabilities are composition,
not construction; (2) the run ledger matches the substrate's grain — an
append-only observation stream in the same encrypted DB, as feeds
accumulate entries; (3) the substrate proves content accumulates here, so
domain-counting corroboration is a known-weak signal against syndication —
C1 adopts the corpus pipeline's dedup precedents (below).

## Corpus composition (agent-composed, no server-to-server coupling)

The corpus MCP owns the content-memory jobs: chunking (`tools/document.rs:280`),
ontology-anchored embedding via the inference router (`tools/semantic.rs:181`),
5W1H/Dublin Core/PKO tagging (`tools/tagging/ops.rs:221`), RAG query with
Lisp queries and db hydration (`tools/storage.rs:75`), semantic dedup at
cosine 0.85 (`tools/corpus.rs:43`), and author/company source discovery
(`tools/gather.rs:79`, `:231`). Critically, `embed` is a method on the same
`InferencePort` the research server already holds for rerank
(`hkask-types/src/ports/inference_port.rs:296`) — the embedding path needs
no new dependency, only wiring.

The idiomatic composition is agent-composed (each server owns its DB; the
agent routes between them — the same reference-bridging pattern as
`training_assemble_dataset`):

- Research records what the server observed (`run_sources.excerpt`, C2).
- The agent, when durable RAG over a run's evidence is wanted, routes the
  content through `corpus_convert`/`corpus_chunk`/`corpus_embed` and records
  the resulting entity_ref back via `run_sources.corpus_ref`.
- No server-to-server call; no schema coupling; `corpus_ref` is the
  reference seam. `excerpt` is the server's non-repudiable observation
  (audit); `corpus_ref` is the agent's pointer to the durable recall copy
  (composition). Different jobs, different owners.

## Candidates (ranked)

### C1 — Evidence module with data-driven scoring (Strong)

**Files:** `hkask_mcp_research.rs` (scoring section out), new
`src/research/evidence.rs`, `src/research/types.rs` (output types
redesigned), `tests/tool_behavior.rs` (rewritten + extended).

**Problem.** The scoring model is sound (distinct-domain corroboration, real
recency, no saturation — tests `hkask_mcp_research.rs:1908-1980`) but the
weights are magic literals in one function, the output carries component
VALUES without contributions, and there is no way to answer "is this
ordering robust?" A caller cannot distinguish confidence 0.8 built on
corroboration from 0.8 built on recency — and `has_published_date: true`
with `published_age_days: null` (unparseable date) is currently ambiguous.

**Solution.** Extract scoring into a deep `evidence` module: a component
enum + const weight table (data-driven — the model IS the table), one pure
evaluator emitting per-component signals with basis strings, and a
sensitivity report computed by substituting named weight profiles.

### C2 — Research-run ledger, server-validated (Strong)

**Files:** `research/db.rs` (DDL + run tables + fns), new
`src/research/runs.rs`, `hkask_mcp_research.rs` (tools + rename sweep),
`research.rs` (export wiring); rename blast radius outside the crate:
`kask_bridge/src/{settings.rs, mcp_env.rs, identity.rs, mcp_servers.rs}`,
`hkask-storage/src/rotation.rs` (test string-split), doc rows.

**Problem.** Nothing records what a research loop actually consulted.
Verification claims ("I checked 5 sources") live only in agent prose. The
Magna Carta audit surface has no enforcement point for evidence claims
(`reg.sovereignty.*` namespaces are registered but unemitted — Magna Carta
`kask/docs/architecture/core/magna-carta.md:96`).

**Solution.** Port Feynman's `ResearchRun` contract but invert the author:
the SERVER records what its tools actually returned (non-repudiable), and
validates agent-declared verification states against that record. An agent
may annotate a source `verified` only if the server observed that source in
the run. Feynman's agent-authored manifest can claim anything; this design
cannot be lied to about the server's own output.

### C3 — Paper identity + OpenAlex provider (Worth exploring)

**Files:** new `src/research/paper_id.rs`, new
`src/research/providers/openalex.rs`, `research.rs` (pool registration),
`hkask_mcp_research.rs` (`resolve_paper` tool).

**Problem.** Paper references arrive as bare strings across tool boundaries.
The pool's scholarly providers (arXiv, SemanticScholar) have no DOI/PMID/
PMCID/OpenAlex resolution, so paper-shaped evidence loses identity before it
reaches `evaluate_evidence` or `cite_sources`.

**Solution.** A typed `PaperId` enum with one parse function (invalid states
unrepresentable), plus OpenAlex as an always-registered free provider, plus
a `resolve_paper` tool that canonicalizes any identifier form.

## Deepened module designs

### C1: `research/evidence.rs` — 7 public items

```rust
pub(crate) enum EvidenceComponent { Base, Corroboration, Recency, Content }

/// The scoring model IS this table. No weight literal lives in a code path.
/// Invariant (unit-test-enforced): weights sum to 1.0; all ≥ 0.
pub(crate) const DEFAULT_PROFILE: [(EvidenceComponent, f64); 4] = [
    (EvidenceComponent::Base, 0.3),
    (EvidenceComponent::Corroboration, 0.3),
    (EvidenceComponent::Recency, 0.2),
    (EvidenceComponent::Content, 0.2),
];

pub(crate) struct ComponentSignal { component, earned, basis }  // basis = why-string
pub(crate) struct ArtifactScore  { confidence, signals, corroboration_count,
                                   published_age_days }
pub(crate) struct SetSensitivity { status: SensitivityStatus }
pub(crate) enum SensitivityStatus {
    Stable { profiles_evaluated: usize },
    Unstable { driver: EvidenceComponent },
    NotEvaluable { reason: String },   // <2 artifacts or all-equal scores
}
pub(crate) struct EvidenceReport { artifacts, distinct_domains, sourced_count,
                                   sensitivity }
pub(crate) fn score_evidence_set(artifacts: &[EvaluateArtifact],
                                  profile: &[(EvidenceComponent, f64); 4])
    -> EvidenceReport;
```

**Determinism.** All arithmetic is pure — the G3 no-LLM-relay contract
(`hkask_mcp_research.rs:1453`) is inherited, not weakened. Sensitivity is
profile substitution, not stochastic perturbation: the named profiles are
`default`, `corroboration_heavy` (corroboration 0.4, recency 0.1),
`recency_heavy` (recency 0.4, corroboration 0.1) — each renormalized to sum
1.0. Ordering of artifacts by confidence is compared across profiles;
`Stable` iff identical. `NotEvaluable` is SURFACED, never silently reported
as stable (degradation contract, `research.md:78-86`).

**Syndication-aware corroboration (the substrate's lesson).**
Distinct-domain counting cannot see syndication: one press release on five
domains scores as five corroborations — a weakness Feynman shares (its
corroboration analog is citation-based, same blind spot). The
`Corroboration` computation becomes duplicate-aware in two tiers:

- **Tier 1 (deterministic, always on when content is present):** 4-word
  shingle Jaccard ≥ 0.5 clusters content-bearing artifacts into content
  clusters; corroboration counts clusters, not domains. The 4-word-sequence
  precedent is the corpus QA-grounding check; the threshold is a pinned
  const under unit test. Content-less artifacts fall back to domain-counting,
  and the signal basis SAYS so ("no content — duplication not checkable").
- **Tier 2 (embedding-assisted, optional, Commit 6):** cosine ≥ 0.85
  (`corpus_deduplicate`'s threshold) via the existing `InferencePort::embed`
  (`inference_port.rs:296`) — the same port already wired for rerank. Model
  resolution: `hkask_inference::model_constants::embedding_model()` (reads
  `HKASK_EMBEDDING_MODEL`, returns `Option<String>` — unset is a legitimate
  DEGRADED mode, not a hard failure, because the deterministic floor
  exists). Wiring follows the corpus emission precedent
  (`emit_corpus_embedding_env` emits unconditionally — consuming servers
  have no fallback for unset env, `mcp_env.rs:183-194`): extend the
  emission to the research server's env, add the `config_env` allowlist
  entry, extend `research_allowlist_matches_actual_reads`.
  Degradation follows the rerank decision record verbatim: embed
  unavailable → shingles-only, surfaced as `duplication_mode: "shingles"`
  with the reason, never a silent fallback.

The set report gains `content_clusters: [{domains, artifact_urls}]` so a
syndicated story is VISIBLE, not merely discounted — the evidence trail
is the point.

**Tool output (redesigned — no compat constraint).** The signal model
SUBSUMES the flat booleans: `has_published_date` and `has_content` are
DELETED from the output; the `Recency` and `Content` signals carry the
information with basis strings ("published field present but unparseable
as RFC 3339 or YYYY-MM-DD" vs "no published field"). The output becomes
`{question, average_confidence, artifacts: [{url, title, confidence,
signals, corroboration_count, published_age_days}], set:
{distinct_domains, sourced_count, content_clusters, sensitivity,
duplication_mode}}`. The ontology keys
(`SEPIO:0000167`, `pko:StepVerification`) are retained — they are the
fixture-guarded ontology labeling contract, not a compatibility shim
(result keys route through the bridge constants, never string
literals).

**Interface budget.** Public items: `EvidenceComponent`,
`DEFAULT_PROFILE`, `ComponentSignal`, `ArtifactScore`, `SensitivityStatus`,
`EvidenceReport`, `score_evidence_set`, plus the syndication threshold
consts (`SHINGLE_JACCARD_THRESHOLD`, embedding tier reuses the corpus
cited threshold) — nine items against the ≤7 target. Essentialist G2
justification for the excess: the two threshold consts conflate the
scoring model with the clustering model if merged — different invariants,
different tests; collapsing the two enums would reintroduce stringly-typed
states (Hoare P1). Reduction available on operator request.

### C2: `research/runs.rs` + run tables — 3 new tools, 1 validator

Schema (extends the same SQLCipher DB — no second DB, no second passphrase
path, no second pool; essentialist G3):

```sql
CREATE TABLE IF NOT EXISTS research_runs (
  run_id TEXT PRIMARY KEY,           -- blake3(question || began_at)[..16]
  question TEXT NOT NULL,
  status TEXT NOT NULL,              -- planned|running|completed|partial|blocked|failed
  began_at TEXT NOT NULL,            -- RFC 3339
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS run_sources (
  run_id TEXT NOT NULL REFERENCES research_runs(run_id),
  url TEXT NOT NULL,
  provider TEXT,                     -- server-observed origin
  title TEXT, published TEXT, source TEXT,
  excerpt TEXT,                     -- what the server actually returned (capped) — the audit copy
  corpus_ref TEXT,                   -- optional entity_ref the agent ingested this content under (corpus composition seam)
  recorded_by TEXT NOT NULL,         -- 'server' (tool-observed) | 'agent' (declared)
  verification_state TEXT,           -- not_checked|inferred|partial|verified|blocked|failed
  verification_basis TEXT,          -- required when state='verified'
  recorded_at TEXT NOT NULL,
  PRIMARY KEY (run_id, url)
);

CREATE INDEX IF NOT EXISTS idx_run_sources_url ON run_sources(url);
-- cross-run audit queries ("was this URL/domain ever consulted?") need
-- only the index now; a lookup tool waits for a named consumer.
```

`RSS_SCHEMA_DDL` is renamed `RESEARCH_SCHEMA_DDL`. It has TWO consumers
and both are updated in the same commit: the server open at
`hkask_mcp_research.rs:1707` and the `hkask-storage` rotation test that
string-splits on the const's name (`rotation.rs:473-480`) — a stale name
there fails the rotation test loudly, which is exactly the gate wanted.
With no compat requirement, the interface renames sweep the full chain:
`HKASK_RSS_DB` → `HKASK_RESEARCH_DB`; settings field
`KaskResearchSettings.rss_db` → `research_db` (+ `Content` struct +
`From` impl, `settings.rs:316-320`/`:872-874`, JSON schema regenerated);
`emit_research_env` (`mcp_env.rs:121-124`); the `managed_database_layout`
entry `("research_rss", ...)` → `("research", "HKASK_RESEARCH_DB",
mcp_server_db("research", "research"))` (`identity.rs:332-336`); the
`config_env` allowlist entry + the `research_allowlist_matches_actual_reads`
pin (`mcp_servers.rs:278-292`, `:1275-1278`); the `require_rss_db` macro
→ `require_research_db` with the message updated
(`hkask_mcp_research.rs:129-140`); the server field `rss_db` →
`research_db`; the composition test fixture (`mcp_servers.rs:1444-1448`);
and the doc rows (`kask-settings.md:554-558`,
`mcp-servers/research.md:106-116`, `kask_bridge/reference.md:342-352`,
the server README). The default DB filename `mcp/research/rss.db` →
`mcp/research/research.db` (see Decisions for the one-time data note).

Tools (all follow the `#[tool]` + `execute_tool` pattern; all
DB-gated paths use the renamed `require_research_db` macro —
`hkask_mcp_research.rs:129-140`, message updated to name
`HKASK_RESEARCH_DB` and `HKASK_DB_PASSPHRASE`, never a silent no-op):

1. `begin_research_run(question) -> {run_id, status: "planned"}`.
2. `annotate_research_run(run_id, url, verification_state, basis?)` —
   agent-declared verification. **Validation gate:** `verified` is accepted
   only when the `(run_id, url)` row exists with `recorded_by='server'`
   (i.e. a research tool actually returned it under this run). Otherwise
   `invalid_argument` — the server refuses verification claims about
   sources it never served. This is the zed-kask strengthening of
   Feynman's `validateResearchRun` (which cannot check its own inputs).
3. `get_research_run(run_id) -> manifest` — question, status, sources with
   per-source verification states, `evaluate_evidence`-scored confidence
   recomputed server-side, and a `validation` block.

Server-side recording (the non-repudiation path): `web_search`,
`web_extract`, `web_find_similar`, and `evaluate_evidence` gain an OPTIONAL
`run_id` parameter. When present, returned results are appended to
`run_sources` with `recorded_by='server'` inside the same request
(best-effort append; a ledger write failure is returned in the tool's
output as a `run_ledger` note, never swallowed — `.rules` failure-signal
discipline).

Pure validator (Feynman's `validateResearchRun` ported, adapted):

```rust
pub(crate) fn validate_research_run(run: &ResearchRunRecord)
    -> Result<(), Vec<String>>;   // every rule names its violated invariant
```

Invariants: non-empty question; status ∈ enum; `completed`/`partial`
requires ≥1 server-recorded source; every `verified` annotation carries a
basis AND a server-recorded row; verification_state ∈ enum (Feynman's
`verificationStateValues`, `contracts.ts:12`).

### C3: `research/paper_id.rs` + `providers/openalex.rs` — 4 public items + 1 tool

```rust
pub(crate) enum PaperId { Doi(String), Arxiv(String), Pmid(String),
                           Pmcid(String), OpenAlex(String) }
pub(crate) fn parse_paper_id(input: &str) -> Result<PaperId, WebError>;
impl PaperId { pub(crate) fn canonical_url(&self) -> String }
pub(crate) fn stable_paper_key(id: &PaperId) -> String;  // ledger-usable key
```

Parse rules ported from Feynman (`paper-rank.ts:1450-1590`): DOI
case-normalized with `10.` prefix check; arXiv `NNNN.NNNNN(vN)`; PMID
digits-only with `pmid:`/URL-prefix stripping; PMCID `PMC\d+`; OpenAlex
`W`-prefixed short form. Every rejection returns a `WebError` variant
naming what was expected — no `Option`-silence.

`OpenAlexProvider` implements the existing `WebSearchProvider` trait
(`research/providers.rs:68`), maps OpenAlex works to `SearchResultOutput`,
and registers in `build_provider_pool` unconditionally (free provider —
`research.rs:63-65` pattern). `resolve_paper(query)` parses any identifier
form, calls OpenAlex, and returns canonical IDs + canonical URL + metadata;
accepts optional `run_id` to record into the ledger (C2 composition).

## Duplication audit (ra-audit)

| Operation | Sites | Classification | Action |
|---|---|---|---|
| Evidence scoring | `score_evidence_set` (single site) | Identical (one site) | Extract, do not duplicate |
| Citation field extraction | `cite_sources` `hkask_mcp_research.rs:1533-1607` | Surface-only (formatting, no DOI normalization) | Leave untouched in this plan; `PaperId::canonical_url` may absorb DOI-canonicalization later |
| Provider scoring | `score_providers` (trait, `providers.rs`) | Divergent (provider selection ≠ evidence quality) | No extraction — distinct domains |
| DB passphrase + gating | `require_rss_db` + `resolve_db_passphrase` (macro renamed `require_research_db` in Commit 2) | Identical (canonical pattern) | Reuse for run tools; do not add a second gate |
| Content dedup | corpus QA-grounding (4-word sequences) + `corpus_deduplicate` cosine 0.85 (`corpus.rs:43`) | Identical pattern, not code | Adopt the precedents and thresholds; do NOT duplicate the corpus embed index in the research DB — embeddings are computed per-call, never stored here |

## Reliability-plan alignment and sequencing (added 2026-09-07)

The tree carries an authorized, in-flight reliability implementation
(`tasks/plan.md`, baseline `2475305420`): T01 (scenario recovery) + D01
(portfolio/companies retention guard) are verified but UNCOMMITTED, and
extraction-transport hardening is in flight in this very crate
(`raw_fetch.rs` +414: per-hop redirect re-validation, validating DNS
resolver, surfaced proxy bypass; `tests/tool_behavior.rs` +92). Its
execution protocol and this repo's rules both state: one editing process
at a time. **This plan's implementation is GATED until the reliability
work lands (or is explicitly queued behind).** No parallel start.

**Write-set overlap map (why the gate is required):**

| Surface | Reliability plan | This plan |
|---|---|---|
| `raw_fetch.rs` + `web_extract` body | T02 transport hardening (in flight) | C2 `run_id` recording in the same tool body |
| `tests/tool_behavior.rs` (research) | +92 lines in flight | Commit 1 rewrites the evidence tests in it |
| `managed_database_layout` (`identity.rs:332-336`) | rotation + inventory derive from it | Commit 2 renames the `research_rss` entry |

**Alignment findings — no conflicts; three reinforcements:**

1. **The rename sweep is single-site and propagates.**
   `managed_database_layout()` is now canonical: `kask_db_paths()`
   (passphrase rotation) and the maintenance inventory
   (`database_maintenance.rs:47` iterates the layout;
   `hkask-storage/maintenance_inventory.rs:41` records DBs at open time)
   both DERIVE from it. Commit 2's layout-entry rename therefore
   propagates automatically to rotation and inventory; the sweep
   VERIFIES the propagated surfaces (rotation list, inventory previews,
   stale-checks) rather than hand-editing them.
2. **The migration note gains a catalog consequence.** An existing
deployment's on-disk database catalog holds the old `rss.db` path;
   after the rename, the inventory's stale-path discipline
   (`late_paths_invalidate_confirmation_and_receipts`, maintenance
   inventory tests) surfaces the old path until reconciled. The
   one-time migration note (Decision 5) must say: move the file AND
   expect the inventory to flag the stale entry — that flag is the
   system working, not a failure.
3. **C2 inherits T01's ratified persistence bar.** "Public response
   surfaces persistence failure; failure-then-retry remains recoverable"
   (`tasks/plan.md` T01, verified 2026-09-07) is the precedent the ledger
   meets by design: append-only, upsert-idempotent annotations
   (PRIMARY KEY `(run_id, url)` — re-annotating after a crash is safe),
   surfaced write failures, no silent ledger skips.

Sequenced order once ungated: reliability Phase A lands → Commit 1 →
Commit 2 → … as written below. This plan is reliability work in the
reliability plan's own sense: failures must not silently skip accounting,
and claims must not silently lack state.

## Migration (strangler — one domain per commit, TDD)

**Gate: Commit 1 does not start until the in-flight reliability work is
landed (or the operator explicitly re-sequences).**

1. **Commit 1 (C1 module):** failing tests in `evidence.rs` (signals,
   profile sums, sensitivity fixture, `NotEvaluable` surface, shingle-cluster
   syndication fixtures) → move scoring
   out of `hkask_mcp_research.rs` → wire `evaluate_evidence` to the
   redesigned output. The five existing scoring tests are REWRITTEN
   against the signal model — the behavioral properties carry over
   (distinct-domain corroboration, no single-artifact saturation,
   stale-below-fresh, base-only floor, full-corroboration term), but
   assertions target the new shape. Default weights are retained by
   design choice, not compatibility.
2. **Commit 2 (C2 schema + rename sweep + begin/get):** failing
   schema/roundtrip tests → `RESEARCH_SCHEMA_DDL` rename (both consumers,
   including the rotation-test string-split) + the interface-rename sweep
   (`HKASK_RESEARCH_DB`, settings field, allowlist + pin test, emit fn,
   layout entry, macro, server field, composition test, default filename,
   doc rows) + run tables → `begin_research_run` + `get_research_run` →
   `run_id` param on `web_search`/`web_extract` with server-side append.
3. **Commit 3 (C2 validation gate):** failing validation tests (verified
   without server record → `invalid_argument`; basis required) →
   `annotate_research_run` + `validate_research_run` → `get_research_run`
   emits the `validation` block.
4. **Commit 4 (C3 identity):** failing `parse_paper_id` table tests →
   module + `stable_paper_key` → `resolve_paper` tool.
5. **Commit 5 (C3 provider):** OpenAlex provider tests (fixture HTTP) →
   pool registration → `resolve_paper` records via `run_id`.
6. **Commit 6 (C1 embedding tier):** failing degradation tests (embed
   unavailable → `duplication_mode: "shingles"` surfaced with reason) →
   `InferencePort::embed` path + `HKASK_EMBEDDING_MODEL` wiring for the
   research server (`model_constants::embedding_model()` resolution;
   unconditional emission per the `emit_corpus_embedding_env` precedent →
   config_env allowlist + `research_allowlist_matches_actual_reads` pin) →
   cosine-tier clustering behind the same signal.
7. **Docs commit:** update `docs/reference/mcp-servers/research.md`
   (tool count, new env/passphrase notes, DDL name) per doc-update
   discipline — diffed against this plan, not regenerated from code.

Every commit: `cargo test -p hkask-mcp-research`, `./script/clippy`
(kask-scoped), and behavior tests in `tests/tool_behavior.rs` extended with
the capability under test present (constructed WITH the DB for run tools —
the capability-stripped constructor may only pin degradation).

## Verification gates (ra-verify)

- **Dependency direction:** `evidence.rs` and `paper_id.rs` depend only on
  `types.rs`; `runs.rs` depends on `db.rs` + `evidence.rs`; tools depend on
  modules; no cycles; no module reaches into `hkask_mcp_research.rs`.
- **Depth test:** deleting `evidence.rs` re-scatters five weight literals
  and the sensitivity logic into the tool file (complexity reappears);
  deleting `runs.rs` removes the only enforcement point for verification
  claims (behavior lost). Both KEEP.
- **P6/P7/P8:** no stubs; all fallible ops return `Result`/`WebError`; no
  `unwrap()`, no `let _ =` on fallible ledger writes.
- **Degradation surfaced:** DB-missing → error naming the requirement;
  sensitivity-not-evaluable → `NotEvaluable` with reason; ledger append
  failure → surfaced note. No path returns an empty-success on failure.
- **Rename sweep completeness:** zero remaining hits for
  `HKASK_RSS_DB`, `RSS_SCHEMA_DDL`, `require_rss_db`, or `.rss_db` across
  `kask/` code, tests, allowlists, and comments (the kata-kanban comment
  at `hkask_mcp_kata_kanban.rs:1759-1769` included; git history is not
  rewritten).

## Essentialist gate results (advisory — operator ratifies)

| Candidate | G1 Exist | G2 Surface | G3 Contract | Verdict |
|---|---|---|---|---|
| C1 evidence module | PASS — extraction reverses god-file drift (1,981-line `hkask_mcp_research.rs`) | PASS WITH JUSTIFICATION — nine public items vs the ≤7 target; excess justified in C1's interface budget (scoring-model vs clustering-model invariants cannot merge) | PASS — const table, no single-impl trait, status enum not bool | Proceed |
| C2 run ledger | PASS — audit loop stays broken without it | PASS — 3 tools + 1 validator | PASS — reuses DB/pool/passphrase/gate | Proceed |
| C3 paper identity | PASS — parse logic would reappear per-tool | PASS — 4 items | PASS — parse returns typed errors | Proceed |
| Calibration profiles | — | — | — | **DEFER** (see Decisions) |
| Citation-graph expansion | — | — | — | **DEFER** |
| Full-text resolution | — | — | — | **DEFER** |

## Grill-me interrogation results (5-level, gaps named)

- **Recall (solid):** baseline weights, corroboration semantics, and date
  parsing are verified above; no plan claim rests on unverified IS.
- **Mechanism (solid after one fix):** continuous weight perturbation was
  replaced by named-profile substitution — renormalization choice pinned
  (each profile sums to 1.0 by construction, unit-test-enforced).
- **Rationale (solid):** server-authored ledger over Feynman's
  agent-authored manifest, because the enforcement point must observe the
  act it certifies (Magna Carta: advertised invariants must point at the
  enforcement line).
- **Edge cases (two gaps found and designed in):** (1) `has_published_date
  : true` with unparseable date — resolved by DELETING the flag; the
  `Recency` signal basis states the parse outcome, so no dual-meaning field
  survives the redesign; (2) single-artifact sensitivity would fabricate
  stability — `NotEvaluable` surfaced instead. Remaining gap: concurrent `run_id` use
  across parallel subagents is serialized by the r2d2 pool but
  `updated_at` ordering is last-writer — acceptable, documented.
- **Synthesis (solid):** C3 resolves and records; C2 accumulates and
  validates; C1 scores and reports robustness — one evidence pipeline, not
  three features.

## Pragmatic-semantics classification

- Every design item above is **OUGHT** (proposed). The IS baseline is the
  only declarative section. 
- Constraint forces: no-LLM-relay scoring = **Prohibition** (G3 contract);
  surface-degradation = **Guardrail**; ≤7 public items = **Guideline**;
  "sensitivity improves agent trust in evidence orderings" = **Hypothesis**
  (measurable post-adoption, not assumed).
- Provenance: IS claims = Implementation (direct read); Feynman claims =
  External (cloned source v0.3.49); transfer-value ratings = Assessment.

## Pragmatic-cybernetics analysis

- **Evidence-quality loop today:** sense (score) → decide (agent) → act
  (conclusions) → return path **absent**. The ledger + annotation is the
  return path (operator audit; future calibration input). Without C2 the
  loop's closure property is broken — this is the cybernetic case for C2
  over "the agent could write its own notes".
- **Standing-watch loops are already closed and healthy:** subscriptions
  → conditional-GET sync (`rss_fetch`) → FTS accumulation → read-state
  drain is a closed loop the substrate provides; this plan builds nothing
  watch-shaped because the loop already exists (variety, closure, and
  fidelity are all substrate-provided).
- **Variety (Ashby):** disturbance class = agent claims about evidence
  ("verified", "N sources") PLUS syndication (same content, many
  domains). Regulator variety today = zero for claims and zero for
  syndication. C2 amplifies claim-checking variety (server-observed
  record); C1's content clusters add the requisite variety for
  duplicated-content disturbances that domain counting cannot absorb.
  Agent-claimed external sources remain attenuated (recorded
  `recorded_by='agent'`, excluded from `verified` eligibility).
- **Weight-adjustment loop:** remains **open by decision** (calibration
  deferred). The deferral is recorded here as a conscious choice with a
  named re-entry trigger, not an oversight — a future agent reading this
  plan must not "recover" calibration as forgotten scope.

## Deterministic gates (Rust enforcement + lisp_eval convergence checks)

Rust unit tests are the enforcement points. During implementation, the
agent additionally runs these `lisp_eval` expressions as PDCA convergence
checks (deterministic, sandboxed):

```lisp
;; G1 — profile sums are 1.0 (default, corroboration_heavy, recency_heavy)
(and (= 1.0 (+ 0.3 0.3 0.2 0.2))
     (= 1.0 (+ 0.3 0.4 0.1 0.2))
     (= 1.0 (+ 0.3 0.1 0.4 0.2)))

;; G2 — sensitivity fixture: A (3 domains, no date) vs B (1 domain, fresh
;; date, content). Default order A > B; recency_heavy must NOT flip it
;; (0.3+0.3+0+0.2=0.8 vs 0.3+0.1+0.2+0.2=0.8 → tie, ordering preserved).
;; The Rust test pins the same fixture; both must agree before commit.
(equal (list 'stable '(0.8 0.8))
       (list 'stable (list (+ 0.3 0.3 0.2) ... )))  ; expanded at impl time

;; G3 — run validation: a verified annotation with recorded_by=server
;; passes; without the row it must fail. Field-presence check over the
;; fixture manifest:
(let ((row (list :url "https://a.example" :recorded_by "server")))
  (and (assoc :url row) (string= "server" (getf row :recorded_by))))
```

G2 is deliberately written as an agreement check between the lisp fixture
and the Rust test — the same data expressed twice, cross-verified, so an
arithmetic drift in either is caught at implementation time, not in
production.

## Decisions required from the operator

1. **Calibration profiles (Feynman `ScoreCalibrationPreference`): DEFERRED.**
   Requires preference storage and a resolved-outcome consumer; overlaps the
   scenarios server's Brier machinery conceptually. Re-entry trigger: when
   `evaluate_evidence` scores become inputs to a resolved-outcome loop
   (e.g., forecast-resolution feedback), revisit as a cross-server design.
   Ratify or override.
2. **Citation-graph expansion: DEFERRED.** Requires extending
   `SearchResultOutput` with citation metadata; re-enter when ranking
   papers (not web evidence) becomes a research job.
3. **Full-text access resolution: DEFERRED.** Feynman's surface is large and
   licensing-sensitive; re-enter on explicit operator demand.
4. **`verified` gate strictness (Guardrail):** annotations about sources the
   server never served are rejected as `invalid_argument`. Alternative:
   accept but downgrade to `inferred`. The plan takes the strict reading
   (fail-closed, Magna Carta Principle 2); override requires a reason.
5. **Default DB filename rename (data note):** the no-compat rename
   includes the default path `mcp/research/rss.db` →
   `mcp/research/research.db` (the `identity.rs` layout entry follows).
   User data is operator-owned (Magna Carta P1): an existing encrypted DB
   is never orphaned silently — the rename ships with a one-time migration
   note (move the file before first launch of the renamed build; the
   passphrase-rotation layout covers the new path). No compat shim; the
   note is the mitigation.
6. **Embedding-assisted syndication tier (Commit 6):** costs one `embed`
   call per content-bearing artifact per `evaluate_evidence` invocation,
   through the same inference bridge as rerank. Tier 1 (deterministic
   shingles) ships regardless and is the floor. Ratify tier 2, defer it,
   or gate it behind an explicit request parameter (the rerank pattern
   applies: degradation surfaced either way).
7. **Sequencing authorization (required before implementation):** the
   reliability implementation is in flight and uncommitted; its
   protocol and this repo's rules allow one editing process. Choose:
   (a) land the reliability work first (authorizing its commits), then
   implement this plan; (b) queue this plan to the receiving coding agent
   after the reliability first tranche; (c) explicit operator
   instruction to interleave in one process. Implementation of this
   plan does not start before that choice.

## References

- Feynman source analysis (this session, clone of v0.3.49):
  `src/rank/paper-rank.ts` (scoring model), `src/research/contracts.ts`
  (ResearchRun v1), `extensions/research-tools/science-databases.ts`.
- Eigenfactor metrics (West & Bergstrom) and time-aware PageRank — cited by
  Feynman's `PAPER_RANK_SOURCES` as the bibliometric grounding for
  component-weighted, velocity-separated scoring; this plan adopts the
  component-signal structure, not the citation-graph components.
- Ousterhout, *A Philosophy of Software Design* — deep-module shape and
  the god-file extraction rationale (Feynman's own
  `scripts/check-architecture.mjs` allowlist names the same disease in its
  6,756-line `paper-rank.ts`).