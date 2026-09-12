---
name: build-corpus-pipeline
description: "Build or refresh a source-complete corpus from a caller-selected folder through conversion, chunking and retrieval, with optional classification, evidence-carrying QA and centroids. Bind run-specific identities, parameters and approvals at intake; verify each required stage before expansion."
---

# Build Corpus Pipeline

Build a functioning, verifiable corpus pipeline, not a hand-produced substitute
for a failed stage. This skill is the reusable procedure; the caller supplies the
source set, corpus identity, requested outputs and operating limits. Keep names,
paths, source inventories, approvals, measured counts and calibration in that
execution's record, never in this skill. A tool return, a nonempty file, and a
completed capability are different claims.

## When to Use

Build or refresh a corpus from a user-selected folder, optionally producing
classified passages, evidence-carrying QA, training exports or style centroids.
The source set need not be literary, single-author or related to a previous run.

## When NOT to Use

For a question over an existing corpus, use `corpus_query`. Training execution
belongs to `lora-training` after its separate model/configuration approval.

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
| `corpus_source`, source selection | Caller-selected folder and agreed file scope, including whether nested files are included; freshly inventory every selected file, including unsupported formats |
| `entity_ref_prefix` | One namespace; use `style:{author}` for an author corpus, with the exact same author identifier in compose/centroid calls |
| `db_path`, `passphrase` | One corpus DB; resolve the current `HKASK_DB_PASSPHRASE` from authorized credentials, never invent or print it |
| `max_tokens` | Optional approximate size target; absent uses `HKASK_CHUNK_MAX_TOKENS` / shared settings (code default 256), not a model tokenizer |
| `overlap_tokens` | Default **64**, yielding **48 words**; explicit `0` disables repetition |
| `multi_tier` | False for the directory QA substrate; per-file/text retrieval can request coarse/medium/fine tiers |
| embedding `model`, `batch_size` | Use the configured embedding model and tool's batching; do not substitute a training or chat model |
| `tag_batch_size` | **10** chunks per tagging inference call by default; distinct from the number of rows in a file partition |
| `concurrency` | Bound tool concurrency to available capacity; AIMD starts at up to 2, grows by 1, halves on capacity failure |
| requested outputs | Retrieval is the core path. Classification, QA/exports and style centroids are explicit branches; QA and tag-selected centroids require classification |
| `reference_author`, `config_path`, dimension selectors | Optional style branch; caller-supplied identity, current cognition YAML and explicit tag predicates for any requested subsets; the identity does not establish source authorship |
| `prompts_per_chunk` | Caller-approved positive integer for QA runs; pass explicitly rather than silently inheriting the tool default of **5** |
| `context_k` | Tool default **3**; `0` explicitly disables KNN, never a recovery from failed context reads |
| `type_distribution` | Five nonnegative integer weights in canonical label order; default `1,1,1,1,1` |
| `max_prompts` | Explicitly `classified_count × prompts_per_chunk`, or `0` for all; not a small fixed cap |
| `dataset`, `owner`, `train_split` | Explicit dataset/owner identity and agreed training split for QA exports; never inherit another corpus's defaults |
| execution record | Caller-selected record of source identities/hashes, stage paths, parameters, counts, verification and unresolved issues; keep credentials out |
| pilot and spend bounds | Measured input scope, selected models, request/time/retry limits and approved cost ceiling; changed source/model/volume invalidates old estimates |

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

## Instructions

### Execution loop

Plan from current inputs → run the smallest discriminating stage probe → compare
actual identities, counts and quality against that stage's gate → fix the failing
capability or premise and rerun that stage. Expand only after the probe passes.
The target is zero unresolved failures across the required source set and stages,
not a plausible output file. Stop an approach after three no-progress attempts.
On operator cancellation, stop and ask what was wrong; do not resubmit the call.

## Stage 0 — Bind inputs and verify readiness

1. Locate and inventory the caller's current source folder. Record each selected
   file's identity, relative path, content hash and extraction mapping. For a
   refresh, diff this against the previous execution: additions, removals and
   content changes. Do not infer sameness from file counts or inherited notes.
   Unsupported files and failed extractions remain visible scope gaps.
2. Record requested outputs, namespace/DB ownership, chunk/overlap parameters,
   QA prompt count/type mix and semantic criteria, optional centroid selectors,
   and pilot/spend bounds. Ask only for missing functional choices; do not borrow
   another execution's identities, volume targets or budget approval.
3. Verify tool schemas, installed/running components, settings-to-provider routing,
   template seeding/cache behavior, output paths and canonical credentials before
   expensive work. Do not hide broken setting propagation with per-call overrides.
   For a source folder outside permitted MCP roots, stage the approved files under
   the artifacts root and reconcile hashes/identity mappings. Do not broaden
   containment, use escaping symlinks or substitute terminal text conversion.
4. Reuse valid extractions only when they match unchanged source content and pass
   current checks. Directory conversion/chunking enumerate immediate children;
   for an agreed nested selection, process files individually with distinct source
   identities and reconcile the complete set. Never silently omit subdirectories,
   flatten colliding names or copy duplicates to simulate concurrency.
5. Mark each stage required or not requested. Verify prerequisites before each
   paid expansion and reprice from measured counts and current model pricing.
   A failed optional branch does not block independent work, but remains an open
   requirement until it succeeds or the operator changes the scope.

### Refresh and invalidation

Use one canonical identity for the requested corpus. When source content,
chunking, embedding models or schemas invalidate derived state, record which
outputs depend on it and regenerate those stages; never relabel stale rows as
current. Remeasure counts rather than forcing a previous total.

Before deleting derived data, inventory exact owned paths/prefixes and coupled
references, confirm workers are stopped and maintenance locks are respected, and
verify retained inputs suffice for reconstruction. Deletion/replacement requires
operator authorization; it is not implied by a docs edit or an ambiguous refresh.
Never purge unrelated namespaces in a shared DB. Preserve originals and valid
extractions, keeping out-of-scope retained inputs outside the active source set.
Remove superseded derived outputs and update their references in the same run;
do not leave parallel abandoned datasets. Clear warm retrieval before selecting
a rebuilt DB. Ambiguous ownership or incomplete retained inputs blocks deletion.

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

When classification is required, call `corpus_tag_chunks(chunks_jsonl, output,
concurrency, tag_batch_size, dry_run=false)`. Start the probe with
`tag_batch_size=1`; increase only within this execution's approved bounds after
identity correlation passes. Split JSONL below the tool's byte cap at record
boundaries, into disjoint files; never split records. When partitioning,
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
If neither QA nor another requested output requires tags, classification may be
marked not requested.

## Stage 5 — Optional style centroid and measured composition

Call `corpus_centroid(author, db_path, passphrase)` after complete embedding.
It selects `style:{author}:` and stores `style:{author}:centroid`. Optional
`refs_file` selects existing newline-delimited references without copying vectors;
optional `dimension` stores `style:{author}:{dimension}:centroid`. Select subsets
using this execution's explicit predicates; subsets may overlap. Report membership
counts, weak mappings and discrimination limitations instead of silently changing
the selectors. Calibrate with subset verbatim and contrasting controls, not only
generated outputs. Do not duplicate embeddings for subsets. Empty or missing
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
the approved `prompts_per_chunk`, `type_distribution`, and
`max_prompts=classified_count*prompts_per_chunk`
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

**Gate:** `prompts_written == classified_count*prompts_per_chunk`, unique prompt IDs, full primary
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
classification and the approved prompt count when required, reconciled generation/ingestion/export,
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

## Constraints

- Keep this procedure independent of corpus names, locations, historical totals,
  model aliases and one execution's approval. Such values belong in the run record.
- Retain source coverage, identity, evidence and explicit failure states across
  every stage; neither a file's existence nor a green mechanical check authorizes
  the next semantic or paid stage.
- Fix the capability rather than hand-completing its output. Prefer the existing
  tool path and the smallest requirement-derived repair; remove superseded paths.
- Validate generalization with a small independent source set and different run
  parameters as well as the requested corpus. Keep fixtures outside production
  inputs and disclose which branches were actually exercised.
- No training submission without its separate operator approval.
