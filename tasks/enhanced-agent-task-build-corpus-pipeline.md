# Build-Corpus-Pipeline Skill: Design and Execute

## Purpose

Create a new kask skill called `build-corpus-pipeline` that ingests a folder of
source documents, converts them to text, chunks them, tags them with
ontology annotations, embeds them as vectors, optionally builds a persona
replica from a named author, and optionally generates QA pairs for LoRA
training. Then execute that skill on the corpus at
`/home/mdz-axolotl/Clones/Library/Researcher/` with the persona "John Brooks".

This is a two-phase task:

- **Phase A — Design and create the skill** (using the `create-skill` process)
- **Phase B — Execute the skill** on the specified corpus and persona

---

## Phase A — Create the `build-corpus-pipeline` Skill

### A1. Research ontological anchors

Search for the domain's process structure. The corpus pipeline is grounded in:

- **Text processing pipeline**: document conversion → segmentation → annotation
  → vectorization (standard NLP corpus construction)
- **PKO (Procedural Knowledge Ontology)**: the pipeline is a procedure with
  specification/execution separation — each stage has a specification (what it
  should produce) and an execution (the MCP tool call)
- **Bloom's Taxonomy**: for the QA generation stage, cognitive levels drive
  question difficulty distribution

Record these anchors and derive the PDCA shape:

```
Plan:  Stage 0 — Validate   → Check corpus source exists, is non-empty, has readable files
Plan:  Stage 1 — Convert    → Extract text from all documents in the source folder
Do:    Stage 2 — Chunk      → Segment text into passages at configurable token granularity
Do:    Stage 3 — Tag        → Annotate chunks with 5W1H + Dublin Core + PKO + FIBO/GOLEM dimensions
Do:    Stage 4 — Embed      → Generate ontology-anchored embedding vectors for all chunks
Check: Stage 5 — Persona    → (Optional) Build authorial replica from the embedded corpus
Do:    Stage 6 — QA Gen     → (Optional) Generate QA pairs from tagged chunks
Do:    Stage 7 — Ingest QA  → (Optional) Parse, quality-filter, dedup, write training JSONL
Act:    Stage 8 — Assemble  → (Optional) Assemble QA pairs into ChatML training dataset
```

### A2. Describe the skill specification

- **Name**: `build-corpus-pipeline`
- **Purpose**: Ingest a folder of source documents through a complete
  text-processing pipeline (convert → chunk → tag → embed) with optional
  persona replica and QA pair generation for LoRA training.
- **Inputs** (all parameterized, none hardcoded):
  - `corpus_source` (string, required): absolute path to a folder containing
    source documents. The skill expects a folder as the source to work from.
  - `reference_persona` (string, optional): author name for persona replica
    construction (e.g., "John Brooks"). When provided, Stage 5 runs
    `corpus_build_persona` to compute a style centroid.
  - `entity_ref_prefix` (string, required): prefix for entity references in
    chunk IDs (e.g., "john-brooks"). Used across chunk, tag, embed, and QA stages.
  - `db_path` (string, required): path to the embedding database file.
  - `passphrase` (string, optional): database passphrase for encrypted stores.
  - `generate_qa` (boolean, default: false): whether to run QA generation
    stages (6-8). When false, the pipeline stops after embedding (Stage 4)
    or persona (Stage 5).
  - `bloom_levels` (array of strings, default: ["Remember", "Understand",
    "Apply", "Analyze"]): Bloom's Taxonomy levels for QA generation.
  - `prompts_per_chunk` (integer, default: 3): number of QA prompts to
    generate per chunk.
  - `max_prompts` (integer, optional): cap on total QA prompts generated.
  - `max_tokens` (integer, default: 512): target token count per chunk.
  - `overlap_tokens` (integer, default: 64): token overlap between adjacent
    chunks.
  - `chunk_strategy` (string, default: "single-tier"): "single-tier" or
    "multi-tier" (coarse/medium/fine granularity levels).
  - `dataset_name` (string, default: "corpus-qa"): label for the assembled
    training dataset.
  - `strip_gutenberg` (boolean, default: true): strip Project Gutenberg
    headers/footers during chunking.

- **Composed skills** (evaluate each for fit before including):

  | Candidate skill | Fit question | Include if... |
  |-----------------|-------------|---------------|
  | `task-breakdown` | Does the pipeline need decomposition into verifiable slices? | Yes — the 8-stage pipeline is naturally decomposable; use for Phase B execution planning |
  | `essentialist` | Are there stages that don't earn their place? | Apply the deletion test to each stage: would removing it change the pipeline's output? Stages 5-8 are optional by design, so this is already satisfied |
  | `grill-me` | Should the skill self-test its output? | Yes — use as a post-pipeline verification step: can the embedded corpus answer questions about the source material? |
  | `pragmatic-semantics` | Should claims about corpus quality be classified by certainty? | Yes — use during QA quality checks to distinguish verified-fact QA from inferred QA |
  | `metacognition` | Should the skill self-assess pipeline progress? | Optional — the pipeline is sequential, not iterative; metacognition adds value only if stages loop |
  | `hypothesis-framer` | Should QA quality be hypothesis-tested? | Optional — could frame "the QA set captures the corpus's key concepts" as a testable hypothesis, but this is an addition, not a core requirement |
  | `idiomatic-lisp` | Is deterministic computation needed? | Yes — use `lisp_eval` for invariant checks: chunk count > 0, all chunks have embeddings, QA pair count matches expected per-chunk rate |
  | `capabilities-reasoner` | Should the skill assess its own capabilities? | No — this is the structural inspiration, not a composition delegate. Its PDCA pattern (Register → Elicit → Evaluate → Reason → Report → Converge) informs the pipeline shape, but the skill does not reason about ML capabilities |

  **Recommended composition**: `task-breakdown` (Phase B planning),
  `grill-me` (post-pipeline verification), `idiomatic-lisp` (deterministic
  invariant checks via `lisp_eval`), `pragmatic-semantics` (QA quality
  classification). Defer `metacognition`, `hypothesis-framer`, and
  `capabilities-reasoner` as composition delegates — they are structural
  inspiration, not runtime delegates.

- **MCP tools** (corpus server):
  - `corpus_is_complex` — Stage 0: check whether PDFs need OCR before conversion
  - `corpus_convert` — Stage 1: extract text from each document
  - `corpus_cache_work` — Stage 1: cache extracted text for reuse
  - `corpus_chunk` — Stage 2: segment text into passages
  - `corpus_tag_chunks` — Stage 3: annotate chunks with ontology dimensions
  - `corpus_embed` — Stage 4: generate embedding vectors
  - `corpus_build_persona` — Stage 5: build authorial replica (optional)
  - `corpus_build_prompts` — Stage 6: build QA generation prompts from tagged chunks (optional)
  - `corpus_generate_qa_batch` — Stage 6: batch-generate QA pairs (optional)
  - `corpus_ingest_qa` — Stage 7: parse, filter, dedup, write training JSONL (optional)
  - `corpus_dedup_chunks` — optional: deduplicate chunks by semantic similarity
  - `corpus_consolidate_chunks` — optional: consolidate related chunks via LLM synthesis
  - `corpus_query` — verification: query the vector index to confirm embeddings work

- **MCP tools** (training server):
  - `training_assemble_dataset` — Stage 8: assemble QA pairs into ChatML JSONL (optional)

- **Agent tools**:
  - `lisp_eval` — invariant checks: chunk count, embedding completeness, QA count
  - `render_template` — structured prompt scaffolding for QA generation prompts
  - `read_file` — read .j2 template specifications
  - `skill` — compose with `task-breakdown`, `grill-me`, etc.

### A3. Scaffold the skill artifacts

Generate:

1. **SKILL.md** at `.agents/skills/build-corpus-pipeline/SKILL.md` with:
   - Frontmatter: `name: build-corpus-pipeline`, `description: ...`
   - "When to Use" / "When NOT to Use" sections
   - "Inputs" section listing all parameterized inputs with defaults
   - "Instructions" section with numbered stages (0-8), each specifying:
     - What MCP tool to call and with what inputs
     - What to do with the result (feed to next stage, check invariant)
     - Termination criteria for that stage
   - "Failure Modes" section (see below)
   - "Composed Skills" table
   - "Constraints" section
   - "Convergence" pattern using `lisp_eval`

2. **.j2 templates** at `kask/registry/templates/build-corpus-pipeline/`:
   - `corpus-validate.j2` — Stage 0: validate source folder, enumerate files,
     check readability, flag OCR-needed PDFs
   - `corpus-qa-generate.j2` — Stage 6: QA generation prompt scaffold with
     Bloom level distribution and per-chunk context
   - `corpus-verify.j2` — post-pipeline: verification questions to test
     embedding quality via `corpus_query`

### A4. Skill PDCA shape (derived from capabilities-reasoner pattern)

The capabilities-reasoner skill's PDCA loop (Register → Elicit → Evaluate →
Reason → Report → Converge) maps to the corpus pipeline as:

```
Plan:  Register → Stage 0 (Validate): enumerate and classify source files
Do:    Elicit  → Stages 1-4 (Convert → Chunk → Tag → Embed): extract and represent
Check: Evaluate → Stage 5 (Persona): measure style centroid against reference
Act:    Reason  → Stages 6-8 (QA Gen → Ingest → Assemble): produce training artifacts
Check: Converge → Verification: lisp_eval invariant checks + corpus_query probe
```

**Key difference from capabilities-reasoner**: the corpus pipeline is
predominantly sequential, not iterative. The convergence check is a
post-pipeline gate, not a re-entry loop. If `lisp_eval` finds 0 chunks or
0 embeddings, the skill reports the failure — it does not re-run the
pipeline.

### A5. Failure modes (per stage)

| Stage | Failure mode | Detection | Action |
|-------|-------------|-----------|--------|
| 0 | Source folder doesn't exist | `ls` returns error | Report error, abort |
| 0 | Folder is empty | file count = 0 | Report error, abort |
| 0 | Folder has only non-text files (images) | no convertible files detected | Report warning, abort |
| 1 | PDF needs OCR but OCR not available | `corpus_is_complex` returns true | Log warning, attempt conversion anyway, report any failures |
| 1 | Conversion produces empty text | extracted text is empty string | Log warning per file, continue with remaining files |
| 2 | Chunking produces 0 chunks | `lisp_eval` count check | Report error, abort |
| 3 | Tagging fails for some chunks | partial tag output | Log per-chunk warning, continue |
| 4 | Embedding fails | `corpus_embed` returns error | Report error, abort |
| 5 | Persona build fails | `corpus_build_persona` returns error | Log warning, continue (persona is optional) |
| 6 | QA generation produces 0 pairs | `lisp_eval` count check | Report warning, skip Stage 7-8 |
| 7 | QA ingest fails | `corpus_ingest_qa` returns error | Report error, skip Stage 8 |

### A6. Convergence criteria (lisp_eval checks)

After Stage 4 (Embed):
```
form: "(let ((chunks (length (assoc \"chunks\" embed_result))))
        (if (> chunks 0) chunks 0))"
```
If 0 → abort with error.

After Stage 7 (QA Ingest):
```
form: "(let ((qa_count (assoc \"qa_count\" ingest_result)))
        (if (and qa_count (> qa_count 0)) qa_count 0))"
```
If 0 → warn, skip Stage 8.

### A7. Validate the skill

Call the `skill` tool:
  name: "skill-maintenance"
  task: "validate skill build-corpus-pipeline"

Fix any validation failures, then proceed to Phase B.

---

## Phase B — Execute the Skill on the John Brooks Corpus

### B1. Inputs

- `corpus_source`: `/home/mdz-axolotl/Clones/Library/Researcher/`
  (contains ~100+ files: HTML articles, PDFs, TXT files — including
  `onceingolconda_johnbrooks.pdf`)
- `reference_persona`: "John Brooks"
- `entity_ref_prefix`: "john-brooks"
- `db_path`: (determine from kask data directory or specify explicitly)
- `passphrase`: (resolve from `HKASK_DB_PASSPHRASE` credential chain)
- `generate_qa`: true (user wants QA pairs for LoRA training)
- `bloom_levels`: ["Remember", "Understand", "Apply", "Analyze", "Evaluate"]
- `prompts_per_chunk`: 3
- `max_tokens`: 512
- `overlap_tokens`: 64
- `dataset_name`: "john-brooks-corpus-qa"

### B2. Execution plan

Use `task-breakdown` to decompose the execution into verifiable slices:

1. **Slice 1 — Validate**: Run Stage 0. Confirm folder exists, enumerate
   files, check which PDFs need OCR via `corpus_is_complex`.

2. **Slice 2 — Convert**: Run Stage 1. Call `corpus_convert` with
   `path` = corpus_source, processing all files. Cache each extracted work
   via `corpus_cache_work`.

3. **Slice 3 — Chunk**: Run Stage 2. Call `corpus_chunk` with the converted
   text, `entity_ref_prefix` = "john-brooks", `max_tokens` = 512,
   `overlap_tokens` = 64.

4. **Slice 4 — Tag**: Run Stage 3. Call `corpus_tag_chunks` with the chunked
   JSONL output.

5. **Slice 5 — Embed**: Run Stage 4. Call `corpus_embed` with the tagged
   JSONL and `db_path`. Run `lisp_eval` convergence check (chunk count > 0).

6. **Slice 6 — Persona**: Run Stage 5. Call `corpus_build_persona` with
   config referencing the embedded corpus and persona "John Brooks".

7. **Slice 7 — QA Generate**: Run Stage 6. Call `corpus_build_prompts` to
   build QA prompts from tagged chunks, then `corpus_generate_qa_batch` to
   generate QA pairs with the specified Bloom levels and prompts_per_chunk.

8. **Slice 8 — QA Ingest**: Run Stage 7. Call `corpus_ingest_qa` to parse,
   quality-filter, dedup, and write training JSONL.

9. **Slice 9 — Assemble**: Run Stage 8. Call `training_assemble_dataset`
   to assemble QA pairs into a ChatML JSONL training dataset.

10. **Slice 10 — Verify**: Run `corpus_query` with a test query about
    John Brooks's writing style. Run `grill-me` Recall + Mechanism probe:
    can the embedded corpus answer questions about the source material?

### B3. Termination

The pipeline is complete when:
- All convertible files have been converted (Stage 1)
- Chunks have been generated (Stage 2, count > 0 verified by `lisp_eval`)
- Chunks have been tagged (Stage 3)
- Embeddings have been generated (Stage 4, verified by `lisp_eval`)
- Persona replica has been built (Stage 5, if `reference_persona` provided)
- QA pairs have been generated and ingested (Stages 6-7, if `generate_qa` = true)
- Training dataset has been assembled (Stage 8, if `generate_qa` = true)
- Verification query returns relevant passages (Slice 10)

### B4. Output

- Embedded corpus database at `db_path`
- Persona replica for "John Brooks" (style centroid)
- Training JSONL dataset at the path specified by `training_assemble_dataset`
- Verification report from `corpus_query` and `grill-me` probe

---

## Research Questions (answer before Phase A.3 scaffolding)

1. **Chunking strategy**: Should the pipeline use multi-tier chunking
   (coarse/medium/fine) for the Researcher corpus, given the mix of short
   HTML articles (~5-27 KB) and long PDFs (up to 38 MB)? Single-tier at
   512 tokens may be sufficient for the HTML files but suboptimal for
   long PDFs. Consider `multi-tier: true` with coarse=2048, medium=512,
   fine=128.

2. **OCR handling**: Several PDFs in the corpus are large (15-38 MB) and
   may be scanned. `corpus_is_complex` should be called on each PDF before
   conversion. If OCR is needed and `force_ocr` is not set, the conversion
   may produce poor text. Decide: set `force_ocr: true` for complex PDFs,
   or skip them and process only the natively-digital files?

3. **Dedup and consolidation**: The corpus has thematic overlap (e.g.,
   multiple files about Toyota Kata, systems thinking, Superforecasting).
   Should `corpus_dedup_chunks` and/or `corpus_consolidate_chunks` run
   between Stage 3 (Tag) and Stage 4 (Embed) to reduce redundancy?
   Default: run `corpus_dedup_chunks` with threshold 0.85 after tagging,
   skip consolidation (it's expensive and the corpus is diverse enough).

4. **QA quality**: How should QA quality be measured? Options:
   - `corpus_ingest_qa` has built-in quality-filtering and exact-match dedup
   - `pragmatic-semantics` can classify QA pairs by certainty level
   - `grill-me` can test whether QA pairs probe Recall vs Mechanism vs Rationale
   Default: rely on `corpus_ingest_qa`'s built-in filters, add
   `pragmatic-semantics` classification as a post-ingest check.

5. **Training dataset format**: `training_assemble_dataset` produces ChatML
   JSONL. Is this the correct format for the target LoRA training job?
   Check `training_validate_config` for harness compatibility (axolotl vs trl)
   before submitting.

---

## Constraints

- The skill must not hardcode the corpus source path, persona name, or any
  parameter — all inputs are parameterized.
- The pipeline is sequential (not iterative) — each stage runs once, in order.
  The only loop is the convergence check, which is a gate, not a re-entry.
- `lisp_eval` is used for deterministic invariant checks only (counts,
  completeness), not for pipeline orchestration.
- The `capabilities-reasoner` PDCA pattern is structural inspiration, not a
  composition delegate. Do not call `capabilities-reasoner` as a skill during
  pipeline execution.
- The `capabilities-researcher.yaml` manifest no longer exists in the codebase
  (referenced only in `kask/security/regressions/RR-0019.yaml`). Do not attempt
  to follow it — derive the pipeline structure from the capabilities-reasoner
  skill's PDCA pattern and the corpus MCP tool surface instead.
- Follow the `create-skill` process (Research → Describe → Scaffold → Validate
  → Converge) for Phase A.
- Follow the `task-breakdown` process for Phase B execution planning.
- Every MCP tool failure must be logged with the tool name and error message
  before continuing or aborting.
- The `render_template` tool may fail with a path canonicalization error for
  relative template refs. If this happens, read the template via `read_file`
  instead and follow its structure manually.
