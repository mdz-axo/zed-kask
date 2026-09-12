---
title: "Corpus MCP Server — Reference"
audience: [developers, operators]
last_updated: 2026-09-11
version: "0.40.0"
status: "Active"
domain: "MCP Servers"
mds_categories: [domain, composition]
---

# Corpus Server (`hkask-mcp-corpus`)

The editor-managed MCP server processes documents into retrievable passages,
classified chunks, evidence-carrying QA and style centroids. There is one current
schema contract and **24 registered tools**, pinned by
`kask/mcp-servers/hkask-mcp-corpus/src/hkask_mcp_corpus.rs:264–290`.
Parameter additions do not add tools.

The [crate README](../../../mcp-servers/hkask-mcp-corpus/README.md) owns the detailed
runtime contract; the [build skill](../../../../.agents/skills/build-corpus-pipeline/SKILL.md)
owns execution and semantic acceptance gates. Paths in implementation citations
below are relative to the repository root. This reference does not assert that
operator data has been rebuilt, ingested or used for training.

## Tool catalog

| Group | Tool | Purpose |
|---|---|---|
| Gather (3) | `corpus_discover` | Discover an author's works and prepare a corpus manifest |
| | `corpus_discover_company` | Discover company documents from an approved-source manifest |
| | `corpus_cache_work` | Cache extracted work text by slug for reuse |
| Process (9) | `corpus_convert` | Extract document/directory text; quality-gated directory resume and explicit OCR staging |
| | `corpus_ocr` | Process PDF/image pages with the configured image-capable OCR model and verification report |
| | `corpus_is_complex` | Cheap PDF triage; optional compact summary |
| | `corpus_chunk` | Shared bounded word windows with real overlap; directory single-tier or file/text multi-tier |
| | `corpus_tag_chunks` | Identity-correlated ontology classification with explicit terminal outcomes |
| | `corpus_embed` | Persist all selected embeddings, original passage text and source metadata |
| | `corpus_extract_assertions` | Extract assertions from chunk text; optional tags guide predicates |
| | `corpus_dedup_chunks` | Source-local similarity clustering; retain highest-salience representatives |
| | `corpus_consolidate_chunks` | Synthesize source-local clusters, re-embed text, preserve derivation; synthesized tags are unverified |
| QA output (5) | `corpus_build_prompts` | Classified primary rows plus complete-source DB context → prepared QA records |
| | `corpus_generate_qa` | Single/cross-reference text QA; absent source identities require empty evidence |
| | `corpus_generate_qa_batch` | Execute prepared messages unchanged with owned output and reconciled outcomes |
| | `corpus_ingest_qa` | Structural admission/exact dedup with evidence retention and explicit storage status |
| | `corpus_prepare_training_dataset` | Alpaca → ChatML plus dataset-size gate and advisory PEFT recommendations |
| Compose (3) | `corpus_compose` | Retrieve exemplars, generate prose, optionally measure centroid distance |
| | `corpus_rewrite` | Rewrite using a quality dimension and that dimension's centroid |
| | `corpus_centroid` | Average eligible existing embeddings selected by prefix or explicit refs |
| Manage (4) | `corpus_cache` | Cache text in `corpus-mcp/cache/` under the visible artifacts directory |
| | `corpus_query` | Retrieve passages; optional answer generation using available source text |
| | `corpus_clear_index` | Clear warm passages and cancel pending publications; no DB deletion |
| | `corpus_purge_qa` | Explicitly purge a verified DB/entity prefix and invalidate overlapping warm data |

## Template deployment

The host seeds templates at startup and injects `HKASK_TEMPLATE_ROOT`. The server
requires that root; cwd and compile-time checkout lookup are not fallbacks. OCR
fails before vision inference if `ocr-extract.j2` cannot be loaded/rendered; it
has no inline substitute. Rebuild/restart the host to seed changed templates,
then ensure the corpus process is fresh because it caches templates. For repository
tests, set `HKASK_TEMPLATE_ROOT="$PWD/kask/registry"` explicitly. Source-faithfulness
still requires checking OCR output; a prompt constraint is not a quality verdict.

The OCR page contract uses color images with a 1288-pixel longest edge and puts
the task alongside the image in a user message. The response decoder separates
required YAML metadata and declared figure annotations from transcribed text.
`page_reports` preserves annotations as model inference, not source quotations or
created files; `errors` preserves page failure reasons. Unsupported image links,
malformed metadata and rotation requests fail visibly. Old staged reports lacking
the current `ocr_protocol` do not resume; no plain-text compatibility route exists.

## Pipeline parameters

Schema sources: `kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs:843–945`,
`tools/tagging/ops.rs:565–593`, `tools/corpus.rs:586–656`, and
`tools/compose_tools.rs:125–175` under the same crate's `src/`.

| Tool | Inputs and defaults |
|---|---|
| `corpus_convert` | `path`; optional `output`, `target_pages`; `force_ocr=false`, `include_structure=false`. Directory mode requires output. |
| `corpus_is_complex` | PDF `path`; optional `target_pages`, `summary=false` |
| `corpus_ocr` | `path`, optional `model` over the configured OCR model |
| `corpus_chunk` | `text` or `path`, or `input_dir` with `output`; required `entity_ref_prefix`; optional `max_tokens`, `overlap_tokens`, `strip_gutenberg`, `multi_tier`, tier bounds, `target_pages`; `index=true`. Directory mode reports per-source bounded title/contents/index/bibliography/reference exclusions in `boilerplate_exclusion_reports`. |
| `corpus_tag_chunks` | `chunks_jsonl`, `output`; `concurrency` from shared ceiling, `tag_batch_size=10`, `dry_run=false` |
| `corpus_embed` | `chunks_jsonl`, optional `tagged_jsonl`, `db_path`, `passphrase`, optional embedding `model`, `batch_size` |
| `corpus_build_prompts` | `tagged_jsonl`, `output`, `db_path`, `passphrase`; `prefix` defaults `corpus:researcher:`, `context_k=3`, `prompts_per_chunk=5`, `type_distribution="1,1,1,1,1"`, `max_prompts=0`, optional `ontology_bloom_overrides` |
| `corpus_generate_qa` | `chunk_id`, `text` or `texts`, optional `bloom_levels`, optional QA `model` |
| `corpus_generate_qa_batch` | `prompts_jsonl`, `output`, `concurrency`, optional QA `model` |
| `corpus_ingest_qa` | `generated_jsonl`, `output`, `db_path`, `passphrase`, `dataset`, `owner`, `dry_run=false`; pass dataset/owner explicitly |
| `corpus_prepare_training_dataset` | `input_jsonl`, `output_jsonl`, operator-approved `base_model`, optional `system_prompt`, `dry_run=false` |
| `corpus_centroid` | `author`, `db_path`, `passphrase`; optional contained `refs_file`, quality `dimension` |
| `corpus_compose` | `prompt`, `author`, `db_path`, `passphrase`, optional `config_path`, `no_validate=false` |
| `corpus_rewrite` | `content`, `author`, `db_path`, `passphrase`, `dimension=composite`, optional `config_path` |

### Document size and containment

Normal PDF conversion passes the contained canonical path to Poppler and uses
page-based OCR only where needed; it does not read the PDF container under the
32 MiB text/JSONL cap. Large PDFs therefore do not require a forced-OCR workaround.
Raw text and JSONL reads remain capped. This is not a Poppler memory or output-size
limit; extraction quality and complete page/source coverage still require review.

Directory OCR staging retains the complete conversion result in a `.report.json`
companion next to each staged text. Resume verifies the stored source path, exact
text and presence of a boolean verification verdict, then returns its report.
Missing/mismatched reports block that source without silently paying for OCR again.
`document_reports` omits bulky text/structure; `verification_failed` counts failed
OCR verdicts separately from `failed` conversion/I/O operations. Neither `staged`
nor `skipped_staged` admits a document to the accepted extraction directory.

### Chunk bounds and source identity

`max_tokens` defaults through `HKASK_CHUNK_MAX_TOKENS` / shared settings (code
default 256). **Overlap defaults to 64 approximate tokens = 48 words**; explicit
`overlap_tokens=0` means no repetition. Both bounds use `floor(tokens / 1.33)`
whitespace words, not tokenizer counts. Invalid/nonprogressing bounds fail before
reading/writing. Every positive-overlap window repeats the previous suffix and
adds new words; zero uses the same structural/sentence engine. Short final
fragments are valid (`kask/mcp-servers/hkask-mcp-corpus/src/helpers.rs:336–398`;
`kask/crates/hkask-memory/src/text_chunking.rs:123–197`).

Directory mode enumerates immediate `.txt` children, containment-checks each,
and publishes single-tier JSONL with `entity_ref`, original `source`, `text`,
`word_count`. Source components are reversible fixed-width UTF-8 hex, avoiding
punctuation collisions. `zero_chunk_files` surfaces coverage loss. Directory
multi-tier is rejected; per-file/text tiers default 2048/512/128 and share the
same engine (`kask/mcp-servers/hkask-mcp-corpus/src/services/convert.rs:935–1113`;
`src/text.rs:7–16` and `src/tools/document.rs:300–346` in that crate).

### Classification and prompt context

`TaggedChunk.classification` is required. Classified JSON carries the clean-break
protocol stamp: `{"status":"classified","ontology_protocol":"published-term-resolution-v1"}`.
Other outcomes are `{"status":"failed","reason":"..."}` and
`{"status":"unverified"}`. Missing status/protocol and pre-canonical records cannot
be promoted. Tag responses correlate short batch-local `correlation_id` values;
canonical entity refs never enter model authority. The classifier emits compact
tuples of short ID, exceptional `who/when/where/why` dimensions, and 3–5 raw
`candidate_terms`. The server adds `what/how`, document type, Analyst expertise
and Dublin Core subjects; `hkask-bridge-ontology` derives `ontology_tags` and
`concepts`. Downstream readers reject wrong protocols or any
candidate/anchor mismatch. Count actual classified rows and reconcile
`tagged + failed = total_chunks`; fallback annotations and numeric method signals
do not qualify.

The prompt builder rejects nonclassified, stale, noncanonical or duplicate refs
and invalid metadata.
With KNN enabled, it reads full stored passage text and one original source from
text h_mem `ontology.dc_source`, groups candidates by source and selects neighbors
across the entire stored source, independent of tagged-file splits. Missing text,
invalid vectors, source ambiguity, primary mismatch and read/template failures
are errors. `context_k=0` is explicit KNN off, not fallback; DB and knowledge-graph
reads remain required. Summary adds `context_enabled`, `context_scope=complete_source`,
`stored_passages`, `context_links` to `total_chunks`, `prompts_written`, `output`.
Links count neighbors per processed chunk, not per prompt
(`kask/mcp-servers/hkask-mcp-corpus/src/services/prompt_builder.rs:119–342`).

`max_prompts=0` means all `chunks × positive prompts_per_chunk`; positive values
cap records. The five weights expand a rotation that restarts per chunk. Validate
weight input: malformed/empty distributions can resolve to factual-only. Namespace
overrides use `namespace:weights|namespace:weights`. For Brooks explicitly select
**two prompts per chunk**; equal weights then select factual/conceptual, not an
even five-level mix. Preserve every source and remeasure totals under real overlap;
do not force the prior 27,518/55,036 counts. The build skill specifies a single
canonical rebuild from retained sources and verified obsolete-artifact deletion,
without backups or a second Brooks corpus. Those are operator data operations,
not automatic behavior of a docs update.

## Canonical QA records

`PreparedQaPrompt` requires exactly `prompt_id`, `chunk_ref`, `source`, `concepts`,
`salience`, `qa_type`, `system`, `user`. Strings are nonblank, concepts may be an
empty array, salience is finite. IDs are unique in the file, 1–64 ASCII
letters/digits/`-`/`_`. Unknown fields fail. Builder IDs are `qa-<UUIDv5>` derived
from source, chunk ref, QA type and within-type ordinal, stable across partitions.
The whole input is validated before inference/output creation
(`kask/mcp-servers/hkask-mcp-corpus/src/services/qa_pipeline.rs:23–103`;
`kask/crates/hkask-types/src/corpus.rs:38–49`).

Required model response shape:

```json
{"qa_pairs":[{"question":"What is the delay?","answer":"72 hours","bloom_level":"factual","evidence_quotes":[{"chunk_ref":"corpus:delay:0","source":"delay.txt","quote":"The delay is 72 hours."}]}]}
```

`QaEvidence { chunk_ref, source, quote }` contains exactly three nonblank strings.
`evidence_quotes` is required, permits `[]` (no evidence), rejects string arrays
and numeric citations, and survives generated envelopes, ingest and audit.
Every citation identifies its own source/chunk, including context citations.
Generation rejects malformed responses, empty pairs and wrong Bloom levels but
**does not verify quotation truth or semantic entailment**.

Accepted pair rows carry primary identity, `prompt_id`, `salience`, `qa_type`,
`response.{instruction,output,type,concepts,evidence_quotes}`, `provenance` and
completion-level `tokens_used` (repeated per pair). Failed prompts carry identity
and `error`, never an ingestible response. Both transports preserve prepared
messages and source metadata unchanged; provider results match `custom_id` to
prompt identity (`kask/mcp-servers/hkask-mcp-corpus/src/services/qa_pipeline.rs:169–235,333–359`).

### Batch ownership, retries and accounting

The dedicated non-thinking QA model is explicit `model`, otherwise
`kask.models.qa_generation_model` → `HKASK_QA_GENERATION_MODEL`. The setting
defaults empty; missing/malformed/unresolved settings fail visibly. No chat,
classifier, `HKASK_QA_MODEL` or training-base fallback. `:batch` selects provider
batch transport.

Input/output aliases, including symlink/hard-link aliases, are rejected before
truncation. One process-wide canonical output lease spans service instances and
both transports. Workers retain ownership until destroyed, including after an
abort request; it is not a cross-process lock. Synchronous AIMD retries only typed
Connection/Overloaded/Timeout errors, at most **3 total attempts** (2s/4s backoff).
Permanent/configuration errors and response rejection are not retried. Provider
submission has no automatic retry because remote acceptance can be unknown
(`kask/mcp-servers/hkask-mcp-corpus/src/services/qa_batch.rs:24–239`;
`src/batch.rs:73–153` and `src/tools/semantic/batch_api.rs:31–38` in that crate).

Successful summaries expose `prompts_total`, `prompts_succeeded`, `prompts_failed`,
`qa_rows_written`, `output`, `batch_api`, `degraded`, with total = succeeded + failed.
A prompt can emit multiple pairs; error rows are not QA rows. Missing/duplicate
known provider IDs and joins fail identified prompts; unknown IDs and IPC failures
are tool errors. Writes/flushes propagate errors; partial output and cancellation
are explicit. Remaining local workers are aborted, without promising remote job
cancellation. There is no automatic resume, atomic replacement or fsync guarantee.
Wait for owners/workers to stop before inspection and an explicit overwrite rerun.

## Ingestion, audit and training

Flat QA and generated envelopes use the same evidence schema. Ingest structurally
requires nonblank instruction/output/QA type/source/chunk ref and complete evidence
entries; it keeps concise answers and first valid case-insensitive exact
instructions. It neither deduplicates against the DB nor verifies semantics.
`prompt_id`, `provenance`, citations and metadata survive into retained JSONL and
QA h_mems; this tool creates no embeddings
(`kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus/qa_parsing.rs:53–112`;
`src/tools/corpus.rs:181–354` in that crate).

- `total_nonblank_rows = generator_errors + malformed + parsed`.
- `parsed = filter_drops + duplicates + retained`; `filtered = duplicates + retained`;
  `deduped = retained`.
- Non-dry `retained = stored + failed`; `stored_h_mems = stored`.
- `status` is `dry_run` (no DB open/output writes), `complete`, or
  `partial_failure` with per-entity `storage_errors`. The output includes all
  retained rows even if storage fails; file writing and inserts are not atomic.

Before re-ingestion, inspect and explicitly purge the verified
`training:qa:{dataset}:` prefix. Indices restart per call; partitioned ingestion
is not whole-dataset replacement. Never broaden a purge or leave stale datasets.

The read-only `kask/scripts/audit-qa-quality.sh` checks each citation's exact
substring and unique source/chunk identity, reconciles physical rows and reports
six-gram repetition, QA-label distribution and source coverage. Ordinary prose
still needs claim extraction, entailment and narrative review; those unperformed
checks propagate null, not success. Only the explicit structured-citation-only
control has no narrative fields. Exit 0/1/2/64 means narrow checks complete / high
citation findings / missing checks or data / usage error, never launch approval.
See the skill for canonical `grounding-verify`, semantic review and size gates.

Pass the corpus `db_path` and `passphrase` to `training_assemble_dataset`; otherwise
it reads the training server's separate store. `corpus_prepare_training_dataset`
offers advisory PEFT settings. Training still requires operator base-model and
LoRA approval; successful data preparation is not trained capability.

## Style and retrieval

Centroid defaults select `style:{author}:` and store `style:{author}:centroid`.
`refs_file` selects existing refs without copying embeddings; duplicates count
once and missing/empty eligible sets fail. Quality `dimension` selects
`style:{author}:{dimension}:centroid`, independently of embedding vector size.
Recomputation replaces the destination. Shared
`hkask_types::corpus::is_corpus_passage_ref` excludes empty, `:rule:` and all
suffix-`:centroid` refs for both centroid selection and passage retrieval; the
shared helper preserves exclusion behavior (`kask/crates/hkask-types/src/corpus.rs:13`;
`kask/crates/hkask-memory/src/memory_store.rs:523–566`).

Cognition YAML puts `centroid_entity_ref` inside `embedding`. Rewrite always
selects the requested dimension (default `composite`) even with `config_path`.
`centroid_missing=true` returns null distance/pass, not validation success.
Compose `no_validate=true` explicitly skips; other lookup errors propagate.
Optional `embedding.retrieval.declared_method.signal` thresholds all must match;
missing method signals exclude passages and increment `method_signals_missing`.

`corpus_query(db_path=...)` hydrates only an **empty** index. Clear it before
selecting a different DB; a nonempty index does not switch DBs. Durable identity
is canonical DB path plus entity ref. Original stored text supports warm/restarted
retrieval; ephemeral passages do not survive restart. `include_text=false` hides
returned text only. Missing usable text surfaces `text_available=false`,
`missing_passage_text`/note and, if no answer context remains, `answer_error`.

Clear/purge invalidate pending overlapping publications; errors and cancellation
are visible. Prefix deletion is literal/case-sensitive. Publication/replacement
and purge are not cross-operation transactions or coordination with external DB
writers. See the [retrieval contract](../../../mcp-servers/hkask-mcp-corpus/README.md#passage-retrieval-contract)
for the complete failure and scope guarantees.
