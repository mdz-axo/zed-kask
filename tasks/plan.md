# Fusion System Removal — Plan

- **Creator**: zed agent (task-breakdown skill, v0.31.0)
- **Date**: 2026-08-01
- **Status**: Planned
- **bibo:Document** — plan.md governs the fusion deprecation removal
- **pko:ProcedureTarget** — zed-kask fusion subsystem (all surfaces: settings, orchestrator, router, discovery, manifests, docs)

## Overview

The fusion system (multi-model panel deliberation) is deprecated and must be
removed entirely. The removal collapses all inference onto the single-model
path — which already exists as the fallback in `crates/zed/src/main.rs`
(`fusion_model.clone().unwrap_or_else(|| ...)` resolves `kask.models.default_model`
or zed's default model). Deletion test (deep-module): every fusion component is
a wrapper over single-model inference; removing it leaves the single-model path,
so complexity vanishes → DELETE.

## Architecture decisions

1. **The generic model-name resolver survives, renamed.** `resolve_fusion_models`
   is a generic registry resolver (provider-prefixed name → `Arc<dyn LanguageModel>`)
   with two non-fusion consumers: `LanguageModelInferencePort::generate_with_model`
   (`kask_bridge/src/inference.rs`, model-override resolution for MCP servers) and
   `main.rs` (`kask.models.default_model` resolution). It moves to
   `kask_bridge/src/model_resolution.rs` as `resolve_model_names` with its two
   resolver unit tests. Everything else in `fusion_model.rs` (FusionLanguageModel,
   MultiModelInferencePort, FusionProviderState, FusionLanguageModelProvider,
   favorites) dies.
2. **`bypass_fusion` / `fusion_config` on `LLMParameters` are write-only** — never
   read anywhere in production (the fusion model doesn't consult them; the
   MediaRouter/IPC path ignores them). Removing the fields is safe once all
   writers (executor, fusion orchestrator, MCP server literals) are cleaned.
3. **`ModelFilterFn` (prompt pricing / intelligence filter) is dead infrastructure** —
   `set_model_filter_fn` has no production caller (only a test). It dies with its
   call sites in `crates/agent/src/agent.rs`, `crates/agent_ui/src/language_model_selector.rs`,
   and `crates/language_model/src/registry.rs`.
4. **`NonEmptyVec`, `ConvergenceVerdict`, `FusionMode`, `AlgoMethod`, `FusionSkill`
   die with `hkask-types/src/fusion.rs`** — no external consumers.
5. **RRF fusion in `hkask-mcp-research` is unrelated** (Reciprocal Rank Fusion,
   search ranking) — explicitly out of scope, do not touch.
6. **kask_bridge depends on zed crates; `crates/zed` will not compile between
   Phase 1 and Phase 2.** Phase-1 checkpoint covers kask crates only; the zed
   binary is fixed in Phase 2.
7. **Operator config migration**: previously-written `agent.favorite_models`
   entries (`kask-fusion/fusion` + discovered favorites) persist in the user's
   settings.json after removal. Code-side: the auto-favorite write is deleted;
   the stale settings entries are an operator-side cleanup (documented, not
   auto-migrated).
8. **`.rules`/`GEMINI.md` fusion entries become stale.** Per rules hygiene, they
   are NOT edited inline; a `Suggested .rules removals` note goes in the PR
   description.

## Dependency graph

| # | Node | depends_on | depth | notes |
|---|------|-----------|-------|-------|
| 1 | `hkask-types` (fusion module, LLMParameters fields) | — | 0 | Foundation; breaks everything below until consumers cleaned |
| 2 | `hkask-inference` (orchestrator, artificial_analysis, config) | 1 | 1 | Deletes `fusion_orchestrator.rs` + `artificial_analysis.rs` |
| 3 | `hkask-templates` (manifest fusion fields, executor) | 1 | 1 | `BundleManifest.fusion`, `BundleManifestStep.fusion`, executor plumbing |
| 4 | `kask_bridge` (fusion_model, settings, skill_executor) | 1,2,3 | 2 | Resolver extraction + deletion of fusion_model.rs |
| 5 | MCP servers (corpus fusion path, LLMParameters literals) | 1,4 | 3 | corpus `embed/service.rs`, condenser, kata-kanban |
| 6 | `crates/language_model` + `agent` + `agent_ui` (ModelFilterFn) | — | 0 | Independent; do first to unblock kask_bridge check |
| 7 | `crates/settings_content` + `settings_ui` (fusion settings + page) | 1 | 1 | KaskFusionSettingsContent, fusion page |
| 8 | `crates/zed/src/main.rs` (composition root) | 4,6,7 | 3 | Largest removal site |
| 9 | Manifests + SKILL.md + docs | — | 0 | No compile impact; memory_remember.yaml semantic change |

## Phases, tasks, checkpoints

### Phase 1 — kask crates (foundation sweep)

**T1 — Remove ModelFilterFn from the language_model crate**
- slice: deprecate/pricing-filter
- files: `crates/language_model/src/registry.rs`, `crates/agent/src/agent.rs`, `crates/agent_ui/src/language_model_selector.rs`
- AC: no `ModelFilterFn` / `model_filter_fn` / `passes_model_filter` / `set_model_filter_fn` references remain; `agent` `refresh_list` and selector filter calls removed; `test_refresh_list_drops_models_that_fail_the_model_filter` deleted.
- verification: `cargo check -p language_model -p agent -p agent_ui`
- deps: None

**T2 — Delete fusion types from hkask-types**
- slice: deprecate/types
- files: `kask/crates/hkask-types/src/fusion.rs` (delete), `hkask_types.rs` (module decl), `template.rs` (LLMParameters fields + `edge_work`), `event.rs` (`reg.fusion`)
- AC: `fusion` module deleted; `bypass_fusion`/`fusion_config` removed from `LLMParameters`; `reg.fusion` removed from CANONICAL_NAMESPACES; no `hkask_types::fusion` references remain.
- verification: `cargo check -p hkask-types` (after T3–T5 land, see checkpoint)
- deps: None (temporarily breaks dependents — coordinated with T3–T5)

**T3 — Delete the fusion orchestrator and Artificial Analysis discovery**
- slice: deprecate/orchestrator
- files: `kask/crates/hkask-inference/src/fusion_orchestrator.rs` (delete), `artificial_analysis.rs` (delete), `config.rs` (parse_fusion_config, `InferenceConfig.fusion`, re-exports, doc comments), `hkask_inference.rs` (module decls, re-exports, doc), `chat_protocol.rs` (test literals)
- AC: both source files deleted; no `FusionConfig`/`FusionMode`/`orchestrate`/`discover_favorites`/`FavoriteModel` references in the crate; `InferenceConfig` retains only media/provider fields.
- verification: `cargo check -p hkask-inference`
- deps: T2

**T4 — Remove fusion from hkask-templates manifests + executor**
- slice: deprecate/manifest-fields
- files: `kask/crates/hkask-templates/src/bundle/manifest.rs`, `manifest_loader.rs`, `executor.rs`, `tests/yaml_schema_validation.rs`, executor test literals (`fusion: None`)
- AC: `BundleManifest.fusion` and `BundleManifestStep.fusion` removed; `manifest_fusion_config` plumbing and `step.fusion` match removed from `execute_select`/`execute_manifest`; fusion yaml-schema test removed.
- verification: `cargo check -p hkask-templates`
- deps: T2

**T5 — Remove fusion from kask_bridge (resolver extraction + deletion)**
- slice: deprecate/bridge
- files: `kask_bridge/src/fusion_model.rs` (delete), `model_resolution.rs` (new — `resolve_model_names` moved from fusion_model.rs + 2 unit tests), `inference.rs` (resolver call site), `settings.rs` (KaskFusionSettings, Default, to_fusion_config, From impl), `skill_executor.rs` (judge_model/panel_models context injection), `kask_bridge.rs` (re-exports)
- AC: `fusion_model.rs` deleted; `resolve_model_names` lives in `model_resolution.rs` and `LanguageModelInferencePort` model-override resolution still works; no `KaskFusionSettings`/`FusionLanguageModel` references in the crate.
- verification: `cargo check -p kask_bridge`
- deps: T2, T3, T4, T1

**T6 — Remove fusion from MCP servers**
- slice: deprecate/mcp
- files: `kask/mcp-servers/hkask-mcp-corpus/src/corpus/embed/service.rs` (fusion path), `corpus/embed/types.rs` (CorpusConfig.fusion), `corpus/discover/config.rs`, `hkask-mcp-condenser/src/hkask_mcp_condenser.rs` (literals), `hkask-mcp-kata-kanban/src/kata.rs` (literals), `hkask-mcp-corpus/src/compose.rs` (literals)
- AC: corpus triple extraction runs single-model only; no `fusion_config`/`bypass_fusion` literals remain.
- verification: `cargo check -p hkask-mcp-corpus -p hkask-mcp-condenser -p hkask-mcp-kata-kanban`
- deps: T2, T5

**Checkpoint 1** — `cargo check -p hkask-types -p hkask-inference -p hkask-templates -p kask_bridge -p hkask-mcp-corpus -p hkask-mcp-condenser -p hkask-mcp-kata-kanban -p language_model -p agent -p agent_ui` passes. Known-broken: `crates/zed` (fixed in Phase 2).

### Phase 2 — zed-side (settings, UI, composition root)

**T7 — Remove the fusion settings section**
- slice: deprecate/settings
- files: `crates/settings_content/src/settings_content.rs`
- AC: `KaskFusionSettingsContent` deleted; `KaskSettingsContent.fusion` field removed; no `kask.fusion` JSON schema surface.
- verification: `cargo check -p settings_content`
- deps: T2

**T8 — Remove the fusion settings UI page**
- slice: deprecate/settings-ui
- files: `crates/settings_ui/src/pages/kask_page/fusion.rs` (delete), `crates/settings_ui/src/pages/kask_page.rs` (mod decl, re-export, SubPageLink, `kask_string_input` fusion arms)
- AC: fusion page file deleted; kask settings page has no Fusion entry; `kask_string_input` no longer writes `kask.fusion`.
- verification: `cargo check -p settings_ui`
- deps: T7

**T9 — Remove the fusion composition wiring from main.rs**
- slice: deprecate/composition-root
- files: `crates/zed/src/main.rs`
- AC: `fusion_config`, `async_cx_for_fusion`, `fusion_configured`, auto-discovery block, `FusionLanguageModel` construction, `FusionLanguageModelProvider` registration, auto-favorite block, `fusion_alert_tx` all removed; `kask.models.default_model` resolves via `resolve_model_names`; inference falls back to the single model.
- verification: `cargo check -p zed`
- deps: T1, T5, T7, T8

**Checkpoint 2** — `cargo check -p zed -p settings_ui -p settings_content -p language_model -p agent -p agent_ui` passes.

### Phase 3 — manifests + docs

**T10 — Remove fusion from skill manifests**
- slice: deprecate/manifests
- files: `kask/registry/manifests/memory_remember.yaml` (`fusion: true` steps + comments), `kask/registry/manifests/{bug-hunt,capabilities-reasoner,create-skill,deep-module,diagnose,essentialist,falsifiability,gradient-hunter,grill-me,idiomatic-rust,prompt-enhance,refactor-architecture,scenario-builder}.yaml` (`# Fusion: omitted` comment blocks), `kask/registry/classify/hmem-extractor.yaml` (fusion comment)
- AC: no `fusion` token remains in any `kask/registry/**/*.yaml`; memory_remember extraction no longer references the algo judge.
- verification: `grep -ri fusion kask/registry --include=*.yaml` → no hits
- deps: None

**T11 — Remove Fusion Mode sections from skill companions**
- slice: deprecate/skill-docs
- files: `.agents/skills/{bug-hunt,capabilities-reasoner,deep-module,diagnose,essentialist,falsifiability,grill-me,idiomatic-lisp,idiomatic-rust,lean-prover,lora-training,refactor-architecture,task-breakdown}/SKILL.md`
- AC: no `kask.fusion` / `HKASK_FUSION` / `## Fusion Mode` references remain in `.agents/skills/**/SKILL.md`; methodology sections (PDCA loops, convergence) retain their non-fusion meaning.
- verification: `grep -ri "fusion" .agents/skills --include=SKILL.md` → no hits
- deps: None

**T12 — Update fork docs**
- slice: deprecate/docs
- files: `DIVERGENCE.md` (D8 row, ModelFilterFn entry), `kask/crates/hkask-inference/README.md`, `kask/crates/hkask-memory/README.md`, `kask/docs/architecture/zed-host-architecture-plan.md`, `kask/docs/diataxis/hkask-inference/reference.md`
- AC: no fusion references in these docs; D8 lists `resolve_model_names` instead of `FusionLanguageModel`; ModelFilterFn removed from the modified-files list.
- verification: `grep -ri fusion DIVERGENCE.md kask/docs kask/crates/hkask-inference/README.md kask/crates/hkask-memory/README.md` → no hits
- deps: None

**Checkpoint 3** — repo-wide `grep -ri fusion` returns only intentional matches (`.rules`, `GEMINI.md`, `hkask-mcp-research` RRF, historical docs).

### Phase 4 — validation

**T13 — Validate the removal**
- slice: deprecate/validation
- files: none (verification only)
- AC: touched kask crates + zed compile; `reg.fusion` span emitters gone; no runtime reference to `kask.fusion` remains.
- verification: `cargo check` for the full touched set; `cargo test -p hkask-templates -p kask_bridge -p hkask-inference` (targeted); `./script/clippy` on touched crates.
- deps: all

**Checkpoint 4** — targeted tests + clippy pass. PR description carries `Suggested .rules removals` for the obsolete fusion entries.

## Risks

| Risk | Impact | Mitigation |
|------|--------|-----------|
| `resolve_fusion_models` deleted instead of relocated → MCP model_override breaks | MCP servers (condenser/corpus/training) use wrong model silently | Extract to `model_resolution.rs` with unit tests moved; verify `LanguageModelInferencePort` override path |
| crates/zed broken between phases | no incremental check of zed | Checkpoint 1 scoped to kask crates; zed fixed in T9 |
| `fusion: true` removed from memory_remember.yaml changes memory extraction behavior | memory extraction silently single-model | Documented; single-model is the intended post-fusion behavior |
| Stale `kask-fusion/fusion` favorite persists in user settings.json | dead entry in model picker | Documented operator-side cleanup (not auto-migrated) |
| RRF fusion in research MCP touched by blanket grep | search ranking broken | Explicitly excluded in T10/T11/T12 scope |
| `.rules`/`GEMINI.md` fusion guidance becomes stale | future agents follow dead guidance | PR description `Suggested .rules removals` (rules hygiene — no inline edit) |
| `reg.fusion` removed from CANONICAL_NAMESPACES while an emitter survives | unregistered span target (silent) | All `reg.fusion` emitters deleted in T2/T3/T5; grep-verified |

## Open questions

1. **Operator settings migration** — previously auto-favorited models (`kask-fusion/fusion` + Artificial Analysis discoveries) persist in the user's `settings.json`. Should the removal include a settings migration that prunes `agent.favorite_models` entries with provider `kask-fusion` or the discovered OpenRouter models? (Not code-required; recommended as a manual operator step.)
2. **`reg.fusion` namespace** — confirmed all emitters are deleted. If any regulation consumer watches `reg.fusion` for alerting, it will silently see nothing; the span namespace removal is correct.
3. **corpus fusion config** — `CorpusConfig.fusion` (yaml `fusion:` block in corpus config files) is removed; existing corpus.yaml files with a `fusion:` block will ignore the unknown field (serde default). Operator cleanup recommended.

## Refinement History

- Iteration 1 (initial): decomposed into 13 tasks across 4 phases. Refinements applied from survey findings: (a) resolver survival extracted as explicit task T5 (sizing/red-flag — a naive "delete fusion_model.rs" would have broken MCP model-override); (b) ModelFilterFn moved to its own task T1 (red-flag — it lives in upstream crates and needs agent/agent_ui call-site removal); (c) RRF fusion exclusion made explicit in risks (red-flag — blanket grep would hit `hkask-mcp-research`).
