---
title: "Research MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-07
version: "0.39.0"
status: "Active"
domain: "Inference"
mds_categories: [domain, composition, lifecycle]
---

# Research MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-research`
**Tools:** 26 — 5 web tools (`web_ping`, `web_search`, `web_find_similar`, `web_extract`, `web_browse`), 15 RSS tools (subscribe/unsubscribe/list/fetch/entries/mark-read/unread-count/search/export/import/discover/edit-tag and the synthetic-feed family), 2 evidence tools (`cite_sources`, `evaluate_evidence`), and 4 research-run/paper tools (`begin_research_run`, `get_research_run`, `annotate_research_run`, `resolve_paper`). (2026-09-03 consolidation: `web_recommend_provider` folded into `web_search` — set `intent` and the tool scores the configured providers, picks the top recommendation, and surfaces the ranking in `provider_recommendations`; `rss_fetch_synthetic` removed — `rss_fetch` already dispatches `synthetic://` streams.)
**Auto-start:** Yes (free providers work with no credentials)

The research server is the web-research surface: a provider pool
(Exa/Tavily/Brave/SerpAPI/Firecrawl plus free Semantic Scholar/arXiv/RawFetch)
with RRF fusion, content extraction, headless browsing, RSS feed management,
response caching, and rate limiting.

## Architecture

- **Credential path:** `ctx.credentials` → `build_provider_pool` — API-key
  providers register only when their key is present; free providers always
  register. A missing key surfaces as `permission_denied` naming the env var
  (`WebError::NoProviderConfigured`), never a silent empty result.
- **Inference path:** the server holds an `Arc<dyn InferencePort>` resolved
  via `hkask_inference::resolve_inference_port()` (a `LazyInferencePort` over
  the zed IPC bridge, `HKASK_INFERENCE_SOCKET`). Its single consumer is the
  deep-strategy rerank stage.
- **Result path:** provider responses reach the caller verbatim in the
  `{"content": ...}` envelope; body-read failures are errors, never empty
  successes.

## Extraction destination policy (SSRF) — verified 2026-09-07

Three layered gates govern every destination a fetch connects to:

1. **Tool layer** — `validate_tool_url_with_dns` (strict): scheme,
   embedded-credential, and literal/dns-resolved address checks before the
   request reaches the pool.
2. **Pool boundary** — `extract_with_fallback` / `browse_with_fallback`
   re-validate (`validate_provider_url`) so each provider in the fallback
   chain is behind the same gate.
3. **Raw-fetch transport** (`providers/raw_fetch.rs`) — the inner gate:
   a custom reqwest redirect policy re-runs the strict literal checks on
   **every redirect hop** and bounds the chain (10 hops, cycles refused); a
   validating DNS resolver
   (`hkask-mcp-server::server::validate_resolved_addresses`) rejects any
   connect-time resolution that lands on loopback/private/unspecified, so a
   hostname cannot re-bind between validation and connect (the DNS-rebinding
   TOCTOU is closed at this transport — other consumers of the shared
   validator keep the documented gap); and proxies are disabled
   (`.no_proxy()`), so the connected destination is always the validated one.

Redirect content is labeled with the final URL it actually came from.

**Address-class policy** (`hkask-mcp-server/src/security.rs`): loopback,
   RFC1918/link-local IPv4, ULA/link-local IPv6, IPv4-mapped IPv6, IPv4
   compatible with NAT64 (`64:ff9b::/96`) unmasking, and unspecified
   destinations (`0.0.0.0/8`, `::` — connecting to `0.0.0.0` routes to
   loopback on Linux) are all refused under the strict config.

**Permissive policy unchanged:** RSS subscribe/fetch/synthesize use the
   permissive config (user-curated feeds may live on local networks) — that
   policy is untouched, including for the new address classes.

**Operator-visible proxy behavior:** raw fetches deliberately ignore
   `HTTP(S)_PROXY`/`ALL_PROXY` (warned at client build); a proxied destination
   cannot be validated, so it is not silently used. Provider-API clients
   (Firecrawl/Tavily/Exa/Brave/SerpAPI) keep their own proxy support.

Verified regressions (`providers/raw_fetch.rs` inline tests +
`tests/tool_behavior.rs`): a redirect to a loopback literal, the 169.254.169.254
metadata address, or a `localhost` name never reaches the sentinel (request
counters prove zero); chains are bounded; cycles refused; the pre-fix behavior
(following the forbidden redirect and returning its content) was observed as
RED before the fix.

**Boundaries of this gate, stated explicitly:** third-party provider APIs fetch
target URLs server-side from their own network position — that fetch is not
this transport and cannot be gated here; and a permitted-redirect (public→public)
end-to-end fixture is not constructible on loopback under the strict gate —
the follow decision is pinned by the redirect-policy unit tests, and the
enforcement tests prove the policy runs on every hop.

**Discover path (T02b, verified 2026-09-07):** `rss_discover_feeds` validates
its user-supplied URL with the strict gate and fetches through its own
`discover_client` field — the same validated client construction as RawFetch
(`validated_fetch_client`: per-hop redirect gate, connect-time resolver,
no-proxy), wired through `ResearchServer::new`. Its reqwest cause chain is
rendered in full, so a rejected hop names its reason. The permissive
`rss_client` remains deliberately separate: `rss_subscribe`/`rss_fetch`/
`rss_synthesize` are user-curated by ratified policy and keep default redirect
following — do not pass that client to strict-policy fetches.

## Deep-search rerank — decision record

**Requirement (operator directive).** The deep strategy's rerank stage must be
a templated LLM call, not a heuristic. The original heuristic implementation
(signal boosts in `apply_rerank`) was a shortcut, not the design. The output
must also be trustworthy: general models commit category errors — well-formed
but semantically wrong judgments that structural validation cannot catch.

**Decision.** Native rerank protocol: ONE `InferencePort::rerank` call
 carrying all candidates as documents, routed through the inference IPC
 bridge to the provider's rerank endpoint (OpenRouter `/api/v1/rerank`).
The default model is a dedicated reranker — `OpenRouter/qwen/qwen3-reranker-8b`,
served via OpenRouter's native rerank endpoint — overridable via
`HKASK_RERANK_MODEL` or the `kask.models.rerank_model` setting. The zed side
of the bridge holds the OpenRouter key (keychain slot at the provider
`api_url`, `https://openrouter.ai/api/v1` — the ONE location) and calls the
provider directly; the MCP
server never sees the credential (same pattern as `GenerateBatch`).

**Why the native protocol.**

1. *No category-error surface* — the reranker's output is a per-document
   `relevance_score`, its own relevance judgment, not a parsed LLM
   generation. The model cannot emit prose, hallucinate a format, or
   misorder a list it must track. The trust problem that motivated this
   design is answered by the protocol, not by validation layered over
   generation.
2. *Consistency by construction* — every candidate is judged by the same
   model with the same internal rubric in the same request.
3. *One call replaces N* — the earlier per-candidate chat-completions
   fanout (and its concurrency cap) is obsolete; cost and latency scale with
   one request, not with `num_results`.
4. *Dedicated rerankers are trained for exactly this shape* — query-document
   relevance judgment (Zhang et al., arXiv:2506.05176). LLM reranking as a
   pattern is established by RankGPT (Sun et al., EMNLP 2023,
   arXiv:2304.09542), whose sliding-window workaround for list-length limits
   is precisely what a native documents-array rerank endpoint makes
   unnecessary. Positional degradation in long contexts (Liu et al., TACL
   2023, arXiv:2307.03172) motivated the earlier per-candidate design; the
   native endpoint inherits that robustness while restoring single-request
   economics.

**Degradation contract.** Every degraded outcome is surfaced in the tool
output's `rerank` field — never a silent fallback:

- The rerank call failed (or returned no valid scores) → `mode: "heuristic"`
  with the error as `reason`; the heuristic RRF order is kept.
- Some documents missing from the response → `mode: "llm"` with a `reason`
  naming the count; unscored candidates keep heuristic order after the
  scored ones.
- Non-deep strategies → no `rerank` field (they do not rerank).

**Canonical-pattern interactions.**

- *RRF fusion* (`providers/mod.rs`): heuristic signals remain the base
  scoring; the rerank stage reorders on top and falls back to RRF order on
  total failure.
- *Inference IPC bridge* (`InferenceMethod::Rerank`): the call routes to
  the zed side, which holds the OpenRouter key and calls the provider's
  rerank endpoint directly — the MCP server never sees the credential
  (same pattern as `GenerateBatch`).
- *Model constants* (`hkask-inference/model_constants.rs`):
  `DEFAULT_RERANK_MODEL` is the single source of truth. The settings chain
  (settings_content → `KaskModelsSettings` → `emit_models_env` →
  `HKASK_RERANK_MODEL` env → `rerank_model()` resolution) overrides it, and
  the research server's `config_env` allowlist passes it through under
  governed launch (pinned by `research_allowlist_matches_actual_reads`).

## Configuration

| Variable | Description |
| --- | --- |
| `HKASK_EXA_API_KEY` | Exa search API key |
| `HKASK_TAVILY_API_KEY` | Tavily search API key |
| `HKASK_BRAVE_API_KEY` | Brave search API key |
| `HKASK_SERPAPI_API_KEY` | SerpAPI key (YouTube transcript search) |
| `HKASK_FIRECRAWL_API_KEY` | Firecrawl extraction API key |
| `HKASK_RESEARCH_DB` | Research SQLite DB path (feed substrate + run ledger; defaults to `<data-dir>/mcp/research/research.db`) |
| `HKASK_DB_PASSPHRASE` | DB encryption passphrase (required for RSS and research-run tools) |
| `HKASK_WEB_CACHE_TTL_SECS` | Response cache TTL (default 300) |
| `HKASK_WEB_CACHE_MAX_ENTRIES` | Response cache max entries (default 50) |
| `HKASK_RERANK_MODEL` | Rerank model override (default `OpenRouter/qwen/qwen3-reranker-8b`) |
| `HKASK_EMBEDDING_MODEL` | Embedding model for the semantic duplication tier (emitted unconditionally by the settings chain; unset is a legitimate degraded mode — the deterministic shingle floor runs with a surfaced reason) |

## One-time data migration — `rss.db` → `research.db`

The default DB filename changed with the research-run ledger landing in
the same database. **Before first launch of the renamed build, move the
file**: `mv <data-dir>/mcp/research/rss.db <data-dir>/mcp/research/research.db`
(keep the same `HKASK_DB_PASSPHRASE`). The renamed build never reads the
old path — an unmoved file is not an error, but the feed substrate and run
ledger start empty. The passphrase-rotation layout and the maintenance
inventory cover the new path; the inventory will flag the old `rss.db` path
as stale until reconciled — **that flag is the system working, not a
failure** (Magna Carta P1: an existing encrypted DB is operator-owned and
is never silently orphaned). Operators who set `HKASK_RESEARCH_DB` to an
explicit path are unaffected (the env var continues to point wherever it
pointed; only the default filename changed).

## Evidence evaluation — the signal model (2026-09-09)

`evaluate_evidence` scores each artifact into per-component signals (base,
corroboration, recency, content) with basis strings, plus a set-level
report. All arithmetic is pure and deterministic (the G3 no-LLM-relay
contract); the weight model IS the const table (`DEFAULT_PROFILE` — no
weight literal lives in a code path).

**Syndication-aware corroboration.** Corroboration counts independent
evidence units, not raw domains: content-bearing artifacts are clustered
by 4-word shingle similarity (Jaccard ≥ 0.5, transitively closed), and a
cluster with a sourced member is one unit — one wire story on three
domains is one visible unit, not three corroborations. Content-less
sourced artifacts fall back to domain counting, and the signal basis says
so. The set report carries `content_clusters` so a syndicated story is
visible, not merely discounted.

**Sensitivity.** The set report carries the ordering's sensitivity under
named weight-profile substitution (`corroboration_heavy`, `recency_heavy`):
`stable` (identical ordering under every profile), `unstable` with the
driver component, or `not_evaluable` with a reason (fewer than 2
artifacts, or all-equal scores) — never a fabricated `stable`.

**Semantic duplication tier (parameter-gated).** `duplication: "semantic"`
opts into the embedding tier: content-bearing artifacts are clustered by
cosine ≥ 0.85 (the `corpus_deduplicate` threshold) via one batch
`InferencePort::embed` call through the inference bridge. Degradation
follows the rerank contract — never silent: no model configured, embed
failure, or a vector-count mismatch runs the deterministic shingle floor
with a surfaced `duplication_reason` naming the cause; the mode is
`shingles`/`semantic`.

## Research-run ledger (2026-09-09)

The server keeps a non-repudiable record of what its tools actually
returned under a research run — the zed-kask strengthening of Feynman's
agent-authored `ResearchRun` manifest, which can claim anything about its
own inputs. `begin_research_run` mints a run id
(blake3(question || began_at)[..16]); `web_search`, `web_extract`, and
`web_find_similar` accept an optional `run_id` and append their returned
sources with `recorded_by='server'` (first observation wins — a re-serve
never clobbers the audit copy; a run-scoped call bypasses the
response-cache read so the ledger records what THIS request returned).
Ledger write failures surface in the tool output as a `run_ledger` note,
never swallowed. `get_research_run` returns the manifest with per-source
confidence recomputed server-side from the ledger's own excerpt copies
and a `validation` block.

**The verified gate (fail-closed).** `annotate_research_run` records
agent-declared verification states (not_checked, inferred, partial,
verified, blocked, failed). `verified` is accepted ONLY for a URL the
server itself recorded under the run and ONLY with a basis — an
annotation about a source the server never served is `invalid_argument`
naming the rule. Annotations are upsert-idempotent on (run_id, url);
annotating an unseen URL records it as an agent-declared row
(`recorded_by='agent'`, excluded from verified eligibility).

**Composition seam.** `run_sources.corpus_ref` is the agent's pointer to
the durable recall copy ingested via the corpus server — different job,
different owner; `excerpt` is the server's own capped audit copy. No
server-to-server coupling.

## Paper identity (2026-09-09)

`resolve_paper` parses any identifier form — DOI (bare, `doi:`, doi.org
URL), arXiv ID, PMID, PMCID, OpenAlex work ID — into a typed identity
with the canonical URL and a stable kind-prefixed ledger key; every
rejection names what was expected. OpenAlex (a free provider, always
registered) enriches with title/authors/year/venue when a record exists;
the identity is the deterministic floor and is returned even when the
metadata lookup degrades (a note either way — never silent). arXiv IDs
have no direct OpenAlex lookup key and resolve to no-record. Pass
`run_id` to record the resolution into the run ledger.

## References

- Zhang, Y., et al. "Qwen3 Embedding: Advancing Text Embedding and Reranking
  Through Foundation Models." arXiv:2506.05176 (2025).
- Sun, W., et al. "Is ChatGPT Good at Search? Investigating Large Language
  Models as Re-Ranking Agents." EMNLP 2023, arXiv:2304.09542.
- Liu, N. F., et al. "Lost in the Middle: How Language Models Use Long
  Contexts." TACL 2023, arXiv:2307.03172.
