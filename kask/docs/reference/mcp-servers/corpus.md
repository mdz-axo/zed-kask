---
title: "Corpus MCP Server — Reference"
audience: [developers, operators]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "MCP Servers"
mds_categories: [domain, composition]
---

# Corpus Server (`hkask-mcp-corpus`)

Unified corpus MCP server — gather, process, and output. Combines document
processing, OCR, chunking, tagging, embedding, QA generation, training data
preparation, style replicas, and prose composition in a single server organized
by corpus flow stage.

## Architecture

```
gather → process → output
```

| Stage | Module | Tools |
|-------|--------|-------|
| Gather | `tools/gather/` | `corpus_discover`, `corpus_cache_work` |
| Process | `tools/document.rs` | `corpus_convert`, `corpus_ocr`, `corpus_is_complex`, `corpus_chunk` |
| Process | `tools/tagging/` | `corpus_tag_chunks` |
| Process | `tools/semantic/` | `corpus_embed`, `corpus_extract_triples` |
| Process | `tools/corpus/` | `corpus_dedup_chunks`, `corpus_consolidate_chunks` |
| QA Output | `tools/semantic/` | `corpus_generate_qa`, `corpus_generate_qa_batch` |
| QA Output | `tools/corpus/` | `corpus_build_prompts`, `corpus_ingest_qa`, `corpus_prepare_training_dataset` |
| Persona Output | `tools/persona/` | `corpus_build_persona`, `corpus_compose`, `corpus_rewrite`, `corpus_mashup`, `corpus_compare`, `corpus_registry`, `corpus_explain` |
| Manage | `tools/storage.rs` | `corpus_cache`, `corpus_query`, `corpus_clear_index`, `corpus_purge_qa` |

## Tool Catalog (27)

### Gather (2)

| Tool | Description |
|------|-------------|
| `corpus_discover` | Discover an academic author's body of work and generate a corpus.yaml. Multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts). |
| `corpus_cache_work` | Cache extracted work content to disk for reuse by the embedding pipeline. |

### Process (9)

| Tool | Description |
|------|-------------|
| `corpus_convert` | Extract text from a document (PDF, MD, HTML, TXT). Falls back to OCR for scanned PDFs. |
| `corpus_ocr` | OCR a document using a local vision model. |
| `corpus_is_complex` | Check whether a PDF page needs OCR (complexity scoring). |
| `corpus_chunk` | Chunk text into passages at configurable token granularity. Single-tier and multi-tier. |
| `corpus_tag_chunks` | Tag chunks with 5W1H, Dublin Core, PKO/FIBO/GOLEM/ESO ontology annotations. |
| `corpus_embed` | Generate embedding vectors with INSTRUCTOR-method ontology annotation. |
| `corpus_extract_triples` | Extract RDF triples with hallucination guard cross-checking ontology tags. |
| `corpus_dedup_chunks` | Deduplicate chunks by semantic embedding similarity. |
| `corpus_consolidate_chunks` | Consolidate related chunks via LLM synthesis. |

### QA Output (5)

| Tool | Description |
|------|-------------|
| `corpus_build_prompts` | Build QA generation prompts with KNN context scaffold and ontology injection. |
| `corpus_generate_qa` | Generate validated QA pairs from one chunk or cross-reference set. |
| `corpus_generate_qa_batch` | Generate validated QA pairs for a batch with concurrent processing. |
| `corpus_ingest_qa` | Parse, quality-filter, dedup, and store QAs as training-ready JSONL. |
| `corpus_prepare_training_dataset` | Prepare a training dataset from ingested QAs. |

### Persona Output (7)

| Tool | Description |
|------|-------------|
| `corpus_build_persona` | Embed a style corpus and create an authorial replica with style centroid. |
| `corpus_compose` | Generate prose in an author's style. |
| `corpus_rewrite` | Rewrite a passage optimized for a Gentle Lovelace quality dimension. |
| `corpus_mashup` | Generate prose blending two authors' styles. |
| `corpus_compare` | Compare author replicas or evaluate a document against persona centroids. |
| `corpus_registry` | Manage the registry of built author replicas. |
| `corpus_explain` | Explain what style centroids are and how the metadata layer works. |

### Manage (4)

| Tool | Description |
|------|-------------|
| `corpus_cache` | Cache processed document text keyed by label. |
| `corpus_query` | Semantic search over indexed passages. Optional LLM-augmented answer. |
| `corpus_clear_index` | Clear the in-memory vector index. |
| `corpus_purge_qa` | Purge QA h_mems from the memory DB by dataset name. |

## Strategy Traits

The persona and QA training branches share operations (chunking, embedding,
triple extraction) but use different implementations. These are declared via
strategy traits in `hkask-services-corpus::embed::strategies`:

| Trait | Persona impl | QA training impl |
|-------|-------------|-----------------|
| `ChunkingStrategy` | `WordCountChunker` (sentence-boundary, word-count) | Token-count (multi-tier, configurable) |
| `EmbeddingStrategy` | Plain (no annotation) | INSTRUCTOR (ontology tags prepended) |
| `TripleExtractionStrategy` | `hkask_services_runtime` (batch) | Jinja2 template + hallucination guard |

Centroid computation is persona-specific (no trait, no QA equivalent).

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_OCR_MODEL` | Vision model for OCR. |
| `HKASK_EMBEDDING_MODEL` | Embedding model for vectorization. |
| `HKASK_TEMPLATE_ROOT` | Root containing `templates/docproc/`. |
| `HKASK_QA_MODEL` | Default provider-prefixed QA model. |
| `HKASK_DEFAULT_MODEL` | Default generation model for prose composition. |
| `HKASK_USE_FAL_DOCRES` | Enable fal.ai docres binarization enhancement. |
| `HKASK_MCP_HOST` | UserPod identity for Regulation narrative memory. |

## Quick Start

The corpus server is a builtin in-process MCP server in zed-kask — it
auto-starts when enabled via KaskSettings (D9a). No standalone CLI command
is needed.

> **Note:** This document lists 27 tools, while `mcp-servers/README.md`
> lists 24. The discrepancy is unresolved here — verify against
> `mcp-servers/hkask-mcp-corpus/src/lib.rs` before relying on either count.
