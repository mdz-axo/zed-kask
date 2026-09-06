# hkask-mcp-corpus

Unified corpus MCP server — gather, process, and output. Combines document
processing and style composition into a single server organized by corpus
flow stage.

## Architecture

```
hkask_mcp_corpus.rs  — Server struct, shared helpers (embedding_dim, normalize_concept,
                        extract_text, filter_outcome_to_pages, OCR fallback threshold)
index.rs              — Passage identity, upsert/publication, scoped invalidation and DB hydration
batch.rs              — Shared batch infrastructure (retry_with_backoff, BatchOutcome,
                        DEGRADED_FAILURE_THRESHOLD, MAX_RETRIES)
text.rs               — Text processing wrappers (chunk_text, strip_gutenberg_headers)
                        — delegates to MemoryStore pure functions, localizes the
                        dependency so callers don't import a DB type for text processing
guard.rs              — Content safety guard (GUARD, INPUT_GUARD_ENABLED) — shared
                        across all LLM-boundary tool groups
fetch.rs              — Shared HTTP fetch + PDF/OCR fallback + HTML stripping
                        (corpus/fetch.rs) — eliminates duplicated download pipelines
template.rs           — Cached minijinja environment + render_one_shot helper
helpers.rs            — Shared utilities (cosine_similarity, cosine_distance,
                        tokens_to_words, chunk_word_bounds, serialize_passages,
                        chunk_structure, read_jsonl, read_jsonl_lenient)
convert.rs            — Format detection, HTML stripping, markdown frontmatter removal,
                        URL sanitization (sanitize_links)
services/
  convert.rs          — ConvertService (document conversion + directory chunking)
  triples.rs          — TriplesService (RDF h_mem extraction from chunks)
  consolidation.rs    — ConsolidationService (cluster + LLM-synthesize + re-embed)
  prompt_builder.rs   — PromptBuilderService (KNN + concept graph + prepared QA records)
  qa_batch.rs         — Prepared QA transport selection and AIMD synchronous scheduling
  qa_pipeline.rs      — Canonical prepared record, validation, completion and fallible output
tools/
  gather/             — corpus_discover, corpus_cache_work
  document.rs         — corpus_convert, corpus_ocr, corpus_chunk (thin wrappers → ConvertService)
  semantic/           — corpus_generate_qa, corpus_generate_qa_batch, corpus_extract_assertions,
                        corpus_embed (thin wrappers → TriplesService)
  corpus/             — corpus_dedup_chunks, corpus_consolidate_chunks, corpus_build_prompts,
                        corpus_ingest_qa, corpus_prepare_training_dataset, corpus_purge_qa
                        (thin wrappers → ConsolidationService / PromptBuilderService)
  tagging/            — corpus_tag_chunks (ontology tagging with validate_ontology_tags)
  compose/             — corpus_compose, corpus_rewrite (prose generation)
  storage.rs          — corpus_cache, corpus_query, corpus_clear_index
ocr/ (13 modules)
  pipeline.rs         — OcrExecutor trait, run_pipeline orchestrator, cross-validation
  config.rs           — ComplexityTier, ThresholdConfig, TriageConfig
  document.rs         — OcrResult, CrossValidation, PipelineOutcome, split_pdftotext_pages
  llm_ocr.rs          — Vision LLM OCR + shared vision_ocr_bytes primitive
  decimation.rs       — PDF→images via pdftoppm + Otsu binarization
  complexity.rs       — Sobel edge detection → page complexity scoring
  routing.rs          — Complexity-driven backend selection with deterministic sampling
  tesseract.rs        — Classical OCR via tesseract CLI (TSV confidence parsing)
  triage.rs           — Per-page pre-OCR complexity detection
  verification.rs     — Post-pipeline quality checks
  calibration.rs      — Threshold drift analysis with Regulation alerting
  server.rs           — PipelineExecutor (Tesseract + LLM backend bundler)
  mod.rs              — Re-exports
backend/              — Office format backends (docx, pptx, xlsx) + shared markdown parser
bridge/               — Ontology bridges (golem, fibo, eso)
corpus/
  discover/           — Academic author discovery (search, cache, concept extraction, config)
  embed/              — EmbedService (corpus embedding pipeline with metadata layer)
  fetch.rs            — Shared HTTP fetch + PDF/OCR/HTML pipeline
runtime/              — Section classifier + provider intelligence + adaptive monitor
```

## Concurrency

Remote LLM work **ramps; it never launches at the ceiling.** The motivating
failure (2026-09-03): a 412-page OCR run fired 96 concurrent page requests at
a 32-worker RunPod endpoint — instant rejections collapsed to "empty output"
and the whole book silently degraded to Tesseract.

- **Adaptive remote gate** — `batch.rs::AdaptiveLimiter` (AIMD): start at
  floor 2, +1 per success, halve per failure, ceiling = `HKASK_MAX_CONCURRENCY`
  (`KaskGeneralSettings.max_concurrency`, default 96). A service with lower
  capacity is discovered by probing, not by stampede. Gates every remote LLM
  call site: the OCR vision executor (process-lifetime — capacity learning
  persists across runs) and the batch services (assertions, consolidation,
  QA batch, embedding, tagging — per-request).
- **Static local bound** — the OCR pipeline's page semaphore caps total
  in-flight pages (local Tesseract execution + LLM tasks waiting on the
  adaptive gate). Local resources don't need adaptation.
- **Circuit breaker stays subordinate** (`ocr/llm_ocr.rs`) — the limiter adapts
  to capacity; the breaker hard-quarantines a dead endpoint.
- **Outcome signal** — the LLM call's own result: `Ok` grows the allowance,
  `Err` (including typed `EmptyOcrOutput`) backs it off. Quality-only issues
  (JSON parse failures after a successful call) stay neutral.

> **Decision record (PM, 2026-09-03):** This AIMD design is the ratified spec
> (operator decision, Option B). It supersedes the stepped-ramp design of
> `93b9951afe` (start at `concurrency_step`, +step per success, one-step
> backoff on 429/503 throttles only, `general.concurrency_step` setting) —
> deleted with the manifest executor in `a71f79b263` and recovered from git
> history during the 2026-09-03 incident. Do not "restore" the superseded
> design from history; this section is the contract.

## Tools (25)

### Gather (2)

| Tool | Description |
|------|-------------|
| `corpus_discover` | Discover an academic author's body of work and generate a corpus.yaml for style exemplar construction. Delegates to the corpus-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes. |
| `corpus_cache_work` | Cache an extracted work's content to disk for reuse by the embedding pipeline. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading. |

### Process (8)

| Tool | Description |
|------|-------------|
| `corpus_convert` | Extract text from a document. For PDFs: tries fast text extraction first (~50ms for text-native), falls back to typed OCR pipeline (decimate→score→route→OCR→verify) if near-empty. Supports `force_ocr` mode. Formats: PDF, MD, HTML, TXT. |
| `corpus_ocr` | OCR a document using a local vision model. Requires `HKASK_OCR_MODEL` or explicit `model` parameter. |
| `corpus_chunk` | Chunk text into passages at configurable token granularity. Accepts raw text or file path. Supports single-tier and multi-tier (coarse/medium/fine). Auto-indexing into in-memory vector store. |
| `corpus_tag_chunks` | Tag chunks with multi-dimensional ontology annotations: 5W1H interrogatory dimensions, Dublin Core metadata, PKO/FIBO/GOLEM domain concepts, and expertise level. Uses LLM-based extraction via Jinja2 template with `validate_ontology_tags` schema enforcement. Computes graph-centrality salience. Input guard is always-on (non-disableable). |
| `corpus_embed` | Generate embedding vectors via the configured embedding model (`HKASK_EMBEDDING_MODEL` or `~/.config/hkask/settings.json`). Ontology tags prepended as annotation prefixes (INSTRUCTOR method). Reports `degraded` outcome on >10% failure rate. |
| `corpus_extract_assertions` | Extract assertions with confidence scores. Uses registry template `docproc/extract-hmems.j2` (falls back to inline prompt). Hallucination guard cross-checks predicate namespace against chunk ontology_tags. |
| `corpus_dedup_chunks` | Deduplicate chunks by semantic embedding similarity (cosine > 0.85 default). Clusters within each source file, keeps highest-salience chunk per cluster. |
| `corpus_consolidate_chunks` | Consolidate semantically related chunks via LLM synthesis. Clusters by cosine > 0.75, synthesizes each multi-chunk cluster into a single passage, re-embeds. Merges ontology tags with normalization. |

### QA Output (5)

| Tool | Description |
|------|-------------|
| `corpus_build_prompts` | Build QA generation prompts from tagged chunks with KNN context scaffold, ontology context, and h_mem knowledge graph. Outputs prompts JSONL consumed by `corpus_generate_qa_batch`. |
| `corpus_generate_qa` | Generate validated QA pairs from one source chunk or a cited cross-reference set. Accepts an optional provider-prefixed `model`; every accepted response includes model, parameters, template, and source provenance. |
| `corpus_generate_qa_batch` | Execute canonical prepared QA records unchanged under one optional provider-prefixed model. AIMD synchronous processing with 3-attempt retry or provider Batch API; shared completion accounting and fallible incremental output. |
| `corpus_ingest_qa` | Parse, quality-filter, dedup, and store generated QAs as training-ready JSONL. Stores h_mems with ontology provenance. |
| `corpus_prepare_training_dataset` | Prepare a training dataset from ingested QAs. |
| `corpus_purge_qa` | Purge embeddings and h_mems by entity-ref prefix in the named DB; invalidate matching warm passages and overlapping in-flight publications. |

### Prepared QA JSONL contract

> **Decision record (operator, 2026-09-05):** Adopt one canonical prepared QA
> record and shared completion/accounting before the separate media-contract
> work. No backward compatibility is required. This supersedes the dual-read
> formats pinned by `45db8da1ef`. Regenerate old prompt files with
> `corpus_build_prompts`; `chunk_id`/`text`/`bloom_levels` aliases and prepared
> files without prompt identity are rejected. The AIMD ratification above is
> unchanged.

Each nonblank input line is exactly one `PreparedQaPrompt` (`services/qa_pipeline.rs`):

```json
{"prompt_id":"qa-1","chunk_ref":"corpus:doc:1","source":"doc.txt","concepts":["mechanism"],"salience":0.5,"qa_type":"factual","system":"Generate grounded QA. Return JSON with a qa_pairs array of question, answer and bloom_level (factual).","user":"The primary passage and its context."}
```

All eight fields are required; unknown fields are rejected. `prompt_id`,
`chunk_ref`, `source`, `qa_type`, `system`, and `user` are nonblank strings.
`prompt_id` is unique **within the input file**, 1–64 ASCII letters, digits,
hyphens or underscores. Repeated `chunk_ref` values are valid. `concepts` is
an array of nonblank strings (the array may be empty); `salience` is a finite
JSON number. `qa_type` names the requested Bloom level and must match every
accepted pair's `bloom_level` exactly. The builder currently rotates factual,
conceptual, analyze, evaluate and create. Blank lines are ignored; an empty
file or any invalid/duplicate record rejects the entire request **before
inference or output creation**.

The builder supplies the response instructions as part of `system`. It emits
`qa-1`, `qa-2`, … identities in file order; when combining files, identities
must remain unique. `max_prompts` caps **prompt records**, not chunks;
`0` emits all `chunks × prompts_per_chunk` records. `prompts_per_chunk` must
be positive. Builder summary: `total_chunks`, `prompts_written`, `output`.

Both transports forward the prepared `system` and `user` unchanged (separate
roles for synchronous inference; separate messages and `custom_id=prompt_id`
for provider batches). Neither formats the instructions as source text or
adds another generation template. Inference must return:

```json
{"qa_pairs":[{"question":"A grounded question?","answer":"A grounded answer.","bloom_level":"factual"}]}
```

The entire response must parse and every pair must have nonblank question and
answer and the requested Bloom level. One accepted pair becomes one
`corpus_ingest_qa`-compatible JSONL row:

```json
{"prompt_id":"qa-1","chunk_ref":"corpus:doc:1","source":"doc.txt","salience":0.5,"qa_type":"factual","response":{"instruction":"A grounded question?","output":"A grounded answer.","type":"factual","concepts":["mechanism"]},"provenance":{"generator_model":"<selected model or router_default>","prompt_template":"prepared-qa","prompt_id":"qa-1","source_chunk_ref":"corpus:doc:1"},"tokens_used":10}
```

`tokens_used` is the prompt completion's usage, repeated on each pair row;
it is not per-pair usage. A failed prompt instead writes
`{"prompt_id":"qa-1","chunk_ref":"corpus:doc:1","source":"doc.txt","error":"reason"}`
(no `response`, so it is not admitted as training data). Out-of-order provider
results are matched by prompt ID. Missing results, duplicate results for a
known ID, provider errors, malformed result envelopes, rejected QA, and task
join failures each count as one failed prompt. Unknown provider IDs and
batch-level IPC failures return a tool error rather than dropping results.

Successful tool returns have exactly these summary fields:

| Field | Meaning |
|---|---|
| `prompts_total` | Validated input record count |
| `prompts_succeeded` | Prompts whose entire QA response passed validation and whose rows were written |
| `prompts_failed` | Prompts with an identified failure row |
| `qa_rows_written` | Accepted QA pair rows only; excludes failure rows |
| `output` | Requested output path |
| `batch_api` | Whether provider batch transport was used |
| `degraded` | Existing `BatchOutcome` failure-rate classification (at least 10%) |

`prompts_total = prompts_succeeded + prompts_failed`; row count is independent
(a prompt can produce multiple pairs). Output is opened before inference,
written incrementally in completion order for synchronous calls, flushed
every 10 prompt completions and once at the end. Serialization, body write,
newline write, or flush errors propagate as tool errors, **never an OK
summary**. Output may be partial on error; this is not atomic replacement or
an `fsync` durability guarantee. The single-chunk/cross-reference
`corpus_generate_qa` capability is unchanged.

### Compose Output (2)

| Tool | Description |
|------|-------------|
| `corpus_compose` | Generate prose in an author's style using exemplar retrieval and centroid validation. Accepts an optional `config_path` to load a cognition config YAML (mashup or style synthesizer) with a Jinja2 system prompt template. |
| `corpus_rewrite` | Rewrite a passage or code snippet in an author's style, optimized for a specific Gentle Lovelace quality dimension (gentle/schriver/hopper/lovelace/composite). Accepts an optional `config_path` for a cognition config YAML. |

### Manage (3)

| Tool | Description |
|------|-------------|
| `corpus_cache` | Cache processed document text keyed by label in `~/.config/hkask/docproc-cache/`. |
| `corpus_query` | Semantic search over indexed passages. Embeds query, computes cosine similarity, returns top-k. Optional LLM-augmented answer via `docproc/rag-answer.j2` template. |
| `corpus_clear_index` | Clear the in-memory vector index between document sets. |

### Passage retrieval contract

The approved retrieval slice retains the **empty-index-only** DB fallback from
`a2134949e2` and persisted `passage_text` hydration from `b51bd23106`.
`corpus_query(db_path=...)` hydrates the index only when empty; `db_path` is
**not** a per-query DB selector and is ignored on a nonempty index. Clear the
index explicitly to query a different DB alone. Ephemeral `corpus_chunk`
passages are not persisted or recovered after restart.

Durable identity is `(canonical DB path, entity_ref)`; relative, absolute and
symlink paths to the same DB share identity. Ephemeral passages have a separate
source/ref identity. Embedding or consolidating the same durable ref replaces
its vector/text rather than duplicating it. Original source entities survive
consolidation. Ontology annotations affect embedding input only: original or
synthesized text is stored as `passage_text`, returned in warm and restarted
search, and used as answer context.

`include_text` defaults to **false** in plain and Lisp modes. It controls only
returned result text, not answer context. Lisp answers use the normalized
natural-language question. Legacy/centroid rows without usable stored text
carry `text_available: false`; `missing_passage_text` and `note` surface the gap.
They are omitted from answer context. If no usable text is retrieved,
`answer_error` explains the gap and generation is not called. No legacy text
is fabricated or resynthesized; re-embed available sources to restore it.

The index owner serializes synchronous DB publication, hydration and purge.
No mutex is held during inference. Clear cancels all pending publications;
purge cancels pending operations whose input/output refs overlap the named
DB/prefix, leaving other cached DBs and ephemeral passages intact. Cancellation
is visible (`corpus_embed`: `cancelled` count and `note`, also counted in
`failed`; consolidation/chunk indexing: explicit error). A new operation
started after invalidation may publish again. Hydration completes before query
inference, so a paused query cannot later republish a cleared DB snapshot.

Replacement and purge use existing MemoryStore APIs, **not a cross-operation
transaction**. Cache invalidation precedes deletion. Errors report partial
application; a failed replacement may have removed its previous embedding,
and an h_mem deletion failure does not restore already-purged embeddings.
Counts are never replaced with zero on DB errors. Failed embedding batches
(including short or dimension-mismatched responses) are counted honestly.
These guarantees coordinate this server's tools, not independent external DB writers.

Retrieval distinguishes a missing effective model (`permission_denied`, naming
`HKASK_EMBEDDING_MODEL`) from an unavailable embedding service (`unavailable`).
An absent environment override alone does not mean the model is missing: the
2026-09-04 operator ruling restored settings defaults (`55a366a30c`). The smoke
test `corpus_query_without_inference_surfaces_structured_error` exercises default,
explicitly blank, and environment-overridden settings in isolated subprocesses.

## OCR Pipeline

The OCR subsystem implements a **typed, multi-backend, self-verifying** pipeline:

```
PDF → [Decimate] → PageQueue → [Score → Route → OCR] → [Verify] → PipelineOutcome
```

- **Decimation:** PDF→page images via `pdftoppm` with Otsu binarization. Per-page fault tolerant — individual corrupt pages are skipped rather than aborting the entire document.
- **Scoring:** Sobel edge detection classifies pages as Simple/Moderate/Complex.
- **Routing:** Simple pages → Tesseract. Complex pages → LLM vision OCR. Moderate pages → Tesseract with 10% dual-routing for cross-validation.
- **Backends:** Tesseract (CLI with TSV confidence parsing) and LLM vision (via `hkask-inference`, quality heuristic confidence scoring).
- **Verification:** Page count matching, empty page detection, degraded-page detection (pages served by a fallback backend after the routed primary failed or the circuit breaker was open), and the final-backend distribution. Every `corpus_ocr`/`corpus_convert` result carries `verification_passed`, `empty_pages`, `degraded_pages`, `backends`, `llm_breaker_open`, and `llm_concurrency` — a dead LLM endpoint can never wear a passing verdict.
- **Circuit breaker:** After 5 consecutive LLM failures the breaker opens; the cooldown escalates per consecutive opening (30s × 2^(n-1), capped at 300s) so a dead endpoint on a long book run does not re-burn a doomed vision call every fixed window. A success resets the escalation.
- **Calibration:** Accumulates cross-validation data. When ≥100 samples show >95% agreement between backends, suggests raising routing thresholds via Regulation alert. **Never auto-adjusts** — P4 affirmative consent required.

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_OCR_MODEL` | Vision model for OCR (e.g., `DI/allenai/olmOCR-2-7B-1025`). Required for OCR tools. Fallback: `~/.config/hkask/settings.json` → `ocr_model`. |
| `HKASK_EMBEDDING_MODEL` | Embedding model for vectorization and semantic search. Overrides `~/.config/zed-kask/settings.json` → `kask.models.embedding_model`, which overlays the shared settings default. |
| `HKASK_EMBEDDING_DIM` | Embedding vector dimension. Default: 1024 (Qwen3-Embedding-0.6B). Malformed values warn and fall back to 1024. |
| `HKASK_TEMPLATE_ROOT` | Root containing `templates/docproc/`. Default: `registry` (relative to CWD). |
| `HKASK_QA_MODEL` | Default provider-prefixed QA model. A request-level `model` wins; otherwise the router uses `HKASK_QA_MODEL`, then `HKASK_DEFAULT_MODEL`. |
| `HKASK_DEFAULT_MODEL` | Default generation model for all inference (also used for prose composition). |
| `HKASK_CLASSIFIER_MODEL` | Model for section type classification. Falls back to `HKASK_DEFAULT_MODEL`. |
| `HKASK_DB_PASSPHRASE` | Passphrase for the semantic memory DB. Default: `"hkask-default-passphrase-2024"` (dev only — set a real passphrase in production). |
| `HKASK_ENABLE_CONTENT_GUARD` | Set to `false` to disable input-guard scanning (output guard is always active). Default: enabled. |
| `HKASK_WEBID` | WebID identity for Regulation narrative memory. |

### OCR Thresholds (via env vars or `settings.json`)

| Variable | Default | Description |
|----------|---------|-------------|
| `HKASK_OCR_SIMPLE_MAX` | 0.05 | Edge-density threshold for Simple tier |
| `HKASK_OCR_MODERATE_MAX` | 0.15 | Edge-density threshold for Moderate tier |
| `HKASK_OCR_SAMPLE_RATE` | 0.10 | Dual-routing sample rate for Moderate pages |
| `HKASK_OCR_TUNEABLE` | true | Whether Regulation calibration may suggest threshold adjustments |
| `HKASK_OCR_CONCURRENCY` | 4 | Number of pages sent to the vision model in parallel |
| `HKASK_OCR_RENDER_DPI` | 72 | Page-render resolution for the OCR pipeline. Default 72 keeps the JPEG payload inside the vision model's 128K-token context; raise to ~150 for better Tesseract accuracy on scanned books at the cost of render memory and LLM payload size. Malformed values warn and fall back to 72. |

### Page Triage Thresholds (per-page pre-OCR complexity detection)

| Variable | Default | Description |
|----------|---------|-------------|
| `HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN` | 20 | Per-page word count at/above which a page is text-native (no OCR) |
| `HKASK_OCR_TRIAGE_MIN_IMAGE_PT` | 25.0 | Min image side (points) to count as a substantial image |
| `HKASK_OCR_TRIAGE_FULL_PAGE_PT` | 500.0 | Image dims (pt) at/above which a no-text page is classified `Scanned` |
| `HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT` | 150.0 | Min image side (pt) to flag `EmbeddedImages` on a text page |
| `HKASK_OCR_TRIAGE_TUNEABLE` | true | Whether Regulation calibration may suggest triage threshold adjustments |

## Regulation Observability

The server emits Regulation spans under these targets for cybernetic feedback:

| Target | When |
|--------|------|
| `reg.pipeline.ocr` | Pipeline verification (every run) |
| `reg.pipeline.ocr.verification_failed` | Verification report fails |
| `reg.pipeline.ocr.low_confidence` | LLM OCR confidence < 0.3 |
| `reg.pipeline.ocr.rate_limit` | Inference rate-limited (429) |
| `reg.pipeline.ocr.collusion` | Both backends produce empty output |
| `reg.pipeline.decimation` | Page load failures |
| `reg.pipeline.decimation.binarize` | Otsu produces uniform output |
| `reg.pipeline.calibration` | Threshold drift detected |
| `reg.docproc.index` | Indexing requested but embedding unavailable |

## Shared Infrastructure

Corpus server integrates with hkask's shared service layer:

- **Settings:** Model defaults from `~/.config/hkask/settings.json` via `hkask-services-core::HkaskSettings`
- **Template rendering:** Minijinja-based (`render_docproc_template` cached, `render_one_shot` for one-shot)
- **Templates:** `registry/templates/docproc/{generate-qa,extract-hmems,rag-answer,tag-chunks,build-prompts,consolidate-chunks,ocr-extract}.j2`
- **Regulation:** Daemon-backed event persistence for Curator consumption
- **Inference:** `hkask-inference` router with provider-prefixed model names
- **Compose:** `compose.rs` — `ComposeService` for prose generation with exemplar retrieval
- **Corpus:** `corpus/embed/service.rs` — `EmbedService::embed_corpus` (style exemplar embedding)
- **Services:** `services/` — `ConvertService`, `TriplesService`, `ConsolidationService`, `PromptBuilderService`

## Quick Start

```bash
# The server starts automatically with kask
the zed-kask editor
# Or standalone:
hkask-mcp-corpus
```