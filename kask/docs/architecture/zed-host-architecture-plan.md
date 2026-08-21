---
title: "zed-kask — Minimal-Divergence Fork Architecture & Migration Plan"
audience: [architects, integrators]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust, lifecycle]
---

# zed-kask — Minimal-Divergence Fork Architecture & Migration Plan

> **One-line frame:** `zed-kask` is a **fork of Zed** that tracks `upstream` (`zed/zed`) and diverges in **exactly three places**: (1) the **skill module** (skill execution via upstream-Zed body injection — `SkillTool::run` reads the `SKILL.md` body and injects it through `render_skill_envelope`), (2) the **Curator agent** (a new native agent backed by hKask), and (3) the **hKask tool-processing code** (compiled-in hKask crates + in-process tool hosting). Everything else stays byte-identical to upstream and is re-merged regularly. hKask is trimmed to **only** the Curator + user sovereignty + the tools. **No backward compatibility.** Principle: _as simple and minimal as possible — and the fork's divergence surface is itself minimal._

## Table of contents

- [§0 — Fork Location & Upstream-Sync Strategy](#0-fork-location--upstream-sync-strategy-load-bearing)
- [§1 — The Enhanced Prompt (minimal-divergence fork)](#1-the-enhanced-prompt-minimal-divergence-fork)
- [§2 — The Essentialist Split](#2-the-essentialist-split-what-zed-kask-owns-vs-what-hkask-keeps)
  - [§2.1 — zed-kask owns (generic)](#21-zed-kask-owns-generic--inherited-from-upstream-not-modified-except-integration-seams)
  - [§2.2 — hKask keeps (unique: curator + sovereignty + tools)](#22-hkask-keeps-unique-curator--sovereignty--tools--compiled-into-zed-kask)
  - [§2.3 — MCP load set (10 on disk)](#23-mcp-load-set-10-on-disk)
- [§3 — The Minimal Divergence Map (D1–D32)](#3-the-minimal-divergence-map-exact-zed-kask-touch-points)
- [§4 — (removed)](#4-removed)
- [§5 — (removed)](#5-removed)
- [§6 — Migration Status (removed)](#6-migration-status)
- [§7 — App-Identity Separation](#7-app-identity-separation-zed-kask--zed-coexistence)
- [§8 — Architecture Notes (removed)](#8-architecture-notes)
- [§9 — (removed)](#9-removed)
- [§10 — (removed)](#10-removed)
- [§11 — Kask Settings & Credentials](#11-kask-settings--credentials-data-service-keys-minimal-divergence)
- [§12 — Kask Panel (removed)](#12-kask-panel-removed)
- [§13 — Composition & Connection Surfaces](#13-composition--connection-surfaces-zoom-out-review)
  - [§13.1 — Governing invariant (dependency direction)](#131-governing-invariant-dependency-direction)
  - [§13.2 — The complete port set](#132-the-complete-port-set-ports-and-adapters)
  - [§13.3 — Composition root (startup — DI pattern)](#133-composition-root-startup--di-pattern)
  - [§13.4 — Consolidated divergence map (D1–D32)](#134-consolidated-divergence-map-d1d32)
- [§14 — Repository Consolidation](#14-repository-consolidation--full-merge-into-zed-kask)
- [References](#references)

---

> **Current state (2026-08-20):** The kask workspace has **16 kask crates** under `kask/crates/` (15 `hkask-*` + `kask_bridge`) plus 10 MCP server crates under `kask/mcp-servers/` and 1 zed-side crate (`crates/kask_extensions_ui/`). D1–D32 are wired at the composition root (`crates/zed/src/main.rs`); the authoritative divergence surface is [`DIVERGENCE.md`](../../../DIVERGENCE.md) at the repo root. The composition-root wiring is documented in [§13.3](#133-composition-root-startup--di-pattern).

---

## 0. Fork Location & Upstream-Sync Strategy (load-bearing)

- **Fork:** `Clones/zed-kask` — `origin` = `github.com/mdz-axo/zed-kask.git`, `upstream` = `github.com/zed/zed.git`, on `main`, currently **in sync** with upstream.
- **Divergence policy:** keep `main` a near-clone of `upstream/main`. All hKask integration is isolated to a **small, named set of crates/files** (§3) so `git fetch upstream && git merge upstream/main` stays low-conflict. No scattered edits across Zed's tree.
- **hKask wiring (FULL MERGE — §14):** hKask's keep-crates + skills registry + scripts + docs are moved **into the zed-kask repo** under a `kask/` namespace (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/skills/`, `kask/scripts/`, `kask/docs/`) and added as zed-kask workspace members. The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. (Replaces the prior path-dep/submodule approach, which dissolved once hKask could no longer run standalone.)
- **Sync cadence (ongoing, Phase 7):** rebase/merge `upstream/main` regularly; resolve conflicts only in the divergent crates; run the hKask integration tests after each sync. The whole point of the fork is to _inherit Zed's improvements for free_ — divergence is the cost, so minimize it.[^fowler-strangler]

---

## 1. The Enhanced Prompt (minimal-divergence fork)

> Fork Zed into **`zed-kask`** (`Clones/zed-kask`), tracking `upstream` and diverging only in three areas.[^conway] hKask is trimmed to the Curator + sovereignty + tools, compiled into zed-kask under the `kask/` namespace. No backward compatibility.
>
> 1. **zed-kask owns the generic surface and infra** (unchanged from upstream): chat (Agent Panel), GitHub, editor UI, comms/voip/CRDT (replacing Matrix entirely), the **inference router** (`crates/language_model*`), the **provider keystore** (`crates/credentials_provider`), thread storage. These stay byte-identical to upstream.
> 2. **Divergence #1 — skill execution:** `crates/agent/src/tools/skill_tool.rs` (`SkillTool::run`) reads the `SKILL.md` body from disk and injects it via `render_skill_envelope` — the model reads the body and follows the instructions. PDCA loops are **model-coordinated**: the `SKILL.md` body describes convergence criteria; the model self-iterates using the `lisp_eval` tool (sandboxed Lisp interpreter, `hkask_lisp::eval_sandboxed_with_budget`) for deterministic checks and the `render_template` tool (minijinja, rendering `kask/registry/templates/`) for structured prompt scaffolding. Template base path is wired via `agent::set_template_base_path()` (OnceLock) in `crates/zed/src/main.rs`.
> 3. **Divergence #2 — Curator agent:** add the Curator (VSM S4) as a native in-process zed-kask agent (singleton; `CuratorHandle` mpsc authority never crosses a process boundary), selectable in the Agent Panel. ACP is optional (only for external-agent interop).
> 4. **Divergence #3 — hKask tool processing:** compile hKask's keep-crates into zed-kask; host the 10 on-disk MCP servers (§2.4) **in-process** (new transport alongside `context_server`'s `StdioTransport`) — *as implemented (D3), the servers run as child processes over stdio, not a new in-process transport*; emit `reg.*` spans directly.
> 5. **Thread → memory:** zed-kask threads parsed into per-user / Curator episodic + semantic memory (extends the existing ACP per-turn encoding).
> 6. **Remove everything redundant from hKask:** inference router, daemon, ACP seam, MCP-stdio, REPL, chat service, Matrix (all of it), communication MCP, backward-compat shims. **Nothing is removed from zed-kask** — it tracks upstream.
> 7. **Magnac Carta P1–P4, P12 non-negotiable.** Provider-side safety and refusal fallbacks are the active defense.

---

## 2. The Essentialist Split (what zed-kask owns vs what hKask keeps)

### 2.1 zed-kask owns (generic — inherited from upstream, NOT modified except integration seams)

Inference routing (`crates/language_model`, `language_model_core`, `language_models`, `language_models_cloud`), provider keystore (`crates/credentials_provider`, `zed_credentials_provider`), chat/Agent Panel (`crates/agent`, `agent_ui`), editor/GitHub/comms/voip/CRDT (`crates/workspace`, `project`, etc.), thread storage (`crates/agent/src/thread_store.rs`), MCP stdio hosting (`crates/context_server`). These stay upstream-identical; we only _add seams_ (in-process transport) where hKask plugs in.[^ousterhout]

### 2.2 hKask keeps (unique: curator + sovereignty + tools) — compiled into zed-kask

**Status (2026-08-20):** workspace builds clean. **16 kask crates** under `kask/crates/` (15 `hkask-*` + `kask_bridge`) plus 10 MCP server crates under `kask/mcp-servers/` and 1 zed-side crate (`crates/kask_extensions_ui/`). 10 MCP servers on disk (curator may be unloaded via `kask.mcp.overrides`).

| Crate                                                                                     | Why irreducible                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `hkask-types`                                                                             | Foundation: IDs, `InferencePort` trait, `RegulationSpan`, vocab. `VoiceDesign`, `ExpectProposal`, and `HMemEntry` live here (consolidated from removed crates).                                                                                                                                           |
| `hkask-storage`                                                                           | **Sovereignty:** per-user/curator data directory encrypted private sphere (P11.1). Dual-backend: SQLite (SQLCipher, default) or PostgreSQL (pgvector, for scale-up).                                                                                                                                                       |
| `hkask-memory`                                                                            | Unique semantic/episodic memory + consolidation.                                                                                                                                                                                                                                                                           |
| `hkask-regulation`                                                                        | Cybernetic nervous system (`reg.*`, variety, algedonic, set-points). Per-agent governed tool calls are bounded by `CallCapManager` (1 call charged per `McpRuntime::invoke`, resets per tick). Pruned from 49 files/15,408 lines to 26 files/9,004 lines — orphaned modules removed.                      |
| `hkask-tool-port`                                                                        | **`ToolPort` dispatch seam.** Not an enforcement point: it holds no tokens, no authorization check (RR-0056), and no taint labels (RR-0053).                                                                                                                                                                                                                                                                       |
| `hkask-keystore`                                                                          | **Sovereignty crypto only:** DB passphrase, internal-secret derivation w/ versioning. Uses the `keyring` crate directly for all keychain access (no `SecretsPort` trait).                                                                                                                        |
| `hkask-ledger`                                                                            | hMem accounting.                                                                                                                                                                                                                                                                                                           |
| `hkask-inference`                                                                         | **Kept (revised):** MCP servers use it directly (`MediaRouter`, `InferenceIpcClient`, `ProviderId`). Reads API keys from the `keyring` crate directly. Embeddings are handled by `kask_bridge::LanguageModelEmbeddingPort` (resolves credentials from `INFERENCE_PROVIDERS` + env var, no `LanguageModelRegistry` lookup). |
| `hkask-mcp-server` (framework)                                                            | MCP server framework with `reg.tool.*` span emission. Servers run standalone with identity from `ServerContext.webid` (resolved from `HKASK_WEBID`, falling back to anonymous).        |
| `hkask-forecast`, `hkask-bridge-ontology`, `hkask-email`, `hkask-lisp` | Domain logic used by keep-crates/MCP servers.                                                                                                                                                                                                                                                                              |
| `hkask-mcp`                                                                               | MCP dispatch + metering. Per-agent `CallCap` charged at `McpRuntime::invoke` as a runaway-loop breaker (fail-open on an unseeded agent — RR-0057).                                                                                                                                                                |
| `hkask-services-core`                                                                     | Shared foundation: `ServiceError`, `ServiceConfig`, `HkaskSettings`. Consumed by 6 crates.                                                                                                                                                          |
| 10 MCP servers (on-disk set)                                                              | **The tools** — child processes over stdio (D3), governed by the in-process `McpRuntime`.                                                                                                                                                                                                                                                                             |

### 2.3 MCP load set (10 on disk)

The original 16 MCP servers have been pruned to **10 on disk**: `companies`, `corpus`, `curator`, `kata-kanban`, `portfolio`, `prediction-markets`, `research`, `scenarios`, `swarm`, `training`. The `BUILT_IN_MCP_SERVERS_IDS` constant in `kask/crates/kask_bridge/src/mcp_servers.rs` enumerates them.[^anthropic-mcp]

| On disk (10)                                                                                                          |
| --------------------------------------------------------------------------------------------------------------------- |
| `companies`, `corpus`, `curator`, `kata-kanban`, `portfolio`, `prediction-markets`, `research`, `scenarios`, `swarm`, `training` |

> The `curator` MCP server is kept on disk but may be unloaded by default (Curator is a native agent, D2). All 10 build clean.

---

## 3. The Minimal Divergence Map (exact zed-kask touch points)

Every hKask integration maps to a **named, isolated** change in zed-kask. This is the entire divergence surface (D1–D32; D4 and D10 removed); everything else tracks upstream. The authoritative record is [`DIVERGENCE.md`](../../../DIVERGENCE.md) at the repo root — this quick table summarizes the seams; see §13.4 for the grouped summary and `DIVERGENCE.md` for the full per-seam detail.[^fowler-strangler]

| #       | Divergence                                                 | zed-kask crate / file                                                                                                                                                               | Status     | Change                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1      | Skill execution                                            | `crates/agent/src/tools/skill_tool.rs` + `crates/agent/src/agent.rs` + `crates/zed/src/main.rs`                                                                                     | ✅ DONE    | `SkillTool::run` reads the `SKILL.md` body from disk and injects it via `render_skill_envelope` (upstream-Zed body injection). The model reads the body and follows the instructions. PDCA loops are model-coordinated; the model self-iterates using the `lisp_eval` tool (sandboxed Lisp) for deterministic checks and the `render_template` tool (minijinja, `kask/registry/templates/`) for structured prompt scaffolding. Template base path wired via `agent::set_template_base_path()` (OnceLock) in `crates/zed/src/main.rs`.                                                                                                                                                                                                                                                                                                                       |
| D2      | Curator agent                                              | `crates/agent_ui/src/agent_ui.rs` + `crates/agent/src/agent.rs`                                                                                                                     | ✅ DONE    | `Agent::Curator` variant; `CURATOR_AGENT_ID`; selectable in Agent Panel.                                                                                                                                                                                                                                                                                                                                                                                                                    |
| D3      | hKask tools in-process                                     | `crates/zed/src/main.rs` (McpRuntime passed directly as `ToolPort`)                                                                                                                 | ✅ DONE    | `McpRuntime` implements `ToolPort` directly (call metering + `reg.tool.*` spans; no per-call authorization — RR-0056). See `DIVERGENCE.md` D3 for the authoritative state. MCP servers run as child processes. Servers run standalone with identity from `ServerContext.webid` (resolved from `HKASK_WEBID`, falling back to anonymous).                                         |
| D4      | Guard layer (removed)                                      | —                                                                                                                                                                                  | ❌ REMOVED | Provider-side safety and refusal fallbacks remain.                                                                                                                              |
| D5      | Sovereignty keys                                           | `kask/crates/hkask-keystore`                                                                                                                                                        | ✅ DONE    | `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase chain, SQLCipher encryption). No `keyring`-injection seam, no `OnceLock`, no parallel zed `CredentialsProvider` path. No key material backs tool authority.                                                                      |
| D6      | Thread → memory                                            | `crates/agent/src/thread.rs` / `thread_store.rs` + `kask/crates/hkask-types` + `kask/crates/kask_bridge`                                                                            | ✅ DONE    | `MemoryPort` trait in `hkask-types`. `BridgeMemoryPort` in `kask_bridge`. Global hook `agent::set_memory_port()` (Mutex — re-settable). Thread turn completion ingests via `cx.background_spawn()`. Hook is `None` at startup; `RealMemoryPort` wired in the deferred post-login task.                                                                                                                              |
| D7      | App-identity                                               | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`                                                | ✅ DONE    | `APP_NAME`→`Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`, bundle IDs `dev.zed-kask.*`.                                                                                                                                                                                                                                                                                                                                                                    |
| D8      | Bridge + adapters                                          | `kask/crates/kask_bridge/`                                                                                                                                                          | ✅ DONE    | `LanguageModelInferencePort` (`InferencePort` over `LanguageModel`, honors `model_override` via `resolve_model_names` registry resolution), `BridgeMemoryPort`, `BridgeThreadCondenser`, `BridgeContextInjector`, `BridgeCuratorContextInjector`, `BridgeMetacognitionProvider`, `KaskSettings`. `McpRuntime` is passed directly as `ToolPort`. Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
| D9      | Settings + credentials                                     | `kask/crates/kask_bridge/src/settings.rs` + `crates/settings_content/src/settings_content.rs` + `crates/settings_ui/src/pages/kask_page.rs` + `crates/settings_ui/src/page_data.rs` | ✅ DONE    | `KaskSettings` struct + `"kask"` section in settings.json; `hkask-keystore` uses the `keyring` crate directly (kask namespace). Settings UI page with sub-pages (Data Services, MCP Servers, Curator, Memory, Condenser, Models) registered in `page_data.rs` after `ai_page`.                                                                                                                  |
| D10     | Kask panel (removed)                                       | —                                                                                                                                                                                  | ❌ REMOVED | Visualization views moved to inline chat-stream widgets (D18). `ToolInvoker` trait + `set_tool_invoker` hook moved to `crates/hkask-tool-invoker/src/hkask_tool_invoker.rs`.                                                                                                                                                                                                        |
| D11–D20 | (See §13.4 consolidated map)                               | —                                                                                                                                                                                   | ✅ DONE    | The §3 quick table predates the consolidated map; D11–D32 are enumerated authoritatively in §13.4 below and in repo-root `DIVERGENCE.md`.                                                                                                                                                                                                                                                                                                                                                   |
| D24     | Edit predictions via `LanguageModelRegistry`              | `crates/zed/src/main.rs` + `kask/crates/kask_bridge/`                                                                                                                              | ✅ DONE    | Edit predictions routed through zed's `LanguageModelRegistry` (glm-5.2). See `DIVERGENCE.md` D24.                                                                                                                                                                                                                                                                                                                                                                                            |
| D25     | Chat Completions `finish_reason: "length"` → `MaxTokens`  | `crates/language_models/src/provider/`                                                                                                                                            | ✅ DONE    | Maps OpenAI Chat Completions `finish_reason: "length"` to `MaxTokens` so the stop reason is surfaced correctly. See `DIVERGENCE.md` D25.                                                                                                                                                                                                                                                                                                                                                   |
| D26     | Tool-use warnings via static-context injection            | `crates/agent/src/agent.rs`                                                                                                                                                        | ✅ DONE    | Tool-use warnings injected via `Thread::static_context` (the same channel as the Curator overlay). See `DIVERGENCE.md` D26.                                                                                                                                                                                                                                                                                                                                                                 |
| D27     | Sandboxed terminal non-interactive shell                   | `crates/terminal/`                                                                                                                                                                 | ✅ DONE    | Sandboxed terminal uses a non-interactive shell. See `DIVERGENCE.md` D27.                                                                                                                                                                                                                                                                                                                                                                                                                     |
| D28     | Standardized Artifact Storage                             | `kask/crates/hkask-types/src/agent_paths.rs` + `kask/crates/kask_bridge/`                                                                                                          | ✅ DONE    | All persistent artifacts (memory DBs, curator DBs, MCP server DBs, skills registry, archived threads) resolve under a single data root via `hkask_types::agent_paths`. See `DIVERGENCE.md` D28 and [`standardized-artifact-storage.md`](standardized-artifact-storage.md).                                                                                                                                                  |

**Discipline:** D1–D32 (D4 and D10 removed) are the _only_ edits to zed-kask's tree outside `kask/`. Any hKask behavior that would require touching other Zed crates is a smell — push the logic into an hKask crate behind one of these seams instead.

---

## 4. (removed)

> The phased migration-plan subsection that previously occupied §4–§5 has been removed. All phases are complete: D1–D32 are wired (see §3 divergence map). The `DIVERGENCE.md` at the repo root is the authoritative record of the divergence surface.[^fowler-strangler]

## 5. (removed)

---

## 6. Migration Status

> The phased migration plan that previously occupied this section has been removed. All phases are complete: D1–D32 are wired (see §3 divergence map). The `DIVERGENCE.md` at the repo root is the authoritative record of the divergence surface.[^fowler-strangler]

---

## 7. App-Identity Separation (zed-kask ↔ zed coexistence)

**Principle (deep-module):** separate the **local filesystem footprint** so `zed-kask` and an upstream `zed` install coexist on the same machine without conflict, while **sharing the Zed account** — the user logs into their existing Zed account and uses zed-kask _as Zed_, with the minimal kask enhancements. Two deep modules own the footprint; a few hardcoded, non-derived points need separate renames (bug-hunt findings).[^ousterhout]

### 7.1 The two deep modules (single knobs)

| Module                              | Knob                                               | Today                                                    | zed-kask                                                       | What it renames                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------------------------- | -------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/paths/src/paths.rs`         | `APP_NAME: &str` (+ derived `APP_NAME_LOWERCASE`)  | `"Zed"`                                                  | `"Zed-Kask"` / `"zed-kask"`                                    | config/data/state/temp/logs dirs on all OSes; `Zed-Kask.log`; db/extensions/themes/snippets/prompts/settings/keymap/AGENTS.md; macOS `~/Library/Application Support/Zed-Kask` + `~/Library/Logs/Zed-Kask` + `~/.local/state/Zed-Kask`; Linux `$XDG_*_HOME/zed-kask`; Windows `%APPDATA%\Zed-Kask` + `%LOCALAPPDATA%\Zed-Kask`. **The file itself comments: "Forks should change this to avoid colliding with Zed's user data."** |
| `crates/release_channel/src/lib.rs` | `app_identifier()` / `app_id()` / `display_name()` | `"Zed-Editor-Stable"` / `"dev.zed.Zed-Stable"` / `"Zed"` | `"Zed-Kask-Editor"` / `"dev.zed-kask.Zed-Kask"` / `"Zed-Kask"` | Windows single-instance mutex `{id}-Instance-Mutex` + named pipe `\\.\pipe\{id}-Named-Pipe`; macOS bundle id (`~/Library/Preferences/dev.zed-kask.Zed-Kask.plist`, LaunchServices identity); Dock/menu display name.                                                                                                                                                                                                             |

**Deletion test:** inlining `APP_NAME`/`app_identifier` at every call site would reappear the platform-path logic everywhere → the modules earn their keep; change the constants, the whole footprint renames. ≤3 public items each, every consumer reads them, nothing writes back → **deep**.

### 7.2 Non-derived collision points (bug-hunt — APP_NAME alone does NOT fix these)

| #   | Point                              | File                                                                                                                       | Risk                                                                                                                                                                                                                                                           | Fix                                                                                                                                                                                                                                                                  |
| --- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | **macOS single-instance TCP port** | `crates/zed/src/zed/mac_only_instance.rs` `address()`                                                                      | Port = `43737 + (channel×100) + uid` — keyed on **release channel + uid only**, NOT on APP_NAME. zed-kask and zed-stable (same channel, same uid) → **same port → the second app sees the "Zed Editor Stable Instance Running" handshake and silently exits.** | Distinct port block (fixed offset, e.g. `+500`, or a `Kask` release-channel arm) + change `instance_handshake()` to "Zed-Kask …".                                                                                                                                    |
| C2  | **Remote SSH/WSL server dirs**     | `crates/paths/src/paths.rs` `remote_server_dir_relative()`/`remote_wsl_server_dir_relative()` + `crates/util/src/shell.rs` | Hardcoded `.zed_server` / `.zed_wsl_server` on the REMOTE host. SSH to a host where zed also runs → collision + version mismatch.                                                                                                                              | `.zed-kask_server` / `.zed-kask_wsl_server` (2 path fns + shell.rs).                                                                                                                                                                                                 |
| C3  | **Binary name**                    | `crates/zed/Cargo.toml` `[[bin]] name = "zed"`                                                                             | Same `zed` binary on PATH → shadows/conflicts.                                                                                                                                                                                                                 | `[[bin]] name = "zed-kask"` (keep package name `zed` to minimize diff).                                                                                                                                                                                              |
| C4  | **macOS bundle display names**     | `crates/zed/Cargo.toml` L281–305 (`"Zed Dev"`…`"Zed"`)                                                                     | Indistinguishable from zed in Dock/Launchpad.                                                                                                                                                                                                                  | `"Zed-Kask …"` (via `display_name()`).                                                                                                                                                                                                                               |
| C5  | **URL scheme `zed://`**            | `crates/zed/src/zed/open_listener.rs` + `assets/settings/default.json` `$schema` + `zed://skill` share links               | Internal `zed://` prefixes are just strings (safe); the OS-level handler is bundle-id-registered (macOS: only one app owns `zed://`).                                                                                                                          | **Decision:** keep `zed://` internally (minimal divergence — don't touch open_listener) and accept the macOS handler conflict, OR rename to `zed-kask://` (full isolation, but diverges `default.json` `$schema` + skill-share links). Lean: keep `zed://`; revisit. |

### 7.3 RENAME vs KEEP (the account-sharing constraint)

| RENAME (local footprint — isolated)                     | KEEP (shared — user logs into their Zed account)                                |
| ------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `APP_NAME`, `app_identifier`, `app_id`, `display_name`  | `default.json` `"server_url": "https://zed.dev"` (collab)                       |
| config/data/state/cache/logs/db/extensions dirs         | `"provider": "zed.dev"`, `"zed.dev": {}` (LLM provider/account)                 |
| `Zed-Kask.log`, settings/keymap/AGENTS.md paths         | `cloud_api_client` `cloud.zed.dev` (account API)                                |
| Windows mutex/pipe, macOS bundle id + plist             | `release_channel::ZED_DOCS_URL` `https://zed.dev/docs` (docs)                   |
| macOS single-instance port + handshake                  | `staging-collab.zed.dev` / `collab.zed.dev` (collab relay)                      |
| `.zed-kask_server` / `.zed-kask_wsl_server` remote dirs | telemetry endpoint (zed.dev) — optional disable                                 |
| binary `zed-kask`                                       | extension marketplace URL (shared; extensions re-installed in the isolated dir) |

**Key invariant:** account/auth/collab traffic goes to `*.zed.dev` keyed on the user's Zed credentials, NOT on bundle id or APP_NAME. Renaming the local identity does **not** affect login — the user signs into the same Zed account and zed-kask behaves as Zed with a separate local footprint.

### 7.4 Verified facts (what breaks?)

- **Does renaming the bundle id break Zed account login?** No — auth is to `cloud.zed.dev` keyed on credentials, not bundle id. (Verified: account endpoints live in `default.json`/`cloud_api_client`, independent of `app_id`.)
- **Does renaming APP_NAME orphan existing Zed settings?** It _isolates_ them — zed-kask starts fresh (re-onboard); the user's zed settings stay untouched in the old `zed` dirs. Intended.
- **C1 is the silent killer:** an APP_NAME rename does NOT prevent the macOS single-instance collision — verified `address()` keys on channel+uid. Must fix C1 explicitly or zed-kask silently exits whenever zed is running.
- **Extensions:** isolated dir = re-install. Minor cost; benefit = no version conflicts with zed's extensions.
- **Telemetry:** distinct install id (renamed data_dir) → zed-kask reports under a different install id to the same endpoint. Acceptable, or disable.

### 7.5 Implementation (complete)

All app-identity tasks (T-A1 through T-A8) are complete (D7 ✅ DONE): `APP_NAME`→`Zed-Kask`, `app_identifier`→`Zed-Kask-Editor`, `app_id`→`dev.zed-kask.Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server`/`.zed-kask_wsl_server`, bundle IDs `dev.zed-kask.*`, URL scheme `zed-kask://`. See `DIVERGENCE.md` D7 for the authoritative list.

---

## 8. Architecture Notes

> The planning-process artifacts (open questions, review findings) that previously occupied this section have been removed. All review findings were resolved during implementation. The architecture is described in §0–§7, §11–§14. The `DIVERGENCE.md` at the repo root is the authoritative divergence surface record.[^fowler-strangler]

---

## 9. (removed)

## 10. (removed)

---

## 11. Kask Settings & Credentials (data-service keys, minimal divergence)

> **Design note:** The D9b design below proposed routing sovereignty keys (D5) through zed's `CredentialsProvider`. The final D5 implementation does NOT do this — `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase, SQLCipher encryption), with no zed-side seam. The `CredentialsProvider` namespace (D9b) is used only for data-service API keys (companies/scenarios), not sovereignty keys. See `DIVERGENCE.md` D5 for the authoritative final state. This section is retained as the design history for D9a/D9b; treat the D5 sovereignty-key references below as superseded by `DIVERGENCE.md` D5.

**Goal:** load API keys for data services (EODHD, FMP, and other kask data services) and all kask-unique config via a **kask settings section** in zed-kask's settings.json + a **kask credentials namespace** in the keystore — leaving core zed settings/keystore code untouched.[^fowler-di]

### 11.1 Evidence

- zed-kask stores provider API keys via the `CredentialsProvider` trait (`read_credentials`/`write_credentials`/`delete_credentials` keyed by URL → OS keychain); `language_models` providers use `api_key_state` + `credentials_provider` (`crates/credentials_provider`, `crates/language_models/src/provider/open_router.rs`). **Secrets live in the keychain, NOT settings.json.**
- The settings UI is `Vec<SettingsPage>` built in `crates/settings_ui/src/page_data.rs::settings_data()`; pages live in `crates/settings_ui/src/pages/` (e.g. `mcp_servers_page.rs`, `llm_providers_page.rs`).
- hKask today reads data-service keys from **env vars** (`HKASK_FMP_API_KEY`, `HKASK_EODHD_API_KEY`) — in `hkask-mcp-companies` (`ctx.get`). They are NOT in hKask's keychain (which holds the DB passphrase chain only).

### 11.2 Design (two additive seams)

**D9a — kask settings section** (`"kask": {...}` in settings.json + a settings struct). A new top-level section, isolated from core zed settings. Holds kask-unique, **non-secret** config:

- `kask.data_services.{eodhd,fmp,polygon,alpha_vantage,tiingo,fred,...}` — enabled toggles + per-service config (endpoints, tiers). The **secret API key is NOT here** — it is in the keychain (D9b); settings holds only the reference/toggle.
- `kask.mcp.load_default` + `overrides` — the default-loaded set (§2.4; 10 on disk total) + per-server toggles (curator may be unloaded via override; filesystem/communication absent).
- `kask.curator` — always-on toggle, regulation set-points (variety window, algedonic thresholds).
- `kask.sovereignty.pod` — data-dir override, consent defaults.
- `kask.guard` — direct-chat guard strategy (R3: buffer / incremental / cascade-only). **Removed** — `cascade_only` is hardcoded; direct chat uses provider-side safety + refusal fallback.
- `kask.memory` — consolidation cadence, confidence floor.
  Registered with zed's settings system so it appears in the `zed://schemas/settings` schema. **Minimal divergence:** one new settings struct + registration; core zed settings structs untouched.

**D9b — kask credentials namespace** (via the existing `CredentialsProvider`). Data-service API keys stored in the OS keychain under kask-namespaced URLs (e.g. `kask://credentials/eodhd`, `kask://credentials/fmp`), alongside zed's provider keys (which use their own URLs). The kask MCP servers (companies/scenarios) read keys via `CredentialsProvider` at runtime — **replacing the env-var approach** (`HKASK_*`). This folds into the T3.0 in-process refactor: MCP servers take a credentials handle, not env vars. The sovereignty keys (D5: DB passphrase) also move here (kask namespace), so the trimmed `hkask-keystore` becomes a thin crypto-derivation layer over the shared `CredentialsProvider` (using the `keyring` crate directly).

### 11.3 Settings UI (additive page)

A new **Kask** page: `crates/settings_ui/src/pages/kask_page.rs` + one entry in `page_data.rs::settings_data()`. Sub-pages mirror the settings section: **Data Services** (per-service enable + key entry → writes to keychain via `CredentialsProvider`), **MCP Servers** (the 10 on-disk servers, with load toggles — curator may be unloaded via override), **Curator**, **Sovereignty/Pod**, **Regulation**, **Memory**. Touches `page_data.rs` minimally (one `SettingsPage` push) — core zed pages untouched.

### 11.4 Configuration translation / migration

Existing hKask config → kask settings + keychain, on first launch (and a `kask import-config` command):

- env `HKASK_FMP_API_KEY` / `HKASK_EODHD_API_KEY` → `CredentialsProvider` entries `kask://credentials/{fmp,eodhd}` + `kask.data_services.{fmp,eodhd}.enabled = true`.
- hKask keychain sovereignty keys (DB passphrase) → `keyring` crate directly (D5 — NOT `CredentialsProvider`).
- hKask config-file settings (regulation thresholds, consolidation cadence, gas defaults) → `kask.*` settings.json section.
  Precedence: explicit settings.json > imported keychain > env-var fallback (during transition).

### 11.5 Implementation (complete)

D9a/D9b/D9c are ✅ DONE. `KaskSettings` struct registered with zed's settings system; `"kask"` section in settings.json. Credentials in keychain under `kask://credentials/<key>` namespace (via `CredentialsProvider`). Settings UI page with sub-pages. See `DIVERGENCE.md` D9 and `reference/kask-settings.md` for the authoritative state.

### 11.6 Design notes

- **Secrets must NOT be in settings.json** — keys live in the keychain (matches zed's provider-key pattern). The `kask` settings section holds only toggles/refs.
- **Dependency direction:** `hkask-keystore` uses the `keyring` crate directly for all keychain access (D5) — NOT zed's `CredentialsProvider`.
- **D9 = divergence seam** (kask settings section + credentials namespace + UI page). See §3 divergence map.

---

## 12. Kask Panel (removed)

**Status (2026-08-04):** The `crates/kask_panel/` crate was **deleted**. The chat panel (tab strip over `ConversationView` per MCP server) was redundant with the agent panel + curator threads. The standalone visualization views (`KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView`) were replaced by inline chat-stream widgets (`hkask-kanban-widget`, `hkask-portfolio-widget`, `hkask-scenarios-widget`) registered with `hkask-viz-core` (D18). The `ToolInvoker` trait + `set_tool_invoker` hook moved to `crates/hkask-tool-invoker/src/hkask_tool_invoker.rs` (the only remaining consumer). The call-cap persona was renamed from `kask-panel` to `swarm-panel`.

---

## 13. Composition & Connection Surfaces (zoom-out review)

The connection surfaces use established patterns (ports-and-adapters, decorator, composition-root DI, zed `Panel`, zed settings/credentials) — correct. This section names them as **one coherent, minimal composition** so the seams are explicit.[^cockburn-hexagonal][^fowler-di]

### 13.1 Governing invariant (dependency direction)

**hKask crates NEVER depend on zed-kask; zed-kask depends on hKask crates.** The **single bidirectional seam** is the zed-kask-side **bridge crate** (`crates/kask_bridge` = D8), which depends on both hKask port traits and zed-kask types and implements every adapter. Every other divergence (D1, D2, D3, D6, D9a, D10) _consumes_ a port implemented by the bridge; no hKask crate reaches into zed-kask internals. (Reconciles R9/D9b.)

### 13.2 The complete port set (ports-and-adapters)

All zed↔kask connection surfaces are a small set of **port traits** (in `hkask-types` + `hkask-tool-port`), each implemented by the **bridge crate** over a zed-kask facility:

| Port (hKask side)                                   | Implemented over (zed-kask side)                                                                                                                                                        | Used by                                                                | D      |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | ------ |
| `InferencePort` (hkask-types; non-streaming)        | `LanguageModel` (streaming) via collect→`InferenceResult`                                                                                                                              | Skill execution (D1), Curator (D2)                                   | D8     |
| `ToolPort` (hkask-tool-port; metering + spans, no authorization) | the in-process MCP tool registry (D3)                                                                                                                                                   | skill body-injection tool calls (D1), kask panel direct invoke (D10)     | D3/D8  |
| `keyring` crate (synchronous OS keychain)           | `CredentialsProvider` (kask namespace)                                                                                                                                                  | data-service keys (companies/scenarios) + sovereignty keys (D5)        | D5/D9b |
| `CuratorTurnPort` (hkask-types; NEW)                | zed native-agent turn → in-process `CuratorAgent` (tokio via bridge)                                                                                                                    | Curator as native agent (D2)                                           | D2/D8  |
| `MemoryPort` (hkask-types; NEW)                     | in-process `EpisodicMemory`/`SemanticMemory` handles. `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced: `None` at startup, real after agent provisioning. | thread→memory ingestion (D6)                                           | D6     |

Hexagonal pattern: hKask defines the ports; the bridge crate is the adapter; the composition root wires them. **No hKask crate imports a zed-kask crate.**

### 13.3 Composition root (startup — DI pattern)

zed-kask app startup constructs the individual hKask components directly (`KaskCore` was never implemented as a single singleton — the composition root wires each component separately) and wires the adapters:[^seemann-di]

1. **Load `KaskSettings` (D9a)** → bind to component construction params (regulation set-points, gas defaults, consolidation cadence, MCP load set = the 10 on disk, §2.4). **Settings→config is construction-time, not a runtime port** (config-struct-validated-on-construction).
2. **Memory port hook is `None` at startup (D6):** `set_memory_port` is not called until the deferred post-login task. `thread.rs` no-ops when the hook is unset. Uses `Mutex` (not `OnceLock`) so the port can be replaced later.
3. **Construct hKask components directly:** per-user/curator data directory storage (SQLite SQLCipher or PostgreSQL via `ServiceConfig::open_driver()`), Regulation runtime, memory, the singleton Curator (`CuratorHandle` mpsc in-process), the 10 MCP servers (standalone, identity from `ServerContext.webid` resolved out of `HKASK_WEBID`).
4. **Build the bridge:** `InferencePort`-over-`LanguageModel` (+guard), `ToolPort`-over-tool-registry, `keyring` crate-over-`CredentialsProvider`, `CuratorTurnPort`, `MemoryPort`; inject into Curator/MCP servers/kask panel. Skill execution (D1) uses upstream-Zed body injection (`SkillTool::run` → `render_skill_envelope`).
5. **Wire the regulation system:** construct `RegulationLedger::default()` + `CyberneticsLoop::new(ledger)` + `NoopEventSink`, seed the `swarm-panel` persona `CallCap`, and call `McpRuntime::with_governance(loop, sink)`. Startup log: "hKask regulation system wired — tool invocations are governed". `hkask-regulation` and `tokio` are now dependencies of `zed`.
6. **Spawn** the regulation + Curator metacognition tokio loops on the `gpui_tokio` runtime (R1) — the loop driver.
7. **Register** the **user agent** + **Curator** native agents (D2).
8. **Deferred agent provisioning (D6 late):** after `AppState::set_global`, a spawned task watches `UserStore::current_user()`. When the Zed user resolves: `provision_agent(username)` creates the directory structure, ensures a DB passphrase (auto-generate random English word if none, via the `keyring` crate directly), and calls `set_memory_port(BridgeMemoryPort(RealMemoryPort))` to replace the `None` port. MCP servers are launched without a per-user `HKASK_WEBID`; they fall back to anonymous identity unless the operator sets `HKASK_WEBID` in the environment.
9. **Migrate** config (T6.3) on first launch.

`KaskCore` (the "shared core" R4 referred to — the single owner of storage/regulation/memory the MCP servers take handles from, to prevent the two-instance pitfall) was never implemented. The composition root constructs individual components directly.

Components construct at zed-kask startup with the memory port hook set to `None`; the agent is provisioned when `UserStore::current_user()` resolves (deferred task). The `MEMORY_PORT` global uses `Mutex` (not `OnceLock`) so the port can be replaced after startup. Per-user/curator data directory storage opens at provisioning time, not at process start.

### 13.4 Consolidated divergence map (D1–D32)

The authoritative divergence surface is [`DIVERGENCE.md`](../../../DIVERGENCE.md) at the repo root — every D-seam, the exact files it touches, what's wired, and the tests that pin it. That file is the source of truth for upstream-sync conflict resolution; this section summarizes the groups so the composition-root wiring below is readable without a context switch.

| Group | D-seams | Summary |
| --- | --- | --- |
| Core integration | D1–D10 | Skill execution (D1), Curator agent (D2), in-process MCP tools (D3), the **removed** guard layer (D4), keychain access (D5), thread→memory (D6), app-identity (D7), the bridge (D8), settings/credentials (D9), and the **removed** Kask panel (D10). |
| Targeted upstream fixes | D11–D20 | Carried until upstream lands them: `time` deprecation allow (D11), OpenAI-compatible env var name (D12), OpenRouter output budget (D13), streaming-reveal timer (D14), bounded cursor-blink timers (D15), app-menu rename + safe terminal-based zed-kask updater (D16, which superseded the **removed** D17 GitHub feed and D19 progress popup), viz-widget block rendering (D18), per-call USD cost in `TokenUsage` (D20). |
| Kask-extension seams | D21–D23 | Widget→agent compose-back injector (D21), block-reachability pins in `main.rs` (D22), `AgentPanelSiblingHost` visibility + worktree spawn wiring (D23). |
| Upstream model/UX/storage seams | D24–D32 | Edit predictions via `LanguageModelRegistry` (D24), Chat Completions `finish_reason: "length"` → `MaxTokens` (D25), tool-use warnings via static-context injection (D26), sandboxed-terminal non-interactive shell (D27), standardized artifact storage (D28), and the additional seams D29–D32 enumerated in `DIVERGENCE.md`. |

Two D-seams are removed: **D4** (provider-side safety and refusal fallbacks remain) and **D10** (visualization views moved to inline chat-stream widgets under D18). Both retain their D-numbers as historical anchors; see `DIVERGENCE.md` for the authoritative state.

---

## 14. Repository Consolidation — full merge into zed-kask

**Decision (§0):** fully merge hKask into the `zed-kask` fork. zed-kask becomes the **single source of truth** for everything hKask is becoming — code, skills, scripts, and docs. The `mdz-axo/hKask` repo is **archived** (read-only reference). This replaces the earlier path-dep/submodule wiring, which dissolved once hKask could no longer compile or run standalone (daemon/ACP/REPL/inference deleted; keep-crates need the in-process bridge + `gpui_tokio`).[^fowler-strangler]

### 14.1 Why (essentialist)

- **hKask crates are not independently shippable** after the deletions — they only compile inside zed-kask. A separate repo for non-standalone crates is friction (cross-repo path-deps, R10 hermeticity, two-clone dev, ownership ambiguity) with no value. P5: a module/repo that can't stand alone shouldn't be kept apart.[^ousterhout]
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
    │                  # hkask-tool-port,
    │                  # hkask-keystore, hkask-ledger, hkask-mcp,
    │                  # hkask-mcp-server, hkask-inference,
    │                  # hkask-forecast, hkask-bridge-ontology, hkask-services-core,
    │                  # hkask-email, hkask-lisp,
    │                  # kask_bridge (D8)
    ├── mcp-servers/   # the 10 on-disk servers (curator may be unloaded via override; hkask-mcp-*)
    ├── skills/        # the skills registry (60 SKILL.md files in .agents/skills/;
    │                  # 62 template crates under kask/registry/templates/)
    ├── scripts/       # check-hkask-no-zed-deps.sh + hKask admin/build scripts
    └── docs/          # ← documentation home (see 14.3)
```

zed-kask's `Cargo.toml` adds `kask/crates/*` + `kask/mcp-servers/*` as workspace members and merges hKask's `[workspace.dependencies]` into its own. The bridge crate `kask_bridge` (D8) lives under `kask/crates/` too — it's ours, not upstream's.

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
- **CI script:** `kask/kask/scripts/check-hkask-no-zed-deps.sh` enforces the dependency invariant (denylist-name check is the real gate).
- **Upstream sync:** conflicts only in the D-seam files + `[workspace.members]`/`[workspace.dependencies]`. `kask/` is additive → never conflicts. `DIVERGENCE.md`: "everything under `kask/` is ours; everything else is upstream."

### 14.6 Migration notes

The repository consolidation is complete. `DIVERGENCE.md` lives at the repo root (the authoritative divergence surface record). The `kask/` namespace isolates hKask from upstream. The `kask/scripts/check-hkask-no-zed-deps.sh` CI script enforces the dependency invariant.

---

## References

[^fowler-strangler]:
    Fowler, M. (2004). _StranglerFigApplication_. https://martinfowler.com/bliki/StranglerFigApplication.html
    Cited for the incremental-migration pattern underlying the minimal-divergence fork strategy, the named divergence surface, and the repository consolidation.

[^conway]:
    Conway, M. E. (1968). How do committees invent? _Datamation_, 14(4), 28–31. https://www.melconway.com/research/committees.html
    Cited for Conway's Law — the fork's three divergence areas mirror the organizational boundary between the Zed and hKask development surfaces.

[^ousterhout]:
    Ousterhout, J. (2021). _A philosophy of software design_ (2nd ed.). Yaknymer Press. https://web.stanford.edu/~ouster/cgi-bin/book.php
    Cited for the deep-module principle (high benefit/cost ratio, minimal interface) applied to the essentialist split, app-identity separation, and repository consolidation rationale.

[^anthropic-mcp]:
    Anthropic, PBC. (2024). _Model context protocol specification_. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol governing the 10 on-disk MCP servers in the load set.

[^fowler-di]:
    Fowler, M. (2004). _Inversion of control containers and the dependency injection pattern_. https://martinfowler.com/articles/injection.html
    Cited for the dependency injection pattern applied to the kask settings/configuration seam and the composition surfaces.

[^miller-capability]:
    Miller, M. S., Yee, K.-P., & Shapiro, J. (2003). _Capability myths demolished_. Systems Research Lab, Johns Hopkins University. https://srl.cs.jhu.edu/pubs/SRL2003-02.pdf
    Cited for the capability-based security model framing the authority-boundary design.

[^cockburn-hexagonal]:
    Cockburn, A. (2005). _Hexagonal architecture_. https://alistair.cockburn.us/hexagonal-architecture/
    Cited for the ports-and-adapters pattern that structures the complete port set between zed-kask and hKask.

[^seemann-di]:
    Seemann, M., & van Deursen, S. (2019). _Dependency injection principles, practices, and patterns_. Manning Publications. https://www.manning.com/books/dependency-injection-principles-practices-patterns
    Cited for the composition-root DI pattern — the startup sequence that constructs and wires all hKask components.
