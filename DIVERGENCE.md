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

## The divergence surface (D1–D12)

Every hKask integration maps to a named, isolated change in zed-kask. These
are the *only* edits to zed-kask's tree outside `kask/`. Any hKask behavior
that would require touching other Zed crates is a smell — push the logic into
an hKask crate behind one of these seams instead.

| D | Surface | zed-kask crate / file | What's wired |
|---|---|---|---|
| D1 | Skill execution | `crates/agent/src/tools/skill_tool.rs` + `crates/agent/src/agent.rs` + `crates/agent_skills/agent_skills.rs` + `crates/zed/src/main.rs` | `SkillTool` runs the hKask manifest cascade via `BridgeManifestExecutor` instead of injecting the `SKILL.md` body. `SKILL.md` is discovery-only. Catalog budget + description-length warnings disabled (skills execute via manifests, not prompt injection). Marketplace-installed skills (`SkillSource::Public`) loaded from `{global_skills_dir}/_marketplace/`. Global skills dir isolated to `paths::data_dir()/agents/skills/` (not shared `~/.agents/skills/`) — zed-kask skills are manifest-driven and not portable to upstream Zed. |
| D2 | Curator agent | `crates/agent/src/agent.rs` + `crates/agent_ui/src/agent_ui.rs` | `Agent::Curator` variant; `CURATOR_AGENT_ID`; selectable in Agent Panel. |
| D3 | hKask tools in-process | `crates/zed/src/main.rs` | `McpRuntime` implements `ToolPort` directly (capability-match gate + gas budgeting + `reg.tool.*` spans) and is passed wherever a `ToolPort` is needed — no bridge adapter. MCP servers run as child processes (stdio). Daemon transport deleted; identity from `ServerContext.webid` (`HKASK_WEBID`). Note: token signature verification against a trusted authority is NOT enforced (tokens are minted and consumed in-process); the enforced gate is the capability match in `McpRuntime::invoke`. |
| D4 | Guard layer | `kask/crates/hkask-guard` + `kask/crates/kask_bridge/src/inference.rs` | `GuardedInferencePort` wraps the `InferencePort` at the composition root. Mandatory content guard (injection/secret scanning) on the skill cascade path. Direct chat is unguarded (provider-side safety + refusal fallback) — there is no configurable strategy; the guard only wraps the cascade. Output scanning is post-hoc redaction of the stored stream, not real-time blocking. |
| D5 | Keychain access | `kask/crates/hkask-keystore` | `hkask-keystore` uses the `keyring` crate directly for all keychain access (DB passphrase chain, SQLCipher encryption). No zed-side seam, no keyring injection — this row exists only to document that the keystore does NOT route through zed's `CredentialsProvider`. The a2a/OCAP secret threading was removed (self-referential token verification — security theater). |
| D6 | Thread → memory | `crates/agent/src/thread.rs` + `crates/agent/src/agent.rs` + `kask/crates/hkask-types` + `kask/crates/kask_bridge/src/memory.rs` | `MemoryPort` trait in `hkask-types` (carries `agent_id` so the port routes by owning agent). `BridgeMemoryPort` + `RealMemoryPort` in `kask_bridge`. Global hook `agent::set_memory_port()` (Mutex — re-settable), wired once in the deferred task after the Zed user resolves; before that the hook is `None` and turn ingest no-ops. Thread turn completion ingests via `cx.background_spawn()`. `Thread` carries `agent_id: Option<AgentId>` set by `NativeAgent::new_session` when the agent is the Curator. `RealMemoryPort::ingest_turn` branches on `agent_id`: Curator turns go to the curator's sovereign `agents/curator/pod.db` as curator-perspective episodic (Private, `curator_webid`) + semantic (Shared) h_mems, mirroring the user agent's episodic loop. User turns go to the user's `memory.db` (episodic, Private, `user_webid`) + a curator-accessible semantic copy in `pod.db`. `open_curator_stores` opens both `curator_episodic` and `curator_semantic` from the same DB. |
| D7 | App-identity | `crates/paths/src/paths.rs`, `crates/release_channel/src/lib.rs`, `crates/zed/src/zed/mac_only_instance.rs`, `crates/zed/Cargo.toml`, `script/install.sh`, `script/uninstall.sh`, `script/bundle-linux` | `APP_NAME`→`Zed-Kask`, `app_identifier`→`Zed-Kask-Editor`, `app_id`→`dev.zed-kask.Zed-Kask`, `display_name`→`Zed-Kask`, port offset +500, binary `zed-kask`, remote dirs `.zed-kask_server` / `.zed-kask_wsl_server`, bundle IDs `dev.zed-kask.*`, URL scheme `zed-kask://`. |
| D8 | Bridge + adapters | `kask/crates/kask_bridge/` | **THE bidirectional seam** — implements all ports over zed-kask facilities. `InferencePort` over `LanguageModel`, `LanguageModelEmbeddingPort` (resolves credentials from `INFERENCE_PROVIDERS` + env var, no `LanguageModelRegistry` lookup), `BridgeManifestExecutor`, `BridgeMemoryPort`, `BridgeContextInjector`, `BridgeCuratorContextInjector` (curator-scoped recall via `RealMemoryPort::recall_context_curator`), `BridgeThreadCondenser`, `BridgeMetacognitionProvider`, `FusionLanguageModel`, `InferenceIpcServer`, `KaskSettings`. Channel pattern solves GPUI/tokio `Send`+`Sync` boundary. |
| D9 | Settings + credentials | `kask/crates/kask_bridge/src/settings.rs` + `crates/settings_content/src/settings_content.rs` + `crates/settings_ui/src/pages/kask_page.rs` + `crates/settings_ui/src/page_data.rs` | `KaskSettings` struct + `"kask"` section in settings.json. Credentials in keychain under `kask://credentials/<key>` namespace (via `CredentialsProvider`). Settings UI page with sub-pages for each kask subsystem. |
| D10 | Kask panel | `crates/kask_panel/` | Native GPUI center-pane `Item` (not a dock `Panel`). Tab strip (10 built-in MCP servers). Each tab hosts the agent panel's `ConversationView` with `Agent::Curator` — the `ConversationView` handles all rendering (messages, input, tool-call cards, scroll, retry, cancel, copy, markdown, streaming, mentions, drag-and-drop). The kask panel only adds the tab strip and tab-switch logic. Per-tab system prompt injected via `CuratorAgentServer::with_extra_static_context` (appended to `CURATOR_STATIC_CONTEXT`). `ToolInvoker` trait + `set_tool_invoker` hook remain for the per-server visualization views (kanban, portfolio, scenarios) which fetch data via direct MCP tool calls. `kask_panel::Toggle` / `ToggleFocus` actions. Deployed on demand via `kask_panel::init(cx)`. |
| D11 | `time::format_description::parse` deprecation allow | `crates/git_ui/src/git_graph.rs` | Upstream `git_graph.rs` calls `time::format_description::parse`, which is `#[deprecated]` in `time 0.3.54+` (the version the workspace resolves to; lower versions break `plist`/`project`). Two call sites (`timestamp_format`, the commit-date formatter) carry `#[allow(deprecated)]` with a `// zed-kask:` pointer to this seam. Remove this seam when upstream migrates to `parse_borrowed`. |
| D12 | OpenAI/Anthropic-compatible env var name | `crates/language_models/src/provider/api_compatible.rs` | Upstream computes the API-key env var name as `format!("{}_API_KEY", id).to_case(Case::UpperSnake)`, which splits `DeepInfra` → `DEEP_INFRA_API_KEY` and leaves `fal.ai`/`Together AI` as invalid env var names (`FAL.AI_API_KEY`, `TOGETHER_AI_API_KEY`). The entire kask ecosystem (`.env` template, MCP servers, keystore, UI text, docs) uses the concatenated alphanumeric form (`DEEPINFRA_API_KEY`, `FALAI_API_KEY`, `TOGETHERAI_API_KEY`), so the upstream computation never matches the env vars kask users set. The `// zed-kask:` block in `ApiCompatibleProviderState::new` strips non-alphanumerics and uppercases instead of using `convert_case`. `convert_case` was removed from `crates/language_models/Cargo.toml` (still used by other crates, so the workspace dep stays). Pinned by `test_api_key_env_var_name_kask_contract` in `api_compatible.rs`. |
| D13 | OpenRouter output budget | `crates/open_router/src/open_router.rs` + `crates/language_models/src/provider/open_router.rs` + `crates/open_router/Cargo.toml` | Upstream omits `max_tokens` from completion requests entirely (`Model::max_output_tokens()` hardcodes `None`). OpenRouter then reserves the model's full default output size (e.g. 64k for claude-haiku-4.5, 128k for glm-5.2) against the key's credit limit before dispatching, and rejects with 402 on keys whose remaining limit can't cover the reservation — even for a one-line prompt. Zed-kask parses `top_provider.max_completion_tokens` from the `/models` and `/models/user` responses into a new `Model::max_output_tokens` field, which flows into the request as an explicit `max_tokens` budget. Settings-defined `available_models[].max_output_tokens` (already in `OpenRouterAvailableModel` upstream, previously unwired) overrides the API-derived value. Pinned by `test_max_completion_tokens_from_api_becomes_request_budget`. |

## Other zed-kask-modified files (supporting D1–D13)

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
- `crates/collab/src/api/kask_skills.rs` + `crates/collab/src/db/queries/kask_skills.rs` — kask skill marketplace API (upload/download/vote/unpublish). Self-hosted automation: `Database::ensure_kask_skill_tables()` (idempotent boot-time schema self-heal, called from `setup_app_database` in `main.rs` — upstream applies schema out-of-band); blob-store misconfiguration warns at startup instead of silently disabling the marketplace; a `manifest.json` upload upserts the catalog row immediately (the 5-minute S3 poll remains as reconciliation for out-of-band writes).
- `crates/kask_extensions_ui/` — kask skill marketplace UI (browse/install/uninstall/vote/publish). Marketplace URL resolution in `kask_marketplace_url()` (`publish.rs`): `HKASK_MARKETPLACE_URL` env var override → client `server_url` (self-hosted default) → `http://localhost:3000` (dev fallback). Zed account credentials are attached to marketplace requests only when the resolved URL is same-host with the credential issuer (`credentials_allowed_for_url`) — a cross-host `HKASK_MARKETPLACE_URL` override sends the request unauthenticated rather than leaking the account token. Card/search/empty-state chrome shared via `crates/marketplace_ui_common/`.
- `crates/agent_skills/agent_skills.rs` — `global_skills_dir()` isolated to `paths::data_dir()/agents/skills/` (not shared `~/.agents/skills/`). One-time migration in `agent.rs::run_skills_scan` moves old skills to new location. `GLOBAL_SKILLS_DIR_DISPLAY` updated. `SkillSource::Global` and `load_marketplace_skills` docs updated.
- `crates/settings_ui/src/pages/skills_visibility.rs` + `crates/settings_ui/src/pages/skills_setup.rs` — skill visibility + marketplace toggle UI.
- `crates/zed/src/zed/open_listener.rs` — `zed-kask://` URL scheme parsing + tests.
   verify the bridge + foundation still compile.
- `crates/windows_resources/src/windows_resources.rs` — dev-channel window title `Kask` (D7 rebrand).
- `crates/gpui_tokio/src/gpui_tokio.rs` — `Tokio::handle_async` helper (get the tokio handle from `AsyncApp`); 3 call sites in `main.rs` deferred wiring.
- `crates/prompt_store/src/prompts.rs` + `crates/agent/src/agent.rs` + `crates/agent/src/thread.rs` — `RuleFrontmatter` (Cline-compatible `alwaysApply`/`globs` rules frontmatter) + `filter_conditional_rules` enforcement on the prompt-render path.
- `crates/agent/src/tools/terminal_tool.rs` — truncation spillover file (full output saved to temp file, path returned for `read_file`), head/tail overlap fix in `select_terminal_output_lines` (upstream candidate), relaxed shell-substitution doc wording (single-quoted `${...}` is safe).
- `crates/agent/src/tools.rs` — `deserialize_optional_u64_from_maybe_string` (models sometimes send `timeout_ms` as a string).
- `crates/agent_ui/src/agent_panel.rs` — eager `SkillIndex` population + `create_thread_with_options` returns the agent used (Curator support, D2).
- `crates/language_model/src/language_model.rs` + `crates/language_model/src/registry.rs` — `api_url()` accessor + `ModelFilterFn` (fusion economic guardrails filter panel candidates by price/intelligence).
- `crates/open_ai/src/list_models.rs` — generic `/v1/models` discovery for OpenAI-compatible providers (D12 support).
- `crates/zed/src/main.rs` (beyond the composition-root blocks) — `.env` loading via dotenvy before settings init, `--printenv` reflects the loaded `.env`, kask MCP server auto-launch, `sync_kask_mcp_servers` re-sync when the inference socket becomes available.
- `crates/client/src/zed_urls.rs` — shared-session URL uses `ZED_URL_SCHEME` (D7).


## hKask workspace members (additive — upstream never touches)

The following are added to the root `Cargo.toml` `[workspace.members]` array.
Upstream merges never conflict with these paths.

- `kask/crates/` — 19 hKask crates (`hkask-types`, `hkask-storage`, `hkask-memory`, `hkask-regulation`, `hkask-templates`, `hkask-guard`, `hkask-capability`, `hkask-keystore`, `hkask-ledger`, `hkask-mcp`, `hkask-mcp-server`, `hkask-inference`, `hkask-condenser`, `hkask-bridge-dublincore`, `hkask-services-core`, `hkask-email`, `hkask-forecast`, `hkask-lisp`, `hkask-goal`, `kask_bridge`). The `hkask-services-{context,corpus,compose,inference,kata-kanban,runtime}` crates were folded into their sole MCP server consumers.
- `kask/mcp-servers/` — 10 MCP server crates (`hkask-mcp-{codegraph,companies,condenser,corpus,curator,kata-kanban,media,research,scenarios,training}`)
- `crates/kask_panel` — zed-kask-side kask panel (D10)
- `crates/kask_extensions_ui` — zed-kask-side kask skill marketplace UI
- `crates/marketplace_ui_common` — shared catalog-page chrome (`MarketplaceCard`, search bar, empty state) extracted from the duplicated copies in `extensions_ui` (upstream, untouched) and `kask_extensions_ui`

## Governing invariant (§13.1)

**hKask crates NEVER depend on zed-kask crates; zed-kask depends on hKask.**
The sole bidirectional seam is `kask_bridge` (D8), which lives under
`kask/crates/` and depends on both sides. Enforced by
`kask/scripts/check-hkask-no-zed-deps.sh` (wired into CI).

## Upstream-sync runbook

1. `git fetch upstream && git merge upstream/main`
2. Conflicts will only appear in:
   - The D-seam files listed above (D1–D13)
   - `[workspace.members]` / `[workspace.dependencies]` in root `Cargo.toml`
3. Everything under `kask/` is additive → never conflicts.
4. After resolving, run `bash kask/scripts/check-hkask-no-zed-deps.sh` to
   verify the §13.1 invariant still holds.
5. Run `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` to