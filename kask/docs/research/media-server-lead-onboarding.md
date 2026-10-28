---
title: "Media MCP Server Lead — Onboarding"
audience: [developers, architects, agents]
last_updated: 2026-10-28
version: "1.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, domain, trust]
---

# Onboarding: Lead Developer — Media MCP Server (`hkask-mcp-media`)

Welcome. This document brings you from "new to this project" to "knows where everything lives and what to do first" for the media MCP server. It is grounded end-to-end in the tree: every load-bearing claim cites the file it comes from, and anything that could not be verified is marked as such.

## Assumed background (read this first)

This document assumes:

- **Rust** — async (tokio), traits, declarative macros, serde/schemars. You should read a `#[tool]`-annotated `pub async fn` and a macro-generated impl block fluently.
- **MCP protocol basics** — you know what a tool, a tool router, and a stdio server are. If not: `kask/docs/diataxis/hkask-mcp-server/tutorial.md` is the project's retained tutorial for the shared server crate, and the public MCP spec (modelcontextprotocol.io) covers the wire protocol.
- **Zed architecture, lightly** — "a panel is an `Entity` with a focus handle" level. Only the zed-side integration notes and any panel work need this; `kask/docs/diataxis/media_panel/reference.md` covers the panel side. If GPUI is new to you, the upstream `crates/gpui` examples and zed.dev docs fill the gap.

**Not assumed:** any prior knowledge of hKask, the `kask/` tree, MovieLabs OMC, Reduct.video, or this server's history. All of that is covered below with citations.

---

## 1. What has been built

The media MCP server curates existing research evidence: ingest, organize,
annotate, search, transcribe, and derive source-linked excerpts/reels. The
operator-approved integration target is **74 tools** (91 − 20 Reduct cloud
operations, removed 2026-10-22, + 3 media-layers tools landed 2026-10-28), not a claim that a rebuilt child
or current integration suite has been validated. The generated `TOOL_NAMES`
and `MediaServer::combined_router()` are the inventory authority. Shared
inference-library creative capabilities are not this server's tool surface.
Historical assets, lineage, and OMC graphs remain readable without restoring
creative execution. The research groups in `src/tools.rs` are:

### Gallery management — 28 tools (`src/tools/gallery.rs`)
`gallery_organize`, `gallery_status`, `gallery_search`, `gallery_refresh`, `describe_image`, `gallery_analyze`, `gallery_name_face`, `face_validate`, `face_register`, `face_scan_folder`, `face_list`, `face_remove`, `gallery_timeline`, `gallery_lineage`, `gallery_list_assets`, `gallery_asset_detail`, `gallery_delete_image`, `gallery_add_media`, `gallery_create_album`, `gallery_list_albums`, `gallery_move_to_album`, `gallery_remove_from_album`, `gallery_delete_album`, `gallery_list_album_members`, `gallery_annotation_create`, `gallery_annotation_list`, `gallery_annotation_update`, `gallery_annotation_delete`

The core lifecycle: `gallery_organize` creates the index and activates a gallery (activation survives restart; startup itself never picks a gallery), `gallery_analyze` persists model-derived source observations (objects, colors, composition, scene captions; faces only by explicit selection), and `gallery_search` finds images either by fuzzy tag match or by caption-embedding similarity (`semantic` mode). Albums are metadata-only groupings. Face recognition runs through vision-LLM prompt templates, not local code — a deliberate design decision with the rationale recorded in the README (`kask/mcp-servers/hkask-mcp-media/README.md:379-383`).

### Source-image and video processing — 9 tools (`src/tools/processing.rs`)
`image_create_collage`, `video_clip`, `video_to_gif`, `video_add_caption`, `video_concat`, `video_caption`, `video_extract_frames`, `video_info`, `video_fetch`

Source-derived FFmpeg operations (clip, GIF, caption, concat, keyframe extraction),
source-image collage, and visual analysis. `video_fetch` downloads a selected
platform URL via yt-dlp and publishes it as a durable gallery asset with rollback
semantics. A collage/caption/excerpt is a derived presentation, not new evidence.

### Audio capture and transcription — 6 tools (`src/tools/audio.rs`)
`transcribe_bundle`, `transcribe_and_store`, `audio_capture`, `record_and_transcribe`, `audio_trim`, `audio_concat`

Whisper-optimized microphone capture (16 kHz mono WAV), transcription with word-level
timings, and lossless FFmpeg trim/concat that publish durable source-derived assets.
Synthetic speech, image/video creation, and creative recipe execution are retired.

### Transcript timeline ("educt") — 21 tools (`src/tools/educt.rs`)
`educt_store_transcript`, `educt_list_transcripts`, `educt_get_transcript`, `educt_delete_transcript`, `educt_store_layer`, `educt_list_layers`, `educt_paragraph_pass`, `educt_speaker_pass`, `educt_correction_pass`, `educt_apply_corrections`, `educt_realign_transcript`, `educt_highlight_pass`, `educt_edl_from_highlights`, `educt_render_edl`, `educt_export`, `educt_locate`, `educt_annotation_create`, `educt_annotation_list`, `educt_annotation_update`, `educt_annotation_delete`, `educt_search_transcripts`

A local transcript-editing system modeled on Reduct.video's published interaction model: immutable word-level time anchors, text selections as media ranges, labeled highlights, ordered Keep ranges composed into an EDL ("Reel"), deterministic FFmpeg rendering, and durable SRT/highlight-CSV/corpus-text exports (`README.md:230-249`). LLM "passes" (paragraph, speaker, correction, highlight) store validated layers over the immutable word timeline. Researcher annotations are separate supplied observations, not authenticated human identity or verified evidence: gallery CRUD uses stable `gallery_asset_id`, `kind` (`tag`/`note`) and `value`; transcript CRUD uses immutable-source word ranges, label/note and stable annotation IDs. `educt_search_transcripts` is bounded lexical substring matching with exact excerpts, source/timing citations and continuation offsets, not semantic retrieval. Missing/unaligned timing stays visible. See the media reference for exact request fields and bounds.

### Media layers — 3 tools (`src/tools/layers.rs`)
`layers_store`, `layers_list`, `layers_get`

Time-keyed layers over an immutable media file — the media sibling of the word-indexed educt layers: items anchor to source-PTS second ranges validated against the ffprobe-anchored duration, over a closed kind vocabulary (shot, semantic, object, ocr, audio_event, researcher, edl). Persistence is SQLite in the server's existing database (`layer_store.rs`); layers are database rows only, source media bytes are never modified. Landed 2026-10-28 (commit `512ac8723a`).

### Async job queue — 4 tools (`src/tools/jobs.rs`)
`job_submit`, `job_list`, `job_status`, `job_cancel`

Process-local research jobs accept only `transcribe`, `audio_trim`, `audio_concat`,
`video_clip`, and `video_concat` with operation-specific serialized params. History
loss on restart is disclosed at the tool boundary; transcript persistence requires
explicit storage. No creative generation or saved recipe is admitted.

### Model browser — 2 tools (`src/tools/models.rs`)
`model_list`, `model_info`

### YouTube discovery — 1 tool (`src/tools/youtube.rs`)
`youtube_search` — exactly one paid SerpApi request per call, with the cost boundary disclosed in the response (`README.md:87-91`).

Per-tool descriptions condensed from the live `#[tool]` descriptors are in the reference doc: `kask/docs/reference/mcp-servers/media.md:337-545`. The crate README (`kask/mcp-servers/hkask-mcp-media/README.md`) carries the ratified behavioral contracts (gallery lifecycle, tool-result contracts, resource bounds) and is the first document to read in full.

---

## 2. Technical design

### Process model and bootstrap

The server is a thin binary over a library: `src/main.rs:6-9` is a one-line `#[tokio::main]` wrapper around `hkask_mcp_media::run()`. `run()` (`src/hkask_mcp_media.rs:548-665`) resolves the inference port, opens the gallery DB, and hands a `MediaServer` constructor closure to `hkask_mcp_server::run_server` — the shared server crate (`kask/crates/hkask-mcp-server`) that provides `run_server`, the `mcp_server!` macro, `McpToolError`, and the `execute_tool` dispatch wrapper (`kask/crates/hkask-mcp-server/src/server.rs:36-42`). All 13 built-in servers follow this same shape (`kask/docs/reference/mcp-servers/README.md`).

### How tools are registered

Three pieces must stay in sync, and tests pin every seam:

1. A tool is a `#[tool(description = ...)]`-annotated `pub async fn` inside a `#[tool(router(...))]` impl block in a registered research `src/tools/*.rs` module.
2. `build.rs` scans `src/**/*.rs` for `#[tool`-annotated functions and generates the `TOOL_NAMES` const into `tool_names.gen.rs` (`kask/mcp-servers/hkask-mcp-media/build.rs:1-13`) — annotation-based, because the modules also contain non-tool `pub async fn` helpers.
3. `combined_router()` sums the registered research sub-routers and the `#[tool_handler(router = Self::combined_router())]` attribute wires it as the runtime surface (`src/hkask_mcp_media.rs:446-461`).

The surface-count pin catches a sub-router silently missing from the sum — an
unwired `#[tool]` block registers nothing while `cargo check` passes.
`tool_names_match_live_router` checks generated/live inventory equality;
`omc_mapping_covers_all_registered_tools` checks ontology coverage.
Adding/removing a tool must reconcile the count pin, OMC mapping, and docs in
the same change. Test presence is not evidence that the current suite passed.

### Two inference routes (D35)

A single `InferencePort` field serves two deliberately different routes (`src/hkask_mcp_media.rs:549-559`, `README.md:346-348`):

- **Vision/chat/embed** cross the IPC bridge (`HKASK_INFERENCE_SOCKET`) to zed's `LanguageModelRegistry`.
- **Transcription** uses `media_generate` child-locally through `MediaRouter`
  with env-injected `DEEPINFRA_API_KEY` / `OPENROUTER_API_KEY`, not an IPC media
  round-trip. Explicit `params.model` wins, otherwise the STT env model; full
  provider qualification is required and cross-provider retry is forbidden.
  The shared inference router still has creative operations for other consumers;
  that does not expose them as media MCP tools or settings.

### Canonical asset storage and publication

This is the server's central invariant. File-producing research operations use canonical publication in `src/assets.rs`:
write a rollback-armed file under `{artifacts_dir}/media-mcp/generated/`, and
commit the gallery Asset row, effective source parameters, operation lineage,
and OMC creation graph in **one SQLite transaction** — coupled files and rows
roll back together. The historical directory name does not imply creative generation. The tool result carries the persisted path and provider metadata, **never the payload** — the "context bomb" incident (2026-08-31: two ~65K-token base64 results breached the context limit) is why this is enforced (`media.md:353-357`).

The gallery store itself is a durable SQLite DB at `{kask_data_dir}/mcp/media/gallery.db` (override: `HKASK_MEDIA_DB`) with **no in-memory fallback** — a DB open failure aborts startup, because the fallback made an outage indistinguishable from an empty gallery (`src/hkask_mcp_media.rs:561-624`). The DB is unencrypted and deliberately does not use `HKASK_DB_PASSPHRASE` (`src/hkask_mcp_media.rs:569-571`). Asset identity is **gallery + canonical absolute path**, not content hash; the missing-file policy is retain-and-mark-missing; unchanged rescans preserve IDs, annotations, and lineage (`README.md:136-157`).

### Admission control and limits

Direct heavy operations (provider calls, FFmpeg/ffprobe, yt-dlp, analysis, capture/transcription, transcript passes) share one **four-slot fail-fast semaphore** with background jobs (`MAX_CONCURRENT_HEAVY_OPERATIONS = 4`, `src/jobs.rs:15`; `admit_heavy_operation` at `src/hkask_mcp_media.rs:283-297`). The fifth operation fails before external work — never queued, clamped, or silently truncated. Cardinality limits live in `hkask_types::media_limits` and are shared with the panel rather than copied: concat ≤ 64 inputs and keyframes ≤ 256. Retired creative variant, image-sequence,
and recipe-graph operations are not admitted.

### Error classification, OMC anchoring, display hints

- Errors are a structured 17-variant `MediaError` mapped **per variant** to MCP wire kinds — e.g. `YtDlpAuthorization` → `permission_denied`, `YtDlpVideoUnavailable` → `not_found`, missing/rejected inference credentials → `permission_denied` (`src/error.rs`; full mapping table at `media.md:550-560`).
- Every tool maps to exactly one of nine MovieLabs OMC concepts via `omc::tool_to_omc` (`src/omc.rs`), baked into output JSON by `media_block::enrich_with_omc_and_provenance` (`media.md:562-580`).
- Results render inline through fenced **media blocks** (`display_hint` / `display_hints` in `src/media_block.rs`): JSON-serialized, carrying the stable `gallery_asset_id`, validated by both the viewer and the D18 widget (`media.md:359-375`).

### Integration with the rest of zed-kask

The server is registered as a built-in server (`id: "media"`, `binary: "hkask-mcp-media"`) in `BUILT_IN_MCP_SERVERS` with explicit credential and env allowlists (`kask/crates/kask_bridge/src/mcp_servers.rs:348-384`): credentials `OPENROUTER_API_KEY`, `DEEPINFRA_API_KEY`, `HKASK_SERPAPI_API_KEY`; env `HKASK_INFERENCE_SOCKET`, `HKASK_DATA_DIR`, `HKASK_ARTIFACTS_DIR`, `HKASK_MEDIA_DB`, `HKASK_MEDIA_STT_MODEL` and `HKASK_MEDIA_VISION_MODEL` only, and `HKASK_EMBEDDING_MODEL`. On the zed side, `crates/media_panel/` is a **cross-domain Steer conversation with Media/Library/Queue/Detail viewing tabs** — the operator drives media work through the curator conversation, the panel's prompt advertises the server's generated `TOOL_NAMES`, and results render inline via the media block renderer (`crates/media_panel/src/media_panel.rs:1-12`). The widget is `crates/hkask-media-widget/` (D18); benchmarks live in `crates/hkask-media-benchmarks/`.

---

## 3. Project goals and principles

### What zed-kask is

zed-kask is a fork of Zed. The fork's own surface lives under `kask/` (crates, MCP servers, scripts, registry, docs); everything else tracks upstream. Divergence from upstream is tracked seam-by-seam in `DIVERGENCE.md` at the repo root — each D-seam names what's ours, where it lives, and its pins. The seams that matter to you:

- **D35** — the media MCP server itself, plus the `MediaRouter`, the MovieLabs OMC ontology bridge, and the gallery store (`DIVERGENCE.md`, seam table row D35).
- **D18** — the viz-widget renderer seam, including `crates/hkask-media-widget/`.
- **D28** — standardized artifact storage (the `{artifacts_dir}` layout your generated media files land in).

The rule: don't "fix" upstream files speculatively — push fixes into `kask/` behind a D-seam, and any zed-side edit carries its DIVERGENCE.md update in the same pass (`.rules`, "Divergence surface").

### The Division of Responsibilities you step into

The project runs a fixed division: **the user is the product manager** — keeper of the functional requirements and judge of what the work is for; **the agent (or, for a given server, its lead) is the technical program manager** — owner of technical design, structure, and execution. Functional questions are the user's to answer; technical questions are decided and presented for functional veto. Each side has an operational rubric: the `product-manager` skill (`.agents/skills/product-manager/SKILL.md`) and the `program-manager` skill (`.agents/skills/program-manager/SKILL.md`). Section 5 extends this — it does not replace it.

### Operating principles

From the project's `.rules` (repo root, loaded into every agent's context):

- **`.rules` are traps to avoid, not maps to follow.** They record non-obvious, repeatedly encountered failure modes — read them before your first change, and re-read the section relevant to whatever you're touching.
- **Evidence-grounded, checkable claims.** Every assertion must be redeemable against a shared ground — the tree, the run, the commit. Completion records cite commit hashes, not intentions; "done, all tests green" before the commit exists has outrun the tree.
- **A coding task that only adds is not done (P5.5).** Name what the change replaces and delete it — old paths, copied pattern instances, tests of removed behavior — in the same change, and report net lines.
- **No creative additions unless explicitly requested.** Scope discipline is a project rule, not a courtesy.
- **Stale comments are active misinformation.** When behavior changes, every comment describing the old behavior is updated in the same change.

---

## 4. Coding guidelines

The concrete rules that apply to media-server work in this tree, derived from `.rules` and `DIVERGENCE.md` — not generic Rust style:

**Rust and build**
- No `unwrap()` in production code — propagate with `?`; in tests prefer `expect` with a message. Never silently discard errors with `let _ =` on fallible operations.
- No `mod.rs` files — `src/module.rs` instead (CI: `kask/scripts/check-no-mod-rs.sh`).
- New crates specify `[lib] path = "..."` in `Cargo.toml` — this crate uses `src/hkask_mcp_media.rs`, not `lib.rs` (`kask/mcp-servers/hkask-mcp-media/Cargo.toml:39-41`).
- Build with `./script/clippy`, not `cargo clippy`.
- Project scripts are bash under `kask/scripts/`, never standalone Python.

**MCP server patterns**
- Tool responses are `{"content": <value>}` envelopes — use `unwrap_tool_envelope`, don't re-implement (`.rules`, "MCP server patterns").
- Error classification must be per-variant, not blanket `McpToolError::internal` — `src/error.rs` is the in-crate exemplar; extend the enum and its mapper when you add a failure mode.
- Tool inputs accepting arbitrary JSON must use `AnyJsonValue`, not `serde_json::Value` (schemars renders `Value` as bare `true`, which breaks strict-schema providers).
- Missing credentials surface as `McpToolError::permission_denied` naming the env var — never `unavailable`, a silent fallback, or an empty result. The canonical pattern is `ctx.credentials.get("ENV_VAR")` → typed error → `permission_denied` at the tool boundary.
- MCP servers are leaf crates — `pub mod` → `pub(crate) mod` tightening is churn; focus dead-code hunts on items with zero references anywhere including `tests/`.

**Media-server specifics**
- The gallery DB is unencrypted and must not consume `HKASK_DB_PASSPHRASE` (`src/hkask_mcp_media.rs:569-571`); there is one global passphrase for every SQLCipher DB elsewhere, resolved only via `hkask_mcp_server::server::resolve_db_passphrase`.
- Any credential write that feeds MCP server env must call `nudge_mcp_servers(cx)` afterward, or the running server keeps the old key until next launch.
- New env vars read by the server must be added to the `config_env` allowlist in `kask/crates/kask_bridge/src/mcp_servers.rs` in the same change — the per-server filter silently drops unlisted vars (see the `HKASK_EMBEDDING_MODEL` comment at `mcp_env.rs:136-143` for the failure mode).
- Cardinality limits come from `hkask_types::media_limits` — import, never copy.
- Every removal ends with a full-repo symbol sweep (code **and** docs) plus a full build before any green claim.

**Test protocol**
- A fix that restricts behavior pins the legitimate case it must not break, in the same change.
- Tests must construct the server with the capability under test — a capability-stripped constructor can only pin degradation, never function.
- Degradation tests must assert the degradation is **surfaced** (a note/status naming the reason), never that an empty result equals success.
- Live-mutation probe suites run with `--test-threads=1` and keep probes self-contained.

**GPUI (panel-side work)**
- No `block_on` on the foreground thread; no tokio timers in foreground tasks (race a GPUI-native timer instead); `AsyncApp` is not `Send`. Full list under "GPUI traps" in `.rules`.

**Commit and PR hygiene**
- Commit messages are plain text at the `git commit` boundary — never markdown fences (the `kask/githooks/commit-msg` hook rejects fenced messages). Imperative PR titles, `Release Notes:` final section. In a shared tree, check `git diff --cached --stat` before any commit and prefer pathspec-limited commits.

---

## 5. What to expect as a new team member

### The combined role, built on what exists

The project's Division of Responsibilities stays exactly as defined: the operator remains the product manager and the ground-truth judge at the project level; the program-manager discipline (recover the spec before building, design before coding, execute surgically, verify against a real definition of done, leave no residue) remains the technical rubric. **Your role extends this additively: for the media server, you hold both sides.**

- **The requirements side (product-manager lens).** You own what the media server should let users do: requirements stated as falsifiable outcome claims, acceptance criteria that can fail, spec provenance for every requirement, and the wire-or-remove queue (features that are advertised but unwired get wired or removed — never left as dead surface). The operator remains the judge of functional ground truth; you are the keeper of the server's requirements and the one who brings requirement questions to them, in functional terms.
- **The technical side (program-manager lens).** You own design, structure, and execution: which patterns the server uses, what the pins enforce, how changes are verified, and driving every finding to a closed state. Technical decisions are yours to make and present with their functional consequence.

Both lenses are already written down as skills — read `.agents/skills/product-manager/SKILL.md` and `.agents/skills/program-manager/SKILL.md` in your first week; they are the operational rubrics this role combines.

### A realistic first week

1. Read the crate README end-to-end (`kask/mcp-servers/hkask-mcp-media/README.md`) — it carries the ratified contracts, not marketing.
2. Read the reference doc (`kask/docs/reference/mcp-servers/media.md`) — architecture, per-tool reference, error classification, key paths.
3. Run the crate's tests and read the pin tests in `src/hkask_mcp_media.rs:439-515` — they encode the invariants that matter most. (Baseline established 2026-09-30: `cargo test -p hkask-mcp-media` — 432 passed, 0 failed, 6 ignored. The six ignored were the opt-in live Reduct probes, removed with the Reduct.cloud capability 2026-10-22; the post-removal serial baseline is 311 passed, 0 failed, 0 ignored, 2026-10-22. Re-run it yourself to confirm your own working baseline.)
4. Trace one source-derived tool end-to-end: `video_clip` → canonical local
   publication (`src/assets.rs`) → gallery Asset + lineage + OMC graph →
   `media_block` display hint → inline render in the panel. Also check a manual
   annotation and a bounded transcript-search citation; neither certifies truth.
5. Read `DIVERGENCE.md` rows D35, D18, D28, and the `.rules` file.

### A realistic first month

- Own one small change end-to-end — the canonical exercise is adding or refining one tool, which forces the full pattern: `#[tool]` fn in the right `tools/` module, router membership, the current inventory/count pin, the `omc::tool_to_omc` arm, the docs-reference row, and tests that pin the behavior. The four pin tests will fail loudly if you miss a seam — that is them working.
- Sit in on the algedonic review (`algedonic-review` skill) to see how skill and tool quality is judged.
- Start the calibration habit below with your first work item.

---

## 6. Historical review starting conditions (2026-09-30)

The five items below preserve the original review's starting conditions, not
current work authorization. Their dispositions are recorded in the media
reference's **Redesign record (2026-10-06/07)**: cloud highlight create/edit and
publication have pinned contracts; contractless cloud operations and cloud
DELETE remain out of scope; local re-alignment exists; face build-out stays
deferred; embedding errors have typed classification; the subprocess DNS window
has an operator-accepted risk ruling with deployment/untrusted-input revisit
conditions. Do not reopen those decisions by treating this historical list as
a live backlog. In particular, face ID remains an explicit opt-in under the
research-curation scope; the Reduct cloud upload/publication opt-ins were
removed with the Reduct.cloud capability (2026-10-22).

Each priority below is a **target condition**, not a backlog item: a measured current condition (cited), an observable target, and the evidence that says it is reached. Work each one with the predict → measure → correct habit: before starting, state the expected observation and falsifier; record a confirmed goal with real source and criteria when useful, omitting routine forecast. After work compare evidence with the expectation. Worker progress, native human closure and independent assessment stay separate; optional forecasts require a complete prospective declaration and frozen resolver. Two things the project's metacognition discipline insists on: never score your own prediction (the outside score is the check), and report a plateau honestly — two unchanged measured iterations while the target still fails means the approach is wrong, not that you need more of it.

**1. Close the transcript re-alignment gap.**
*Current condition:* a correction that changes token cardinality leaves the corrected text readable and corpus-exportable, but timing-dependent navigation, highlighting, and SRT fail with an explicit unaligned precondition — local re-transcription/re-alignment is the honest capability gap (`README.md:252-259`).
*Target condition:* a re-alignment path (re-transcription or word-level re-anchoring) that restores timed consumers after cardinality-changing corrections, with the unaligned precondition retained as the failure mode for the cases it genuinely cannot handle.
*Reached when:* a correction that inserts/deletes words can subsequently drive `educt_locate`, highlighting, and SRT export — pinned by a test that does exactly that.

**2. Decide and execute the face-recognition build-out.**
*Current condition:* full build-out is explicitly **deferred**; the working core is the two vision-LLM templates (`validate_face_ref`, `match_faces` in `src/templates.rs`), and any expansion is meant to stay on the LLM-template surface (`README.md:379-383`).
*Target condition:* a decision, recorded with the operator, on whether the deferred build-out happens (e.g. better matching prompts, multi-reference voting) and what its acceptance criteria are — then either the implementation with its tests or an explicit "still deferred" with the triggering condition written down.
*Reached when:* the README's deferral note names a decision date and outcome instead of an open deferral.

**3. Type the embedding-error classification.**
*Current condition:* `classify_embedding_error` string-matches credential-missing substrings because `EmbeddingGenerationError` has no typed `NotConfigured` variant yet (`media.md:560`, `kask/crates/hkask-types/src/ports/embedding.rs:30`).
*Target condition:* the typed variant exists and the string-matching is deleted, with the per-variant `permission_denied` classification preserved.
*Reached when:* `classify_embedding_error` matches on variants, and a test pins missing-credential → `permission_denied` without substring matching.

**4. Treat the DNS-rebinding limitation as a decision, not a footnote.**
*Current condition:* the widget's preflight cannot pin opaque yt-dlp/FFmpeg connect-time DNS, so DNS rebinding inside those subprocesses is an explicitly documented transport limitation rather than a claimed guarantee (`README.md:78-85`).
*Target condition:* either a documented accepted-risk ruling from the operator, or a mitigation (e.g. subprocess DNS pinning) with tests.
*Reached when:* the README paragraph states a ruling or a mitigation, not an open limitation.

**The calibration habit, stated once.** For every work item on this list: grasp the current condition from the tree (cited evidence, not vibes), set the target one step beyond it, **record your prediction and confidence before you start**, do the work, measure the gap, and let the operator's score correct your calibration. Your first-month goal is not to be perfectly calibrated — it is to have a measured, scored record of your own estimates for media-server work, which is the only thing that makes your second month's estimates better.

---

**Grounding note.** Research-curation scope was approved by the operator on
2026-10-21. Contracts come from the crate README, media reference, D35, and
bridge registration; the generated/live inventory must be checked on the
integrated tree. The 2026-09-30 test receipt (432 passed, 6 opt-in live Reduct
probes ignored) is historical, not a current validation claim. Current build,
focused tests, and deployment need their own receipts. Not read in depth:
`crates/hkask-media-widget/` (referenced through D18 and README contracts).
