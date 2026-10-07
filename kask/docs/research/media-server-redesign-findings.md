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

## Phases 3–4 — pending

**Phase 3 (next):** integration candidates through the essentialist gates
(Exist / Surface / Contract) — the cross-server surface enumerated in the
review doc's coverage table is the candidate list. **Phase 4:** the formal
layer — EDL Keep-range union semantics and layer-validation invariants as
Lean candidates (the re-anchored interpolation's monotonicity/confinement
invariants, proven in `reanchored_corrected_words`'s tests, are the first
candidates), finite structural checks via `lisp_eval`. Phase 5 (proposals:
PR-M1 and successors) and Phase 6 (plan) follow their own loops.
