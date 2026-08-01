---
title: zed-kask — Minimal-Divergence Fork Architecture & Migration Plan
audience: hKask architects / zed-kask integrators
last_updated: 2026-08-01
version: 0.32.2
status: Active
domain: architecture
mds_categories: [composition, trust, lifecycle]
---

# zed-kask — Minimal-Divergence Fork Architecture & Migration Plan



> **Build status (2026-07-29):** The kask workspace has **19 kask crates** under `kask/crates/` (18 `hkask-*` + `kask_bridge`) plus 11 MCP server crates under `kask/mcp-servers/` and 2 zed-side crates (`crates/kask_panel/` D10, `crates/kask_extensions_ui/`). The `zed` binary crate parses correctly but can't fully build on this machine (missing x11 system libs — a Linux GUI dependency, not a code issue).
>
> **Integration progress (D1–D14):**
>
> | D | Surface | Status | What's wired |
> |---|---|---|---|
| D1 | Skill execution | ✅ **DONE** | `SkillTool` has optional `SkillManifestExecutor`; composition root in `main.rs` constructs `BridgeManifestExecutor` with `InferencePort` + `ToolPort` + registry paths and calls `agent::set_manifest_executor()`. 48 skills linked (SKILL.md + manifest.yaml in `kask/registry/`). |
> | D2 | Curator agent | ✅ **DONE** | `Curator` variant in `agent_ui::Agent` enum; `CURATOR_AGENT_ID` in `agent` crate; selectable in Agent Panel. |
> | D3 | Tools in-process | ✅ **DONE** | `McpRuntime` implements `ToolPort` directly (capability-match gate + gas budgeting + `reg.tool.*` spans) and is passed wherever a `ToolPort` is needed — no bridge adapter. MCP servers run as child processes (stdio). Daemon transport removed; identity is `ServerContext.webid` resolved from `HKASK_WEBID` (no `userpod`/`MCPBootstrap`). |
> | D4 | Guard layer | ✅ **DONE** | `GuardedInferencePort` wraps the `InferencePort` at the composition root. `hkask-guard` crate's `ContentGuard::mandatory()` provides input scanning (prompt injection, role override, token limit) and output scanning (secret redaction). Guard wraps the skill cascade path (ManifestExecutor). Direct chat uses zed's `LanguageModel::stream_completion` with provider-side safety + refusal fallback (`cascade_only` is hardcoded — the `kask.guard.direct_chat_strategy` setting was deleted in the 2026-07-31 simplification pass; see `tasks/plan.md` C6). `hkask-guard` added as dep of `zed` crate. |
> | D5 | Sovereignty keys | ✅ **DONE** | `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase chain, SQLCipher encryption). No `keyring`-injection seam, no `OnceLock`, no parallel zed `CredentialsProvider` path — the prior injection design was removed in the 2026-07-31 simplification pass (see `tasks/plan.md` D5 verdict). The a2a/OCAP secret threading (`resolve_a2a_secret`, `get_or_create_ocap_secret`, `resolve_secret_chain`, `resolve_treasury_key`, `resolve_wallet_seed`, `sign_wallet_bytes`, `InternalSecrets`, `derive_all_internal_secrets`) was deleted as self-referential security theater (token signature verified against the token's own embedded key — denied nothing); `panel_default_token` mints the in-process capability token with a static key. The capability-match gate in `McpRuntime::invoke` is the real authority. |
> | D6 | Thread → memory | ✅ **DONE** | `MemoryPort` trait defined in `hkask-types` (`TurnRecord`, `MemoryError`, `MemoryFuture`). `BridgeMemoryPort` (adapts `agent::ThreadMemoryPort` → `hkask_types::MemoryPort`) in `kask_bridge`. Global hook `agent::set_memory_port()` / `agent::memory_port()` (**Mutex** pattern, not OnceLock — re-settable). Thread turn completion in `thread.rs::run_turn()` extracts last user prompt + agent response + model + title and calls `ingest_turn()` fire-and-forget via `cx.background_spawn()`. The hook is `None` at startup (no `LoggingMemoryPort` — deleted in the 2026-07-31 simplification pass, see `tasks/plan.md` C4); `thread.rs` no-ops when the hook is unset. Full hKask memory stack (SQLCipher, episodic/semantic, consolidation, WebID mapping) wired via `RealMemoryPort` in the deferred post-login task. |
> | D7 | App-identity | ✅ **DONE** | `APP_NAME`→`Zed-Kask`, `app_identifier`/`app_id`/`display_name` renamed, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`, bundle IDs `dev.zed-kask.*`. |
| D8 | Bridge + adapters | ✅ **DONE** | `kask_bridge` crate: `LanguageModelInferencePort` (`InferencePort` over `LanguageModel`), `BridgeManifestExecutor`, `BridgeMemoryPort`, `BridgeThreadCondenser`, `BridgeContextInjector`, `BridgeCuratorContextInjector`, `BridgeMetacognitionProvider`, `KaskSettings`. `McpRuntime` is passed directly as `ToolPort` (no `BridgeToolPort` — collapsed in the 2026-07-31 simplification pass, see `tasks/plan.md` C3). Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
| D9a | Settings section | ✅ **DONE** | `KaskSettings` struct registered with zed's settings system; `"kask"` section in settings.json. Covers MCP, data services, curator, guard, memory. |
> | D9b | Credentials namespace | ✅ **DONE** | `keyring` crate directly (synchronous OS keychain) (kask namespace: `kask://credentials/<key>`). `InferenceConfig::from_secrets()` reads API keys via `keyring` crate with env var fallback. |
| D9c | Settings UI page | ✅ **DONE** | `crates/settings_ui/src/pages/kask_page.rs` — top-level "Kask" page with sub-pages: Data Services (API key entry → keychain via `CredentialsProvider` + enable toggles), MCP Servers (11 built-in servers + `load_default` master toggle + per-server overrides), Curator (`always_on` + `algedonic_threshold`), Memory (`consolidation_cadence_secs` + `confidence_floor` + `recall_limit` + `recall_min_confidence` + `auto_inject`), Condenser (`profile` + `auto_compress_tool_results` + `persona_keywords` + `saliency_window`), Models. The Guard sub-page was deleted in the 2026-07-31 simplification pass (`direct_chat_strategy` was the only field; see `tasks/plan.md` C6). Registered in `page_data.rs::settings_data()` after `ai_page`. `credentials_provider` added as direct dep of `settings_ui`. |
> | D10 | Kask panel | ✅ **DONE** | `crates/kask_panel/` — native GPUI center-pane `Item` implementing `workspace::item::Item` (NOT a dock `Panel`; opens via `workspace.add_item_to_active_pane(...)` so it doesn't block other settings/panels). Tab strip (11 built-in MCP servers); each tab hosts the agent panel's `ConversationView` with `Agent::Curator` — the `ConversationView` handles all rendering (messages, input, tool-call cards, scroll, retry, cancel, copy, markdown, streaming, mentions, drag-and-drop). The kask panel only adds the tab strip and tab-switch logic. Per-tab system prompt injected via `CuratorAgentServer::with_extra_static_context`. `ToolInvoker` trait + `set_tool_invoker` hook remain for the per-server visualization views (kanban, portfolio, scenarios) which fetch data via direct MCP tool calls. The `set_scoped_inference` / `set_curator_session_factory` / `set_regulation_status` hooks and the `PanelScopedInference` / `PanelToolInvoker`-wrapping-`BridgeToolPort` adapters were removed in the kask panel refactor; the kask panel has structural-pin tests asserting these traits/structs do not exist (see `crates/kask_panel/src/kask_panel.rs` `FORBIDDEN_SYMBOLS`). `kask_panel::Toggle` / `ToggleFocus` actions. Deployed on demand via `kask_panel::init(cx)` (called in `main.rs`); NOT registered in `zed.rs::initialize_panels()`. `Item::tab_icon` returns `Icon::new(IconName::Kask)`. |
>
> **Composition root** (`crates/zed/src/main.rs`, after `gpui_tokio::init`):
> 1. Constructs `RegulationLedger::default()` + `CyberneticsLoop::new(ledger)` (with alert channel + event sink + optional alert email sink) + `FlatEnergyEstimator` + `NoopEventSink` and calls `McpRuntime::new().with_governance(...)` — startup log: "hKask regulation system wired". `McpRuntime` is the `ToolPort` (passed directly wherever a `ToolPort` is needed — no `BridgeToolPort`).
> 2. Spawns the CyberneticsLoop tick cycle (10s interval) and the MetacognitionLoop (30s tick) on the GPUI-global tokio runtime via `gpui_tokio::Tokio::spawn`.
> 3. Wires `BridgeMetacognitionProvider` over the MetacognitionLoop and calls `agent::set_metacognition_provider()` (Mutex — re-settable; re-set with the memory-health probe in the deferred task).
> 4. After `settings::init(cx)`: reads `KaskSettings`, calls `ensure_openai_compatible_entries` to register enabled inference providers in zed's `LanguageModelRegistry`, and computes the MCP server auto-launch set.
> 5. In the **deferred post-login task** (watches `UserStore::current_user()`): `provision_agent(username)` creates the per-user data directory, ensures a DB passphrase (auto-generated random English word, stored in keychain via `hkask-keystore`'s direct `keyring` use), opens the SQLCipher DB, and constructs `RealMemoryPort`.
> 6. Wraps `RealMemoryPort` in `BridgeMemoryPort` and calls `agent::set_memory_port()` (D6 — Mutex, re-settable; was `None` until this point).
> 7. Re-sets `set_metacognition_provider` with the memory-health probe attached (`BridgeMetacognitionProvider::with_memory_port`).
> 8. Wires `BridgeContextInjector` (agent context recall) and `BridgeCuratorContextInjector` (curator-scoped recall) via `set_context_injector` / `set_curator_context_injector` (OnceLock — one-shot).
> 9. Wires `LazyToolRouter` via `set_tool_router` (Mutex — re-settable).
> 10. Constructs `PanelToolInvoker` (holding a `gpui::BackgroundExecutor` for spawning trait-method tasks without a `cx` in scope) and calls `kask_panel::set_tool_invoker()` (D10). The curator turns route through `NativeAgent` — the `ConversationView` handles streaming + tool dispatch. The `set_scoped_inference` / `set_curator_session_factory` / `set_regulation_status` hooks were removed in the kask panel refactor; the kask panel has structural-pin tests asserting these traits/structs do not exist.
> 11. Wires `BridgeThreadCondenser` via `set_thread_condenser` (Mutex — re-settable) when `kask.condenser.auto_compress_tool_results` is on.
> 12. Constructs `LanguageModelInferencePort` (InferencePort over zed's default `LanguageModel`), wraps it with `GuardedInferencePort` (D4), constructs `BridgeManifestExecutor` with guarded inference + `McpRuntime` (as `ToolPort`) + registry paths + tokio handle, and calls `agent::set_manifest_executor()` (OnceLock — one-shot). Token minting uses `panel_default_token` (static key — the a2a/OCAP secret threading was deleted as self-referential security theater).
> 13. Auto-launches the enabled MCP servers via `McpRuntime::start_server()` (stdio child processes) once the inference IPC socket is available.
>
> **Note:** There is no `keyring`-injection step and no `resolve_a2a_secret()` call — both were removed in the 2026-07-31 simplification pass (see `tasks/plan.md`). `hkask-keystore` uses the `keyring` crate directly. There is no `BridgeToolPort` — `McpRuntime` is passed directly as the `ToolPort`. There is no `LoggingMemoryPort` — the memory port hook is `None` until the deferred task wires `RealMemoryPort`.
>
> **Revised approach for `hkask-inference`:** Kept (MCP servers use it directly). Reads API keys via `keyring` crate. Long-term: replace with `InferencePort` over zed's `LanguageModel`, but keeping it unblocks the MCP servers immediately.
>

> **Build status (2026-08-01):** D1–D14 are wired at the composition root (`crates/zed/src/main.rs`). The `zed` binary cannot fully build on this machine (missing x11 system libs — a Linux GUI dependency, not a code issue); validation is via `cargo check` and `diagnostics`. Deferred items: T3.4 (double-gate reconciliation), T4.2/T4.3 (memory/thread-watcher), T6.2 (clean-machine verification). These are deferred verification and design decisions, not architecture.
>
> **One-line frame:** `zed-kask` is a **fork of Zed** that tracks `upstream` (`zed/zed`) and diverges in **exactly three places**: (1) the **skill module** (skill execution → hKask's `ManifestExecutor`), (2) the **Curator agent** (a new native agent backed by hKask), and (3) the **hKask tool-processing code** (compiled-in hKask crates + in-process tool hosting). Everything else stays byte-identical to upstream and is re-merged regularly. hKask is trimmed to **only** the Curator + user sovereignty + the tools. **No backward compatibility.** Principle: *as simple and minimal as possible — and the fork's divergence surface is itself minimal.*



---

## 0. Fork Location & Upstream-Sync Strategy (load-bearing)

- **Fork:** `Clones/zed-kask` — `origin` = `github.com/mdz-axo/zed-kask.git`, `upstream` = `github.com/zed/zed.git`, on `main`, currently **in sync** with upstream.
- **Divergence policy:** keep `main` a near-clone of `upstream/main`. All hKask integration is isolated to a **small, named set of crates/files** (§3) so `git fetch upstream && git merge upstream/main` stays low-conflict. No scattered edits across Zed's tree.
- **hKask wiring (FULL MERGE — §14):** hKask's keep-crates + skills registry + scripts + docs are moved **into the zed-kask repo** under a `kask/` namespace (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/skills/`, `kask/scripts/`, `kask/docs/`) and added as zed-kask workspace members. The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. (Replaces the prior path-dep/submodule approach, which dissolved once hKask could no longer run standalone.)
- **Sync cadence (ongoing, Phase 7):** rebase/merge `upstream/main` regularly; resolve conflicts only in the divergent crates; run the hKask integration tests after each sync. The whole point of the fork is to *inherit Zed's improvements for free* — divergence is the cost, so minimize it.

---

## 1. The Enhanced Prompt (minimal-divergence fork)

> Fork Zed into **`zed-kask`** (`Clones/zed-kask`), tracking `upstream` and diverging only in three areas. hKask is trimmed to the Curator + sovereignty + tools, compiled into zed-kask under the `kask/` namespace. No backward compatibility.
>
> 1. **zed-kask owns the generic surface and infra** (unchanged from upstream): chat (Agent Panel), GitHub, editor UI, comms/voip/CRDT (replacing Matrix entirely), the **inference router** (`crates/language_model*`), the **provider keystore** (`crates/credentials_provider`), thread storage. These stay byte-identical to upstream.
> 2. **Divergence #1 — skill execution:** change `crates/agent_skills` + `crates/agent/src/tools/skill_tool.rs` so a skill activation runs hKask's **manifest model** — `manifest.yaml` + Jinja2 templates driving a WordAct/FlowDef/KnowAct/RenderAct cascade with PDCA loops, gas/rjoule, OCAP gating — via the compiled-in `ManifestExecutor`, instead of `render_skill_envelope()` injecting the `SKILL.md` body.
> 3. **Divergence #2 — Curator agent:** add the Curator (VSM S4) as a native in-process zed-kask agent (singleton; `CuratorHandle` mpsc authority never crosses a process boundary), selectable in the Agent Panel. ACP is optional (only for external-agent interop).
> 4. **Divergence #3 — hKask tool processing:** compile hKask's keep-crates into zed-kask; host the 11 on-disk MCP servers (§2.4) **in-process** (new transport alongside `context_server`'s `StdioTransport`); emit `reg.*` spans directly.
> 5. **Thread → memory:** zed-kask threads parsed into per-user / Curator episodic + semantic memory (extends the existing ACP per-turn encoding).
> 6. **Remove everything redundant from hKask:** inference router, daemon, ACP seam, MCP-stdio, REPL, chat service, Matrix (all of it), communication MCP, backward-compat shims. **Nothing is removed from zed-kask** — it tracks upstream.
> 7. **Magnac Carta P1–P4, P12 non-negotiable.** `hkask-guard` becomes a layer in zed-kask's inference path so **every** LLM boundary (direct chat + skill cascade + Curator) is guarded — coverage *improves*.

---

## 2. The Essentialist Split (what zed-kask owns vs what hKask keeps)

### 2.1 zed-kask owns (generic — inherited from upstream, NOT modified except integration seams)

Inference routing (`crates/language_model`, `language_model_core`, `language_models`, `language_models_cloud`), provider keystore (`crates/credentials_provider`, `zed_credentials_provider`), chat/Agent Panel (`crates/agent`, `agent_ui`), editor/GitHub/comms/voip/CRDT (`crates/workspace`, `project`, etc.), thread storage (`crates/agent/src/thread_store.rs`), MCP stdio hosting (`crates/context_server`). These stay upstream-identical; we only *add seams* (guard layer, in-process transport) where hKask plugs in.

### 2.2 hKask keeps (unique: curator + sovereignty + tools) — compiled into zed-kask

**Status (2026-07-29):** workspace builds clean. **19 kask crates** under `kask/crates/` (18 `hkask-*` + `kask_bridge`) plus 11 MCP server crates under `kask/mcp-servers/` and 2 zed-side crates (`crates/kask_panel/` D10, `crates/kask_extensions_ui/`). 11 MCP servers on disk (curator may be unloaded via `kask.mcp.overrides`).

| Crate | Why irreducible |
|---|---|
| `hkask-types` | Foundation: IDs, `InferencePort` trait, `RegulationSpan`, vocab. `VoiceDesign` and `ExpectProposal` moved here from deleted crates. `HMemEntry` moved here from deleted `hkask-git-cas`. |
| `hkask-storage` | **Sovereignty:** per-user/curator data directory encrypted private sphere (P11.1). Dual-backend: SQLite (SQLCipher, default) or PostgreSQL (pgvector, for scale-up). `user_store` deleted (multi-user identity store — zed account replaces it). |
| `hkask-memory` | Unique semantic/episodic memory + consolidation. |
| `hkask-regulation` | Cybernetic nervous system (`reg.*`, variety, algedonic, set-points). Pruned from 49 files/15,408 lines to 26 files/9,004 lines — orphaned modules deleted (see §2.3). `WalletManager` manages gas/rJoule balances and uses `WalletConfig.gas_per_rjoule` for conversion (moved from deleted `hkask-wallet`). |
| `hkask-templates` | **The tools/skills:** `ManifestExecutor` + registry + cascade + PDCA. |
| ~~`hkask-pods`~~ (deleted) | Pod abstraction (ActivePods, PodDeployment, PodFactory, PodRegistry, PodContext, PerPodLedger, LoopScheduler, AgentPod, PodKind, PodLifecycleState) deleted. Replaced by user/curator data directories. `VoiceDesign` moved to `hkask-types`. |
| `hkask-guard` | **Magna Carta floor (P3.1)** — becomes a layer in zed-kask's inference path. |
| `hkask-capability` | **OCAP** — sovereignty enforcement. |
| `hkask-keystore` (trimmed) | **Sovereignty crypto only:** OCAP signing, DB passphrase, internal-secret derivation w/ versioning. Uses the `keyring` crate directly for all keychain access (no `SecretsPort` trait). Wallet-specific resolvers (`resolve_treasury_key`, `resolve_wallet_seed`, `sign_wallet_bytes`) deleted. |
| ~~`hkask-wallet`~~ (deleted), `hkask-ledger` | rJoule energy budget + hMem accounting. `hkask-wallet` deleted — `gas_per_rjoule` config lives in `hkask-types::WalletConfig`. `GAS_PER_RJOULE` and `WalletConfig` were already in `hkask-types`. |
| `hkask-inference` | **Kept (revised):** MCP servers use it directly for now (`MediaRouter`, `InferenceIpcClient`, `ProviderId`). Reads API keys from the `keyring` crate directly. Embeddings are handled by `kask_bridge::LanguageModelEmbeddingPort` (resolves credentials from `INFERENCE_PROVIDERS` + env var, no `LanguageModelRegistry` lookup). |
| `hkask-mcp-server` (framework) | Trim if zed-kask's context_server hosts them natively; keep the `reg.tool.*`+OCAP gating. Daemon transport (`src/daemon/`, 6 files) and `startup.rs` deleted; `bootstrap_mcp_server()` removed (replaced by `hkask_mcp_server::run_server` factory). Servers run standalone with identity from `ServerContext.webid` (resolved from `HKASK_WEBID`, falling back to anonymous). |
| `hkask-forecast`, ~~`hkask-goal`~~ (deleted), `hkask-condenser`, ~~`hkask-git-cas`~~ (deleted), `hkask-bridge-dublincore` | Domain logic used by keep-crates/MCP servers. `hkask-git-cas` deleted — `GitCASPort` trait and supporting types deleted from `hkask-types`; `HMemEntry` moved to `hkask-types/src/lib.rs`. `hkask-goal` deleted — `GoalState` retained in `hkask-types`; `Goal`/`GoalArtifact`/`GoalCriterion` removed. |
| ~~`hkask-test-harness`~~ (deleted) | Test infra deleted — `ExpectProposal` moved to `hkask-types`. |
| `hkask-mcp` | MCP governance. `FlatEnergyEstimator` (10 gas per call) added here. |
| `hkask-services-core` | Shared foundation: `ServiceError`, `ServiceConfig`, `HkaskSettings`. Consumed by 6 crates. The other `hkask-services-*` crates were folded into their sole MCP server consumers (F6 refactor-architecture pass): `hkask-services-kata-kanban` → `hkask-mcp-kata-kanban`; `hkask-services-corpus` + `hkask-services-compose` + `hkask-services-inference` + `hkask-services-runtime` → `hkask-mcp-corpus`. `hkask-services-context` deleted (governance module moved to `hkask-mcp-curator`; `mcp_server_guard` + `storage_guard` were dead code). |
| 11 MCP servers (on-disk set) | **The tools** — hosted in-process in zed-kask. |

### 2.3 hKask deletes (redundant; jobs move to zed-kask)

**DELETED (confirmed on disk):** `hkask-identity` (zed account replaces it), `hkask-communication` (Matrix → zed voip), `hkask-mcp-cloud-gateway` (no cloud deployment), `hkask-acp` (cross-process seam dissolved), `hkask-api` (HTTP server — zed owns in-process paths), `hkask-cli` (slim CLI surface — to be rebuilt as `kask` CLI for backup/wallet/repair/admin only), `hkask-repl` (zed agent panel replaces it), `hkask-services-chat` (zed owns chat), `hkask-services-onboarding` (zed first-launch replaces it), `hkask-services-runtime` daemon_impl module (deleted; classify/guard/provider_intel kept), `hkask-services-skill`, `hkask-services-wallet`, `hkask-mcp-communication`, `hkask-mcp-filesystem`, `hkask-mcp-memory`, `hkask-mcp-skill`, `hkask-mcp-regulation`, **`hkask-pods`** (pod abstraction deleted — ActivePods/PodDeployment/PodFactory/PodRegistry/PodContext/PerPodLedger/LoopScheduler/AgentPod/PodKind/PodLifecycleState gone; `VoiceDesign` moved to `hkask-types`; `GovernanceContext` `A2ARuntime`/`ConsentManager` fields removed; `KanbanService` `pod_manager` field and `activate_pod` method removed), **`hkask-wallet`** (`gas_per_rjoule` config lives in `hkask-types::WalletConfig`; `GAS_PER_RJOULE` and `WalletConfig` were already in `hkask-types`; wallet-specific keystore resolvers deleted), **`hkask-test-harness`** (`ExpectProposal` moved to `hkask-types`), **`hkask-services-self-heal`**, **`hkask-git-cas`** (`GitCASPort` trait and supporting types deleted from `hkask-types`; `HMemEntry` moved to `hkask-types/src/lib.rs`; `SnapshotLoop` deleted from `hkask-regulation`). **`SecretsPort` trait and `CredentialsSecretsPort` adapter deleted** — keystore uses the `keyring` crate directly for all keychain access. **Regulation orphaned modules deleted** (~6,400 lines, 20 modules): `api_metering`, `seam_watcher`+`seam_types`+`seam_span`, `contract_events`+`contract_span`, `acp_span`, `classify_span`, `snapshot_loop`, `circuit_breaker`, `slo_manager`+`slo_types`+`slo_span`, `set_point_calibrator`, `wallet_gas_calibrator`+`wallet_energy_estimator`, `gas_report`, `dynamic_gas_table`, `composite_energy_estimator`, `calibrated_energy_estimator`, `calibrator`, `inference_estimator`, `table_energy_estimator`. **`fed_*` fields removed from `SetPoints`** (7 vestigial federation fields). **Dead types removed from `hkask-types`:** `git_cas` port module (5 files), `pipeline_manifest`/`pipeline_runner`/`pipeline_state`, `flowdef_validation`, dead wallet types (`ApiKeyMaterial`, `PriceFeedConfig`, `RJ_PER_USDC`, `TxHash`).

**Folded into MCP server consumers (F6 refactor-architecture pass):** `hkask-services-kata-kanban` → `hkask-mcp-kata-kanban`; `hkask-services-corpus` + `hkask-services-compose` + `hkask-services-inference` + `hkask-services-runtime` → `hkask-mcp-corpus`. **`hkask-services-context` deleted** — `governance.rs` moved to `hkask-mcp-curator` (its only consumer); `mcp_server_guard.rs` + `storage_guard.rs` were dead code (zero instantiations). `hkask-services-core` kept (genuinely shared by 6 crates).

---

### 2.4 MCP load set (11 on disk)

The original 16 MCP servers have been pruned to **11 on disk**. The `BUILT_IN_MCP_SERVERS` constant in `kask/crates/kask_bridge/src/mcp_servers.rs` enumerates them.

| On disk (11) | Removed / merged |
|---|---|
| `codegraph`, `companies`, `condenser`, `corpus`, `curator`, `kata-kanban`, `media`, `research`, `scenarios`, `swarm`, `training` | `communication` (Matrix/TTS → zed voip), `filesystem` (zed provides fs tools), `memory` (consolidated into `hkask-memory` crate), `skill` (skill execution is native via D1), `regulation` (consolidated into `hkask-regulation` crate), `docproc`/`replica` (folded into `corpus`) |

> The `curator` MCP server is kept on disk but may be unloaded by default (Curator is a native agent, D2). All 11 build clean.

---

## 3. The Minimal Divergence Map (exact zed-kask touch points)

Every hKask integration maps to a **named, isolated** change in zed-kask. This is the entire divergence surface (D1–D14); everything else tracks upstream.

| # | Divergence | zed-kask crate / file | Status | Change |
|---|---|---|---|---|
| D1 | Skill execution | `crates/agent/src/tools/skill_tool.rs` + `crates/agent/src/agent.rs` + `crates/zed/src/main.rs` | ✅ DONE | `SkillTool` has optional `SkillManifestExecutor`; composition root wires `BridgeManifestExecutor`. `SKILL.md` stays discovery-only; manifest YAML drives the cascade. |
| D2 | Curator agent | `crates/agent_ui/src/agent_ui.rs` + `crates/agent/src/agent.rs` | ✅ DONE | `Agent::Curator` variant; `CURATOR_AGENT_ID`; selectable in Agent Panel. |
| D3 | hKask tools in-process | `crates/zed/src/main.rs` (McpRuntime passed directly as `ToolPort`) | ✅ DONE | `McpRuntime` implements `ToolPort` directly (capability-match gate + gas budgeting + `reg.tool.*` spans). MCP servers run as child processes. Daemon transport removed; `bootstrap_mcp_server()` removed (replaced by `hkask_mcp_server::run_server` factory). Servers run standalone with identity from `ServerContext.webid` (resolved from `HKASK_WEBID`, falling back to anonymous). The former `BridgeToolPort` adapter was collapsed in the 2026-07-31 simplification pass (see `tasks/plan.md` C3). |
| D4 | Guard layer | `crates/language_model_core`/`language_model` + `kask/crates/hkask-guard` | ✅ DONE | `GuardedInferencePort` wraps `InferencePort` at composition root. Content guard scans input (injection, role override, token limit) and output (secret redaction). Guards skill cascade path; direct chat uses provider-side safety + refusal fallback (`cascade_only` hardcoded — `kask.guard.direct_chat_strategy` deleted 2026-07-31). |
| D5 | Sovereignty keys | `kask/crates/hkask-keystore` | ✅ DONE | `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase chain, SQLCipher encryption). No `keyring`-injection seam, no `OnceLock`, no parallel zed `CredentialsProvider` path. The a2a/OCAP secret threading was deleted as self-referential security theater (see `tasks/plan.md` D5 verdict); `panel_default_token` mints the in-process capability token. |
| D6 | Thread → memory | `crates/agent/src/thread.rs` / `thread_store.rs` + `kask/crates/hkask-types` + `kask/crates/kask_bridge` | ✅ DONE | `MemoryPort` trait in `hkask-types`. `BridgeMemoryPort` in `kask_bridge`. Global hook `agent::set_memory_port()` (Mutex — re-settable). Thread turn completion ingests via `cx.background_spawn()`. Hook is `None` at startup (no `LoggingMemoryPort` — deleted in the 2026-07-31 simplification pass, see `tasks/plan.md` C4); `RealMemoryPort` wired in the deferred post-login task. |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml` | ✅ DONE | `APP_NAME`→`Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`, bundle IDs `dev.zed-kask.*`. |
| D8 | Bridge + adapters | `kask/crates/kask_bridge/` | ✅ DONE | `LanguageModelInferencePort` (`InferencePort` over `LanguageModel`, honors `model_override` via `resolve_model_names` registry resolution), `BridgeManifestExecutor`, `BridgeMemoryPort`, `BridgeThreadCondenser`, `BridgeContextInjector`, `BridgeCuratorContextInjector`, `BridgeMetacognitionProvider`, `KaskSettings`. `McpRuntime` is passed directly as `ToolPort` (no `BridgeToolPort` — collapsed in the 2026-07-31 simplification pass, see `tasks/plan.md` C3). Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
| D9 | Settings + credentials | `kask/crates/kask_bridge/src/settings.rs` + `crates/settings_content/src/settings_content.rs` + `crates/settings_ui/src/pages/kask_page.rs` + `crates/settings_ui/src/page_data.rs` | ✅ DONE | `KaskSettings` struct + `"kask"` section in settings.json; `hkask-keystore` uses the `keyring` crate directly (kask namespace). Settings UI page with sub-pages (Data Services, MCP Servers, Curator, Memory, Condenser, Models) registered in `page_data.rs` after `ai_page`. The Guard sub-page was deleted in the 2026-07-31 simplification pass (`direct_chat_strategy` was the only field; see `tasks/plan.md` C6). |
| D10 | Kask panel | `crates/kask_panel/` | ✅ DONE | Native GPUI center-pane `Item` implementing `workspace::item::Item` (not a dock `Panel`). Tab strip (11 built-in MCP servers); each tab hosts the agent panel's `ConversationView` with `Agent::Curator`. `ToolInvoker` trait + `set_tool_invoker` hook for per-server visualization views (kanban, portfolio, scenarios). `kask_panel::Toggle`/`ToggleFocus` actions. Deployed on demand via `kask_panel::init(cx)`; NOT loaded in `zed.rs::initialize_panels()`. The `set_scoped_inference` / `set_curator_session_factory` / `set_regulation_status` hooks and `PanelScopedInference` / `PanelToolInvoker`-wrapping-`BridgeToolPort` adapters were removed in the kask panel refactor; structural-pin tests assert they do not exist. |

**Discipline:** D1–D14 are the *only* edits to zed-kask's tree outside `kask/`. Any hKask behavior that would require touching other Zed crates is a smell — push the logic into an hKask crate behind one of these seams instead.
## 6. Migration Status

> The phased migration plan that previously occupied this section has been removed per `DOCUMENTATION_STANDARDS.md` §10. All phases are complete: D1–D14 are wired (see §3 divergence map). The `DIVERGENCE.md` at the repo root is the authoritative record of the divergence surface.

---

## 7. App-Identity Separation (zed-kask ↔ zed coexistence)

**Principle (deep-module):** separate the **local filesystem footprint** so `zed-kask` and an upstream `zed` install coexist on the same machine without conflict, while **sharing the Zed account** — the user logs into their existing Zed account and uses zed-kask *as Zed*, with the minimal kask enhancements. Two deep modules own the footprint; a few hardcoded, non-derived points need separate renames (bug-hunt findings).

### 7.1 The two deep modules (single knobs)

| Module | Knob | Today | zed-kask | What it renames |
|---|---|---|---|---|
| `crates/paths/src/paths.rs` | `APP_NAME: &str` (+ derived `APP_NAME_LOWERCASE`) | `"Zed"` | `"Zed-Kask"` / `"zed-kask"` | config/data/state/temp/logs dirs on all OSes; `Zed-Kask.log`; db/extensions/themes/snippets/prompts/settings/keymap/AGENTS.md; macOS `~/Library/Application Support/Zed-Kask` + `~/Library/Logs/Zed-Kask` + `~/.local/state/Zed-Kask`; Linux `$XDG_*_HOME/zed-kask`; Windows `%APPDATA%\Zed-Kask` + `%LOCALAPPDATA%\Zed-Kask`. **The file itself comments: "Forks should change this to avoid colliding with Zed's user data."** |
| `crates/release_channel/src/lib.rs` | `app_identifier()` / `app_id()` / `display_name()` | `"Zed-Editor-Stable"` / `"dev.zed.Zed-Stable"` / `"Zed"` | `"Zed-Kask-Editor"` / `"dev.zed-kask.Zed-Kask"` / `"Zed-Kask"` | Windows single-instance mutex `{id}-Instance-Mutex` + named pipe `\\.\pipe\{id}-Named-Pipe`; macOS bundle id (`~/Library/Preferences/dev.zed-kask.Zed-Kask.plist`, LaunchServices identity); Dock/menu display name. |

**Deletion test:** inlining `APP_NAME`/`app_identifier` at every call site would reappear the platform-path logic everywhere → the modules earn their keep; change the constants, the whole footprint renames. ≤3 public items each, every consumer reads them, nothing writes back → **deep**.

### 7.2 Non-derived collision points (bug-hunt — APP_NAME alone does NOT fix these)

| # | Point | File | Risk | Fix |
|---|---|---|---|---|
| C1 | **macOS single-instance TCP port** | `crates/zed/src/zed/mac_only_instance.rs` `address()` | Port = `43737 + (channel×100) + uid` — keyed on **release channel + uid only**, NOT on APP_NAME. zed-kask and zed-stable (same channel, same uid) → **same port → the second app sees the "Zed Editor Stable Instance Running" handshake and silently exits.** | Distinct port block (fixed offset, e.g. `+500`, or a `Kask` release-channel arm) + change `instance_handshake()` to "Zed-Kask …". |
| C2 | **Remote SSH/WSL server dirs** | `crates/paths/src/paths.rs` `remote_server_dir_relative()`/`remote_wsl_server_dir_relative()` + `crates/util/src/shell.rs` | Hardcoded `.zed_server` / `.zed_wsl_server` on the REMOTE host. SSH to a host where zed also runs → collision + version mismatch. | `.zed-kask_server` / `.zed-kask_wsl_server` (2 path fns + shell.rs). |
| C3 | **Binary name** | `crates/zed/Cargo.toml` `[[bin]] name = "zed"` | Same `zed` binary on PATH → shadows/conflicts. | `[[bin]] name = "zed-kask"` (keep package name `zed` to minimize diff). |
| C4 | **macOS bundle display names** | `crates/zed/Cargo.toml` L281–305 (`"Zed Dev"`…`"Zed"`) | Indistinguishable from zed in Dock/Launchpad. | `"Zed-Kask …"` (via `display_name()`). |
| C5 | **URL scheme `zed://`** | `crates/zed/src/zed/open_listener.rs` + `assets/settings/default.json` `$schema` + `zed://skill` share links | Internal `zed://` prefixes are just strings (safe); the OS-level handler is bundle-id-registered (macOS: only one app owns `zed://`). | **Decision:** keep `zed://` internally (minimal divergence — don't touch open_listener) and accept the macOS handler conflict, OR rename to `zed-kask://` (full isolation, but diverges `default.json` `$schema` + skill-share links). Lean: keep `zed://`; revisit. |

### 7.3 RENAME vs KEEP (the account-sharing constraint)

| RENAME (local footprint — isolated) | KEEP (shared — user logs into their Zed account) |
|---|---|
| `APP_NAME`, `app_identifier`, `app_id`, `display_name` | `default.json` `"server_url": "https://zed.dev"` (collab) |
| config/data/state/cache/logs/db/extensions dirs | `"provider": "zed.dev"`, `"zed.dev": {}` (LLM provider/account) |
| `Zed-Kask.log`, settings/keymap/AGENTS.md paths | `cloud_api_client` `cloud.zed.dev` (account API) |
| Windows mutex/pipe, macOS bundle id + plist | `release_channel::ZED_DOCS_URL` `https://zed.dev/docs` (docs) |
| macOS single-instance port + handshake | `staging-collab.zed.dev` / `collab.zed.dev` (collab relay) |
| `.zed-kask_server` / `.zed-kask_wsl_server` remote dirs | telemetry endpoint (zed.dev) — optional disable |
| binary `zed-kask` | extension marketplace URL (shared; extensions re-installed in the isolated dir) |

**Key invariant:** account/auth/collab traffic goes to `*.zed.dev` keyed on the user's Zed credentials, NOT on bundle id or APP_NAME. Renaming the local identity does **not** affect login — the user signs into the same Zed account and zed-kask behaves as Zed with a separate local footprint.

### 7.4 Verified facts (what breaks?)

- **Does renaming the bundle id break Zed account login?** No — auth is to `cloud.zed.dev` keyed on credentials, not bundle id. (Verified: account endpoints live in `default.json`/`cloud_api_client`, independent of `app_id`.)
- **Does renaming APP_NAME orphan existing Zed settings?** It *isolates* them — zed-kask starts fresh (re-onboard); the user's zed settings stay untouched in the old `zed` dirs. Intended.
- **C1 is the silent killer:** an APP_NAME rename does NOT prevent the macOS single-instance collision — verified `address()` keys on channel+uid. Must fix C1 explicitly or zed-kask silently exits whenever zed is running.
- **Extensions:** isolated dir = re-install. Minor cost; benefit = no version conflicts with zed's extensions.
- **Telemetry:** distinct install id (renamed data_dir) → zed-kask reports under a different install id to the same endpoint. Acceptable, or disable.

### 7.5 Implementation (complete)

All app-identity tasks (T-A1 through T-A8) are complete (D7 ✅ DONE): `APP_NAME`→`Zed-Kask`, `app_identifier`→`Zed-Kask-Editor`, `app_id`→`dev.zed-kask.Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`/`.zed-kask_wsl_server`, bundle IDs `dev.zed-kask.*`, URL scheme `zed-kask://`. See `DIVERGENCE.md` D7 for the authoritative list.

## 8. Architecture Notes

> The planning process artifacts (open questions, review findings) that previously occupied this section have been removed per `DOCUMENTATION_STANDARDS.md` §10. All review findings were resolved during implementation. The architecture is described in §0–§7, §11–§14. The `DIVERGENCE.md` at the repo root is the authoritative divergence surface record.

---

---

## 11. Kask Settings & Credentials (data-service keys, minimal divergence)

> **Correction (2026-08-01):** The D9b design below proposed routing sovereignty keys (D5) through zed's `CredentialsProvider`. The final D5 implementation does NOT do this — `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase, SQLCipher encryption), with no zed-side seam. The `CredentialsProvider` namespace (D9b) is used only for data-service API keys (companies/scenarios), not sovereignty keys. The a2a/OCAP secret threading was deleted as self-referential security theater. See DIVERGENCE.md D5 for the authoritative final state. This section is retained as the design history for D9a/D9b; treat the D5 sovereignty-key references below as superseded by DIVERGENCE.md D5.

**Goal:** load API keys for data services (EODHD, FMP, and other kask data services) and all kask-unique config via a **kask settings section** in zed-kask's settings.json + a **kask credentials namespace** in the keystore — leaving core zed settings/keystore code untouched.

### 11.1 Evidence
- zed-kask stores provider API keys via the `CredentialsProvider` trait (`read_credentials`/`write_credentials`/`delete_credentials` keyed by URL → OS keychain); `language_models` providers use `api_key_state` + `credentials_provider` (`crates/credentials_provider`, `crates/language_models/src/provider/open_router.rs`). **Secrets live in the keychain, NOT settings.json.**
- The settings UI is `Vec<SettingsPage>` built in `crates/settings_ui/src/page_data.rs::settings_data()`; pages live in `crates/settings_ui/src/pages/` (e.g. `mcp_servers_page.rs`, `llm_providers_page.rs`).
- hKask today reads data-service keys from **env vars** (`HKASK_FMP_API_KEY`, `HKASK_EODHD_API_KEY`) — in `hkask-mcp-companies` (`ctx.get`). ~~in `hkask-wallet/price_feed.rs` (`std::env::var`)~~ (`hkask-wallet` deleted). They are NOT in hKask's keychain (which holds DB passphrase/OCAP signing only).

### 11.2 Design (two additive seams)

**D9a — kask settings section** (`"kask": {...}` in settings.json + a settings struct). A new top-level section, isolated from core zed settings. Holds kask-unique, **non-secret** config:
- `kask.data_services.{eodhd,fmp,polygon,alpha_vantage,tiingo,fred,...}` — enabled toggles + per-service config (endpoints, tiers). The **secret API key is NOT here** — it is in the keychain (D9b); settings holds only the reference/toggle.
- `kask.mcp.load_default` + `overrides` — the default-loaded set (§2.4; 11 on disk total) + per-server toggles (curator may be unloaded via override; filesystem/communication absent).
- `kask.curator` — always-on toggle, regulation set-points (variety window, algedonic thresholds).
- `kask.sovereignty.pod` — data-dir override, consent defaults.
- `kask.guard` — direct-chat guard strategy (R3: buffer / incremental / cascade-only).
- `kask.memory` — consolidation cadence, confidence floor.
Registered with zed's settings system so it appears in the `zed://schemas/settings` schema. **Minimal divergence:** one new settings struct + registration; core zed settings structs untouched.

**D9b — kask credentials namespace** (via the existing `CredentialsProvider`). Data-service API keys stored in the OS keychain under kask-namespaced URLs (e.g. `kask://credentials/eodhd`, `kask://credentials/fmp`), alongside zed's provider keys (which use their own URLs). The kask MCP servers (companies/scenarios) read keys via `CredentialsProvider` at runtime — **replacing the env-var approach** (`HKASK_*`). This folds into the T3.0 in-process refactor: MCP servers take a credentials handle, not env vars. The sovereignty keys (D5: DB passphrase, OCAP signing) also move here (kask namespace), so the trimmed `hkask-keystore` becomes a thin crypto-derivation layer over the shared `CredentialsProvider` (using the `keyring` crate directly — `SecretsPort` trait deleted).

### 11.3 Settings UI (additive page)
A new **Kask** page: `crates/settings_ui/src/pages/kask_page.rs` + one entry in `page_data.rs::settings_data()`. Sub-pages mirror the settings section: **Data Services** (per-service enable + key entry → writes to keychain via `CredentialsProvider`), **MCP Servers** (the 11 on-disk servers, with load toggles — curator may be unloaded via override), **Curator**, **Sovereignty/Pod**, **Guard/Regulation**, **Memory**. Touches `page_data.rs` minimally (one `SettingsPage` push) — core zed pages untouched.

### 11.4 Configuration translation / migration
Existing hKask config → kask settings + keychain, on first launch (and a `kask import-config` command):
- env `HKASK_FMP_API_KEY` / `HKASK_EODHD_API_KEY` → `CredentialsProvider` entries `kask://credentials/{fmp,eodhd}` + `kask.data_services.{fmp,eodhd}.enabled = true`.
- hKask keychain sovereignty keys (DB passphrase, OCAP signing) → `keyring` crate directly (D5 — NOT `CredentialsProvider`).
- hKask config-file settings (regulation thresholds, consolidation cadence, gas defaults) → `kask.*` settings.json section.
Precedence: explicit settings.json > imported keychain > env-var fallback (during transition).

### 11.5 Implementation (complete)

D9a/D9b/D9c are ✅ DONE. `KaskSettings` struct registered with zed's settings system; `"kask"` section in settings.json. Credentials in keychain under `kask://credentials/<key>` namespace (via `CredentialsProvider`). Settings UI page with sub-pages. See `DIVERGENCE.md` D9 and `reference/kask-settings.md` for the authoritative state.

### 11.6 Design notes
- **Secrets must NOT be in settings.json** — keys live in the keychain (matches zed's provider-key pattern). The `kask` settings section holds only toggles/refs.
- **Dependency direction:** `hkask-keystore` uses the `keyring` crate directly for all keychain access (D5) — NOT zed's `CredentialsProvider`. The `SecretsPort` trait was deleted.
- **D9 = divergence seam** (kask settings section + credentials namespace + UI page). See §3 divergence map.
---

## 12. Kask Panel (per-MCP-server one-on-one windows)

**Requirement:** a "Kask" panel in zed-kask where the user can launch a window per kask MCP server and interact with it **one-on-one** to reach the server's **deeper functionality** (direct tool invocation + scoped inference), within the zed-kask app — distinct from the conversational Agent Panel (which drives tools via the agent).

### 12.1 Design rationale

The kask panel provides per-MCP-server one-on-one interaction (direct tool invocation + scoped inference), distinct from the conversational Agent Panel. The original implementation in the deleted `hkask-repl` crate (`McpScopedWindow`) was reimplemented natively in GPUI (option B below) — the ratatui TUI was deleted entirely.

### 12.2 Implementation decision

**Decision: native GPUI panel (option B).** A zed-native `Item` (`crates/kask_panel`, GPUI) — a server catalog (the 11 on-disk servers, §2.4) + a per-server view with direct `:tool args` invocation and scoped-inference input, calling the in-process MCP tools and guarded inference (D8) directly. No PTY, no view socket, no daemon listener. The alternative (ratatui-in-terminal) was rejected — it would re-introduce an IPC boundary.

### 12.3 Design (D10 — native GPUI kask panel)
- **zed side:** `Item` impl `crates/kask_panel` (implements `pub trait Item`, `crates/workspace/src/item.rs`; opens in the center pane via `workspace.add_item_to_active_pane(...)` — NOT a dock `Panel`). Renders: a list of the 11 on-disk MCP servers (from the in-process tool registry, §2.4); selecting one opens a per-server sub-view.
- **Per-server sub-view:** (1) the server's tool list (introspected from the in-process MCP server) + a `:tool_name args` direct-invocation input → calls the in-process tool through the OCAP-gated path (same `McpRuntime::invoke` / `ToolGovernance` /gas as the agent; emits `reg.tool.*`); (2) a natural-language scoped-inference input → runs guarded inference (D8) with only that server's tools in scope. Results rendered inline.
- **OCAP:** the panel invokes tools under the caller's `DelegationToken` (scoped to `webid`) exactly as the agent does — direct invocation does NOT bypass OCAP. Double-gate (F10) applies: panel invokes are still `McpRuntime::invoke` / `ToolGovernance`-gated.

### 12.4 Implementation (complete)

D10 is ✅ DONE. The kask panel is deployed on demand via `kask_panel::init(cx)` (called in `main.rs`); NOT registered in `zed.rs::initialize_panels()`. `kask_panel::Toggle`/`ToggleFocus` actions. The `ToolInvoker` trait + `set_tool_invoker` hook remain for per-server visualization views (kanban, portfolio, scenarios). See `DIVERGENCE.md` D10 for the authoritative state.

### 12.5 Verified facts

- **Direct invocation does not bypass sovereignty** — it reuses the OCAP-gated `McpRuntime::invoke` / `ToolGovernance` path; only the LLM is bypassed, not OCAP/gas.
- **Variety/regulation:** direct one-on-one invokes still emit `reg.tool.*` and consume gas — the cybernetic loop sees panel activity, so regulation is not bypassed.

---

## 13. Composition & Connection Surfaces (zoom-out review)

The connection surfaces use established patterns (ports-and-adapters, decorator, composition-root DI, zed `Panel`, zed settings/credentials) — correct. This section names them as **one coherent, minimal composition** so the seams are explicit.

### 13.1 Governing invariant (dependency direction)
**hKask crates NEVER depend on zed-kask; zed-kask depends on hKask crates.** The **single bidirectional seam** is the zed-kask-side **bridge crate** (`crates/kask_bridge` = D8), which depends on both hKask port traits and zed-kask types and implements every adapter. Every other divergence (D1, D2, D3, D6, D9a, D10) *consumes* a port implemented by the bridge; no hKask crate reaches into zed-kask internals. (Reconciles R9/D9b.)

### 13.2 The complete port set (ports-and-adapters)
All zed↔kask connection surfaces are a small set of **port traits** (in `hkask-types` + `hkask-capability`), each implemented by the **bridge crate** over a zed-kask facility:

| Port (hKask side) | Implemented over (zed-kask side) | Used by | D |
|---|---|---|---|
| `InferencePort` (hkask-types; non-streaming) | `LanguageModel` (streaming) via collect→`InferenceResult`; `GuardedInferencePort` wraps it | ManifestExecutor (D1), Curator (D2), kask panel scoped inference (D10) | D4/D8 |
| `ToolPort` (hkask-capability; OCAP+gas) | the in-process MCP tool registry (D3) | ManifestExecutor FlowDef `execute`, kask panel direct invoke (D10) | D3/D8 |
| `keyring` crate (synchronous OS keychain) | `CredentialsProvider` (kask namespace) | data-service keys (companies/scenarios) + sovereignty keys (D5) | D5/D9b |
| `CuratorTurnPort` (hkask-types; NEW) | zed native-agent turn → in-process `CuratorAgent` (tokio via bridge) | Curator as native agent (D2) | D2/D8 |
| `MemoryPort` (hkask-types; NEW) | in-process `EpisodicMemory`/`SemanticMemory` handles. `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced: logging at startup, real after agent provisioning. | thread→memory ingestion (D6) | D6 |

Hexagonal pattern: hKask defines the ports; the bridge crate is the adapter; the composition root wires them. **No hKask crate imports a zed-kask crate.**

### 13.3 Composition root (startup — DI pattern)
zed-kask app startup constructs the individual hKask components directly (~~`KaskCore`~~ was never implemented as a single singleton — the composition root wires each component separately) and wires the adapters:
1. **Load `KaskSettings` (D9a)** → bind to component construction params (regulation set-points, gas defaults, consolidation cadence, guard strategy, MCP load set = the 11 on disk, §2.4). **Settings→config is construction-time, not a runtime port** (config-struct-validated-on-construction).
2. **Memory port hook is `None` at startup (D6):** `set_memory_port` is not called until the deferred post-login task. `thread.rs` no-ops when the hook is unset. Uses `Mutex` (not `OnceLock`) so the port can be replaced later. (The former `LoggingMemoryPort` no-op placeholder was deleted in the 2026-07-31 simplification pass — see `tasks/plan.md` C4.)
3. **Construct hKask components directly:** per-user/curator data directory storage (SQLite SQLCipher or PostgreSQL via `ServiceConfig::open_driver()`), Regulation runtime, memory, the singleton Curator (`CuratorHandle` mpsc in-process), the 11 MCP servers (standalone, identity from `ServerContext.webid` resolved out of `HKASK_WEBID`), the `ManifestExecutor`.
4. **Build the bridge:** `InferencePort`-over-`LanguageModel` (+guard), `ToolPort`-over-tool-registry, `keyring` crate-over-`CredentialsProvider`, `CuratorTurnPort`, `MemoryPort`; inject into `ManifestExecutor`/Curator/MCP servers/kask panel.
5. **Wire the regulation system:** construct `RegulationLedger::default()` + `CyberneticsLoop::new(ledger)` + `FlatEnergyEstimator` (10 gas per call, in `hkask-mcp`) + `NoopEventSink` and call `McpRuntime::with_governance()`. Startup log: "hKask regulation system wired — tool invocations are governed". `hkask-regulation` and `tokio` are now dependencies of `zed`.
6. **Spawn** the regulation + Curator metacognition tokio loops on the `gpui_tokio` runtime (R1) — the loop driver.
7. **Register** the **user agent** + **Curator** native agents (D2) and call `kask_panel::init(cx)` (D10) which registers the `KaskPanel` center-pane `Item` + `Toggle`/`ToggleFocus` actions. KaskPanel is NOT a dock panel — it deploys on demand via `Toggle` into the active center pane.
8. **Deferred agent provisioning (D6 late):** after `AppState::set_global`, a spawned task watches `UserStore::current_user()`. When the Zed user resolves: `provision_agent(username)` creates the directory structure, ensures a DB passphrase (auto-generate random English word if none, via the `keyring` crate directly), and calls `set_memory_port(BridgeMemoryPort(RealMemoryPort))` to replace the logging port. MCP servers are launched without a per-user `HKASK_WEBID`; they fall back to anonymous identity unless the operator sets `HKASK_WEBID` in the environment.
9. **Migrate** config (T6.3) on first launch.

~~`KaskCore` is the "shared core" R4 referred to — the single owner of storage/regulation/memory the MCP servers take handles from (prevents the two-instance pitfall).~~ `KaskCore` was never implemented. The composition root constructs individual components directly. The daemon transport was deleted rather than refactored to in-process handles.

Components construct at zed-kask startup with a logging memory port; the agent is provisioned when `UserStore::current_user()` resolves (deferred task). The `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced after startup. Per-user/curator data directory storage opens at provisioning time, not at process start.

### 13.4 Consolidated divergence map (D1–D14)
| D | Surface | zed-kask file | Connection (port/adapter) |
|---|---|---|---|
| D1 | skill execution | `agent_skills` + `agent/tools/skill_tool.rs` | skill_tool → bridge.ManifestExecutor(InferencePort, ToolPort) |
| D2 | Curator agent | `agent.rs` + `native_agent_server` + `agent_servers` | native agent → CuratorTurnPort → in-process Curator |
| D3 | tools in-process | `crates/zed/src/main.rs` (McpRuntime passed directly as `ToolPort`) | in-process transport → ToolPort; MCP servers run standalone with identity from `ServerContext.webid` (`HKASK_WEBID`) (daemon transport deleted; the former `BridgeToolPort` adapter was collapsed in the 2026-07-31 simplification pass — see `tasks/plan.md` C3) |
| D4 | guard | `language_model_core`/`language_model` | `GuardedInferencePort` wraps `InferencePort`-over-`LanguageModel` |
| D5 | sovereignty keys | `kask/crates/hkask-keystore` | **via the `keyring` crate directly** (no injection seam — the prior `keyring`-injection design and `SecretsPort` trait were deleted; see `tasks/plan.md` D5 verdict). DB passphrase auto-provisioned on first run (random English word, stored in keychain). The a2a/OCAP secret threading was deleted as self-referential security theater; `panel_default_token` mints the in-process capability token. |
| D6 | thread→memory | `agent/thread.rs`/`thread_store.rs` | thread hook → `MemoryPort` → in-process memory. Hook is `None` at startup (no `LoggingMemoryPort` — deleted in the 2026-07-31 simplification pass, see `tasks/plan.md` C4); upgraded to `RealMemoryPort` when Zed user resolves (deferred provisioning via `provision_agent`). |
| D7 | app-identity | `paths.rs`, `release_channel`, `mac_only_instance`, `Cargo.toml`, scripts | (zed-kask self-change; not a zed↔kask seam) |
| D8 | bridge + adapters | new `crates/kask_bridge` + `gpui_tokio` | **THE bidirectional seam** — implements all ports (no `BridgeToolPort` — `McpRuntime` is passed directly as `ToolPort`; collapsed in the 2026-07-31 simplification pass, see `tasks/plan.md` C3) |
| D9 | settings + credentials | new `KaskSettings` section + `CredentialsProvider` namespace + `kask_page` | `KaskSettings` → component params; `hkask-keystore` uses the `keyring` crate directly. The Guard sub-page was deleted in the 2026-07-31 simplification pass (`direct_chat_strategy` was the only field; see `tasks/plan.md` C6). |
| D10 | kask panel | new `crates/kask_panel` (Panel) | panel → `ToolInvoker` (for per-server visualization views) + tool registry. The `set_scoped_inference` / `set_curator_session_factory` / `set_regulation_status` hooks and `PanelScopedInference` adapter were removed in the kask panel refactor; structural-pin tests assert they do not exist. |



---

## 14. Repository Consolidation — full merge into zed-kask

**Decision (§0):** fully merge hKask into the `zed-kask` fork. zed-kask becomes the **single source of truth** for everything hKask is becoming — code, skills, scripts, and docs. The `mdz-axo/hKask` repo is **archived** (read-only reference). This replaces the earlier path-dep/submodule wiring (T0.6), which dissolved once hKask could no longer compile or run standalone (daemon/ACP/REPL/inference deleted; keep-crates need the in-process bridge + `gpui_tokio`).

### 14.1 Why (essentialist)
- **hKask crates are not independently shippable** after the deletions — they only compile inside zed-kask. A separate repo for non-standalone crates is friction (cross-repo path-deps, R10 hermeticity, two-clone dev, ownership ambiguity) with no value. P5: a module/repo that can't stand alone shouldn't be kept apart.
- **Removes R10 entirely** — no path-deps, no submodule, one clone/build/CI.
- **Strengthens minimal divergence + upstream sync:** under a `kask/` namespace, hKask's crates/skills/scripts/docs are **additive paths upstream doesn't have** → `git merge upstream/main` never touches `kask/` → near-zero conflict. The only upstream-merge surfaces are the D-seams (in zed's tree) and the `[workspace.members]`/`[workspace.dependencies]` arrays.

### 14.2 The `kask/` namespace (ours vs upstream)
Everything hKask lives under one top-level dir so "ours" is isolated from "upstream":
```
zed-kask/
├── crates/            # upstream zed (unchanged) + D-seam edits
├── mcp-servers/       # (none — hKask's are under kask/)
├── extensions/        # upstream
└── kask/              # ── OURS (additive; upstream never touches here) ──
    ├── crates/        # hkask-types, hkask-storage, hkask-memory, hkask-regulation,
    │                  # hkask-templates, hkask-guard, hkask-capability,
    │                  # hkask-keystore, hkask-ledger,
    │                  # hkask-mcp-server, kask_bridge (D8), kask_panel (D10)
    │                  # (deleted: hkask-pods, hkask-wallet, hkask-test-harness,
    │                  #  hkask-services-self-heal, hkask-git-cas,
    │                  #  hkask-services-context, hkask-services-corpus,
    │                  #  hkask-services-compose, hkask-services-inference,
    │                  #  hkask-services-runtime, hkask-services-kata-kanban —
    │                  #  the last 6 were folded into their MCP server consumers)
    ├── mcp-servers/   # the 11 on-disk servers (curator may be unloaded via override; hkask-mcp-*)
    ├── skills/        # the skills registry (manifest.yaml + *.j2; Pattern A source of truth)
    ├── scripts/       # check-hkask-no-zed-deps.sh + hKask admin/build scripts
    └── docs/          # ← documentation home (see 14.3)
```
zed-kask's `Cargo.toml` adds `kask/crates/*` + `kask/mcp-servers/*` as workspace members and merges hKask's `[workspace.dependencies]` into its own. The bridge crate `kask_bridge` and panel `kask_panel` (D8/D10) live under `kask/crates/` too — they're ours, not upstream's.

### 14.3 Documentation home (`kask/docs/`)
All Kask documentation lives **inside zed-kask** under `kask/docs/`:
- `kask/docs/architecture/` — this plan (`zed-host-architecture-plan.md`), the four-pattern architecture, principles, ADRs.
- `kask/docs/reference/` — reference documentation (MCP servers, settings, regulation spans, skills).
- `kask/docs/explanation/` — explanation documentation (cognition, skills, training).
- `kask/docs/qa/` — QA strategy and per-tool contracts.
- `kask/docs/diataxis/` — per-crate Diataxis documentation set.
- `DIVERGENCE.md` stays at the zed-kask **repo root** (the fork's headline doc, referenced on every sync) and points into `kask/docs/` for detail.
The `mdz-axo/hKask` repo is archived with a `README.md` pointing to `zed-kask/kask/`.

### 14.4 Migration (complete)

The repository consolidation is complete: hKask keep-crates, MCP servers, skills registry, scripts, and docs are all under `kask/` in zed-kask. The `mdz-axo/hKask` repo is archived. `DIVERGENCE.md` lives at the repo root.

### 14.5 Connection surfaces (§13)
- **§13.1 invariant still holds:** hKask crates (under `kask/crates/hkask-*`) still must NOT depend on zed crates (under `crates/`); `kask_bridge` (under `kask/crates/`) is still the sole bidirectional seam. One repo, same rule.
- **CI script:** `kask/scripts/check-hkask-no-zed-deps.sh` enforces the dependency invariant (denylist-name check is the real gate).
- **Upstream sync:** conflicts only in the D-seam files + `[workspace.members]`/`[workspace.dependencies]`. `kask/` is additive → never conflicts. DIVERGENCE.md: "everything under `kask/` is ours; everything else is upstream."

### 14.6 Migration notes

The repository consolidation is complete. `DIVERGENCE.md` lives at the repo root (the authoritative divergence surface record). The `kask/` namespace isolates hKask from upstream. The `scripts/check-hkask-no-zed-deps.sh` CI script enforces the dependency invariant.
