# hkask-mcp-docproc

Unified document processing MCP server — format conversion, OCR, chunking, triple extraction,
embedding, QA generation, caching, and semantic query. Supersedes the former
`hkask-mcp-markitdown` and `hkask-mcp-doc-knowledge` servers.

## Architecture

```
lib.rs           — Server struct, Regulation observer, shared helpers (extract_text,
                   embedding model resolution, normalize_concept)
helpers.rs        — Math/text helpers (cosine_similarity, tokens_to_words, chunk_word_bounds,
                   serialize_passages)
json_extract.rs   — Brace-balanced JSON extraction from LLM responses (RR-0017)
tools/
  document.rs    — docproc_convert, docproc_ocr, docproc_chunk
  semantic/      — docproc_generate_qa, docproc_generate_qa_batch, docproc_extract_triples,
                   docproc_embed (tool methods in mod.rs; helpers in qa.rs, triples.rs,
                   ontology_io.rs)
  corpus/        — docproc_dedup_chunks, docproc_consolidate_chunks, docproc_build_prompts,
                   docproc_ingest_qa, docproc_prepare_training_dataset (tool methods in
                   mod.rs; helpers in clustering.rs, qa_types.rs, qa_parsing.rs)
  storage.rs     — docproc_cache, docproc_query, docproc_clear_index, docproc_purge_qa
  tagging/       — docproc_tag_chunks (ontology tagging with validate_ontology_tags)
ocr/ (11 modules)
  pipeline.rs    — OcrExecutor trait, run_pipeline orchestrator, cross-validation,
                   semantic enrichment, Levenshtein distance
  config.rs      — ComplexityTier, ComplexityScore, OcrBackend, ThresholdConfig
  document.rs    — OcrResult, CrossValidation, PipelineError, VerificationReport, PipelineOutcome
  decimation.rs  — PDF→images via pdftoppm + Otsu binarization + optional fal.ai docres
  complexity.rs  — Sobel edge detection → page complexity scoring
  routing.rs     — Complexity-driven backend selection with deterministic sampling
  llm_ocr.rs     — Vision LLM OCR via hkask-inference
  tesseract.rs   — Classical OCR via tesseract CLI (TSV confidence parsing)
  verification.rs — Post-pipeline quality checks (page count, word count, empty pages)
  calibration.rs — Threshold drift analysis with Regulation alerting (human approval required)
  mod.rs         — Re-exports
convert.rs        — Format detection, HTML stripping, markdown frontmatter removal
template.rs       — Cached minijinja environment for docproc prompt templates
```

## Tools (17)

| Tool | Description |
|------|-------------|
| `docproc_convert` | Extract text from a document. For PDFs: tries fast text extraction first (~50ms for text-native), falls back to typed OCR pipeline (decimate→score→route→OCR→verify) if near-empty. Supports `force_ocr` mode. Formats: PDF, MD, HTML, TXT. |
| `docproc_ocr` | OCR a document using a local vision model. Requires `HKASK_OCR_MODEL` or explicit `model` parameter. |
| `docproc_chunk` | Chunk text into passages at configurable token granularity. Accepts raw text or file path. Supports single-tier and multi-tier (coarse/medium/fine). Auto-indexing into in-memory vector store. |
| `docproc_tag_chunks` | Tag chunks with multi-dimensional ontology annotations: 5W1H interrogatory dimensions, Dublin Core metadata, PKO/FIBO/GOLEM domain concepts, and expertise level. Uses LLM-based extraction via Jinja2 template with `validate_ontology_tags` schema enforcement. Computes graph-centrality salience. Input guard is always-on (non-disableable). |
| `docproc_dedup_chunks` | Deduplicate chunks by semantic embedding similarity (cosine > 0.85 default). Clusters within each source file, keeps highest-salience chunk per cluster. |
| `docproc_consolidate_chunks` | Consolidate semantically related chunks via LLM synthesis. Clusters by cosine > 0.75, synthesizes each multi-chunk cluster into a single passage, re-embeds. Merges ontology tags with normalization. |
| `docproc_build_prompts` | Build QA generation prompts from tagged chunks with KNN context scaffold, ontology context, and h_mem knowledge graph. Outputs prompts JSONL consumed by `docproc_generate_qa_batch`. |
| `docproc_generate_qa` | Generate validated QA pairs from one source chunk or a cited cross-reference set. Accepts an optional provider-prefixed `model`; every accepted response includes model, parameters, template, and source provenance. |
| `docproc_generate_qa_batch` | Generate validated QA pairs for a batch under one optional provider-prefixed model. Concurrent processing with 3-attempt retry and `degraded` outcome classification on >10% failure rate. |
| `docproc_ingest_qa` | Parse, quality-filter, dedup, and store generated QAs as training-ready JSONL. Stores h_mems with ontology provenance. |
| `docproc_prepare_training_dataset` | Prepare a training dataset from ingested QAs. |
| `docproc_extract_triples` | Extract RDF triples with confidence scores. Uses registry template `docproc/extract-hmems.j2` (falls back to inline prompt). Hallucination guard cross-checks predicate namespace against chunk ontology_tags (M4 fix). |
| `docproc_embed` | Generate embedding vectors via the configured embedding model (`HKASK_EMBEDDING_MODEL` or `~/.config/hkask/settings.json`). Ontology tags prepended as annotation prefixes (INSTRUCTOR method). Reports `degraded` outcome on >10% failure rate. |
| `docproc_cache` | Cache processed document text keyed by label in `~/.config/hkask/docproc-cache/`. |
| `docproc_query` | Semantic search over indexed passages. Embeds query, computes cosine similarity, returns top-k. Optional LLM-augmented answer via `docproc/rag-answer.j2` template. |
| `docproc_clear_index` | Clear the in-memory vector index between document sets. |
| `docproc_purge_qa` | Purge QA h_mems from the memory DB by dataset name. |

## OCR Pipeline

The OCR subsystem implements a **typed, multi-backend, self-verifying** pipeline:

```
PDF → [Decimate] → PageQueue → [Score → Route → OCR] → [Verify] → PipelineOutcome
```

- **Decimation:** PDF→page images via `pdftoppm` with Otsu binarization. Per-page fault tolerant — individual corrupt pages are skipped rather than aborting the entire document.
- **Scoring:** Sobel edge detection classifies pages as Simple/Moderate/Complex.
- **Routing:** Simple pages → Tesseract. Complex pages → LLM vision OCR. Moderate pages → Tesseract with 10% dual-routing for cross-validation.
- **Backends:** Tesseract (CLI with TSV confidence parsing) and LLM vision (via `hkask-inference`, quality heuristic confidence scoring).
- **Verification:** Page count matching, empty page detection, word count estimation (±50% guardrail).
- **Calibration:** Accumulates cross-validation data. When ≥100 samples show >95% agreement between backends, suggests raising routing thresholds via Regulation alert. **Never auto-adjusts** — P4 affirmative consent required.

## Configuration

| Variable | Description |
|----------|-------------|
| `HKASK_OCR_MODEL` | Vision model for OCR (e.g., `DI/allenai/olmOCR-2-7B-1025`). Required for OCR tools. Fallback: `~/.config/hkask/settings.json` → `ocr_model`. |
| `HKASK_EMBEDDING_MODEL` | Embedding model for vectorization and semantic search. Fallback: `~/.config/hkask/settings.json` → `embedding_model`. |
| `HKASK_TEMPLATE_ROOT` | Root containing `templates/docproc/`. Default: `registry` (relative to CWD). |
| `HKASK_QA_MODEL` | Default provider-prefixed QA model. A request-level `model` wins; otherwise the router uses `HKASK_QA_MODEL`, then `HKASK_DEFAULT_MODEL`. |
| `HKASK_USE_FAL_DOCRES` | Set to `true` to enable fal.ai docres binarization enhancement (opt-in, ~40s latency). Requires `FA_API_KEY`. |
| `HKASK_MCP_HOST` | UserPod identity for Regulation narrative memory. |

### OCR Thresholds (via env vars or `settings.json`)

| Variable | Default | Description |
|----------|---------|-------------|
| `HKASK_OCR_SIMPLE_MAX` | 0.05 | Edge-density threshold for Simple tier |
| `HKASK_OCR_MODERATE_MAX` | 0.15 | Edge-density threshold for Moderate tier |
| `HKASK_OCR_SAMPLE_RATE` | 0.10 | Dual-routing sample rate for Moderate pages |
| `HKASK_OCR_TUNEABLE` | true | Whether Regulation calibration may suggest threshold adjustments |

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

Docproc integrates with hkask's shared service layer:

- **Settings:** Model defaults from `~/.config/hkask/settings.json` via `hkask-services-core::HkaskSettings`
- **Template rendering:** Minijinja-based (same pattern as `self_heal.rs` and `ManifestExecutor`)
- **Templates:** `registry/templates/docproc/{generate-qa,extract-hmems,rag-answer}.j2`
- **Regulation:** Daemon-backed event persistence for Curator consumption
- **Inference:** `hkask-inference` router with provider-prefixed model names

## QA Model and Output Contract

Set a provider-prefixed model only after confirming that its provider is configured and the model ID is available to that provider. For example:

```json
{
  "text": "…source passage…",
  "chunk_id": "corpus:researcher:example:0",
  "bloom_levels": ["analyze"],
  "model": "OR/openai/gpt-5.6-terra"
}
```

DocProc rejects malformed JSON, empty questions or answers, unsupported Bloom levels, and missing or out-of-range citations for cross-reference requests. Accepted results include `provenance.generator_model`, `provenance.generator_parameters`, `provenance.prompt_template`, and `provenance.source_chunk_ref`.

## Quick Start

```bash
# The server starts automatically with kask
kask chat
# Or standalone:
hkask-mcp-docproc
```
