---
name: build-corpus-pipeline
description: "Build a source-complete corpus through convert → chunk → embed → classify, with optional style centroids and evidence-carrying QA for operator-approved LoRA training. Gate every stage on identity, actual outcomes, source coverage and semantic review."
---

# Build Corpus Pipeline

Build a functioning, verifiable corpus pipeline, not a hand-produced substitute
for a failed stage. Use one canonical corpus and current schemas throughout.
A tool return, a nonempty file, and a completed capability are different claims.

## Anchors and contract

- **PKO** separates a procedure from its execution: record each stage's inputs,
  outputs, measured outcomes and unresolved failures.
- **Dublin Core / PROV-O** anchor source identity and derivation. A citation is
  `QaEvidence { chunk_ref, source, quote }`, not a semantic verdict.
- **Bloom's taxonomy** informs QA difficulty. The engine's five labels are
  `factual`, `conceptual`, `analyze`, `evaluate`, `create`; labels alone do not
  establish actual cognitive difficulty.

The [corpus README](../../../kask/mcp-servers/hkask-mcp-corpus/README.md) specifies
wire contracts; the [tool reference](../../../kask/docs/reference/mcp-servers/corpus.md)
lists parameters. Enforcement anchors (paths relative to the repository root):

| Invariant | Current enforcement |
|---|---|
| One bounded word-window engine, real overlap including explicit zero | `kask/crates/hkask-memory/src/text_chunking.rs:123`; `kask/mcp-servers/hkask-mcp-corpus/src/helpers.rs:373` |
| Required terminal classification and identity-correlated tags | `kask/crates/hkask-types/src/corpus.rs:228`; `kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs:66` |
| Full-source stored context and partition-stable prompt IDs | `kask/mcp-servers/hkask-mcp-corpus/src/services/prompt_builder.rs:119`; `kask/crates/hkask-types/src/corpus.rs:38` |
| Structured evidence across generation and ingestion | `kask/mcp-servers/hkask-mcp-corpus/src/services/qa_pipeline.rs:19`; `kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus/qa_parsing.rs:53` |
| Exclusive QA output ownership and typed retries | `kask/mcp-servers/hkask-mcp-corpus/src/services/qa_batch.rs:99`; `kask/mcp-servers/hkask-mcp-corpus/src/batch.rs:73` |

## Inputs and preflight

Resolve real paths and credentials before calling tools. Keep sources, accepted
extractions, stage outputs and scratch controls separate. Never put probes,
duplicate sources or synthetic fixtures in an extraction input directory.

| Input | Contract |
|---|---|
| `corpus_source` | Retained source directory; inventory every source before processing |
| `entity_ref_prefix` | One namespace; use `style:{author}` for an author corpus, with the exact same author identifier in compose/centroid calls |
| `db_path`, `passphrase` | One corpus DB; resolve the current `HKASK_DB_PASSPHRASE` from authorized credentials, never invent or print it |
| `max_tokens` | Optional approximate size target; absent uses `HKASK_CHUNK_MAX_TOKENS` / shared settings (code default 256), not a model tokenizer |
| `overlap_tokens` | Default **64**, yielding **48 words**; explicit `0` disables repetition |
| `multi_tier` | False for the directory QA substrate; per-file/text retrieval can request coarse/medium/fine tiers |
| embedding `model`, `batch_size` | Use the configured embedding model and tool's batching; do not substitute a training or chat model |
| `tag_batch_size` | **10** chunks per tagging inference call by default; distinct from the number of rows in a file partition |
| `concurrency` | Bound tool concurrency to available capacity; AIMD starts at up to 2, grows by 1, halves on capacity failure |
| `enable_qa` | Select before work; if true, QA stages are required, not silently skipped on failure |
| `reference_author`, `config_path` | Optional style branch; exact author identity and current cognition YAML |
| `prompts_per_chunk` | Skill/run setting **2**; pass explicitly because the tool default is **5** |
| `context_k` | Tool default **3**; `0` explicitly disables KNN, never a recovery from failed context reads |
| `type_distribution` | Five nonnegative integer weights in canonical label order; default `1,1,1,1,1` |
| `max_prompts` | Explicitly `classified_count × prompts_per_chunk`, or `0` for all; not a small fixed cap |
| `dataset`, `owner`, `train_split` | Explicit dataset/owner identity and agreed training split; never inherit another corpus's defaults |

QA requires a dedicated non-thinking generator: explicit tool `model`, otherwise
Settings → Kask → Models → **QA Generation Model** (`kask.models.qa_generation_model`,
injected as `HKASK_QA_GENERATION_MODEL`). The setting defaults empty. Missing,
malformed or unresolved configuration must fail visibly. No chat, classifier,
`HKASK_QA_MODEL`, or training-base fallback. Tagging independently requires the
configured classifier; OCR requires the configured image-capable OCR model.
The training base model and LoRA configuration still require operator approval.

Use tool concurrency by default. Additional agent threads require explicit
operator approval and disjoint stage output paths. File partitioning changes
scheduling only: preserve every row, source, entity reference and prompt ID.
Never copy source files into nested input directories to simulate partitioning.
Do not fan out DB ingestion with independently restarting retained-row indices.

## Stage 0 — Establish scope and rebuild plan

1. Inventory retained originals and accepted extractions, including their exact
   paths, source identities and per-source word counts. A bad extraction is a
   processing failure, not permission to remove its source from scope.
2. Record the target source set, chunk/overlap parameters, two prompts per chunk,
   desired QA type mix, semantic quality criteria and dataset-size requirement.
3. Confirm model configuration, writable output paths, DB identity and credential
   access. Do not bypass containment with terminal conversions or manual QA.
4. Classify each stage as required or explicitly not requested. A requested style
   branch may fail without blocking independent QA, but the whole goal remains
   incomplete until the branch succeeds or the operator changes the scope.

### One Brooks corpus from retained sources

This is an execution procedure, **not a claim that a rebuild has run**.
Preserve **the operator's current approved source set and two prompts per chunk**.
The 2026-09-11 ruling replaces the former 125-source set with the **120 files in
`Clones/Library/Researcher`**; compare identities/content, not just counts. Preserve
removed originals and valid extractions outside the active corpus input set.
The historical **27,518 chunks / 55,036 prompts** must be **remeasured under the
real-overlap contract and current source set**; do not force those totals or lower
coverage to reproduce them.

Before a rebuild, inspect and explicitly identify the obsolete Brooks DB and
derived chunks, tags, prompts, generated QA, training exports and centroid
artifacts. Verify ownership, stopped workers, dependencies and that retained
sources/extractions suffice for reconstruction. Delete only those verified
obsolete DB/derived artifacts, including coupled DB sidecars when safe with the
DB closed. Do not retain a second Brooks DB, numbered corpus variant, backup or
compatibility dataset. Do not delete originals or accepted source extractions.
Ambiguous ownership or insufficient retained sources blocks deletion.

Rebuild the single canonical corpus from Stage 2 when accepted extractions are
complete, otherwise from Stage 1. Clear stale in-memory retrieval before selecting
the rebuilt DB. Recompute embeddings, classifications, prompts, QA and centroids;
do not relabel stale rows as current. Cleanup and rebuild are explicit data
operations: a docs-only request authorizes none of them.

## Stage 1 — Convert and audit extraction

Use `corpus_convert(path, output)` for the source set. Directory conversion
requires an output directory, resumes only quality-passing outputs, and places
OCR-derived text in `{output}-ocr-staging`. File mode writes its requested output;
that write is not a quality acceptance verdict. Staged OCR has a `.report.json`
companion with its complete conversion result; preserve it for review. Directory
responses expose `document_reports` and `verification_failed` separately from
conversion/I/O `failed`. Check both: a failed page verdict can coexist with a
successfully written staged file. Resume requires a matching report; missing or
mismatched evidence blocks without automatic re-OCR. Do not fabricate a report
for old staged text; diagnose it and explicitly regenerate only when needed.

For PDFs, `corpus_is_complex(path, summary=true)` provides cheap routing evidence.
Preflight required OCR with a small `target_pages` slice and `force_ocr=true`, then
inspect the report before bulk work. Missing configuration, endpoint errors,
`error_count`, `quality_failed_pages` or breaker-open results block expansion.
Source-confirmed blank pages can be recorded as such; never infer that every empty
page is benign. `include_structure=true` is only needed for the block view.

Audit every extraction against the original: source coverage, word counts,
legibility, repetition, script/language consistency and deterministic OCR quality
results. The directory resume floor is 50 words plus its quality gates; it is not
semantic proof. Merge staged OCR into accepted extractions only after review.
For deliberate replacement of a passing extraction, verify it is derived and
reproducible from a retained original before deletion/reconversion; no backup
corpus. Audit failures stay blocked, not promoted by a copy operation.

**Gate:** every inventoried source is represented exactly once by accepted text;
no unresolved failed extraction or duplicate/probe artifact remains. Bash and
`jq` can measure files/JSON; do not introduce Python tooling.

## Stage 2 — Chunk once, measure coverage and overlap

Call `corpus_chunk` with `input_dir` set to the accepted `.txt` directory,
explicit `output`, `entity_ref_prefix`, selected `max_tokens`, `overlap_tokens`,
`multi_tier=false`, and `index=false` when Stage 3 will persist embeddings.
Directory mode enumerates immediate `.txt` children, not a recursive file tree.
Every child is containment-checked before reading; broken/escaping symlinks fail.

All chunk modes use the shared structural/sentence word-window engine. Effective
maximum and overlap are `floor(tokens / 1.33)` whitespace words. Each passage is
bounded, contributes new source words, and positive overlap repeats the preceding
suffix exactly; zero follows the same engine with no repetition. A small final
passage is valid. `max_tokens` must yield at least one word, and positive overlap
must yield at least one word and remain smaller than the maximum in both units.

Directory source ID components are reversible `utf8-` plus fixed-width hex of
UTF-8 filename bytes, not punctuation substitution. Keep the original `source`
string alongside the encoded `entity_ref`. For file/text calls, use a unique
namespace per source; do not invent IDs by replacing punctuation.

**Gate:** reconcile parsed records with `total_chunks`, unique references,
`total_documents`, the complete source set and empty `zero_chunk_files`. Measure
word bounds and adjacent overlap per source. Estimate counts using effective
stride `max_words - overlap_words`, then measure actual output: structural ends
and final fragments affect counts. A JSONL line count alone cannot verify any of
these properties. Never change overlap or remove sources to hit an old count.

## Stage 3 — Persist all embeddings and provenance

Call `corpus_embed(chunks_jsonl, tagged_jsonl=null, db_path, passphrase, model,
batch_size)` for every chunk in the single corpus. Tags are optional here.
The stored original `passage_text` and text h_mem `ontology.dc_source` support
later context retrieval; vectors alone are insufficient.

**Gate:** successful embeddings equal measured chunk count; reconcile all failed
and cancelled rows, with none unresolved. Read/store errors are not zero counts.
Do not accept a small loss merely because a tool's degraded threshold is 10%.
A bad DB credential is not an excuse to leave a second DB alongside the first.

## Stage 4 — Classify, then count actual outcomes

Call `corpus_tag_chunks(chunks_jsonl, output, concurrency, tag_batch_size=10,
dry_run=false)`. Split input JSONL into bounded disjoint files if necessary;
keep original identities and verify the merged identity set equals the input.

Each output `TaggedChunk` requires `classification`:

```json
{"status":"classified"}
```

or `{"status":"failed","reason":"the actual failure"}` or
`{"status":"unverified"}`. Missing status is invalid, not implicit success.
A classifier response is an array keyed by exact `chunk_ref`; a singleton object
is allowed only for one input. Missing, duplicate, unknown or malformed entries
reject the entire affected batch. There is no positional or string fallback.
Failure annotations and deterministic method signals do not make a row classified.
Synthesized consolidation text is `unverified` and must itself be classified.

**Gate:** count `.classification.status == "classified"`, reconcile it with
returned `tagged`, `failed` and `total_chunks`, and require all chunk identities
classified. Failed/unverified rows block QA; output line count and annotation
presence are not substitutes. Re-run only after diagnosing the failure and
replace affected terminal records by identity, never duplicate them in a merge.
If QA was explicitly disabled, tagging may be marked not requested.

## Stage 5 — Optional style centroid and measured composition

Call `corpus_centroid(author, db_path, passphrase)` after complete embedding.
It selects `style:{author}:` and stores `style:{author}:centroid`. Optional
`refs_file` selects existing newline-delimited references without copying vectors;
optional `dimension` stores `style:{author}:{dimension}:centroid`. Empty or missing
eligible selections fail; duplicates count once. The shared
`hkask_types::corpus::is_corpus_passage_ref` excludes empty, `:rule:` and all
suffix-`:centroid` refs from centroid sources, compose exemplars and prompt context.
Recomputation replaces the destination, not a new centroid version.

Use `corpus_compose` with the same author/DB and current cognition `config_path`.
The YAML places `centroid_entity_ref` inside `embedding`. Compare measured
`centroid_distance`/`style_passed` to the config thresholds; calibrate thresholds
against real corpus text, not a universally hardcoded distance or exemplar count.
`centroid_missing=true` with null distance/pass is unvalidated, not a pass.
`no_validate=true` explicitly skips validation. Rewrite always targets its
requested dimension (default `composite`), even with a literary config.

**Gate:** requested centroid exists and the measured style criterion is met,
or report the branch blocked. Independent QA may continue, not erase the blocker.

## Stage 6 — Build complete-source QA prompts

Call `corpus_build_prompts` with `tagged_jsonl`, `output`, the single `db_path`
and `passphrase`, explicit `prefix` matching the chunk namespace, `context_k`,
`prompts_per_chunk=2`, `type_distribution`, and `max_prompts=classified_count*2`
(or `0` for all). Pass `ontology_bloom_overrides` only when deliberately selected.
No inference occurs in this stage.

The builder rejects nonclassified/duplicate refs, blank source/text, invalid
salience, prefix mismatches and absent required templates. With KNN enabled it
reads stored passages across the entire namespace, groups by original source,
and selects nearest neighbors from the primary source even across input file
splits. Context includes actual text, `chunk_ref` and `source`, not a display
preview. Missing/ambiguous provenance, missing text, invalid vectors, primary
text/source disagreement or failed DB reads are errors. `context_k=0` explicitly
disables KNN only; DB/knowledge-graph reads still occur and must succeed.

Builder IDs are deterministic `qa-<UUIDv5>` from source, chunk ref, QA type and
within-type ordinal. Preserve them when splitting/merging; do not renumber files.
The eight required `PreparedQaPrompt` fields are `prompt_id`, `chunk_ref`,
`source`, `concepts`, `salience`, `qa_type`, `system`, `user`. Unknown fields fail.

**Gate:** `prompts_written == classified_count*2`, unique prompt IDs, full primary
source/chunk coverage, and expected `context_enabled`, `context_scope`,
`stored_passages`, `context_links`. `context_links` counts neighbors per processed
chunk, not per prompt. The type rotation restarts for each chunk: at two prompts
and equal weights it selects factual/conceptual, not all five labels. Report the
actual distribution and any gap against the requested mix; never claim even
five-level coverage or increase prompts per chunk without operator approval.

## Stage 7 — Generate QA with owned outputs

Start with an operator-authorized bounded pilot, keeping the full source/prompt
inventory as the target. Run Stage 8 on that pilot before expanding to full
production; a pilot is evidence for a gate, never a reduced completion scope.
Call `corpus_generate_qa_batch(prompts_jsonl, output, concurrency, model)`.
Preflight validates the whole prepared file/model before creating output.
Input/output aliases (including symlink and hard-link aliases) are rejected.
A process-wide lease owns the canonical output across service instances and both
transports. It remains owned until workers are destroyed, not merely until abort
is requested. Never race a timed-out/cancelled call with a replacement writer.

Synchronous inference retries only typed Connection/Overloaded/Timeout failures,
at most **3 total attempts**, with 2s/4s backoff. Configuration/auth/model failures
and rejected QA are not retried. Provider-batch submission is not retried because
remote acceptance can be unknown. `:batch` selects that transport; prepared
`system`/`user` messages and prompt IDs remain unchanged.

Every pair must include nonblank `question`, `answer`, requested `bloom_level`
and `evidence_quotes`, an array of structured `QaEvidence` objects. The array may
be empty (no citation), never absent, a string array or numeric passage citations.
Example model response:

```json
{"qa_pairs":[{"question":"What is the delay?","answer":"72 hours","bloom_level":"factual","evidence_quotes":[{"chunk_ref":"corpus:delay:0","source":"delay.txt","quote":"The delay is 72 hours."}]}]}
```

Generated envelopes retain `prompt_id`, primary `chunk_ref`/`source`, `qa_type`,
`salience`, `response.{instruction,output,type,concepts,evidence_quotes}`,
`provenance` and `tokens_used`. Each citation has its own identity, including
context citations. Matching quotation bytes does not validate answer synthesis.
Manual single/cross-reference `corpus_generate_qa` calls with only text have no
source identity: they request empty evidence, not invented sources. They are not
a cited replacement for this prepared pipeline or a failed stage.

**Gate:** reconcile `prompts_total = prompts_succeeded + prompts_failed` against
all prepared IDs, require no unresolved failed prompts, and separately measure
`qa_rows_written`. A prompt may yield multiple pairs; row count is not prompt
coverage. Failure rows have identity plus `error`, never training data.
Writes/flushes can fail and leave explicit partial output. Cancellation warns;
no automatic resume, atomic replacement or fsync guarantee exists. Once owners
stop, inspect/reconcile partial records before any explicit rerun, which overwrites
the destination. Do not append blindly or treat `degraded=false` as completion.

## Stage 8 — Audit before ingestion

Run the read-only mechanical checker on current generated envelopes and the
complete chunks file:

```bash
bash kask/scripts/audit-qa-quality.sh <generated.jsonl> <chunks.jsonl>
```

Use actual discovered paths in execution. Inputs use structured evidence in both
envelopes and flat QA; source rows carry `entity_ref`, `source`, `text`. Verify
each quote against its own unique chunk **and** source. Bare string quotes,
missing metadata, malformed rows, duplicate refs and generation errors are gaps
or findings, not silently repaired evidence. An empty evidence array is valid
generation structure but a missing-citation quality gap.

The audit reports reconciled physical rows, citation verification, six-gram
**document frequency** (5% review-candidate threshold), QA-label distribution and
identified source coverage. These are not semantic contamination, actual Bloom
difficulty or subject-diversity verdicts. It does not extract every factual/IS/OUGHT
claim, check paraphrase entailment, compute sourced derivations, judge reasoning
or completeness, or perform narrative-leak review. Ordinary prose retains
unperformed claim/narrative checks even if verbatim. Exact evidence does not
promote answer prose beyond `model_inference`.

Canonical provenance vocabulary: `tool_verified`, `platform_derived`,
`model_inference`, `unavailable`, `tool_no_match`, `pending_check`, `rejected`.
Keep claim/source, cross-check and a substantive `why` (at least 40 characters).
SAR/CVR/HFR/NLR weights are **0.30/0.25/0.20/0.25**. Missing applicable checks,
zero checked claims/citations or incomplete rows propagate null; never average
only passing/measurable rows. Only the explicit citation-only control, where
`output` encodes a JSON array of canonical citation objects also present in
`evidence_quotes`, has no narrative fields: disclose “NLR vacuous”. Do not convert
ordinary QA into that control to manufacture a score.

Exit codes: 0 narrow mechanical checks complete; 1 high citation findings;
2 missing checks/data; 64 usage error. **None authorizes ingestion or a live run.**
Use `bash kask/scripts/test-audit-qa-quality.sh` and, if available, `shellcheck`
for bounded synthetic controls in scratch; preserve real controls read-only.
The checker loads inputs in memory: bound controls before full-artifact audits
and disclose coverage. No synthetic controls belong in production input paths.

**Semantic gate before pilot expansion, full generation or ingestion:** require
every applicable canonical `grounding-verify` report to have non-null
`fact_score >= 0.80`, no high/critical findings, all missing applicable checks
resolved with evidence, plus semantic review of boilerplate, actual cognitive
difficulty and subject matter. Record genuinely inapplicable checks explicitly.
Operator review must actually resolve the missing checks; it is not a blanket
waiver. A partial mechanical audit cannot open this gate. Retain current
verification evidence and correction findings, not duplicate corpus versions.

## Stage 9 — Ingest, assemble and seek training approval

1. Dry-run `corpus_ingest_qa(generated_jsonl, output, db_path, passphrase,
   dataset, owner, dry_run=true)`. It validates/deduplicates without opening the
   DB or writing output. Admission is structural only: nonblank instruction,
   output, QA type, source and chunk ref; complete structured evidence entries.
   Concise answers survive. First valid case-insensitive exact instructions win;
   no minimum length, semantic dedup, DB dedup or semantic quality test is implied.
2. After semantic acceptance, ingest with `dry_run=false`. For re-ingestion,
   inspect the exact `training:qa:{dataset}:` prefix in the named DB and explicitly
   purge it before replacement. Do not infer/broaden a purge or retain parallel
   datasets. Retained-row indices restart per call; arbitrary partitioned calls
   to the same dataset are not a safe replacement for whole-dataset reconciliation.
3. Reconcile `total_nonblank_rows = generator_errors + malformed + parsed`,
   `parsed = filter_drops + duplicates + retained`, and non-dry
   `retained = stored + failed`. `stored_h_mems = stored`, `deduped = retained`,
   `filtered = duplicates + retained`. `status=partial_failure` and each
   `storage_errors` entry block completion. The file includes all retained rows
   even if storage fails; file writing and DB inserts are not one transaction.
4. Preserve `evidence_quotes`, source/chunk identity, `prompt_id`, `provenance`,
   concepts, difficulty and QA type in the retained training JSONL/h_mems for
   audits. For ChatML, call `training_assemble_dataset` with the same explicit
   corpus `db_path` and `passphrase`, `dataset`, `train_split`, `output_path`;
   omitting the DB queries the training server's separate store. Reconcile actual
   train/validation totals with stored survivors and the agreed size target.
   If using `corpus_prepare_training_dataset`, supply the operator-approved
   `base_model`; its PEFT recommendation is advisory, not approval to train.
5. Stop before training submission until the operator approves the base model
   and LoRA configuration under `lora-training`. Do not infer approval from a
   valid dataset, recommendation or audit exit code.

## Stage 10 — Verify and report actual state

Clear the in-memory index before testing a different DB; query with explicit
`db_path`, `passphrase`, `include_text=true` and a relevant question. DB hydration
occurs only when the index is empty; a nonempty index is not switched by `db_path`.
Verify source/text identity and relevant retrieval, not merely a positive count.

Use `lisp_eval` to check measured stage equalities and a separately evidenced
semantic-gate boolean. Require all requested sources, complete embeddings,
classified chunks, two prompts per chunk, reconciled generation/ingestion/export,
semantic acceptance, relevant retrieval and requested style validation. Never
substitute fixed totals, incomplete coverage, a model's success claim or file size.

Report every stage as not requested, blocked/failed, partial, or verified with
its actual tool outcome and evidence. Manual extraction merges, file partitions,
cleanup, audits and reruns carry the same status/accounting discipline. A blocked
optional branch is not an overall pass. Name unresolved IDs/errors and the next
owner/action; three no-progress retries halt for operator attention. Report tool
failures via `curator_report_skill_use_issue`, never silently bypass the engine.
No claim of a rebuild, ingestion, centroid or training completion is valid without
the corresponding run. Code/doc work cites its commit, or explicitly says
**uncommitted**; training readiness is not evidence of trained capability.
