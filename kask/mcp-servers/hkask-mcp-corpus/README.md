# hkask-mcp-corpus

Corpus processing, retrieval, evidence-carrying QA and style composition over
MCP. The server has **25 registered tools**; parameters extend existing tools,
not the tool count. The router and count test are in
`src/hkask_mcp_corpus.rs:252–307`.

Use the [tool reference](../../docs/reference/mcp-servers/corpus.md) for parameters
and the [build-corpus-pipeline skill](../../../.agents/skills/build-corpus-pipeline/SKILL.md)
for source-complete execution and operator quality gates. This document describes
the current code contract, not a completed live corpus run.

## Implementation map

| Boundary | Owner |
|---|---|
| Tool registration and shared model/concurrency resolution | `src/hkask_mcp_corpus.rs` |
| Convert, OCR and directory chunking | `src/tools/document.rs`, `src/services/convert.rs`, `src/ocr/` |
| Word-window chunking | `hkask-memory/src/text_chunking.rs`; corpus budget adapter in `src/helpers.rs`, source encoding in `src/text.rs` |
| Identity-correlated classification | `src/tools/tagging/ops.rs` |
| Canonical `TaggedChunk`, `ClassificationOutcome`, `QaEvidence`, prompt IDs and passage-ref exclusion | `hkask-types/src/corpus.rs` |
| Durable passage publication and warm/DB retrieval | `src/index.rs`, `src/tools/storage.rs` |
| Source-scoped context and prepared prompts | `src/services/prompt_builder.rs` |
| QA validation, envelope and accounting | `src/services/qa_pipeline.rs`, `src/tools/semantic/qa.rs` |
| Output ownership, transport and retry | `src/services/qa_batch.rs`, `src/batch.rs`, `src/path_safety.rs` |
| QA ingestion and retained metadata | `src/tools/corpus.rs`, `src/tools/corpus/qa_parsing.rs` |
| Style selection, centroid and validation | `src/compose.rs`, `src/tools/compose_tools.rs`, `hkask-memory/src/memory_store.rs` |

Source identity and derivation use [Dublin Core](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/)
and [PROV-O](https://www.w3.org/TR/prov-o/); procedure metadata uses PKO. These
anchors do not turn generated assertions or exact citations into verified prose.

## Tools (25)

| Group | Registered tools |
|---|---|
| Gather (3) | `corpus_discover`, `corpus_cache_work`, `corpus_discover_company` |
| Process (11) | `corpus_convert`, `corpus_ocr`, `corpus_is_complex`, `corpus_chunk`, `corpus_build_chunk_representations`, `corpus_embedding_inventory`, `corpus_tag_chunks`, `corpus_embed`, `corpus_extract_assertions`, `corpus_dedup_chunks`, `corpus_consolidate_chunks` |
| QA output (5) | `corpus_build_prompts`, `corpus_generate_qa_batch`, `corpus_ground_generated_qa`, `corpus_ingest_qa`, `corpus_prepare_training_dataset` |
| Compose (3) | `corpus_compose`, `corpus_rewrite`, `corpus_centroid` |
| Manage (4) | `corpus_cache`, `corpus_query`, `corpus_clear_index`, `corpus_purge_qa` |

## Template deployment

The host seeds registry templates at startup and supplies `HKASK_TEMPLATE_ROOT`.
The corpus server caches loaded templates until restart. Repository tests must
set `HKASK_TEMPLATE_ROOT="$PWD/kask/registry"` explicitly; there is no cwd or
compile-time checkout fallback. OCR requires the deployed `ocr-extract.j2` and
fails before vision inference when it is missing or invalid, rather than using
an inline prompt. Template updates therefore require host reseeding and a fresh
corpus process. The configured OCR model must emit the documented page-response
contract: YAML page metadata followed by text/HTML tables and optional inline
`page_x_y_width_height.png` figure annotations. The decoder rejects malformed
metadata, rotation requests and unsupported image destinations. Printed text
remains in `text`; `page_reports` retains metadata and figure annotations as
`model_inference`, without claiming that image files or verified crops exist.
Literal Markdown in code is preserved. Images use color and a 1288-pixel longest
edge, matching the publisher's input contract. Empty/failed conversions cannot
write an extraction file; these mechanical checks are not semantic acceptance.

## Extraction and chunking

`corpus_convert` handles documents or a source directory. Directory mode requires
`output`, persists one `.txt` per supported source and resumes only outputs that
pass its 50-word floor and deterministic quality checks. OCR-derived directory
outputs go to `{output}-ocr-staging`; merging into accepted extractions is an
explicit quality-gated operation. Each staged OCR text has a `.report.json`
companion containing the complete conversion result. Resume requires a matching
source path, exact staged text, current `ocr_protocol` and explicit verification verdict; a missing or
mismatched report fails visibly without re-OCR or admission. Directory responses
return `document_reports` (without text/structure) and `verification_failed`,
separate from I/O/conversion `failed`. A staged or resumed file is not an accepted
extraction, including when its whole-file text passes quality checks. File mode honors its `output` path; when present, the response returns summary metadata and the output path without duplicating the full text inline. Writing text does not certify quality (`src/tools/document.rs:30–109`).

PDF extraction and page rendering consume a contained file path, not an
in-memory copy of the PDF container. A PDF larger than the 32 MiB raw-text/JSONL
read cap can therefore use normal conversion without forcing paid OCR of native
pages. Single-file `corpus_convert` accepts `pdf_text_order=raw` for PDFs whose
text-object order reads columns sequentially; `layout` remains the default.
Choose `raw` only after comparing page order and tables on the source PDF;
non-PDF and directory conversions reject it. The option does not repair damaged
mathematical glyphs or authorize accepting a conversion without a source audit. Text inputs retain that cap; this does not promise a bound on Poppler's
memory use or extracted output size. `tools/document_tests.rs` pins large native
PDF conversion, oversized-text rejection and PDF symlink containment.

`corpus_is_complex` performs PDF text-layer/image-inventory triage; `summary=true`
returns routing counts and examples instead of every page. OCR uses the configured
image-capable model through the page pipeline. PDFs are decimated into pages;
zero text is an error. Results expose verification, empty/quality-failed pages,
error counts and breaker state. Review them before accepting text or expanding a
probe into a bulk run (`src/tools/document.rs:103–167`). There is no alternate
OCR backend to silently accept on endpoint failure.

All text, document and directory chunk modes use **one shared structural/sentence
word-window engine** (`hkask-memory/src/text_chunking.rs:123–197`). It sanitizes
text, prefers structural/sentence ends within the budget, advances on every
window and permits a short final fragment. Positive overlap repeats exactly the
preceding suffix while adding new words; explicit zero uses the same engine with
no repetition. Cleaning can remove boilerplate, so source coverage still needs
caller verification.

- `max_tokens` resolves from the request, otherwise shared chunk settings
  (`HKASK_CHUNK_MAX_TOKENS` overrides settings; code default 256).
- `overlap_tokens` defaults to **64 approximate tokens = 48 whitespace words**.
  It is repeated context, not a minimum chunk length. `0` is explicit off.
- Both effective bounds are `floor(tokens / 1.33)`. These are **not BPE/model-token
  limits**. Unicode, code and long words can consume more model tokens.
- The maximum must yield a word; positive overlap must yield a word and be smaller
  than the maximum in both token and word units. Invalid budgets fail before
  extraction/output (`src/helpers.rs:336–398`).
- Directory mode consumes immediate `.txt` children, requires `output`, writes
  single-tier JSONL and rejects `multi_tier=true`. Per-file/text multi-tier uses
  coarse/medium/fine defaults 2048/512/128, each validated against the overlap.
- Directory IDs use `prefix:utf8-<filename UTF-8 hex>:<ordinal>`. Fixed-width hex is
  reversible and collision-free for distinct source strings; punctuation is not
  collapsed (`src/text.rs:7–16`). Original `source` remains separate provenance.
  Reusing a prefix/source identity across unrelated inputs is caller error.
- Directory enumeration containment-checks **each child**, including symlink
  targets, before reading. It publishes JSONL from a temporary file only after
  all sources serialize. Indexing, if requested, is not part of that file's
  publication transaction (`src/services/convert.rs`).
- Before chunking, page-delimited text drops bounded blank, title, copyright,
  contents and index pages. Form-feed-free books use conservative section
  boundaries: front matter requires a metadata/contents signal followed by a
  body heading; trailing bibliography, references and works-cited sections
  require an exact heading after two-thirds of document words; a trailing index
  additionally requires index-entry structure. Prose mentions do not trigger
  removal. Well-formed Markdown image nodes are also excluded with inline byte-range
  accounting so model-inferred alt text cannot become source-evidence QA while
  surrounding source prose and semantic HTML remain (`hkask-memory/src/text_chunking.rs`).

Directory results include `total_documents`, `total_chunks`, resolved
`max_tokens`, `overlap_tokens`, `overlap_words`, `budget_basis`,
`source_id_encoding`, `zero_chunk_files`, `boilerplate_exclusion_reports`,
`indexed` and paths. Every source has an exclusion report with input, retained
and removed word counts plus each reason and page/line boundary. Reconcile actual
source/record identity, exclusion totals and bounds, not just file lines.
Nonempty `zero_chunk_files` is an explicit coverage failure for the pipeline.
`corpus_build_chunk_representations` applies this same filter once per accepted
canonical source before constructing every reference/current/fine/parent policy;
its schema-v2 manifest seals all source exclusion reports and reconstruction is
checked against the retained view, not the pre-filter bytes.

## Classification contract

Every on-disk `TaggedChunk` requires a `classification: ClassificationOutcome`:

```json
{"classification":{"status":"classified","ontology_protocol":"published-term-resolution-v1"}}
```

The other outcomes are `{"status":"failed","reason":"actual failure"}` and
`{"status":"unverified"}`. Missing classification or protocol is a schema error;
pre-canonical classifier records are not upgraded. Synthesized consolidation text
is unverified until classified itself (`hkask-types/src/corpus.rs`).

`corpus_tag_chunks` defaults to **10 chunks per inference call**. Canonical
`entity_ref` values remain server-owned and are not sent to the model. Each
returned entry must carry its exact short batch-local `correlation_id` (`item-N`);
after whole-batch validation the server restores the original canonical identity.
Array order is irrelevant. A singleton object is accepted only for one input.
Unknown, duplicate, omitted or malformed correlation entries reject the entire
affected batch before any tags are accepted; no position-based association or
string-only fallback exists (`src/tools/tagging/ops.rs`).

Failed inference, rejected responses and task join failures emit failed outcomes
with reasons. Structural fallback annotations and deterministic `method_signals`
remain measurable but do not imply classification. The returned `tagged` count
is **classified successes**; `tagged + failed = total_chunks`. Dry-run reports
inputs only and writes no tagged artifact. A full-source QA pipeline requires all
input identities classified, irrespective of the shared 10% degraded threshold.
The summary reports planned batches, provider responses, successful-response token
usage, `reported_cost_usd`, and whether cost reporting is complete. A null cost is
unknown—not zero—and must not be used to justify expansion under a dollar ceiling.

The classifier returns a compact tuple containing its short correlation ID,
exceptional `who/when/where/why` dimensions, and 3–5 raw `candidate_terms`. The
server adds universal `what/how`, grounded `bibo:Document`, default `Analyst`,
and Dublin Core subjects. The model never chooses an ontology namespace, prefix,
URI or fallback tier. The shared `hkask-bridge-ontology` resolver preserves
candidates and deterministically derives `ontology_tags` and `concepts`. Downstream embedding, assertion and QA
readers reject mismatched derived fields or the wrong protocol. Graph salience
and QA concept context use preserved descriptive candidates, because exact
published resolution may honestly collapse many terms to the common core; they
do not treat candidates as namespace/URI authority. Tagging includes `how`
and stores measured signals in `ontology.method_signals`; consolidation recomputes
signals from synthesized text but remains unverified.

## Complete-source prompt context

`corpus_build_prompts` takes current-protocol, canonically reconciled tagged rows
and an explicit matching `prefix` (tool default `corpus:researcher:`). It rejects
stale/noncanonical rows, duplicate refs, blank source/text and prefix mismatches.
Primary-only preparation (`context_k=0`, the default) does not open a DB and needs
no DB path or passphrase.

With `context_k > 0`, the builder loads stored embeddings and full `passage_text`
across the namespace, excluding reserved rule/centroid refs. Each passage must
have a valid vector and exactly one original source in text h_mem
`ontology.dc_source`. It groups candidates by source and selects the nearest
neighbors for the primary passage **from the complete source**, not just rows in
the current tagged input file. Ties use reference order. Missing text, invalid
vectors, ambiguous/missing provenance, duplicate stored refs, dimension mismatch,
primary source/text disagreement or failed reads are errors, not empty context
(`src/services/prompt_builder.rs:119–246`).

With positive `context_k`, `db_path` and `passphrase` are required. The builder
loads complete-source embeddings and provenance, selects neighbors by similarity,
and stores canonical passage mappings as local `p1`, `p2`, etc. Canonical IDs are
metadata in the prepared request but never enter model messages.

Builder defaults: `context_k=0`, `qa_pairs_per_chunk=2`,
`type_distribution="1,1,1,1,1"`, `max_pairs=0` (all). One prepared request per
chunk carries both factual and conceptual levels by default; a positive cap limits
requested **pairs**, and the final request may contain fewer levels. Summary fields
separate `prompts_written` (provider calls) from `pairs_requested` (expected QA
rows), plus context scope/link counts.

## Prepared QA JSONL contract

Each line has five required fields: `prompt_id`, protocol
`prepared-qa-local-evidence-v1`, ordered `passages`, `candidate_terms`, and
`qa_types`. Each passage has sequential local ID `p0`, `p1`, canonical
`chunk_ref`/`source`, and text. Unknown or old rendered-message fields are rejected.

The builder generates one deterministic `qa-<UUIDv5>` ID from source, chunk, the
ordered level set, and ordinal zero. Stored canonical identities are withheld
from rendered model messages.

### Evidence and generated records

The server partitions primary passage `p0` into overlapping exact source spans with
local IDs `e0`, `e1`, etc. A complete `prepared-qa-adjudication-v2` manifest is
required: it binds one passage decision and one ordered level mandate to every
prepared prompt's
`prompt_id`, `chunk_ref`, and `source` — there is no unadjudicated generation
path. V1, partial, duplicate, unknown, reordered, or
identity-mismatched rows fail before output creation. A reviewed passage skip writes
all mandated prompt-wide terminal rows without inference. A reviewed admit bypasses
any passage-quality call: the generator receives the level mandates,
selects evidence for mandated generation, and its parsed plan is compared to the
mandates deterministically. One mismatch receives one generator-owned correction with
the exact error; a second mismatch fails the prompt.

The disposition planner returns one ordered plan per requested
level; a malformed plan receives exactly one generator-owned schema correction before
the prompt fails closed. A generated level fixes one to three evidence IDs before any
question or answer is written; conceptual generation also fixes one closed relation
kind (`mechanism`, `relationship`, `causal_relationship`, `distinction`, `purpose`,
`framework`, or `transferable_principle`). An unsupported level records its canonical
`<level>_support_absent` reason.

The writer receives only the planned generated levels and their selected source
spans. It cannot add, remove, reorder, relabel or skip levels, and it cannot
select new evidence. It writes compact `{"level":"...","question":"...","answer":"..."}`
objects; a malformed writer response receives exactly one metered schema correction
before the prompt fails closed. No second model reviews, corrects, or accepts
anything during generation: writer output completes directly as an unverified
candidate. The server recombines the writer's completed levels with planned skips
and restores immutable `QaEvidence {chunk_ref, source, quote}` from `p0`.

Conceptual QA must explain its planned relation; direct recall of a name, list,
title, number, stated explanation or sentence paraphrase is factual. Questions and
answers must preserve source modality and cannot add advice, normative rules, causal
mechanisms or changed components absent from evidence. Material OCR/layout damage,
interleaved page furniture, sentence-breaking footnotes, unrelated seams and
required-thought truncation are prompt-wide contamination. A coherent continuation
fragment or short legible factual passage is not contamination merely because it is
short, begins mid-sentence, contains notation or lacks conceptual support.

`non_substantive_passage` and `contaminated_or_garbled` skip every requested level.
For an admitted passage, factual must generate; each other level either generates
(`conceptual` with one closed relation, all others with null relation) or skips with
its exact `<level>_support_absent` reason. Any malformed plan, or
writer response rejects the whole prompt after its applicable single correction.
Semantic QA still requires the separate Stage 8 audit; exact evidence restoration does
not certify answer entailment. Generated rows use
`prepared-qa-grounding-candidate-v1` with
`grounding_status=pending_external_verification`; skips carry
`grounding_status=not_applicable_skip`; reviewed rows additionally record
`adjudication_protocol=prepared-qa-adjudication-v2`. Existing prepared JSONL remains
`prepared-qa-local-evidence-v1` and does not need rebuilding.

Generated rows are **candidates, never verified QA** — no model can promote its own
output to verified. Acceptance happens only at ingestion, behind the re-executed
`corpus-qa-grounding-v1` gate. `corpus_ground_generated_qa` first produces a
hash-bound bundle (`manifest.json` + `grounding-rows.jsonl`) — deterministic,
zero-inference — whose rows carry `row_key` (prompt_id + qa_type + physical
line), the candidate's raw-line SHA-256, source identity, preserved candidate
terms, **server-recomputed** `TermResolution`s through the published ladder, and
per-claim records: every evidence quote checked byte-exactly against its
uniquely identified canonical chunk (provenance `tool_verified`, strength 2,
byte span), the answer checked byte-exact inside the row's own source-grounded
evidence (`tool_verified`, strength 2) or recorded honestly as
`model_inference` strength 1, and the instruction premise recorded
`not_applicable` with its unperformed check. No verified/authorized/confidence
fields exist anywhere in the bundle — it records mechanical facts and
authorizes nothing.

`corpus_ingest_qa` then re-executes everything before dedup, output, or DB
access: it re-hashes the bundle, checks row bijection against the candidate
file, requires sources classified under `published-term-resolution-v1` with
reconciling terms, recomputes the ontology resolutions, re-derives every claim,
and requires each artifact row to equal its re-execution — self-reported
strengths, spans, or resolutions cannot open the gate. Admission requires
**every citation claim at strength 2** (`tool_verified`/`platform_derived`) and
**at least one such citation**: the evidence a row rests on is byte-verified. A
synthesized answer that is not byte-exact inside that evidence is a
`model_inference` observation — the row is admitted on its citations and the
answer is persisted as model-mediated, never relabelled verified. A
model-mediated oracle judging entailment is *not* an admission path (see the
architecture note below). Manifest identity, the re-executed answer provenance
and the canonical ontology persist into the training JSONL and QA h_mems.
Nothing generated before this gate is ingestible; historical pilot artifacts
(v2–v8) are immutable evidence only.

One generated pair becomes one unverified candidate envelope:

```json
{"prompt_id":"qa-example","chunk_ref":"corpus:delay:0","source":"delay.txt","qa_type":"factual","response":{"instruction":"What is the delay?","output":"72 hours","type":"factual","concepts":["delay"],"evidence_quotes":[{"chunk_ref":"corpus:delay:0","source":"delay.txt","quote":"The delay is 72 hours."}]},"provenance":{"generator_model":"OpenRouter/example-model","grounding_status":"pending_external_verification","adjudication_protocol":null,"disposition_plan_protocol":"prepared-qa-disposition-plan-v1","prompt_protocol":"prepared-qa-grounding-candidate-v1","prepared_prompt_protocol":"prepared-qa-local-evidence-v1","prompt_id":"qa-example","source_chunk_ref":"corpus:delay:0"}}
```

The model identifiers above are illustrative, not configured defaults. Batch usage
and cost totals include every actual quality, planning, writing, and correction
provider response; they are not repeated on pair rows. Summaries separately reconcile `qa_levels_requested`,
`qa_rows_written`, `qa_levels_skipped`, and `skip_reason_counts`, and report
`reviewed_passage_admits`, `reviewed_passage_skips`, `reviewed_level_generates`, and
`reviewed_level_skips` when a v2 manifest is present. A skip writes
primary identity, requested `qa_type`, `status:"skipped"`, its closed reason and
protocol provenance, with no response. A failed prompt writes primary `prompt_id`,
`chunk_ref`, `source`, `error`, provider `completion_tokens`, and provider
`finish_reason`, with no response; unavailable provider telemetry is null rather
than fabricated. Skips and failed prompts are never training data.

### Grounding-gate architecture references

The QA pipeline generates unverified candidates and defers acceptance to a
re-executed mechanical gate — not to a second model:

- W3C, *PROV-O: The PROV Ontology*
  ([w3.org/TR/prov-o](https://www.w3.org/TR/prov-o/)) supplies the provenance
  model: a candidate is an entity attributed to its generator, and acceptance
  is a separate activity bound to entities the generator did not author. The
  `row_key`/`candidate_sha256` bindings and the artifact-equals-re-execution
  requirement make every acceptance checkable against the exact bytes it
  derives from.
- Thorne et al., *FEVER: a large-scale dataset for Fact Extraction and
  VERification* ([arXiv:1803.05355](https://arxiv.org/abs/1803.05355))
  grounds claim verification in retrieved textual evidence recorded by
  annotators without knowledge of the sentence a claim was derived from —
  the same separation the gate enforces: evidence quotes and answers are
  mechanically matched to canonical source bytes, never to generator output
  or a model's judgment about it.
- The project's Verification Commons Protocol v1.0 (2026-09-07), adapted from
  Elinor Ostrom's institutional design for governing common-pool resources
  (see `.agents/skills/grounding-verify/SKILL.md`), supplies the bounded
  provenance lattice (`tool_verified`/`platform_derived` at strength 2,
  `model_inference` as observation) and the explicit-unperformed-check
  discipline. The decoupled-verifier variant (a spawned model judging
  entailment) was rejected 2026-09-18: model-mediated judgments are
  observations, never authority, so citations — never the answer — carry
  admission, all at strength 2 by re-execution with no compensatory score.
- Coarse `5w1h_core` resolutions are honest anchors from the published
  fallback ladder (`hkask-bridge-ontology/src/term_resolution.rs`), never
  failures, and never model-supplied namespaces or URIs.

The adaptation here is: generator writes candidate → external grounding
manifest cites canonical source bytes and binds to the candidate's SHA-256 →
ingestion accepts only complete, exact-identity rows whose citations re-execute
byte-verified. A synthesized answer is admitted on those citations and persisted
as model-mediated; it is never promoted to factual truth, and operator
acceptance stays outside generation and ingestion. Stage 8 remains the external
semantic audit, advisory only.

## QA routing, scheduling and output ownership

A dedicated non-thinking QA generator is mandatory: explicit `model` overrides
Settings → Kask → Models → QA Generation Model (`kask.models.qa_generation_model`,
injected as `HKASK_QA_GENERATION_MODEL`). The setting defaults empty. Missing or
malformed configuration fails before output creation/inference; unresolved
models fail at the bridge/provider. No chat, classifier, consolidation-model or
training-base substitution. QA generation disables reasoning and has one
synchronous transport. The QA model is not approval of a training base.

Synchronous inference uses AIMD: starts at up to 2, adds one on success and halves
on transient capacity failure, bounded by requested concurrency. Each retry gets
its own slot. Only typed `Connection`, `Overloaded`, `Timeout` errors retry,
with **at most 3 total attempts** and 2s/4s backoff. Auth/config/model failures and
open circuits do not retry. A successful planning or writing response that fails
its typed schema receives exactly one metered schema correction attempt; a second
schema failure fails the whole prompt without partial rows
(`src/batch.rs`; `src/services/qa_batch.rs`).

Before truncating output, the generator validates the entire input, resolves the
model and rejects input/output identity aliases, including symlinks and hard links.
A **process-wide canonical-path lease** excludes competing writers across service
instances, including unresolved destination symlinks. The lease
is not a cross-process lock. Synchronous workers hold it until actually destroyed;
requesting abort does not release ownership early. On cancellation/write failure,
`JoinSet` aborts remaining local tasks; this is not a promise that remote work was
cancelled (`src/services/qa_batch.rs`; `src/path_safety.rs`).

Rows are written incrementally in synchronous completion order, directly to a
file; flush occurs every 10 prompt completions and at finish. Provider results
are correlated by ID: missing/duplicate known IDs, malformed entries and provider
errors each fail that prompt. Unknown IDs and batch-level IPC errors are tool
errors. Join failures retain prompt identity for a failed-prompt row.

| Summary field | Meaning |
|---|---|
| `prompts_total` | Validated input record count |
| `prompts_succeeded` | Entire response accepted and rows written |
| `prompts_failed` | Identified failed-prompt records |
| `qa_rows_written` | Accepted pairs only; a prompt can yield multiple pairs |
| `tokens_used` | Total usage across every returned planning/writing provider response, including rejected responses |
| `completion_tokens_used`, `completion_token_reporting_complete` | Completion-token coverage; incomplete reporting is unknown, never zero |
| `finish_reason_counts`, `finish_reason_reporting_complete` | Provider stop-reason distribution and whether every provider response reported one |
| `provider_responses`, `reported_cost_usd`, `cost_reporting_complete` | Cost coverage; null/incomplete cost is unknown, never zero |
| `output` | Requested destination |
| `degraded` | Shared failure-rate classification, at least 10%; not completeness |

Every successful return reconciles `prompts_total = prompts_succeeded +
prompts_failed`. Serialization/write/newline/flush errors propagate. Partial
output remains explicit in errors and cancellation warnings; no automatic retry,
resume, atomic replacement or fsync guarantee exists. After all workers stop,
a caller may explicitly rerun, **overwriting** output. Inspect/reconcile the
partial artifact first; never append blindly or race a timeout with a new call.

## Ingest QA contract

`corpus_ingest_qa` accepts generated envelopes or flat QA with the same required
metadata/evidence. It uses contained, size-capped UTF-8 input. `dataset` and
`owner` must be nonblank, including dry-run. In envelopes, instruction/output
and evidence are in `response`; primary `qa_type`, `source`, `chunk_ref` are
outside. Flat rows carry them beside instruction/output.

Structural admission is unchanged: nonblank instruction, output, QA type, source
and chunk ref, required structured evidence array, and complete evidence entries.
On top of structure, every ingestion runs the `corpus-qa-grounding-v1` gate
before dedup, output, and DB access (see the grounding contract above): the
manifest is required, every check is re-executed server-side, and admission
requires every citation claim at strength 2 plus at least one such citation.
A synthesized answer that is not byte-exact is admitted as `model_inference` and
persisted as model-mediated. Concise answers are valid; retained text is not
trimmed or rewritten. Invalid concepts, blank supplied prompt IDs, non-object
provenance or malformed evidence are malformed rows. First structurally valid
case-insensitive exact instructions win in file order; there is no semantic
dedup or existing-DB dedup (`src/tools/corpus/qa_parsing.rs:53–112`;
`src/tools/corpus.rs`).

Training JSONL retains `instruction`, empty `input`, `output`, `qa_type`, `type`,
`source`, `chunk_ref`, `evidence_quotes`, `prompt_id`, `provenance`, `difficulty`,
`concepts`, and the `grounding` identity object (protocol, manifest SHA-256,
`row_key`, and the re-executed `answer_provenance` — `tool_verified` when the
answer is byte-exact in its own evidence, else `model_inference`). h_mem values
preserve these alongside question/answer, `bloom_level`
and dataset; DC/BIBO and PKO metadata plus the candidate terms resolved through
the shared published-ontology resolver are in the ontology column. An absent
supplied `type` uses `qa_type`. No embeddings are generated (`src/tools/corpus.rs`).

| Reconciliation | Meaning |
|---|---|
| `total_nonblank_rows = generator_errors + malformed + parsed` | Blank lines excluded; non-null outer/response `error` is a generator failure |
| `parsed = filter_drops + duplicates + retained` | All parsed QA accounted for |
| `filtered = duplicates + retained`; `deduped = retained` | Exposed filter/dedup counts |
| Non-dry `retained = stored + failed`; `stored_h_mems = stored` | Failed inserts never count as stored |
| `storage_errors` | Logged and returned entity/reason for every insert failure |
| `status` | `dry_run`, `complete`, or `partial_failure` |

Dry-run writes no output, opens no DB, and returns zero storage attempts. Non-dry
output contains all retained QA even when DB storage fails. File writing precedes
DB opening; output and per-row inserts are not one transaction. A tool error or
partial status may leave output/earlier inserts intact.

Before re-ingestion, inspect and explicitly purge only the verified
`training:qa:{dataset}:` prefix in the named DB. Ingest does not replace or purge a
prior run. Its entity format is `training:qa:{dataset}:{source}:{retained_index}`;
indices restart per call, so independent input partitions do not provide a
whole-dataset idempotency guarantee. Reconcile the complete dataset rather than
silently colliding or leaving stale records.

## Audit and training gates

`bash kask/scripts/audit-qa-quality.sh <generated.jsonl> <chunks.jsonl>` is read-only
with respect to inputs. It verifies each structured citation against its own
unique `(chunk_ref, source)` and exact nonempty substring. It reports every
physical row, missing/invalid metadata, boilerplate six-gram document frequencies,
QA-label counts and source coverage. It does **not** certify paraphrase entailment,
claim completeness, actual Bloom difficulty, semantic subject diversity or
narrative leaks. Ordinary prose leaves applicable checks unperformed; missing
checks propagate null scores rather than a passing partial average. The explicit
citation-only control uses JSON-encoded structured citation objects, not prose.

Exits 0/1/2/64 mean narrow checks complete / high citation findings / missing
checks or data / usage error. No exit authorizes ingestion, a pilot or training.
Run bounded synthetic controls via `kask/scripts/test-audit-qa-quality.sh` in
scratch, then apply the skill's canonical semantic review gates. Keep current
source/evidence artifacts available after ChatML conversion.

`training_assemble_dataset` must receive the corpus DB/path credentials to read
its stored QA; its default DB belongs to the training server.
`corpus_prepare_training_dataset` converts Alpaca to ChatML and supplies advisory
PEFT recommendations for a provided base model. The operator still approves the
base model and LoRA configuration before training submission.

## Centroids and composition

`corpus_centroid(author, db_path, passphrase)` selects `style:{author}:` and stores
`style:{author}:centroid`. Optional `refs_file` is a contained UTF-8 file of existing
refs, one per line; whitespace/blank lines are ignored and duplicates count once.
Missing eligible refs or an empty selection fail before storage. Optional quality
`dimension` targets `style:{author}:{dimension}:centroid`; it is trimmed/lowercased,
nonblank, has no `:`, and is independent of vector size. Recomputing replaces the
destination, not additional centroid rows (`src/tools/compose_tools.rs:161–175,250–300`).

The shared `hkask_types::corpus::is_corpus_passage_ref` excludes empty refs, `:rule:`
refs and every suffix-`:centroid` ref. Prefix and explicit-ref centroid selection,
compose retrieval and prompt context use this same helper; this consolidation of
the predicate does not change selection behavior (`hkask-types/src/corpus.rs:13`;
`hkask-memory/src/memory_store.rs:523–566`; `src/compose.rs:324`).

Cognition YAML places `centroid_entity_ref` inside `embedding` and can declare
`embedding.retrieval.declared_method.signal` thresholds. All supplied signal
thresholds must match. Missing method signals exclude passages and increment
`method_signals_missing`; malformed metadata/read failures are errors.

Compose uses config prompt/retrieval/validation settings. Rewrite **always**
validates against the requested dimension (default `composite`), even when YAML
names another centroid. Both expose `centroid_missing`: absent requested target
means true with null distance/pass, not successful validation or fallback.
Explicit `corpus_compose(no_validate=true)` returns false with null validation
values; other lookup failures are errors. Judge measured `style_passed` against
the configured threshold, not a universally hardcoded distance.

## Retrieval-calibration runtime

`kask/scripts/audit/calibrate-chunk-retrieval.sh` creates a run-owned runtime
capsule before representation or paid embedding work. It stable-copies the runner,
query builder, host wrapper, evaluator, receipt inspector and selected corpus binary,
publishes a hashed manifest, then re-executes from `<output-dir>/runtime`
(`kask/scripts/audit/calibrate-chunk-retrieval.sh:74-131,189-208`). Schema-v2
preseal identity binds those component hashes and the manifest; the runner verifies
them before every shard and evaluation (`:383-409,514-527,571-581,818-828`). A
shared-tree edit or `target/debug` rebuild therefore cannot change an active run.
Resume enters the existing verified capsule and never substitutes current shared
executables (`:137-144`).

`kask/scripts/audit/inspect-chunk-calibration-run.sh <output-dir>` is the read-only
status surface. It verifies hashed preseal/checkpoint receipts and derives lifecycle
state, row/shard progress, attempts, actual model, pending retry refs and next unit.
It reports the embedding-to-evaluation transition without claiming completion and
rejects tampered receipts or premature final artifacts
(`kask/scripts/audit/inspect-chunk-calibration-run.sh:44-186`).

## Passage retrieval contract

`corpus_query(db_path=...)` hydrates stored embeddings/text only when the index is
empty. It is **not a per-query DB selector** on a nonempty index. Call
`corpus_clear_index` before selecting a different DB alone. Ephemeral chunk
indexing is not persistent. Durable identity is canonical DB path plus entity ref;
repeated embed/consolidate replaces that entry. `corpus_embed` stores the
provider-confirmed actual model identity when the provider supplies one, otherwise
it retains the requested model identity and reports that actual identity is
unavailable. `corpus_embedding_inventory` compares an exact shard ref set with an
existing DB and expected provider-confirmed model; it neither embeds nor creates a
DB. Original/synthesized `passage_text`, not annotation-prefixed embedding input,
supplies retrieved text and answer context. Publication also upserts
text/method-signals h_mems.

`include_text=false` defaults in plain/Lisp modes and affects returned text only.
Rows without usable stored text expose `text_available=false`,
`missing_passage_text` and a note; they are omitted from answer context. If no
usable context remains, `answer_error` surfaces the gap without generation.
Do not fabricate missing text; reconstruct from retained sources.

The index owner serializes DB publication/hydration/invalidation without holding
its lock across inference. Clear cancels pending publications; purge invalidates
matching canonical DB/ref namespaces and overlapping in-flight operations only.
Cancellation is explicit, including `corpus_embed.cancelled` and `note` (also
counted failed). Other DBs/ephemeral entries survive a scoped purge. Later new
operations may publish again. These guarantees coordinate this server's tools,
not independent external DB writers (`src/index.rs`; `src/retrieval_tests.rs`).

Purge/replacement are not cross-operation transactions. Storage errors propagate
with partial-application warnings, not zero counts. h_mem prefix deletion is
literal and case-sensitive: `%`, `_` and backslash do not widen the selection.

## Configuration

| Setting / environment | Use |
|---|---|
| `kask.models.qa_generation_model` / `HKASK_QA_GENERATION_MODEL` | Required dedicated QA generator; no chat fallback |
| `HKASK_CLASSIFIER_MODEL` | Configured tag/assertion classifier |
| `HKASK_OCR_MODEL` | Configured image-capable OCR model |
| `HKASK_EMBEDDING_MODEL` | Embedding override over shared model settings/defaults |
| `HKASK_EMBEDDING_DIM` | Vector dimension, default 1024; malformed values warn |
| `HKASK_CHUNK_MAX_TOKENS` | Approximate chunk-size override |
| `HKASK_MAX_CONCURRENCY` | Shared default concurrency ceiling (96); tools accept narrower bounds |
| `HKASK_DB_PASSPHRASE` | Authorized shared SQLCipher credential; no doc-supplied password |
| `HKASK_TEMPLATE_ROOT` | Root for docproc templates |
| `HKASK_QA_MODEL` | Consolidation model, not a QA-generation alias |
| `HKASK_DEFAULT_MODEL` | General generation, including composition; not QA fallback |


The server runs as an editor-managed MCP child process. Source reading,
configuration/authorization failure, partial output and unperformed validation
must remain distinguishable from an empty corpus or successful execution.
