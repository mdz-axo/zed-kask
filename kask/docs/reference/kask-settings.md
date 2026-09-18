---
title: "Kask Settings Reference"
audience: [developers, operators, agents]
last_updated: 2026-09-18
version: "0.39.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, domain]
---

# Kask Settings Reference

The **Settings → Kask** section configures hKask features in zed-kask. It has
sub-pages for data services, inference providers, MCP servers, and per-server
configuration. The canonical source of truth for every default is the `Default`
impl in `kask/crates/kask_bridge/src/settings.rs` — `From<Content>` and
`mcp_env()` both read from `Default`, never from inlined magic numbers or
`#[serde(default = "...")]` attributes (which never fire because the settings
system deserializes `SettingsContent`, not `KaskSettings`).

## Top-level struct (`KaskSettings`)

`KaskSettings` has 14 subsections plus two top-level storage-root fields (`kask/crates/kask_bridge/src/settings.rs:35-96`):

| Field | Type | Default source |
|-------|------|---------------|
| `data_dir` | `String` | `""` → runtime resolves `~/.local/share/zed-kask/` (hidden infrastructure tree: databases, agents/, mcp/, skills/, threads/); emitted as `HKASK_DATA_DIR` |
| `artifacts_dir` | `String` | `""` → runtime resolves `~/Documents/zk-data/` (visible artifacts tree: ALL artifact files and outputs at `{server}-mcp/{artifact-type}/`); emitted as `HKASK_ARTIFACTS_DIR` |
| `general` | `KaskGeneralSettings` | `Default` |
| `mcp` | `KaskMcpSettings` | `Default` |
| `curator` | `KaskCuratorSettings` | `Default` |
| `memory` | `KaskMemorySettings` | `Default` |
| `condenser` | `KaskCondenserSettings` | `Default` |
| `research` | `KaskResearchSettings` | derived `Default` |
| `companies` | `KaskCompaniesSettings` | `Default` |
| `corpus` | `KaskCorpusSettings` | `Default` |
| `scenarios` | `KaskScenariosSettings` | derived `Default` |
| `prediction_markets` | `KaskPredictionMarketsSettings` | derived `Default` |
| `swarm` | `KaskSwarmSettings` | `Default` |
| `training` | `KaskTrainingSettings` | derived `Default` |
| `media` | `KaskMediaSettings` | `Default` |
| `models` | `KaskModelsSettings` | `Default` |

## Inference admission and resilience (`KaskGeneralSettings`)

The operator-ratified core-review D3 contract (2026-09-04) is a total
**admission-to-completion** deadline: queue wait, model resolution, stream
establishment, and drain all share `general.inference_timeout_secs` (default
300 seconds). This supersedes establishment-only descriptions; AIMD is unchanged.
Zero disables this server timer, not cancellation or admission limits. IPC
clients retain their 600-second transport fallback when the timer is disabled;
otherwise they allow the published server timeout plus 30 seconds of grace.

Accepted bridge requests are bounded at twice `general.max_concurrency`;
active calls remain bounded at that configured concurrency. Exhausting admission
capacity returns `Overloaded` before provider dispatch; full active utilization
alone is healthy. Expiry returns `Timeout`. Caller/channel closure cancels local
queued or running work and releases capacity.

`general.inference_circuit_failure_threshold` (default 3) consecutive transient failures opens the local inference circuit. While open, requests return `CircuitOpen` before dispatch. After `general.inference_circuit_open_secs` (default 30), one half-open probe is admitted; success closes and transient failure reopens the circuit. Permanent authorization, configuration, model, and provider failures do not open the transient circuit; Regulation escalates them with typed evidence. Unknown-effect requests are not automatically replayed. The settings/default contract is at `kask/crates/kask_bridge/src/settings.rs:98-137`; the state machine and receipt path are at `kask/crates/kask_bridge/src/inference_resilience.rs:44-160`.

## MCP Servers (`KaskMcpSettings`)

Toggle which of the built-in kask MCP servers are loaded.[^mcp-spec-settings]
The 11 servers (`BUILT_IN_MCP_SERVERS` registry in `kask/crates/kask_bridge/src/mcp_servers.rs`,
IDs via `builtin_mcp_server_ids()`):
`companies`, `corpus`, `curator`, `kata-kanban`, `media`, `portfolio`, `prediction-markets`,
`research`, `scenarios`, `swarm`, `training`. The crates live under `kask/mcp-servers/`
(11 `hkask-mcp-*` crates).

| Field | Type | Default |
|-------|------|--------|
| `load_default` | `bool` | `true` — load all built-in servers |
| `overrides` | `HashMap<String, bool>` | empty — per-server overrides (e.g. "curator": false) |
| `delegated_tools` | `HashMap<String, Vec<String>>` | empty — deny delegated IPC tools unless explicitly granted |

The master `load_default` toggle controls all servers; individual `overrides`
take precedence. Set `load_default: false` to disable all kask MCP servers.
Load/unload toggles take effect at runtime: the `SettingsStore` observer
(`sync_kask_mcp_runtime_servers`, D45) stops/starts the governed server
through the `McpRuntime`'s own primitives. The servers also appear in
Settings → AI → MCP Servers as managed rows (D45).

### Delegated tools: parent authority (D1, 2026-09-04)

`kask.mcp.delegated_tools` maps a **child server ID** to exact runtime
`server/tool` names. For example (illustrative; not installed automatically):

```json
{"kask":{"mcp":{"delegated_tools":{"swarm":["research/rss_search"]}}}}
```

The parent injects only that child's opaque `HKASK_TOOL_GRANT` token. The IPC
server requires both membership in the parent-held grant and the request's
allowlist. Existing agent-card allowlists alone no longer authorize IPC calls.
Missing, empty, malformed, revoked, or insufficient grants deny; wildcards are
not supported. Unchanged grants retain stable tokens; permission changes and
unload invalidate old tokens, and reload issues a new token. Disabled children
cannot regain grants during environment-diff passes. These are per-server
capabilities, not PID-bound credentials or OS isolation against arbitrary same-UID
processes. Grant values and raw request parse failures are not logged.

Worktree creation is a separate host effect selected explicitly in this map:
`"kata-kanban": ["host/create_worktree_thread", "research/rss_search"]`.
The child must also send its agent-card tool narrowing. The host intersects it
with the grant, bounds the spawn queue to 32 requests, and rechecks revocation
and disconnect before dequeue admission (`kask_bridge/src/inference_ipc_server.rs`,
`WorktreeSpawnRequest::execute`). Empty narrowing grants no child tools.
MCP grants never enable native builtins such as terminal. No fallback executor
starts on refusal or a lost reply. A queued UI thread is reported as pending,
not proof of execution. Grants remain server-scoped: native agents use their
host-bound `create_thread`/`spawn_agent` path; native `kanban_task_spawn` is denied
until per-invocation attribution/authority is implemented (repair plan P2).

## Data Services

API keys for data services (Exa, Tavily, Brave, SerpAPI, Firecrawl, FMP,
EODHD, Nebius, HuggingFace, FRED, etc.) are stored in the system
keychain under `kask://credentials/<key>`. Inference-provider keys
(OpenRouter, DeepInfra, RunPod) are the exception — each lives at its
provider's `api_url` keychain slot (one key, one location), the same slot
zed's `ApiKeyState` reads; see [Inference Providers](#inference-providers).
There are no settings.json
toggles — a service is enabled when its key is present in the keychain.
When MCP servers start, the composition root reads keys from the keychain
and injects them as environment variables into the MCP server child process.

| Keychain key | Env var injected |
|--------------|-------------------|
| `kask://credentials/exa` | `HKASK_EXA_API_KEY` |
| `kask://credentials/tavily` | `HKASK_TAVILY_API_KEY` |
| `kask://credentials/brave` | `HKASK_BRAVE_API_KEY` |
| `kask://credentials/serpapi` | `HKASK_SERPAPI_API_KEY` |
| `kask://credentials/firecrawl` | `HKASK_FIRECRAWL_API_KEY` |
| `kask://credentials/fmp` | `HKASK_FMP_API_KEY` |
| `kask://credentials/eodhd` | `HKASK_EODHD_API_KEY` |
| `https://api.runpod.io` (provider slot) | `RUNPOD_API_KEY` |
| `kask://credentials/nebius_project_id` | `NEBIUS_PROJECT_ID` |
| `kask://credentials/hf_token` | `HF_TOKEN` |
| `kask://credentials/fred` | `HKASK_FRED_API_KEY` |

**To configure**: Enter the API key via Settings → Kask → Data Services.
The key is written to the keychain immediately and the MCP server restarts
with the new key.

## Inference Providers

Inference providers (OpenRouter, DeepInfra, RunPod, Ollama) are NOT
configured through the kask settings section — there is no
`KaskInferenceProvidersSettings` struct. Providers are registered via zed's
native **Settings → AI → LLM Providers**, and each provider's API key
lives at exactly ONE keychain location: the provider's `api_url`
(`https://openrouter.ai/api/v1`, `https://api.deepinfra.com/v1/openai`,
`https://api.runpod.io`) — the same slot zed's `ApiKeyState` reads.
Every consumer — `ApiKeyState`, MCP server env injection
(`credential_urls_for_mcp` via `credential_url_for_key`), the embedding
port (`resolve_embedding_credentials`), and the IPC batch/rerank paths —
resolves that one slot. The former `kask://credentials/<key>` duplicates
are dead data nothing reads: they were the 2026-08-31 split-brain in which
a stale copy fed MCP servers a dead key while the user's fresh key sat
unread at `api_url` (the DeepInfra 401).

The Data Services RunPod row writes the same `https://api.runpod.io` slot
and additionally drives the key into the RunPod endpoint provider live
(via its `set_api_key` path), so `RunPod/*` models refresh without a
restart.

fal.ai is not an inference provider here — it is not OpenAI-compatible
(`/v1/chat/completions` returns 404; `/v1/models` uses `Authorization: Key`).
Its `FALAI_API_KEY` is managed as a data-service credential (see Data
Services) and consumed by the media and corpus MCP servers. Cline was removed
from the kask provider set.

**To add models**: go to Settings → AI → LLM
Providers, find the provider, and add models via its configuration sub-page.

## Curator (`KaskCuratorSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `always_on` | `bool` | `true` | Curator agent runs regulation loops in background |
| `algedonic_threshold` | `f64` | `0.8` | Algedonic signal threshold (0.0–1.0) |
| `email` | `KaskCuratorEmailSettings` | `Default` | Outbound algedonic alerts via MXroute |

### Curator Email (`KaskCuratorEmailSettings`)

Non-secret fields. The SMTP password is stored in the OS keychain under
`kask://credentials/hkask_smtp_password`, not here. The composition root
reads it from the keychain into the EDITOR process env (the alert email
sink runs in-process there, wired from settings via
`CuratorAlertEmailSink::try_from_settings`); MCP server children receive no
email vars — no server reads them. `alert_email`/`smtp_username` select the
alert recipient; the SMTP transport itself (`hkask-email`'s `send_email`)
reads its remaining configuration (`HKASK_MXROUTE_SERVER`,
`HKASK_SMTP_USERNAME`, `HKASK_CURATOR_EMAIL`) from the editor process env.

| Field | Type | Default | Consumed |
|-------|------|---------|-----------|
| `mxroute_server` | `String` | `""` | settings→transport note above |
| `smtp_username` | `String` | `""` | alert-recipient fallback; note above |
| `curator_email` | `String` | `""` | settings→transport note above |
| `alert_email` | `String` | `""` | alert recipient (falls back to `smtp_username`) |
| `authorized_emails` | `Vec<String>` | `[]` | settings→transport note above |
| `inbox_poll_interval_secs` | `u64` | `0` | `HKASK_INBOX_POLL_INTERVAL_SECS` (0 = disabled; reserved for future IMAP) |
| `digest_interval_secs` | `u64` | `0` | `HKASK_DIGEST_INTERVAL_SECS` (0 = disabled; reserved for future digest) |

When `email` is `None` or unconfigured, the alert email sink falls back to the
log-only sink (`LogAlertEmailSink` in `crates/zed/src/main.rs`).

## Guard

There is no `KaskGuardSettings` struct. Direct chat is unguarded (provider-side safety + refusal fallback); the guard only wraps the skill cascade path. There is no configurable `direct_chat_strategy` — the `cascade_only` behavior is hardcoded.[^owasp-llm-guard-settings]

## Memory (`KaskMemorySettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `consolidation_cadence_secs` | `u64` | `300` | 0 = disabled; otherwise at least 60s. First pass waits one interval; cadences above one hour are preserved. |
| `confidence_floor` | `f64` | `0.3` | Memory retention floor (0.0–1.0) |
| `recall_limit` | `u32` | `5` | Max snippets retrieved for context injection |
| `recall_min_confidence` | `f64` | `0.3` | Min confidence for injection (0.0–1.0) |
| `auto_inject` | `bool` | `true` | Auto-inject recalled memories into prompts |
| `memory_life_days` | `f64` | `180` | Memory life S in days (Wozniak-Gorzelanczyk forgetting curve `R(t) = exp(-t/S)`). Half-life is `S·ln(2)`. Applied when the bridge curator store is constructed/reopened; emitted as `HKASK_MEMORY_LIFE_DAYS` for the curator MCP server, which applies it on both embedding-capable and degraded store paths |

**T16 settings-flow closure (2026-09-08):** the content schema and `KaskMemorySettings` conversion feed the bridge's `RealMemoryPort::new` → `CuratorStore` → `MemoryStore::with_memory_life_days`. `emit_curator_distillation_env` emits a non-default value; the curator server's config allowlist admits it; `memory_life_days_from_env` validates it (malformed/non-positive/non-finite values warn and use the canonical default). The bridge regulation sensor reads the applied store, using a default estimate if the store is unavailable. These are constructor/reopen settings, not a new live bridge-reconfiguration mechanism.

Step 6 of the settings-flow checklist is **already implemented**, not skipped: Settings → Kask → Memory renders the Memory Life field in `crates/settings_ui/src/pages/kask_page/memory.rs:73–94` and writes `settings.kask.memory.memory_life_days`. The incoming handoff's claim that no memory knobs had a UI was incorrect. No new UI was added. Bridge store/emission/allowlist tests, the full Zed check, and scoped lint pass (see `tasks/plan.md`).

## Condenser (`KaskCondenserSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `profile` | `String` | `"normal"` | `"heavy"` (10% retention, 30 max lines), `"normal"` (20%, 80), `"soft"` (60%, 200), `"light"` (95%, no max) |
| `auto_compress_tool_results` | `bool` | `false` | Compress tool results before message history |

## Research (`KaskResearchSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `research_db` | `String` | `""` | Empty resolves under the Kask data directory; feeds and research-run ledger share this server-owned database path (`kask/crates/kask_bridge/src/settings.rs:322-329`) |

## Companies (`KaskCompaniesSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `chronic_staleness_days` | `u32` | `0` | 0 = use hardcoded default (90); >0 = override |
| `fermi_defaults` | `String` | `""` | JSON with `growth` + `margin` arrays; empty = hardcoded defaults |
| `investor_required_return` | `f64` | `0.15` | Investor equity hurdle used in companies-MCP modified WACC; emitted on every managed launch |

No `transactions_dir` field — the portfolio transactions dir is derived from the artifacts dir as `portfolio-mcp/transactions/` by `mcp_env()`. See the Portfolio section below.

## Corpus (`KaskCorpusSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `embedding_dim` | `u32` | `1024` | Must match embedding model output |
| `embedding_model` | `String` | `"ollama/qwen3-embedding:0.6b"` | Corpus-layer default; `models.embedding_model` overrides it |
| `template_root` | `String` | `"kask/registry"` | Jinja2 registry root; `mcp_env()` publishes the effective root to MCP servers and `KaskSettings::resolved_template_root()` resolves the same rule in-process (curator memory chunk tagging) |

These are the only `KaskCorpusSettings` fields (`kask/crates/kask_bridge/src/settings.rs:355-386`). OCR model selection lives under `KaskModelsSettings`; OCR pipeline thresholds are not Kask settings fields.

## Scenarios (`KaskScenariosSettings`)

No fields — the scenarios data dir is derived from the global `data_dir` as `mcp/scenarios/` by `mcp_env()`. The server reads it via `HKASK_SCENARIOS_DATA`.

## Prediction Markets (`KaskPredictionMarketsSettings`)

Prediction-markets data-service configuration (`kask/crates/kask_bridge/src/settings.rs:388-399`). Its data directory is derived from the top-level `data_dir`; there is no subsection `data_dir` field.

| Field | Type | Default | Env var injected | Notes |
|-------|------|---------|-------------------|-------|
| `cache_ttl_secs` | `u64` | `0` | `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | Market-data cache TTL; 0 = server default |
| `base_events` | `String` | `""` | `HKASK_PREDICTION_MARKETS_BASE_EVENTS` | Base-event registry: `"domain:series,..."` pairs for CMP construction |

## Swarm (`KaskSwarmSettings`)

Agent Bestiary World (ABW) swarm integration (added 2026-08-01). See `diataxis/swarm_system/` and `DIVERGENCE.md` D2/D33.[^reynolds-swarm-settings]

| Field | Type | Default | Env var injected | Notes |
|-------|------|---------|-------------------|-------|
| `mode` | `SwarmModeConfig` | `"abw"` | `HKASK_SWARM_MODE` | `"abw"` (Agent Bestiary World, v1) or `"local"` (local substrate crates, v2 §15) |
| `api_url` | `String` | `""` | `HKASK_ABW_API_URL` | ABW API base URL override; empty = `https://agent-bestiary.world` |
| `max_credits_per_dispatch` | `u32` | `50` | `HKASK_ABW_MAX_CREDITS` | Per-dispatch credit ceiling (S3 budget gate); dispatches above this are refused pre-spend |
| `curator_consent_default` | `bool` | `false` | `HKASK_ABW_CURATOR_CONSENT_DEFAULT` | When `false`, `swarm_xaman` requires a per-call `consent_token`; `true` = operator globally opted in |
| `default_agent_model` | `String` | `""` | server configuration | Empty uses the server's own default agent model |
| `a2a_http_enabled` | `bool` | `false` | server configuration | Opt-in loopback A2A HTTP gateway |
| `embedding_dim` | `usize` | `1024` | `HKASK_EMBEDDING_DIM` | Shared semantic-memory vector dimension |

The complete subsection and defaults are at `kask/crates/kask_bridge/src/settings.rs:416-517`.

No `local_agents_dir`, `local_swarms_dir`, or `memory_db_path` fields — these paths are derived from the global `data_dir` as `mcp/swarm/agents/curated/`, `mcp/swarm/swarms/`, and `mcp/swarm/memory.db` by `mcp_env()`. The server reads them via `HKASK_LOCAL_AGENTS_DIR`, `HKASK_LOCAL_SWARMS_DIR`, and `HKASK_SWARM_MEMORY_DB`.

The ABW API key is a secret — it lives in the keychain under
`kask://credentials/hkask_abw_api_key`, injected as `HKASK_ABW_API_KEY` by
`mcp_env_with_credentials`, not by `mcp_env()`. The bridge `Default` impl (`kask/crates/kask_bridge/src/settings.rs:497-517`) MUST stay in sync with `SwarmConfig::default()` in `kask/mcp-servers/hkask-mcp-swarm/src/config.rs:137-158` — the two impls are
deliberately duplicated across the crate boundary to avoid a circular
dependency.

## Training (`KaskTrainingSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `host` | `String` | `""` | `"nebius"` or `"runpod"`; empty = auto-detect from API keys |
| `cache_dir` | `String` | `""` | Dataset pipeline cache; empty = agent adapters directory |

## Media (`KaskMediaSettings`)

| Field | Type | Default | Env var |
|-------|------|---------|---------|
| `tts_model` | `String` | `""` | `HKASK_MEDIA_TTS_MODEL` |
| `stt_model` | `String` | `"OpenRouter/openai/whisper-large-v3-turbo"` | `HKASK_MEDIA_STT_MODEL` |
| `vision_model` | `String` | `"OpenRouter/openai/gpt-4o-mini"` | `HKASK_MEDIA_VISION_MODEL` |
| `image_gen_model` | `String` | `""` | `HKASK_MEDIA_IMAGE_GEN_MODEL` |
| `video_model` | `String` | `""` | `HKASK_MEDIA_VIDEO_MODEL` |

Empty TTS, image, or video values remain unconfigured and fail visibly; STT and vision defaults come from shared inference constants (`kask/crates/kask_bridge/src/settings.rs:533-569`, `kask/crates/hkask-inference/src/model_constants.rs:173-203`).

## Models (`KaskModelsSettings`)

Kask-wide model configuration (`settings.rs`). Every model field carries a
**code default** (operator ruling 2026-09-04, superseding the former
no-hidden-models spec): the system works out of the box, and the settings
UI / settings.json overrides the defaults. The default values are the
operator's configured models, verbatim.[^ousterhout-models-settings]

| Field | Type | Default | Resolution |
|-------|------|---------|------------|
| `default_model` | `String` | `"OpenRouter/z-ai/glm-5.3"` | Injected as `HKASK_DEFAULT_MODEL` (`mcp_env.rs`); the zed-side inference stack resolves it from the registry, falling back to the zed default when the provider is not configured |
| `embedding_model` | `String` | `""` (see note) | `effective_embedding_model()` resolves `models.embedding_model` → `corpus.embedding_model` → empty; the **embedding default lives in `KaskCorpusSettings::default()`** (`"ollama/qwen3-embedding:0.6b"`) so a models-layer default cannot shadow corpus overrides; injected as `HKASK_EMBEDDING_MODEL` |
| `classifier_model` | `String` | `"OpenRouter/z-ai/glm-5.2"` | Injected as `HKASK_CLASSIFIER_MODEL` (`mcp_env.rs`); consumed by corpus tagging, assertion extraction, and the memory write path's chunk tagging. glm-5.2 because the classifier must be non-thinking (or thinking-disable-able) — glm-5.3-flash cannot disable thinking |
| `qa_generation_model` | `String` | `""` | Explicit QA tool `model` > this setting (`HKASK_QA_GENERATION_MODEL`, corpus allowlist). Unset/invalid fails visibly; never chat, classifier, or training base model |
| `ocr_model` | `String` | `"ollama/glm-ocr:latest"` | Injected as `HKASK_OCR_MODEL` |
| `rerank_model` | `String` | `"deepinfra/Qwen/Qwen3-Reranker-8B"` | Shared `DEFAULT_RERANK_MODEL`; injected as `HKASK_RERANK_MODEL` |

QA generation is configured under **Settings → Kask → Models → QA Generation Model**.
The input writes `kask.models.qa_generation_model`; the host settings writer reloads it,
`From<Content>` preserves it, and `mcp_env` emits it for corpus. The existing settings
observer restarts the server when that filtered environment changes. Clearing the input
leaves QA unconfigured; it does not select the active model. Explicit blank/malformed
tool overrides are errors, not requests to fall back.

`resolve_qa_generation_model` in `hkask_inference::model_constants` enforces that
precedence and provider qualification; the registry/provider remains authoritative for
availability. Both corpus single and prepared-batch QA pass the selected model explicitly.
The training server currently has no QA-generation caller: its evaluation model and
training `base_model` remain unchanged, and it does not receive the QA env variable.
The old `HKASK_QA_MODEL` remains a consolidation input only, not a QA-generation alias.

There is no dedicated QA verification model setting (deleted 2026-09-18): an LLM
verifying another LLM is circular self-authorization. Generation emits unverified
candidates only (`prepared-qa-grounding-candidate-v1`, `grounding_status:
pending_external_verification`), and acceptance is decided by the corpus server's
identity-bound external grounding gate at ingestion — never by a second model call.
Reviewed passage/level decisions reach generation as a required
`prepared-qa-adjudication-v2` manifest; there is no unadjudicated generation path.

All QA transports request non-thinking generation. Synchronous OpenRouter serialization
and provider-batch JSONL send `reasoning.effort: "none"`; `exclude` alone only hides
reasoning text ([OpenRouter reasoning documentation](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)).
The operator supplied alias [`~openai/gpt-sol-latest`](https://openrouter.ai/~openai/gpt-sol-latest)
(displayed as GPT-5.6 Sol on 2026-09-11) would be written as
`OpenRouter/~openai/gpt-sol-latest` in this setting. This is **not a default or a claim
of catalog/provider verification**; the coordinator owns those checks. No settings are
installed automatically.

`hkask_inference::model_constants` also defines **env-var accessors** —
`classifier_model()`, `embedding_model()`, `ocr_model()`, `rerank_model()` —
each returning `Option<String>` (`None` = env var not injected; the settings
layers carry the code defaults and inject these env vars for MCP server
children). The general/chat, embedding, classifier, QA-generation, and OCR defaults live in the settings `Default` implementations (`kask/crates/kask_bridge/src/settings.rs:599-676`; `kask/crates/hkask-services-core/src/standalone_settings.rs`). Shared constants remain where multiple settings layers consume one ratified value: media STT, media vision, and research reranking are defined in `kask/crates/hkask-inference/src/model_constants.rs:173-203`. TTS, video, and image-generation remain explicitly unconfigured when their media setting is empty.

## Keychain Architecture

There is ONE keychain namespace: zed's `CredentialsProvider` — keys stored
under `kask://credentials/<key>` (data services) or the provider's `api_url`
slot (inference providers — one key, one location,
D5).[^owasp-keychain-settings] The legacy `service=hkask` namespace is fully
removed and purged at startup (`hkask-keystore/src/keychain.rs`); the single
internal key is `hkask_db_passphrase` in the `kask://credentials/` namespace.

The composition root bridges the keychain to child processes: at MCP server
startup, `build_mcp_server_env` reads keys from the keychain and injects them
as env vars. MCP servers read API keys from env vars only — there is no
keychain fallback for API keys, and a missing credential surfaces as
`permission_denied` naming the env var.

### Restart-on-keychain-write

A keychain write through the settings UI (`write_credential` / `delete_credential`
in `crates/settings_ui/src/pages/kask_page.rs`) does **not** by itself restart
running MCP servers — they have already captured the env at spawn time. To close
this gap, those handlers call `nudge_mcp_servers(cx)` after a `kask://credentials/...`
write/delete. The nudge re-writes `kask.mcp.load_default` to itself via
`update_settings_file`, firing the `SettingsStore` observer →
`sync_kask_mcp_runtime_servers` → env diff → server restart with a fresh keychain
read. The nudge fires inside the async spawn, after the keychain write completes,
so the restart reads the new key. It only fires for `kask://credentials/...` URLs,
not for inference-provider `api_url` writes (those go through zed's provider
registry, which has its own reload path).

### `HKASK_DB_PASSPHRASE` resolution

The SQLCipher passphrase is resolved through a canonical 2-tier helper,
`hkask_mcp_server::server::resolve_db_passphrase(&ctx.credentials)`:
`ctx.credentials.get("HKASK_DB_PASSPHRASE")` → `resolve_credential("HKASK_DB_PASSPHRASE")`
(env → hKask keychain). All six DB-passphrase-consuming servers (kata-kanban,
training, research, curator, condenser, corpus) use this helper, not inline
re-implementations; `ServerContext::resolve_db_credential` delegates to it. A miss
returns `McpToolError::permission_denied` naming the env var.

Corpus cannot read `ctx.credentials` from serde-default call sites, so it captures
the resolved passphrase at server construction into `static CORPUS_DB_PASSPHRASE:
OnceLock<Option<String>>` (`semantic/mod.rs`) via `set_corpus_db_passphrase`;
`default_corpus_passphrase()` reads the `OnceLock` first, falls back to
`resolve_credential`, giving the full 2-tier chain (creds → env → `hkask-keystore`
keychain) without changing serde-default signatures.

### First-run provisioning

`kask_bridge::identity::provision_db_passphrase` delegates to the one canonical `hkask_keystore::provision_db_passphrase_string` chain: environment override → existing `kask://credentials/hkask_db_passphrase` entry → first-run default (`kask/crates/kask_bridge/src/identity.rs:121-141`; `kask/crates/hkask-keystore/src/keychain.rs:367-429`). Governed MCP server launch calls that wrapper before building the server environment (`kask/crates/kask_bridge/src/mcp_servers.rs:838-861`). There is no separate swarm-memory passphrase.

On first run, the DB passphrase defaults to `"allostery"`. There is ONE
passphrase for every SQLCipher database (curator, swarm memory, kata-kanban,
research, training) — no per-DB passphrases. The user can change it later
via the settings UI (Security page), which triggers atomic DB rotation
before saving the new passphrase.

### Passphrase rotation

Changing a SQLCipher passphrase requires re-encrypting the entire database —
there is no in-place `PRAGMA rekey` that survives a crash. The rotation is handled by `hkask_storage::rotate_passphrase` (`kask/crates/hkask-storage/src/rotation.rs:108-204`), which:

1. Opens the source DB with the old passphrase (verifies it).
2. Creates `<db>.new` encrypted with the new passphrase.
3. Copies all user tables + `sqlite_sequence` in a single transaction.
4. Atomically renames: `<db>` → `<db>.old`, `<db>.new` → `<db>`.
   Deletes `.old` on success.

If any step fails, the original DB is untouched — the old passphrase remains
in effect. The caller writes the new passphrase to the keychain ONLY after
rotation returns `Ok(())`.

The bridge layer wraps this in one function (`kask/crates/kask_bridge/src/identity.rs:210-283`):

- `rotate_all_kask_db_passphrases(new_passphrase)` — rotates EVERY kask
  SQLCipher DB that exists at its resolved path (curator, swarm memory,
  kata-kanban, research, training), rolling back the already-rotated DBs
  if any rotation fails. Corpus DBs are caller-supplied per-workflow paths
  and are not covered.

It resolves the old passphrase from the keychain and each DB path from
env/data-dir. The settings UI calls it on a background spawn before
writing the new passphrase to the keychain and nudging MCP servers to
restart.

**From the settings UI:** the Security page changes the one DB passphrase. Rotation completes before the new keychain value is written; on failure, a warning is emitted and the old passphrase remains (`crates/settings_ui/src/pages/kask_page/security.rs`).


## Environment Variable Reference

All env vars can be set either via the settings UI (keychain) or via shell
environment. Shell env vars take precedence over keychain values. `KaskSettings::mcp_env` delegates to the environment builder at `kask/crates/kask_bridge/src/settings.rs:740-742` and `kask/crates/kask_bridge/src/mcp_env.rs`; settings defaults determine which optional values are emitted.
`mcp_env()` also unconditionally injects `HKASK_MCP_SERVER_IDS` (the
comma-joined `BUILT_IN_MCP_SERVERS_IDS`, consumed only by the swarm server's
`config_env` allowlist) and passes through `HKASK_DATA_DIR` and
`HKASK_CURATOR_WEBID` → `HKASK_WEBID` when set in the parent environment.

### Storage

| Env Var | Service | Notes |
|---------|--------|-------|

| `HKASK_DB_PATH` | SQLite path | |
| `HKASK_DB_PASSPHRASE` | SQLite passphrase | SQLCipher encryption |
| `HKASK_EMBEDDING_DIM` | Embedding dimension | Default 1024 (from `KaskCorpusSettings::default()`) |
| `HKASK_DATA_DIR` | hKask data directory | Passed to all MCP servers for path resolution (hidden infrastructure root) |
| `HKASK_ARTIFACTS_DIR` | hKask artifacts directory | Passed to MCP servers that resolve artifact routes (visible root, default `~/Documents/zk-data/`; from `kask.artifacts_dir` setting → env → platform default) |

### Data Services

| Env Var | Service |
|---------|--------|
| `HKASK_EODHD_API_KEY` | EODHD |
| `HKASK_FMP_API_KEY` | FMP |
| `HKASK_EXA_API_KEY` | Exa |
| `HKASK_TAVILY_API_KEY` | Tavily |
| `HKASK_BRAVE_API_KEY` | Brave Search |
| `HKASK_SERPAPI_API_KEY` | SerpAPI |
| `HKASK_FIRECRAWL_API_KEY` | Firecrawl |
| `RUNPOD_API_KEY` | RunPod |
| `RUNPOD_S3_ACCESS_KEY` | RunPod S3 |
| `RUNPOD_S3_SECRET` | RunPod S3 |
| `RUNPOD_TEMPLATE_ID` | RunPod template |
| `NEBIUS_PROJECT_ID` | Nebius |
| `NEBIUS_SUBNET_ID` | Nebius |
| `HF_TOKEN` | HuggingFace |
| `FALAI_API_KEY` | fal.ai (media generation) |

### Inference Providers

| Env Var | Service |
|---------|--------|
| `OPENROUTER_API_KEY` | OpenRouter |

`FALAI_API_KEY` is listed under Data Services (fal.ai is a media platform,
not an OpenAI-compatible chat endpoint).

### Curator Email

Read by `hkask-email` in the EDITOR process (the in-process alert email
transport); not delivered to MCP server children and not keychain entries
(except the password, which the composition root reads from the keychain
into the editor process env).

| Env Var | Service |
|---------|--------|
| `HKASK_MXROUTE_SERVER` | MXroute server hostname |
| `HKASK_SMTP_USERNAME` | SMTP auth + From header |
| `HKASK_SMTP_PASSWORD` | SMTP password (keychain → editor process env) |
| `HKASK_CURATOR_EMAIL` | From address (defaults to `HKASK_SMTP_USERNAME`) |
| `HKASK_ALERT_EMAIL` | Alert recipient (defaults to `HKASK_SMTP_USERNAME`) |
| `HKASK_AUTHORIZED_EMAILS` | Authorized sender allowlist (comma-separated) |
| `HKASK_INBOX_POLL_INTERVAL_SECS` | Inbox poll interval (0 = disabled) |
| `HKASK_DIGEST_INTERVAL_SECS` | Digest interval (0 = disabled) |

### Per-Server Config

| Env Var | Server | Source setting |
|---------|--------|----------------|

| `HKASK_CHRONIC_STALENESS_DAYS` | companies | `companies.chronic_staleness_days` |
| `HKASK_FERMI_DEFAULTS` | companies | `companies.fermi_defaults` |
| `HKASK_INVESTOR_REQUIRED_RETURN` | companies | `companies.investor_required_return` |
| `HKASK_TRANSACTIONS_DIR` | portfolio | derived from the artifacts dir as `portfolio-mcp/transactions/` |
| `HKASK_TEMPLATE_ROOT` | corpus, swarm | `corpus.template_root` |
| `HKASK_SCENARIOS_DATA` | scenarios | derived from `data_dir` as `mcp/scenarios/` |
| `HKASK_PREDICTION_MARKETS_DATA` | prediction-markets | derived from `data_dir` as `mcp/prediction-markets/` |
| `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | prediction-markets | `prediction_markets.cache_ttl_secs` |
| `HKASK_PREDICTION_MARKETS_BASE_EVENTS` | prediction-markets | `prediction_markets.base_events` |
| `HKASK_SWARM_MODE` | swarm | `swarm.mode` |
| `HKASK_ABW_API_URL` | swarm | `swarm.api_url` |
| `HKASK_ABW_MAX_CREDITS` | swarm | `swarm.max_credits_per_dispatch` |
| `HKASK_ABW_CURATOR_CONSENT_DEFAULT` | swarm | `swarm.curator_consent_default` |
| `HKASK_LOCAL_AGENTS_DIR` | swarm | derived from `data_dir` as `mcp/swarm/agents/curated/` |
| `HKASK_LOCAL_SWARMS_DIR` | swarm | derived from `data_dir` as `mcp/swarm/swarms/` |
| `HKASK_SWARM_MEMORY_DB` | swarm | derived from `data_dir` as `mcp/swarm/memory.db` |
| `HKASK_ABW_API_KEY` | swarm | ABW API key (keychain only) |
| `HKASK_TRAINING_HOST` | training | `training.host` |
| `HKASK_TRAINING_CACHE_DIR` | training | `training.cache_dir` |
| `HKASK_DEFAULT_MODEL` | all | `models.default_model` |
| `HKASK_EMBEDDING_MODEL` | all | `models.embedding_model` / `corpus.embedding_model` |
| `HKASK_CLASSIFIER_MODEL` | all | `models.classifier_model` |
| `HKASK_QA_GENERATION_MODEL` | corpus | `models.qa_generation_model` (no default) |
| `HKASK_WEBID` | curator | mapped from `HKASK_CURATOR_WEBID` |
| `HKASK_MCP_SERVER_IDS` | swarm | `BUILT_IN_MCP_SERVERS_IDS` joined (unconditional) |
| `HKASK_CURATOR_DB` | curator | injected by deferred task |
| `HKASK_KANBAN_DB` | kata-kanban | Operator override for kanban DB path (default `mcp/kata-kanban/kanban.db`) |
| `HKASK_RESEARCH_DB` | research | Operator override for research DB path (default `mcp/research/research.db`) |
| `HKASK_TRAINING_DB` | training | Operator override for training DB path (default `mcp/training/training.db`) |
| `HKASK_SWARM_CONSENT_STORE` | swarm | Operator override for consent store path (default `mcp/swarm/consent.db`) |

The collab server is launched directly by zed-kask (not via `mcp_env()`), so
there are no `collab.*`-derived env vars.

### Runtime Tuning

These env vars tune internal runtime behavior — connection healing, health
checks, and resource caps. They are read once at startup and cached for the
process lifetime. Unset or unparsable values fall back to documented defaults
(with a `warn!` on parse failure).

| Env Var | Default | Notes |
|---------|---------|-------|
| `HKASK_MCP_RECONNECT_COOLDOWN_SECS` | `5` | Min interval between reconnect attempts for the same MCP server. Bounds crash-loop damage. |
| `HKASK_MCP_STARTUP_MAX_RETRIES` | `3` | Max retry attempts when an MCP server fails to start (spawn or handshake). |
| `HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS` | `500` | Initial backoff (ms) for startup retries. Doubles each attempt. |
| `HKASK_MCP_STARTUP_MAX_BACKOFF_SECS` | `10` | Cap on startup retry backoff (seconds). |
| `HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS` | `60` | Interval between proactive MCP server health checks. |
| `HKASK_MCP_MAX_HEALTH_FAILURES` | `3` | Max consecutive health-check failures before the supervisor stops auto-healing. |
| `HKASK_MCP_MAX_READ_BYTES` | `33554432` (32 MiB) | Read size cap for `read_capped` (CWE-400). 0 is rejected (would block all reads). |
| `HKASK_TEMPLATE_CACHE_PATH` | Platform cache dir | Template cache directory. Default: `$XDG_CACHE_HOME/hkask/templates` (Linux), `~/Library/Caches/hkask/templates` (macOS). |
| `HKASK_MEMORY_LIFE_DAYS` | `180` | Memory retention in days (≈6 months). Controls decay constant in the Bayesian forgetting model. |
| `HKASK_CHUNK_MAX_TOKENS` | `256` | Max tokens per chunk for document chunking (≈192 words, paragraph-level). |

The skill-span history cap (`max_skill_span_history`) is configurable via the
`HKASK_REG_CONFIG` YAML file, not env vars. See `SetPointsConfig` in
`hkask-regulation/src/set_points.rs`.

## Footnotes

[^mcp-spec-settings]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP server model the `load_default`/`overrides` toggle controls.

[^owasp-secrets-settings]: OWASP. (2023). *OWASP Secrets Management Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the secrets-in-keychain-not-in-config principle the data services settings follow.

[^owasp-llm-guard-settings]: OWASP. (2025). *OWASP Top 10 for Large Language Model Applications*. OWASP Foundation. https://owasp.org/www-project-top-10-for-large-language-model-applications/
    Cited for the LLM-specific security model the former guard layer (D4, removed 2026-08-10) was built to wrap the skill process with; provider-side safety and refusal fallbacks remain the active defense.

[^reynolds-swarm-settings]: Reynolds, C. W. (1987). Flocks, herds and schools: A distributed behavioral model. *ACM SIGGRAPH Computer Graphics*, 21(4), 25–34. https://doi.org/10.1145/37402.37406
    Cited for the swarm-coordination model the ABW swarm settings configure.

[^ousterhout-models-settings]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the two-layer default design (empty-string fields with effective_* fallback to constants) that avoids magic-number defaults.

[^owasp-keychain-settings]: OWASP. (2023). *OWASP Secrets Management Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the dual-namespace keychain design that separates settings-UI credentials from MCP-server credentials.
