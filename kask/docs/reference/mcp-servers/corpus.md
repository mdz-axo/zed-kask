---
title: "Corpus MCP Server — Reference"
audience: [developers, operators]
last_updated: 2026-09-05
version: "0.39.0"
status: "Active"
domain: "MCP Servers"
mds_categories: [domain, composition]
---

# Corpus Server (`hkask-mcp-corpus`)

Unified corpus MCP server — gather, process, and output. Combines document
processing, OCR, chunking, tagging, embedding, QA generation, training data
preparation, style exemplar composition in a single server organized
by corpus flow stage.[^rag-corpus-arch]

## Architecture

```
gather → process → output
```

| Stage | Module | Tools |
|-------|--------|-------|
| Gather | `tools/gather/` | `corpus_discover`, `corpus_cache_work` |
| Process | `tools/document.rs` | `corpus_convert`, `corpus_ocr`, `corpus_is_complex`, `corpus_chunk` |
| Process | `tools/tagging/` | `corpus_tag_chunks` |
| Process | `tools/semantic/` | `corpus_embed`, `corpus_extract_assertions` |
| Process | `tools/corpus/` | `corpus_dedup_chunks`, `corpus_consolidate_chunks` |
| QA Output | `tools/semantic/` | `corpus_generate_qa`, `corpus_generate_qa_batch` |
| QA Output | `tools/corpus/` | `corpus_build_prompts`, `corpus_ingest_qa`, `corpus_prepare_training_dataset` |
| Compose | `tools/compose_tools.rs` | `corpus_compose`, `corpus_rewrite` |
| Manage | `tools/storage.rs` | `corpus_cache`, `corpus_query`, `corpus_clear_index`, `corpus_purge_qa` |

## Tool Catalog (25)

Tool count verified against `#[tool(description = ...)]` annotations in
`mcp-servers/hkask-mcp-corpus/src/` (2026-08-05 audit).

### Gather (2)

| Tool | Description |
|------|-------------|
| `corpus_discover` | Discover an academic author's body of work and generate a `corpus.yaml` for style exemplar construction. Multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts); agentic and curated modes. |
| `corpus_cache_work` | Cache an extracted work's content to disk (`{cache_dir}/{slug}.txt`) so the embedding pipeline can skip re-downloading. |

### Process (9)

| Tool | Description |
|------|-------------|
| `corpus_convert` | Extract text from a document or directory; automatic OCR fallback for scanned PDFs. Directory mode persists one `.txt` per source and resumes non-empty outputs. |
| `corpus_ocr` | OCR a document using a local vision model (`HKASK_OCR_MODEL` or explicit `model` parameter). |
| `corpus_is_complex` | Check whether a PDF needs OCR before a full parse; per-page triage verdicts with typed reasons (scanned, no-text, sparse-text, embedded-images). Cheap text-layer + image-inventory pass. |
| `corpus_chunk` | Chunk text into passages at configurable token granularity; raw text or file path (PDF/MD/HTML/TXT with OCR fallback); single-tier or multi-tier (coarse/medium/fine). |
| `corpus_tag_chunks` | Tag chunks with multi-dimensional ontology annotations: 5W1H, Dublin Core, PKO process concepts, FIBO/GOLEM domain concepts, expertise level; LLM-based extraction with graph-centrality salience. |
| `corpus_embed` | Generate ontology-anchored embedding vectors for corpus chunks; optional INSTRUCTOR-style tag prepending (Su et al. 2023); batch-embeds and stores vectors in the memory DB. |
| `corpus_extract_assertions` | Extract RDF h_mems (subject, predicate, object) from text via the classifier model with 3-attempt retry; tagged chunks guide predicate selection (GOLEM for narrative, schema.org for expository). |
| `corpus_dedup_chunks` | Deduplicate chunks by semantic embedding similarity: cosine clusters per source file above threshold (default 0.85), keeping the highest-salience chunk per cluster. |
| `corpus_consolidate_chunks` | Consolidate semantically related chunks via LLM synthesis (cosine clusters above threshold, default 0.75); re-embeds consolidated text with provenance. |

### QA Output (5)

| Tool | Description |
|------|-------------|
| `corpus_build_prompts` | Build QA generation prompts from tagged chunks with KNN context scaffold, ontology context, and h_mem knowledge graph; outputs prompts JSONL for `corpus_generate_qa_batch`. |
| `corpus_generate_qa` | Generate QA pairs from a single chunk or multi-chunk cross-reference set; Bloom's taxonomy levels; multi-chunk mode requires synthesis across passages with source citation. |
| `corpus_generate_qa_batch` | Execute canonical prepared QA JSONL unchanged via AIMD synchronous calls or provider batches; shared validation, completion accounting and fallible incremental output. |
| `corpus_ingest_qa` | Ingest generated QA pairs: parse, quality-filter, exact-match dedup, write training JSONL, store QA h_mems with 5W1H + Dublin Core / PKO metadata. |
| `corpus_prepare_training_dataset` | Convert Alpaca-format QA JSONL to ChatML training format, apply the lora-training G-D1 dataset-size gate, and return PEFT config recommendations. Bridges the corpus pipeline to the training server. |

### Prepared QA contract (ratified 2026-09-05)

**Operator decision:** One typed prepared QA record; retire compatibility
with the dual-read formats pinned by `45db8da1ef`. Regenerate old prompt files
using `corpus_build_prompts`. This implements the approved corpus identity /
accounting work first, without changing the AIMD ratification (2026-09-03) or
the single/cross-reference `corpus_generate_qa` capability.

`services/qa_pipeline.rs::PreparedQaPrompt` is shared by the builder, reader
and both generation transports. Each nonblank JSONL line contains exactly:

| Required field | Contract |
|---|---|
| `prompt_id` | File-unique string: 1–64 ASCII letters, digits, `-`, `_` |
| `chunk_ref` | Nonblank source chunk reference; may repeat across prompts |
| `source` | Nonblank source string |
| `concepts` | Array of nonblank strings; empty array allowed |
| `salience` | Finite JSON number |
| `qa_type` | Nonblank requested Bloom level; generated `bloom_level` must match exactly |
| `system` | Nonblank, fully prepared system instructions including response format |
| `user` | Nonblank, fully prepared user instructions/passage |

No unknown fields or legacy aliases (`chunk_id`, `text`, `bloom_levels`) are
accepted. The entire file is validated, including duplicate IDs, before
inference or output creation; empty files are invalid. Blank lines are
ignored. Builder IDs are `qa-1`, `qa-2`, … within each file; combined files
must preserve uniqueness. `max_prompts` limits records, not chunks (`0` =
all chunks × positive `prompts_per_chunk`). The builder returns
`total_chunks`, `prompts_written`, `output`.

```json
{"prompt_id":"qa-1","chunk_ref":"corpus:doc:1","source":"doc.txt","concepts":[],"salience":0.5,"qa_type":"factual","system":"Return grounded JSON with qa_pairs containing question, answer and bloom_level (factual).","user":"Primary passage."}
```

Both transports preserve `system`/`user` exactly. Synchronous calls use the
role-aware `InferencePort`; provider batches use `custom_id=prompt_id`.
Configured model routing is retained, including the original provider prefix
or batch suffix. The builder prepares the output instructions once; neither
transport rewraps them as passage text. Expected model output:

```json
{"qa_pairs":[{"question":"Question?","answer":"Answer.","bloom_level":"factual"}]}
```

The shared completion path validates the full response before writing any
accepted rows. Empty arrays, blank questions/answers, incorrect Bloom levels,
and malformed JSON fail the prompt. Each accepted pair is written in the
existing ingest envelope with prompt identity provenance:

```json
{"prompt_id":"qa-1","chunk_ref":"corpus:doc:1","source":"doc.txt","salience":0.5,"qa_type":"factual","response":{"instruction":"Question?","output":"Answer.","type":"factual","concepts":[]},"provenance":{"generator_model":"<selected model or router_default>","prompt_template":"prepared-qa","prompt_id":"qa-1","source_chunk_ref":"corpus:doc:1"},"tokens_used":10}
```

Tokens are completion-level usage repeated across its pair rows, not per-pair
usage. Failed prompts write only `prompt_id`, `chunk_ref`, `source`, `error`;
these rows cannot be ingested as QA. Out-of-order results are matched by ID.
Missing/duplicate known-ID responses, provider errors, malformed result
entries, parse rejection and task join failures each count once as a failed
prompt. Unknown provider IDs or batch-level IPC failures are tool errors.

**Generation summary fields (both transports):**

- `prompts_total`: validated input records.
- `prompts_succeeded`: fully accepted and written prompt responses.
- `prompts_failed`: identified failed prompts.
- `qa_rows_written`: accepted QA pair rows only, not error rows or prompts.
- `output`: requested path.
- `batch_api`: transport boolean.
- `degraded`: existing `BatchOutcome` classification (failure rate ≥10%).

Every successful return satisfies `prompts_total = prompts_succeeded +
prompts_failed`. Output is preflighted before inference, written incrementally,
flushed every 10 prompt completions and at the end. Write, newline and flush
failures propagate as tool errors, never a successful summary. Partial output
may remain on error; no atomic replacement or `fsync` guarantee is implied.
See the [corpus README](../../../mcp-servers/hkask-mcp-corpus/README.md#prepared-qa-jsonl-contract)
for the decision record and full operational contract.

### Compose Output (2)

| Tool | Description |
|------|-------------|
| `corpus_compose` | Generate prose in an author's style using exemplar retrieval and centroid validation. Accepts an optional `config_path` to load a cognition config YAML (mashup or style synthesizer) with a Jinja2 system prompt template. |
| `corpus_rewrite` | Rewrite a passage or code snippet in an author's style, optimized for a specific quality dimension (gentle/schriver/hopper/lovelace/composite). Accepts an optional `config_path` for a cognition config YAML. |

### Manage (4)

| Tool | Description |
|------|-------------|
| `corpus_cache` | Cache processed document text keyed by label in the corpus cache directory (`corpus-mcp/cache/` under the visible artifacts dir, ~/Documents/zk-data/). |
| `corpus_query` | Query the in-memory vector index for passages relevant to a natural-language question: embeds the query, computes cosine similarity against indexed passages, returns top-k results, and can optionally generate an LLM-augmented answer (`tools/storage.rs:73-100`). |
| `corpus_clear_index` | Clear the in-memory vector index; call when starting a new document set to avoid cross-document contamination. |
| `corpus_purge_qa` | Purge QA embeddings and h_mems by entity-ref prefix (embeddings first, then matching h_mems); useful before re-ingesting old training data. |

## Vector index

**Retrieval error contract:** `corpus_query` resolves `HKASK_EMBEDDING_MODEL`
first, then the shared settings file over `HkaskSettings::default()`. The
2026-09-04 operator decision in `55a366a30c` restored code defaults, superseding
the former no-hidden-models policy; see [model settings](../kask-settings.md#models-kaskmodelssettings).
No env override therefore still permits a default model. Explicitly blank model
settings with no env override yield `permission_denied`, naming the setting;
a configured model whose embedding service is unavailable yields `unavailable`.
The isolated `corpus_query_without_inference_surfaces_structured_error` smoke test
covers both conditions and an explicit environment override without live inference.

`index.rs::PassageIndex` owns all cache mutations. Durable identity is canonical
DB path plus entity reference; ephemeral `corpus_chunk` passages have separate
source/ref identity. Repeated embed/consolidate replaces a durable entry. Tag
prefixes are embedding input only; original/synthesized `passage_text` is stored
through MemoryStore and published to the warm index. Consolidation retains its
original source entities.

The approved retrieval slice preserves `a2134949e2`'s **empty-index-only**
fallback: `corpus_query(db_path=...)` hydrates stored embeddings/text only when
the index is empty (`b51bd23106`). It does not switch databases on a nonempty
index. Clear explicitly before selecting a different DB alone. Ephemeral
passages are lost on restart, not rebuilt automatically from JSONL.

Plain and Lisp queries default to `include_text=false`, which suppresses text
only in returned results. Answer generation receives usable original text and
the normalized natural-language question. Missing legacy/centroid text is
surfaced by `missing_passage_text`, `note`, and result `text_available=false`;
these rows are omitted from RAG context. No usable context yields `answer_error`
without a generation call. Re-embed original sources to restore missing text.

`corpus_purge_qa` invalidates only matching DB/ref cache entries, including
relative/absolute/symlink aliases, and cancels overlapping in-flight operations.
Other DBs and ephemeral entries survive. Clear cancels all pending publications
but does not delete DB rows. Cancellation is explicit (`corpus_embed.cancelled`
and `note`, included in `failed`; other indexing paths return an error).
Later operations can publish new data. Hydration and store/cache publication
are synchronous under the same owner lock; inference never holds that lock.

Purge/replacement are not cross-operation transactions: failures can leave a
partially modified DB, but known-deleted results are invalidated and errors
propagate rather than inventing zero counts. An h_mem failure after embedding
purge cannot restore the removed cache results. These guarantees are local to
one corpus server, not independent external DB writers. See the corpus crate
README's Passage retrieval contract and `retrieval_tests.rs` for the offline
real-DB tool/service regression coverage.

## Strategy Traits

The persona and QA training branches share operations (chunking, embedding,
triple extraction) but use different implementations. These are declared via
strategy traits in `hkask_mcp_corpus::corpus::embed::strategies`:[^instructor-corpus-strategy]

| Trait | Persona impl | QA training impl |
|-------|-------------|-----------------|
| `ChunkingStrategy` | `WordCountChunker` (sentence-boundary, word-count) | Token-count (multi-tier, configurable) |

(`EmbeddingStrategy` and `TripleExtractionStrategy` traits were removed —
the persona and QA branches now inline their embedding and triple-extraction
implementations directly.)

Centroid computation is persona-specific (no trait, no QA equivalent).

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_OCR_MODEL` | Vision model for OCR. |
| `HKASK_EMBEDDING_MODEL` | Embedding model for vectorization. |
| `HKASK_TEMPLATE_ROOT` | Root containing `templates/docproc/`. |
| `HKASK_QA_MODEL` | Default provider-prefixed QA model. |
| `HKASK_DEFAULT_MODEL` | Default generation model for prose composition. |
| `HKASK_WEBID` | WebID identity for Regulation narrative memory. |

## Quick Start

The corpus server is a builtin MCP server in zed-kask (a child process over stdio) — it
auto-starts when enabled via KaskSettings (D9a). No standalone CLI command
is needed.[^mcp-spec-corpus-quickstart]

## Footnotes

[^rag-corpus-arch]: Lewis, P., et al. (2020). Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks. arXiv. https://arxiv.org/abs/2005.11401
    Cited for the gather-process-output pipeline design that the corpus server's flow stages follow.

[^instructor-corpus-strategy]: Su, W., et al. (2023). One Embedder, Any Task: Instruction-Finetuned Text Embeddings. arXiv. https://arxiv.org/abs/2212.09741
    Cited for the instruction-conditioned embedding paradigm the strategy traits implement for the persona and QA branches.

[^mcp-spec-corpus-quickstart]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the builtin MCP server model the quick-start section describes.
