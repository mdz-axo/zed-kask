---
title: "zed-kask — Minimal-Divergence Fork Architecture & Migration Plan"
audience: [architects, integrators]
last_updated: 2026-09-23
version: "0.43.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust, lifecycle]
---

# zed-kask — Minimal-Divergence Fork Architecture & Migration Plan

> **One-line frame:** `zed-kask` is a fork of Zed that tracks `upstream` and carries a named, test-pinned divergence surface. Zed-side files are not assumed byte-identical: `DIVERGENCE.md` records the current numbered seams and their retired gaps, while hKask libraries live under `kask/`, the editor owns the process-global Regulation/runtime graph, and 12 MCP servers run as governed child processes. The former Kask panel is deleted; the Agent panel, cross-domain Steer panels, and inline widgets are the live surfaces.

## Table of contents

- [§0 — Fork Location & Upstream-Sync Strategy](#0-fork-location--upstream-sync-strategy-load-bearing)
- [§1 — The Enhanced Prompt (minimal-divergence fork)](#1-the-enhanced-prompt-minimal-divergence-fork)
- [§2 — The Essentialist Split](#2-the-essentialist-split-what-zed-kask-owns-vs-what-hkask-keeps)
  - [§2.1 — zed-kask owns (generic)](#21-zed-kask-owns-generic--inherited-from-upstream-not-modified-except-integration-seams)
  - [§2.2 — hKask keeps (unique: curator + sovereignty + tools)](#22-hkask-keeps-unique-curator--sovereignty--tools--compiled-into-zed-kask)
  - [§2.3 — MCP load set (12 on disk)](#23-mcp-load-set-12-on-disk)
- [§3 — The Minimal Divergence Map (current numbered D-seams)](#3-the-minimal-divergence-map-exact-zed-kask-touch-points)
- [§4 — (removed)](#4-removed)
- [§5 — (removed)](#5-removed)
- [§6 — Migration Status](#6-migration-status)
- [§7 — App-Identity Separation](#7-app-identity-separation-zed-kask--zed-coexistence)
- [§8 — Architecture Notes](#8-architecture-notes)
- [§9 — (removed)](#9-removed)
- [§10 — (removed)](#10-removed)
- [§11 — Kask Settings & Credentials](#11-kask-settings--credentials-data-service-keys-minimal-divergence)
- [§12 — Kask Panel (removed)](#12-kask-panel-removed)
- [§13 — Composition & Connection Surfaces](#13-composition--connection-surfaces-zoom-out-review)
  - [§13.1 — Governing invariant (dependency direction)](#131-governing-invariant-dependency-direction)
  - [§13.2 — The complete port set](#132-the-complete-port-set-ports-and-adapters)
  - [§13.3 — Composition root (startup — DI pattern)](#133-composition-root-startup--di-pattern)
  - [§13.4 — Consolidated divergence map (current numbered D-seams)](#134-consolidated-divergence-map-current-numbered-d-seams)
- [§14 — Repository Consolidation](#14-repository-consolidation--full-merge-into-zed-kask)
- [References](#references)

---

> **Current state (2026-09-23):** `kask/crates/` contains 19 libraries (18 `hkask-*` plus `kask_bridge`) and `kask/mcp-servers/` contains 12 server packages. The editor-side integration also includes Swarm, Kanban, Portfolio, and Media Steer panels plus inline viz widgets. [`DIVERGENCE.md`](../../../DIVERGENCE.md) is the authoritative Zed-side seam table; retired numbers are not active seams.

---

## 0. Fork Location & Upstream-Sync Strategy (load-bearing)

- **Fork:** `Clones/zed-kask` — `origin` = `github.com/mdz-axo/zed-kask.git`, `upstream` = `github.com/zed/zed.git`, on `main`, currently **in sync** with upstream.
- **Divergence policy:** keep `main` a near-clone of `upstream/main`. All hKask integration is isolated to a **small, named set of crates/files** (§3) so `git fetch upstream && git merge upstream/main` stays low-conflict. No scattered edits across Zed's tree.
- **hKask wiring (FULL MERGE — §14):** hKask's keep-crates, MCP servers, template registry, scripts, and docs live **inside the zed-kask repo** under `kask/` (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/registry/`, `kask/scripts/`, `kask/docs/`); executable skill specifications live at the repo root under `.agents/skills/`. The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. (Replaces the prior path-dep/submodule approach, which dissolved once hKask could no longer run standalone.)
- **Sync cadence (ongoing, Phase 7):** rebase/merge `upstream/main` regularly; resolve conflicts only in the divergent crates; run the hKask integration tests after each sync. The whole point of the fork is to _inherit Zed's improvements for free_ — divergence is the cost, so minimize it.[^fowler-strangler]

---

## 1. The Enhanced Prompt (minimal-divergence fork)

> Fork Zed into **`zed-kask`** (`Clones/zed-kask`), tracking `upstream` and diverging only in three areas.[^conway] hKask is trimmed to the Curator + sovereignty + tools, compiled into zed-kask under the `kask/` namespace. No backward compatibility.
>
> 1. **zed-kask inherits the generic surface and infrastructure**: chat, editor UI, language-model routing, provider credentials, and thread storage. Some of those upstream files carry isolated D-seams; only files absent from `DIVERGENCE.md` are presumed unchanged.
> 2. **Divergence #1 — skill execution:** `SkillTool::run` resolves the current project skill catalog, then reads the selected body through the injected project-aware resolver (`crates/agent/src/tools/skill_tool.rs:184-288`; implementation at `crates/agent/src/agent.rs:4339-4383`; registration at `:1016-1021`). Project-local skills resolve through project buffers, including remote workspaces and unsaved edits; global skills resolve through the filesystem. Slash activation uses the same resolver (`agent.rs:2317-2345`). The model reads the resulting `render_skill_envelope` and coordinates any PDCA loop, using `lisp_eval` for deterministic checks and `render_template` for `kask/registry/templates/` scaffolding. The template base path is wired at `crates/zed/src/main.rs:700-711`.
> 3. **Divergence #2 — Curator agent:** the Curator (VSM S4) is a native in-process zed-kask agent (`Agent::Curator` variant, D2), selectable in the Agent Panel. Curation→Cybernetics directives flow through the `CuratorDirective` channel (`kask/crates/hkask-types/src/curator.rs:46`, wired in `crates/zed/src/main.rs:778,792`). ACP is optional (only for external-agent interop).
> 4. **Divergence #3 — hKask tool processing:** compile hKask's keep-crates into zed-kask; host the 11 on-disk MCP servers (§2.3) as **child processes over stdio** (D3), governed by the in-process `McpRuntime`. Child tool wrappers emit tracing target `reg.tool`; governed completions persist `SpanKind::ToolCompleted`, with `reg.mcp` reserved for persistence diagnostics (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-131`; `kask/crates/hkask-mcp/src/runtime.rs:1534-1543`).
> 5. **Thread → memory:** zed-kask threads parsed into per-user / Curator memory (D6).
> 6. **Remove everything redundant from hKask:** the former inference router, daemon, ACP seam, standalone MCP launcher, REPL, chat service, Matrix transport, communication MCP, and backward-compatibility shims. **Nothing is removed from zed-kask** — it tracks upstream.
> 7. **Magna Carta P1–P4, P12 non-negotiable.** Provider-side safety and refusal fallbacks are the active defense.

---

## 2. The Essentialist Split (what zed-kask owns vs what hKask keeps)

### 2.1 zed-kask owns (generic — inherited from upstream, NOT modified except integration seams)

Inference routing (`crates/language_model`, `language_model_core`, `language_models`, `language_models_cloud`), provider keystore (`crates/credentials_provider`, `zed_credentials_provider`), chat/Agent Panel (`crates/agent`, `agent_ui`), editor/GitHub/comms/voip/CRDT (`crates/workspace`, `project`, etc.), thread storage (`crates/agent/src/thread_store.rs`), MCP stdio hosting (`crates/context_server`). These stay upstream-identical; we only _add seams_ where hKask plugs in.[^ousterhout]

### 2.2 hKask keeps (unique: curator + sovereignty + tools) — compiled into zed-kask

**Status (2026-09-19):** **19 kask crates** under `kask/crates/` (18 `hkask-*` + `kask_bridge`) plus **12 MCP server crates** under `kask/mcp-servers/` and zed-side crates (`swarm_panel`, `kanban_panel`, `portfolio_panel`, `hkask-steer`, `hkask-viz-core`, `hkask-*-widget`, `hkask-tool-invoker`, `hkask-conversation-injector`, `marketplace_ui_common`). The `kask_extensions_ui` crate was removed 2026-08-20 (skill marketplace retired). 12 MCP servers on disk (curator may be unloaded via `kask.mcp.overrides`).

| Crate                                                                                     | Why irreducible                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `hkask-types`                                                                             | Foundation: IDs, `InferencePort` trait (single/stream/message/vision/embed/list/rerank plus child-local `media_generate`; no `generate_batch` method; `kask/crates/hkask-types/src/ports/inference_port.rs:161-373`), `MemoryPort`, Regulation records, Curator directives, vocab, `VoiceDesign`, and `ExpectProposal`. |
| `hkask-storage`                                                                           | **Sovereignty:** per-user/curator data directory encrypted private sphere (P11.1). SQLCipher-encrypted SQLite with sqlite-vec virtual tables for vectors (`kask/crates/hkask-storage/src/core/connection.rs:157`, `kask/crates/hkask-storage/src/core/sql/schema.sql:7`). Hosts the media `GalleryStore` (D35).                                                                                                                                                       |
| `hkask-memory`                                                                            | Unique memory + confidence-based consolidation.                                                                                                                                                                                                                                                                           |
| `hkask-regulation`                                                                        | Cybernetic nervous system (`reg.*`, variety, algedonic, set-points). Per-agent governed tool calls are bounded by `CallCapManager` (1 call charged per `McpRuntime::invoke`, resets per tick). 18 source files / 12,734 lines (measured 2026-09-09).                      |
| `hkask-tool-port`                                                                        | **`ToolPort` dispatch seam** (`kask/crates/hkask-tool-port/src/tool_port.rs:89`). Not an enforcement point: it holds no tokens, no authorization check (RR-0056), and no taint labels (RR-0053).                                                                                                                                                                                                                                                                       |
| `hkask-keystore`                                                                          | **Sovereignty crypto only:** DB passphrase, internal-secret derivation w/ versioning. Uses `oo7` (async Secret Service API) directly for all keychain access (no `SecretsPort` trait; `kask/crates/hkask-keystore/Cargo.toml:14`). `DEFAULT_PASSPHRASE` ("allostery") is the single source of truth for first-run provisioning.                                                        |
| `hkask-steer-core`                                                                            | The zed-free half of the Steer prompt surface: rendering and verification of the tool-advertisement contract against the server's build.rs-generated `TOOL_NAMES`. Split from `crates/hkask-steer` (2026-09-07) so the prompt-truth logic builds without the zed closure. (Replaces `hkask-ledger`, deleted 2026-09-08 with the local budget system.)                                                                                                                                                           |
| `hkask-event-store`                                                                       | Shared event store: harness-summary events feed the CyberneticsLoop's rollout-impact checks through `HarnessRegressionMonitor` (`kask_bridge/src/rollout_event_bridge.rs`). The monitor owns its scan cursor and advances only through summaries whose required checks were accepted; query failure or typed queue backpressure leaves the unaccepted suffix for retry. Assessment and verdict write-back remain later CyberneticsLoop phases. The training rollout bridge is a separate consumer of the same store. |
| `hkask-inference`                                                                         | **Kept (revised):** MCP children use its process-local `MediaRouter` for media and its `InferenceIpcClient` for chat, vision, embedding, model-list, and rerank calls into the editor (`kask/crates/hkask-inference/src/hkask_inference.rs:190-383`). It reads provider configuration from the child environment (`config.rs:109-129,218-228`). The removed D34 `generate_batch` bridge is not part of this surface. |
| `hkask-mcp-server` (framework)                                                            | MCP server framework with exact tracing target `reg.tool`; this child-stderr signal is observability, not a Regulation-ledger record (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:108-131`). Servers resolve `ServerContext.webid` from `HKASK_WEBID`, warning before anonymous fallback (`kask/crates/hkask-mcp-server/src/server/transport.rs:89-103`). |
| `hkask-forecast`, `hkask-bridge-ontology`, `hkask-email`, `hkask-lisp` | Domain logic used by keep-crates/MCP servers.                                                                                                                                                                                                                                                                              |
| `hkask-mcp`                                                                               | Child-process lifecycle, MCP dispatch, and metering. Per-agent `CallCap` is charged at `McpRuntime::invoke` as a runaway-loop breaker (fail-open on an unseeded agent — RR-0057); governed completion persists `SpanKind::ToolCompleted`, and persistence failures warn at `reg.mcp` (`kask/crates/hkask-mcp/src/runtime.rs:1499-1543`).                                                                                                                                                                |
| `hkask-condenser`                                                                         | In-process thread condensation via `kask_bridge::BridgeThreadCondenser`.                                                                                                                                                                                                                                                  |
| `hkask-services-core`                                                                     | Shared foundation: `ServiceError`, `ServiceConfig`, `HkaskSettings`.                                                                                                                                                          |
| 12 MCP servers (on-disk set)                                                              | **The tools** — child processes over stdio (D3), governed by the in-process `McpRuntime`.                                                                                                                                                                                                                                                                             |

### 2.3 MCP load set (12 on disk)

The original 16 MCP servers were pruned to 10, then the **media** server was recovered (D35, 2026-08-28) and the **spreadsheet** server was added (2026-09-18) — **12 on disk**: `companies`, `corpus`, `curator`, `kata-kanban`, `media`, `portfolio`, `prediction-markets`, `research`, `scenarios`, `spreadsheet`, `swarm`, `training`. The `BUILT_IN_MCP_SERVERS` constant in `kask/crates/kask_bridge/src/mcp_servers.rs:55-547` enumerates them (media entry at `mcp_servers.rs:485-526`); `builtin_mcp_server_ids()` (`mcp_servers.rs:549-553`) derives the ID list.[^anthropic-mcp]

| On disk (12)                                                                                                          |
| --------------------------------------------------------------------------------------------------------------------- |
| `companies`, `corpus`, `curator`, `kata-kanban`, `media`, `portfolio`, `prediction-markets`, `research`, `scenarios`, `spreadsheet`, `swarm`, `training` |

> The Curator MCP server is distinct from the native Curator agent and may be disabled by settings. The media server is pinned at exactly 93 registered tools by `tool_surface_is_exactly_93_registered_tools` (`kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs:478-489`); its OMC mapping is checked across the same registered set at `:515-529`.

---

## 3. The Minimal Divergence Map (exact zed-kask touch points)

`DIVERGENCE.md` is the canonical map. Seam numbers are identifiers, not a
continuous count of active changes. Consult the table for the latest row;
currently the earlier active groups are:

`D1–D3`, `D5–D9`, `D11–D16`, `D18`, `D20–D29`, `D31–D33`,
`D35–D37`, `D39–D48`, `D51–D52`, and later rows starting at D54 (see the table). D34 has no row in the
current active table; the former bridge batch API is not a live seam.

Retired numbers are never reused: D4 (guard layer), D10 (Kask panel), D17 and
D19 (retired audit seams), D30 (local skill marketplace), D38 (folded into
D37), D49 (folded into D42), D50 (folded into D46), and D53 (folded into
D54). This distinction is enforced by reading the current table and its retired
seams paragraph, not by carrying a duplicate per-seam table here.[^fowler-strangler]

Two shared-code mappings are especially easy to misstate:

- **D25** is implemented in the shared `ChatCompletionEventMapper` at
  `crates/language_model_core/src/chat_completion.rs:433-449`; its length pins
  are `length_with_pending_tool_calls_remains_truncated` (`:928-932`) and
  `stream_maps_length_finish_reason_to_max_tokens_stop` (`:985-989`). It is no
  longer an OpenAI/OpenRouter provider-local mapper.
- **D35** keeps media generation child-local. `LazyInferencePort::media_generate`
  calls the process-local `MediaRouter` (`kask/crates/hkask-inference/src/hkask_inference.rs:356-375`).
  The inference IPC bridge has no media method, and `InferencePort` has no
  `generate_batch` method.

## 4. (removed)

> Use the numbered seam register for the current implementation. See §3 and `DIVERGENCE.md`; retired or absent numbers are not active wiring.[^fowler-strangler]

## 5. (removed)

---

## 6. Migration Status

> Use the numbered seam register for the current implementation. See §3 and `DIVERGENCE.md`; retired or absent numbers are not active wiring.[^fowler-strangler]

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
| --- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | **macOS single-instance TCP port** | `crates/zed/src/zed/mac_only_instance.rs` `address()`                                                                      | Port = `43737 + (channel×100) + uid` — keyed on **release channel + uid only**, NOT on APP_NAME. zed-kask and zed-stable (same channel, same uid) → **same port → the second app sees the "Zed Editor Stable Instance Running" handshake and silently exits.** | Distinct port block (fixed offset, e.g. `+500`, or a `Kask` release-channel arm) + change `instance_handshake()` to "Zed-Kask …".                                                                                                                                    |
| C2  | **Remote SSH/WSL server dirs**     | `crates/paths/src/paths.rs` `remote_server_dir_relative()`/`remote_wsl_server_dir_relative()` + `crates/util/src/shell.rs` | Hardcoded `.zed_server` / `.zed_wsl_server` on the REMOTE host. SSH to a host where zed also runs → collision + version mismatch.                                                                                                                              | `.zed-kask_server` / `.zed-kask_wsl_server` (2 path fns + shell.rs).                                                                                                                                                                                              |
| C3  | **Binary name**                    | `crates/zed/Cargo.toml` `[[bin]] name = "zed"`                                                                             | Same `zed` binary on PATH → shadows/conflicts.                                                                                                                                                                                                                 | `[[bin]] name = "zed-kask"` (keep package name `zed` to minimize diff).                                                                                                                                                                                              |
| C4  | **macOS bundle display names**     | `crates/zed/Cargo.toml` L281–305 (`"Zed Dev"`…`"Zed"`)                                                                     | Indistinguishable from zed in Dock/Launchpad.                                                                                                                                                                                                                 | `"Zed-Kask …"` (via `display_name()`).                                                                                                                                                                                                                               |
| C5  | **URL scheme `zed://`**            | `crates/zed/src/zed/open_listener.rs` + `assets/settings/default.json` `$schema` + skill share links                       | Internal `zed://` prefixes are just strings (safe); the OS-level handler is bundle-id-registered (macOS: only one app owns `zed://`).                                                                                                                          | **Resolved:** renamed to `zed-kask://` — `open_listener.rs` parses it, `crates/cli/src/main.rs` adds it to `URL_PREFIX`, terminal hyperlinks regex updated (see `DIVERGENCE.md` D7 supporting-files list).                                                                                                                                 |

### 7.3 RENAME vs KEEP (the account-sharing constraint)

| RENAME (local footprint — isolated)                     | KEEP (shared — user logs into their Zed account)                                |
| ------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `APP_NAME`, `app_identifier`, `app_id`, `display_name`  | `default.json` `"server_url": "https://zed.dev"` (collab)                       |
| config/data/state/cache/logs/db/extensions dirs         | `"provider": "zed.dev"`, `"zed.dev": {}` (LLM provider/account)                 |
| `Zed-Kask.log`, settings/keymap/AGENTS.md paths         | `cloud_api_client` `cloud.zed.dev` (account API)                                |
| Windows mutex/pipe, macOS bundle id + plist             | `release_channel::ZED_DOCS_URL` `https://zed.dev/docs` (docs)                   |
| macOS single-instance port + handshake                  | `staging-collab.zed.dev` / `collab.zed.dev` (collab relay)                      |
| `.zed-kask_server` / `.zed-kask_wsl_server` remote dirs | telemetry endpoint (zed.dev) — optional disable                                 |
| binary `zed-kask`, URL scheme `zed-kask://`             | extension marketplace URL (shared; extensions re-installed in the isolated dir) |

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

## 11. Settings, inference environment, and keystore

The current system has two credential paths with different owners.[^fowler-di]

1. **Editor/provider credentials.** Zed's provider credential stores and Kask
   data-service credential URLs are read in the editor process. The composition
   root builds a filtered environment per `BuiltinMcpServer.credentials` and
   `config_env` (`kask/crates/kask_bridge/src/mcp_servers.rs:28-38,55-547`).
2. **Child inference configuration.** `hkask-inference` reads provider base URLs,
   API keys, and model settings from its process environment only; there is no
   child-side keychain fallback (`kask/crates/hkask-inference/src/config.rs:109-129,218-228`).
   This is why media generation stays child-local: the media child receives
   `OPENROUTER_API_KEY` and `DEEPINFRA_API_KEY`, while the editor process does
   not mirror those values into a Kask keyring
   (`kask/crates/kask_bridge/src/mcp_servers.rs:468-505`).
3. **Sovereignty key storage.** `hkask-keystore` uses `oo7` directly for its
   Secret Service entries (`kask/crates/hkask-keystore/Cargo.toml:12-16`;
   `kask/crates/hkask-keystore/src/keychain.rs:36-38,133-161`). It does not use
   the Rust `keyring` crate or route DB-passphrase access through Zed's
   `CredentialsProvider`.

There is one DB passphrase for the SQLCipher stores. The canonical MCP helper
checks injected credentials/environment and then the `hkask-keystore` entry;
missing data-service credentials remain authorization failures rather than silent
fallbacks. Credential writes that feed child env trigger the managed-server
restart path so running children receive the new values.

## 12. Kask Panel (removed)

Deleted — visualization views are inline chat-stream widgets under D18.

---

## 13. Composition & Connection Surfaces (zoom-out review)

The connection surfaces use established patterns (ports-and-adapters, decorator, composition-root DI, zed `Panel`, zed settings/credentials) — correct. This section names them as **one coherent, minimal composition** so the seams are explicit.[^cockburn-hexagonal][^fowler-di]

### 13.1 Governing invariant (dependency direction)

**hKask crates NEVER depend on zed-kask; zed-kask depends on hKask crates.** The **single bidirectional seam** is the zed-kask-side **bridge crate** (`kask/crates/kask_bridge` = D8), which depends on both hKask port traits and zed-kask types and implements every adapter. Every other divergence (D1, D2, D3, D6, D9) _consumes_ a port implemented by the bridge; no hKask crate reaches into zed-kask internals. (Reconciles R9/D9b.) Enforced by `kask/scripts/check-hkask-no-zed-deps.sh` (wired into CI).

### 13.2 Connection surfaces

| Surface | Implementation | Boundary |
|---|---|---|
| Inference chat/vision/embed/list/rerank | `LanguageModelInferencePort` plus inference IPC | MCP child ↔ editor process; Zed `LanguageModelRegistry` stays editor-side (`kask/crates/kask_bridge/src/inference_chat.rs:393-403,1065-1088`) |
| Media generation | `LazyInferencePort::media_generate` → process-local `MediaRouter` | Child-local; no media IPC (`kask/crates/hkask-inference/src/hkask_inference.rs:356-383`) |
| Provider batch QA | Corpus/provider batch implementation | Not an `InferencePort::generate_batch` method and not an IPC bridge surface |
| Tool dispatch | `McpRuntime` implements `ToolPort` | Editor-owned runtime → MCP children over stdio (`kask/crates/hkask-mcp/src/runtime.rs:576-680,1499-1543`) |
| Memory ingestion | Re-settable agent hook → `BridgeMemoryPort` | Editor thread → shared curator storage; chunk embeddings include `passage_text` (`kask/crates/kask_bridge/src/memory/ingest.rs:313-317,394-404`) |
| Sovereignty keys | `hkask-keystore` → `oo7::Keyring` | Direct Secret Service access (`kask/crates/hkask-keystore/src/keychain.rs:36-38,133-161`) |
| Data-service/provider credentials | Zed credential URLs → filtered child env | Per-server `credentials`/`config_env` allowlists (`kask/crates/kask_bridge/src/mcp_servers.rs:28-38,55-547`) |

`kask_bridge` is the only crate that depends on both Zed types and hKask port
types. MCP server crates remain Zed-free and communicate with the editor through
stdio and the inference socket.

### 13.3 Composition root and startup

The editor constructs one process-global regulatory graph and one managed MCP
runtime:[^seemann-di]

1. Load `KaskSettings` and set-points.
2. Construct the shared `RegulationLedger` and `CyberneticsLoop`, then pass the
   governed loop into one `McpRuntime` (`crates/zed/src/main.rs:772-896`).
3. Wire process-global recorders for agent-path MCP outcomes, skill outcomes,
   and operator feedback to the same ledger (`crates/zed/src/main.rs:907-1012`).
4. Start the Cybernetics and Metacognition loops on the GPUI-global Tokio
   runtime (`crates/zed/src/main.rs:1065-1122,1232-1247`).
5. Enter deferred provisioning without waiting for account resolution. If the
   current Zed user is available, derive the agent name from it; otherwise use
   `kask` and proceed immediately (`crates/zed/src/main.rs:1552-1571`).
6. Launch enabled entries from `BUILT_IN_MCP_SERVERS` as child binaries. Each
   child resolves its own `ServerContext.webid`; absent or invalid
   `HKASK_WEBID` is warned and becomes anonymous
   (`kask/crates/hkask-mcp-server/src/server/transport.rs:89-103`).
7. When the inference model resolves, wire `LanguageModelInferencePort`, the
   local resilience source, and the IPC socket; resynchronize managed children
   after the socket path changes (`crates/zed/src/main.rs:3169-3192`).

There is no `KaskCore` singleton and no Kask panel. “Process-global Regulation”
means the editor has one shared ledger/loop graph for all managed dispatch paths;
it does not mean MCP tools execute in-process.

### 13.4 Consolidated divergence map (current numbered D-seams)

Use `DIVERGENCE.md` directly for merge recovery. Its active rows and
retired-number paragraph are the complete classification; §3 records the current
sets without duplicating their implementation prose. In particular:

- D10 is retired because the Kask panel is deleted.
- D25 is a shared `ChatCompletionEventMapper` mapping.
- D34 is absent from the active table; no `generate_batch` bridge exists.
- D35 is the child-local media path and 93-tool media server (`kask/mcp-servers/hkask-mcp-media/src/hkask_mcp_media.rs:478-489`).
- D38, D49, D50, and D53 are folded into D37, D42, D46, and D54.
- The latest active numbered seam is the final row of `DIVERGENCE.md`'s table.

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
├── .agents/skills/    # 77 executable SKILL.md specifications
├── crates/            # upstream zed + isolated D-seam edits
├── extensions/        # upstream
└── kask/              # ── OURS (additive; upstream never touches here) ──
    ├── crates/        # 18 hkask-* libraries + kask_bridge (D8)
    ├── mcp-servers/   # the 11 on-disk servers (curator may be unloaded via override)
    ├── registry/      # 67 prompt-template directories under registry/templates/
    ├── scripts/       # check-hkask-no-zed-deps.sh + hKask admin/build scripts
    └── docs/          # ← documentation home (see 14.3)
```

zed-kask's `Cargo.toml` adds `kask/crates/*` + `kask/mcp-servers/*` as workspace members and merges hKask's `[workspace.dependencies]` into its own. The bridge crate `kask_bridge` (D8) lives under `kask/crates/` too — it's ours, not upstream's.

### 14.3 Documentation home (`kask/docs/`)

All Kask documentation lives **inside zed-kask** under `kask/docs/`. The 2026-08-28 condensation removed the former `explanation/` and `qa/` directories and folded completed plans into their successors; `plans/` remains only for active work such as the Logisheets capability plan. The documentation standard retains a below-70 target, while the current tree count is validated separately rather than asserted here:

- `kask/docs/architecture/` — this plan (`zed-host-architecture-plan.md`), the memory system ([`memory-system-specification.md`](memory-system-specification.md)), standardized artifact storage, the agent system prompt catalogue, principles, ADRs.
- `kask/docs/reference/` — reference documentation (MCP servers incl. the forecasting stack, settings, regulation spans, skills).
- `kask/docs/diataxis/` — per-crate Diataxis documentation set (incl. `diataxis/swarm_system/` for swarm content).
- `kask/docs/diagrams/` + `kask/docs/DIAGRAMS_INDEX.md` — diagram sources and index.
- `DIVERGENCE.md` stays at the zed-kask **repo root** (the fork's headline doc, referenced on every sync) and points into `kask/docs/` for detail.
  The `mdz-axo/hKask` repo is archived with a `README.md` pointing to `zed-kask/kask/`.

Completed plan and explanation content was repointed: swarm system docs → `kask/docs/diataxis/swarm_system/`, memory → `kask/docs/architecture/memory-system-specification.md`, forecasting → `kask/docs/reference/mcp-servers/README.md#the-forecasting-stack-three-layer-architecture`, and the implemented inference-Regulation loop → `kask/docs/diataxis/hkask-regulation/{explanation,reference}.md`. The remaining `kask/docs/plans/logisheets-spreadsheet-capability-plan.md` is active plan content.

### 14.4 Migration (complete)

The repository consolidation is complete: hKask keep-crates, MCP servers, skills registry, scripts, and docs are all under `kask/` in zed-kask. The `mdz-axo/hKask` repo is archived. `DIVERGENCE.md` lives at the repo root.

### 14.5 Connection surfaces (§13)

- **§13.1 invariant still holds:** hKask crates (under `kask/crates/hkask-*`) still must NOT depend on zed crates (under `crates/`); `kask_bridge` (under `kask/crates/`) is still the sole bidirectional seam. One repo, same rule.
- **CI script:** `kask/scripts/check-hkask-no-zed-deps.sh` enforces the dependency invariant (denylist-name check is the real gate).
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
    Ousterhout, J. (2021). _A philosophy of software design_ (2nd ed.). Yaknymer Press. https://web.stanford.edu/~ousterhout/cgi-bin/book.php
    Cited for the deep-module principle (high benefit/cost ratio, minimal interface) applied to the essentialist split, app-identity separation, and repository consolidation rationale.

[^anthropic-mcp]:
    Anthropic, PBC. (2024). _Model context protocol specification_. https://modelcontextprotocol.io/specification
    Cited for the MCP protocol governing the 11 on-disk MCP servers in the load set.

[^fowler-di]:
    Fowler, M. (2004). _Inversion of control containers and the dependency injection pattern_. https://martinfowler.com/articles/injection.html
    Cited for the dependency injection pattern applied to the kask settings/configuration seam and the composition surfaces.

[^cockburn-hexagonal]:
    Cockburn, A. (2005). _Hexagonal architecture_. https://alistaircockburn.us/hexagonal-architecture/
    Cited for the ports-and-adapters pattern that structures the complete port set between zed-kask and hKask.

[^seemann-di]:
    Seemann, M., & van Deursen, S. (2019). _Dependency injection principles, practices, and patterns_. Manning Publications. https://www.manning.com/books/dependency-injection-principles-practices-patterns
    Cited for the composition-root DI pattern — the startup sequence that constructs and wires all hKask components.
