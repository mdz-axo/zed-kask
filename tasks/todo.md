# Media Panel Slice 1: Model Browser — Todo

> Slice 1 (model browser) is complete — `model_list`/`model_info` ship in the
> server's tool surface. The backlog below is the media viewer/player work
> scoped during the video-viewer session (2026-08-29).

## Media Viewer Backlog (2026-08-29)

- [ ] **V1** Interactive video editing in the viewer
  - [ ] Mark in/out points on the selected asset's transport (buttons + keyboard)
  - [ ] Trim dispatches `video_clip` with the marked range; result surfaces via its `display_hint`
  - [ ] Multi-select in Library/Media → `video_concat`; result surfaces as a new asset
  - [ ] Optional overflow actions: `video_to_gif`, `video_remix`, `video_info`
  - [ ] All dispatches via `shared_tool_invoker` (governed path), errors in the viewer status line
  - [ ] Manual verification: trim the vonnegut clip to 30s and play the result
- [ ] **V2** Save / copy stream affordance on the viewer
  - [ ] Copy asset src (path or URL) to clipboard from the viewer header
  - [ ] "Save" on streamed assets dispatches `video_fetch` (persist to artifacts dir) and surfaces the local copy
  - [ ] `video_info` action showing probe metadata (duration, codec, fps) in the Detail tab
- [ ] **V3** Viewer error recovery / reload
  - [ ] Per-asset retry button on the error state (not just the global refresh)
  - [ ] Retry rebuilds the widget for that asset only (targeted viz-cache eviction by body hash)
  - [ ] Streamed-URL expiry (googlevideo 403) detected and re-resolved on retry
- [ ] **V4** Dead/redundant code refactor (media stack)
  - [ ] Extract `import_media_file` helper in `hkask-mcp-media/src/assets.rs` — `video_fetch`, `gallery_add_video`, `gallery_add_audio` duplicate read→hash→`add_media`→display-hint
  - [ ] Shared yt-dlp binary probing: server `YtDlpRunner::detect` and widget `streaming::newest_yt_dlp_binary` are deliberately duplicated (crates cannot share the dep) — find a shared home (e.g. `hkask-types`) or pin the sync with a cross-reference test
  - [ ] Audit the media server's 60+ tool surface for dead tools (zero steer-prompt mentions + zero viewer dispatches)

## Phase 1: Foundation

- [ ] **T1** Add `MediaModelInfo`, `ModelListRequest`, `ModelInfoRequest` to `types.rs`
  - [ ] `MediaModelInfo` has: id, name, provider, modality, capabilities, is_default, description
  - [ ] All structs derive `Debug, Deserialize, JsonSchema`
  - [ ] `cargo check -p hkask-mcp-media` passes

## Phase 2: Core Tools

- [ ] **T2** Implement `model_list` and `model_info` in `tools/models.rs`
  - [ ] `model_list` returns ≥4 models (image, video, tts, stt, vision)
  - [ ] `model_info` returns single model for valid id, error for invalid
  - [ ] Both use `execute_tool_semantic` + `ontology_anchor` pattern
  - [ ] Both use `#[tool_router(router = models_router, vis = "pub")]`

## Phase 3: Wiring

- [ ] **T3** Wire tools into server
  - [ ] Add `pub mod models;` to `tools.rs`
  - [ ] Add `Self::models_router()` to `combined_router()`
  - [ ] Add `"model_list" | "model_info" => Some(PARTICIPANT)` to `omc::tool_to_omc`
  - [ ] Import `PARTICIPANT` in `omc.rs`
  - [ ] Update tool count test: 42 → 44
  - [ ] Add `participant` to `ontology_anchor_distinguishes_tool_families` test

## Checkpoint 1

- [ ] `cargo test -p hkask-mcp-media` passes
- [ ] `./script/clippy` clean
- [ ] Tool surface is exactly 44
- [ ] All 44 tools have OMC anchors
- [ ] `model_list` / `model_info` anchor on `omc:Participant`

## Phase 4: Behavior Tests

- [ ] **T4** Add behavior tests
  - [ ] `model_list` returns ≥4 models
  - [ ] `model_list` with provider filter works
  - [ ] Each model has correct modality
  - [ ] Each model has non-empty capabilities
  - [ ] `model_info` valid id returns correct model
  - [ ] `model_info` invalid id returns `not_found`
  - [ ] OMC mapping test: both tools → `PARTICIPANT`