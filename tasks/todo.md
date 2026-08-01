# Fusion Removal — TODO

## Phase 1 — kask crates

- [ ] **T1 — Remove ModelFilterFn** (`language_model`, `agent`, `agent_ui`)
  - [ ] `crates/language_model/src/registry.rs`: delete `ModelFilterFn`, `model_filter_fn` field, `set_model_filter_fn`, `passes_model_filter`, filter in `available_models`
  - [ ] `crates/agent/src/agent.rs`: remove `passes_model_filter` calls in `refresh_list`; delete `test_refresh_list_drops_models_that_fail_the_model_filter`
  - [ ] `crates/agent_ui/src/language_model_selector.rs`: remove filter call
- [ ] **T2 — Delete hkask-types fusion types**
  - [ ] delete `kask/crates/hkask-types/src/fusion.rs`; remove `pub mod fusion;` from `hkask_types.rs`
  - [ ] `template.rs`: remove `bypass_fusion`, `fusion_config` from `LLMParameters`; fix `edge_work`
  - [ ] `event.rs`: remove `"reg.fusion"` from CANONICAL_NAMESPACES
- [ ] **T3 — Delete orchestrator + Artificial Analysis** (`hkask-inference`)
  - [ ] delete `fusion_orchestrator.rs`, `artificial_analysis.rs`
  - [ ] `config.rs`: delete `parse_fusion_config`, `InferenceConfig.fusion`, fusion re-exports, env-var docs
  - [ ] `hkask_inference.rs`: remove `pub mod` decls + re-exports + doc lines
  - [ ] `chat_protocol.rs`: fix `LLMParameters` test literals
- [ ] **T4 — Remove fusion from hkask-templates**
  - [ ] `bundle/manifest.rs`: remove `BundleManifest.fusion`, `BundleManifestStep.fusion`
  - [ ] `manifest_loader.rs`: remove `ManifestFile.fusion`
  - [ ] `executor.rs`: remove `manifest_fusion_config` plumbing, `step.fusion` match, `execute_select` param
  - [ ] `tests/yaml_schema_validation.rs`: delete `all_fusion_skill_references_are_valid`
  - [ ] executor tests: drop `fusion: None` literals
- [ ] **T5 — Remove fusion from kask_bridge**
  - [ ] create `model_resolution.rs` with `resolve_model_names` (+ 2 moved unit tests)
  - [ ] delete `fusion_model.rs`
  - [ ] `inference.rs`: use `resolve_model_names`
  - [ ] `settings.rs`: delete `KaskFusionSettings`, `Default`, `to_fusion_config`, `From<KaskFusionSettingsContent>`
  - [ ] `skill_executor.rs`: delete judge_model/panel_models context injection
  - [ ] `kask_bridge.rs`: remove fusion re-exports + `FavoriteModel`
- [ ] **T6 — Remove fusion from MCP servers**
  - [ ] `hkask-mcp-corpus`: `embed/service.rs` fusion path, `embed/types.rs` `CorpusConfig.fusion`, `discover/config.rs` default
  - [ ] `hkask-mcp-condenser` / `hkask-mcp-kata-kanban` / `compose.rs`: strip fusion fields from `LLMParameters` literals
- [x] **Checkpoint 1** — cargo check kask crates + language_model/agent/agent_ui

## Phase 2 — zed-side

- [ ] **T7 — Remove fusion settings section** (`crates/settings_content`)
  - [ ] delete `KaskFusionSettingsContent`; remove `KaskSettingsContent.fusion`
- [ ] **T8 — Remove fusion settings UI** (`crates/settings_ui`)
  - [ ] delete `pages/kask_page/fusion.rs`
  - [ ] `kask_page.rs`: remove `mod fusion`, re-export, Fusion SubPageLink, `kask_string_input` fusion arms
- [ ] **T9 — Remove composition wiring** (`crates/zed/src/main.rs`)
  - [ ] remove `fusion_config` resolution, discovery block, `FusionLanguageModel` construction, provider registration, auto-favorite block
  - [ ] remove `fusion_alert_tx`, `async_cx_for_fusion`, `fusion_configured`
  - [ ] `kask.models.default_model` via `resolve_model_names`
- [x] **Checkpoint 2** — cargo check zed + settings + language_model + agent + agent_ui

## Phase 3 — manifests + docs

- [ ] **T10 — Clean manifests** (`kask/registry/**/*.yaml`)
  - [ ] `memory_remember.yaml`: remove `fusion: true` steps + comments
  - [ ] 13 manifests: remove `# Fusion: omitted` comment blocks
  - [ ] `classify/hmem-extractor.yaml`: remove fusion comment
- [ ] **T11 — Clean skill companions** (`.agents/skills/**/SKILL.md`, 13 files)
  - [ ] remove `## Fusion Mode` sections + `kask.fusion` references
- [ ] **T12 — Update docs**
  - [ ] `DIVERGENCE.md` (D8, ModelFilterFn entry), hkask-inference README, hkask-memory README, zed-host-architecture-plan.md, diataxis reference.md
- [x] **Checkpoint 3** — repo grep: only intentional fusion matches remain

## Phase 4 — validation

- [ ] **T13 — Validate**
  - [ ] cargo check full touched set
  - [ ] targeted tests (`hkask-templates`, `kask_bridge`, `hkask-inference`)
  - [ ] clippy on touched crates
  - [ ] PR description `Suggested .rules removals`
- [x] **Checkpoint 4** — tests + clippy pass
