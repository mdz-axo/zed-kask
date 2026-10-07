---
title: "Media MCP Server — Redesign Review Findings"
audience: [operators, developers, architects]
last_updated: 2026-10-07
version: "1.0.0"
date: 2026-10-06
status: "Active"
domain: "Composition"
mds_categories: [composition, domain]
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

## Phase 1 — Primitives derivation (COMPLETE 2026-10-06)

**Hypothesis (FINER-gated):** a primitive model derived from the four
verified reference models predicts the server's shape with named gaps.
F 9 (all four models verified and read — OMC at domain_supplement, the v3
reference page-by-page for the write surface, the transcript discipline at
derived, the README contracts ratified), I 10 (the model structures Phase
2's per-tool scoring), N 8 (composition over verified sources), E 10, R 9
(each primitive names its source and its exists? status — checkable).
**Verdict: HELD** — 39 primitives across 8 families; every gap the model
names is decision-recorded (ruled or documented with reasons), none
accidental.

### The primitive model (derived from the reference models, scored against the implementation)

**1. Asset & gallery** — source: OMC (CreativeWork = the root artifact;
Scene = rendered media; the creation graph = lineage; Participant =
models) + the README storage contract (`persist_slim_and_enrich`: one
SQLite transaction for file, Asset row, lineage, OMC graph).

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Asset (CreativeWork) | stable identity, bytes at a path, media kind, dimensions/hash; add, list, detail, delete | ✓ `gallery_add_media`, `gallery_list_assets`, `gallery_asset_detail`, `gallery_delete_image` |
| Index | organize a folder into the store; refresh (rescan + re-analyze); status | ✓ `gallery_organize`, `gallery_refresh`, `gallery_status` |
| Retrieval | tag search, semantic search (embedding), timeline grouping | ✓ `gallery_search`, `gallery_timeline` |
| Analysis (Scene) | faces, objects, colors, composition, scene pipelines; single-image description | ✓ `gallery_analyze`, `describe_image` |
| Lineage (creation graph) | record generation (op, prompt, model, seed, parents); read; reproduce | ✓ `gallery_record_generation`, `gallery_lineage`, `gallery_reproduce` |
| Album | metadata-only grouping; create/list/move/remove/delete/members | ✓ six album tools |
| Face registry (Participant) | validate reference, register, scan folder, list, remove, name group | ✓ `face_*`; build-out deferred (MF-3 ruling) |

*Seam-carriage:* results carry paths + display hints (```media blocks),
never payloads; the OMC concept mapping carries to the widget's shared
concept→explain dispatch.

**2. Generation** — source: OMC CreativeWork production + the inference
routing contract (two routes: IPC bridge / child-local router; configured
models only, no hidden defaults).

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Generate | prompt → provider → new asset with lineage; variants via num_images | ✓ `generate_image`, `generate_video` |
| Transform (derived form) | image-to-image with strength; region-selective edit; style transfer | ✓ `transform_image`, `image_edit_region`, `image_apply_style` |
| Upscale | higher-resolution derived form | ✓ `upscale_image` |
| Prompt expansion | short → rich prompt (Fooctor V2 pattern) | ✓ `expand_prompt` |

**3. Processing** — source: OMC Sequence (a contiguous segment of a
creative work) and Shot (a single capture/frame).

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Sequence ops | clip (subsequence), concat (join), from-images (synthesize), to-gif (format), remix (composite) | ✓ `video_clip`, `video_concat`, `video_from_images`, `video_to_gif`, `video_remix` |
| Overlay | caption text over sequence; video description; meme (text + motion) | ✓ `video_add_caption`, `video_caption`, `video_meme` |
| Frame extraction (Shot) | keyframes as searchable gallery assets with lineage | ✓ `video_extract_frames` |
| Image ops | background removal; collage | ✓ `image_remove_background`, `image_create_collage` |
| Probe | metadata read (duration, codec, fps) | ✓ `video_info` |
| Ingest | external URL → local asset, preflight-validated | ✓ `video_fetch` |
| Animation | image → video | ✓ `image_to_video` |

**4. Transcript / educt** — source: `transcript_linked_media` (ruling
2026-09-25): "a transcript bundled with its media, word-aligned, so
editing or selecting text edits the media and correcting the transcript
never moves its timings."

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Bundle (immutable source) | word-anchored timings; transcribe, store, list, get, delete; untimed storage is a surfaced degradation | ✓ `transcribe_bundle`, `transcribe_and_store`, `educt_store/list/get/delete_transcript` |
| Layer | typed kinds (speaker, paragraph, correction, highlight, EDL, realignment), provenance-carrying, validated before storage; store, list | ✓ `educt_store_layer`, `educt_list_layers` |
| Pass (LLM over working transcript) | paragraph, speaker (audio/text), correction, highlight | ✓ four pass tools |
| Working transcript (projection) | apply corrections; alignment states original/aligned/re_anchored/unaligned/untimed | ✓ `educt_apply_corrections` + the shared projection |
| Re-alignment | store the decision; equal-slice interpolation; precondition retained | ✓ `educt_realign_transcript` (MF-2) |
| Locate | deterministic quote → word/time ranges; ambiguity surfaced | ✓ `educt_locate` |
| EDL composition + render | Keep-range union from highlights; render → durable gallery media | ✓ `educt_edl_from_highlights`, `educt_render_edl` |
| Export | durable documents: SRT, highlights CSV, corpus text | ✓ `educt_export` |

*Seam-carriage:* the corpus_text export feeds corpus ingestion by
composition (the transcript-reel skill); SRT/highlights CSV are durable
documents with provenance.

**5. Cloud (Reduct)** — source: the pinned v3 reference.

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Connection | key delivery report; read-only probe | ✓ `reduct_connection_status`, `reduct_connection_probe` |
| Snapshots | bounded provider-supplied reads: projects, recordings, reels | ✓ three snapshot tools |
| Recording lifecycle | create, import URL, upload (gallery/local), status, transcript read, highlight read | ✓ six tools |
| Reel composition | create reel, add doc-range clip, add title, edit clip range, detail | ✓ five tools |
| Highlight writes | create, partial edit | ✓ `reduct_add/edit_recording_highlight` (MF-1 + follow-up) |
| Publication | explicit bool; token value never returned | ✓ `reduct_publish_reel` (MF-1) |

*Seam-carriage:* local educt never uploads or falls back from a cloud
request — the local/cloud separation is a contract; acknowledgements mean
submission, never verified composition.

**6. Async & workflow** — source: README admission-control contracts.

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Job | queued heavy op: submit, list, status, cancel | ✓ four job tools |
| Workflow | saved generation graph: save, list, load, delete | ✓ four workflow tools |

**7. Model & discovery** — source: the inference routing contract
(configured-only listing; no hidden models).

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Model catalog | configured-only list; per-model info | ✓ `model_list`, `model_info` |
| Discovery | paid single-page YouTube search with disclosed fetch accounting | ✓ `youtube_search` |

**8. Audio & voice** — source: OMC CreativeWork (audio) + the transcript
discipline (the bundle's audio source).

| Primitive | Properties / methods | Exists? |
|---|---|---|
| Capture | microphone recording; record + transcribe in one call | ✓ `audio_capture`, `record_and_transcribe` |
| Speech | voice design; TTS | ✓ `voice_design`, `generate_speech` |
| Audio sequence ops | trim, concat (lossless) | ✓ `audio_trim`, `audio_concat` |

### Named gaps (the model's predictive output)

Every gap the from-scratch model names is decision-recorded — none is an
accidental absence:

1. **Face build-out** (Participant refinement) — deferred, MF-3 ruling
   2026-10-06, reopening condition recorded.
2. **Cloud DELETEs** (highlight delete, unpublish) — ruled out by the
   zero-DELETE posture 2026-10-06; highlight delete's contract stays
   pinned for a posture change.
3. **Contractless cloud capabilities** (strikethroughs, redactions, media
   download, transcript correction) — out of scope with per-capability
   contract-status reasons (MF-1 adjudication).
4. **DNS-rebinding TOCTOU window** — accepted risk, operator ruling
   2026-10-06, revisit condition recorded (MF-5).

The model is COMPLETE at the primitive level: nothing the four reference
models imply is missing beyond the four ruled/documented gaps above. The
finer deviations — per-tool fidelity to these primitives — are Phase 2's
scope.

## Phase 2 — Fidelity review (COMPLETE 2026-10-06, contract-level)

**Hypothesis (FINER-gated):** each tool's deviation from the Phase 1
primitive model is evidenced at file:line. F 8 (contract-level per
family, not line-by-line per tool — stated scope), I 10, N 7, E 10, R 9.
**Verdict: HELD** — 3 findings (MF-6…MF-8), all cited; the primitive model
held as the scoring instrument.

**Scope, honestly stated:** this pass verified each family's load-bearing
contracts at file:line (the storage contract, the routing contract, the
transcript discipline's invariants, the v3 write contracts, the admission
contracts) — the fidelity check a Phase 2 owes the primitive model. A
line-by-line audit of all 102 tools is a follow-on depth pass, not this
record's claim.

### Faithful (verified at file:line)

- **Asset & gallery (26):** `delete_file=true` requires destructive mode
  (`tools/gallery.rs:1406-1409`); `gallery_reproduce` errors on corrupt
  lineage params, never defaults (`:1322-1333`); every asset-producing path
  composes through the rollback-armed `persist_slim_and_enrich`
  (`src/assets.rs:804-816`, rollback `:179-248`).
- **Generation (6):** all six tools compose through `persist_slim_and_enrich`
  (`tools/generation.rs:62,117,213,246,291,411`) — lineage recorded on every
  derived form.
- **Processing (15):** `video_info` preflights remote URLs
  (`validate_tool_url_with_dns`, `tools/processing.rs:1281-1300`);
  `video_fetch`'s yt-dlp output is media-validated before FFmpeg (README
  contract, verified in MF-5's scope pass).
- **Transcript/educt (18):** EDL Keep-range union semantics — Keep ops
  disjoint, Cut ops union, the union covers exactly the covered words
  (`src/transcript_select.rs:189,231,258-261`); `educt_render_edl` cleans
  temp clips on error paths too (`tools/educt.rs:217-221,245`); the
  realignment path is honored consistently by every timed consumer (MF-2
  receipts).
- **Cloud/Reduct (20):** acknowledgements mean submission, never verified
  composition; token values never returned (reel detail strips, publication
  reports presence only — MF-1 receipts); read bounds enforced
  (`read_bounded` 2/4/8 MiB per tool).
- **Async & workflow (8):** workflows persist in the gallery DB
  (`gallery_workflow` table, `tools/workflows.rs:3-6`); jobs' ephemerality is
  documented and surfaced (MF-8).
- **Model & discovery (3):** configured-only listing, unset modality absent
  (`tools/models.rs:15-25`); `youtube_search` reads exactly one paid page
  with disclosed accounting (`tools/youtube.rs:121,148-149`).
- **Audio & voice (6):** `audio_trim`/`audio_concat` lossless stream copy
  (`tools/audio.rs:499-501`); `audio_concat` imports its cardinality limit
  (`MAX_CONCAT_ITEMS`, `:567`) — the correct pattern MF-6's sites lack.

### Findings

**MF-6 — Copied cardinality limits outside `media_limits` (deviation →
PR-M1).** IS: the capture-duration cap is a hardcoded `3600.0` literal at
`tools/audio.rs:368` and `:412` (`audio_capture`, `record_and_transcribe`),
and the Reduct upload cap is a function-local `MAX_UPLOAD_BYTES`
(`tools/reduct.rs:1686`) — neither imports from
`hkask_types::media_limits`, the contract's single source for cardinality
limits ("Cardinality limits import from hkask_types::media_limits, never
copied" — onboarding record §contracts). `audio_concat` shows the correct
pattern (`MAX_CONCAT_ITEMS` import, `audio.rs:567`). OUGHT: the caps
belong in `media_limits` (`MAX_CAPTURE_DURATION_SECS`,
`MAX_REDUCT_UPLOAD_BYTES`) and import at the sites — a copied limit drifts
silently when the canonical changes (the feedback loop's fidelity property:
the enforcement site no longer reads the policy source). Effort XS.

**MF-7 — The publish body-name inference (watched, no action now).** IS:
`reduct_publish_reel`'s body parameter name (`publish`) is inferred from the
reference's field-name convention because the v3 PDF's body-name line is an
OCR-flagged gap; the inference is documented at the construction site
(`tools/reduct.rs`, `publish_reel` doc comment). OUGHT: a live authorized
publish probe would pin the actual wire name — but a live publish creates a
public link (the never-publish-by-default invariant makes casual probing
the wrong trade). Watched, trigger: pin it before the first production
publish, in the same authorized-throwaway session shape the reel-composition
probes used.

**MF-8 — Job-history ephemerality (no action, documented).** IS: the job
controller is in-memory (`src/jobs.rs:55`, `HashMap`), documented and
surfaced (`jobs.rs:3-5` — "a missing record is therefore surfaced as possible
server restart data loss"; tool descriptions carry "History is ephemeral and
lost when the media server restarts"). The primitive model's Job primitive
requires submit/list/status/cancel, not persistence — no deviation. Watched,
trigger: a workflow that depends on job history across restarts reopens this
as a persistence proposal.

### Findings-to-proposal mapping (reconciled via `lisp_eval` → green)

| Finding | Record |
|---|---|
| MF-6 | PR-M1 — move the two copied caps into `media_limits`, import at sites (XS) |
| MF-7 | Watched — live probe before first production publish |
| MF-8 | No-action — documented ephemerality; trigger recorded |

### Depth pass — tranche 1: gallery family (COMPLETE 2026-10-06)

26 tools audited line-by-line against the Phase 1 primitives (full read of
`tools/gallery.rs`). **Faithful:** 24 — organize (mode validation, activation-after-persistence, `mode_preserved` surfaced), status (no_gallery surfaced as status), search (per-mode input requirements), refresh (faces off by default, per-stage error surfacing, `images_pending_after_bound`), analyze (pipeline/target validation, `nothing_to_analyze` surfaced), name_face (face_id → not_found), the five face tools (force skips the vision call and thus admission; missing face → not_found), timeline (store failures propagate — comment at `:1037-1038`; display hints per image), record_generation/lineage (op validation; `lineage: null` surfaced), list_assets (limit clamped 1-500), asset_detail (exactly-one-of; corrupt OMC graph → internal), reproduce (no lineage → not_found; corrupt params → internal, never defaults; image-ops re-resolve the source), delete_image (delete_file gated on destructive mode → permission_denied; file-first ordering with the already-deleted failure message), add_media (containing-gallery resolution, not active gallery; failed_precondition naming the remedy), the six album tools (name validation; idempotency documented).

**MF-9 — Misplaced doc comment (found → executed same change).** IS: the
asset-detail doc text ("Get complete details … the inspector-panel data
source") sat above `gallery_list_assets` (`tools/gallery.rs:1183-1187`),
merging into its rustdoc — the stale/misplaced-comment misinformation class:
an agent reading the doc above `gallery_list_assets` sees the wrong tool's
contract. OUGHT: the doc sits above `gallery_asset_detail`. **Executed
2026-10-06** (uncommitted): the comment moved to `gallery_asset_detail`;
zero behavior change; clippy 0 errors; tests 450/0/6 green.

**MF-10 — Unknown-date timeline period keys (watched).** IS: for images
without EXIF dates, `gallery_timeline` derives the period key from the
literal `"unknown"` (`tools/gallery.rs:1052-1065`): month mode →
`"unknown"` (7 chars, correct), but year mode → `"unkn"` (`take(4)`) and
decade mode → `"unk0s"` (`get(..3)` + `"0s"`) — odd labels surfaced in the
result's `period` field. The grouping stays deterministic and correct (all
unknown dates group together); the labels are cosmetic. OUGHT (if acted on):
unknown dates group under `"unknown"` in every mode — an XS fix at the
period-key match. **Watched, no action:** the trigger is an operator or
workflow presenting decade/year timelines of undated images where the
`unk0s` label is user-visible friction.

### Depth pass — tranche 2: transcript/educt family (COMPLETE 2026-10-06)

19 tools audited line-by-line (full read of the educt regions and the
transcription tools in `tools/audio.rs`) against the
`transcript_linked_media` primitive. **Family-boundary correction:** the
Phase 1 reconciliation env used `transcript_educt 18 / audio_voice 6`; the
Phase 1 family table assigns `record_and_transcribe` to the transcript
family, so the boundary is **19 / 5** (both sum to 102, so the green held,
but the boundary was off by one tool — corrected here).

**Faithful: 17** — transcribe_bundle (local-input check, SSRF for network
URLs, `audio.rs:277-284`), transcribe_and_store (summary-only return,
NoWordTimings degradation surfaced, `audio.rs:348-353`),
record_and_transcribe (capture cap from `media_limits` — PR-M1; honest
`partial` state preserving the audio path on transcription failure,
`audio.rs:476-485`), store_transcript (stringified-form tolerance
documented, `educt.rs:312-328`; degradation surfaced), list_transcripts
(limit clamped 50/500, per-record degradation visibility), get_transcript
(not_found; working transcript included), delete_transcript (not_found on
zero removals; exports/renders preserved with detached identities),
store_layer (validated against word count, named-invariant rejection),
list_layers (oldest first), paragraph/speaker/correction passes
(NoWordTimings preconditions; speaker source dispatch rejects structured on
the audio path, never a silent no-op, `educt.rs:610-618`; unknown source →
invalid_argument), apply_corrections (realignment honored consistently
with the working transcript), realign_transcript (MF-2 receipts),
highlight_pass (unaligned fails visibly before inference),
edl_from_highlights (empty selection → not_found; union merge),
render_edl (whole-transcript-cut rejection; intermediates tracked and
cleaned on error paths; audio/video path selection), export (format
validation; SRT requires timed words; corpus_text degradations surfaced
per alignment state), locate (no_match surfaced with the
quote-the-rendered-form note; unaligned fails visibly).

**MF-11 — Unaligned corpus-text degradation message unnamed the remedy
(found → executed same change).** IS: the corpus_text export's unaligned
degradation message (`tools/educt.rs`, the `CorpusText` arm) said only
"hits cannot map back to word ranges" — while the parallel
`require_timed_words` message (updated in MF-2) names
`educt_realign_transcript`. The MF-2 message sweep missed this parallel
site — the same residue class as stale comments. OUGHT: both messages name
the remedy. **Executed 2026-10-06** (uncommitted): the degradation message
now names `educt_realign_transcript`; zero behavior change; gates green.

**MF-12 — Highlight/EDL layer sorts lacked the ID tie-break (found →
executed same change).** IS: `educt_edl_from_highlights` and
`educt_render_edl` sorted their layer selections by `created_at` only
(`tools/educt.rs`, both sort sites), while the correction-layer
selections (apply_corrections, realign) break timestamp ties with
`.then_with(id)` — among same-timestamp layers the highlight/EDL path
picked the OLDEST of the tied newest group (stable sort over the store's
oldest-first order), the correction paths the newest-by-ID. OUGHT: one
tie-break discipline across the family — the ID tie-break, matching the
correction selections. **Executed 2026-10-06** (uncommitted): both sorts
carry `.then_with(|| b.id.cmp(&a.id))` with the rationale documented;
zero wire change (ties are rare and the store order was deterministic);
gates green.

**Observed-fine, no finding:** `record_and_transcribe` relays the captured
audio as a base64 data URI (`audio.rs:435-437`) rather than the file path —
a deliberate choice (the data URI carries bytes to cloud STT providers;
`transcribe_bundle` passes paths/URLs for the caller-controlled source),
bounded by the capture cap (≤ 3600 s at 16 kHz mono ≈ 154 MiB base64 at the
extreme). If IPC size limits surface in practice, the fix is passing the
local path (the file exists); recorded as the trigger.

### Depth pass — tranche 3: cloud/Reduct family (COMPLETE 2026-10-06)

20 tools audited line-by-line (full read of `tools/reduct.rs`) against the
Phase 1 cloud primitives and the pinned v3 contracts. **Faithful: 20/20,
zero findings** — the honest result for the freshest code: the write
surface was built against the pinned contracts with the loopback fixture
battery (MF-1 receipts), and the depth pass confirms rather than finds.

Per-primitive verification: **Connection** — status checks the key
(permission_denied naming the env var and the Settings path, `:1327-1332`)
and reports honest `not_checked` states; probe returns only the HTTP
outcome, never the body (`:583-594`). **Snapshots** — limit 1-100
validated on all three (`:667`, `:737`); bounded reads (2 MiB); the parse
extracts id+title only, discloses `provider_returned_count` /
`returned_count` / `truncated` / `pagination: "unknown"` — the honest
no-pagination-claim contract (`:722-726`). **Recording lifecycle** —
create (title validation), import (URL ≤ 4096 + `validate_tool_url_with_dns`
SSRF preflight before Reduct fetches it, `:1109-1114`), upload_gallery
(indexed-asset-only; missing/media-type check; canonicalize-vs-indexed-path
symlink check `:1674-1682`; size cap from `media_limits` (PR-M1); hash
verified before transfer `:1701`), upload_local (absolute path, streamed),
status (512 KiB), transcript (format validation, 8 MiB, provider structure
not re-timed). **Reel composition** — text/range validation
(`validate_reel_text` `:821-828`, `validate_reel_range` `:830-837`); the
clip edit reads back the block type before POST and refuses title blocks
(`:937-945`). **Highlight writes + publication** — MF-1 + follow-up
receipts. **Token discipline, verified on every read path:**
`redact_share_tokens` strips `share_token` recursively from any object
shape (`:748-763`); `parse_reel_detail` redacts blocks and projects
`publication_state` (published/unpublished/undetermined) instead of the
token (`:787-794`); `publish_reel` reports presence only; the snapshots
extract id+title only, so no token can flow. The invariant holds with no
exception site.

### Depth pass — tranche 4: processing family (COMPLETE 2026-10-06)

15 tools audited line-by-line (full read of `tools/processing.rs`) against the
Phase 1 Sequence/Shot primitives. **Faithful: 14** — every ffmpeg tool
validates before dispatch (clip/gif/caption/remix range checks `:539-549`,
`:603-615`, `:722-726`, `:778-782`), preflights remote URLs
(`validate_tool_url_with_dns` at `:556-558`, `:622-624`, `:733-735`,
`:789-791`, `:957-960`, `:994-996`, `:1289-1291`, `:1318`), and composes
through the canonical publishers (`publish_local_media` /
`persist_slim_and_enrich`). Cardinality limits import from `media_limits`
(`video_from_images` `:870-875`, `video_concat` `:942-947`,
`video_extract_frames` `:1085-1090`). `video_remix` tracks intermediates and
cleans them on every path, with a final-rollback guard for the gif
(`:806-844`). `video_extract_frames` promotes scratch frames durably with
per-frame failure cleanup and honest completed/failed/partial status
(`:1110-1166`). `video_caption` surfaces frame-read failures
(`:1022-1032`). `video_fetch` validates + preflights the URL, cleans an
empty yt-dlp output with the cleanup failure appended (`:1338-1349`), and
surfaces the provider warning (`:1359-1363`). `video_meme` names the font
fallback unavailable with the install remedy (`:1201-1206`). The test-only
local-video validation gate is `#[cfg(test)]`-scoped with a Drop guard
(`:10-65`). Not verified at this boundary (the runner module's own scope):
`video/ffmpeg.rs`'s scratch-directory lifecycle for `extract_keyframes` —
the tool-boundary contracts (read-failure surfacing, durable promotion,
status) are what this tranche pins.

**MF-13 — `image_create_collage` bypasses the canonical storage contract
(deviation → proposal PR-M2 → EXECUTED 2026-10-06, operator-accepted).** IS
(at finding): the collage composed locally (image crate, `:432-478`), wrote
its output to `std::env::temp_dir()` (`:480-489`), and returned the bare
path (`:504`) — the ONLY asset-producing tool in the server that did not
compose through `publish_local_media` or `persist_slim_and_enrich`: no
gallery row, no stable identity, no lineage, no rollback-armed publication.
OUGHT: the Phase 1 Asset primitive — every derived form gets the storage
contract. **Executed (PR-M2):** `LocalMediaFormat::Png` arm added; the
collage now composes to a scratch write that `publish_local_media` consumes
into the durable artifacts dir with the gallery row + lineage
(op `image_create_collage`) + OMC graph, rollback-armed on every failure
path; the result gains `gallery_asset_id` + `display_hint`, keeps `output`
(now the durable path) and the layout fields (merged from effective
params); the temp-dir write and bare-path result are deleted. Pin:
`collage_publishes_through_canonical_storage` (gallery row, lineage op,
durable path under the artifacts root, hint carries the asset id).

### Depth pass — tranche 5: generation, async & workflow, model & discovery, audio & voice (COMPLETE 2026-10-06 — the depth pass closes at 102/102)

22 tools audited line-by-line (full read of `tools/generation.rs`, `tools/jobs.rs`, `tools/workflows.rs`, `tools/models.rs`, `tools/youtube.rs`, and the remaining `tools/audio.rs` regions) against the Phase 1 primitives. **Faithful: 20.**

- **Generation (6):** `generate_image` validates prompt + count against `MAX_GENERATION_VARIANTS` (`generation.rs:22-31`), persists each variant individually with causal per-variant failures and honest completed/failed/partial status (`:72-142`), one display hint per variant; `transform_image`/`upscale_image`/`image_edit_region` validate + preflight + persist slim; `expand_prompt` validates and surfaces the preset list on unknown style. **Observed boundary, not a deviation:** generation's transform/upscale/edit_region preflight unconditionally (`validate_tool_url_with_dns` rejects non-http(s) schemes, `hkask-mcp-server/src/security.rs:78-85`), so they accept only URLs — local paths fail visibly at preflight, and the gallery-index equivalents live in the gallery/processing families (`image_apply_style`), so the surface is coherent.
- **Async & workflow (8):** `job_submit` validates op against the asset-producing allowlist (a non-generation op has no asset to persist, `jobs.rs:107-118`), admission owns a slot BEFORE the record becomes visible (`:134-147`), cancellation is honored at every stage (biased selects at `:184-188`, `:207-211`), publication is rollback-armed on every failure path (`:296-320`), per-stage failures surface with warnings; `job_list`/`job_status` surface `history_scope`/`restart_behavior` (the MF-8 honest ephemerality) and `job_status`'s not_found names the remedy; `workflow_save` validates empty/size (`MAX_WORKFLOW_GRAPH_BYTES`)/JSON validity, `workflow_list` uses `DEFAULT_/MAX_WORKFLOW_LIST_LIMIT` from `media_limits`.
- **Model & discovery (3):** configured-only listing (unset modality absent), `model_info` not_found names `model_list`; `youtube_search` validates query + range, `permission_denied` naming `HKASK_SERPAPI_API_KEY`, single-page disclosure (`result_scope`, `provider_pages_fetched: 1`, `provider_request_limit`).
- **Audio & voice (5):** `audio_trim`/`audio_concat` validate ranges/cardinality (`MAX_CONCAT_ITEMS`), preflight remote URLs, publish through the canonical path as WAV; `audio_capture`'s cap is PR-M1's canonical constant.

**MF-14 — `voice_design` hardcoded the model label (found → executed same
change).** IS: the result reported `"model": "llama-3.3-70b"` regardless of
the model that actually ran (`tools/audio.rs:206,210`) — while the call
passes the RESOLVED STT model (`:188-199`). A hardcoded label that can
disagree with the resolved model is active misinformation (the
hallucinated-model-id class, incident catalog #1) and a re-declared model
literal (the model_constants rule). OUGHT: the result reports the resolved
model. **Executed 2026-10-06** (uncommitted): both branches report
`model.as_str()`; no test pinned the old label (verified by sweep); gates
green.

**MF-15 — `generate_speech` silently fell back on malformed voice_design
(found → executed same change).** IS: a `voice_design` JSON that failed to
parse silently became the default voice "Rachel"
(`tools/audio.rs:230-237`) — the no-silent-fallbacks class ("a default
substitution" is the rule's own wording): the caller could not tell their
voice design was rejected. OUGHT: malformed input surfaces as a caller
error with its parse cause; the no-voice-design-provided branch keeps the
documented default. **Executed 2026-10-06** (uncommitted):
`invalid_argument` naming the parse error; the optional-param default
branch unchanged; gates green.

**Depth pass coverage reconciliation:** 26 (gallery) + 19 (transcript) +
20 (cloud) + 15 (processing) + 6 (generation) + 8 (async & workflow) + 3
(model & discovery) + 5 (audio & voice) = **102/102** — every registered
tool audited line-by-line against the Phase 1 primitive model. Findings
across the pass: MF-9…MF-15 (7 executed in-change, 1 proposal PR-M2
executed on operator acceptance, 1 watched MF-10, plus the pre-tranche
MF-6/7/8 records).

## Phase 3 — Integration review (COMPLETE 2026-10-06)

**Hypothesis (FINER-gated):** each cross-server integration candidate
passes or fails the essentialist gates (Exist / Surface / Contract).
F 9, I 9, N 8, E 10, R 9. **Verdict: HELD** — 8 candidates, 7 "already
exists", 1 proposal (PR-S1).

| Candidate | Exist | Surface | Contract | Verdict |
|---|---|---|---|---|
| `media_panel` (TOOL_NAMES re-export, Steer prompt rendering) | ✓ live | none new | the name pin is the chain's integrity check | **already exists** — the pin macro covers it |
| `hkask-media-widget` (D18) | ✓ live | none new | shared OMC concept→explain dispatch, single implementation (`hkask-bridge-ontology/src/omc.rs`) | **already exists** — the duplication trap is already closed |
| `hkask-inference` media router (D35) | ✓ live | none new | two-route routing contract, operator decision recorded in its README | **already exists** |
| `kask_bridge` registration | ✓ live | none new | `id: "media"`, env allowlists | **already exists** |
| Shared pin macro (`ede1ba0ca8`) | ✓ live | none new | one implementation for every server | **already exists** |
| Corpus server | ✓ composition-level | **none — deliberately** | `educt_export` `corpus_text` → `transcript-reel` skill → corpus ingestion; grep found no direct code reference (the apparent hits were `reduction_pct` false positives) | **no action** — the composition path IS the contract; a direct API would couple the servers |
| Spreadsheet what-if | ✓ contract-shape sibling | none | both ```media and ```spreadsheet display-hint blocks are D18 panel widgets | **no action** — a sibling shape, not a consumer |
| `transcript-reel` skill | ✓ the skill exists; the realignment path it should name does too (`educt_realign_transcript`, MF-2) | one line in the skill's correction step (`SKILL.md:61-68` names `educt_correction_pass` → `educt_apply_corrections`; the realignment path after cardinality-changing corrections is absent) | the skill's process text | **proposal PR-S1** — skills are the operator's algedonic-review surface; filed, not executed |

## Phase 4 — Formal layer (COMPLETE 2026-10-06, honestly scoped)

**Hypothesis (FINER-gated):** the EDL union semantics and interpolation
invariants are either provable in core Lean under stated assumptions or
already pinned by two test oracles — and the choice is recorded, never
forced. F 9, I 8, N 8, E 10, R 9. **Verdict: HELD** — every candidate
named with its pin; the Lean layer deferred with its trigger.

| Candidate | Already pins it | Verdict |
|---|---|---|
| EDL Keep-range union coverage ("the union covers exactly the words any input range covered") | `union_ranges_covers_exactly_the_input_words` (`transcript_select.rs:701-717`) — the merge implementation against an independent HashSet-distinct-words oracle | **Two oracles already** — Lean would verify the same contract the set oracle pins; not forced |
| Keep-op disjointness (EDL validation) | `edl_to_keep_ranges` rejects overlapping Keeps (`SelectionError`, `transcript_select.rs:82-83`) + the EDL validation delegate (`transcript_layers.rs:234-244`) + tests | **Two oracles already** — validation rejects, composition asserts |
| Re-anchored interpolation monotone/confined/exact-endpoints | WAS single-oracle (the four MF-2 unit cases pin concrete outputs) — **now two**: `reanchored_interpolation_contract_holds_across_a_sweep` (`transcript_layers.rs`) asserts the contract identities (valid, monotone, non-overlapping, confined, exact endpoints) over an exhaustive span×token sweep (864 cases), independent of the slicing formula | **Two oracles now** — the sweep is the Phase-4 deliverable |

**The Lean layer, deferred with its trigger (attempted, not forced):** a
spec-pin file (`reanchored_interpolation.lean`) was drafted over Nat with
stated assumptions and `#print axioms` receipts; `lean_check` (Lean
4.34.0, core — no Mathlib in `kask/lean/lakefile.toml`) showed the needed
core lemma formulations require positivity the contract does not carry
(`Nat.mul_div_cancel_left` wants positivity proofs on both operands —
span may be 0; `Nat.div_le_div_left` wants `0 < numerator`). The gluing
cost exceeds the contract's oracle value, and the bound says do not force
proofs where tests pin against two oracles — the sweep delivers the same
contract pin in the project's own pattern. The file was deleted (it did
not check; a non-checking spec pin is a broken artifact). **Trigger:** if
the interpolation contract gains algebraic structure the sweep cannot
express (e.g. cross-edit global timing invariants, or a move to rational
slices), the formal layer reopens — with Mathlib as a prerequisite
decision.

## Phase 5 — Proposals (COMPLETE 2026-10-06)

In `media-server-redesign-proposals.md` (created this session): PR-M1
(executed — receipts in the findings doc), PR-S1 (the transcript-reel
skill realignment line — operator review). The findings-to-proposals map
reconciles via `lisp_eval` → green (every finding MF-1…MF-8 in exactly
one proposal or no-action record).

## Phase 6 — Plan (COMPLETE 2026-10-06)

In `media-server-redesign-improvement-plan.md` (created this session):
slices with execution status and receipts, net summary, operator
decisions enumerated. The review's doc set now matches the scenarios
precedent's shape: review, findings, proposals, improvement plan.
