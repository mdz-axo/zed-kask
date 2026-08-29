---
title: "Research MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-29
version: "0.39.0"
status: "Active"
domain: "Inference"
mds_categories: [domain, composition, lifecycle]
---

# Research MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-research`
**Tools:** 22 — `web_ping`, `web_search`, `web_recommend_provider`, `web_find_similar`, `web_extract`, `web_browse`, plus 16 RSS tools (subscribe/unsubscribe/list/fetch/entries/mark-read/unread-count/search/export/import/discover/edit-tag and the synthetic-feed family)
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

## Deep-search rerank — decision record

**Requirement (operator directive).** The deep strategy's rerank stage must be
a templated LLM call, not a heuristic. The original heuristic implementation
(signal boosts in `apply_rerank`) was a shortcut, not the design. The output
must also be trustworthy: general models commit category errors — well-formed
but semantically wrong judgments that structural validation cannot catch.

**Decision.** Per-candidate (pointwise) scoring: one LLM call per
(query, candidate) pair, each returning a strict-JSON relevance score 0-100;
candidates sort by descending score. Calls fan out concurrently, bounded by
`HKASK_RERANK_MAX_CONCURRENCY` (default 8). The default model is a dedicated
reranker — `DeepInfra/Qwen/Qwen3-Reranker-8B` — overridable via
`HKASK_RERANK_MODEL` or the `kask.models.rerank_model` setting.

**Why per-candidate, not list-ordering.**

1. *Consistency by construction* — every candidate gets the identical prompt
   and rubric, so judgments are comparable. A list-ordering prompt makes each
   item's judgment depend on its neighbors.
2. *Bounded blast radius* — a wrong score misorders one item and cannot
   cascade. The model's output space is a per-item number, never a permutation
   it must track across a long list.
3. *Positional robustness* — long-list ordering is where LLMs degrade:
   performance collapses for items in the middle of the input context (Liu et
   al., TACL 2023, arXiv:2307.03172). Per-candidate prompts have no middle.
4. *Dedicated rerankers are trained for this shape* — query-document relevance
   judgment (Zhang et al., arXiv:2506.05176). LLM reranking as a pattern is
   established by RankGPT (Sun et al., EMNLP 2023, arXiv:2304.09542), which
   introduced sliding windows to work around list-length limits —
   per-candidate scoring is the same insight taken to its limit.

**Degradation contract.** Every degraded outcome is surfaced in the tool
output's `rerank` field — never a silent fallback:

- All calls failed → `mode: "heuristic"` with the first error as `reason`;
  the heuristic RRF order is kept.
- Some calls failed → `mode: "llm"` with a `reason` naming the failure count;
  unscored candidates keep heuristic order after the scored ones.
- Non-deep strategies → no `rerank` field (they do not rerank).

**Canonical-pattern interactions.**

- *RRF fusion* (`providers/mod.rs`): heuristic signals remain the base
  scoring; the rerank stage reorders on top and falls back to RRF order on
  total failure.
- *LazyInferencePort* (`hkask-inference`): scoring routes through the zed IPC
  bridge per call; a socket appearing after server start is picked up without
  a restart.
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
| `HKASK_RSS_DB` | RSS SQLite DB path (defaults to `<data-dir>/mcp/research/rss.db`) |
| `HKASK_DB_PASSPHRASE` | DB encryption passphrase (required for RSS tools) |
| `HKASK_WEB_CACHE_TTL_SECS` | Response cache TTL (default 300) |
| `HKASK_WEB_CACHE_MAX_ENTRIES` | Response cache max entries (default 50) |
| `HKASK_RERANK_MODEL` | Rerank model override (default `DeepInfra/Qwen/Qwen3-Reranker-8B`) |
| `HKASK_RERANK_MAX_CONCURRENCY` | Rerank fanout cap (default 8; malformed values warn and use the default) |

## References

- Zhang, Y., et al. "Qwen3 Embedding: Advancing Text Embedding and Reranking
  Through Foundation Models." arXiv:2506.05176 (2025).
- Sun, W., et al. "Is ChatGPT Good at Search? Investigating Large Language
  Models as Re-Ranking Agents." EMNLP 2023, arXiv:2304.09542.
- Liu, N. F., et al. "Lost in the Middle: How Language Models Use Long
  Contexts." TACL 2023, arXiv:2307.03172.
