# DIVERGENCE.md — zed-kask fork vs upstream zed/zed

This file is the **single source of truth** for what `zed-kask` changes relative to upstream `zed/zed`, and for what lives where now that hKask is **fully merged into this repo**. Use it on every `upstream` sync. Detail: `kask/docs/architecture/zed-host-architecture-plan.md`.

## Repository consolidation (full merge)
hKask is **fully merged into zed-kask** under a `kask/` namespace (`kask/crates/hkask-*`, `kask/mcp-servers/hkask-mcp-*`, `kask/skills/`, `kask/scripts/`, `kask/docs/`). The `mdz-axo/hKask` repo is **archived** (read-only reference). zed-kask is the single source of truth — one clone, one build, one CI. **Everything under `kask/` is OURS (additive); upstream never touches it, so it never conflicts on sync.** Everything outside `kask/` is upstream zed except the D-seam edits below.

## Governing invariant
**hKask crates (under `kask/crates/hkask-*`) NEVER depend on zed crates (under `crates/`); zed-kask depends on hKask.** The **sole bidirectional seam** is the bridge crate `kask/crates/kask_bridge` (D8), which depends on both hKask port traits and zed types and implements every adapter. Enforced by `kask/scripts/check-hkask-no-zed-deps.sh` in CI.

## Divergence map (D1–D10) — the ONLY edits to zed-kask's tree OUTSIDE `kask/`
Everything else outside `kask/` is byte-identical to upstream and re-merged without conflict.

| D | Surface | zed-kask file (outside `kask/`) | Change | Connection (port) |
|---|---|---|---|---|
| D1 | Skill execution | `crates/agent_skills` + `crates/agent/src/tools/skill_tool.rs` | Replace `render_skill_envelope()` body-injection with the compiled-in `ManifestExecutor` cascade (WordAct/FlowDef/KnowAct/RenderAct + PDCA + gas/rjoule + OCAP). `SKILL.md` frontmatter stays discovery-only. | skill_tool → kask_bridge.ManifestExecutor(InferencePort, ToolPort) |
| D2 | Curator agent | `crates/agent/src/agent.rs` + `native_agent_server.rs` + `crates/agent_servers` | Register the Curator as a native in-process agent (singleton; `CuratorHandle` mpsc in-process). ACP optional. | native agent → `CuratorTurnPort` → in-process Curator |
| D3 | Tools in-process | `crates/context_server/src/client.rs` + `transport/` | Add an in-process transport alongside `StdioTransport`; host the 12 default-load hKask MCP tools in-process. ⚠ MCP servers are refactored off `DaemonClient` to direct `KaskCore` handles. | in-process transport → `ToolPort` |
| D4 | Guard layer | `crates/language_model_core`/`language_model` | `GuardedInferencePort` wraps an `InferencePort`-over-`LanguageModel` adapter (collect stream→`InferenceResult`). | `InferencePort` adapter |
| D5 | Sovereignty keys | `crates/credentials_provider` / `zed_credentials_provider` | hKask sovereignty keys (OCAP signing, DB passphrase, internal secrets) via the `SecretsPort` adapter over `CredentialsProvider` (kask namespace). | `SecretsPort` |
| D6 | Thread→memory | `crates/agent/src/thread.rs` / `thread_store.rs` | Hook thread completion → hKask memory ingestion (episodic + semantic) via `MemoryPort`. | `MemoryPort` |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`/`uninstall.sh`/`bundle-linux` | Rename the local footprint (`APP_NAME`→`Zed-Kask`, `app_identifier`/`app_id`/`display_name`, single-instance port, remote-server dirs `.zed-kask_server`, binary `zed-kask`). **Keep** shared `*.zed.dev` account/collab endpoints. | — |
| D8 | Bridge + adapters | `kask/crates/kask_bridge` + `gpui_tokio` | **THE bidirectional seam** (under `kask/`). Bridges GPUI↔tokio; implements all ports: `InferencePort`-over-`LanguageModel`, `ToolPort`-over-tool-registry, `SecretsPort`-over-`CredentialsProvider`, `CuratorTurnPort`, `MemoryPort`. | all ports |
| D9 | Settings + credentials | new `KaskSettings` section (`"kask": {...}` in settings.json) + `CredentialsProvider` kask namespace + `crates/settings_ui/src/pages/kask_page.rs` | kask-unique non-secret config (data-service toggles, MCP load set, curator, sovereignty, guard, memory) + data-service API keys in the keychain. | `KaskSettings` → `KaskCore` params; `SecretsPort` |
| D10 | Kask panel | `kask/crates/kask_panel` (`impl Panel`) | Native GPUI panel: catalog of the 12 MCP servers + per-server view (direct `:tool args` + scoped inference), reusing zed `ui` components + theme tokens. | panel → `ToolPort` + `InferencePort` |

> D8, D10 (and the hKask keep-crates, skills, scripts, docs) live **under `kask/`** (additive — not an upstream-merge surface). D1–D7, D9 are edits in zed's tree (the real merge surfaces) + the `[workspace.members]`/`[workspace.dependencies]` arrays in the root `Cargo.toml`.

## hKask keep-crates (now under `kask/crates/`)
`hkask-types`, `hkask-storage`, `hkask-memory`, `hkask-regulation`, `hkask-templates` (ManifestExecutor), `hkask-pods` (Curator+UserPod), `hkask-guard`, `hkask-capability` (OCAP/ToolPort), `hkask-identity` (WebID), `hkask-keystore` (trimmed: sovereignty crypto), `hkask-wallet`, `hkask-ledger`, `hkask-mcp-server` (framework), + the 15 MCP server crates under `kask/mcp-servers/` (12 loaded by default).

## MCP default load set (12)
Loaded: `memory`, `condenser`, `research`, `companies`, `media`, `docproc`, `training`, `replica`, `kata-kanban`, `codegraph`, `scenarios`, `regulation`.
Kept, NOT loaded by default: `curator` (Curator is a native agent; `regulation` MCP covers span queries), `skill` (skill execution is native via D1; management → `kask` CLI).
Deleted: `communication` (Matrix/TTS → zed-kask voip), `filesystem` (zed provides fs tools).

## Port set (in `kask/crates/hkask-types` + `hkask-capability`; implemented by `kask/crates/kask_bridge`)
`InferencePort` (non-streaming) • `ToolPort` (OCAP+gas) • `SecretsPort` (NEW) • `CuratorTurnPort` (NEW) • `MemoryPort` (NEW). Hexagonal: hKask defines ports; the bridge is the adapter; the composition root wires them.

## Composition root (startup)
zed-kask app startup constructs **one `KaskCore`** (per-pod SQLCipher storage + Regulation + memory + singleton Curator + 12 MCP servers + `ManifestExecutor`), wires the bridge adapters, spawns the regulation/Curator tokio loops on `gpui_tokio`, registers the UserPod + Curator agents + `KaskPanel`, loads `KaskSettings` → `KaskCore` params, runs config migration. `KaskCore` is the single owner of the handles the MCP servers take.

## Documentation home (`kask/docs/`)
All Kask docs live in `kask/docs/`: `architecture/` (the plan, four-pattern arch, principles, ADRs), `specs/` (D1–D10 seam specs, port/adapter contracts, MCP load set), `plans/` (migration, upstream-sync runbook). This `DIVERGENCE.md` stays at the repo root (the fork's headline doc) and points into `kask/docs/`.

## Upstream-sync procedure
1. `git fetch upstream && git merge upstream/main`.
2. Resolve conflicts ONLY in the D1–D9 files listed above (in zed's tree) + the root `Cargo.toml` `[workspace.members]`/`[workspace.dependencies]` arrays. `kask/` is additive → never conflicts; if a conflict appears under `kask/`, investigate before resolving.
3. `cargo build` (zed-kask) + `cargo test` (kask integration tests) after each sync.
4. Update this file if a divergence moves or a new Dn is added.
5. Re-run `kask/scripts/check-hkask-no-zed-deps.sh` — must pass (the §13.1 invariant).

## Reference
Full design + reasoning + tasks + open questions: `kask/docs/architecture/zed-host-architecture-plan.md` (§3 divergence map, §2.4 MCP load set, §11 settings/credentials, §12 kask panel + §12.6 GPUI reuse map, §13 composition & connection surfaces, §14 repository consolidation).

## Dependency policy — conform to zed's versions
**hKask conforms to zed's dependency versions where there are package conflicts.** Do not bump zed's workspace deps to accommodate hKask — refactor hKask to use zed's stack instead.

### The libsqlite3-sys conflict (resolved by conforming)
- zed pins `libsqlite3-sys = "0.30.1"` (via `sqlez` AND `sqlx-sqlite` → `sea-orm` → `collab`). Both use `links = "sqlite3"`.
- hKask's `rusqlite 0.39` requires `libsqlite3-sys ^0.37.0` — incompatible. No `rusqlite` version uses `^0.30` (the range that includes 0.30.1).
- **Resolution:** hKask's `hkask-storage` will be **rewritten to use zed's `sqlez`** (or `sqlx`) instead of `rusqlite`. This is a deferred task (T0.6-storage). Until then, `hkask-storage` is NOT a workspace member, and `hkask-regulation`'s storage-backed modules are feature-gated behind `#[cfg(feature = "storage")]`.
- SQLCipher: `sqlez` uses plain SQLite (no SQLCipher). hKask's per-pod encryption (P11.1) will be implemented as an **application-layer encryption** (encrypt before store, decrypt after read) rather than SQLCipher at the SQLite level. This preserves the sovereign private sphere without requiring SQLCipher in zed's libsqlite3-sys build.
