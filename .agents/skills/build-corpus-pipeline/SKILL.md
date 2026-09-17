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
| Required current-protocol classification, identity correlation and server-resolved published anchors | `kask/crates/hkask-types/src/corpus.rs`; `kask/crates/hkask-bridge-ontology/src/term_resolution.rs`; `kask/mcp-servers/hkask-mcp-corpus/src/tools/tagging/ops.rs` |
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
| content scope policy | Confirm whether the canonical pre-chunk boilerplate filter matches the requested corpus: page-delimited books remove bounded blank/title/copyright/contents/index pages, and unpaged books use conservative section signals. Use `target_pages` only for an explicit source subset or OCR probe, not to duplicate routine book-furniture filtering |
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
| `qa_pairs_per_chunk` | Caller-approved positive level count carried by one prepared prompt per chunk; default **2**. Generation uses one disposition-planning response plus a writer response only when at least one level is supported |
| `context_k` | Default **0** for primary-only factual/conceptual QA; positive KNN context requires the corpus DB and authorized passphrase |
| `type_distribution` | Five nonnegative integer weights in canonical label order; default `1,1,1,1,1` |
| `max_pairs` | Explicit pair cap, or `0` for all `classified_count × qa_pairs_per_chunk`; not a small fixed cap |
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

### Observable execution contract

A resumable final artifact is not a progress surface. Before any stage call spanning
more than one natural work unit (source file, JSONL partition, prompt shard), verify
whether the tool exposes durable status or incremental progress. If it returns only
a final summary, partition the stage at those natural boundaries and execute bounded
waves. Never place a large corpus behind one final-only synchronous call merely
because the implementation can resume after cancellation.

Bind an operator-visible reporting cadence before expansion. Size the next wave from
the measured pilot duration so one wave completes within that cadence; do not encode
a corpus-specific file count in this skill. Independent units may run concurrently
inside a wave when they have disjoint outputs. Reconcile the whole wave before
starting another; aggregate tools partition JSONL only at record boundaries and
must reconcile the union of identities with the manifest.

The execution ledger is the durable status API when the MCP tool has none. Before a
wave, record `planned`, `completed`, `succeeded`, `failed`, `remaining`, the current
unit or wave, `started_at`, and `updated_at`. After every unit or wave, atomically
update those fields and report the same counts to the operator, including the last
completed identity, current blocker and next checkpoint. A stage is not “running”
when neither tool status nor a changing durable checkpoint can prove progress.

Cancellation is a state transition, not permission to retry. Inspect worker
liveness and durable outputs, stop orphaned workers when safe, classify each unit as
completed/failed/unresolved, and resume only unresolved identities into new outputs.
Never race a canceled writer, infer zero progress from a missing final response, or
make the operator ask whether work is still alive.

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
Record every removed artifact in the execution cleanup manifest with path, reason,
model/protocol if known, size and disposition. Remove superseded, partial,
wrong-model and pre-current-protocol derived outputs from active stage directories
and update their references in the same run; do not leave parallel abandoned
datasets. Never reinterpret them through a compatibility adapter. Clear warm
retrieval before selecting a rebuilt DB. Ambiguous ownership or incomplete
retained inputs blocks deletion.

## Stage 1 — Convert and audit extraction

Conversion preserves the document text and page boundaries needed by the canonical
pre-chunk boilerplate filter. Do not manually trim routine title/copyright/contents
or terminal-index pages with `target_pages`; that would create a second content-scope
authority and bypass the filter's exclusion report. Use `target_pages` only when the
caller selected an explicit page subset or for a bounded OCR probe.

Build a conversion queue by joining the immutable source manifest to one unique
expected extraction path per source. Inventory existing outputs before inference:
reuse a prior extraction only when its source hash and quality evidence match, and
classify every other identity as pending. Record the queue counts in the execution
ledger before the first conversion wave.

When `corpus_convert` has no incremental status surface, use file mode for each
pending source and execute bounded waves with disjoint output files. Reconcile and
checkpoint each wave before scheduling the next. A directory call is allowed only
when the tool exposes observable progress, or a measured pilot shows the entire
bounded set completes within the operator's reporting cadence and the operator has
accepted final-only reporting. Directory convenience never overrides observability.

If a measured pilot shows that one source file alone exceeds the reporting cadence
and `target_pages` is supported, partition that source into nonoverlapping page
windows with disjoint fragment outputs. Before every call, reconcile the range
against the queue and require its fragment output path to be absent; a completed
range is immutable and is never resubmitted or overwritten. Record
planned/completed/failed page ranges in the same queue. Publish the final extraction
only after the ordered range union
covers the selected physical pages exactly once, every fragment passes its checks,
and deterministic concatenation preserves page order and explicit form-feed page
boundaries through a temporary file plus atomic rename. If the tool output cannot be
assembled without changing source text or losing page identity, block and surface
the missing capability instead of hiding another long call.

Directory conversion requires an output directory, resumes only quality-passing
outputs, and places OCR-derived text in `{output}-ocr-staging`. File mode writes its
requested output; that write is not a quality acceptance verdict. Staged OCR has a
`.report.json` companion with its complete conversion result; preserve it for
review. Whether conversion is file or directory mode, retain per-source outcomes
and aggregate `document_reports` and `verification_failed` separately from
conversion/I/O `failed`. Check both: a failed page verdict can coexist with a
successfully written staged file. Selective-OCR page arrays may be indexed within
the OCR subset rather than by physical PDF page number; map them through the
ordered `ocr_pages`/triage identities before making any page claim. Blank or failed
pages that may be routine book furniture remain provisional until Stage 2 joins
them to the canonical boilerplate exclusion report; do not block the corpus or
accept the page before that join. Resume requires a matching report; missing or
mismatched evidence blocks without automatic re-OCR. Do not fabricate a report for old staged text; diagnose it and
explicitly regenerate only when needed.

For substantive image/diagram-only pages that yield no verbatim text, use a dual
representation when the caller has approved text collapse without multimodal QA:
exclude those pages from source-evidence QA, preserve rendered page images in a
separate hashed archive, and store text descriptions with explicit
`model_inference` provenance and `included_in_text_qa=false`. Record page identities
and the text/nontext boundary in the conversion report. Never present inferred
figure descriptions as source quotes or silently drop the archived pages.

For PDFs, `corpus_is_complex(path, summary=true)` provides cheap routing evidence.
Preflight required OCR with a small `target_pages` slice and `force_ocr=true`, then
inspect the report before bulk work. Missing configuration, endpoint errors,
`error_count`, `quality_failed_pages` or breaker-open results block expansion.
Source-confirmed blank pages can be recorded as such; never infer that every empty
page is benign. `include_structure=true` is only needed for the block view.

Audit every extraction for word counts, legibility, repetition, script/language
consistency and deterministic OCR outcomes. Document-level counts are a conversion
signal, not yet the retained-content verdict: Stage 2 owns the canonical front/index
filter and retained-page coverage check. The directory resume floor is 50 words plus
its quality gates; it is not semantic proof. Merge staged OCR into accepted extractions only after review.
For deliberate replacement of a passing extraction, verify it is derived and
reproducible from a retained original before deletion/reconversion; no backup
corpus. Audit failures stay blocked, not promoted by a copy operation.

**Gate:** every inventoried source has exactly one current-run extraction outcome;
conversion/I/O failures and unambiguously substantive-page OCR failures are zero.
Page warnings plausibly attributable to routine book furniture may advance only as
identified provisional items that Stage 2 must resolve before embedding or
classification. No duplicate/probe artifact remains. Bash and `jq` can measure
files/JSON; do not introduce Python tooling.

## Stage 2 — Chunk once, measure coverage and overlap

### Reference-model calibration

When the selected chunk policy is unvalidated for the corpus/retriever, changes a
shared default, or the caller requests calibration, compare it before paid downstream
expansion. Chunk size is a retrieval policy, not a universal constant. Use one
candidate-independent gold query/evidence set derived from source pages or other
pre-chunk source units; QA generated from any candidate chunking cannot evaluate that
same candidate.

The minimum reference suite is:

1. **Passage baseline** — greedy sentence-bounded 100-word passages, no overlap,
   merging a final passage below 50 words backward (Lewis et al., 2020; Chen et al.,
   2024).
2. **Current candidate** — the caller's proposed structure/sentence-aware maximum,
   floor and overlap, recorded exactly rather than relabeled as the baseline.
3. **Small-to-big challenger** — fine deterministic child units linked to
   source-faithful parent passages; retrieve children and expand parents. Generated
   propositions, summaries and contextual prefixes may be retrieval keys but never
   source quotations.

Hold corpus, query set, actual embedding model, retriever, top-k and retrieved-word
budget constant. Run `kask/scripts/audit/calibrate-chunk-retrieval.sh <run-spec-json>
<output-dir>`; use `--resume` only with the same immutable identity. The run spec must
name every accepted source with raw/canonical paths and SHA-256 values, one entity-ref
namespace, the exact embedding model, embedding batch size, query limit, fixed
`corpus_query_cosine` controls, and complete current/fine/parent shared-contract
parameters (`min_words`, `max_words`, `overlap_words`, `sentence_boundary`) and a
caller-approved `selection.max_budgeted_exact_evidence_loss_count`. That count states
how many held-out exact-evidence hits the caller will trade for better source recall,
lower duplication or lower index/context cost; never invent a universal materiality
threshold. Never put a DB passphrase in this record.

The runner derives candidate-independent queries from accepted canonical sources,
calls `corpus_build_chunk_representations`, embeds reference/current/fine retrieval
representations into isolated indexes with one actual model, queries all three, and
records fidelity, Recall@k, MRR, duplicate overlap, retrieved words, index size and
measured wall/storage costs. The representation builder must apply the same canonical
furniture filter once per source before every policy, publish a schema-v2 manifest with
all source exclusion reports, and verify reconstruction against that retained view; a
manifest without those reports is a calibration seam failure, not a historical baseline. nDCG and answer grounding remain explicitly unavailable
unless separately measured; never encode absence as zero. Selection admits only
source-fidelity-passing policies, then ranks retrieval quality under the fixed budget.
The sealed run identity hashes accepted sources, queries, the actual model, full policy
and retriever parameters, every representation, the child-parent map, parents and all
indexes; any resume drift fails. Do not add semantic breakpoint chunking, dynamic
routing, contextual generation or linked tiers merely to complete the comparison. The
design taxonomy is segmentation × embedding paradigm (Zhou et al., 2026);
in-document needle retrieval and in-corpus retrieval are separate evaluation strata.

After calibration selects a policy, call `corpus_chunk` with `input_dir` set to the
accepted `.txt` directory, explicit `output`, `entity_ref_prefix`, selected
`max_tokens`, `overlap_tokens`,
`multi_tier=false`, and `index=false` when Stage 3 will persist embeddings.
Directory mode enumerates immediate `.txt` children, not a recursive file tree.
Every child is containment-checked before reading; broken/escaping symlinks fail.

Before word windows, the shared chunk engine is the one content-scope authority:
page-delimited books remove bounded blank/title/copyright/contents/index pages;
form-feed-free books use conservative front/back section signals. Reconcile every
`boilerplate_exclusion_report` with its source and cross-check provisional Stage-1
page warnings: an excluded warning is resolved, while a warning on retained content
returns that source to conversion/OCR. Never reproduce this filter in orchestration.

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
For a manifest-backed queue, run
`kask/scripts/audit/run-corpus-classification-queue.sh` with a positive bounded
`max-units` wave and report its durable queue checkpoint before the next wave;
do not hide the remaining corpus behind one final-only invocation. If a terminal
unit contains both classified and failed rows, call
`kask/scripts/audit/reconcile-corpus-classification-unit.sh` once: it preserves the
original output, marks the unit `reconciled_partial`, conservatively reserves its
unknown cost, and appends a new immutable pending unit containing only failed
source identities. Never relabel the original unit pending or replay its successful
rows. The committed host runner `call-corpus-tool-via-host.sh` is the queue's
credential-safe synchronous transport.

Each output `TaggedChunk` requires `classification`:

```json
{"status":"classified","ontology_protocol":"published-term-resolution-v1"}
```

or `{"status":"failed","reason":"the actual failure"}` or
`{"status":"unverified"}`. Missing status/protocol is invalid, not implicit
success; pre-current-protocol classifier data is stale and receives no
compatibility upgrade. A classifier response is an outer array of compact tuples:
`["item-N", ["who", "why"], ["term one", "term two", "term three"]]`.
Canonical `entity_ref` values remain server-owned and are restored only after
whole-batch short-ID correlation succeeds. The model returns only exceptional
`who/when/where/why` dimensions and 3–5 raw candidate terms; the server supplies
`what/how`, document type, default expertise and Dublin Core subjects. The model
never assigns ontology namespaces, prefixes, URIs or fallback tiers. The shared
bridge resolver preserves candidates and derives canonical
`ontology_tags`/`concepts`. Missing, duplicate, unknown or malformed tuples reject
the entire affected batch. There is no singleton, positional or string fallback. Failure annotations and deterministic method signals do not make a row
classified. Synthesized consolidation text is `unverified` and must itself be
classified.

**Gate:** count current-protocol `.classification.status == "classified"`,
recompute anchors from every row's `candidate_terms`, require exact equality with
stored `ontology_tags`/`concepts`, reconcile returned `tagged`, `failed` and
`total_chunks`, and require all chunk identities classified. Failed, unverified,
stale or noncanonical rows block embedding-with-tags, assertions and QA; output
line count and annotation presence are not substitutes. Record planned batches, provider responses,
successful-response token usage, reported cost and cost-reporting completeness at
every checkpoint. A null or incomplete provider cost is unknown, never zero; do
not expand under a dollar ceiling until the remaining bound is supportable from
reported cost or an explicit conservative estimate. Re-run only after diagnosing
the failure and replace affected terminal records by identity, never duplicate
them in a merge.
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

Call `corpus_build_prompts` with `tagged_jsonl`, `output`, explicit `prefix`,
`context_k`, approved `qa_pairs_per_chunk`, `type_distribution`, and
`max_pairs=classified_count*qa_pairs_per_chunk` (or `0` for all). Default
`context_k=0` is primary-only and requires no DB credential. Positive context
requires the single `db_path` and authorized `passphrase`. No inference occurs.

The builder rejects nonclassified/duplicate refs, blank source/text and prefix
mismatches. Positive KNN context reads complete-source passages and provenance,
selects same-source neighbors, and stores canonical mappings as `p0`, `p1`, etc.
Canonical identities never enter rendered model messages.

One compact `PreparedQaPrompt` per chunk contains `prompt_id`, protocol,
`passages`, `candidate_terms`, and ordered `qa_types`. No rendered messages,
salience or canonical concept cache is persisted. IDs are deterministic from
source, chunk ref and the ordered level set.

**Gate:** `prompts_written == classified_count` when uncapped,
`pairs_requested == classified_count*qa_pairs_per_chunk`, unique prompt IDs, full
primary coverage, sequential local passage IDs, and expected context metrics. At
two pairs and equal weights each request selects factual/conceptual. Report actual
pair distribution; do not increase pair count without operator approval.

## Stage 7 — Generate QA with owned outputs

Start with an operator-authorized bounded pilot, keeping the full source/prompt
inventory as the target. Build a source-balanced pilot input with
`kask/scripts/audit/select-position-diverse-chunks.sh <tagged-jsonl> <new-output-jsonl> <chunks-per-source>`;
it preserves complete classified records and selects deterministic interior quantiles,
rather than silently treating each source's `:0` chunk as representative. Run Stage 8
on that pilot before expanding to full production; a pilot is evidence for a gate,
never a reduced completion scope. Call
`corpus_generate_qa_batch(prompts_jsonl, output, concurrency, model)`.
Preflight validates the whole prepared file/model before creating output. Record
prompt-level tokens, provider responses, reported cost and cost completeness at
every shard; null/incomplete cost is unknown and blocks paid expansion.
Input/output aliases (including symlink and hard-link aliases) are rejected.
A process-wide lease owns the canonical output across service instances and both
transports. It remains owned until workers are destroyed, not merely until abort
is requested. Never race a timed-out/cancelled call with a replacement writer.

Synchronous inference retries only typed Connection/Overloaded/Timeout failures,
at most **3 total attempts**, with 2s/4s backoff. Configuration/auth/model failures
do not retry. A returned disposition-plan or writer payload that fails its typed
schema receives exactly one metered correction attempt; a second rejection fails
the whole prompt without partial rows. There is one synchronous prepared-prompt
transport; no provider-batch side path.

The model returns exactly one disposition per requested level, in order. A
supported level is a grounded QA tuple; an unsupported or contaminated level is an
explicit quality skip:

```json
[["factual","What is the delay?","72 hours",["e0"]],["conceptual",null,"conceptual_support_absent",[]]]
```

A generated pair requires a nonblank question, answer and one to three server-owned
evidence IDs. The server restores canonical `QaEvidence` only after those IDs
resolve. Conceptual QA must require an explicitly supported mechanism, relationship,
distinction, purpose, framework or transferable principle; direct recall of a name,
list, title, number or sentence paraphrase is factual. Legal notices, publication
metadata, navigation, marketing, watermarks, isolated captions and garbled text are
not QA material. `non_substantive_passage` and `contaminated_or_garbled` are
prompt-wide: every requested level must carry the same skip, even if another span
appears usable. Use only the closed skip reasons `non_substantive_passage`,
`contaminated_or_garbled`, or the requested level's `<level>_support_absent` reason.
Malformed, ambiguous, wrong-level or evidence-bearing skips reject the whole prompt.
Generated envelopes retain primary identity, candidate terms, QA type, canonical
evidence and protocol/model provenance. Skip envelopes retain primary identity,
requested QA type, closed reason and provenance but no response, and are never
training data. Usage and cost totals include every returned disposition-planning and
QA-writing provider response and are not repeated per pair. Matching quotation bytes
still does not validate answer synthesis or cognitive difficulty.

**Gate:** reconcile `prompts_total = prompts_succeeded + prompts_failed` against
all prepared IDs and require no unresolved failed prompts. Separately reconcile
`qa_levels_requested = qa_rows_written + qa_levels_skipped` for accepted prompts,
inspect `skip_reason_counts`, and require every physical row to be either an
accepted QA envelope, explicit skip, or identified prompt failure. A prompt may
yield multiple pairs and/or skips; QA row count is neither prompt nor requested-level
coverage. Failure and skip rows are never training data. Before expansion, audit
both retained QA and skip decisions: a model that generates factual recall under a
conceptual label or skips substantive support has failed the semantic gate.
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
or findings, not silently repaired evidence. A canonical quality-gated
`status="skipped"` disposition is a reconciled terminal non-QA row: its claim,
citation and narrative checks are genuinely inapplicable, while malformed skips
remain invalid shapes. An empty evidence array on generated QA is valid generation
structure but a missing-citation quality gap.

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
