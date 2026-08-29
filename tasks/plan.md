# Media Panel Implementation Plan

## Target Condition

A media panel in zed-kask that surfaces the media MCP server's full capability set
through a complete CRUD lifecycle — create, view, transform, delete/curate — with
first-class support for iterative evolution of assets (lineage, remixing,
reproduction, versioning), grounded in the MovieLabs OMC ontology as the structural
scaffold. The panel composes existing MCP tools; it does not replace them.

## Current State

- **Slices 1–11**: ✅ Complete — 67 registered tools (pinned by
  `tool_surface_is_exactly_67_registered_tools`). Widget track W1 (YouTube
  streaming) complete. See §Full-System Verification for the wiring status of
  each slice.
- **Canonical Pattern Audit**: ✅ Complete (see §Evaluation below)
- **Policy: no backward compatibility.** There is no migration path for
  pre-existing DB schemas — schema changes are clean breaks. `init_schema`
  declares the current schema via `CREATE TABLE IF NOT EXISTS` only; no
  `ALTER TABLE` migrations, no column-exists guards, no defensive defaults
  masking missing columns (`image_from_row` propagates a missing
  `media_type` column as an error).

## Evaluation: Canonical Pattern Audit

Before proceeding past Slice 2, I audited the other MCP servers to verify the
media server follows canonical patterns and to identify useful tools/efficiencies
it could leverage. This audit is a permanent evaluation step in the plan — every
future slice must pass through it.

### Audit Method

Surveyed 4 MCP servers: `hkask-mcp-kata-kanban`, `hkask-mcp-corpus`,
`hkask-mcp-curator`, `hkask-mcp-swarm`. Examined: DB storage patterns, passphrase
resolution, core-crate interactions, error handling, self-healing, and shared
service utilities.

### Findings

#### 1. DB Storage — Media server is intentionally different (correct)

| Server | DB Encryption | Passphrase | Pattern |
|---|---|---|---|
| kata-kanban | SQLCipher | `HKASK_DB_PASSPHRASE` via `resolve_db_passphrase` | `open_or_repair` → `SqliteDriver::new_labeled` → `HMemStore::from_driver` |
| corpus | SQLCipher | `HKASK_DB_PASSPHRASE` via `resolve_db_passphrase` | Same as kata-kanban; `OnceLock` for process-wide passphrase |
| curator | SQLCipher | `HKASK_DB_PASSPHRASE` via `resolve_db_passphrase` | `CuratorDb::from_context` → self-healing re-open on failure |
| swarm | SQLCipher (ledger, events, memory) | `HKASK_DB_PASSPHRASE` | `LazyEventStore` / `LazyLocalMemory` — lazy init + self-healing |
| **media** | **Unencrypted SQLite** | **None** | `SqliteDriver::file_pool` → `GalleryStore::from_driver` |

**Assessment**: The media server's unencrypted gallery DB is **intentionally
correct** — gallery metadata (tags, faces, captions) is not a secret, and the
comment at `hkask_mcp_media.rs:453` explicitly documents this: "The file DB is
unencrypted (gallery metadata is not a secret), so it does NOT use
`HKASK_DB_PASSPHRASE` — avoiding leaking the global SQLCipher key to this child
process." No change needed.

#### 2. Passphrase Resolution — Media server correctly does NOT use it

The canonical 2-tier chain (`resolve_db_passphrase` → `ctx.credentials` → env →
keychain) is used by kata-kanban, corpus, curator, and swarm. The media server
correctly does NOT declare `HKASK_DB_PASSPHRASE` as a credential requirement —
it only declares `OPENROUTER_API_KEY`. **No change needed.**

#### 3. Self-Healing Pattern — Media server lacks it (gap, low priority)

The curator and swarm servers use a self-healing pattern: if the DB open fails
transiently, the next tool call re-attempts the open. The media server opens the
gallery DB once at startup and aborts if it fails (no re-open).

**Assessment**: The media server's approach is correct for its use case — a
gallery DB open failure at startup is a hard error (the comment at line 436
explains why: "a gallery DB open failure aborts startup... The fallback silently
degraded every subsequent tool call to 'gallery empty' — a broken feedback
loop"). Self-healing would add complexity for little benefit. **No change
needed**, but noted for future consideration if the gallery DB becomes a
shared resource.

#### 4. `hkask_services_core::ServiceError` — Media server does NOT use it (gap)

The corpus server uses `hkask_services_core::ServiceError` with typed
`ErrorKind` (NotFound, Conflict, Forbidden, BadRequest, ServiceUnavailable) and
`DomainKind` (Storage, Memory, Inference, etc.), then maps to `McpToolError`
via `map_service_error`. The media server uses `McpToolError` directly with
ad-hoc classification functions (`classify_inference_error`,
`classify_embedding_error`, `map_media_error`, `map_gallery_store_error`).

**Assessment**: The media server's ad-hoc error classification is sufficient
for its current scope. Adopting `ServiceError` would add a layer of indirection
without clear benefit — the media server's errors are already correctly
classified at the `McpToolError` boundary. **No change needed now**, but if
the media server grows a service layer (e.g., a `MediaService` that orchestrates
multi-tool workflows), it should adopt `ServiceError` at that point.

#### 5. `HkaskSettings` — Media server does NOT use it (gap, actionable)

The corpus server uses `hkask_services_core::settings::HkaskSettings` for
model resolution — a 3-tier priority: env var > settings.json > hardcoded
default. The media server uses `model_constants::resolve()` which is a 2-tier
chain: env var > hardcoded default. It misses the `settings.json` tier.

**Assessment**: This is a **real gap**. The media server's model resolution
should go through `HkaskSettings` so operators can set model defaults in
`settings.json` without using env vars. However, `HkaskSettings` currently
only has `embedding_model`, `classifier_model`, and `ocr_model` fields — it
would need `image_gen_model`, `video_model`, `tts_model`, `stt_model`, and
`vision_model` fields added. This is a **cross-crate change** (to
`hkask-services-core`) and should be a follow-up task, not a blocker for
Slice 2. **Action: add as a future task in the plan.**

#### 6. `EventStore` — Media server does NOT use it (not applicable)

The swarm server uses `hkask_event_store::EventStore` for rollout tracking
(model requests, verdicts, harness summaries). The media server's job queue
is ephemeral (in-memory `HashMap`) — it doesn't need persistent event logging.

**Assessment**: If the job queue needs to survive restarts (persistent job
history), the `EventStore` pattern is the right fit — it's a lightweight
append-only log with `from_driver` + `EventFilter`. But for now, ephemeral
is correct — jobs are transient, and `gallery_record_generation` already
provides persistent lineage. **No change needed now**, but noted as the
upgrade path if persistent job history is requested.

#### 7. `IdempotencyStore` — Media server does NOT use it (not applicable)

The kata-kanban server uses `IdempotencyStore` for replay protection on
state-changing operations. The media server's generation tools are not
idempotent (generating an image twice produces two different images), so
replay protection doesn't apply. **No change needed.**

#### 8. `LazyLocalMemory` — Media server does NOT use it (not applicable)

The swarm server uses `LazyLocalMemory` for semantic memory with self-healing.
The media server has no semantic memory layer — gallery search uses
embeddings via `InferencePort::embed`, not a memory store. **No change
needed.**

#### 9. `agent_paths::mcp_server_db` — Media server uses it correctly ✅

All servers use `hkask_types::agent_paths::mcp_server_db(server_id, purpose)`
for DB path resolution. The media server uses
`mcp_server_db("media", "gallery")` — correct.

#### 10. `execute_tool_semantic` + `ontology_anchor` — Media server uses it correctly ✅

All servers use `execute_tool_semantic` with an ontology anchor for span
tagging. The media server's `ontology_anchor` → `omc::tool_to_omc` pattern is
the canonical pattern — other servers use similar `tool_to_*` mappings.

### Audit Summary

| Finding | Status | Action |
|---|---|---|
| DB storage pattern | ✅ Correct (intentionally unencrypted) | None |
| Passphrase resolution | ✅ Correct (intentionally not used) | None |
| Self-healing | ⚠️ Gap (low priority) | None — hard-fail at startup is correct for gallery |
| `ServiceError` | ⚠️ Gap (not needed yet) | Adopt if a service layer is added |
| `HkaskSettings` | ❌ Gap (actionable) | **Add as future task** — model resolution should go through settings.json |
| `EventStore` | ✅ Not applicable | None — upgrade path if persistent job history needed |
| `IdempotencyStore` | ✅ Not applicable | None |
| `LazyLocalMemory` | ✅ Not applicable | None |
| `agent_paths` | ✅ Correct | None |
| `execute_tool_semantic` | ✅ Correct | None |

### Audit-Derived Future Task

**T-FUTURE-1: Route media model resolution through `HkaskSettings`**
- Add `image_gen_model`, `video_model`, `tts_model`, `stt_model`,
  `vision_model` fields to `HkaskSettings` in `hkask-services-core`
- Update `models::resolve()` in the media server to use
  `HkaskSettings::load().image_gen_model()` etc. (3-tier: env > settings > default)
- This is a cross-crate change — schedule after Slice 4, not blocking
- **Priority**: P2

---

## Slice 2: Generation Queue (In Progress)

### Target

The agent can submit async generation jobs and track their status. Fills the
`Task` OMC concept with real-time job tracking.

### Architecture Decisions

1. **In-memory `JobStore`** (`Arc<Mutex<HashMap<String, JobRecord>>>`) —
   ephemeral, not persisted. Persistent lineage is already handled by
   `gallery_record_generation` / `gallery_lineage`. The job store is for
   real-time queue visibility only. This matches the audit finding: the
   `EventStore` pattern is the upgrade path if persistence is needed.

2. **`tokio::spawn` for background generation** — `job_submit` spawns a
   background task that calls `vision_port.media_generate`, updates the job
   record on completion. This is the canonical async pattern for the media
   server (the IPC bridge already uses submit+poll for video).

3. **New `tools/jobs.rs` module** — follows the existing P5 essentialism split.
   A new `tools/jobs.rs` + `jobs_router` keeps the tool surface modular.

4. **`JobRecord` type in `types.rs`** — the data model for the job queue.
   Fields: `id`, `op`, `status`, `created_at`, `completed_at`, `result`,
   `error`. Serialized as JSON in the tool response.

5. **New `jobs.rs` module** (not in `tools/`) — holds the `JobStore` type
   and `new_job_store()` constructor. This mirrors the `gallery.rs` module
   pattern (state management separate from tool implementations).

### Tasks

- [x] **T5: Add `JobRecord` and request types to `types.rs`**
  - `JobRecord`: id, op, status, created_at, completed_at, result, error
  - `JobSubmitRequest`: op, params (JSON string)
  - `JobListRequest`: status filter, limit
  - `JobStatusRequest`: job_id
  - `JobCancelRequest`: job_id
  - **Status**: Done

- [x] **T6: Create `jobs.rs` module with `JobStore` type**
  - `JobStore = Arc<Mutex<HashMap<String, JobRecord>>>`
  - `new_job_store()` constructor
  - **Status**: Done

- [x] **T7: Implement `job_submit`, `job_list`, `job_status`, `job_cancel` in `tools/jobs.rs`**
  - `job_submit`: creates job record, spawns background `tokio::spawn` task
  - `job_list`: reads from store, optional status filter, sorted newest-first
  - `job_status`: reads single job by id
  - `job_cancel`: sets status to "cancelled" (background task checks on completion)
  - All use `execute_tool_semantic` + `ontology_anchor` pattern
  - **Status**: Done

- [x] **T8: Wire tools into server**
  - Add `pub mod jobs;` to lib root + `pub mod jobs;` to `tools.rs`
  - Add `job_store: jobs::JobStore` field to `MediaServer` struct
  - Add `Self::jobs_router()` to `combined_router()`
  - Add OMC mapping: `job_submit | job_list | job_status | job_cancel → TASK`
  - Update tool count test: 44 → 48
  - Update `run()` to pass `jobs::new_job_store()`
  - Update `make_server` test helper to pass `jobs::new_job_store()`
  - **Status**: Done

- [ ] **T9: Add behavior tests for job tools**
  - Test: `job_store` starts empty
  - Test: `job_store` insert and get
  - Test: OMC mapping: all 4 job tools → `TASK`
  - **Status**: Tests written, need to verify they pass

### Checkpoint 2: Job queue compiles and tests pass

- [ ] All tests pass: `cargo test -p hkask-mcp-media`
- [ ] Tool surface is exactly 48 registered tools
- [ ] All 48 tools have OMC anchors
- [ ] `job_*` tools anchor on `omc:Task`

---

## Widget Track (Parallel to Server Slices)

The server slices (1–10) add MCP tools to the media server. The widget
track adds rendering and interaction capabilities to the GPUI media widget
(`crates/hkask-media-widget/`). These are parallel concerns — the server
provides the data, the widget renders it.

### W1: Video URL Streaming ✅ Complete

**Commit**: `8097579683` — "Add video stream resolution and DB repair"

**What was delivered**:
- `streaming.rs` — `resolve_stream_url(url)` resolves platform URLs
  (YouTube, Vimeo, etc.) via `yt-dlp -g` on a background thread. Direct
  video file URLs (mp4, webm, m3u8) bypass yt-dlp and stream natively.
  Falls back to the original URL when yt-dlp is missing or fails.
- `media_widget.rs` — `load_resolved` now handles `http(s)://` video URLs
  (previously silently dropped). Added `video_loading` state +
  `load_video_stream_async` for background URL resolution with loading
  indicator in the transport bar.
- `video_decoder.rs` — `VideoPlayer::open_url(&str)` delegates to
  `open(Path::new(url))` — FFmpeg's `avformat_open_input` accepts URL
  strings and selects the http/https protocol handler.
- `Cargo.toml` — Added `ffmpeg-next/build-lib-openssl` to the `vendored`
  feature to enable HTTPS protocol support in the compiled-from-source
  FFmpeg.

**Validation**: 4 streaming tests pass (`detects_direct_video_urls`,
`rejects_platform_page_urls`, `passes_through_direct_video_urls`,
`falls_back_to_original_url_when_yt_dlp_missing`). Full widget test suite:
20 tests pass.

**Usage**: `media` blocks with YouTube URLs now render in the widget:
```
```media
{"kind":"video","src":"https://www.youtube.com/watch?v=4ec0lSd7qH4"}
```

**GPUI thread-safety**: `load_video_stream_async` spawns a background task
via `cx.background_spawn` that calls `streaming::resolve_stream_url` (which
runs `smol::process::Command`). The result is applied on the foreground
thread via `this.update(cx, ...)`. No `block_on` on the foreground thread,
no `tokio::time::Sleep` in foreground tasks.

### W1 Follow-ups (Assessed)

**A. `video_fetch` MCP tool** — **Schedule as Server Slice 11**
A new media MCP server tool that downloads a video from a URL (YouTube,
Vimeo, direct file) to local storage, indexes it in the gallery, and
returns a `media_block("video", &local_path)` display hint. Complements
streaming: stream for immediate viewing, save for persistence.
Infrastructure exists: `FfmpegRunner` for subprocess management,
`persist_generated_asset` for persistence + gallery indexing,
`video_concat` as the pattern for URL-accepting tools. Needs a
`YtDlpRunner` (mirroring `FfmpegRunner`'s detect-and-run pattern) and an
OMC mapping entry. ~50 lines + tests.
**Decision: Schedule as Slice 11** — it bridges the widget and server,
and the infrastructure is ready.

**B. `yt-dlp` as a documented runtime dependency** — **Defer**
The streaming resolver and the hypothetical `video_fetch` tool both
depend on `yt-dlp` at runtime. It's not in the install scripts. Options:
(a) add to install scripts, (b) document as optional in README, (c) leave
undocumented (degrades gracefully — direct URLs work without it).
**Decision: Defer to Slice 11** — when `video_fetch` is built, `yt-dlp`
becomes a documented dependency of that tool. The streaming widget
degrades gracefully without it (direct video URLs work, platform URLs
fail with a clear error). Adding it to install scripts should happen
alongside the `video_fetch` tool, not before.

**C. yt-dlp version and JS runtime** — **Defer (system-config issue)**
The system's `yt-dlp` defaults to the `deno` JS runtime (not installed).
YouTube extraction required `--js-runtimes node` and the `android`
player client (`-f 18`). The `streaming.rs` resolver uses yt-dlp's
defaults.
**Decision: Defer** — this is a system-config issue, not a code issue.
The resolver should NOT hardcode `--js-runtimes node` (it would break on
systems where node isn't installed). If yt-dlp fails, the resolver falls
back to the original URL. When `video_fetch` is built (Slice 11), it can
add a `--js-runtimes` auto-detection step.

**D. DIVERGENCE.md entry** — **Not needed**
The `build-lib-openssl` addition is a zed-kask-side build-config change
in `crates/hkask-media-widget/Cargo.toml` (not `kask/`). It doesn't
modify an upstream file. The `streaming.rs` module and `load_resolved`
fix are entirely in `crates/hkask-media-widget/` (zed-kask-side).
**Decision: No DIVERGENCE.md entry needed** — the changes are all in
zed-kask-side crates, not in `kask/` or upstream Zed files.

**E. End-to-end runtime test** — **Defer (manual verification)**
No runtime test has been done — actually streaming a YouTube video
through the widget in a running Zed-Kask instance. The code compiles,
tests pass, and FFmpeg config confirms HTTPS is enabled.
**Decision: Defer to manual verification** — there's no GPUI test harness
for streaming video through a running widget. The unit tests cover the
URL resolution logic; the FFmpeg HTTPS config is verified at build time.
A runtime test requires a running Zed instance with a YouTube URL.

---

## Future Slices (Outline)

### Slice 3: Video/Audio Gallery Indexing (`Asset` OMC concept)
- `gallery_add_video`, `gallery_add_audio`, `gallery_delete_video`, `gallery_delete_audio`
- Extend `GalleryStore` to index non-image assets
- **Audit check**: Does `GalleryStore` need schema changes? Check before starting.

### Slice 4: Asset Detail View (`Asset` OMC concept)
- Inspector panel with metadata, tags, lineage tree
- Uses existing `gallery_*` tools — no new tools needed, just UI

### Slice 5: Album/Project Organization (`Asset` OMC concept)
- `gallery_create_album`, `gallery_list_albums`, `gallery_move_to_album`, `gallery_delete_album`
- **Audit check**: Should albums use `HMemStore` (like kata-kanban) or extend
  `GalleryStore`? Check before starting.

### Slice 6: Variant Grid (`CreativeWork` OMC concept)
- `generate_variants` tool + grid UI

### Slice 7: Region-Selective Editing (`Version` OMC concept)
- `image_edit_region` tool + mask drawing UI

### Slice 8: Workflow Composer (`Task` OMC concept)
- `workflow_save`, `workflow_load`, `workflow_list`, `workflow_delete`
- **Audit check**: Should workflows use `EventStore` or `HMemStore`? Check before starting.

### Slice 9: Video Timeline Editor (`Sequence` OMC concept)
- Timeline strip UI for existing `video_*` tools

### Slice 10: Audio Editing (`MediaSource` OMC concept)
- `audio_trim`, `audio_concat` — ✅ implemented
- `audio_overdub` — ❌ **not implemented (known gap)**; no code, no tool,
  no test. Do not assume audio editing is complete.

### Slice 11: Video Fetch (`Asset` OMC concept, bridges Widget + Server)
- `video_fetch` tool — downloads a video from a URL (YouTube, Vimeo, direct
  file) to local storage via `yt-dlp`, indexes it in the gallery, returns a
  `media_block("video", &local_path)` display hint
- Needs `YtDlpRunner` (mirrors `FfmpegRunner`'s detect-and-run pattern)
- Uses `persist_generated_asset` for gallery indexing (already supports video)
- OMC mapping: `video_fetch` → `ASSET` (it produces a stored asset)
- Documents `yt-dlp` as a runtime dependency in the install scripts
- **Evaluation Protocol check**: Does `yt-dlp` need a `CredentialRequirement`?
  No — it's a local binary, not an API key. But the tool should surface a
  clear `unavailable` error if `yt-dlp` is not installed (mirroring
  `require_ffmpeg`).
- **Widget integration**: the returned `media_block` uses a local filesystem
  path (bare absolute path; `file://` URLs are also handled by the widget
  since the 2026-08-28 verification pass), so the widget renders it via the
  existing local-file path (no streaming needed). This is the "save for
  persistence" complement to W1's "stream for immediate viewing."

### T-FUTURE-1: Route model resolution through `HkaskSettings`
- Cross-crate change to `hkask-services-core`
- Add media model fields to `HkaskSettings`
- Update `models::resolve()` to use 3-tier priority

---

## Full-System Verification (2026-08-28)

Six verification lenses ran against the completed 11-slice + W1 implementation
(metacognition/inference, UI layout, cybernetic feedback loops, refactor
architecture, agent/swarm discovery, skills). Findings below are grounded in
file:line citations from the lens reports.

### Verification matrix (11 slices × wiring dimensions)

Legend: ✅ wired · ⚠️ partial · ❌ missing. "UI" = dedicated GPUI surface
beyond the inline markdown `MediaWidget`.

| Slice | Server tools | UI | Agent discovery | Skill integration | Feedback loops | Inference | Cross-crate |
|---|---|---|---|---|---|---|---|
| S1 Model browser | ✅ | ❌ | ⚠️ router can prune generic names | ❌ | ✅ | ⚠️ browser was informational-only → now actionable via `model` param | ✅ |
| S2 Job queue | ✅ | ❌ | ⚠️ same | ❌ | ⚠️ panic-orphan fixed; restart loss documented | ✅ | ✅ |
| S3 Video/audio indexing | ✅ | ❌ | ✅ | ❌ | ✅ | n/a | ✅ |
| S4 Asset detail | ✅ | ❌ | ✅ | ❌ | ✅ | n/a | ✅ (FaceRegistryRecord serde verified) |
| S5 Albums | ✅ | ❌ | ✅ | ❌ | ✅ | n/a | ✅ |
| S6 Variants | ✅ | ❌ (no grid renderer) | ✅ | ❌ | ⚠️ display-hint contract fixed | ⚠️ single-image fallback under-delivered → fixed | ✅ |
| S7 Region edit | ✅ | ❌ (no mask canvas) | ✅ | ❌ | ✅ | ✅ mask chain verified end-to-end; OpenRouter excludes ImageToImage and fails loudly | ✅ |
| S8 Workflows | ✅ | ❌ | ⚠️ | ❌ | ✅ | n/a | ✅ |
| S9 Timeline | ✅ | ❌ | ✅ | ❌ | ✅ | n/a | ✅ |
| S10 Audio editing | ⚠️ overdub missing | ❌ | ✅ | ❌ | ✅ | n/a | ✅ |
| S11 Video fetch | ✅ | ⚠️ renders via inline widget | ✅ | ❌ | ⚠️ unavailable classified as generic failure (pre-existing, systemic) | n/a | ✅ (`file://` routing fixed) |

### Systemic findings (cross-slice)

1. **All 9 planned UI surfaces are server-only.** `crates/media_panel` is an
   empty, unregistered scaffold; the only rendering surface is the inline
   markdown `MediaWidget` (single asset + transport + Explain/Disagree).
   N fenced media blocks render as N stacked widgets — no grid, timeline,
   inspector, queue bar, album tree, or mask canvas exists.
2. **Agent-path MCP calls had no regulation instrumentation** — ✅ fixed
   by T-V1 (see New tasks): outcomes now record client-side into the
   `RegulationLedger`. Historically, `reg.tool.*` spans were emitted to
   child stderr and discarded at debug level by zed's context-server client
   (`crates/context_server/src/client.rs:319-327`); outcome recording
   (`RegulationLedger`) only covered the McpRuntime path. The doc comment at
   `tool_span.rs:160-165` still overstates span coverage (the spans remain
   unconsumed — the client-side recording is the fix, not span
   consumption).
3. **`LazyToolRouter` can prune generically-named media tools**
   (`model_list`, `job_status`, `workflow_list`) on non-media-phrased complex
   requests — description-scored, budget 40 across all servers. Exact-name
   mention and skill-active bypass are the recoveries.
4. **No local swarm agent can reach media tools**: every curated card
   declares `"mcp_tools": []`, and local-agent tool defs carry empty
   parameter schemas (`agent_executor.rs:242-246`).
5. **Zero skill-side consumers for the 25 new tools.** `media-workflow`
   references only pre-expansion tools; `logo-builder`'s registry templates
   cite a "SKILL.md §6" that doesn't exist.
6. **`ToolRetryTracker` is result-shaped only** — `job_status` "running"
   polls count as successes (no false death-spiral, but no infinite-poll
   backstop either).

### Remediations executed in this verification pass (P0)

All verified: `cargo test -p hkask-mcp-media` (89 lib + 63 schema + 1 doc),
`-p hkask-storage --lib gallery` (20), `-p hkask-inference --lib media` (10),
`-p hkask-types --lib` (18), `-p hkask-media-widget` (20) — all pass.
Clippy clean for all touched crates (one pre-existing `redundant_clone` in
`hkask-storage/src/core/connection.rs:172` from parallel in-flight keystore
work remains, unrelated to this pass).

1. **`image_to_video` silently discarded its `model` param**
   (`tools/processing.rs` destructured `model: _model`). Added
   `MediaGenerateParams.model` (`hkask-types`) and a provider-override path
   in every DeepInfra + OpenRouter `execute` arm; the tool now passes `model`
   through. The model browser is actionable for this op.
2. **`generate_variants` violated the display-hint contract** — hints were
   nested at `variants[].display_hint`, outside the documented top-level
   `display_hints` array. Now emits both; the single-image fallback also
   issues additional calls (capped at `count`) instead of returning 1 variant
   regardless of `count`.
3. **`YtDlpRunner` returned `MediaError::FfmpegUnavailable`** for missing
   yt-dlp (operator saw "ffmpeg not available"). Added
   `MediaError::YtDlpUnavailable` → `unavailable`.
4. **Job panic-orphan**: a panic/abort in the spawned generation task left
   the record "running" forever. Added `JobPanicGuard` (drop-guard marks the
   job failed unless defused on normal completion). `job_status` `not_found`
   now explains restart loss vs. bad ID.
5. **`file://` media blocks were dead code** — `PathMediaStorage::resolve`
   misrouted them to the failing plain-path branch.
   `crates/hkask-media-widget/src/media_ref.rs` now strips `file://` and
   resolves the underlying path.
6. **Schema migration affordances removed** (per the no-backward-compat
   policy): dropped the `media_type` `ALTER TABLE` + column guard;
   `image_from_row` now hard-reads the column.
7. **Schema-compliance tests now cover all 25 new request structs**
   (63 total, up from 38).
8. **IPC wire protocol dropped `mask` / `model` (found 2026-08-28 while
   completing the media-panel takeover)** — `InferenceParams` had no
   `media_mask` / `media_model` fields, so in production (media server as
   a child process) `image_edit_region`'s mask silently degraded to
   whole-image editing and the per-call model override was dropped at the
   socket boundary; the in-process `MediaRouter` path was unaffected,
   which is why per-crate tests passed. Fixed end-to-end:
   `InferenceParams.media_mask`/`media_model` added, the IPC client maps
   them from `MediaGenerateParams`, the zed-side dispatch reconstructs
   them. Pinned by `kask_bridge` `dispatch_media_generate_threads_mask_and_model`.

### New tasks discovered (not yet done)

- **T-V1 (P1, systemic)**: ✅ **Done (2026-08-28)** — agent-path MCP tool
  outcomes now flow into the regulation loop.
  `ContextServerTool::run` wraps `run_inner` to record every outcome
  (server name, tool name, success, error text) via a process-global
  re-settable hook (`agent::set_mcp_tool_outcome_recorder` /
  `record_mcp_tool_outcome`, Mutex slot), wired in `main.rs` to the shared
  `RegulationLedger::record_outcome` on the GPUI-global tokio runtime.
  Domain is the MCP server name, matching `McpRuntime::invoke`, so both
  dispatch paths aggregate into the same reliability domain — the
  `ToolReliabilitySensor` and curator now see agent-initiated MCP calls.
  Pinned by `agent::internal_tests::mcp_outcome_recorder_records_and_is_replaceable`
  + `context_server_registry::tests::{test_mcp_run_outcome_maps_success,
  test_mcp_run_outcome_maps_error_text,
  test_mcp_run_outcome_empty_error_text_falls_back}`. D-seam documented in
  DIVERGENCE.md (Other zed-kask-modified files). The `reg.tool.*` stderr spans
  remain unconsumed on this path — the outcome is recorded client-side
  instead, which is the stronger signal (it observes the actual result, not
  the server's self-report).
- **T-V2 (P1)**: Classify `unavailable` (not-configured: yt-dlp/ffmpeg
  missing) distinctly from real failures in `ToolRetryTracker` and
  `ToolReliabilitySensor`, so environment gaps don't pollute reliability
  domains or trigger retry death-spirals.
- **T-V3 (P1)**: Video Explain dispatch mismatch — `omc:Asset → gallery_analyze`
  hands a `.mp4` path as `image_url` (`media_widget.rs:640-655`). Route video
  blocks to a video-appropriate explain path.
- **T-V4 (P2)**: UI surfaces — ⚠️ **partially done (2026-08-28)**: a
  **Steer-only media panel** landed (`crates/media_panel/`, modeled on the
  portfolio panel): chat-driven CRUD scoped to the `media` MCP server,
  status bar button, View menu entry, tool advertisement verified against
  the generated `TOOL_NAMES` via `ensure_steer`. This is the chat-driven
  variant, not the 3-zone visual panel — variant grid, asset inspector,
  queue bar, model browser grid, album tree, timeline, workflow composer,
  and mask canvas remain unbuilt (the inline `MediaWidget` still renders
  single assets in stacked fenced blocks). Priority order for the visual
  surfaces: variant grid, asset inspector, queue bar, model browser, album
  tree, timeline, workflow composer, mask canvas (largest new build).
- **T-V5 (P2)**: Skill updates — `media-workflow` should add `generate_variants`,
  `image_edit_region`, `video_fetch`→`video_info`→`video_to_gif`, audio
  pipeline, workflow-composer flow, album outputs, `job_*` async pattern;
  `logo-builder` needs its missing §6 (model selection via
  `model_list`/`model_info`) that two registry templates already cite.
- **T-V6 (P2)**: Local agent cards — declare media `mcp_tools` on at least one
  curated card; fix empty parameter schemas in `agent_executor.rs`.
- **T-V7 (P2)**: Register `hkask-media-widget` on the divergence surface
  (D18 lists 4 widgets, code ships 5 — audit finding F3,
  `kask/docs/plans/architecture-audit-2026-08-26.md:176-178`).
- **T-V8 (P3)**: `tools/workflows.rs` fails the strict deletion test (4
  1:1 passthrough tools, no module-local helpers) — fold into
  `tools/gallery.rs` or leave as domain grouping; judgment call.
- **T-V9 (P3)**: SVG renders via `img()` not `svg()` (`media_widget.rs:747`),
  contradicting `media_ref.rs:13` doc; duplicate `lang == "media"` in the
  D18 gate (`markdown.rs:2723`).
- **T-FUTURE-1: ✅ Done (2026-08-28)** — media model resolution now has the
  settings tier: `KaskMediaSettingsContent` (settings.json `kask.media`)
  → `KaskMediaSettings` + `From` impl → `emit_media_env()` in `mcp_env()`
  → `HKASK_MEDIA_{TTS,STT,VISION,IMAGE_GEN,VIDEO}_MODEL` env vars → the
  server's `models::resolve` / provider `model_constants::resolve`.
  Effective resolution is 3-tier: settings.json > env var > default. The
  settings UI page (`settings_ui` `kask_page/media.rs`) reads from
  `kask_bridge::KaskMediaSettings` per the canonical pattern (no
  `hkask-mcp-media` dependency in `settings_ui`); model constants are
  re-exported from `kask_bridge`.

---

## Evaluation Protocol (for every future slice)

Before starting each future slice, run this checklist:

1. **DB pattern**: Does the new feature need a DB? If yes, should it use
   `GalleryStore` (unencrypted), `HMemStore` (encrypted via
   `HKASK_DB_PASSPHRASE`), or `EventStore` (append-only log)?
2. **Passphrase**: Does the new feature handle secrets? If yes, use
   `resolve_db_passphrase` — never inline env-var reads.
3. **Error handling**: Does the new feature need typed errors? If it
   orchestrates multiple tools, consider `ServiceError` with `ErrorKind` +
   `DomainKind`.
4. **Model resolution**: Does the new feature resolve model names? Use
   `model_constants::resolve()` for now; plan migration to `HkaskSettings`.
5. **Self-healing**: Does the new feature open a DB that might fail
   transiently? Consider the `LazyLocalMemory` / `CuratorDb` self-healing
   pattern.
6. **Path resolution**: Use `agent_paths::mcp_server_db(server_id, purpose)`
   for DB paths, `agent_paths::resolve_under_data_dir` for other paths.
7. **Tool pattern**: Use `execute_tool_semantic` + `ontology_anchor` for
   every tool. Add the OMC mapping in `omc::tool_to_omc`. Update the tool
   count test.
8. **Credential requirements**: Declare every API key the server needs in
   `CredentialRequirement::optional` / `required`. Missing credentials surface
   as `permission_denied`, not silent fallback.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| In-memory job store loses jobs on restart | Low | By design — persistent lineage is in `gallery_record_generation`. `job_status` `not_found` now explains restart loss explicitly. `EventStore` is the upgrade path. |
| `tokio::spawn` background task panics | Medium | **Mitigated (2026-08-28)** — `JobPanicGuard` drop-guard marks the job failed on panic/abort. |
| Job store lock contention under high load | Low | `Mutex` is held briefly (insert, update, read). No long-held locks. |
| `HkaskSettings` migration is a cross-crate change | Medium | Schedule as T-FUTURE-1 after Slice 4. Not blocking. |
| yt-dlp not installed / wrong JS runtime | Low | Streaming degrades gracefully (direct URLs work, platform URLs fail with clear error). `video_fetch` (Slice 11) will document yt-dlp as a dependency. |
| FFmpeg HTTPS protocol not compiled | Medium | Resolved in W1 — `build-lib-openssl` added to vendored feature. Verified at build time. |
| No runtime test for YouTube streaming | Low | Unit tests cover URL resolution logic. Runtime test requires manual verification with a running Zed instance. |

## Refinement History

1. **Canonical Pattern Audit** (after Slice 2 implementation): Audited 4 MCP
   servers. Found 1 actionable gap (`HkaskSettings` model resolution), 2
   low-priority gaps (self-healing, `ServiceError`), and confirmed the media
   server's intentional deviations (unencrypted DB, no passphrase) are
   correct. Added T-FUTURE-1 and the Evaluation Protocol checklist.

2. **Widget Track Integration** (after Slice 4): Integrated the completed
   YouTube streaming work (commit `8097579683`) as Widget Slice W1. Added a
   parallel "Widget Track" section to the plan. Assessed 5 follow-ups
   (A–E): scheduled `video_fetch` as Server Slice 11, deferred yt-dlp
   dependency documentation to Slice 11, deferred JS-runtime detection as a
   system-config issue, confirmed no DIVERGENCE.md entry needed, deferred
   runtime test to manual verification. Added 3 new risks (yt-dlp, FFmpeg
   HTTPS, runtime test gap).

3. **Full-System Verification** (2026-08-28, after Slices 1–11 + W1): Ran 6
   verification lenses (metacognition/inference, UI layout, cybernetics,
   refactor architecture, agent/swarm discovery, skills) over the completed
   67-tool surface. Executed 7 P0 remediations (model-param wiring,
   variants display-hint contract + count fallback, YtDlpUnavailable error
   variant, job panic guard, `file://` widget routing, migration-affordance
   removal per the no-backward-compat policy, schema-compliance coverage for
   all new request structs). Recorded the verification matrix, systemic
   findings, and 9 new tasks (T-V1–T-V9) in §Full-System Verification.