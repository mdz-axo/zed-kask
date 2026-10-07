---
title: "Media MCP Server — Redesign Review Findings"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - media-server-redesign-review.md
---

# Findings — Media MCP Server Redesign Review

Companion to `media-server-redesign-review.md` (method, coverage, ground
truth). **State:** the five operator-named target conditions are recorded
below as the review's first accepted inputs (MF-1…MF-5, each re-grasped at
current file:line 2026-10-06); Phase 1 is opened with its derivation plan;
Phases 2–4 are pending and fill in as their loops run. Findings from the
fidelity review will be numbered MF-6 onward so every finding id maps to
exactly one proposal or no-action record (reconciled via `lisp_eval` at
Phase 5, the scenarios precedent).

## The five target conditions (onboarding record §6, re-grasped 2026-10-06)

These are the primary inputs to Phases 5/6: the review's findings and
proposals feed them, and the plan's slices execute them. When the operator
directs a specific condition, its kata loop runs directly — the phases are
its method, not a prerequisite re-run.

### MF-1 — Reduct cloud write surface (condition 1)

- **Current condition:** strikethroughs, highlight **writes**, redactions,
  publishing, media download, and transcript correction remain unimplemented
  (`kask/mcp-servers/hkask-mcp-media/README.md:340-344`). Page 41 of the
  v3 reference lists strikethrough paths without body/response schema, and
  there is no private Reel render/download contract. Publication never
  happens by default — a share token creates a publicly accessible link
  (`README.md:338`).
- **Target condition:** each unimplemented capability is either implemented
  against the pinned provider contract (fixture-tested request path/body,
  parsed acknowledgement, HTTP refusal — the way `reduct_create_reel` et
  al. were) or explicitly documented as out of scope with the reason.
- **Reached when:** the capability has contract tests and a docs-reference
  row, or the README's unimplemented list no longer names it.
- **Decision status:** technical execution (no operator decision required
  beyond slice acceptance); the publication-never-by-default invariant is
  non-negotiable in every slice.

### MF-2 — Transcript re-alignment (condition 2) — **CLOSED 2026-10-06, commit `c9a5c3b855`**

- **Current condition (at close):** the local re-alignment path exists as
  word-level re-anchoring. `educt_realign_transcript` (the 99th tool,
  `src/tools/educt.rs`) stores a `realignment` layer recording the decision
  to project a correction with re-anchored timings;
  `reanchored_corrected_words`
  (`src/transcript_layers.rs`) is the pure projection: each
  cardinality-changing edit's replacement tokens slice its source range's
  span equally (floor-ms, monotone, confined to the span), M==N edits keep
  the aligned path's one-for-one semantics (source confidence retained),
  interpolated tokens carry no STT confidence, and the immutable source
  bundle and its timings are never modified. The working transcript gains
  the `re_anchored` alignment state, honored consistently by every timed
  consumer (locate, highlight pass, SRT, corpus export) and by
  `educt_apply_corrections`; the unaligned precondition's message now names
  the re-alignment path. The precondition itself is RETAINED: without a
  realignment layer a cardinality-changing correction still fails timed
  consumers (the existing
  `cardinality_changing_correction_is_explicitly_unaligned` pin still
  passes), and an untimed source cannot be re-anchored (the tool refuses
  with the NoWordTimings reason).
- **Reached-when check:** the pin
  `realignment_restores_timed_consumers_after_cardinality_change`
  (`src/hkask_mcp_media.rs` tool_behavior_tests) does exactly the
  condition's sentence: stores a timed transcript, applies an insert
  correction (1 word → 2 tokens), asserts locate fails with
  `FailedPrecondition` naming `educt_realign_transcript`, stores the
  realignment layer, then drives `educt_locate` (word range 0-4, time
  range 0-3500ms over the re-anchored timings), the highlight pass, and
  SRT export carrying the corrected text. Four unit tests pin the
  projection algebra (insertion slicing, deletion/many-to-few,
  one-for-one passthrough, zero-duration degenerate).
- **Receipts:** `./script/clippy -p hkask-mcp-media -p hkask-inference`
  green (0 errors); `cargo test -p hkask-mcp-media` 442/0/6 (316 main +
  125 deser + 1 doc; +5 over the MF-4 baseline), `cargo test -p
  hkask-inference` 57/0/0. Three-piece sync: pin 98→99
  (`tool_surface_is_exactly_99_registered_tools`), `omc::tool_to_omc`
  arm (VERSION_INFO), docs rows updated (media.md count + tool list +
  Reduct pin refs, README re-alignment paragraph, fleet table in
  mcp-servers/README.md, diataxis reference, architecture plan). Residue
  sweep: zero live references to the 98 pin or count (the loop-register
  98/98 is a dated historical review record).
- **Original condition record (2026-10-06 re-grasp, for the history):** a
  correction that changes token cardinality left the corrected text
  readable and corpus-exportable, but timing-dependent navigation
  (`educt_locate`), highlighting, and SRT export failed with an explicit
  unaligned precondition (`README.md:252-259`); Reduct could re-align
  such edits server-side, and local re-transcription/re-alignment was the
  honest capability gap.

### MF-3 — Face-recognition build-out (condition 3) — OPERATOR DECISION

- **Current condition:** full build-out is explicitly **deferred**; the
  working core is the two vision-LLM templates (`validate_face_ref`,
  `match_faces` in `src/templates.rs`), and any expansion is meant to stay
  on the LLM-template surface (`README.md:379-383`). There is no local
  embedding model and no local geometric matching — a previous
  LLM-produced-"embedding" cosine path was removed because LLMs cannot
  emit geometrically consistent vectors, and its store column was dropped
  with it.
- **Target condition:** a decision, recorded with the operator, on whether
  the deferred build-out happens (e.g. better matching prompts,
  multi-reference voting) and what its acceptance criteria are — then
  either the implementation with its tests or an explicit "still deferred"
  with the triggering condition written down.
- **Reached when:** the README's deferral note names a decision date and
  outcome instead of an open deferral.
- **Decision status:** **operator decision required.** Options in
  functional terms: (a) keep deferred — the working core stays the two
  templates, the README records the ruling and the triggering condition;
  (b) build out on the LLM-template surface — better matching prompts
  and/or multi-reference voting, with acceptance criteria stated before
  implementation. Presented with costs when this condition's loop runs.

### MF-4 — Embedding-error typing (condition 4) — **CLOSED 2026-10-06, commit `1a8c500c12`**

- **Current condition (at close):** the typed variant exists and the
  string-matching is deleted. `EmbeddingGenerationError::NotConfigured(String)`
  lives at `kask/crates/hkask-types/src/ports/embedding.rs` (doc comment
  names the permission_denied mapping); `hkask-inference`'s direct-embedding
  fallback constructs it — `DirectEmbeddingPort::try_new` now returns
  `Result<Self, DirectEmbeddingPortError>` with the three failure modes
  typed (`NoProviderPrefix`, `MissingApiKey { env_var }`, `ClientBuild`),
  mapped by `direct_embed_error` / `direct_inference_error` so a missing
  credential is `NotConfigured` **naming the env var** (the former `Option`
  collapse misclassified all three modes as `Connection` — a live bug:
  the missing-credential case surfaced as `unavailable`, and the deleted
  string-matcher's patterns matched no live message at all).
  `classify_embedding_error` matches on the variant
  (`kask/mcp-servers/hkask-mcp-media/src/error.rs`);
  `is_credential_missing_error` and its stale doc comment are deleted; the
  `embed_text` call-site prefix changed from "Embedding model unavailable.
  Configure a cloud provider" (wrong on a permission_denied error) to
  "Embedding failed"; `media.md`'s classifier row updated.
- **Reached-when check:** `classify_embedding_error` matches on variants ✓;
  the pin test
  `embedding_not_configured_classifies_by_variant_not_substring`
  (media `error.rs` classification_tests) asserts missing-credential →
  `permission_denied` naming the env var AND the fails-without-fix
  direction — a `Connection` error carrying the old credential substring
  stays `unavailable`. The construction-side mapping is pinned by
  `direct_fallback_errors_classify_by_failure_mode` (hkask-inference).
- **Receipts:** `./script/clippy -p hkask-types -p hkask-inference -p
  hkask-mcp-media` green; consumer sweep compiled clean
  (`-p hkask-services-core -p hkask-mcp-corpus -p hkask-mcp-curator -p
  kask_bridge` — the variant lands non-transient in corpus's retry/breaker
  matches and non-retryable in services-core's `From` impl, both correct);
  `cargo test -p hkask-inference` 56/0/0 (+1), `cargo test -p hkask-mcp-media`
  437/0/6 (+1 over the 436 baseline). Residue sweep: zero references to
  `is_credential_missing_error` or the old collapsed message anywhere
  (code and docs).
- **Original condition record (2026-10-06 re-grasp, for the history):**
  `classify_embedding_error` string-matched credential-missing substrings
  via `is_credential_missing_error` (`src/error.rs:181-184`, used at
  `:220-227`) because `EmbeddingGenerationError` had no typed
  `NotConfigured` variant — its variants were `InvalidRequest`,
  `Connection`, `Api`, `Json`, `EmptyResponse`, `DimensionMismatch`
  (`kask/crates/hkask-types/src/ports/embedding.rs:30-42`).

### MF-5 — DNS-rebinding limitation (condition 5) — OPERATOR DECISION

- **Current condition:** the widget's preflight treats every block locator
  as untrusted (local media under the artifacts root or exact
  gallery-observed paths; symlink escapes fail; inline data MIME-checked
  and capped at 32 MiB; public fetches validate DNS and reject redirects),
  but it cannot pin opaque yt-dlp/FFmpeg connect-time DNS — DNS rebinding
  inside those subprocesses remains an explicitly documented transport
  limitation rather than a claimed guarantee (`README.md:83-85`).
- **Target condition:** either a documented accepted-risk ruling from the
  operator, or a mitigation (e.g. subprocess DNS pinning) with tests.
- **Reached when:** the README paragraph states a ruling or a mitigation,
  never an open limitation.
- **Decision status:** **operator decision required.** Options in
  functional terms: (a) accepted risk — the README records the ruling and
  its scope (which subprocesses, which threat); (b) mitigation —
  subprocess DNS pinning with tests, at the cost of the pinning
  infrastructure's complexity and its own failure modes. Presented with
  costs when this condition's loop runs.

## Phase 1 — Primitives derivation (opened, pending)

Derive the primitive model from the four verified reference models BEFORE
scoring the implementation against it:

1. **OMC creation vocabulary** — the tool→concept mapping's source: for
   each primitive, which OMC concept carries it across the server's seams
   (the shared concept→explain dispatch is the widget contract).
2. **Reduct.video v3 API** — the cloud-surface primitives: project,
   recording, reel, block, highlight, publication state; which are
   read-only vs. write, and what the unimplemented capabilities would add.
3. **Transcript discipline** (`transcript_linked_media`, ruling
   2026-09-25) — the educt primitives: the immutable word-anchored source
   bundle, the validated layer kinds (speaker, paragraph, correction,
   highlight, EDL), the working-transcript projection, the Keep-range
   union composition, and the re-alignment boundary (MF-2).
4. **Ratified README contracts** — the storage primitive
   (`persist_slim_and_enrich`: one SQLite transaction for file, Asset
   row, lineage, OMC graph), the error-classification contract, the
   publication-never-by-default invariant, and the unencrypted-gallery-DB
   startup contract.

For each primitive: properties and methods, reference-model source,
exists?, and what carries across the seams. The derivation's output seeds
Phase 2's per-tool fidelity review (98 tools, findings numbered MF-6
onward).

## Phases 2–4 — pending

Phase 2 (fidelity, per tool with file:line), Phase 3 (integration,
essentialist gates), and Phase 4 (formal layer — EDL Keep-range union
semantics and layer-validation invariants as Lean candidates, finite
structural checks via `lisp_eval`) fill in as their PDCA loops run. No
findings recorded yet beyond MF-1…MF-5.
