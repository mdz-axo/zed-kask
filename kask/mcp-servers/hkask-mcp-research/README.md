# hkask-mcp-research

Web search, extraction, evidence scoring, paper identity, and the research-run ledger.

## Tools (12)

| Tool                     | Description                                                                                                                                              |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `web_ping`               | Liveness and provider health check                                                                                                                       |
| `web_search`             | Search the web with RRF fusion. Set `provider` for a single-provider call (no fusion), or `intent` to have the tool pick the top-scored provider. Strategy: quick (best-scored single), web (all), news, deep (all, twice the results, content from the top three, rerank) |
| `web_find_similar`       | Find pages similar to a given URL using Exa findSimilar                                                                                                  |
| `web_extract`            | Extract content from a URL into markdown or structured JSON                                                                                              |
| `web_browse`             | Browse a URL via Firecrawl (JS-heavy), Tavily `/extract`, or Exa `/contents` — fallback across all configured browse providers                           |
| `begin_research_run`     | Begin a research run: opens a server-side ledger recording what the research tools actually return under the run                                          |
| `get_research_run`       | Get a run's manifest: sources, verification states, server-side recomputed confidence, and the validation block                                          |
| `finish_research_run`    | Mark a run finished (completed, partial, blocked, failed); completed and partial require at least one server-recorded source                             |
| `annotate_research_run`  | Annotate a run's source with an agent-declared verification state; `verified` requires a server-recorded source and a basis (fail-closed)                |
| `resolve_paper`          | Resolve a paper reference (DOI, arXiv, PMID, PMCID, OpenAlex, or a title via Crossref) to a typed identity with canonical URL and OpenAlex metadata      |
| `evaluate_evidence`      | Score evidence against a question: per-component signals with basis strings, syndication-aware corroboration, sensitivity, duplication mode              |
| `cite_sources`           | Generate citations from retrieved sources (apa, bibtex, chicago, json)                                                                                   |

The 15 `rss_` feed tools were removed on 2026-11-12 by operator decision. What that means for an existing database is in `kask/docs/reference/mcp-servers/research.md`, section "Feed tools removed".

`web_extract` accepts `markdown` or `json`. RawFetch, the keyless extractor, cannot produce structured JSON: a `json` request needs Firecrawl. For a PDF it returns the text layer; a scanned PDF with no text layer is a typed error naming the corpus server's OCR route. Responses (including cache hits) must match the requested format.

The `web_search.strategy` input schema advertises `quick`, `web` (`semantic` alias), `news`, and `deep` (`research` alias). `evaluate_evidence.duplication` advertises `semantic` or omission. An unsupported value is a typed invalid-argument error naming the accepted values.

## Configuration

| Variable                         | Description                                       |
| -------------------------------- | ------------------------------------------------- |
| `HKASK_EXA_API_KEY`              | Exa search API key                                |
| `HKASK_TAVILY_API_KEY`           | Tavily search API key                             |
| `HKASK_BRAVE_API_KEY`            | Brave search API key                              |
| `HKASK_SERPAPI_API_KEY`          | SerpAPI key (Google search, YouTube transcripts, explicit-only `google_scholar` and `google_books`) |
| `HKASK_FIRECRAWL_API_KEY`        | Firecrawl extraction API key                      |
| `HKASK_SEMANTIC_SCHOLAR_API_KEY` | Optional. Semantic Scholar runs keyless on the shared free tier; a key lifts its rate limit |
| `HKASK_RESEARCH_DB`              | Research SQLite DB path (optional — defaults to `<data-dir>/mcp/research/research.db`) |
| `HKASK_DB_PASSPHRASE`            | DB encryption passphrase (required for the research-run tools) |
| `HKASK_WEB_CACHE_TTL_SECS`       | Response cache TTL (default: 300)                 |
| `HKASK_WEB_CACHE_MAX_ENTRIES`    | Response cache max entries (default: 50)          |
| `HKASK_RERANK_MODEL`             | Model for the deep strategy's rerank stage: one rerank call carrying every candidate, through the inference bridge. Unset: the heuristic order is kept and the response says why |
| `HKASK_PRIVATE_RERANK_MODEL`     | Local rerank model for a private-provenance deep search. Unset: the rerank stage is skipped for private calls and the response says why |
| `HKASK_EMBEDDING_MODEL`          | Embedding model for the semantic duplication tier (unset = the deterministic shingle floor runs with a surfaced reason) |
| `HKASK_PRIVATE_EMBEDDING_MODEL`  | Local embedding model for a private-provenance semantic duplication tier. Unset: the shingle floor runs for private calls with a surfaced reason |

Free providers (Semantic Scholar, arXiv, OpenAlex for search; Crossref for
title resolution; RawFetch for extraction) are always registered — no API key
is required for scholarly search, paper resolution or plain-page extraction.
The research-run tools require `HKASK_DB_PASSPHRASE`; the DB path defaults to
`<data-dir>/mcp/research/research.db` (databases are infrastructure and stay
in the internal data dir — artifact files and outputs go to the visible
artifacts dir under `{server}-mcp/`).

## Quick Start

```bash
# No API keys required for scholarly search (free providers)
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-research
```

## Usage

```
"Search for latest AI research papers"   → web_search
"Extract this page as markdown"          → web_extract
"Which paper is this DOI?"               → resolve_paper
"Record what this search returned"       → begin_research_run, then run_id on web_search
```
