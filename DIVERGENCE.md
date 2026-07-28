# DIVERGENCE.md — zed-kask fork divergence from upstream Zed

> **Purpose:** the headline doc for upstream-sync conflict resolution. Every
> `git fetch upstream && git merge upstream/main` consults this file to know
> which crates/files are divergent and why. Referenced from
> `kask/docs/architecture/zed-host-architecture-plan.md` (§13.4).
>
> **Rule:** everything under `kask/` is ours (additive; upstream never touches
> here → near-zero merge conflict). Everything else tracks upstream; the only
> divergences are the D-seams listed below + the `[workspace.members]` /
> `[workspace.dependencies]` arrays in the root `Cargo.toml`.

## The divergence surface (D1–D10)

Every hKask integration maps to a named, isolated change in zed-kask. These
are the *only* edits to zed-kask's tree outside `kask/`. Any hKask behavior
that would require touching other Zed crates is a smell — push the logic into
an hKask crate behind one of these seams instead.

| D | Surface | zed-kask crate / file | What's wired |
|---|---|---|---|
| D1 | Skill execution | `crates/agent/src/tools/skill_tool.rs` + `crates/agent/src/agent.rs` + `crates/agent_skills/agent_skills.rs` + `crates/zed/src/main.rs` | `SkillTool` runs the hKask manifest cascade via `BridgeManifestExecutor` instead of injecting the `SKILL.md` body. `SKILL.md` is discovery-only. Catalog budget + description-length warnings disabled (skills execute via manifests, not prompt injection). Marketplace-installed skills (`SkillSource::Public`) loaded from `~/.agents/skills/_marketplace/`. |
| D2 | Curator agent | `crates/agent/src/agent.rs` + `crates/agent_ui/src/agent_ui.rs` | `Agent::Curator` variant; `CURATOR_AGENT_ID`; selectable in Agent Panel. |
| D3 | hKask tools in-process | `kask/crates/kask_bridge/src/tool_port.rs` + `crates/zed/src/main.rs` | `BridgeToolPort` wraps `McpRuntime` (implements `ToolPort` with OCAP/gas/spans). MCP servers run as child processes (stdio). Daemon transport deleted; identity from `ServerContext.webid` (`HKASK_WEBID`). |
| D4 | Guard layer | `kask/crates/hkask-guard` + `kask/crates/kask_bridge/src/inference.rs` | `GuardedInferencePort` wraps the `InferencePort` at the composition root. Mandatory content guard (injection/secret scanning) on the skill cascade path. Direct chat uses provider-side safety + refusal fallback (`cascade_only` default per `kask.guard.direct_chat_strategy`). |
| D5 | Sovereignty keys | `kask/crates/hkask-keystore` + `crates/credentials_provider` | `hkask-keystore` uses the `keyring` crate directly for all keychain access. Global `keyring` injection via `hkask_keystore::keyring` (OnceLock pattern). Composition root injects before `resolve_a2a_secret()`. |
| D6 | Thread → memory | `crates/agent/src/thread.rs` + `kask/crates/hkask-types` + `kask/crates/kask_bridge/src/memory.rs` | `MemoryPort` trait in `hkask-types`. `LoggingMemoryPort` + `BridgeMemoryPort` + `RealMemoryPort` in `kask_bridge`. Global hook `agent::set_memory_port()` (Mutex — re-settable). Thread turn completion ingests via `cx.background_spawn()`. |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`, `script/uninstall.sh`, `script/bundle-linux` | `APP_NAME`→`Zed-Kask`, `app_identifier`→`Zed-Kask-Editor`, `app_id`→`dev.zed-kask.Zed-Kask`, `display_name`→`Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server` / `.zed-kask_wsl_server`, bundle IDs `dev.zed-kask.*`, URL scheme `zed-kask://`. |
| D8 | Bridge + adapters | `kask/crates/kask_bridge/` | **THE bidirectional seam** — implements all ports over zed-kask facilities. `InferencePort` over `LanguageModel`, `BridgeToolPort` over `McpRuntime`, `BridgeManifestExecutor`, `BridgeMemoryPort`, `BridgeContextInjector`, `BridgeThreadCondenser`, `BridgeMetacognitionProvider`, `FusionLanguageModel`, `InferenceIpcServer`, `KaskSettings`. Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
| D9 | Settings + credentials | `kask/crates/kask_bridge/src/settings.rs` + `crates/settings_content/src/settings_content.rs` + `crates/settings_ui/src/pages/kask_page.rs` + `crates/settings_ui/src/page_data.rs` | `KaskSettings` struct + `"kask"` section in settings.json. Credentials in keychain under `kask://credentials/<key>` namespace (via `CredentialsProvider`). Settings UI page with sub-pages for each kask subsystem. |
| D10 | Kask panel | `crates/kask_panel/` | Native GPUI center-pane `Item` (not a dock `Panel`). Server selector (10 built-in MCP servers). Direct `:tool args` invocation (OCAP-gated) + scoped inference. `kask_panel::Toggle` / `ToggleFocus` actions. Deployed on demand via `kask_panel::init(cx)`. Per-server visualization views (portfolio, scenarios, kanban). |

## Other zed-kask-modified files (supporting D1–D10)

These files carry `// zed-kask:` comments but are supporting edits, not
primary divergence seams:

- `crates/client/src/client.rs` — `Client::credentials()` accessor for kask skill publish/vote pipelines.
- `crates/cli/src/main.rs` — `zed-kask://` added to `URL_PREFIX` array.
- `crates/zed_actions/src/lib.rs` — `OpenApplicationUrl` action doc references `zed-kask://`.
- `crates/gpui/src/app.rs` — `register_url_scheme` doc example uses `zed-kask`.
- `crates/terminal/src/alacritty/hyperlinks.rs` — `zed-kask://` added to URL regex (hardcoded — terminal doesn't depend on `paths` crate).
- `crates/agent/src/tool_router.rs` — `LazyToolRouter` filters MCP tools only; built-in tools bypass the router.
- `crates/agent/src/templates.rs` — Agent Skills system-prompt section diverges (manifest-driven, not body-injection).
- `crates/agent_ui/src/conversation_view/thread_view.rs` — `render_skill_loading_issues` only shows `LoadFailed` (description-length + catalog-budget issues disabled).
- `crates/collab/src/api/kask_skills.rs` + `crates/collab/src/db/queries/kask_skills.rs` — kask skill marketplace API (upload/download/vote/unpublish).
- `crates/kask_extensions_ui/` — kask skill marketplace UI (browse/install/uninstall/vote/publish).
- `crates/settings_ui/src/pages/skills_visibility.rs` + `crates/settings_ui/src/pages/skills_setup.rs` — skill visibility + marketplace toggle UI.
- `crates/zed/src/zed/open_listener.rs` — `zed-kask://` URL scheme parsing + tests.

## hKask workspace members (additive — upstream never touches)

The following are added to the root `Cargo.toml` `[workspace.members]` array.
Upstream merges never conflict with these paths.

- `kask/crates/` — 18 hKask crates (`hkask-types`, `hkask-storage`, `hkask-memory`, `hkask-regulation`, `hkask-templates`, `hkask-guard`, `hkask-capability`, `hkask-keystore`, `hkask-ledger`, `hkask-mcp`, `hkask-mcp-server`, `hkask-inference`, `hkask-condenser`, `hkask-bridge-dublincore`, `hkask-services-core`, `hkask-email`, `hkask-forecast`, `hkask-goal`, `kask_bridge`). The `hkask-services-{context,corpus,compose,inference,kata-kanban,runtime}` crates were folded into their sole MCP server consumers.
- `kask/mcp-servers/` — 10 MCP server crates (`hkask-mcp-{codegraph,companies,condenser,corpus,curator,kata-kanban,media,research,scenarios,training}`)
- `crates/kask_panel` — zed-kask-side kask panel (D10)
- `crates/kask_extensions_ui` — zed-kask-side kask skill marketplace UI

## Governing invariant (§13.1)

**hKask crates NEVER depend on zed-kask crates; zed-kask depends on hKask.**
The sole bidirectional seam is `kask_bridge` (D8), which lives under
`kask/crates/` and depends on both sides. Enforced by
`kask/scripts/check-hkask-no-zed-deps.sh` (wired into CI).

## Upstream-sync runbook

1. `git fetch upstream && git merge upstream/main`
2. Conflicts will only appear in:
   - The D-seam files listed above (D1–D10)
   - `[workspace.members]` / `[workspace.dependencies]` in root `Cargo.toml`
3. Everything under `kask/` is additive → never conflicts.
4. After resolving, run `bash kask/scripts/check-hkask-no-zed-deps.sh` to
   verify the §13.1 invariant still holds.
5. Run `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` to
   verify the bridge + foundation still compile.
