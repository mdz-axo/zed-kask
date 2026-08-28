# Media Panel Slice 1: Model Browser — Todo

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