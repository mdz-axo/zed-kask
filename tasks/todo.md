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
- [x] **T3 — Delete orchestrator + Artificial Analysis** (`hkask-inference`)
  - [x] delete `fusion_orchestrator.rs`, `artificial_analysis.rs`
  - [x] `config.rs`: delete `parse_fusion_config`, `InferenceConfig.fusion`, fusion re-exports, env-var docs
  - [x] `hkask_inference.rs`: remove `pub mod` decls + re-exports + doc lines
  - [x] `chat_protocol.rs`: fix `LLMParameters` test literals
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
- [x] **T9 — Remove composition wiring** (`crates/zed/src/main.rs`)
  - [x] remove `fusion_config` resolution, discovery block, `FusionLanguageModel` construction, provider registration, auto-favorite block
  - [x] remove `fusion_alert_tx`, `async_cx_for_fusion`, `fusion_configured`
  - [x] `kask.models.default_model` via `resolve_model_names`
- [x] **Checkpoint 2** — cargo check zed + settings + language_model + agent + agent_ui

## Phase 3 — manifests + docs

- [x] **T10 — Clean manifests** (`kask/registry/**/*.yaml`)
  - [x] `memory_remember.yaml`: remove `fusion: true` steps + comments
  - [x] 15 manifests: remove `# Fusion:` comment blocks (incl. superforecasting, swarm-intelligence)
  - [x] remove `fusion: false` step fields (scenario-builder, superforecasting, task-breakdown)
  - [x] `classify/hmem-extractor.yaml`: remove fusion comment
- [x] **T11 — Clean skill companions** (`.agents/skills/**/SKILL.md`, 13 files)
  - [x] remove `## Fusion Mode` sections + `kask.fusion` references
  - [x] registry-template READMEs (superforecasting, scenario-builder) + task-breakdown quality-gate flag
- [x] **T12 — Update docs**
  - [x] `DIVERGENCE.md` (D8, ModelFilterFn entry), hkask-inference README, hkask-memory README, diataxis reference.md
  - [x] `kask/docs/**` current-state sweep (cognition-and-replica, kask-settings, regulation-spans, kask_bridge reference, tutorials, READMEs)
- [x] **Checkpoint 3** — repo grep: only intentional fusion matches remain (RRF, confusion, skill-bundler, plans/audits history)

## Phase 4 — validation

- [x] **T13 — Validate**
  - [x] cargo check full touched set (13 crates)
  - [x] targeted tests (`hkask-types`, `hkask-inference`, `hkask-templates`, `kask_bridge` — all pass)
  - [ ] clippy on touched crates
  - [ ] PR description `Suggested .rules removals`
- [x] **Checkpoint 4** — tests pass; clippy + PR notes pending
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
- Operator settings migration — stale `kask-fusion/fusion` + AA-discovered favorites persist in user `settings.json` (manual cleanup)

## Debug — Agent Swarm panel: clicking "Local" closes/panics the app (2026-08-02)

**Repro:** Open the Agent Swarm panel → click the "Local" backend toggle (the
`ABW | Local` ToggleButtonGroup in the header, `crates/swarm_panel/src/
swarm_panel.rs` render ~L3015). The zed-kask app exits.

**Findings so far (2026-08-02):**
- `Zed.log` (`~/.local/share/zed/logs/Zed.log`) has NO fatal panic — only the
  known non-fatal rust-analyzer `old_memo.revisions.changed_at` LSP panics.
- **Root cause of the missing log: `crates/zed` installs NO panic hook**
  (`grep -r "set_hook|panic_hook|take_hook" crates/zed/**/*.rs` → 0 matches).
  The default Rust panic hook prints to **stderr**, which `Zed.log` does not
  capture. A main-thread GPUI panic aborts the process and leaves no trace in
  the log when zed-kask is launched from a desktop launcher (no terminal
  stderr). This is why the crash is invisible in the log.
- **Ruled out by inspection** (none of these panic):
  - `SwarmConfig::from_env` (`kask/mcp-servers/hkask-mcp-swarm/src/config.rs`)
    — every env read is `.unwrap_or(default)`; no `unwrap`/`expect`/indexing.
  - `LocalSwarmRuntime::new` (`local_runtime.rs` L86) — returns `Result`,
    uses `?`/`map_err`; no panicking ops.
  - `sync_kask_mcp_servers` (`crates/zed/src/main.rs` L2449) — the
    synchronous SettingsStore observer that fires on the toggle; no
    `unwrap`/`expect` (`settings.mcp.overrides.get(...).unwrap_or(&true)`).
  - `SwarmPanel` production code — `grep "\.unwrap\(\)|\.expect\("` matches
    only in `mod tests`, not in render/handlers.
- **Leading hypothesis (needs a backtrace to confirm):** a GPUI main-thread
  panic on the toggle path. `set_swarm_mode` (`swarm_panel.rs` L1625) runs
  inside the panel's `cx.listener`, writes `kask.swarm.mode` via
  `SettingsStore::update_settings_file`, then `self.steer_conversation.take()`
  drops the Steer `ConversationView` if one is open. If the Steer conversation
  had focus, dropping a focused entity during the panel's own update can trip
  GPUI's focus/borrow machinery (the `.rules` "entity updated while already
  being updated" / center-pane focus traps). Repro correlation to check: does
  the crash only happen when Steer is open before clicking Local?

**Next steps (capture the backtrace, then pinpoint):**
1. **Launch zed-kask from a terminal** with `RUST_BACKTRACE=full` and
   reproduce — the panic + backtrace print to stderr (the terminal), giving
   the exact file:line.
2. **Install a panic hook** in `crates/zed/src/main.rs` that `log::error!`s
   the panic message + backtrace (and chains to the default hook) so future
   main-thread panics appear in `Zed.log` instead of being lost to stderr.
   One-time enabler that fixes the "no trace in the log" gap for ALL panics,
   not just this one.
3. Once the backtrace is captured, fix the root cause (likely the
   `steer_conversation.take()` during update → defer the drop to a task that
   runs after the update closure returns, or avoid dropping a focused
   `ConversationView` mid-update).

**RESOLVED (2026-08-03):** The panic hook captured the exact panic:
```
thread 'main' panicked at tokio-1.53.1/.../runtime/context/current.rs:82:21:
`EnterGuard` values dropped out of order. Guards returned by
`tokio::runtime::Handle::enter()` must be dropped in the reverse order as
they were acquired.
```
Root cause: `sync_kask_mcp_runtime_servers` (`crates/zed/src/main.rs`) held
`let _tokio_guard = tokio_handle.enter()` across `.await`s inside a `cx.spawn`.
The log shows the swarm restart fired twice (a mode toggle plus a window-close
registry churn), so two `cx.spawn` tasks each acquired an `EnterGuard` and
interleaved at await points on the single foreground thread → out-of-order
drop → panic. This is the `.rules` "background_spawn of tokio-dependent
futures" / `Tokio::handle_async(&*cx).enter()` trap.

Fix: removed the foreground `enter()` guard; build the changed-server list on
the foreground (needs `AsyncApp`), then dispatch the tokio-dependent
`stop_server` / `start_server_with_env` through `gpui_tokio::Tokio::spawn`
(entering the reactor on the worker thread — no manual guard held across
awaits). `McpRuntime: Send + Sync` (its `governance` field is all-Send-Sync;
`RegulationSink: Send + Sync`), so the future is `Send` and `Tokio::spawn`
accepts it. The deferred launch loop's `enter()` guard (~L1948) is the only
remaining await-held guard; it runs once at startup and can't self-overlap,
so it's safe.

Files: `crates/zed/src/main.rs` — `install_panic_hook()` (new) called first in
`main()`; `sync_kask_mcp_runtime_servers` restructured.
Validation: `cargo check -p zed` clean. Repro (toggle Local / close the swarm
panel) should no longer panic; if any panic recurs, the hook now logs it to
`Zed.log` as `PANIC at <file:line>: <msg>\nbacktrace: ...`.

**Follow-up (separate observation, 2026-08-03):** the user noted "this time the
tool invoker wasn't wired" — i.e. the swarm server's tool-dispatch hook
(`set_tool_invoker`) was unset, so `swarm_delegate` falls back to the
unavailable stub. This is distinct from the panic. Likely the deferred task
that wires `set_tool_invoker` (the `.rules` "Model-dependent kask wiring must
run in the deferred task") didn't reach the wiring, or the swarm server was
restarted by `sync_kask_mcp_runtime_servers` after the wiring and the new
child process didn't inherit it. Trace: confirm `set_tool_invoker` is called
in the deferred task and that a settings-change restart of the swarm server
doesn't lose it (the hook is process-global, so a child restart shouldn't
affect it — but verify the wiring isn't gated on a condition that's false at
that point).
