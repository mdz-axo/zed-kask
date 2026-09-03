---
name: build-corpus-pipeline
description: "Ingest a folder of source documents through a complete text-processing pipeline (convert → chunk → embed → tag) with optional style exemplar construction and QA pair generation for LoRA training. 10-stage PDCA pipeline grounded in PKO procedural ontology, Dublin Core metadata, and Bloom's Taxonomy for QA generation."
---

# Build Corpus Pipeline

Ingest a folder of source documents through a complete text-processing
pipeline: document conversion → segmentation → vectorization → ontology
annotation, with optional style exemplar construction and QA pair
generation for LoRA training.

## Ontological Anchors

| Ontology | Domain | Role in skill |
|----------|--------|---------------|
| **PKO** (Procedural Knowledge Ontology) | Industrial processes | The pipeline is a procedure with specification/execution separation — each stage has a specification (what it should produce) and an execution (the MCP tool call). Stages are sequential with dependency edges. |
| **Dublin Core** | Metadata, documentation | Stage 4 tags chunks with Dublin Core metadata (creator, date, subject, source, type). The corpus itself is a metadata-managed artifact. |
| **Bloom's Taxonomy** | Educational assessment | Stage 6–7 QA generation uses Bloom cognitive levels to drive question difficulty distribution. |
| **Text processing pipeline** (standard NLP) | Corpus construction | The canonical pipeline: convert → segment → vectorize → annotate. Each stage's output feeds the next stage's input. Embedding precedes tagging because `corpus_embed` accepts `tagged_jsonl` as optional — the full corpus can be vectorized without waiting for LLM-based annotation. |

## PDCA Shape

Derived from the standard NLP corpus construction pipeline, adapted
through PKO's specification/execution separation. Embedding (Stage 3)
precedes tagging (Stage 4) because the embedding tool accepts tags as
optional input — this allows the full corpus to be vectorized immediately
after chunking, while the slower LLM-based tagging proceeds in batches
for QA generation.

```
Plan:  Stage 0 — Validate   → Check corpus source exists, is non-empty, has readable files
Plan:  Stage 1 — Convert    → Extract text from all documents in the source folder
Do:    Stage 2 — Chunk      → Segment text into passages at configurable token granularity
Do:    Stage 3 — Embed      → Generate ontology-anchored embedding vectors for ALL chunks
Check: Stage 4 — Tag        → Annotate chunks in batches (5W1H + Dublin Core + PKO + FIBO/GOLEM)
Do:    Stage 5 — Compose    → (Optional) Build style exemplar from the embedded corpus
Do:    Stage 6 — QA Prompts → (Optional) Build QA generation prompts from tagged chunks
Do:    Stage 7 — QA Gen     → (Optional) Batch-generate QA pairs from prompts
Do:    Stage 8 — Ingest QA  → (Optional) Parse, quality-filter, dedup, write training JSONL
Act:    Stage 9 — Assemble  → (Optional) Assemble QA pairs into ChatML training dataset
Act:   Stage 10 — Verify    → Grill-me interrogation + convergence check
```

## When to Use

- You have a folder of source documents (PDFs, HTML, TXT, MD) and need to
  build a text corpus with embeddings for semantic retrieval.
- You want to construct a style exemplar (authorial style model) from a
  corpus of authored works.
- You want to generate QA pairs from a corpus for LoRA fine-tuning.
- You need the full convert → chunk → embed → tag pipeline as a single
  governed sequential process with per-stage convergence checks.

## When NOT to Use

- You have a single document, not a folder — use `corpus_convert` directly.
- You already have chunks and only need embeddings — call `corpus_embed` directly.
- You need real-time interactive Q&A, not a build pipeline — use `corpus_query`.
- You want to discover an author's works from the web — use `corpus_discover`.

## Inputs

All inputs are parameterized. None are hardcoded.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `corpus_source` | string | yes | — | Absolute path to a folder containing source documents |
| `entity_ref_prefix` | string | yes | — | Prefix for entity references in chunk IDs (e.g. "john-brooks") |
| `db_path` | string | yes | — | Path to the vector database file for embeddings and h_mems |
| `passphrase` | string | yes | — | Passphrase for the encrypted vector DB. Resolve via `hkask_mcp_server::server::resolve_db_passphrase` helper if available, otherwise from credentials. |
| `reference_author` | string | no | null | Author name for style exemplar construction (e.g. "John Brooks"). When provided, Stage 5 runs. |
| `config_path` | string | no | null | Path to a cognition config YAML (mashup or style synthesizer). When provided to `corpus_compose`, loads the Jinja2 system prompt template, embedding model, retrieval parameters, and validation thresholds from the file. |
| `enable_qa` | boolean | no | true | Whether to run Stages 6–9 (QA generation and training dataset assembly) |
| `max_tokens` | integer | no | 512 | Maximum tokens per chunk |
| `overlap_tokens` | integer | no | 64 | Token overlap between adjacent chunks |
| `multi_tier` | boolean | no | true | Whether to use multi-tier chunking (coarse + medium + fine) |
| `embedding_model` | string | no | from config or `DEFAULT_EMBEDDING_MODEL` | Embedding model for vectorization |
| `batch_size` | integer | no | 25 | Embedding batch size |
| `tag_batch_size` | integer | no | 1 | Number of chunks per tagging LLM call. MUST be 1 — the classifier model returns a single JSON object, not a JSON array, so batch_size > 1 results in only the first chunk getting real tags (10% success rate). With batch_size=1, success rate is 75-100%. Do NOT set this higher than 1. |
| `bloom_levels` | list | no | ["remember","understand","apply","analyze","evaluate","create"] | Bloom levels for QA difficulty distribution |
| `prompts_per_chunk` | integer | no | 2 | QA prompts generated per chunk. With Bloom expansion across 5 levels (factual/conceptual/analyze/evaluate/create), each chunk can produce up to 5 QA pairs. Default 2 balances dataset size against generation cost. |
| `max_prompts` | integer | no | tagged_count × prompts_per_chunk | Maximum total QA prompts. MUST be set to at least `tagged_count × prompts_per_chunk` to avoid capping output below the target. For a 33K-chunk corpus at 2 prompts/chunk, this is 67,646. Setting max_prompts to 500 or 600 produces a toy dataset — do NOT do this. |
| `context_k` | integer | no | 5 | KNN context scaffold chunks per QA prompt |
| `train_split` | float | no | 0.9 | Fraction of QA pairs for training split |
| `dataset_name` | string | no | derived from `entity_ref_prefix` | Dataset name for training assembly |
| `concurrency` | integer | no | 4 | Per-tool internal concurrency for tagging and QA generation |
| `parallel_subagents` | boolean | no | true | Whether to use `spawn_agent` subagents for parallelizable stages (Convert, Tag, QA Gen). When false, stages run sequentially with tool-level concurrency only. |

## Concurrency and Parallel Execution

The pipeline parallelizes independent work units using `spawn_agent`
subagents, bounded by the process-wide concurrency settings in
`KaskGeneralSettings`:

| Setting | Default | Role |
|---------|---------|------|
| `max_concurrency` | 96 | Process-wide ceiling on concurrent cloud inference calls. Shared across skill execution, corpus OCR, and MCP tool calls. |
| `ocr_concurrency` | 4 | Corpus-specific: pages sent to the vision model in parallel during OCR. Overridable via `HKASK_OCR_CONCURRENCY`. |

### Concurrency dispatch pattern

When dispatching parallel work units (file conversions, chunk batches,
tagging batches), start with a small batch of 4 concurrent subagents and
scale up on success:

1. **Start** with 4 concurrent subagents.
2. **On success** (all agents returned without error or throttle), add
   4 more agents for the next round.
3. **On throttle** (429/503 from the inference provider), back off to the
   last successful concurrency level and hold there.
4. **Ceiling**: never exceed `max_concurrency` concurrent agents.

This avoids jumping straight to `max_concurrency` and triggering
provider throttles.

### When to use subagents vs. tool concurrency

- **`spawn_agent` subagents**: use when work units are independent files
  or batch files that can be processed without shared state. Each
  subagent gets its own session and can call MCP tools independently.
  `MAX_SUBAGENT_DEPTH` is 1 — subagents cannot spawn further subagents.
- **Tool-level `concurrency` parameter**: `corpus_tag_chunks` and
  `corpus_generate_qa_batch` accept a `concurrency` parameter that
  controls internal parallelism within a single tool call. Use this for
  batches small enough to complete within the tool's timeout.
- **Combined pattern**: for large corpora, split work into batches,
  dispatch batches to subagents (step-up ramp), and let each subagent
  call the tool with its own `concurrency` parameter. The subagent count
  × per-subagent concurrency should stay within `max_concurrency`.

### Parallelizable stages

| Stage | Parallelizable? | Work unit | Pattern |
|-------|----------------|-----------|--------|
| Stage 1 (Convert) | Yes — per file | Each source file | Spawn subagents per file (or per file group), step-up ramp |
| Stage 1a (OCR) | Yes — per PDF | Each complex PDF | Spawn subagents per PDF, bounded by `ocr_concurrency` |
| Stage 2 (Chunk) | Partial — per sub-directory | Groups of extracted text files | Split input dir, dispatch chunk calls to subagents, merge outputs |
| Stage 3 (Embed) | No — single DB write | All chunks at once | Use tool's `batch_size` parameter for internal batching |
| Stage 4 (Tag) | Yes — per batch | Chunks JSONL batch files | Spawn subagents per batch, step-up ramp, each with `concurrency` param |
| Stage 6 (QA Prompts) | Partial — per batch | Tagged chunk batches | Dispatch to subagents if prompt count is large |
| Stage 7 (QA Gen) | Yes — per batch | Prompt batch files | Spawn subagents per batch, step-up ramp |

### Fixed vs Parameterizable

- **Fixed**: the pipeline shape (convert → chunk → embed → tag), the ontology
  annotation dimensions (5W1H + Dublin Core + PKO + FIBO/GOLEM), and the
  embedding output format (ontology-anchored vectors stored in the DB).
- **Parameterizable**: chunk granularity, overlap, embedding model, batch
  size, tag batch size, multi-tier chunking, Bloom level distribution,
  prompts per chunk, max prompts, context scaffold size, train split ratio.

## Composed Skills

| Skill | Role | When invoked |
|-------|------|-------------|
| `essentialist` | Stage 0 deletion test | Apply G1 (deletion test) to each stage — is this stage necessary? Stages 5–9 are already optional. |
| `task-breakdown` | Pipeline decomposition | Decompose the stages into INVEST-compliant verifiable tasks for execution tracking |
| `pragmatic-semantics` | QA certainty classification | QA quality checks classify pair certainty (IS/OUGHT, epistemic mode) to prevent mixing declarative with speculative in training data |
| `grill-me` | Verification (Stage 10) | Socratic interrogation of pipeline output across escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis) |
| `idiomatic-lisp` | Deterministic invariant checks | Use `lisp_eval` for structural invariants between stages (chunk counts, embedding completeness, QA pair counts). Full idiomatic-lisp design principles not needed for sequential pipeline. |

## Process Awareness Scaffolding

### The anti-degradation principle (STRONGER than per-stage gates)

The per-stage quality gates below check individual stage output. But the
meta-failure is declaring the PIPELINE complete when the final output is
below target. The rule:

**The pipeline is NOT complete until `train_examples ≥ target_condition.min_qa_pairs`.
A pipeline producing 542 examples when the target is 40,000 is FAILING at
98.6%, not 'complete' or 'improved' or 'a good start'.**

Do NOT compare output to zero (542 > 0 = success). Compare output to the
target (542/40,000 = 1.4% = failure).

### Semantic quality verification (not just structural)

Structural checks (no duplicates, correct format, non-empty answers) are
NECESSARY but NOT SUFFICIENT. After assembling the training dataset, run
these semantic checks:

1. **Boilerplate contamination**: grep for repeated analytical phrases
   across answers. If >10% of answers contain the same multi-word phrase
   not from the source text, the prompt template is leaking.
2. **Grounding rate**: sample 10 random QA pairs, extract 4-word sequences
   from answers, search for them in source chunks. Target: ≥60% of
   sequences found in source text. Below 30% = answers are synthesized,
   not grounded.
3. **Bloom distribution**: verify even distribution across Bloom levels.
   Heavy skew toward one level means the prompt cycling is broken.
4. **Subject diversity**: verify QA pairs cover the corpus's actual subject
   matter, not a single imposed frame. If 70%+ of answers mention the same
   concept, the prompt template is forcing an external lens.

### Stage 4 tagging constraint

The `corpus_tag_chunks` tool's `tag_batch_size` parameter MUST be 1. The
classifier model (glm-5.2) returns a single JSON object, not a JSON array.
With batch_size > 1, only the first chunk per LLM call receives real
ontology tags — the rest get fallback tags (`{sumo: [entity]}`). This was
the root cause of the 90% tagging failure rate observed across multiple
sessions. With batch_size=1, the success rate is 75-100%.

Throughput with batch_size=1: each 20-chunk file at concurrency=10 takes
~5 seconds. For 33,823 chunks in 677 batch files of 50, split each into
2-3 calls of 20 chunks, totaling ~1,700 calls × 5s = ~2.4 hours.


Every stage has a **quality gate** — a deterministic check that the stage's
output meets expected parameters. Quality gates are NOT soft warnings. If a
gate fails, the pipeline HALTS. Do not proceed to downstream stages with
degraded input.

### The anti-degradation rule

**Never silently reduce input size to work around a stage failure.** If a
stage produces fewer outputs than expected, either:
1. Fix the root cause and re-run the stage, OR
2. Halt with a failure report explaining what went wrong.

Do NOT create a "representative subset" or "sample" to bypass a timeout or
failure. A 33,000-chunk corpus that gets reduced to 380 chunks is a 98.8%
data loss — the pipeline's downstream stages would produce garbage
embeddings and a meaningless style exemplar. The quality gate exists to prevent
exactly this.

### Expected-range estimation

Before running the pipeline, estimate the expected chunk count:

```
expected_chunks ≈ (total_text_tokens / max_tokens) × tier_multiplier
```

Where `tier_multiplier` is 1 for single-tier, ~2-3 for multi-tier chunking.
For a 138-document corpus at ~250 tokens/chunk with multi-tier, expect
25,000–40,000 chunks. Record this estimate and use it in Stage 2's quality
gate.

## Instructions

### Stage 0 — Validate corpus source

1. Check the `corpus_source` path exists and is a directory:
   ```
   ls -la {{ corpus_source }}
   find {{ corpus_source }} -type f | wc -l
   ```

2. Count files by extension to confirm the corpus has readable content:
   ```
   find {{ corpus_source }} -type f | sed 's/.*\.//' | sort | uniq -c | sort -rn
   ```

3. Apply the essentialist deletion test: is this corpus worth processing?
   If the file count is 0, halt with error: "corpus_source is empty or
   does not exist".

4. **Quality gate**: verify file count > 0 AND at least one readable
   file type (pdf, html, txt, md). Call `lisp_eval`:
   ```
   form: "(if (and (> file_count 0) (> readable_count 0)) 'pass 'fail)"
   ```
   Substitute the actual counts as literals. If `'fail`, halt.

### Stage 1 — Convert documents to text

**Parallelizable**: per-file. If `parallel_subagents` is true, spawn
subagents to convert files concurrently with the step-up ramp.

1. Enumerate all source files and classify by type:
   ```
   find {{ corpus_source }} -type f -name '*.pdf' -o -name '*.PDF' | wc -l
   find {{ corpus_source }} -type f -name '*.html' | wc -l
   find {{ corpus_source }} -type f -name '*.txt' -o -name '*.md' | wc -l
   ```

2. **Sequential fallback**: if `parallel_subagents` is false or the
corpus is small (≤ 20 files), call `corpus_convert` on the source folder:
   - `path`: `{{ corpus_source }}`
   - `output`: `corpus/extracted/{{ entity_ref_prefix }}/`

   If `corpus_convert` is unavailable or the source path is outside the
   MCP tool's allowed root, convert files via terminal commands:
   - PDFs: `pdftotext -q <input> <output>`
   - HTML: Python `html.parser` to extract text
   - TXT/MD: copy as-is
   Run a conversion script that handles all file types and logs per-file
   results.

3. **Parallel subagent dispatch** (if `parallel_subagents` is true and
   corpus has > 20 files): split files into groups and spawn subagents
   with the step-up ramp:

   a. Group files into work units of ~10 files each (or 1 file per group
      for large PDFs > 10 MB).
   b. Start with 4 subagents. Call
      `spawn_agent` for each work unit:
      - `label`: "Convert batch {{ batch_index }}"
      - `message`: "Convert these files to text in
        `corpus/extracted/{{ entity_ref_prefix }}/`: {{ file_list }}.
        Use `pdftotext -q` for PDFs, Python html.parser for HTML, copy
        for TXT/MD. Log per-file results. Report the count of
        successfully converted files."
   c. On all agents succeeding, add 4 more agents for
      the next round.
   d. On any agent throttling (429/503) or erroring, hold at the current
      level for the next round before ramping further.
   e. Continue until all work units are dispatched, up to `max_concurrency`
      concurrent agents.
   f. Collect all subagent outputs and aggregate the converted file count.

4. For PDFs that may need OCR, call `corpus_is_complex` first:
   - `path`: path to each PDF
   - If complex and OCR is available, call `corpus_convert` with
     `force_ocr: true` for that file (the page-by-page OCR pipeline —
     `corpus_ocr` is the single-image tool; on PDFs it routes through the
     same pipeline and errors on zero text). OCR concurrency is bounded by
     `ocr_concurrency` (default 4, overridable via `HKASK_OCR_CONCURRENCY`).
     For multiple complex PDFs, spawn subagents per PDF up to
     `ocr_concurrency` concurrent agents.
   - If complex and OCR unavailable, HALT with a failure report naming
     the file: "PDF {{ filename }} requires OCR but OCR is unavailable".
     Never skip the file and continue — a silently skipped source is data
     loss presented as progress.
   - After OCR, verify the OCR output the same way as any extraction
     (word-count floor in step 5). An OCR result with zero words is a
     failure, not a success.

5. Verify the conversion output — count reconciliation AND per-file word
   counts. Byte-size checks are insufficient: a 412-byte extraction of a
   scanned PDF is zero-word garbage that passes a `< 100c` check (observed:
   5 of 138 extractions were 21–412 bytes of garbage and passed the old
   gate). Run the word-count audit:
   ```
   python3 -c "
   import os
   d = 'corpus/extracted/{{ entity_ref_prefix }}'
   files = sorted(os.listdir(d))
   failed = []
   for f in files:
       text = open(os.path.join(d, f), encoding='utf-8', errors='replace').read()
       words = len(text.split())
       if words < 50:
           failed.append((f, len(text), words))
   print(f'{len(files)} extracted, {len(failed)} failed the word-count floor')
   for f, size, words in failed:
       print(f'  {f}: {size}B, {words} words')
   "
   ```

6. **Quality gate**: extracted_count == source_count (every input file
   has an output — no silent skips) AND zero failed extractions (every
   file passes the ≥ 50-word floor or has been routed through OCR). A file
   whose extraction is empty or garbage is a FAILED EXTRACTION requiring
   OCR — it is NEVER "not a valid source". Call `lisp_eval`:
   ```
   form: "(if (= extracted_count source_count)
            (if (= failed_extractions 0) 'pass 'fail-quality)
            'fail-coverage)"
   ```
   Substitute actual counts as literals.
   - `'pass`: proceed to Stage 2
   - `'fail-coverage`: HALT — files were skipped silently; identify them
     before proceeding
   - `'fail-quality`: route every failed extraction through OCR
     (`corpus_convert` with `force_ocr: true`), re-run the audit, and do
     not proceed until all pass. Never drop a failed file from the corpus.

### Stage 2 — Chunk the text

**Parallelizable**: per sub-directory for very large corpora. For most
corpora (≤ 500 files), a single `corpus_chunk` call is sufficient.

1. **Sequential** (default): call `corpus_chunk` on the extracted text
directory:
   - `input_dir`: `corpus/extracted/{{ entity_ref_prefix }}/`
   - `output`: `corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl`
   - `entity_ref_prefix`: `{{ entity_ref_prefix }}`
   - `max_tokens`: `{{ max_tokens }}`
   - `overlap_tokens`: `{{ overlap_tokens }}`
   - `multi_tier`: `{{ multi_tier }}`

2. **Parallel subagent dispatch** (if `parallel_subagents` is true and
   extracted file count > 500): split the extracted directory into
   sub-directories of ~100 files each, then spawn subagents with the
   step-up ramp:
   a. Create sub-directories:
      ```
      cd corpus/extracted/{{ entity_ref_prefix }}/
      files=(*)
      batch_size=100
      for i in "${!files[@]}"; do
        batch=$((i / batch_size))
        mkdir -p "batch-$batch"
        cp "${files[$i]}" "batch-$batch/"
      done
      ```
   b. Spawn subagents per sub-directory with `spawn_agent`:
      - `label`: "Chunk batch {{ batch_index }}"
      - `message`: "Call `corpus_chunk` on
        `corpus/extracted/{{ entity_ref_prefix }}/batch-{{ batch_index }}/`
        with entity_ref_prefix `{{ entity_ref_prefix }}-batch-{{ batch_index }}`,
        max_tokens {{ max_tokens }}, overlap_tokens {{ overlap_tokens }},
        multi_tier {{ multi_tier }}. Output to
        `corpus/chunks/{{ entity_ref_prefix }}-batch-{{ batch_index }}.jsonl`.
        Report the chunk count."
   c. Follow the concurrency dispatch pattern: start at 4, add
      4 per round on success, cap at `max_concurrency`.
   d. After all subagents complete, merge batch outputs:
      ```
      cat corpus/chunks/{{ entity_ref_prefix }}-batch-*.jsonl > corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl
      ```
   e. Verify merged chunk count = sum of batch chunk counts.

3. Count the chunks produced:
   ```
   wc -l corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl
   ```

3. Verify chunk content quality — check average text length is in a
   reasonable range (not all tiny or all huge):
   ```
   python3 -c "
   import json
   lengths = []
   with open('corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl') as f:
       for line in f:
           d = json.loads(line)
           lengths.append(len(d.get('text','')))
   avg = sum(lengths) / len(lengths) if lengths else 0
   print(f'chunks={len(lengths)} avg_len={avg:.0f} min={min(lengths)} max={max(lengths)}')
   "
   ```

4. Verify source coverage — every extracted file must appear in the chunk
   output. A silent per-file drop here corrupted a real run (13 of 133
   sources vanished from the v2 chunk set with no error). Reconcile:
   ```
   python3 -c "
   import json, os
   sources = set()
   with open('corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl') as f:
       for line in f:
           sources.add(json.loads(line).get('source'))
   extracted = set(os.listdir('corpus/extracted/{{ entity_ref_prefix }}'))
   missing = extracted - sources
   print(f'{len(sources)} of {len(extracted)} sources chunked')
   for m in sorted(missing):
       print(f'  MISSING: {m}')
   "
   ```

5. **Quality gate**: chunk count > 0 AND chunk count in the expected
   range AND source coverage complete (distinct_sources ==
   extracted_count). Call `lisp_eval`:
   ```
   form: "(if (and (> chunk_count 0)
                    (>= chunk_count expected_min)
                    (<= chunk_count expected_max)
                    (= distinct_sources extracted_count))
            'pass
            (if (> chunk_count 0)
              'suspicious
              'fail))"
   ```
   Substitute actual values as literals. `expected_min` and `expected_max`
   come from the expected-range estimation (see Quality Gate Discipline).
   - `'pass`: proceed to Stage 3
   - `'suspicious`: log warning with actual vs expected range, investigate
     chunking parameters, but proceed if investigation confirms the count
     is reasonable for this corpus
   - `'fail`: halt with error: "chunking produced no output"
   - Missing sources (coverage failure) is a HALT regardless of the chunk
     count: re-chunk the missing files; never proceed with silent coverage
     loss. The chunk tool's own result reports `total_documents` — verify
     it equals the extracted file count at invocation time.

### Stage 3 — Embed the chunks

**This stage embeds ALL chunks.** The `corpus_embed` tool accepts
`tagged_jsonl` as an optional parameter — embeddings can be generated
without tags. Tags (Stage 4) are only needed for QA generation, not for
embedding or style exemplar building.

1. Call `corpus_embed` on the full chunks JSONL:
   - `chunks_jsonl`: `corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl`
   - `tagged_jsonl`: null (tags not yet available; embedding proceeds
     without them)
   - `db_path`: `{{ db_path }}`
   - `passphrase`: `{{ passphrase }}`
   - `model`: `{{ embedding_model }}`
   - `batch_size`: `{{ batch_size }}`

2. The tool result reports how many embeddings were generated. Record
   `embedding_count` from the result.

3. **Quality gate**: embedding_count == chunk_count (100% embedded).
   Call `lisp_eval`:
   ```
   form: "(let ((fail_rate (- 1.0 (/ embedding_count chunk_count))))
            (cond ((= embedding_count chunk_count) 'complete)
                  ((> fail_rate 0.10) 'halt)
                  (t 'partial)))"
   ```
   Substitute actual values as literals.
   - `'complete`: proceed to Stage 4
   - `'partial`: log warning with failure rate, investigate failed chunks.
     Proceed only if the failures are isolated and explainable.
   - `'halt`: halt with error summary listing failed chunks. Do NOT
     proceed to style exemplar or QA with incomplete embeddings.

### Stage 4 — Tag the chunks (batched, parallel)

**Parallelizable**: per batch. The `corpus_tag_chunks` tool makes LLM
calls per chunk and will time out on large inputs (observed: timeout on
382 chunks at concurrency 4). Split the chunks JSONL into batches of
`tag_batch_size` (default 200) and process batches concurrently via
`spawn_agent` subagents with the step-up ramp.

1. Split the chunks JSONL into batches:
   ```
   python3 -c "
   import json
   batch_size = {{ tag_batch_size }}
   with open('corpus/chunks/{{ entity_ref_prefix }}-chunks.jsonl') as f:
       lines = f.readlines()
   for i in range(0, len(lines), batch_size):
       batch = lines[i:i+batch_size]
       with open(f'corpus/chunks/{{ entity_ref_prefix }}-batch-{i//batch_size}.jsonl', 'w') as out:
           out.writelines(batch)
   print(f'Split {len(lines)} chunks into {(len(lines) + batch_size - 1) // batch_size} batches')
   "
   ```

2. **Sequential fallback** (if `parallel_subagents` is false): for each
   batch file, call `corpus_tag_chunks`:
   - `chunks_jsonl`: path to the batch file
   - `output`: path to the tagged batch output file
   - `concurrency`: `{{ concurrency }}`

   If a batch times out, reduce `concurrency` to 2 and retry. If it still
   times out, reduce the batch size to 100 and re-split. Do NOT skip
   batches — every chunk must be tagged or the quality gate fails.

3. **Parallel subagent dispatch** (if `parallel_subagents` is true):
   Spawn subagents to tag batches concurrently with the concurrency dispatch pattern:
   a. Start with 4 subagents. For each,
      call `spawn_agent`:
      - `label`: "Tag batch {{ batch_index }}"
      - `message`: "Call `corpus_tag_chunks` on
        `corpus/chunks/{{ entity_ref_prefix }}-batch-{{ batch_index }}.jsonl`
        with output
        `corpus/chunks/{{ entity_ref_prefix }}-batch-{{ batch_index }}-tagged.jsonl`
        and concurrency {{ concurrency }}. Report the tagged chunk count.
        If the tool times out, reduce concurrency to 2 and retry. If it
        still times out, report the error — do NOT skip the batch."
   b. On all agents succeeding, add 4 more agents for
      the next round.
   c. On any agent throttling (429/503) or erroring, hold at the current
      concurrency level for the next round.
   d. Cap at `max_concurrency` concurrent agents. The product
      (subagent count × per-subagent `concurrency`) should stay within
      `max_concurrency` to avoid exceeding the process-wide limiter.
   e. Collect all subagent outputs. Any batch that failed must be retried
      — either by the same subagent (using `session_id`) or a new one.

4. Concatenate all tagged batch outputs into the final tagged JSONL:
   ```
   cat corpus/chunks/{{ entity_ref_prefix }}-batch-*-tagged.jsonl > corpus/chunks/{{ entity_ref_prefix }}-tagged.jsonl
   ```

5. The tagging annotates each chunk with:
   - 5W1H interrogatory dimensions (Who, What, When, Where, Why, How)
   - Dublin Core metadata (creator, date, subject, source, type)
   - PKO process concepts (Procedure, Step, StepExecution)
   - FIBO/GOLEM domain concepts
   - Expertise level

6. Count the tagged chunks produced:
   ```
   wc -l corpus/chunks/{{ entity_ref_prefix }}-tagged.jsonl
   ```

7. **Quality gate**: tagged_count ≥ 90% of chunk_count. Call `lisp_eval`:
   ```
   form: "(let ((ratio (/ tagged_count chunk_count)))
            (cond ((>= ratio 0.90) 'pass)
                  ((= tagged_count 0) 'fail)
                  (t 'partial)))"
   ```
   Substitute actual values as literals.
   - `'pass`: proceed to Stage 5
   - `'partial`: log warning with coverage rate, investigate which batches
     failed, re-run failed batches. Proceed only after coverage reaches 90%.
   - `'fail`: halt with error: "tagging produced no output"

   **Note**: If `enable_qa` is false, Stage 4 can be skipped entirely —
   embeddings (Stage 3) and style exemplar (Stage 5) do not require tags.

### Stage 5 — Build style exemplar (optional)

**Gate**: runs only if `reference_author` is provided.

1. If `config_path` is provided, use it. Otherwise, note that a config
   YAML must exist or be generated for the style exemplar.

2. Call `corpus_compose` with the author's style config to build the
   style centroid and validate it against the corpus embeddings:
   - `prompt`: a brief description of the desired style
   - `author`: `{{ reference_author }}`
   - `db_path`: `{{ db_path }}`
   - `passphrase`: `{{ passphrase }}`
   - `config_path`: `{{ config_path }}` (if provided — loads the
     cognition YAML with the Jinja2 system prompt template and
     validation thresholds; omit for the generic inline config)

3. **Quality gate**: style centroid within validation thresholds.
   Call `lisp_eval`:
   ```
   form: "(let ((dist centroid_distance)
                 (ex exemplar_count))
            (and (<= dist 0.40)
                 (>= ex 100)
                 (<= ex 10000)))"
   ```
   Substitute actual values as literals.
   If false, log warning: "style centroid outside validation thresholds"
   but continue — QA generation can proceed without the style exemplar.

4. If `corpus_compose` fails, log the error and continue without
   the style exemplar. Do not halt — the QA pipeline does not depend on it.

### Stage 6 — Build QA prompts (optional)

**Gate**: runs only if `enable_qa` is true AND Stage 4 produced tagged chunks.

1. Call `corpus_build_prompts` on the tagged chunks:
   - `tagged_jsonl`: `corpus/chunks/{{ entity_ref_prefix }}-tagged.jsonl`
   - `output`: `corpus/qa/{{ entity_ref_prefix }}-prompts.jsonl`
   - `db_path`: `{{ db_path }}`
   - `passphrase`: `{{ passphrase }}`
   - `context_k`: `{{ context_k }}`
   - `prompts_per_chunk`: `{{ prompts_per_chunk }}`
   - `max_prompts`: `{{ max_prompts }}`
   - `type_distribution`: derived from `{{ bloom_levels }}`

2. Count the prompts produced:
   ```
   wc -l corpus/qa/{{ entity_ref_prefix }}-prompts.jsonl
   ```

3. **Quality gate**: prompt_count > 0. Call `lisp_eval`:
   ```
   form: "(if (> prompt_count 0) 'pass 'fail)"
   ```
   Substitute actual value as literal.
   If `'fail`, warning: "no QA prompts generated — check prompt generation"
   and skip Stages 7–9.

### Stage 7 — Generate QA pairs (optional, parallel)

**Gate**: runs only if `enable_qa` is true and Stage 6 produced prompts.

**Parallelizable**: per prompt batch. If the prompt count is large
(> 200), split prompts into batches and dispatch to subagents with the
step-up ramp.

1. **Sequential** (default for ≤ 200 prompts): call
   `corpus_generate_qa_batch`:
   - `prompts_jsonl`: `corpus/qa/{{ entity_ref_prefix }}-prompts.jsonl`
   - `output`: `corpus/qa/{{ entity_ref_prefix }}-generated.jsonl`
   - `concurrency`: `{{ concurrency }}`

2. **Parallel subagent dispatch** (if `parallel_subagents` is true and
   prompt count > 200): split the prompts JSONL into batches of ~200
   prompts each, then spawn subagents with the step-up ramp:
   a. Split prompts:
      ```
      split -l 200 corpus/qa/{{ entity_ref_prefix }}-prompts.jsonl corpus/qa/{{ entity_ref_prefix }}-prompt-batch-
      ```
   b. Spawn subagents per prompt batch with `spawn_agent`:
      - `label`: "QA gen batch {{ batch_index }}"
      - `message`: "Call `corpus_generate_qa_batch` on
        `corpus/qa/{{ entity_ref_prefix }}-prompt-batch-{{ batch_index }}`
        with output
        `corpus/qa/{{ entity_ref_prefix }}-gen-batch-{{ batch_index }}.jsonl`
        and concurrency {{ concurrency }}. Report the QA pair count."
   c. Follow the concurrency dispatch pattern: start at 4, add
      4 per round on success, cap at `max_concurrency`.
   d. Merge batch outputs:
      ```
      cat corpus/qa/{{ entity_ref_prefix }}-gen-batch-*.jsonl > corpus/qa/{{ entity_ref_prefix }}-generated.jsonl
      ```

3. Count the QA pairs generated:
   ```
   wc -l corpus/qa/{{ entity_ref_prefix }}-generated.jsonl
   ```

4. **Quality gate**: qa_count > 0. Call `lisp_eval`:
   ```
   form: "(if (> qa_count 0) 'pass 'fail)"
   ```
   Substitute actual value as literal.
   If `'fail`, warning: "no QA pairs generated" and skip Stages 8–9.

### Stage 8 — Ingest QA pairs (optional)

**Gate**: runs only if `enable_qa` is true and Stage 7 produced QA pairs.

1. Call `corpus_ingest_qa`:
   - `generated_jsonl`: `corpus/qa/{{ entity_ref_prefix }}-generated.jsonl`
   - `output`: `corpus/qa/{{ entity_ref_prefix }}-training.jsonl`
   - `db_path`: `{{ db_path }}`
   - `passphrase`: `{{ passphrase }}`
   - `dataset`: `{{ dataset_name }}`
   - `owner`: `{{ entity_ref_prefix }}`

2. The ingestion applies quality filters:
   - Exact-match dedup (case-insensitive on instruction)
   - Non-empty answer check
   - Answer length range check
   - Bloom level coverage check

3. Count the ingested QA pairs:
   ```
   wc -l corpus/qa/{{ entity_ref_prefix }}-training.jsonl
   ```

4. **Quality gate**: ingested_count > 0. Call `lisp_eval`:
   ```
   form: "(if (> ingested_count 0) 'pass 'filtered_all)"
   ```
   Substitute actual value as literal.
   If `'filtered_all`, warning: "all QA pairs filtered by quality checks".

### Stage 9 — Assemble training dataset (optional)

**Gate**: runs only if `enable_qa` is true and Stage 8 ingested QA pairs.

1. Call `training_assemble_dataset`:
   - `output_path`: `corpus/qa/{{ entity_ref_prefix }}-chatml.jsonl`
   - `dataset`: `{{ dataset_name }}`
   - `train_split`: `{{ train_split }}`
   - `db_path`: the corpus memory DB path used by `corpus_ingest_qa` in Stage 8
   - `passphrase`: the corpus DB passphrase (HKASK_DB_PASSPHRASE)

   WITHOUT `db_path` + `passphrase` the assembler queries the TRAINING
   server's own DB, which is empty for this corpus — Stage 8's QA pairs
   live in the corpus DB, and the assembler finds zero pairs. Always pass
   both fields.

2. Count the training examples:
   ```
   wc -l corpus/qa/{{ entity_ref_prefix }}-chatml.jsonl
   ```

3. **Quality gate**: example_count > 0. Call `lisp_eval`:
   ```
   form: "(if (> example_count 0) 'pass 'fail)"
   ```
   Substitute actual value as literal.
   If `'fail`, warning: "no training examples assembled".

### Stage 10 — Verify pipeline output

1. Call `corpus_query` with a test question to verify the vector index:
   - `query`: a question relevant to the corpus content
   - `top_k`: 5

2. Call the `grill-me` skill to interrogate the pipeline output:
   - **Recall**: How many chunks? Embeddings? QA pairs?
   - **Mechanism**: Does embedding count match chunk count? All chunks tagged?
   - **Rationale**: Why was chunk granularity set to {{ max_tokens }}? Appropriate?
   - **Edge cases**: HTML files converted correctly? PDFs skipped due to OCR?
     Were any batches dropped during tagging?
   - **Synthesis**: Does the style centroid match expected style? Would QA
     set produce a capable model?

3. **Final convergence check**: Call `lisp_eval` with all stage results:
   ```
   form: "(let ((conv_rate (/ extracted_count source_count))
                 (chunk_ok (> chunk_count 0))
                 (embed_complete (eq embedding_count chunk_count))
                 (tag_ok (>= (/ tagged_count chunk_count) 0.90))
                 (qa_ok (if enable_qa (> ingested_count 0) t))
                 (train_ok (if enable_qa (> example_count 0) t))
                 (query_ok (> query_result_count 0)))
            (and (>= conv_rate 0.80)
                 chunk_ok
                 embed_complete
                 (if enable_qa tag_ok t)
                 qa_ok
                 train_ok
                 query_ok))"
   ```
   Substitute actual values as literals. If true, the pipeline is complete.
   If false, log which criteria failed.

## Failure Modes

| Failure | Detection | Action |
|---------|-----------|--------|
| Empty corpus source folder | `find` returns 0 files | Error: "corpus_source is empty or does not exist" — HALT |
| No text-extractable files | All files are binary/corrupt | Error: "no readable text files in corpus_source" — HALT |
| OCR needed but unavailable | `corpus_is_complex` returns true, OCR fails | HALT with a failure report naming the file — never skip silently |
| Empty conversion output | `corpus_convert` produces 0 text files | Error: "no text extracted from any file" — HALT |
| Low conversion quality | any extraction fails the ≥ 50-word floor | Route failed files through OCR and re-audit — HALT until all pass; a failed extraction is never "not a valid source" |
| Zero chunks produced | `corpus_chunk` output has 0 lines | Error: "chunking produced no output" — HALT |
| Chunk count outside expected range | chunk_count < expected_min or > expected_max | Warning: investigate chunking parameters before proceeding |
| Tagging batch timeout | `corpus_tag_chunks` times out on a batch | Reduce concurrency to 2, retry. If still fails, reduce batch size to 100 and re-split. Do NOT skip batches. |
| Tagging partial failures | Some chunks lack annotations | Re-run failed batches. If coverage < 90% after re-runs, HALT. |
| Embedding failures | Per-chunk embedding errors | If >10% fail, halt with error summary. If ≤10%, proceed with warning. |
| Embedding count mismatch | embedding_count != chunk_count | Investigate. If >10% missing, HALT. Do NOT proceed to style exemplar with incomplete embeddings. |
| Style exemplar build failure | `corpus_compose` returns error | Log error, continue without style exemplar (QA can proceed) — non-blocking |
| Zero QA prompts | `corpus_build_prompts` produces 0 prompts | Warning, skip Stages 7–9 |
| Zero QA pairs generated | `corpus_generate_qa_batch` produces 0 pairs | Warning, skip Stages 8–9 |
| Zero QA pairs ingested | `corpus_ingest_qa` ingests 0 pairs | Warning: "all QA pairs filtered by quality checks" |
| Input degradation attempted | Agent tries to reduce input size to bypass a failure | HALT: "anti-degradation rule violated — fix root cause or halt, do not silently reduce input" |
| Subagent throttle | Inference provider returns 429/503 during parallel dispatch | Back off to last successful concurrency level, hold there for next round. Do not exceed `max_concurrency`. |
| Subagent depth exceeded | Subagent tries to spawn another subagent | `MAX_SUBAGENT_DEPTH` is 1 — subagents cannot spawn further subagents. Plan batch sizes so each subagent completes independently. |
| Subagent batch failure | A subagent's assigned batch fails or times out | Retry the batch with reduced concurrency (2) or reduced batch size (100). Do NOT skip. Use `session_id` to resume a failed subagent. |

## Convergence Criteria

The pipeline is complete when ALL of the following hold:

1. `corpus_convert` produced text files for ≥80% of source documents
2. `corpus_chunk` produced >0 chunks AND chunk count is in the expected range
3. `corpus_embed` embedded 100% of chunks (embedding_count == chunk_count)
4. (If QA enabled) `corpus_tag_chunks` tagged ≥90% of chunks
5. (If style exemplar enabled) `corpus_compose` produced a style centroid within validation thresholds (centroid_distance ≤ 0.40, exemplar_count 100–10000)
6. (If QA enabled) `corpus_ingest_qa` ingested >0 QA pairs
7. (If QA enabled) `training_assemble_dataset` produced >0 training examples
8. `corpus_query` returns relevant results for a test question
9. `lisp_eval` final convergence check (Stage 10, step 3) returns true

If any criterion fails, log the failure and halt — do not continue to
downstream stages with incomplete input. The only exceptions are:
- Stage 5 (style exemplar build) is non-blocking: a style exemplar failure does not
  halt the QA pipeline.
- Stage 4 (tagging) can be skipped if `enable_qa` is false, since
  embeddings and style exemplar do not require tags.

## Constraints

- The pipeline stages are sequential — each stage depends on the prior
  stage's output. Do not run stages in parallel. Within a stage, work
  units (files, batches) MAY be parallelized via `spawn_agent` subagents
  following the step-up ramp pattern.
- **Anti-degradation rule**: Never silently reduce input size to work
  around a stage failure. If a stage fails or times out, either fix the
  root cause (reduce batch size, reduce concurrency, fix the tool call)
  or halt with a failure report. Do NOT create subsets or samples to
  bypass the failure. A 33,000-chunk corpus reduced to 380 chunks is a
  98.8% data loss — the downstream stages would produce garbage.
- **Concurrency dispatch pattern**: when dispatching parallel subagents, start
  at 4 concurrent agents, add 4 per round on success, and cap at
  `max_concurrency` (default 96). On throttle (429/503), hold at the last
  successful level. The product of subagent count × per-subagent tool
  `concurrency` should stay within `max_concurrency` to avoid exceeding the
  process-wide limiter. `max_concurrency` lives in `KaskGeneralSettings` and
  is configurable via the settings UI (General page).
- **Subagent depth limit**: `MAX_SUBAGENT_DEPTH` is 1 — subagents spawned
  by the pipeline agent cannot spawn further subagents. Each subagent
  must complete its assigned work unit independently. Plan batch sizes
  accordingly.
- Stages 5–9 are optional and gated by their respective parameters.
  A missing style exemplar does not block QA generation.
- Stage 4 (tagging) can be skipped if `enable_qa` is false — embeddings
  (Stage 3) and style exemplar (Stage 5) do not require tags.
- Use `lisp_eval` for all deterministic invariant checks between stages.
  Do not eyeball counts. Substitute actual values as literals in the
  `form` parameter — the `env` parameter binding is unreliable.
- Every quality gate is a HARD gate. If a gate fails, HALT. Do not
  convert a failure into a warning and proceed.
- Passphrase resolution: use the `hkask_mcp_server::server::resolve_db_passphrase`
  helper if available (2-tier chain: ctx.credentials → env → `hkask-keystore` keychain).
  Do not inline re-implementations. A missing credential is an
  authorization failure, not a transient error.
- If any MCP tool call fails, call `curator_report_skill_use_issue` with:
  `skill_name: "build-corpus-pipeline"`, `tool_name: <failed tool>`,
  `error: <error message>`. Then either fix and retry, or halt — do not
  silently continue with degraded input.
- Tagging must be batched. The `corpus_tag_chunks` tool makes LLM calls
  per chunk and times out on large inputs. Default batch size: 200
  chunks. If a batch times out, reduce concurrency first, then batch
  size. Never skip batches.
- OCR concurrency is bounded by `ocr_concurrency` (default 4, overridable
  via `HKASK_OCR_CONCURRENCY`). When spawning subagents for OCR, do not
  exceed this limit — OCR runs in a subprocess with its own local
  semaphore, separate from the process-wide limiter.
- Do not fabricate corpus metadata. If a document's creator is unknown,
  tag the Dublin Core `creator` field as "unknown" rather than guessing.
