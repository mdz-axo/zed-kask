# Fusion Removal — TODO

## Phase 1 — kask crates

- [x] **T1 — Remove ModelFilterFn** (`language_model`, `agent`, `agent_ui`)
  - [x] `crates/language_model/src/registry.rs`: delete `ModelFilterFn`, `model_filter_fn` field, `set_model_filter_fn`, `passes_model_filter`, filter in `available_models`
  - [x] `crates/agent/src/agent.rs`: remove `passes_model_filter` calls in `refresh_list`; delete `test_refresh_list_drops_models_that_fail_the_model_filter`
  - [x] `crates/agent_ui/src/language_model_selector.rs`: remove filter call
- [x] **T2 — Delete hkask-types fusion types**
  - [x] delete `kask/crates/hkask-types/src/fusion.rs`; remove `pub mod fusion;` from `hkask_types.rs`
  - [x] `template.rs`: remove `bypass_fusion`, `fusion_config` from `LLMParameters`; fix `edge_work`
  - [x] `event.rs`: remove `"reg.fusion"` from CANONICAL_NAMESPACES
  - [x] `regulation.rs`: remove `RegulationSpan::Fusion` variant + `as_str` + `FromStr` + exhaustive-match test (count 22→21)
  - [x] `macros.rs`: update `enum_snake_str!` doc example (FusionMode → SampleMode)
  - [x] `inference_ipc.rs`: remove "fusion" from doc comment
- [x] **T3 — Delete orchestrator + Artificial Analysis** (`hkask-inference`)
  - [x] delete `fusion_orchestrator.rs`, `artificial_analysis.rs`
  - [x] `config.rs`: delete `parse_fusion_config`, `InferenceConfig.fusion`, fusion re-exports, env-var docs
  - [x] `hkask_inference.rs`: remove `pub mod` decls + re-exports + doc lines
  - [x] `chat_protocol.rs`: fix `LLMParameters` test literals
  - [x] `model_constants.rs`: remove fusion doc comment
  - [x] `inference_ipc_client.rs`: remove "fusion" from doc comment
- [x] **T4 — Remove fusion from hkask-templates**
  - [x] `bundle/manifest.rs`: remove `BundleManifest.fusion`, `BundleManifestStep.fusion`
  - [x] `manifest_loader.rs`: remove `ManifestFile.fusion`
  - [x] `executor.rs`: remove `manifest_fusion_config` plumbing, `step.fusion` match, `execute_select` param
  - [x] `tests/yaml_schema_validation.rs`: delete `all_fusion_skill_references_are_valid`
  - [x] executor tests: drop `fusion: None` literals
- [x] **T5 — Remove fusion from kask_bridge**
  - [x] create `model_resolution.rs` with `resolve_model_names` (+ 2 moved unit tests)
  - [x] delete `fusion_model.rs`
  - [x] `inference.rs`: use `resolve_model_names`
  - [x] `settings.rs`: delete `KaskFusionSettings`, `Default`, `to_fusion_config`, `From<KaskFusionSettingsContent>`
  - [x] `skill_executor.rs`: delete judge_model/panel_models context injection
  - [x] `kask_bridge.rs`: remove fusion re-exports + `FavoriteModel`
  - [x] `inference_ipc_server.rs`: remove fusion from doc comments
- [x] **T6 — Remove fusion from MCP servers**
  - [x] `hkask-mcp-corpus`: `embed/service.rs` fusion path, `embed/types.rs` `CorpusConfig.fusion`, `discover/config.rs` default
  - [x] `hkask-mcp-condenser` / `hkask-mcp-kata-kanban` / `compose.rs`: strip fusion fields from `LLMParameters` literals
- [x] **Checkpoint 1** — cargo check kask crates + language_model/agent/agent_ui

## Phase 2 — zed-side

- [x] **T7 — Remove fusion settings section** (`crates/settings_content`)
  - [x] delete `KaskFusionSettingsContent`; remove `KaskSettingsContent.fusion`
- [x] **T8 — Remove fusion settings UI** (`crates/settings_ui`)
  - [x] delete `pages/kask_page/fusion.rs`
  - [x] `kask_page.rs`: remove `mod fusion`, re-export, Fusion SubPageLink, `kask_string_input` fusion arms
  - [x] `models.rs`: remove fusion mentions from settings UI labels
- [x] **T9 — Remove composition wiring** (`crates/zed/src/main.rs`)
  - [x] remove `fusion_config` resolution, discovery block, `FusionLanguageModel` construction, provider registration, auto-favorite block
  - [x] remove `fusion_alert_tx`, `async_cx_for_fusion`, `fusion_configured`
  - [x] `kask.models.default_model` via `resolve_model_names`
- [x] **Checkpoint 2** — cargo check zed + settings + language_model + agent + agent_ui

## Phase 3 — manifests + docs

- [x] **T10 — Clean manifests** (`kask/registry/**/*.yaml`)
  - [x] `memory_remember.yaml`: remove `fusion: true` steps + comments
  - [x] 13 manifests: remove `# Fusion: omitted` comment blocks
  - [x] `classify/hmem-extractor.yaml`: remove fusion comment
  - [x] `task-breakdown/manifest.yaml`: remove "fusion: false" from quality-gate description
  - [x] `create-skill-scaffold.j2`: remove `fusion:` block example + `fusion: false` step field
  - [x] `task-breakdown-quality-gate.j2`: remove "fusion: false" from comment
  - [x] `web-synthesis.yaml`: clarify "full fusion" → "full RRF fusion"
  - [x] `RR-0028.yaml`: remove deleted `fusion_orchestrator.rs` from `discovered_in`
- [x] **T11 — Clean skill companions** (`.agents/skills/**/SKILL.md`)
  - [x] remove `## Fusion Mode` sections + `kask.fusion` references
  - [x] `task-breakdown/SKILL.md`: remove "fusion: false" annotations
- [x] **T12 — Update docs**
  - [x] `DIVERGENCE.md` (D8 FusionLanguageModel, ModelFilterFn entry)
  - [x] `kask/crates/hkask-inference/README.md` (fusion bullet + env vars)
  - [x] `kask/crates/hkask-memory/README.md` (algo/no-judge merge → single-model)
  - [x] `kask/docs/explanation/fusion-mode.md` (deleted)
  - [x] `kask/docs/reference/kask-settings.md` (fusion section)
  - [x] `kask/docs/reference/regulation-spans.md` (reg.fusion row)
  - [x] `kask/docs/diataxis/{hkask-types,kask_bridge,hkask-inference,hkask-templates}/reference.md`
  - [x] `kask/docs/diataxis/hkask-inference/tutorial.md`
  - [x] `kask/docs/architecture/zed-host-architecture-plan.md`
  - [x] `kask/docs/architecture/core/PRINCIPLES.md`
  - [x] `kask/docs/explanation/{README,cognition-and-replica,forecasting-and-scenarios}.md`
  - [x] `kask/docs/README.md`
  - [x] `README.md` (root — remove "fusion" from inference routing description)
- [x] **Checkpoint 3** — repo grep: only intentional fusion matches remain (RRF, historical plans/audits, .rules/GEMINI.md)

## Phase 4 — validation

- [x] **T13 — Validate**
  - [x] cargo check full touched set (hkask-types, kask_bridge, hkask-templates, hkask-inference, settings_ui)
  - [x] targeted tests (hkask-types 85 pass, hkask-templates 120 pass, kask_bridge 94 pass, yaml_schema_validation 4 pass)
  - [x] clippy on touched crates (clean)
  - [ ] PR description `Suggested .rules removals` (pending PR creation)
- [x] **Checkpoint 4** — tests + clippy pass

## Remaining (out of scope — follow-up commit)

- `.rules` / `GEMINI.md` fusion traps (3 entries) — retire in dedicated follow-up per "No drive-by additions"
- Historical docs (`kask/docs/plans/`, `kask/docs/audits/`) — point-in-time records, left as-is
- Operator settings migration — stale `kask-fusion/fusion` + AA-discovered favorites persist in user `settings.json` (manual cleanup)
