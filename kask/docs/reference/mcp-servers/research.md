---
title: "Research MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-11-12
version: "0.40.2"
status: "Active"
domain: "Inference"
mds_categories: [domain, composition, lifecycle]
---

# Research MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-research`
**Tools:** 11 — 4 web tools (`web_ping`, `web_search`, `web_find_similar`, `web_extract`), 2 evidence tools (`cite_sources`, `evaluate_evidence`), and 5 research-run/paper tools (`begin_research_run`, `get_research_run`, `finish_research_run`, `annotate_research_run`, `resolve_paper`). (2026-09-03 consolidation: `web_recommend_provider` folded into `web_search` — set `intent` and the tool scores the configured providers, picks the top recommendation, and surfaces the ranking in `provider_recommendations`. 2026-11-12: the 15 `rss_` feed tools were removed by operator decision — see [Feed tools removed](#feed-tools-removed-2026-11-12); `web_browse` was removed the same day — see [web_browse removed](#web_browse-removed-2026-11-12).)
**Auto-start:** Yes with the default built-in set. Free providers work without provider credentials; encrypted research-run persistence requires `HKASK_DB_PASSPHRASE` (`kask/crates/kask_bridge/src/settings.rs:140-165`; `kask/crates/kask_bridge/src/mcp_servers.rs:211-253`).

The research server is the web-research surface: a provider pool
(Exa/Tavily/Brave/SerpAPI/Firecrawl plus free Semantic Scholar/arXiv/OpenAlex/RawFetch)
with RRF fusion. The SerpAPI key also registers two explicit-only engines,
`google_scholar` and `google_books`, that run only when named with
`web_search(provider=...)` and never join fused, `quick` or intent-routed
searches (`src/research/providers/serapi.rs`), content extraction,
response caching, and rate limiting.

## Architecture

- **Credential path:** `ctx.credentials` → `build_provider_pool` — API-key
  providers register only when their key is present; free providers always
  register. A missing key for a recognized provider surfaces as
  `permission_denied` naming the env var
  (`WebError::NoProviderConfigured`), never a silent empty result; an
  unrecognized provider name is `invalid_argument` naming the registered
  providers (`known_provider_kind`, 2026-10-09) — a typo or quoted value
  never reads as a credentials problem.
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
2. **Pool boundary** — `extract_with_fallback` re-validates
   (`validate_provider_url`) so each provider in the fallback chain is
   behind the same gate.
3. **Raw-fetch transport** (`kask/mcp-servers/hkask-mcp-research/src/research/providers/raw_fetch.rs`) — the inner gate:
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

**Address-class policy** (`kask/crates/hkask-mcp-server/src/security.rs`): loopback,
   RFC1918/link-local IPv4, ULA/link-local IPv6, IPv4-mapped IPv6, IPv4
   compatible with NAT64 (`64:ff9b::/96`) unmasking, and unspecified
   destinations (`0.0.0.0/8`, `::` — connecting to `0.0.0.0` routes to
   loopback on Linux) are all refused under the strict config.

**No permissive path:** every fetch this server makes goes through the
   strict config. The permissive config (loopback and private addresses
   allowed) served the feed tools only and left this server with them on
   2026-11-12.

**Operator-visible proxy behavior:** raw fetches deliberately ignore
   `HTTP(S)_PROXY`/`ALL_PROXY` (warned at client build); a proxied destination
   cannot be validated, so it is not silently used. Provider-API clients
   (Firecrawl/Tavily/Exa/Brave/SerpAPI) keep their own proxy support.

Verified regressions (`kask/mcp-servers/hkask-mcp-research/src/research/providers/raw_fetch.rs` inline tests +
`kask/mcp-servers/hkask-mcp-research/tests/tool_behavior.rs`): a redirect to a loopback literal, the 169.254.169.254
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


## Deep-search rerank — decision record

**Requirement (operator directive).** The deep strategy's rerank stage must be
a templated LLM call, not a heuristic. The original heuristic implementation
(signal boosts in `apply_rerank`) was a shortcut, not the design. The output
must also be trustworthy: general models commit category errors — well-formed
but semantically wrong judgments that structural validation cannot catch.

**Decision.** Native rerank protocol: ONE `InferencePort::rerank` call
 carrying all candidates as documents, routed through the inference IPC
 bridge to the serving arm named by the model string's provider prefix
 (`detect_rerank_provider`, `kask/crates/hkask-inference/src/rerank.rs:47-63`
 — the model string IS the routing signal). Two arms:

- **Cloud (`OpenRouter/`-prefixed)** — OpenRouter's rerank endpoint
  (`/api/v1/rerank`); the zed side of the bridge holds the OpenRouter key
  (keychain slot at the provider `api_url`, `https://openrouter.ai/api/v1` —
  the ONE location) and calls the provider directly; the MCP server never
  sees the credential (same pattern as `GenerateBatch`). The bridge rejects
  an unrecognized prefix with `InvalidArgument` naming the requirement
  (`kask/crates/kask_bridge/src/inference_ipc_server.rs:1019-1030`).
- **Local (`ollama/`-prefixed, case-insensitive)** — the D100 §8.1 routed
  shape (operator ruling 2026-11-06): the private leg's rerank models serve
  through the local ollama chat API's Qwen3-Reranker judge protocol — ollama
  has NO rerank endpoint (`/v1/rerank` and `/api/rerank` both 404,
  probe-verified 2026-11-06), so the local arm issues one deterministic
  (temperature 0, generation-bounded) chat call per document and reads the
  FIRST sampled token as the binary verdict — `yes` → 1.0, `no` → 0.0
  (probe-verified on `qwen3-reranker-4b:latest`; the 0.6b's community
  template mis-serves the protocol — rambling output and a wrong verdict on
  an irrelevant document — and is not the default). No credential: the
  ollama descriptor is keyless. The binary score is deliberate — a single
  sampled token's logprob is not a graded probability, so yes-scored
  documents tie above no-scored ones and each group keeps its heuristic
  relative order (the consumer's stable sort). Implementation:
  `kask/crates/hkask-inference/src/rerank.rs:150-310`, dispatched at
  `kask/crates/kask_bridge/src/inference_ipc_server.rs:1033-1066`.

The settings-chain default for `kask.models.rerank_model` is
`DEFAULT_RERANK_MODEL` = `OpenRouter/qwen/qwen3-reranker-8b`
(`kask/crates/hkask-inference/src/model_constants.rs:390`, wired as the
settings default at `kask/crates/kask_bridge/src/settings.rs:553`) — the
same Qwen3-Reranker-8B the 2026-09-11 operator ruling named, on the
servable cloud path. The ruling's first literal
(`deepinfra/Qwen/Qwen3-Reranker-8B`) was unroutable by construction — the
native path serves only the two provider prefixes — so every
default-configured deep search silently degraded to `mode: "heuristic"`
(the operator-reported bug, repaired 2026-11-06, pinned by
`default_rerank_model_is_routable_by_the_native_path`).

**Private-mode routing (D100 §8.1, operator ruling 2026-11-06 — private
mode uses the ollama qwen reranker).** The deep-strategy rerank stage reads
the call's provenance: a private-provenance call reranks through the private
leg's LOCAL model ONLY — the setting `kask.private_inference.rerank_model`
(default `ollama/qwen3-reranker-4b:latest` per `DEFAULT_PRIVATE_RERANK_MODEL`,
emitted unconditionally as `HKASK_PRIVATE_RERANK_MODEL`, admitted by the
research server's allowlist), resolved at the construction seam
(`kask/mcp-servers/hkask-mcp-research/src/hkask_mcp_research.rs:401-421`).
Unset/empty is the operator's explicit opt-out → the surfaced skip: the
heuristic RRF order stands, the reason naming the boundary, the refused
cloud setting, and the private setting that routes locally. The configured
cloud reranker never sees private content. Pins:
`private_call_reranks_via_the_private_model_only` (the recording
falsifier), `private_call_skips_llm_rerank_with_the_boundary_surfaced`
(the opt-out), `public_call_reranks_through_the_configured_model` (the
public control).

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

- *RRF fusion* (`kask/mcp-servers/hkask-mcp-research/src/research/providers.rs`): heuristic signals remain the base
  scoring; the rerank stage reorders on top and falls back to RRF order on
  total failure.
- *Inference IPC bridge* (`InferenceMethod::Rerank`): the call routes to
  the zed side, which dispatches by the model string's provider prefix —
  `OpenRouter/` holds the OpenRouter key and calls the provider's rerank
  endpoint directly; `ollama/` serves the local judge-protocol arm over
  loopback, keyless. The MCP server never sees any credential
  (same pattern as `GenerateBatch`).
- *Model constants* (`kask/crates/hkask-inference/src/model_constants.rs`):
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
| `HKASK_SERPAPI_API_KEY` | SerpAPI key (Google search, YouTube transcripts, and the explicit-only `google_scholar` and `google_books` engines) |
| `HKASK_FIRECRAWL_API_KEY` | Firecrawl extraction API key |
| `HKASK_RESEARCH_DB` | Research SQLite DB path (the run ledger; defaults to `<data-dir>/mcp/research/research.db`) |
| `HKASK_DB_PASSPHRASE` | DB encryption passphrase (required for the research-run tools) |
| `HKASK_WEB_CACHE_TTL_SECS` | Response cache TTL (default 300) |
| `HKASK_WEB_CACHE_MAX_ENTRIES` | Response cache max entries (default 50) |
| `HKASK_RERANK_MODEL` | Rerank model (settings default `OpenRouter/qwen/qwen3-reranker-8b` — repaired 2026-11-06: the previous `deepinfra/…` default was unroutable by the native path, so every default-configured deep search degraded to heuristic mode; the default keeps the ruled Qwen3-Reranker-8B on the servable OpenRouter path). An `ollama/`-prefixed value serves through the local judge-protocol arm |
| `HKASK_PRIVATE_RERANK_MODEL` | The private leg's LOCAL rerank model for private-provenance deep-search rerank (`kask.private_inference.rerank_model`, default `ollama/qwen3-reranker-4b:latest`; emitted unconditionally — an empty value is the explicit opt-out, the surfaced heuristic skip) |
| `HKASK_EMBEDDING_MODEL` | Embedding model for the semantic duplication tier (emitted unconditionally by the settings chain; unset is a legitimate degraded mode — the deterministic shingle floor runs with a surfaced reason) |

## Feed tools removed (2026-11-12)

The 15 `rss_` tools (subscribe, unsubscribe, list, fetch, entries,
mark-read, unread-count, search, OPML export and import, discover, edit-tag,
and the three synthetic-feed tools) were removed by operator decision during
the mcp-tool-review of this server. No code called them; their one
documented consumer, the superforecasting skill's LEAP step, now reads the
source page with `web_extract`.

What this means for stored data:

- **`research.db`** keeps every table it already has. The server no longer
  creates the feed tables (`feeds`, `subscriptions`, `entries`,
  `entry_states`, `entries_fts`, `synthetic_feeds`) and no code reads or
  writes them; no statement drops them. Measured on one temporary database
  with the 27-tool and the 12-tool binary: a run written by the 27-tool
  binary is read and finished by the 12-tool binary, a run written by the
  12-tool binary is read by the 27-tool binary, and feed rows written by
  the 27-tool binary are unchanged after the 12-tool binary has opened and
  written the same file.
- **`rss.db`**, the default filename before the run ledger landed, is not
  read: the server source holds no reference to that filename. It is
  operator-owned and is left where it is.
- A subscription list held in either file is no longer reachable through
  this server. A build before the removal can still export it as OPML.

## web_browse removed (2026-11-12)

`web_browse` was removed by operator decision during the mcp-tool-review of
this server. Its description promised interactive browsing through a
headless browser. Measured on the three providers behind it (Firecrawl,
Tavily, Exa), on an isolated copy with the operator's keys: none read the
`instruction` argument, and the tool returned the same page text as
`web_extract` for the same URL (byte-identical through Firecrawl). No skill,
template or code named the tool. Use `web_extract` to read a page.

## Structured extraction (2026-11-12)

`web_extract` with `format: "json"` is served by Firecrawl only; RawFetch
refuses it. Until this date every such request failed: the provider sent
Firecrawl's v1 body (`jsonOptions`), which the v2 `/scrape` endpoint refuses
with HTTP 400 "Unrecognized key in body", and `json_schema` was accepted by
the tool and never sent. The provider now sends the v2 form, one
`formats` entry `{"type": "json", "schema": …, "prompt": …}`, carrying
`json_schema` and `json_prompt` when given. Measured on an isolated copy
with the operator's key against a public page: the request returned the
structured object.

A Firecrawl response that lacks the requested field (`data.json` for
`json`, `data.markdown` for `markdown`) is an error. Before, a missing
`data.json` was returned as the content `null` and a missing
`data.markdown` as empty text, both as successes.

When every provider in the extract chain fails, the error lists what each
one said, in chain order (`every extract provider failed — firecrawl: …;
rawfetch: …`), under the error kind of the first provider that tried.
Before, only the last provider's error was returned, so a Firecrawl refusal
reached the caller as RawFetch declining JSON. A single failure is returned
unchanged.

## Page text from the keyless extractor (2026-11-12)

RawFetch is the extractor that needs no key, and the fallback when
Firecrawl fails. Until this date it returned an empty string for any page
that begins with a doctype, which is nearly every page: its hand-written
scanner read every `<!` as the start of a comment. It also lost all text
after an inline script containing `<`, left character references
undecoded, and dropped link targets. Measured on one public page before
the change: 0 characters keyless, 937 through Firecrawl.

It now reads the page with html5ever's tokenizer and writes:

- headings with their level, paragraph breaks, itemised and numbered
  lists, one line per table row, and preformatted blocks inside a fence;
- link text with its target, resolved against the fetched URL (fragments
  and non-web schemes keep only their text);
- no script, style, SVG, form-control or template content;
- `<noscript>` content as text, since this fetcher runs no script;
- the page `<title>` as `metadata.title`.

`main_content_only` (default true) now applies to RawFetch: when the page
has a `<main>` or `role="main"` element, that element is the result;
navigation, sidebars and footers are left out either way. With `false`
the whole page is returned. Before, only Firecrawl read the argument.

By content type: HTML is read as above; an image, audio, video or font is
refused naming the type; anything else (plain text, JSON, XML) is returned
as served, with its lines intact.

Measured after the change, same page, keyless, isolated copy: 937
characters in 128 ms; the text matches Firecrawl's apart from line
wrapping and one date Firecrawl rewrote as relative ("last month").

A page is given 2 seconds to parse. Ordinary markup takes milliseconds
per megabyte (5.2 MB in 72 ms, release build); markup built to be slow is
refused with a typed error instead of holding the call. No tree is built:
html5ever's tree builder took 326 ms for 16,000 nested `<div>` and 7 ms
for 16,000 side by side, and nesting depth is chosen by the page.

Not changed: the size of a fetched body is bounded only by the 30-second
request timeout, and a page that needs script to render still yields
little text, reported by the existing under-500-characters note.

## Citation — APA 7 (2026-11-12)

`cite_sources` formats sources as an APA 7 reference list (operator ruling
2026-11-12; rebuilt from a tool that offered four styles and matched none
of them — a `published` of "3 days ago" became the year "3 da"). The
formatter is `src/research/apa.rs`; it is deterministic and makes no model
call.

**Rule source.** Purdue OWL's APA 7 pages "Reference List: Electronic
Sources", "Reference List: Author/Authors" and "Reference List: Basic
Rules", read 2026-11-12. OWL's own examples are the module's test fixtures.
APA's own site was not readable by a plain fetch that day.

**Input.** `sources`: each needs a `title` and a `url` or `doi`. Optional:
`authors`, `published`, `site_name`, `journal`, `volume`, `issue`, `pages`,
`retrieved`. An author is a string (a group or organization, kept as
written) or an object `{family, given}` (a person).

**What is applied.**

- Authors inverted with initials; `&` before the last of 2 to 20 authors;
  for 21 or more, the first 19, an ellipsis and the last.
- No author: the title takes the author position.
- No date: `(n.d.)`.
- A site name equal to the single group author is left out.
- A web page's title is italic; a journal article's title is plain and the
  journal and volume are italic.
- A DOI is written as `https://doi.org/…` and used instead of the URL.
- `Retrieved Month D, YYYY, from` only when `retrieved` is given.
- The list is alphabetized by first author (by title when there is none);
  the same author runs earliest first with undated works first; the same
  author and year take letter suffixes (`2004a`, `2004b`).

**What is not done, and why.**

- Titles are not re-cased. APA uses sentence case for page and article
  titles, which needs knowledge of proper nouns.
- Nothing is looked up. A missing author or date stays missing.
- A `published` value that is not `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or an
  RFC 3339 timestamp is refused, naming the source. A search result's
  `published` can be "4 days ago" or "January 13, 2026": convert it first.

**Output.** `{references: [{source_index, reference}], count, notes}`.
`source_index` is the source's position in the request, since the list is
reordered. Italics are markdown asterisks. `notes` names anything the
formatter could not settle (two undated works by the same author are
listed by title without a suffix).

Three choices are not stated on the OWL pages and are the implementation's
own: a hyphenated given name keeps its hyphen in the initials, no period is
added after a title ending in `.`, `?` or `!`, and italics are asterisks.

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
cosine ≥ 0.85 (the `corpus_dedup_chunks` threshold) via one batch
`InferencePort::embed` call through the inference bridge. The vectors are
compared within the batch only — never stored — so the call requests the
model's native width (no `dimensions` parameter; the injected default is
`OpenRouter/qwen/qwen3-embedding-8b`). Degradation
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
The tool output carries a `run_ledger` note: `{recorded, already_recorded}`
on success (`already_recorded` counts URLs the run already held, so a
repeat observation is not mistaken for a lost write), and
`{recorded: 0, error}` on a write failure — never swallowed. `get_research_run` returns the manifest with per-source
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

## Paper identity (2026-09-09; title mode + OA candidate 2026-09-30; exact-first ranking 2026-10-01)

`resolve_paper` has two modes. Identifier mode: parses any identifier
form — DOI (bare, `doi:`, doi.org URL), arXiv ID, PMID, PMCID, OpenAlex
work ID — into a typed identity with the canonical URL and a stable
kind-prefixed ledger key; every rejection names what was expected.
Bibliographic mode (2026-09-30): pass `title` instead of `query` and
Crossref's bibliographic search returns up to 3 candidate works (DOI,
title, year, venue, first author), all surfaced so the caller can verify
the match (a title search can hit a different work than intended);
candidates rank exact-title matches first (2026-10-01: Crossref's
relevance can put a superstring title above the exact work — observed
live; the stable sort keeps Crossref's order within each group, so
equally-exact candidates such as a preprint and its published version
stay consumer-verifiable); the
top candidate resolves to the typed identity. OpenAlex (a free provider,
always registered) enriches with title/authors/year/venue when a record
exists — plus `oa_pdf_url`, the candidate open-access copy URL from
`best_oa_location.pdf_url` (falling back to `open_access.oa_url`): a
candidate, not a verified document (the copy can be a different version
than the cited work — the consumer verifies identity). The identity is
the deterministic floor and is returned even when the metadata lookup
degrades (a note either way — never silent). arXiv IDs have no direct
OpenAlex lookup key and resolve to no-record. Pass `run_id` to record the
resolution into the run ledger.

## Retrieval-path honesty (2026-09-30)

Three changes from the zk-reference retrieval sweep's lessons:

- `web_extract` surfaces a degradation note when the captured body is
  near-empty (< 500 chars) — the origin likely served a JS shell, a
  bot-block page, or an empty body; when the fetch metadata carries a
  non-2xx status the note names it instead (a gone/moved URL;
  2026-10-01, P2) — and does not cache the degraded
  extraction, so a retry re-fetches instead of replaying the shell.
  The length floor applies to `markdown` only (2026-11-12): a structured
  `json` answer is as long as its answer, and a correct 75-character
  object had been labelled a JS shell and left uncached.
- `web_search` results carry `oa_pdf_url` when a scholarly provider
  holds a candidate open-access copy (OpenAlex `best_oa_location.pdf_url`,
  Semantic Scholar `openAccessPdf.url`) — the same candidate-not-verified
  contract as `resolve_paper`'s enrichment.
- The static `provider_profiles` table moved from every `web_search`
  response (~1KB of repeated context per call) to `web_ping`; web_search
  keeps the per-call `provider_recommendations` audit — of tool-selected
  picks on every selection path since 2026-10-09, of intent-driven picks
  before that. The Semantic Scholar provider retries 429s with 1s/2s backoff
  (two retries; a persistent 429 surfaces with the attempt count), and
  the compound-provider timeout is 20s (raised from 10s — SerpAPI
  consistently exceeded the old bound and never participated).

## Provider selection & closed-vocabulary boundary (2026-10-09)

Two changes from the agent-friction troubleshooting pass (the observed
trigger: `web_search`'s `strategy` parameter rejected quoted values, and
agents burned retries discovering it):

**One live-merged scoring model, two consumers.** The quick strategy's
pick and the `provider_recommendations` ranking share a single model —
`live_merged_score` (`src/research/providers.rs`): the static profile
score (cost + latency tier) plus the live success-rate/p50 penalty from
the in-process `reg.web.provider` feedback loop
(`src/research/performance.rs`; penalties apply only at
`MIN_SAMPLES_FOR_LIVE = 3` samples). The quick pick
(`pick_quick_provider`) therefore drops a persistently failing provider
automatically — pinned by
`pick_quick_provider_drops_a_persistently_failing_provider`. Selection
precedence is unchanged: explicit `provider` > `intent` > `strategy`.

**The audit ranking on every tool-selected path.**
`provider_recommendations` is computed on every call where the tool makes
the selection (intent pick, quick pick, web/news/deep fusion);
explicit-`provider` calls skip it — the caller's deliberate override
carries `selected_provider` and needs no ranking (the field is absent
from the response when empty). The ranking's domain is the profile
table, which since 2026-10-09 includes the free scholarly providers
(openalex, arxiv, semantic_scholar) — so quick's pick of a free
provider is visible in its own audit trail, and `web_ping`'s profile
table lists them too. Their Medium latency keeps the static score at
the former neutral 0.5, so quick picks are unchanged; the deliberate
consequence is that academic/research intents now rank the free
scholarly sources above paid exa (they win by exactly the cost gap),
routing those queries to the free sources. The explicit-only Google
engines stay absent from the ranking by design — they are never
auto-picked, so the audit has nothing to say about them.

**The closed-vocabulary boundary contract.** Every enum-ish string field
parses through one helper pair in `src/research/types/validation.rs`:
`normalize_closed_vocab` (trim, strip one layer of symmetric quotes,
trim) then `parse_closed_vocab` (a spelling→value table) or
`parse_closed_vocab_const` (a domain-owned `&[&str]` const — runs.rs's
`FINISH_STATUSES` / `VERIFICATION_STATES` and types.rs's `INTENTS` keep
single vocabulary ownership, no duplicated tables). Covered fields:
`web_search` strategy/intent/provider/freshness, `web_extract` format,
`evaluate_evidence` duplication,
`finish_research_run` status and `annotate_research_run`
verification_state. Each of these fields publishes its values as an `enum`
in the tool's input schema, except `provider`, whose valid values depend on
which keys are configured. Matching is
case-insensitive; quoted, padded, and case-variant emissions parse to the
canonical spelling, and quoted and bare forms share one cache entry
(normalization precedes the cache key). Unknown values reject with a
teaching error naming the field and the accepted spellings:
`{field} must be one of ..., got '{raw}'`.

**Unknown arguments are refused (2026-11-12).** Every request type denies
unknown fields and every input schema carries `additionalProperties:
false`. Before this, a misspelled optional argument was dropped without a
word: `evaluate_evidence` called with `duplicaton: "semantic"` ran the
default tier and reported nothing. The refusal is `invalid_argument` and
names the field. Pins: `every_tool_refuses_an_unknown_argument_by_name`,
`every_tool_schema_is_closed_to_unknown_arguments`.

**Provider-name error taxonomy.** A `provider` value that isn't
registered errors by which mistake it is (`known_provider_kind`,
`src/research/providers.rs`): an unrecognized name (typo, quoted value)
is `invalid_argument` naming the registered providers; a
recognized-but-unconfigured kind is `permission_denied` naming the env
var that enables it. Before the split both read as "set the API key",
sending agents chasing credentials for spelling errors — and the old
error taught a tool that no longer exists (`web_recommend_provider`,
folded into `web_search` 2026-09-03).

## Capability adoption record (2026-09-09)

The evidence model, research-run ledger, and paper identity above were
adopted from the Feynman (`advaitpaliwal/feynman`, v0.3.49) architecture
analysis through the `refactor-architecture` process. All six capability
commits landed, each TDD with observed RED:

| Commit | Domain | Hash |
|---|---|
| 1 | C1 evidence module (signal model, shingle syndication, sensitivity) | `3086057c20` |
| 2 | C2 schema + rename sweep + begin/get + run_id recording | `a46bc81c4d` |
| 3 | C2 validation gate (annotate + validate_research_run) | `165560e722` |
| 4 | C3 paper identity (PaperId + resolve_paper) | `b1bec58688` |
| 5 | C3 OpenAlex provider + enrichment + run recording | `4ca415f53f` |
| 6 | C1 embedding tier (parameter-gated, Decision 6 default) | `d65c512c67` |

**Deviations from the ratified design — pending operator ratification:**

1. **`evaluate_evidence` does NOT gain `run_id`** (departs from the C2
   design's tool list). Its artifacts are agent-passed; recording them as
   `recorded_by='server'` would launder arbitrary URLs into the
   non-repudiation ledger, defeating Decision 4's fail-closed verified
   gate. The three genuinely server-fetched tools (`web_search`,
   `web_extract`, `web_find_similar`) record; the agent composes
   `evaluate_evidence` over the ledger's own copies via `get_research_run`.
2. **The G2 sensitivity fixture was reconstructed.** The plan's G2 lisp
   gate carried an internal contradiction (its arithmetic flips the
   ordering its prose says must not flip). The intent — a stable fixture
   whose ordering survives substitution — was preserved; the pinned
   fixtures live in `kask/mcp-servers/hkask-mcp-research/src/research/evidence.rs` tests with the lisp agreement
   checks passing.

**Interface-budget note:** the essentialist pass during implementation
reduced C1 to 8 public items (the two heavy profiles became private;
`SetSensitivity` was deleted as a one-field pass-through wrapper) — under
the plan's own budgeted 9.

The full design rationale (candidate ranking, module designs, verification gates, and decisions) remains recoverable from git history; this section is its active successor.

## References

- Zhang, Y., et al. "Qwen3 Embedding: Advancing Text Embedding and Reranking
  Through Foundation Models." arXiv:2506.05176 (2025).
- Sun, W., et al. "Is ChatGPT Good at Search? Investigating Large Language
  Models as Re-Ranking Agents." EMNLP 2023, arXiv:2304.09542.
- Liu, N. F., et al. "Lost in the Middle: How Language Models Use Long
  Contexts." TACL 2023, arXiv:2307.03172.
