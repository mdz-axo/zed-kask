# Media Panel Implementation Plan

## Target Condition

A media panel in zed-kask that surfaces the media MCP server's full capability set
through a complete CRUD lifecycle — create, view, transform, delete/curate — with
first-class support for iterative evolution of assets (lineage, remixing,
reproduction, versioning), grounded in the MovieLabs OMC ontology as the structural
scaffold. The panel composes existing MCP tools; it does not replace them.

## Current State

- **Slice 1 (Model Browser)**: ✅ Complete — `model_list` + `model_info` tools
  fill the `Participant` OMC concept. 44 registered tools.
- **Slice 2 (Generation Queue)**: 🔄 In progress — `job_submit`, `job_list`,
  `job_status`, `job_cancel` tools fill the `Task` OMC concept. 48 registered tools.
- **Canonical Pattern Audit**: ✅ Complete (see §Evaluation below)

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
- `audio_trim`, `audio_concat`, `audio_overdub`

### T-FUTURE-1: Route model resolution through `HkaskSettings`
- Cross-crate change to `hkask-services-core`
- Add media model fields to `HkaskSettings`
- Update `models::resolve()` to use 3-tier priority

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
| In-memory job store loses jobs on restart | Low | By design — persistent lineage is in `gallery_record_generation`. `EventStore` is the upgrade path. |
| `tokio::spawn` background task panics | Medium | The task updates the job record on both success and failure; a panic leaves the job in "running" state. Future: add `tokio::spawn` with `catch_unwind` or a timeout. |
| Job store lock contention under high load | Low | `Mutex` is held briefly (insert, update, read). No long-held locks. |
| `HkaskSettings` migration is a cross-crate change | Medium | Schedule as T-FUTURE-1 after Slice 4. Not blocking. |

## Refinement History

1. **Canonical Pattern Audit** (after Slice 2 implementation): Audited 4 MCP
   servers. Found 1 actionable gap (`HkaskSettings` model resolution), 2
   low-priority gaps (self-healing, `ServiceError`), and confirmed the media
   server's intentional deviations (unencrypted DB, no passphrase) are
   correct. Added T-FUTURE-1 and the Evaluation Protocol checklist.