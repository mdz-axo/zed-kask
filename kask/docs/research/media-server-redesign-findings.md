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

### MF-1 — Reduct cloud write surface (condition 1) — **CLOSED 2026-10-06, commit `f66b136fe2`**

- **Current condition (at close):** every capability on the unimplemented
  list is adjudicated against the pinned v3 reference
  (`~/Downloads/Reduct-Video.pdf`, 55 pages, operator-supplied, not
  committed). **Implemented** (fixture-tested request path/body, X-Auth-Key
  header, parsed acknowledgement, HTTP refusal):
  - `reduct_add_recording_highlight` — v3 pages 23-24: POST
    `.../highlight` with `start_time`/`end_time` seconds and optional
       labels (a color tag like `#orange` selects a non-yellow color);
    acknowledgement `{"highlight": id}`.
  - `reduct_publish_reel` — v3 pages 33-34: POST `.../publish` with an
       explicit bool (never defaulted — `true` asks the provider to create
       a share token, a publicly accessible link); acknowledgement carries
       `publish` + `share_token`, and the token VALUE is never returned
       (publish state and token presence only — the surface's token
       discipline). A publish=true ack without a token is surfaced as
       failed_precondition ("may have succeeded; inspect"), never claimed.
    The body parameter name follows the reference's own field-name
       convention (GET and acknowledgement both carry `publish`; the PDF's
       body-name line is one of its OCR-flagged gaps) — the inference is
       documented at the construction site.
- **Out of scope, with reasons** (README's per-capability adjudication):
  strikethroughs (page 41 lists paths without body/response schema —
  implementing would guess the wire format); redactions (pages 21-22 list
  paths without body/response schema, and the reference's own warning says
  API audio redactions only redact audio, never transcript text); media
  download (page 21 lists the path without body/response schema; no private
  Reel render/download contract); transcript correction (the v3 API exposes
  no transcript-correction endpoint at all — verified across the reference;
  local re-alignment exists since MF-2).
- **Pinned but unimplemented — the cloud-DELETE posture (operator decision
  pending, 2026-10-06):** the remaining pinned write paths are both DELETE
  operations — highlight delete (v3 page 25, fully pinned including the
  `{"<highlight id>": "deleted"}` acknowledgement) and unpublish via
  `DELETE .../share_token` (v3 page 33, path pinned; response shape
  undocumented). The surface has never exposed a DELETE tool (20 tools,
  zero deletes); whether irreversible cloud deletes belong on it is an
  operator decision. **Follow-up implemented 2026-10-06 (uncommitted at
  record time):** `reduct_edit_recording_highlight` (v3 pages 24-25, fully
  pinned — POST only the provided fields, labels overwrite per the
  reference's warning; acknowledgement `{"<highlight id>": ...
  }`) completes the highlight-write symmetry with the block surface's
  create+edit pattern. Pin 101→102; fixture tests
  (`highlight_edit_sends_only_the_provided_fields`, input sanity, HTTP
  refusal); receipts: clippy 0 errors, `cargo test -p hkask-mcp-media`
  449/0/6 (323 main + 125 deser + 1 doc).
- **Reached-when check:** both tools have loopback contract tests
  (`highlight_and_publish_writes_send_only_the_v3_contract`,
  `publish_ack_without_token_is_surfaced_not_claimed`,
  `publish_http_refusal_is_classified_never_claimed`,
  `highlight_input_sanity_is_refused_before_any_request` —
  `src/tools/reduct.rs` tests) and docs-reference rows (media.md Reduct
  table); the README's unimplemented list is replaced by the per-capability
  adjudication.
- **Receipts:** `./script/clippy -p hkask-mcp-media` green (0 errors);
  `cargo test -p hkask-mcp-media` 446/0/6 (320 main + 125 deser + 1 doc;
  +4 over the MF-2 baseline). Three-piece sync: pin 99→101
  (`tool_surface_is_exactly_101_registered_tools`), OMC arms
  (`reduct_add_recording_highlight` → VERSION_INFO,
  `reduct_publish_reel` → SEQUENCE), docs rows (media.md count + history +
  Reduct table, README adjudication + publication prose, fleet table,
  diataxis, architecture plan).
- **Original condition record (2026-10-06 re-grasp, for the history):**
  strikethroughs, highlight **writes**, redactions, publishing, media
  download, and transcript correction remained unimplemented
  (`README.md:340-344`); page 41 listed strikethrough paths without
  body/response schema; no private Reel render/download contract;
  publication never happens by default.

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

### MF-3 — Face-recognition build-out (condition 3) — **CLOSED 2026-10-06: still deferred, decision recorded**

- **Current condition (at close):** full build-out remains **deferred**, and
  the deferral is now a recorded decision instead of an open one. The
  README's deferral note names the decision date (2026-10-06), the outcome
  (still deferred), the basis (the build-out was presented for ruling twice
  with options and costs — keep deferred / build on the LLM-template
  surface with acceptance criteria — and the operator directed proceeding
  without a build directive), the reopening condition (an explicit build
  directive), and the triggering condition (a workflow needing
  multi-reference voting or match reliability beyond the current
  two-template core). The working core remains the two vision-LLM templates
  (`validate_face_ref`, `match_faces` in `src/templates.rs`); there is no
  local embedding model and no local geometric matching — a previous
  LLM-produced-"embedding" cosine path was removed because LLMs cannot
  emit geometrically consistent vectors, and its store column was dropped
  with it.
- **Reached-when check:** the README's deferral note names a decision date
  and outcome instead of an open deferral ✓ (the face-recognition design
  section, updated 2026-10-06).
- **Decision basis, honestly recorded:** this closure uses the condition's
  own explicit "still deferred" branch — it is NOT a build decision made
  unilaterally. The operator's revealed direction (three proceed directives,
  none selecting build after two presentations) is the recorded basis; one
  word ("build") reopens the condition with acceptance-criteria definition
  as the first step.

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

### MF-5 — DNS-rebinding limitation (condition 5) — **CLOSED 2026-10-06: accepted risk (operator ruling)**

- **Current condition (at close):** the README paragraph states the ruling,
  not an open limitation. **Verified scope:** exactly two subprocess paths
  make outbound connections in this server's usage — yt-dlp (`video_fetch`)
  and ffprobe (`video_info` on a remote URL; ffprobe is bundled with FFmpeg,
  whose transcode invocations receive only local, preflight-validated
  paths — `src/tools/processing.rs:1281-1300` runs `validate_tool_url_with_dns`
  before the remote probe). Both paths preflight-validate DNS; the residual
  gap is the connect-time TOCTOU window between the preflight's validated
  resolution and the subprocess's own DNS lookup.
- **Mitigation priced out (verified 2026-10-06):** neither yt-dlp nor
  ffprobe exposes a resolve-style flag; URL-to-IP rewriting breaks TLS
  certificate validation (no SNI override); hosts-file mutation is global
  system state. The only implementable mitigation is refusing remote URLs
  in those two tools — amputating the media-ingestion workflow.
- **Ruling (operator, 2026-10-06): accepted risk**, on the media-lead's
  recommendation — narrow exposure (preflight-validated inputs,
  agent-mediated callers, a targeted timing attack), small blast radius
  (low-grade SSRF on a workstation; yt-dlp output media-validated before
  FFmpeg; local gallery), and the remove-vs-accept trade favoring acceptance.
  **Revisit condition:** a server deployment with valuable internal
  services, or a workflow feeding URLs from untrusted content without
  agent mediation, voids the acceptance and reopens the mitigation
  question.
- **Reached-when check:** the README paragraph states a ruling ✓ (the
  DNS-rebinding paragraph, updated 2026-10-06 — scope, priced-out
  mitigation, the ruling, and the revisit condition).

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
Phase 2's per-tool fidelity review (102 tools, findings numbered MF-6
onward).

**Primitive-family skeleton (scoped 2026-10-06 — the derivation's working
structure; per-primitive depth is the phase's remaining work):**

| Family | Reference-model source | Covers (tool groups) |
|---|---|---|
| Asset & gallery | OMC CreativeWork/Scene/Shot + README storage contract | gallery_* (26), face_* |
| Generation | OMC CreativeWork + inference routing contract | generate_*, transform, upscale, expand_prompt, image_edit_region |
| Processing | OMC Sequence (contiguous segments) | video_*, image_to_video, collages, memes, captions |
| Transcript (educt) | `transcript_linked_media` (ruling 2026-09-25) | transcribe*, educt_* (16), record_and_transcribe |
| Cloud (Reduct) | v3 API reference (pinned) | reduct_* (20) |
| Async & workflow | README contracts (admission control) | job_*, workflow_* |
| Model & discovery | inference routing contract | model_*, youtube_search |
| Audio & voice | OMC CreativeWork (audio) + transcript discipline | audio_*, voice_design, generate_speech |

## Phases 2–4 — pending

Phase 2 (fidelity, per tool with file:line), Phase 3 (integration,
essentialist gates), and Phase 4 (formal layer — EDL Keep-range union
semantics and layer-validation invariants as Lean candidates, finite
structural checks via `lisp_eval`) fill in as their PDCA loops run. No
findings recorded yet beyond MF-1…MF-5.
