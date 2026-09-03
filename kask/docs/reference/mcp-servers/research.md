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
**Tools:** 22 — 5 web tools (`web_ping`, `web_search`, `web_find_similar`, `web_extract`, `web_browse`), 15 RSS tools (subscribe/unsubscribe/list/fetch/entries/mark-read/unread-count/search/export/import/discover/edit-tag and the synthetic-feed family), and 2 evidence tools (`cite_sources`, `evaluate_evidence`). (2026-09-03 consolidation: `web_recommend_provider` folded into `web_search` — set `intent` and the tool scores the configured providers, picks the top recommendation, and surfaces the ranking in `provider_recommendations`; `rss_fetch_synthetic` removed — `rss_fetch` already dispatches `synthetic://` streams.)
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
| `HKASK_RSS_DB` | RSS SQLite DB path (defaults to `<data-dir>/mcp/research/rss.db`) |
| `HKASK_DB_PASSPHRASE` | DB encryption passphrase (required for RSS tools) |
| `HKASK_WEB_CACHE_TTL_SECS` | Response cache TTL (default 300) |
| `HKASK_WEB_CACHE_MAX_ENTRIES` | Response cache max entries (default 50) |
| `HKASK_RERANK_MODEL` | Rerank model override (default `OpenRouter/qwen/qwen3-reranker-8b`) |


## References

- Zhang, Y., et al. "Qwen3 Embedding: Advancing Text Embedding and Reranking
  Through Foundation Models." arXiv:2506.05176 (2025).
- Sun, W., et al. "Is ChatGPT Good at Search? Investigating Large Language
  Models as Re-Ranking Agents." EMNLP 2023, arXiv:2304.09542.
- Liu, N. F., et al. "Lost in the Middle: How Language Models Use Long
  Contexts." TACL 2023, arXiv:2307.03172.
