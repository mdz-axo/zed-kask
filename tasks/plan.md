# Media Panel Slice 1: Model Browser

## Target Condition

The agent can call `model_list` and `model_info` MCP tools to enumerate available
media generation models (image, video, audio, vision) with their provider,
modality, capabilities, and default status. This fills the unused OMC `Participant`
concept and unblocks model-specific parameter panels for all subsequent media panel
slices.

## Overview

The `hkask-mcp-media` server has 42 registered tools but no way to enumerate
available models. The `Participant` OMC concept is defined in
`hkask-bridge-ontology/src/omc.rs` but no tool maps to it. This slice adds two
tools (`model_list`, `model_info`) that construct model information from the
existing `model_constants` defaults and `MediaOp` capability mapping.

**Approach**: Static construction from `model_constants` — no cross-crate trait
changes. The model list is built from the configured default models (resolvable
via env vars) and their known capabilities. Full dynamic model enumeration
(querying provider APIs) is a follow-up.

## Architecture Decisions

1. **Static model list from `model_constants`** — avoids touching `MediaProvider`
   trait, `ProviderRegistry`, `InferencePort`, and the IPC bridge. The media MCP
   server already imports `hkask_inference::model_constants`. The model list is
   accurate for configured defaults; dynamic enumeration is a future enhancement.

2. **New `tools/models.rs` module** — follows the existing P5 essentialism split
   (`tools/audio.rs`, `tools/gallery.rs`, `tools/generation.rs`,
   `tools/processing.rs`). A new `tools/models.rs` + `models_router` keeps the
   tool surface modular.

3. **`PARTICIPANT` OMC concept** — `model_list` and `model_info` map to
   `omc:Participant`, filling the unused concept. The `omc::tool_to_omc` function
   gets two new arms.

4. **`MediaModelInfo` type in `types.rs`** — the data model for the model browser.
   Fields: `id`, `name`, `provider`, `modality`, `capabilities`, `is_default`,
   `description`. Serialized as JSON in the tool response.

5. **Capability mapping from `MediaOp`** — each model is annotated with which
   `MediaOp`s it supports. The mapping is derived from the provider's `supports()`
   method, but since we're constructing statically, we hardcode the known
   capability sets per model (matching the `impl MediaProvider` blocks).

## Phased Task List

### Phase 1: Foundation (model data type + capability mapping)

- [ ] **T1: Add `MediaModelInfo` type and request structs to `types.rs`**
  - Add `MediaModelInfo` struct with fields: `id`, `name`, `provider`, `modality`,
    `capabilities` (Vec<String>), `is_default` (bool), `description` (Option<String>)
  - Add `ModelListRequest` struct with optional `provider` filter
  - Add `ModelInfoRequest` struct with `model_id` field
  - All derive `Debug, Deserialize, JsonSchema` (MCP tool input pattern)
  - **AC**: Types compile, derive traits are correct, `MediaModelInfo` serializes
    to JSON with all fields present
  - **Verify**: `cargo check -p hkask-mcp-media` passes
  - **Dependencies**: None
  - **Files**: `kask/mcp-servers/hkask-mcp-media/src/types.rs`
  - **Scope**: XS

### Phase 2: Core Tools (model_list + model_info)

- [ ] **T2: Implement `model_list` and `model_info` tools in `tools/models.rs`**
  - Create `kask/mcp-servers/hkask-mcp-media/src/tools/models.rs`
  - `model_list`: constructs `Vec<MediaModelInfo>` from `models::resolve()` for
    each modality (image_gen, video, tts, stt, vision). Each model entry includes
    provider (parsed from model name prefix), modality, capabilities, is_default
    (true — these are the configured defaults). Optional `provider` filter.
  - `model_info`: returns `MediaModelInfo` for a specific `model_id`. Finds the
    model in the list from `model_list` logic.
  - Both tools use `execute_tool_semantic` with `Self::ontology_anchor` (same
    pattern as all other tools)
  - Both tools use `#[tool_router(router = models_router, vis = "pub")]`
  - **AC**: `model_list` returns JSON array of `MediaModelInfo` with ≥4 entries
    (image, video, tts, stt, vision). `model_info` returns a single
    `MediaModelInfo` for a valid model_id, or a `not_found` error for invalid.
  - **Verify**: Unit test: `model_list` returns expected models; `model_info`
    returns correct details; invalid model_id returns error
  - **Dependencies**: T1
  - **Files**: `kask/mcp-servers/hkask-mcp-media/src/tools/models.rs`
  - **Scope**: S

### Phase 3: Wiring (registration + OMC mapping + tests)

- [ ] **T3: Wire tools into the server — router, OMC mapping, tool count test**
  - Add `pub mod models;` to `tools.rs`
  - Add `Self::models_router()` to `combined_router()` in `hkask_mcp_media.rs`
  - Add `"model_list" | "model_info" => Some(PARTICIPANT)` arm to `omc::tool_to_omc`
  - Import `PARTICIPANT` in `omc.rs` (already in `hkask_bridge_ontology::omc`)
  - Update `tool_surface_is_exactly_42_registered_tools` test → 44
  - Add `model_list` and `model_info` to the `ontology_anchor_covers_all_registered_tools`
    test (automatic — the test iterates all registered tools)
  - Add `ontology_anchor_distinguishes_tool_families` test: add `participant` concept
    to the distinct-concepts array
  - **AC**: `combined_router().list_all().len() == 44`. All 44 tools have non-None
    ontology anchors. `model_list` and `model_info` anchor on `PARTICIPANT`.
  - **Verify**: `cargo test -p hkask-mcp-media -- tool_surface ontology_anchor` passes
  - **Dependencies**: T2
  - **Files**: `kask/mcp-servers/hkask-mcp-media/src/tools.rs`,
    `kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs`,
    `kask/mcp-servers/hkask-mcp-media/src/omc.rs`
  - **Scope**: XS

### Checkpoint 1: Tool surface compiles and tests pass

- [ ] All tests pass: `cargo test -p hkask-mcp-media`
- [ ] Clippy clean: `./script/clippy`
- [ ] Tool surface is exactly 44 registered tools
- [ ] All 44 tools have OMC anchors
- [ ] `model_list` and `model_info` anchor on `omc:Participant`

### Phase 4: Behavior Tests

- [ ] **T4: Add behavior tests for model_list and model_info**
  - Test: `model_list` returns ≥4 models (image, video, tts, stt, vision)
  - Test: `model_list` with `provider` filter returns only matching models
  - Test: each model has correct modality (image_gen → "image", tts → "audio", etc.)
  - Test: each model has non-empty capabilities
  - Test: `model_info` with valid model_id returns the correct model
  - Test: `model_info` with invalid model_id returns `not_found` error
  - Test: `model_list` and `model_info` map to `PARTICIPANT` OMC concept
  - **AC**: All behavior tests pass
  - **Verify**: `cargo test -p hkask-mcp-media -- model_list model_info` passes
  - **Dependencies**: T3
  - **Files**: `kask/mcp-servers/hkask-mcp-media/src/tools/models.rs` (test module)
  - **Scope**: S

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Model name prefix parsing is fragile (provider prefix varies) | Medium | Use the same `strip_prefix` pattern from `media_providers.rs` — strip `DeepInfra/` or `OpenRouter/` prefix |
| `model_constants::resolve()` reads env vars at call time — tests may see different models depending on env | Low | Tests assert structural properties (count, modality, capabilities), not specific model names |
| Tool count test is a pin test — must update exactly | Low | Update from 42 to 44 in T3 |
| `PARTICIPANT` import may not be available in `omc.rs` | Low | Already imported: `omc.rs` line 11 imports from `hkask_bridge_ontology::omc` — add `PARTICIPANT` to the import list |

## Open Questions

1. Should `model_list` also list models from `InferencePort::list_models()` (zed's
   chat/vision models)? **Decision: No** — this slice is about *media generation*
   models, not chat/vision models. The vision model is included because
   `describe_image` / `gallery_analyze` use it, but chat models are out of scope.

2. Should the model list be dynamic (querying provider APIs for all available
   models)? **Decision: Not in this slice** — static construction from
   `model_constants` is sufficient for the first vertical slice. Dynamic
   enumeration requires `MediaProvider` trait changes and is a follow-up.

3. Should `model_info` include pricing information? **Decision: No** — local-first,
   user's own API keys. Pricing is provider-specific and changes frequently.

## Refinement History

No refinement needed — plan converged on first iteration. The scope is small
(4 tasks, all in one crate, no cross-crate changes) and the dependency chain is
linear (T1 → T2 → T3 → T4).