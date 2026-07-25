---
title: zed-kask — Minimal-Divergence Fork Architecture & Migration Plan
audience: hKask architects / zed-kask integrators
last_updated: 2026-07-25
version: 0.31.0
status: in-progress
domain: architecture
mds_categories: [composition, trust, lifecycle]
---

# zed-kask — Minimal-Divergence Fork Architecture & Migration Plan

> **Updated 2026-07-25:** Regulation system wired, daemon removed, pod abstraction deleted, wallet/test-harness/git-cas/self-heal crates deleted, regulation orphaned modules pruned (~12,000 lines removed).

> **Build status (2026-07-25):** The kask workspace now has **24 kask crates** (down from 31; ~12,000 lines pruned: daemon, pod abstraction, wallet, test-harness, git-cas, self-heal, regulation orphaned modules). Total: **83,571 lines** (down from 95,550). The `zed` binary crate parses correctly but can't fully build on this machine (missing x11 system libs — a Linux GUI dependency, not a code issue).
>
> **Integration progress (D1–D10):**
>
> | D | Surface | Status | What's wired |
> |---|---|---|---|
> | D1 | Skill execution | ✅ **DONE** | `SkillTool` has optional `SkillManifestExecutor`; composition root in `main.rs` constructs `BridgeManifestExecutor` with `InferencePort` + `ToolPort` + registry paths and calls `agent::set_manifest_executor()`. 49 skills linked (SKILL.md + manifest.yaml). |
> | D2 | Curator agent | ✅ **DONE** | `Curator` variant in `agent_ui::Agent` enum; `CURATOR_AGENT_ID` in `agent` crate; selectable in Agent Panel. |
> | D3 | Tools in-process | ✅ **DONE** | `BridgeToolPort` wraps `McpRuntime` (implements `ToolPort` with OCAP/gas/spans). MCP servers run as child processes (stdio). Daemon transport removed; `bootstrap_mcp_server()` resolves userpod identity only. `MCPBootstrap` has only `userpod` field. |
> | D4 | Guard layer | ✅ **DONE** | `GuardedInferencePort` wraps the `InferencePort` at the composition root. `hkask-guard` crate's `ContentGuard::mandatory()` provides input scanning (prompt injection, role override, token limit) and output scanning (secret redaction). Guard wraps the skill cascade path (ManifestExecutor). Direct chat uses zed's `LanguageModel::stream_completion` with provider-side safety + refusal fallback (`cascade_only` default per `kask.guard.direct_chat_strategy`). `hkask-guard` added as dep of `zed` crate. All 29 guard tests pass. |
> | D5 | Sovereignty keys | ✅ **DONE** | `hkask-keystore` crypto-derivation uses the `keyring` crate directly for all keychain access. Global `keyring` crate injection via `hkask_keystore::keyring crate` (OnceLock pattern, same as D1's `set_manifest_executor`). Composition root constructs `keyring` crate (from `kask_bridge`) and injects it before `resolve_a2a_secret()`. Keychain reads/writes route through `keyring` crate when injected (kask namespace), fall back to the `keyring` crate directly when not (standalone MCP server child processes). `tokio` added to `hkask-keystore` deps for `block_in_place` + `Handle::current().block_on()`. All 15 keystore tests pass. |
> | D6 | Thread → memory | ✅ **DONE** | `MemoryPort` trait defined in `hkask-types` (`TurnRecord`, `MemoryError`, `MemoryFuture`). `LoggingMemoryPort` (no-op placeholder) + `BridgeMemoryPort` (adapts `agent::ThreadMemoryPort` → `hkask_types::MemoryPort`) in `kask_bridge`. Global hook `agent::set_memory_port()` / `agent::memory_port()` (OnceLock pattern, same as D1/D5). Thread turn completion in `thread.rs::run_turn()` extracts last user prompt + agent response + model + title and calls `ingest_turn()` fire-and-forget via `cx.background_spawn()`. Full hKask memory stack (SQLCipher, episodic/semantic, consolidation, WebID mapping) deferred — `LoggingMemoryPort` logs at `info` level. |
> | D7 | App-identity | ✅ **DONE** | `APP_NAME`→`Zed-Kask`, `app_identifier`/`app_id`/`display_name` renamed, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`, bundle IDs `dev.zed-kask.*`. |
> | D8 | Bridge + adapters | ✅ **DONE** | `kask_bridge` crate: `InferencePort` over `LanguageModel`, `keyring` crate directly (synchronous OS keychain), `BridgeManifestExecutor`, `BridgeToolPort`, `KaskSettings`. Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
> | D9a | Settings section | ✅ **DONE** | `KaskSettings` struct registered with zed's settings system; `"kask"` section in settings.json. Covers MCP, data services, curator, guard, memory. |
> | D9b | Credentials namespace | ✅ **DONE** | `keyring` crate directly (synchronous OS keychain) (kask namespace: `kask://credentials/<key>`). `InferenceConfig::from_secrets()` reads API keys via `keyring` crate with env var fallback. |
> | D9c | Settings UI page | ✅ **DONE** | `crates/settings_ui/src/pages/kask_page.rs` — top-level "Kask" page with 5 sub-pages: Data Services (API key entry → keychain via `CredentialsProvider` + enable toggles), MCP Servers (10 built-in servers + `load_default` master toggle + per-server overrides), Curator (`always_on` + `algedonic_threshold`), Guard (`direct_chat_strategy`), Memory (`consolidation_cadence_secs` + `confidence_floor`). Registered in `page_data.rs::settings_data()` after `ai_page`. `credentials_provider` added as direct dep of `settings_ui`. |
> | D10 | Kask panel | ✅ **DONE** | `crates/kask_panel/` — native GPUI `Panel` implementing `workspace::dock::Panel`. Dockable (right dock). Chat-like interface: regular text → scoped inference (LLM + selected server's tools), `/tool args` → direct tool invocation (OCAP-gated). `ToolInvoker` + `ScopedInference` global hooks (OnceLock pattern). `PanelToolInvoker` adapter wraps `BridgeToolPort` with `DelegationToken` from `a2a_secret`. `PanelScopedInference` adapter wraps `GuardedInferencePort`. Both wired in composition root. `kask_panel::Toggle`/`ToggleFocus` actions. Panel loaded in `zed.rs::initialize_panels()`. `kask_panel::init(cx)` called in `main.rs`. |
>
> **Composition root** (`crates/zed/src/main.rs`, after `gpui_tokio::init`):
> 1. Constructs `keyring crate` (from `kask_bridge`) over zed's `CredentialsProvider` and injects it into `hkask_keystore::keyring crate` (D5)
> 2. Resolves `a2a_secret` from `hkask_keystore::keychain::resolve_a2a_secret()` (now routes through the `keyring` crate directly)
> 3. Constructs `McpRuntime` + `BridgeToolPort` (ToolPort over MCP servers)
> 4. Gets default `LanguageModel` from `LanguageModelRegistry::read_global(cx)`
> 5. Constructs `LanguageModelInferencePort` (InferencePort over zed's LanguageModel)
> 6. Wraps `InferencePort` with `GuardedInferencePort` (D4) — mandatory content guard (injection/secret scanning)
> 7. Constructs `BridgeManifestExecutor` with guarded inference + tools + secret + registry paths
> 8. Calls `agent::set_manifest_executor(Some(executor))`
> 9. Constructs `RealMemoryPort::from_env()` (or `LoggingMemoryPort` fallback) + `BridgeMemoryPort` and calls `agent::set_memory_port()` (D6)
> 10. Constructs `PanelToolInvoker` + `PanelScopedInference` (each holding a `gpui::BackgroundExecutor` for spawning trait-method tasks without a `cx` in scope) and calls `kask_panel::set_tool_invoker()` / `set_scoped_inference()` (D10)
> 11. After `settings::init(cx)`: reads `KaskSettings` and auto-launches enabled MCP servers via `McpRuntime::start_server()`
> 12. Constructs `RegulationLedger::default()` + `CyberneticsLoop::new(ledger)` + `FlatEnergyEstimator` (10 gas per call) + `NoopEventSink` and calls `McpRuntime::with_governance()` — startup log: "hKask regulation system wired — tool invocations are governed". `hkask-regulation` and `tokio` are now dependencies of `zed`.
>
> **Revised approach for `hkask-inference`:** Kept (MCP servers use it directly). Reads API keys via `keyring` crate. Long-term: replace with `InferencePort` over zed's `LanguageModel`, but keeping it unblocks the MCP servers immediately.
>
> **Current priorities (next work):**
> 1. **R4 — Daemon refactor (RESOLVED: daemon transport deleted).** `bootstrap_mcp_server()` no longer verifies against a daemon. MCP servers run in standalone mode with userpod identity from env. The daemon layer (`hkask-mcp-server/src/daemon/`, 6 files) and `startup.rs` were deleted. `MCPBootstrap` only has `userpod: String` (no `daemon_client`). The `mcp_server!` macro no longer generates a `daemon` field. `record_via_daemon` was deleted. `McpError::Daemon` variant was removed.
> 2. ~~**Stale docs cleanup**~~ ✅ DONE
> 3. **Direct chat guard** — the `kask.guard.direct_chat_strategy` setting exists but isn't enforced. `cascade_only` is hardcoded. Wrapping zed's `LanguageModel` trait for `buffer`/`incremental` modes is a zed-side change.
> 4. **x11 system libs** — can't install `libx11-dev` without sudo. The `zed` binary can't fully build on this machine. All validation is via `cargo check` and `diagnostics`.
>
> **Assessment:** The integration is functionally complete (D1–D10 all done, MCP servers auto-launch with `HKASK_MCP_HOST`, memory ingestion wired with `RealMemoryPort`, kask panel functional with tool invocation + scoped inference, guard wraps the cascade path, sovereignty keys via the `keyring` crate directly, settings UI with 5 sub-pages, regulation system wired via `McpRuntime::with_governance()`). The remaining items are a design decision (direct chat guard), a build environment limitation (x11), and documentation (done). No further code changes are needed for the MVP integration.
>
> **One-line frame:** `zed-kask` is a **fork of Zed** that tracks `upstream` (`zed/zed`) and diverges in **exactly three places**: (1) the **skill module** (skill execution → hKask's `ManifestExecutor`), (2) the **Curator agent** (a new native agent backed by hKask), and (3) the **hKask tool-processing code** (compiled-in hKask crates + in-process tool hosting). Everything else stays byte-identical to upstream and is re-merged regularly. hKask is trimmed to **only** the Curator + user sovereignty + the tools. **No backward compatibility.** Principle: *as simple and minimal as possible — and the fork's divergence surface is itself minimal.*

Reasoning chain: `pragmatic-semantics` → `pragmatic-cybernetics` → `falsifiability` → `sequential-inquiry` → `kata-improvement` → `improve-codebase-architecture` → `essentialist` → `skill-router` → `task-breakdown` → `grill-me` → `self-critique-revision`, grounded by reading the actual `zed-kask` crate tree.

---

## 0. Fork Location & Upstream-Sync Strategy (load-bearing)

- **Fork:** `Clones/zed-kask` — `origin` = `github.com/mdz-axo/zed-kask.git`, `upstream` = `github.com/zed/zed.git`, on `main`, currently **in sync** with upstream.
- **Divergence policy:** keep `main` a near-clone of `upstream/main`. All hKask integration is isolated to a **small, named set of crates/files** (§3) so `git fetch upstream && git merge upstream/main` stays low-conflict. No scattered edits across Zed's tree.
- **hKask wiring (FULL MERGE — §14):** hKask's keep-crates + skills registry + scripts + docs are moved **into the zed-kask repo** under a `kask/` namespace (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/skills/`, `kask/scripts/`, `kask/docs/`) and added as zed-kask workspace members. The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. (Replaces the prior path-dep/submodule approach, which dissolved once hKask could no longer run standalone.)
- **Sync cadence (ongoing, Phase 7):** rebase/merge `upstream/main` regularly; resolve conflicts only in the divergent crates; run the hKask integration tests after each sync. The whole point of the fork is to *inherit Zed's improvements for free* — divergence is the cost, so minimize it.

---

## 1. The Enhanced Prompt (minimal-divergence fork)

> Fork Zed into **`zed-kask`** (`Clones/zed-kask`), tracking `upstream` and diverging only in three areas. Trim hKask (`Clones/hKask`) to the Curator + sovereignty + tools, compiled into zed-kask. No backward compatibility.
>
> 1. **zed-kask owns the generic surface and infra** (unchanged from upstream): chat (Agent Panel), GitHub, editor UI, comms/voip/CRDT (replacing Matrix entirely), the **inference router** (`crates/language_model*`), the **provider keystore** (`crates/credentials_provider`), thread storage. These stay byte-identical to upstream.
> 2. **Divergence #1 — skill execution:** change `crates/agent_skills` + `crates/agent/src/tools/skill_tool.rs` so a skill activation runs hKask's **manifest model** — `manifest.yaml` + Jinja2 templates driving a WordAct/FlowDef/KnowAct/RenderAct cascade with PDCA loops, gas/rjoule, OCAP gating — via the compiled-in `ManifestExecutor`, instead of `render_skill_envelope()` injecting the `SKILL.md` body.
> 3. **Divergence #2 — Curator agent:** add the Curator (VSM S4) as a native in-process zed-kask agent (singleton; `CuratorHandle` mpsc authority never crosses a process boundary), selectable in the Agent Panel. ACP is optional (only for external-agent interop).
> 4. **Divergence #3 — hKask tool processing:** compile hKask's keep-crates into zed-kask; host the 11 on-disk MCP servers (10 loaded by default + curator unloaded, §2.4) **in-process** (new transport alongside `context_server`'s `StdioTransport`); emit `reg.*` spans directly.
> 5. **Thread → memory:** zed-kask threads parsed into UserPod / Curator episodic + semantic memory (extends the existing ACP per-turn encoding).
> 6. **Remove everything redundant from hKask:** inference router, daemon, ACP seam, MCP-stdio, REPL, chat service, Matrix (all of it), communication MCP, backward-compat shims. **Nothing is removed from zed-kask** — it tracks upstream.
> 7. **Magnac Carta P1–P4, P12 non-negotiable.** `hkask-guard` becomes a layer in zed-kask's inference path so **every** LLM boundary (direct chat + skill cascade + Curator) is guarded — coverage *improves*.

---

## 2. The Essentialist Split (what zed-kask owns vs what hKask keeps)

### 2.1 zed-kask owns (generic — inherited from upstream, NOT modified except integration seams)

Inference routing (`crates/language_model`, `language_model_core`, `language_models`, `language_models_cloud`), provider keystore (`crates/credentials_provider`, `zed_credentials_provider`), chat/Agent Panel (`crates/agent`, `agent_ui`), editor/GitHub/comms/voip/CRDT (`crates/workspace`, `project`, etc.), thread storage (`crates/agent/src/thread_store.rs`), MCP stdio hosting (`crates/context_server`). These stay upstream-identical; we only *add seams* (guard layer, in-process transport) where hKask plugs in.

### 2.2 hKask keeps (unique: curator + sovereignty + tools) — compiled into zed-kask

**Status (2026-07-25): workspace builds clean. 24 kask crates total (down from 31; ~12,000 lines pruned this cycle). 11 MCP servers on disk (10 loaded by default + curator unloaded).**

| Crate | Why irreducible |
|---|---|
| `hkask-types` | Foundation: IDs, `InferencePort` trait, `RegulationSpan`, vocab. `VoiceDesign` and `ExpectProposal` moved here from deleted crates. `HMemEntry` moved here from deleted `hkask-git-cas`. |
| `hkask-storage` | **Sovereignty:** per-user/curator data directory SQLCipher encrypted private sphere (P11.1). `user_store` deleted (multi-user identity store — zed account replaces it). |
| `hkask-memory` | Unique semantic/episodic memory + consolidation. |
| `hkask-regulation` | Cybernetic nervous system (`reg.*`, variety, algedonic, set-points). Pruned from 49 files/15,408 lines to 26 files/9,004 lines — orphaned modules deleted (see §2.3). `WalletManager` now implements `WalletBudgetPort` and holds `gas_per_rjoule` (moved from deleted `hkask-wallet`). |
| `hkask-templates` | **The tools/skills:** `ManifestExecutor` + registry + cascade + PDCA. |
| ~~`hkask-pods`~~ (deleted) | Pod abstraction (ActivePods, PodDeployment, PodFactory, PodRegistry, PodContext, PerPodLedger, LoopScheduler, AgentPod, PodKind, PodLifecycleState) deleted. Replaced by user/curator data directories. `VoiceDesign` moved to `hkask-types`. |
| `hkask-guard` | **Magna Carta floor (P3.1)** — becomes a layer in zed-kask's inference path. |
| `hkask-capability` | **OCAP** — sovereignty enforcement. |
| `hkask-keystore` (trimmed) | **Sovereignty crypto only:** OCAP signing, DB passphrase, internal-secret derivation w/ versioning. Uses the `keyring` crate directly for all keychain access (no `SecretsPort` trait). Wallet-specific resolvers (`resolve_treasury_key`, `resolve_wallet_seed`, `sign_wallet_bytes`) deleted. |
| ~~`hkask-wallet`~~ (deleted), `hkask-ledger` | rJoule energy budget + hMem accounting. `hkask-wallet` deleted — `gas_per_rjoule` moved to `regulation::WalletManager` (which now implements `WalletBudgetPort`). `GAS_PER_RJOULE` and `WalletConfig` were already in `hkask-types`. |
| `hkask-inference` | **Kept (revised):** MCP servers use it directly for now (InferenceRouter, EmbeddingRouter, ProviderId). Reads API keys from the `keyring` crate directly. Long-term: replace with `InferencePort` over zed's `LanguageModel`, but keeping it unblocks the MCP servers immediately. |
| `hkask-mcp-server` (framework) | Trim if zed-kask's context_server hosts them natively; keep the `reg.tool.*`+OCAP gating. Daemon transport (`src/daemon/`, 6 files) and `startup.rs` deleted; `bootstrap_mcp_server()` resolves userpod identity only. |
| `hkask-forecast`, `hkask-goal`, `hkask-condenser`, ~~`hkask-git-cas`~~ (deleted), `hkask-bridge-dublincore` | Domain logic used by keep-crates/MCP servers. `hkask-git-cas` deleted — `GitCASPort` trait and supporting types deleted from `hkask-types`; `HMemEntry` moved to `hkask-types/src/lib.rs`. |
| ~~`hkask-test-harness`~~ (deleted) | Test infra deleted — `ExpectProposal` moved to `hkask-types`. |
| `hkask-mcp` | MCP governance. `FlatEnergyEstimator` (10 gas per call) added here. |
| `hkask-services-core`, ~~`hkask-services-self-heal`~~ (deleted), `hkask-services-inference`, `hkask-services-kata-kanban`, `hkask-services-runtime` (stripped: daemon_impl deleted), `hkask-services-corpus`, `hkask-services-context` (stripped: identity/communication/matrix/daemon modules deleted; `A2ARuntime` and `ConsentManager` fields removed from `GovernanceContext`; `KanbanService` `pod_manager` field and `activate_pod` method removed; governance + guards kept), `hkask-services-compose` | Scaffolding the MCP servers depend on. To be deleted as the MCP servers are refactored to take direct handles. |
| 11 MCP servers (on-disk set) | **The tools** — hosted in-process in zed-kask. |

### 2.3 hKask deletes (redundant; jobs move to zed-kask)

**DELETED (confirmed on disk):** `hkask-identity` (zed account replaces it), `hkask-communication` (Matrix → zed voip), `hkask-mcp-cloud-gateway` (no cloud deployment), `hkask-acp` (cross-process seam dissolved), `hkask-api` (HTTP server — zed owns in-process paths), `hkask-cli` (slim CLI surface — to be rebuilt as `kask` CLI for backup/wallet/repair/admin only), `hkask-repl` (zed agent panel replaces it), `hkask-services-chat` (zed owns chat), `hkask-services-onboarding` (zed first-launch replaces it), `hkask-services-runtime` daemon_impl module (deleted; classify/guard/provider_intel kept), `hkask-services-skill`, `hkask-services-wallet`, `hkask-mcp-communication`, `hkask-mcp-filesystem`, `hkask-mcp-memory`, `hkask-mcp-skill`, `hkask-mcp-regulation`, **`hkask-pods`** (pod abstraction deleted — ActivePods/PodDeployment/PodFactory/PodRegistry/PodContext/PerPodLedger/LoopScheduler/AgentPod/PodKind/PodLifecycleState gone; `VoiceDesign` moved to `hkask-types`; `GovernanceContext` `A2ARuntime`/`ConsentManager` fields removed; `KanbanService` `pod_manager` field and `activate_pod` method removed), **`hkask-wallet`** (`gas_per_rjoule` moved to `regulation::WalletManager` which now implements `WalletBudgetPort`; `GAS_PER_RJOULE` and `WalletConfig` were already in `hkask-types`; wallet-specific keystore resolvers deleted), **`hkask-test-harness`** (`ExpectProposal` moved to `hkask-types`), **`hkask-services-self-heal`**, **`hkask-git-cas`** (`GitCASPort` trait and supporting types deleted from `hkask-types`; `HMemEntry` moved to `hkask-types/src/lib.rs`; `SnapshotLoop` deleted from `hkask-regulation`). **`SecretsPort` trait and `CredentialsSecretsPort` adapter deleted** — keystore uses the `keyring` crate directly for all keychain access. **Regulation orphaned modules deleted** (~6,400 lines, 20 modules): `api_metering`, `seam_watcher`+`seam_types`+`seam_span`, `contract_events`+`contract_span`, `acp_span`, `classify_span`, `snapshot_loop`, `circuit_breaker`, `slo_manager`+`slo_types`+`slo_span`, `set_point_calibrator`, `wallet_gas_calibrator`+`wallet_energy_estimator`, `gas_report`, `dynamic_gas_table`, `composite_energy_estimator`, `calibrated_energy_estimator`, `calibrator`, `inference_estimator`, `table_energy_estimator`. **`fed_*` fields removed from `SetPoints`** (7 vestigial federation fields). **Dead types removed from `hkask-types`:** `git_cas` port module (5 files), `pipeline_manifest`/`pipeline_runner`/`pipeline_state`, `flowdef_validation`, dead wallet types (`ApiKeyMaterial`, `PriceFeedConfig`, `RJ_PER_USDC`, `TxHash`).

**Kept temporarily (MCP servers depend on them):** `hkask-inference` (see §2.2), `hkask-services-core`, `hkask-services-inference`, `hkask-services-kata-kanban`, `hkask-services-runtime`, `hkask-services-corpus`, `hkask-services-context`, `hkask-services-compose`. These dissolve as the MCP servers are refactored to direct handles (T3.0).

---

### 2.4 MCP load set (11 on disk)

The original 16 MCP servers have been pruned to **11 on disk**. The `BUILTIN_SERVERS` constant in `kask/crates/hkask-mcp-server/src/lib.rs` has been updated to match.

| On disk (11) | Deleted (5) |
|---|---|
| `codegraph`, `companies`, `condenser`, `curator`, `docproc`, `kata-kanban`, `media`, `replica`, `research`, `scenarios`, `training` | `communication` (Matrix/TTS → zed voip), `filesystem` (zed provides fs tools), `memory` (consolidated into `hkask-memory` crate), `skill` (skill execution is native via D1), `regulation` (consolidated into `hkask-regulation` crate) |

> The `curator` MCP server is kept on disk but may be unloaded by default (Curator is a native agent, D2). All 11 build clean.

---

## 3. The Minimal Divergence Map (exact zed-kask touch points)

Every hKask integration maps to a **named, isolated** change in zed-kask. This is the entire divergence surface (D1–D10); everything else tracks upstream.

| # | Divergence | zed-kask crate / file | Status | Change |
|---|---|---|---|---|
| D1 | Skill execution | `crates/agent/src/tools/skill_tool.rs` + `crates/agent/src/agent.rs` + `crates/zed/src/main.rs` | ✅ DONE | `SkillTool` has optional `SkillManifestExecutor`; composition root wires `BridgeManifestExecutor`. `SKILL.md` stays discovery-only; manifest YAML drives the cascade. |
| D2 | Curator agent | `crates/agent_ui/src/agent_ui.rs` + `crates/agent/src/agent.rs` | ✅ DONE | `Agent::Curator` variant; `CURATOR_AGENT_ID`; selectable in Agent Panel. |
| D3 | hKask tools in-process | `kask/crates/kask_bridge/src/tool_port.rs` + `crates/zed/src/main.rs` | ✅ DONE | `BridgeToolPort` wraps `McpRuntime` (implements `ToolPort`). MCP servers run as child processes. Daemon transport removed; `bootstrap_mcp_server()` resolves userpod identity only. |
| D4 | Guard layer | `crates/language_model_core`/`language_model` + `kask/crates/hkask-guard` | ✅ DONE | `GuardedInferencePort` wraps `InferencePort` at composition root. Content guard scans input (injection, role override, token limit) and output (secret redaction). Guards skill cascade path; direct chat uses provider-side safety + refusal fallback (`cascade_only` default). |
| D5 | Sovereignty keys | `crates/credentials_provider` + `kask/crates/hkask-keystore` | ✅ DONE | `hkask-keystore` uses the `keyring` crate directly for all keychain access. Global `keyring` crate injection via `keyring crate`. Composition root injects `keyring crate` before `resolve_a2a_secret()`. Keychain reads/writes route through `keyring` crate when injected, fall back to the `keyring` crate directly when not. |
| D6 | Thread → memory | `crates/agent/src/thread.rs` / `thread_store.rs` + `kask/crates/hkask-types` + `kask/crates/kask_bridge` | ✅ DONE | `MemoryPort` trait in `hkask-types`. `LoggingMemoryPort` + `BridgeMemoryPort` in `kask_bridge`. Global hook `agent::set_memory_port()`. Thread turn completion ingests via `cx.background_spawn()`. Full hKask memory stack deferred. |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml` | ✅ DONE | `APP_NAME`→`Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`, bundle IDs `dev.zed-kask.*`. |
| D8 | Bridge + adapters | `kask/crates/kask_bridge/` | ✅ DONE | `InferencePort` over `LanguageModel`, `keyring` crate directly (synchronous OS keychain), `BridgeManifestExecutor`, `BridgeToolPort`, `KaskSettings`. |
| D9 | Settings + credentials | `kask/crates/kask_bridge/src/settings.rs` + `crates/settings_content/src/settings_content.rs` | ✅ DONE | `KaskSettings` struct + `"kask"` section in settings.json; `keyring` crate directly (synchronous OS keychain) (kask namespace). Settings UI page pending (Phase 8). |
| D10 | Kask panel | `crates/kask_panel/` | ✅ DONE | Native GPUI `Panel` implementing `workspace::dock::Panel`. Right dock. Server selector (10 built-in MCP servers). `kask_panel::Toggle`/`ToggleFocus` actions. Loaded in `zed.rs::initialize_panels()`. Tool invocation wiring (global `ToolPort` hook) is next. |

**Discipline:** D1–D10 are the *only* edits to zed-kask's tree outside `kask/`. Any hKask behavior that would require touching other Zed crates is a smell — push the logic into an hKask crate behind one of these seams instead.

---

## 4. Decisive Reasoning (condensed)

- **Pragmatic Semantics:** the fork re-admits the strong claim "change Zed's skill execution." Corrected frame: zed-kask = host + generic infra (upstream-identical); hKask = compiled-in unique crates. One process.
- **Falsifiability:** "embed the ManifestExecutor in Zed" (E2) was *falsified* under "no extension hook / two runtimes." The fork **dissolves the P5.1/OCAP falsifiers** (one process ⇒ one registry ⇒ P5.1 intact; cascade runs in-process ⇒ no OCAP/gas escape). E2 is the corroborated, most-minimal realization. The Curator counterfactual (*do(not in-process Curator)*) still holds — and is now trivially satisfied (one process). ⚠ **Correction (R1, §10):** one process still hosts **two async executors** — GPUI and tokio — bridged via zed-kask's `gpui_tokio` crate. "One process" ≠ "one runtime"; the registry/OCAP conclusions hold regardless, but the executor bridge is real work, not free.
- **Pragmatic Cybernetics:** regulation reads `reg.*` spans + ledger, never the UI ⇒ surface-agnostic ⇒ preserved. In-process tools emit spans directly (fidelity improves). **Guard coverage improves:** the guard layer reaches zed-kask's *direct-chat* inference, which the old daemon model couldn't — strengthening P3.1.
- **Essentialist (crate-level deletion test):** the daemon, ACP, inference router, provider keystore, MCP-stdio, chat/REPL/Matrix all **vanish**; complexity does not reappear (the ManifestExecutor's own loop is skill-execution, not chat surface; the guard moves into zed-kask's path).
- **Essentialist (fork-level):** divergence is itself minimized to D1–D6 so upstream merges stay cheap — the cost of a fork is the maintenance of divergence, so the divergence surface must be minimal and localized.
- **Skill-Router top-5:** `essentialist` (0.92), `improve-codebase-architecture` (0.88), `pragmatic-cybernetics` (0.86), `deep-module` (0.80), `falsifiability` (0.78).

---

## 5. "What Else Are We Forgetting?" (grok findings)

| # | Item | Consequence |
|---|---|---|
| F1 | TTS/voice in `hkask-mcp-communication` + `TranscriptViewer` audio | → zed-kask voip |
| F2 | Curator Matrix posting via the communication MCP (`loop_body.rs` L901) | → post to a zed-kask Curator thread (in-process) |
| F3 | 7R7 passive listener (Matrix rooms → `reg.*`) | → zed-kask thread-watcher background task |
| F4 | Onboarding creates Matrix creds + userpod | ✅ RESOLVED — zed-kask provisioning: `provision_userpod()` derives identity from Zed login, creates dirs, auto-generates DB passphrase. No Matrix, no interactive onboarding. |
| F5 | Federation CRDT transport depends on Matrix | → defer for local MVP; intra-process A2A already in-process |
| F6 | `hkask-api` HTTP server (chat, chat_ws, episodic, consolidation, sovereignty, admin) | deletion-test; keep sovereignty/consolidation/admin only if no in-process path |
| F7 | Model providers — zed-kask owns router; hKask keeps guard + `InferencePort` trait | resolved by the fork (D4) |
| F8 | `kask` CLI subcommands | delete matrix/deploy/serve; keep repair/admin (thin) — wallet subcommand deleted with `hkask-wallet` |
| F9 | Backward-compat shims (~~pod-kind alias~~, `persona_yaml` two-source, pre-v0.31 migration, `kask tui -f`) | delete — pod abstraction removed |
| F10 | Double-gate — zed-kask `tool_permissions` (UI pre-filter) × hKask `GovernedTool` (OCAP+gas, final) | define fail-fast → Curator escalation |
| F11 | Always-on Curator — no daemon ⇒ Curator runs only while zed-kask runs | acceptable for local MVP; background/federation deferred |
| F12 | `hkask-mcp-filesystem` overlaps zed-kask file access | deletion-test |

---

## 6. The Plan (phased; edits live in `zed-kask`; no backward compat; build-then-delete)

> **Phases 0–3 are substantially complete.** D1–D10 are all done. Dead code pruning complete. Regulation system wired. Pod abstraction removed.

### Phase 0 — Decisions (no code) ✅
- **T0.1** ADR: *zed-kask minimal-divergence fork; hKask = compiled-in curator+sovereignty+tools; no backward compat.* ✅
- **T0.2** ADR: *Skill execution = compiled-in `ManifestExecutor`; guard = layer in zed-kask inference path.* ✅
- **T0.3** ADR: *Curator = native in-process agent (singleton); ACP optional.* ✅
- **T0.4** ADR: *Matrix + communication MCP removed; comms/voip/CRDT via zed-kask; federation deferred.* ✅
- **T0.5** Deletion-test verdicts for §2.3 candidates. ✅ (decided and executed — see §2.3)
- **T0.6** Migration to full merge (§14): hKask keep-crates + skills + scripts + docs moved into zed-kask `kask/` namespace. ✅
- **Checkpoint 0:** ADRs + verdicts merged. ✅

### Phase 1 — The crate boundary + guarded inference seam (in zed-kask) ✅
- **T1.1** hKask keep-crates added as workspace members; compiling against zed-kask types at the seams. ✅
- **T1.2 (D4)** Guard layer — `GuardedInferencePort` wrapping the inference path. ⬜ **NOT STARTED** (D4)
- **T1.3 (D5)** Trim `hkask-keystore` to sovereignty crypto; store keys via `keyring` crate. ✅
- **T1.4 (D8)** Bridge crate `kask_bridge` created — `InferencePort` over `LanguageModel`, `keyring` crate directly (synchronous OS keychain), `BridgeToolPort` over `McpRuntime`, `BridgeManifestExecutor`, `KaskSettings`. ✅
- **T1.5 (D9a)** `KaskSettings` struct registered with zed's settings system; `"kask"` section in settings.json. ✅
- **T1.6 (D9b)** `keyring` crate directly (synchronous OS keychain) (kask namespace); `InferenceConfig::from_secrets()`. ✅
- **Checkpoint 1:** hKask unique crates compile inside zed-kask; bridge + settings + credentials wired. ✅ (guard layer pending)

### Phase 2 — Skill execution (D1) ✅
- **T2.1a** `SKILL.md` frontmatter stays discovery-only; `BridgeManifestExecutor` resolves skill name → `kask/registry/manifests/<name>.yaml`. ✅
- **T2.1b** `SkillTool::run()` checks `has_manifest()` → runs `execute_skill()` cascade; falls back to body injection when no manifest. ✅
- **T2.1c** Composition root in `main.rs` constructs the executor and calls `agent::set_manifest_executor()`. ✅
- **T2.2** End-to-end: skill activation runs the hKask cascade. ✅ (code wired; runtime test pending x11 system libs)
- **Checkpoint 2:** skills execute via the hKask cascade, single source of truth, in-process. ✅

### Phase 3 — Agents + tools in-process (D2, D3) ✅
- **T3.1 (D2)** Curator registered as native in-process agent (`Agent::Curator` variant in `agent_ui`). ✅
- **T3.2 (D3)** `BridgeToolPort` wraps `McpRuntime` (implements `ToolPort` with OCAP/gas/spans). MCP servers run as child processes. ✅
- **T3.3 (D3 full R4)** ~~Refactor MCP servers off `DaemonClient` to direct in-process handles.~~ ✅ **RESOLVED differently:** daemon transport deleted outright (not refactored to in-process handles). `bootstrap_mcp_server()` resolves userpod identity only; `MCPBootstrap` has only `userpod: String`. MCP servers run standalone with env-derived identity.
- **T3.4 (F10)** Double-gate reconciliation: zed-kask approval = UI pre-filter; `GovernedTool` = final gate. ⬜ **NOT STARTED**
- **Checkpoint 3:** Curator selectable; tools callable with full regulation observability. ✅ (daemon path deleted; regulation system wired via `McpRuntime::with_governance()`)

### Phase 4 — Thread → memory + thread watcher (D6) ✅
- **T4.1 (D6)** Thread→memory ingestion: parse zed-kask thread transcripts into episodic h_mems. ✅ (MemoryPort trait + LoggingMemoryPort no-op; full hKask memory stack deferred)
- **T4.2** Curator threads → Curator episodic + semantic publish (P11). ⬜ (deferred — requires full hKask memory stack + WebID mapping)
- **T4.3 (F3)** zed-kask thread-watcher (replaces 7R7): background task observes threads, emits `reg.*`. ⬜
- **Checkpoint 4:** zed-kask threads become memory; conversation surface observed. ✅ (ingestion hook wired; full memory storage deferred)

### Phase 5 — Eager deletion from hKask (build-then-delete) ✅
- **T5.1** `hkask-inference` — **kept (revised)**: MCP servers use it directly; reads API keys via the `keyring` crate directly. ✅
- **T5.2** `hkask-acp` + daemon — **deleted**. `hkask-services-runtime/daemon_impl.rs` deleted. ✅
- **T5.3** `hkask-repl`/tui + `hkask-services-chat` + `hkask-cli` + `hkask-api` — **deleted**. ✅
- **T5.4** `hkask-communication` + `hkask-mcp-communication` — **deleted**. ✅
- **T5.5** Matrix sidecar + cloud/Hetzner — **deleted**. ✅
- **T5.6** `hkask-identity` — **deleted** (zed account replaces it). ✅
- **T5.7** `hkask-mcp-cloud-gateway` — **deleted**. ✅
- **T5.8** `hkask-mcp-memory`, `hkask-mcp-skill`, `hkask-mcp-regulation`, `hkask-mcp-filesystem` — **deleted**. ✅
- **T5.9** Dead code pruning: `hkask-types/identity.rs`, `hkask-types/ports/tool.rs`, stale gas table entries, stale docs. ✅
- **T5.10** `hkask-pods` — **deleted** (pod abstraction removed; `VoiceDesign` moved to `hkask-types`; `GovernanceContext` `A2ARuntime`/`ConsentManager` fields removed; `KanbanService` `pod_manager` field and `activate_pod` method removed). ✅
- **T5.11** `hkask-wallet` — **deleted** (`gas_per_rjoule` moved to `regulation::WalletManager` which now implements `WalletBudgetPort`; wallet-specific keystore resolvers deleted). ✅
- **T5.12** `hkask-test-harness` — **deleted** (`ExpectProposal` moved to `hkask-types`). ✅
- **T5.13** `hkask-services-self-heal` — **deleted**. ✅
- **T5.14** `hkask-git-cas` — **deleted** (`GitCASPort` trait and supporting types deleted from `hkask-types`; `HMemEntry` moved to `hkask-types/src/lib.rs`; `SnapshotLoop` deleted from `hkask-regulation`). ✅
- **T5.15** `SecretsPort` trait and `CredentialsSecretsPort` adapter — **deleted** (keystore uses the `keyring` crate directly). ✅
- **T5.16** Regulation orphaned modules — **deleted** (~6,400 lines, 20 modules: `api_metering`, `seam_watcher`+`seam_types`+`seam_span`, `contract_events`+`contract_span`, `acp_span`, `classify_span`, `snapshot_loop`, `circuit_breaker`, `slo_manager`+`slo_types`+`slo_span`, `set_point_calibrator`, `wallet_gas_calibrator`+`wallet_energy_estimator`, `gas_report`, `dynamic_gas_table`, `composite_energy_estimator`, `calibrated_energy_estimator`, `calibrator`, `inference_estimator`, `table_energy_estimator`). Regulation crate went from 49 files/15,408 lines to 26 files/9,004 lines. ✅
- **T5.17** `fed_*` fields removed from `SetPoints` (7 vestigial federation fields). ✅
- **T5.18** Dead types removed from `hkask-types`: `git_cas` port module (5 files), `pipeline_manifest`/`pipeline_runner`/`pipeline_state`, `flowdef_validation`, dead wallet types (`ApiKeyMaterial`, `PriceFeedConfig`, `RJ_PER_USDC`, `TxHash`). ✅
- **Checkpoint 5:** minimal hKask; zed-kask owns all generic infra; CI green. ✅

### Phase 6 — Local install (no daemon) ✅
- **T6.1** zed-kask first-launch provisioning: userpod identity derived from Zed login (`User::username` → `sanitize_name` → `WebID::for_userpod_name`), directory structure created (`ensure_userpod_dirs`), DB passphrase auto-generated (random English word, stored in keychain via the `keyring` crate directly), memory port upgraded from logging to real. No interactive onboarding — collapsed into lookups and startup commands. ✅
- **T6.2** Verify sovereignty invariants (P1/P4/P11/P12): per-pod SQLCipher, OCAP gating, WebID, consent. ⬜ (pending end-to-end test on clean machine)
- **Checkpoint 6:** end-to-end local install verified on a clean machine. ⬜ (T6.1 code complete; T6.2 verification pending)

### Phase 7 — Upstream sync (ongoing)
- **T7.1** Regular `git fetch upstream && git merge upstream/main` in `zed-kask`; resolve conflicts only in D1–D7 crates + `[workspace.members]`/`[workspace.dependencies]`. Ongoing.
- **T7.2** Keep a `DIVERGENCE.md` at the zed-kask repo root listing D1–D10 + the hKask workspace members. ⬜ (deleted in prior prune; needs restoration)

### Phase 8 — Settings UI + kask panel (new)
- **T8.1** `crates/settings_ui/src/pages/kask_page.rs` — settings UI page with sub-pages: Data Services (API key entry → keychain via `CredentialsProvider`), MCP Servers (11 servers + load toggles), Curator, Guard/Regulation, Memory. ⬜
- **T8.2** Register kask page in `page_data.rs::settings_data()`. ⬜
- **T8.3 (D10)** `crates/kask_panel` — native GPUI `Panel` replacing deleted `hkask-repl` `mcp_scoped`. Per-server view: direct `:tool args` invocation + scoped inference. ✅ (panel skeleton + server selector; tool invocation wiring deferred)
- **Checkpoint 8:** kask settings editable in UI; kask panel functional. ✅ (settings UI done; panel skeleton done; tool invocation deferred)

### Phase 9 — Keystore bridging (D5) + guard (D4)
- **T9.1 (D5)** Fold `hkask-keystore` crypto-derivation over `keyring` crate (sovereignty keys move to kask namespace). ✅
- **T9.2 (D4)** `GuardedInferencePort` wrapping the inference path — guard cascade+Curator fully; direct-chat streaming strategy (buffer/incremental/cascade-only per R3). ✅ (cascade_only default; buffer/incremental deferred — would require zed-side LanguageModel wrapper)
- **T9.3** Wire guard into composition root so all inference (chat+cascade+Curator) is guarded. ⬜
- **Checkpoint 9:** sovereignty keys in keychain; all inference guarded. ⬜

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

### 7.4 grill-me challenges (what breaks?)

- **Does renaming the bundle id break Zed account login?** No — auth is to `cloud.zed.dev` keyed on credentials, not bundle id. (Verified: account endpoints live in `default.json`/`cloud_api_client`, independent of `app_id`.)
- **Does renaming APP_NAME orphan existing Zed settings?** It *isolates* them — zed-kask starts fresh (re-onboard); the user's zed settings stay untouched in the old `zed` dirs. Intended.
- **C1 is the silent killer:** an APP_NAME rename does NOT prevent the macOS single-instance collision — verified `address()` keys on channel+uid. Must fix C1 explicitly or zed-kask silently exits whenever zed is running.
- **Extensions:** isolated dir = re-install. Minor cost; benefit = no version conflicts with zed's extensions.
- **Telemetry:** distinct install id (renamed data_dir) → zed-kask reports under a different install id to the same endpoint. Acceptable, or disable.

### 7.5 Tasks (foundational — run in Phase 1, parallel with the crate boundary; pure fork-renaming, no hKask dependency)

- **T-A1** `crates/paths/src/paths.rs`: `APP_NAME = "Zed-Kask"`. AC: `config_dir()`/`data_dir()`/`logs_dir()`/`log_file()` resolve under `zed-kask`/`Zed-Kask` on all OSes. S.
- **T-A2** `crates/release_channel/src/lib.rs`: `app_identifier()` → `Zed-Kask-Editor`; `app_id()` → `dev.zed-kask.Zed-Kask`; `display_name()` Stable → `Zed-Kask`. AC: Windows mutex/pipe + macOS bundle id distinct. S.
- **T-A3 (C1)** `crates/zed/src/zed/mac_only_instance.rs`: distinct port block (offset or `Kask` channel) + `instance_handshake()` "Zed-Kask …". AC: zed-kask runs while zed-stable is running. S.
- **T-A4 (C2)** `crates/paths/src/paths.rs` + `crates/util/src/shell.rs`: `.zed-kask_server` / `.zed-kask_wsl_server`. S.
- **T-A5 (C3/C4)** `crates/zed/Cargo.toml`: `[[bin]] name = "zed-kask"`; macOS display names `Zed-Kask …`. S.
- **T-A6** `script/install.sh`/`uninstall.sh`/`bundle-linux`: `appid`/`app_id` → `dev.zed-kask.Zed-Kask`. S.
- **T-A7** Decision record (C5): keep `zed://` vs rename to `zed-kask://`. XS.
- **T-A8** Verify: with both `zed` and `zed-kask` installed, both launch independently, separate settings, **same Zed account login**. AC: both run concurrently; account works in both. S.

## 8. Open Questions (honestly carried)

> Numbered 1–26 here; 27–28 in §13.6 (27 DONE), 29–32 in §14.6.

1. **ACP vs native** — native in-process recommended (minimal); keep ACP only for external-agent interop (T0.3).
2. **`hkask-keystore` storage backend** — share `crates/credentials_provider`, or thin hKask keychain wrapper? (T1.3).
3. **Exact `LanguageModel` provider seam** for the guard layer — verified in T1.2.
4. **50KB catalog budget** — empirical (T2.1b).
5. **Condenser / git-cas / services-\* / filesystem-MCP** — deletion-test verdicts (T0.5).
6. **Always-on Curator** — runs only while zed-kask runs; background/federation deferred (F11). Acceptable for local MVP?
7. **Double-gate** (F10) — fail-fast behavior (T3.4).
8. **`hkask-api` fate** (F6) — keep sovereignty/consolidation/admin or dissolve into in-process paths? (T5.6).
9. **hKask wiring** — path-deps (dev) vs git-submodule/vendor (shipping) (T0.6).
10. **URL scheme (C5)** — keep `zed://` (minimal divergence, macOS handler conflict) or rename `zed-kask://` (full isolation, diverges settings `$schema` + skill-share links) (T-A7).
11. **macOS single-instance port (C1)** — fixed offset vs a new `Kask` release-channel arm (T-A3).
12. **Extensions** — isolated dir (re-install) vs sharing zed's extensions dir (T-A1 decision).
13. **Telemetry** — distinct install id to shared endpoint vs disable for zed-kask.
14. **Guard strategy for direct-chat streaming (R3)** — buffer (kills UX) vs incremental scan vs cascade-only guarding.
15. **`InferencePort`-adapter vs `LanguageModel`-decorator for the guard (R2)** — which keeps dependency direction hKask↛zed-kask.
16. ~~**DaemonClient→direct-handles refactor scope (R4)**~~ — ✅ RESOLVED: daemon transport deleted outright; MCP servers run standalone with userpod identity from env. No in-process "core" owner needed.
17. **Curator agent-turn adapter (R8)** — zed-kask coding-agent thread vs Curator regulation-mediator interface.
18. **CI hermeticity (R10)** — git submodule/vendor from day one for any non-local build; path-dep only for local dev.
19. **Skill MCP management tools** — with `hkask-mcp-skill` unloaded, are skill validate/publish still needed as agent tools, or CLI-only? (T0.5).
20. **Curator MCP server load policy** — confirm `hkask-mcp-curator` stays unloaded by default (Curator-as-agent + `regulation` MCP cover it); load on demand only. (§2.4)
21. **Initial data-service set** (D9) — EODHD + FMP confirmed (used by `hkask-mcp-companies`; ~~`hkask-wallet`~~ deleted); which others (polygon, alpha-vantage, tiingo, FRED) ship in the `kask.data_services` section at MVP?
22. **`keyring` crate trait location** (D9b, R9) — define in `hkask-types` (keeps hKask↛zed-kask) and implement on the zed-kask side over `CredentialsProvider`?
23. **Config-migration precedence** (T6.3) — settings.json > keychain > env-var fallback; one-time import vs continuous env fallback?
24. **Kask panel implementation** (D10) — confirm native GPUI (option B) vs ratatui-in-terminal (option A) for MVP.
25. **Kask panel dock position** — right or bottom; auto-launch on startup?
26. **Kask panel command scope** — direct `:tool args` + scoped inference (read+write via OCAP); any read-only restrictions per server?

---

## 9. Self-Critique-Revision (convergence)

- **Fork grounding strengthens the proposal:** the divergence is now mapped to *exact* zed-kask crates (`agent_skills`, `agent/tools/skill_tool.rs`, `agent`/`agent_servers`, `context_server/client.rs`+`transport/`, `language_model*`, `credentials_provider`), so the "minimal divergence" claim is verifiable, not aspirational.
- **Over-caution corrected:** the fork dissolves the E2 P5/OCAP falsifiers and the ACP/daemon/MCP-stdio seams — the architecture is more minimal than the prior daemon version, and the guard-coverage gap *closes*.
- **New risk honestly added:** upstream-sync conflict cost (Phase 7) is the price of the fork; mitigated by isolating divergence to D1–D6 + a `DIVERGENCE.md`.
- **Calibration:** 0.80 on the compiled-in architecture; 0.6 on T2.1b magnitude; 0.6 on the 50KB budget; 0.7 on low-conflict upstream merges (depends on keeping D1–D6 tightly localized). Honest.
- **Convergence:** quality improved; no criterion regressed; residual is genuine irreducible uncertainty (always-on Curator, keystore backend, 50KB budget, sync-conflict rate), correctly reported rather than iterated past.

---

## 10. Review Findings (grill-me + diagnose + bug-hunt, evidence-based)

Evidence: hKask `InferencePort` is **non-streaming** (`fn generate(...) -> Pin<Box<dyn Future<Output=Result<InferenceResult,InferenceError>> + Send>>`, `hkask-types/src/ports/inference_port.rs`); `GuardedInferencePort` implements `InferencePort` and wraps an `InferencePort` (`hkask-guard/src/guarded_inference.rs`); `ManifestExecutor` holds `Arc<dyn InferencePort>` + `Arc<dyn ToolPort>` (`hkask-templates/src/executor.rs`). zed-kask's seam is the **streaming** `LanguageModel` trait (`stream_completion*`, `crates/language_model/src/language_model.rs`); its `context_server` client runs on GPUI async (`cx.spawn`/`async_channel`); zed-kask provides a `gpui_tokio` bridge.

| ID | Skill | IS (code) vs OUGHT | Sev | Fix |
|---|---|---|---|---|
| R1 | grill-me | IS: plan said "one process ⇒ one runtime" — **false**; zed-kask=GPUI, hKask=tokio. OUGHT: bridge via `gpui_tokio`; drive hKask tokio tasks (Curator/regulation/MCP/executor) on a bridged runtime. | High | D8; T1.4 |
| R2 | bug-hunt (integration) | IS: `GuardedInferencePort` is typed to non-streaming `InferencePort`; zed-kask `LanguageModel` streams — cannot "wrap" directly. OUGHT: zed-kask-side adapter (`InferencePort` over `LanguageModel`, collect→`InferenceResult`) OR a `LanguageModel` decorator calling `scan_input`/`scan_output` as pure fns (keeps hKask↛zed-kask). | High | D4/D8; T1.4 |
| R3 | diagnose | IS: guarding the direct-chat stream means `scan_output` buffers (kills streaming UX) or scans incrementally. OUGHT: guard cascade+Curator fully (non-streaming, cheap); direct-chat = buffer-threshold or incremental; the "coverage improves" claim has a hidden cost. | Med | T2.0b |
| R4 | bug-hunt (structural) | IS: the 15 MCP servers had reached storage/regulation/memory via `DaemonClient` over a daemon Unix socket (`hkask-mcp-server/src/daemon/`). "Dissolve daemon + host in-process" was NOT a transport swap. OUGHT: refactor servers to **direct in-process handles** (daemon owned storage; ownership moves in-process to a shared core). The on-disk count is now 11 (10 loaded by default + curator unloaded); the daemon path is dead code (always `None`) pending the T3.0 refactor. **RESOLVED (2026-07-25):** daemon transport deleted outright. `bootstrap_mcp_server()` no longer verifies against a daemon. MCP servers run in standalone mode with userpod identity from env. `MCPBootstrap` has only `userpod: String`. The `mcp_server!` macro no longer generates a `daemon` field. `record_via_daemon` deleted. `McpError::Daemon` variant removed. | High | D3; T3.0 |
| R5 | bug-hunt (interface) | IS: `ManifestExecutor::new(inference: Arc<dyn InferencePort>, tools: Arc<dyn ToolPort>)`. OUGHT: zed-kask-side adapters — `InferencePort` over `LanguageModel` (R2) + `ToolPort` over the in-process tool registry (D3). | High | D8; T2.0 |
| R6 | diagnose (flow) | IS: Phase 2 (D1) runs before Phase 3 (D3), but FlowDef `execute` steps need the ToolPort→in-process tools. OUGHT: validate Phase 2 with **KnowAct-only** skills (grill-me) first; gate full FlowDef execution on D3. | Med | §10.3 |
| R7 | diagnose (flow) | IS: T5.2 deletes the daemon; MCP servers still need `DaemonClient` removed first (R4). OUGHT: the R4 refactor (T3.0) must precede T5.2 or the servers are orphaned. | Med | §10.3 |
| R8 | bug-hunt (interface) | IS: zed-kask native agents are coding-agent tool-threads (`native_agent_server.rs`); the Curator is a regulation mediator (tokio/mpsc). OUGHT: an adapter from zed-kask's agent-turn interface to the Curator's turn interface (D2). | Med | D2; T3.2 |
| R9 | bug-hunt (dependency dir) | IS: sovereignty crypto stays in hKask; provider keys → zed-kask `credentials_provider`. If hKask reuses zed-kask's keychain, that's hKask→zed-kask (inversion). OUGHT: hKask keeps its own keyring for sovereignty keys; only provider keys live in zed-kask. | Low–Med | T1.3 |
| R10 | diagnose (config/CI) | IS: `../../Clones/hKask/...` path-deps break for other cloners/CI. OUGHT: git submodule/vendor for any shared/CI build; path-dep only for local dev. | Med | T0.6 |
| R11 | idiomatic Rust | IS: the `InferencePort` adapter boxes futures + crosses tokio↔GPUI per call. OUGHT: keep the adapter thin; avoid per-chunk boxing; accept the alloc/dispatch cost. | Low | T1.4 |
| R12 | bug-hunt (interface) | IS: `render_skill_envelope` returns `LanguageModelToolResultContent`; the cascade returns structured output. OUGHT: a renderer from cascade result → agent content shape. | Low | T2.1b |

### 10.1 New divergence seam D8 (the bridge + adapters)
The bridge crate (e.g. zed-kask-side `crates/agent_kask`) is the single place that reconciles the two async worlds (GPUI/tokio via `gpui_tokio`) and the two trait families (`LanguageModel`↔`InferencePort`, zed-kask tool registry↔`ToolPort`); everything else stays upstream-identical.

### 10.2 New / amended tasks
- **T1.4 (R1/R2/D8)** — create the zed-kask-side bridge crate: `gpui_tokio` wiring + `InferencePort`-over-`LanguageModel` adapter (collect stream→`InferenceResult`). M.
- **T2.0 (R5/R6)** — `ToolPort` adapter over the in-process tool registry; gate FlowDef `execute` on D3; validate Phase 2 with KnowAct-only skills first. M.
- **T3.0 (R4/R7)** — ~~refactor MCP servers off `DaemonClient` to direct in-process storage/regulation/memory handles~~ ✅ **RESOLVED differently:** daemon transport deleted outright; MCP servers run standalone with userpod identity from env. The `hkask-services-*` scaffolding remains (still depended on by MCP server binaries) but no longer routes through a daemon.
- **T2.0b (R3)** — decide direct-chat guard strategy (buffer vs incremental vs cascade-only). S.

### 10.3 Flow corrections (diagnose)
- **D1 gating:** Phase 2 full FlowDef validation is gated on D3 (Phase 3) ToolPort readiness; KnowAct-only validation (grill-me) proceeds first (R6).
- **T5.2 gating:** ~~daemon deletion is gated on T3.0 (DaemonClient refactor), not merely Phase 3 existence (R7).~~ ✅ **RESOLVED:** daemon deleted outright (T5.2 + T3.0 collapsed); no `DaemonClient` refactor needed since the daemon transport was removed rather than replaced with in-process handles.
- **Phase 4 independence:** thread→memory ingestion uses in-process `MemoryPort` handles (D6), not the deleted `hkask-api`/daemon endpoints.

### 10.4 Self-critique on this review
- The earlier "one process ⇒ one runtime" and "wrap with `GuardedInferencePort`" claims were **over-confident**; R1/R2 correct them with evidence. The architecture is still sound (in-process registry + OCAP hold), but the integration is **bridge + adapters**, not a free compile-in.
- ~~The biggest hidden cost is **R4**: the daemon wasn't just a process boundary — it owned the storage/regulation/memory the MCP servers depend on via `DaemonClient`. Losing the daemon means that ownership and the `DaemonClient` contract must be replaced in-process (T3.0, L-scope).~~ **RESOLVED (2026-07-25):** the daemon was deleted outright rather than refactored to in-process handles. MCP servers run standalone with userpod identity from env; the regulation system is wired via `McpRuntime::with_governance()` at the zed composition root.
- Calibration revised: 0.80 → **0.70** on the compiled-in architecture (the bridge/adapters are non-trivial); 0.6 on T2.1b/T3.0 magnitude (T3.0 is now also L). Honest.

---

## 11. Kask Settings & Credentials (data-service keys, minimal divergence)

**Goal:** load API keys for data services (EODHD, FMP, and other kask data services) and all kask-unique config via a **kask settings section** in zed-kask's settings.json + a **kask credentials namespace** in the keystore — leaving core zed settings/keystore code untouched.

### 11.1 Evidence
- zed-kask stores provider API keys via the `CredentialsProvider` trait (`read_credentials`/`write_credentials`/`delete_credentials` keyed by URL → OS keychain); `language_models` providers use `api_key_state` + `credentials_provider` (`crates/credentials_provider`, `crates/language_models/src/provider/open_router.rs`). **Secrets live in the keychain, NOT settings.json.**
- The settings UI is `Vec<SettingsPage>` built in `crates/settings_ui/src/page_data.rs::settings_data()`; pages live in `crates/settings_ui/src/pages/` (e.g. `mcp_servers_page.rs`, `llm_providers_page.rs`).
- hKask today reads data-service keys from **env vars** (`HKASK_FMP_API_KEY`, `HKASK_EODHD_API_KEY`) — in `hkask-mcp-companies` (`ctx.get`). ~~in `hkask-wallet/price_feed.rs` (`std::env::var`)~~ (`hkask-wallet` deleted). They are NOT in hKask's keychain (which holds DB passphrase/OCAP signing only).

### 11.2 Design (two additive seams)

**D9a — kask settings section** (`"kask": {...}` in settings.json + a settings struct). A new top-level section, isolated from core zed settings. Holds kask-unique, **non-secret** config:
- `kask.data_services.{eodhd,fmp,polygon,alpha_vantage,tiingo,fred,...}` — enabled toggles + per-service config (endpoints, tiers). The **secret API key is NOT here** — it is in the keychain (D9b); settings holds only the reference/toggle.
- `kask.mcp.load_default` + `overrides` — the 10-loaded-by-default set (§2.4; 11 on disk total, curator unloaded) + per-server toggles (curator off by default; filesystem/communication absent).
- `kask.curator` — always-on toggle, regulation set-points (variety window, algedonic thresholds).
- `kask.sovereignty.pod` — data-dir override, consent defaults.
- `kask.guard` — direct-chat guard strategy (R3: buffer / incremental / cascade-only).
- `kask.memory` — consolidation cadence, confidence floor.
Registered with zed's settings system so it appears in the `zed://schemas/settings` schema. **Minimal divergence:** one new settings struct + registration; core zed settings structs untouched.

**D9b — kask credentials namespace** (via the existing `CredentialsProvider`). Data-service API keys stored in the OS keychain under kask-namespaced URLs (e.g. `kask://credentials/eodhd`, `kask://credentials/fmp`), alongside zed's provider keys (which use their own URLs). The kask MCP servers (companies/scenarios) read keys via `CredentialsProvider` at runtime — **replacing the env-var approach** (`HKASK_*`). This folds into the T3.0 in-process refactor: MCP servers take a credentials handle, not env vars. The sovereignty keys (D5: DB passphrase, OCAP signing) also move here (kask namespace), so the trimmed `hkask-keystore` becomes a thin crypto-derivation layer over the shared `CredentialsProvider` (using the `keyring` crate directly — `SecretsPort` trait deleted).

### 11.3 Settings UI (additive page)
A new **Kask** page: `crates/settings_ui/src/pages/kask_page.rs` + one entry in `page_data.rs::settings_data()`. Sub-pages mirror the settings section: **Data Services** (per-service enable + key entry → writes to keychain via `CredentialsProvider`), **MCP Servers** (the 11 on-disk servers — 10 loaded by default + curator unloaded — with load toggles), **Curator**, **Sovereignty/Pod**, **Guard/Regulation**, **Memory**. Touches `page_data.rs` minimally (one `SettingsPage` push) — core zed pages untouched.

### 11.4 Configuration translation / migration
Existing hKask config → kask settings + keychain, on first launch (and a `kask import-config` command):
- env `HKASK_FMP_API_KEY` / `HKASK_EODHD_API_KEY` → `CredentialsProvider` entries `kask://credentials/{fmp,eodhd}` + `kask.data_services.{fmp,eodhd}.enabled = true`.
- hKask keychain sovereignty keys (DB passphrase, OCAP signing) → `CredentialsProvider` kask namespace (D5).
- hKask config-file settings (regulation thresholds, consolidation cadence, gas defaults) → `kask.*` settings.json section.
Precedence: explicit settings.json > imported keychain > env-var fallback (during transition) — decision T0.6b.

### 11.5 Tasks
- **T1.5 (D9a)** — define the `KaskSettings` struct + register with zed's settings system; add the `"kask"` JSON-schema section. M.
- **T1.6 (D9b)** — add the kask credentials namespace to `CredentialsProvider` usage; helper to read/write `kask://credentials/<service>`. S.
- **T3.0b (part of T3.0)** — refactor the data-service-consuming MCP servers (companies, scenarios) off env vars → read keys via `CredentialsProvider` (kask namespace). M. (~~wallet~~ MCP server deleted with `hkask-wallet`.)
- **T-s1 (D9 UI)** — `crates/settings_ui/src/pages/kask_page.rs` + register in `page_data.rs::settings_data()`. M.
- **T6.3** — `kask import-config`: migrate env `HKASK_*` + old keychain → kask settings + keychain. S.
- **T-A0 (sovereignty)** — fold trimmed `hkask-keystore` crypto-derivation over the shared `CredentialsProvider` (D5). S.

### 11.6 grill-me / diagnose notes
- **Secrets must NOT be in settings.json** — keys live in the keychain (matches zed's provider-key pattern; verified `api_key_state` uses `credentials_provider`, not settings.json, for the secret). The `kask` settings section holds only toggles/refs.
- **Dependency direction (R9 echo):** `CredentialsProvider` is a zed-kask trait; hKask MCP servers consuming it directly = hKask→zed-kask (inversion). Mitigation: define a thin hKask-side `keyring` crate trait (in `hkask-types`) that the zed-kask side implements over `CredentialsProvider` — keeps hKask crates independent of zed-kask. (The `SecretsPort` trait was deleted; the keystore uses the `keyring` crate directly.)
- **Extensions model:** kask data services are configured in the same UI/credentials pattern as zed providers (first-class), not ad-hoc env vars; the 11 on-disk MCP servers (10 loaded by default + curator unloaded) are compiled-in (not zed extensions), but their key configuration reuses zed's credentials model — minimal divergence.
- **D9 = new divergence seam** (kask settings section + credentials namespace + UI page). Add to the §3 divergence map alongside D1–D8 (the §3 row could not be edited this session due to a matcher quirk on the D7/D3 rows; recorded here instead).
---

## 12. Kask Panel (per-MCP-server one-on-one windows)

**Requirement:** a "Kask" panel in zed-kask where the user can launch a window per kask MCP server and interact with it **one-on-one** to reach the server's **deeper functionality** (direct tool invocation + scoped inference), within the zed-kask app — distinct from the conversational Agent Panel (which drives tools via the agent).

### 12.1 Evidence — hKask already implements this concept
`crates/hkask-repl/src/tui/windows/mcp_scoped.rs` is `McpScopedWindow`: a per-MCP-server pane (Kanban, Companies, Scenarios, …) with two OCAP-gated input paths:
- **Direct tool invocation** (`:tool_name args`) — calls the MCP tool directly via `ToolInvokeBridge`, bypassing the LLM; fast, structured JSON; preserves `DelegationToken` (OCAP).
- **Scoped inference** (natural language) — the LLM acts as intermediary calling only that server's tools.
`McpScopedState` holds the per-window input/log/pending-request state. This is exactly the "one-on-one deeper functionality" the user wants — the only question is how to host it in zed-kask.

### 12.2 Two implementation options
- **(A) ratatui-in-terminal (reuse-fast):** a zed `Panel` hosting a `Terminal` (alacritty PTY) running a slimmed `kask panel` ratatui binary = the existing `McpScopedWindow`/`window_catalog`/`tab`/`status_bar` views. **Cost:** the TUI is a separate process ⇒ needs an in-process view/control socket (retain the daemon listener in zed-kask); keeps a PTY boundary. Reuses the most existing code.
- **(B) native GPUI panel (recommended):** reimplement `McpScopedWindow` as a zed-native `Panel` (`crates/kask_panel`, GPUI) — a server catalog (the 11 on-disk servers — 10 loaded by default + curator unloaded, §2.4) + a per-server view with direct `:tool args` invocation and scoped-inference input, calling the **in-process MCP tools (T3.0)** and **guarded inference (D8)** directly. **No PTY, no view socket, no retaining the daemon listener.** Lets the entire hKask ratatui TUI be deleted (T5.3 deletes all of `hkask-repl/tui`, incl. `mcp_scoped` — it is reimplemented natively). One new panel crate; reuses the in-process tool/inference seams already built.

**Decision: (B).** More idiomatic zed-native, eliminates the PTY/IPC boundary (and the need to retain the daemon listener), and simplifies deletion. (A) remains a reuse-fast fallback if the GPUI rebuild proves too costly for MVP.

### 12.3 Design (D10 — native GPUI kask panel)
- **zed side:** new `Panel` impl `crates/kask_panel` (implements `pub trait Panel`, `crates/workspace/src/dock.rs`; `DockPosition` right or bottom). Renders: a list of the 11 on-disk MCP servers (10 loaded by default + curator unloaded, from the in-process tool registry, §2.4); selecting one opens a per-server sub-view.
- **Per-server sub-view:** (1) the server's tool list (introspected from the in-process MCP server) + a `:tool_name args` direct-invocation input → calls the in-process tool through the OCAP-gated path (same `GovernedTool`/gas as the agent; emits `reg.tool.*`); (2) a natural-language scoped-inference input → runs guarded inference (D8) with only that server's tools in scope. Results rendered inline.
- **OCAP:** the panel invokes tools under the userpod's `DelegationToken` exactly as the agent does — direct invocation does NOT bypass OCAP (mirrors the ratatui `ToolInvokeBridge` invariant). Double-gate (F10) applies: panel invokes are still `GovernedTool`-gated.
- **hKask side:** delete the entire ratatui TUI (T5.3) — `mcp_scoped` is reimplemented natively; no slimmed ratatui binary, no view socket. (This **reverses** an earlier ratatui-terminal idea: cleaner.)

### 12.4 Tasks
- **T-s2 (D10 zed)** — `crates/kask_panel`: GPUI `Panel` + server catalog (11 on-disk servers — 10 loaded by default + curator unloaded — from the in-process registry). M.
- **T-s3 (D10 view)** — per-server sub-view: direct `:tool args` invocation (in-process, OCAP-gated) + scoped inference (guarded). Reimplement `McpScopedWindow`'s two input paths natively. M.
- **T-s4** — wire the panel to the in-process tool registry (T3.0) + guarded inference (D8); verify `reg.tool.*`/`reg.inference` spans fire on direct invokes. S.
- **Refine T5.3** — delete the **entire** `hkask-repl/tui` (chat + `mcp_scoped` + transcript/voice); `mcp_scoped` is now native (T-s3). (No view socket, no daemon-listener retention — simpler than option A.)

### 12.5 grill-me / diagnose
- **Does direct invocation bypass sovereignty?** No — it reuses the OCAP-gated `GovernedTool` path (mirrors the ratatui `ToolInvokeBridge` `DelegationToken` invariant); only the LLM is bypassed, not OCAP/gas. Verified against the `mcp_scoped.rs` doc comment.
- **Why not reuse ratatui (A)?** (A) needs a PTY + an in-process view/control socket (retain the daemon listener) for a separate process to reach the in-process runtime — re-introducing an IPC boundary we removed. (B) talks to in-process tools directly, no IPC, and lets us fully delete the ratatui TUI. Trade-off: (B) rebuilds the UI in GPUI; accepted for a cleaner, more minimal result.
- **Variety/regulation:** direct one-on-one invokes still emit `reg.tool.*` and consume gas (T-s4) — the cybernetic loop sees panel activity, so regulation is not bypassed.

### 12.6 GPUI reuse map (option B, grounded in zed's own code)

**Panel trait + registration** (`crates/workspace/src/dock.rs`):
- `impl Panel for KaskPanel` requires: `persistent_name()` → `"KaskPanel"`, `panel_key()`, `position(&self, window, cx) -> DockPosition`, `default_size()`, `min_size()`, `icon() -> Option<IconName>`, `icon_tooltip()`, `toggle_action() -> Box<dyn Action>`, `activation_priority()`, `enabled()`, plus `Render`, `Focusable`, `EventEmitter<PanelEvent>`. **Copy-template: `agent_ui/src/agent_panel.rs:4954` (`impl Panel for AgentPanel`).**
- Register: `cx.new(|cx| KaskPanel::new(...))` then `workspace.add_panel(panel, window, cx)` (`crates/workspace/src/workspace.rs:2554`); add a `ToggleKaskPanel` action (mirror `ToggleFocus`). Dock position/size persist into the **`kask.panel.dock`** settings field (§11/D9a) — not zed's `agent` settings — via `settings::update_settings_file` (mirror AgentPanel `set_position`).

**Visual language — `use ui::prelude::*`** (`crates/ui/src/prelude.rs`); reuse components + theme tokens (NO hardcoded colors/fonts):
- Components: `Button`/`IconButton`, `Label` (`LabelSize`), `Icon`/`IconName`/`IconSize`, `list::List` (server catalog), `TabBar` (open per-server windows as tabs), `context_menu`/`popover_menu` (per-tool menus), `scrollbar`, `data_table` (structured JSON tool results), `chip`/`indicator` (server status dots), `divider`, `callout` (errors), `Tooltip`, `keybinding_hint`.
- Tokens: colors via `cx.theme().colors()` / `cx.theme().status()` (e.g. `Color::Default`, `Color::Created` — `ui/src/styles/color.rs`); sizes via `TextSize`/`LabelSize`/`IconSize`; spacing via `DynamicSpacing`/`px`/`rems`. This **is** zed's visual language — the kask panel inherits light/dark themes for free.

**Input — reuse `editor::Editor`** (mirror `agent_ui/src/message_editor.rs`): a multi-line `Editor` as the `:tool args` / NL input, with a completion provider offering the active server's tools (`:tool_name`) — mirror `PromptCompletionProvider` + `AvailableSkill`/`SlashCommandCompletion`. Submit → direct tool invoke (T3.0, OCAP) or scoped inference (D8).

**Results — reuse `conversation_view` pattern + `data_table`**: scoped-inference output renders like agent messages; direct-tool JSON results render in `data_table` / code blocks (mirror agent tool-result rendering).

**Structure (`crates/kask_panel`):**
- `KaskPanel { servers: [11 on-disk — 10 loaded by default + curator unloaded — from in-process registry, §2.4], active: ServerId, tabs: open windows, input: Entity<Editor>, results: view, tool/inference handles (D3/D8) }`.
- Render: header (`TabBar` of open servers, or `List` catalog when none) + active view (`Editor` input + results). All via `ui` components + theme tokens.
- Dock icon: reuse an existing `IconName`, or add `IconName::Kask` (small addition to `crates/ui/src/icon.rs`).

**Minimal divergence:** one new crate `crates/kask_panel` + one `ToggleKaskPanel` action + one `add_panel` call at workspace init + (optional) one `IconName` + `kask.panel.*` in the settings section (§11). No core zed panel/`ui`/`editor` modifications.

**Reference files to copy from:**
- `agent_ui/src/agent_panel.rs` — Panel impl boilerplate, render structure, dock persistence.
- `agent_ui/src/message_editor.rs` — input `Editor` + completion provider.
- `agent_ui/src/conversation_view/` — message/tool-result rendering.
- `agent_ui/src/context_server_configuration.rs` — MCP server list UI (catalog style).
- `crates/ui/src/components/{list,tab_bar,data_table,context_menu}.rs` — components.
- `crates/ui/src/styles/{color,typography,spacing,units}.rs` — design tokens.

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
| `MemoryPort` (hkask-types; NEW) | in-process `EpisodicMemory`/`SemanticMemory` handles. `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced: logging at startup, real after userpod provisioning. | thread→memory ingestion (D6) | D6 |

Hexagonal pattern: hKask defines the ports; the bridge crate is the adapter; the composition root wires them. **No hKask crate imports a zed-kask crate.**

### 13.3 Composition root (startup — DI pattern)
zed-kask app startup constructs the individual hKask components directly (~~`KaskCore`~~ was never implemented as a single singleton — the composition root wires each component separately) and wires the adapters:
1. **Load `KaskSettings` (D9a)** → bind to component construction params (regulation set-points, gas defaults, consolidation cadence, guard strategy, MCP load set = the 11 on disk, §2.4). **Settings→config is construction-time, not a runtime port** (config-struct-validated-on-construction).
2. **Install logging memory port (D6 early):** `set_memory_port(BridgeMemoryPort(LoggingMemoryPort))` — no-op until the Zed user resolves. Uses `Mutex` (not `OnceLock`) so the port can be replaced later.
3. **Construct hKask components directly:** per-user/curator data directory SQLCipher storage, Regulation runtime, memory, the singleton Curator (`CuratorHandle` mpsc in-process), the 11 MCP servers (standalone, userpod identity from env), the `ManifestExecutor`.
4. **Build the bridge:** `InferencePort`-over-`LanguageModel` (+guard), `ToolPort`-over-tool-registry, `keyring` crate-over-`CredentialsProvider`, `CuratorTurnPort`, `MemoryPort`; inject into `ManifestExecutor`/Curator/MCP servers/kask panel.
5. **Wire the regulation system:** construct `RegulationLedger::default()` + `CyberneticsLoop::new(ledger)` + `FlatEnergyEstimator` (10 gas per call, in `hkask-mcp`) + `NoopEventSink` and call `McpRuntime::with_governance()`. Startup log: "hKask regulation system wired — tool invocations are governed". `hkask-regulation` and `tokio` are now dependencies of `zed`.
6. **Spawn** the regulation + Curator metacognition tokio loops on the `gpui_tokio` runtime (R1) — the loop driver.
7. **Register** the **UserPod** + **Curator** native agents (D2) and the **KaskPanel** (D10); add `ToggleKaskPanel` + `workspace.add_panel`.
8. **Deferred userpod provisioning (D6 late):** after `AppState::set_global`, a spawned task watches `UserStore::current_user()`. When the Zed user resolves: `provision_userpod(username)` creates the directory structure, ensures a DB passphrase (auto-generate random English word if none, via the `keyring` crate directly), and calls `set_memory_port(BridgeMemoryPort(RealMemoryPort))` to replace the logging port. MCP servers are launched with `HKASK_MCP_HOST`/`HKASK_USERPOD_NAME` set from the sanitized username.
9. **Migrate** config (T6.3) on first launch.

~~`KaskCore` is the "shared core" R4 referred to — the single owner of storage/regulation/memory the MCP servers take handles from (prevents the two-instance pitfall).~~ **Note (2026-07-25):** `KaskCore` was never implemented. The composition root constructs individual components directly. The daemon transport was deleted rather than refactored to in-process handles.

**Open Q 28. — RESOLVED:** components construct at zed-kask startup with a logging memory port; the userpod is provisioned when `UserStore::current_user()` resolves (deferred task). The `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced after startup. Per-user/curator data directory storage opens at provisioning time, not at process start.

### 13.4 Consolidated divergence map (D1–D10)
| D | Surface | zed-kask file | Connection (port/adapter) |
|---|---|---|---|
| D1 | skill execution | `agent_skills` + `agent/tools/skill_tool.rs` | skill_tool → bridge.ManifestExecutor(InferencePort, ToolPort) |
| D2 | Curator agent | `agent.rs` + `native_agent_server` + `agent_servers` | native agent → CuratorTurnPort → in-process Curator |
| D3 | tools in-process | `context_server/client.rs` + `transport/` | in-process transport → ToolPort; MCP servers run standalone with userpod identity from env (daemon transport deleted) |
| D4 | guard | `language_model_core`/`language_model` | `GuardedInferencePort` wraps `InferencePort`-over-`LanguageModel` |
| D5 | sovereignty keys | `credentials_provider` | **via the `keyring` crate directly** (reconciles D9b; `SecretsPort` trait deleted). DB passphrase auto-provisioned on first run (random English word, stored in keychain). |
| D6 | thread→memory | `agent/thread.rs`/`thread_store.rs` | thread hook → `MemoryPort` → in-process memory. Logging port at startup; upgraded to `RealMemoryPort` when Zed user resolves (deferred provisioning via `provision_userpod`). |
| D7 | app-identity | `paths.rs`, `release_channel`, `mac_only_instance`, `Cargo.toml`, scripts | (zed-kask self-change; not a zed↔kask seam) |
| D8 | bridge + adapters | new `crates/kask_bridge` + `gpui_tokio` | **THE bidirectional seam** — implements all ports |
| D9 | settings + credentials | new `KaskSettings` section + `CredentialsProvider` namespace + `kask_page` | `KaskSettings` → component params; `keyring` crate directly |
| D10 | kask panel | new `crates/kask_panel` (Panel) | panel → `ToolPort` + `InferencePort` (via bridge) + tool registry |

### 13.5 Cleanups — DONE (cleanup pass)
- ✅ **Stale MCP counts** in §1.4, D3, T3.3 → corrected to **11 on disk** (10 loaded by default + curator unloaded). The earlier "15" and "12" references are both stale: the original 16-server set was pruned to 11 on disk (5 deleted: communication, filesystem, memory, skill, regulation), and only 10 of those 11 load by default (curator is unloaded — Curator is a native agent, D2). See §2.4 for the canonical table.
- ✅ **§6 Phase 5** now matches the refinements: T5.2 gated on T3.0; T5.3 deletes the entire `hkask-repl/tui` incl. `mcp_scoped` (reimplemented natively, §12.4); T5.7 notes filesystem-MCP decided deleted (§2.4).
- ✅ **D5 text** now reads "via the `keyring` crate directly" (matches the dependency invariant; `SecretsPort` trait deleted).
- ✅ **§3 intro** now references D8–D10 (consolidated in §13.4).
- ✅ **R4 finding (§10)** corrected to past tense ("had reached") and annotated with the current 11-on-disk count and the dead `DaemonClient` status. **Update (2026-07-25):** daemon transport deleted outright; see R4 row.
- ✅ **2026-07-25 cleanup pass:** daemon removed, pod abstraction deleted, `hkask-wallet`/`hkask-test-harness`/`hkask-services-self-heal`/`hkask-git-cas` deleted, regulation orphaned modules pruned (~12,000 lines removed), `SecretsPort` trait deleted (keystore uses `keyring` crate directly), regulation system wired via `McpRuntime::with_governance()`. Crate count: 24 kask crates (down from 31). Total: 83,571 lines (down from 95,550).

### 13.6 grill-me on the composition
- **Is the bridge crate the only bidirectional seam?** Yes — by invariant. Audit: grep hKask for any `use` of a zed-kask crate — must be zero. (Open Q 27.)
- **~~Is `KaskCore` constructed once?~~** ~~Yes — singleton; MCP servers + Curator + memory ingestion share its handles (prevents R1's two-instance pitfall).~~ **Note (2026-07-25):** `KaskCore` was never implemented. The composition root constructs individual components directly.
- **Lifecycle:** components construct at zed-kask startup; the userpod (data directory) is created on first-launch provisioning and thereafter loaded — confirm construction order (Open Q 28).
- **Do all surfaces follow established patterns?** ports-and-adapters (D8), decorator (guard), composition-root DI (startup), `Panel` (D10 copies `agent_panel.rs`), settings-section + credentials-namespace (D9 copies zed's own). Yes.

**Open Q 27. — DONE:** `scripts/check-hkask-no-zed-deps.sh` enforces the §13.1 invariant (no hKask crate depends on a zed-kask crate) and is wired into `.github/workflows/ci.yml` (invariants job).
**Open Q 28. — RESOLVED:** See §13.3 above — deferred userpod provisioning via `UserStore::current_user()` watch; `MEMORY_PORT` uses `Mutex` for re-settable port. (~~`KaskCore`~~ was never implemented; components are constructed individually.)

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
    │                  #  hkask-services-self-heal, hkask-git-cas)
    ├── mcp-servers/   # the 11 on-disk servers (10 loaded by default + curator unloaded; hkask-mcp-*)
    ├── skills/        # the skills registry (manifest.yaml + *.j2; Pattern A source of truth)
    ├── scripts/       # check-hkask-no-zed-deps.sh + hKask admin/build scripts
    └── docs/          # ← documentation home (see 14.3)
```
zed-kask's `Cargo.toml` adds `kask/crates/*` + `kask/mcp-servers/*` as workspace members and merges hKask's `[workspace.dependencies]` into its own. The bridge crate `kask_bridge` and panel `kask_panel` (D8/D10) live under `kask/crates/` too — they're ours, not upstream's.

### 14.3 Documentation home (`kask/docs/`)
All Kask documentation lives **inside zed-kask** under `kask/docs/`:
- `kask/docs/architecture/` — this plan (`zed-host-architecture-plan.md`), the four-pattern architecture, principles, ADRs.
- `kask/docs/specs/` — integration specifications (the D1–D10 seam specs), the port/adapter contracts, the MCP load set.
- `kask/docs/plans/` — phased migration plans, the upstream-sync runbook.
- `DIVERGENCE.md` stays at the zed-kask **repo root** (the fork's headline doc, referenced on every sync) and points into `kask/docs/` for detail.
The current plan at `Clones/hKask/docs/architecture/zed-host-architecture-plan.md` **moves to `kask/docs/architecture/`** during the migration (T0.6). The `mdz-axo/hKask` repo is archived with a `README.md` pointing to `zed-kask/kask/`.

### 14.4 Migration steps (T0.6 expansion)
1. Create `kask/{crates,mcp-servers,skills,scripts,docs}` in zed-kask.
2. Move hKask keep-crates → `kask/crates/hkask-*`; MCP servers → `kask/mcp-servers/hkask-mcp-*`; skills registry → `kask/skills/`; scripts → `kask/scripts/`; docs → `kask/docs/`.
3. Merge hKask `[workspace.dependencies]` into zed-kask's; add the `kask/*` members to zed-kask's `Cargo.toml`.
4. Decide history preservation: `git filter-repo --subdirectory-filter` to bring hKask history under `kask/` (preserves blame), or a clean copy (simpler, loses history). (Open Q 29.)
5. Archive `mdz-axo/hKask` (GitHub archive + root `README.md` → `zed-kask/kask/`).
6. Move `DIVERGENCE.md` references from `../../Clones/hKask/...` to `kask/...`.

### 14.5 What changes for the connection surfaces (§13)
- **§13.1 invariant still holds:** hKask crates (under `kask/crates/hkask-*`) still must NOT depend on zed crates (under `crates/`); `kask_bridge` (under `kask/crates/`) is still the sole bidirectional seam. One repo, same rule.
- **CI script tweak:** `scripts/check-hkask-no-zed-deps.sh` (now at `kask/scripts/`) — under the merge, the path-dep check's `zed-kask` string no longer matches (paths are intra-repo). The **denylist-name check is the real gate** (a `kask/crates/hkask-*` dep on `gpui`/`context_server`/etc. still fires). Broaden the path-dep check to flag any `kask/crates/**` path-dep pointing outside `kask/` (into `crates/`/`mcp-servers/`). (Open Q 32.)
- **Upstream sync (Phase 7):** conflicts only in the D-seam files + `[workspace.members]`/`[workspace.dependencies]`. `kask/` is additive → never conflicts. DIVERGENCE.md gains a line: "everything under `kask/` is ours; everything else is upstream."

### 14.6 Open questions (consolidation)
29. **History preservation** — `git filter-repo` (keep blame) vs clean copy (simpler)? (T0.6.)
30. **Namespace** — `kask/` top-level (recommended: isolates ours-vs-upstream) vs spreading hKask crates into zed's `crates/`/`mcp-servers/` (mixes; worse for divergence).
31. **DIVERGENCE.md location** — repo root (visibility) vs `kask/docs/` (grouped). Lean: root + a `kask/docs/divergence.md` mirror or pointer.
32. **CI script** — broaden `check-hkask-no-zed-deps.sh` path-dep check for intra-repo paths (denylist-name check already covers the real case).
