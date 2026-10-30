---
title: "Kask Settings Reference"
audience: [developers, operators, agents]
last_updated: 2026-09-28
version: "0.41.0"
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

`KaskSettings` has 14 subsections plus two top-level storage-root fields (`kask/crates/kask_bridge/src/settings.rs:35-97`):

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
| `corpus` | `KaskCorpusSettings` | `Default` |
| `scenarios` | `KaskScenariosSettings` | derived `Default` |
| `prediction_markets` | `KaskPredictionMarketsSettings` | derived `Default` |
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

`general.inference_circuit_failure_threshold` (default 3) consecutive transient failures opens the local inference circuit. While open, requests return `CircuitOpen` before dispatch. After `general.inference_circuit_open_secs` (default 30), one half-open probe is admitted; success closes and transient failure reopens the circuit. Permanent authorization, configuration, model, and provider failures do not open the transient circuit; Regulation escalates them with typed evidence. Unknown-effect requests are not automatically replayed. The settings/default contract is at `kask/crates/kask_bridge/src/settings.rs:109-139`; the state machine and receipt path are at `kask/crates/kask_bridge/src/inference_resilience.rs:44-160`.

## MCP Servers (`KaskMcpSettings`)

Toggle which of the built-in kask MCP servers are loaded.[^mcp-spec-settings]
The 8 servers (`BUILT_IN_MCP_SERVERS` registry in `kask/crates/kask_bridge/src/mcp_servers.rs`,
IDs via `builtin_mcp_server_ids()`):
`corpus`, `curator`, `experimentation`, `kata-kanban`, `media`,
`prediction-markets`, `research`, `scenarios`. The crates live under `kask/mcp-servers/`
(8 `hkask-mcp-*` crates).

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
Settings → AI → MCP Servers as managed rows (D45). A loaded server with no discovered tools, no live connection, or an unreadable
connection snapshot is shown as neutral `Unknown`, not as actively `Starting` or
failed. `Running` requires both discovered tools and a live transport; an
explicitly unloaded server remains `Stopped`. The settings window re-renders
on the runtime's tool or connection change signal. A live transport reading
is not a successful-tool-call probe: a later call can still fail.

### Delegated tools: parent authority (D1, 2026-09-04)

`kask.mcp.delegated_tools` maps a **child server ID** to exact runtime
`server/tool` names. For example (illustrative; not installed automatically):

```json
{"kask":{"mcp":{"delegated_tools":{"kata-kanban":["research/rss_search"]}}}}
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

API keys for data services (Exa, Tavily, Brave, SerpAPI, Firecrawl, FRED, etc.) are stored in the system
keychain under `kask://credentials/<key>`. Inference-provider keys
(OpenRouter, DeepInfra, RunPod, KiloCode) are the exception — each lives at its
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
| `https://api.runpod.io` (provider slot) | `RUNPOD_API_KEY` |
| `kask://credentials/fred` | `HKASK_FRED_API_KEY` |

**To configure**: Enter the API key via Settings → Kask → Data Services.
The key is written to the keychain immediately and the MCP server restarts
with the new key.

## Inference Providers

Inference providers (OpenRouter, DeepInfra, RunPod, KiloCode, Ollama) are NOT
configured through the kask settings section — there is no
`KaskInferenceProvidersSettings` struct. Providers are registered via zed's
native **Settings → AI → LLM Providers**, and each provider's API key
lives at exactly ONE keychain location: the provider's `api_url`
(`https://openrouter.ai/api/v1`, `https://api.deepinfra.com/v1/openai`,
`https://api.runpod.io`, `https://api.kilo.ai/api/gateway`) — the same slot zed's `ApiKeyState` reads.
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
| `interaction_mode` | `CuratorInteractionMode` | `collaboration` | User–Curator dyad level, set only by the user: `control` (Level 1), `collaboration` (Level 2), `learning_collaboration` (Level 3). Read per new Curator thread; see `architecture/functional-interaction-spec.md` §9 |
| `email` | `KaskCuratorEmailSettings` | `Default` | Outbound algedonic alerts via MXroute |

Interaction mode adds level-specific guidance to the **system prompt's
Session Context** for each new Curator thread; it does not replace the shared
agent prompt, change an existing thread, grant tool rights, or enforce a
permission boundary. Level 1 asks Curator to wait for explicit decisions on
proposals; Level 2 (default) asks it to work toward and record shared decisions;
Level 3 also asks both sides to challenge and learn from each other with
evidence. Level 4 (trust and delegation) is a future design, not a setting.
The behavior contract and its limits are specified in
[Functional Interaction Specification §9](../architecture/functional-interaction-spec.md#9-the-usercurator-dyad-and-its-levels).

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

These are the only `KaskCuratorEmailSettings` fields (`kask/crates/kask_bridge/src/settings.rs:213-230`).

When `email` is `None` or unconfigured, the alert email sink falls back to the
log-only sink (`LogAlertEmailSink` in `crates/zed/src/main.rs`).

## Affordance (`KaskAffordanceSettings`)

Per-turn instrument-affordance injection (D94, spec B): for substantive
prompts (≥ 20 words), the skill catalog is matched to the request by embedding
similarity and the top-3 candidates are surfaced with their exact invocation
handles in a bounded block at the request tail — converting skill discovery
from recall into perception (the measured failure: 100% activation when a
skill is named, 0/3 primary-contract discovery otherwise —
`research/native-first-denominator-sample.md`).

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `enabled` | `bool` | `false` | Read per turn — a flip applies to the next turn without a restart. Default off so the operator controls the pre/post boundary against the native-first-rate baseline; see `research/instrument-affordance-experiment.md` §5 for the mechanism and the next experiment (n ≥ 30). |

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
| `federated_auto_inject` | `bool` | `false` | Opt in to injecting selected federated sources into curator chat |
| `federated_source_ids` | `Vec<String>` | `[]` | Source IDs selected from the curator federated sources manifest |
| `memory_life_days` | `f64` | `90` | Memory life S in days (Wozniak-Gorzelanczyk forgetting curve `R(t) = exp(-t/S)`). Half-life is `S·ln(2)`. Applied when the bridge curator store is constructed/reopened; emitted as `HKASK_MEMORY_LIFE_DAYS` for the curator MCP server, which applies it on both embedding-capable and degraded store paths |
| `distillation_cadence_secs` | `u64` | `600` | ALWAYS-mode distillation cadence in seconds (0 = disabled); the curator server's background pass distills finished threads into additive candidate lesson h_mems at the 0.5 confidence floor |
| `distillation_idle_secs` | `u64` | `300` | A thread counts as finished when its newest turn is at least this many seconds old |
| `forgetting_days` | `u64` | `7` | Distillation-gated forgetting age in days (0 = disabled). Default matches the curator server's `DEFAULT_FORGETTING_DAYS` |

These are the only `KaskMemorySettings` fields (`kask/crates/kask_bridge/src/settings.rs:233-300`).

**Settings-flow closure (2026-09-08):** the content schema and `KaskMemorySettings` conversion feed the bridge's `RealMemoryPort::new` → `CuratorStore` → `MemoryStore::with_memory_life_days`. `emit_curator_distillation_env` emits a non-default value; the curator server's config allowlist admits it; `memory_life_days_from_env` validates it (malformed/non-positive/non-finite values warn and use the canonical default). The bridge regulation sensor reads the applied store, using a default estimate if the store is unavailable. These are constructor/reopen settings, not a new live bridge-reconfiguration mechanism.

Step 6 of the settings-flow checklist is **already implemented**, not skipped: Settings → Kask → Memory renders the Memory Life field in `crates/settings_ui/src/pages/kask_page/memory.rs:177-192` and writes `settings.kask.memory.memory_life_days` (2026-10-13: the page also renders the Forgetting Horizon field for `kask.memory.forgetting_days` at `:194-209`). No new UI was added. Bridge store/emission/allowlist tests, the full Zed check, and scoped lint pass (evidence: `tasks/plan.md` in git history, last at `b1375ff3be`).

## Condenser (`KaskCondenserSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `profile` | `String` | `"normal"` | `"heavy"` (10% retention, 30 max lines), `"normal"` (20%, 80), `"soft"` (60%, 200), `"light"` (95%, no max) |
| `auto_compress_tool_results` | `bool` | `false` | Compress tool results before message history |

## Research (`KaskResearchSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `research_db` | `String` | `""` | Empty resolves under the Kask data directory; feeds and research-run ledger share this server-owned database path (`kask/crates/kask_bridge/src/settings.rs:330-336`) |

## Corpus (`KaskCorpusSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `embedding_dim` | `u32` | `1024` | The width every embed request asks MRL-capable models for via the OpenAI-compatible `dimensions` parameter (the default model, Qwen3-Embedding-8B, truncates its native 4096 to this); must equal the vec0 table width |
| `embedding_model` | `String` | `"OpenRouter/qwen/qwen3-embedding-8b"` | Corpus-layer default (moved from `ollama/qwen3-embedding:0.6b` on 2026-09-28); `models.embedding_model` overrides it; the OpenRouter route needs `OPENROUTER_API_KEY` |
| `template_root` | `String` | `"kask/registry"` | Jinja2 registry root; `mcp_env()` publishes the effective root to MCP servers and `KaskSettings::resolved_template_root()` resolves the same rule in-process (curator memory chunk tagging) |

These are the only `KaskCorpusSettings` fields (`kask/crates/kask_bridge/src/settings.rs:362-393`). OCR model selection lives under `KaskModelsSettings`; OCR pipeline thresholds are not Kask settings fields.

### Embedding model lifecycle (2026-09-28 migration)

The embedding model migrated from `Qwen/Qwen3-Embedding-0.6B` to
`OpenRouter/qwen/qwen3-embedding-8B` (operator decision 2026-09-28). The
width did NOT change: 1024 is the MRL truncation width requested via the
OpenAI-compatible `dimensions` parameter, so width-bound vec0 tables keep
their schema — only the embedding space changed, and vectors from the two
models are not comparable (stale rows poison KNN rather than erroring, so
they were purged, not kept).

Resolution is one chain everywhere — `KaskSettings::effective_embedding_model()`
(`models.embedding_model` → `corpus.embedding_model` → code default). The
zed-side embedding port binds the provider named by that same chain once at
startup (`crates/zed/src/main.rs`); MCP children receive the same value as
`HKASK_EMBEDDING_MODEL`. A mid-session change to the embedding model (or its
provider) therefore needs a Zed restart to re-bind the port — until then,
embeds fail with `cannot use the embedding port bound to …`, surfaced on
every semantic-recall path. This was the 2026-09-28 incomplete-migration
failure: the port had been bound from a second, divergent chain
(`corpus.embedding_model` only, or the zed process's own env), so after the
model change the children requested the new model against a port still
bound to the old provider.

Artifacts built for the 0.6B model were deleted 2026-09-28: the sealed
calibration corpus under `zk-data/corpus-mcp/calibration/`, the
`gentle-lovelace` style DB (rebuilt from
`zk-data/corpus-mcp/styles/gentle-rebuild/chunks.jsonl` with the new model),
and every stale embedding row in the curator memory DB
(≈11,923 + 58; the h_mems were kept — `curator_memory_backfill_embeddings`
re-embeds them with the new model after the restart).

## Scenarios (`KaskScenariosSettings`)

No fields — the scenarios data dir is derived from the global `data_dir` as `mcp/scenarios/` by `mcp_env()`. The server reads it via `HKASK_SCENARIOS_DATA`.

## Prediction Markets (`KaskPredictionMarketsSettings`)

Prediction-markets data-service configuration (`kask/crates/kask_bridge/src/settings.rs:397-407`). Its data directory is derived from the top-level `data_dir`; there is no subsection `data_dir` field.

| Field | Type | Default | Env var injected | Notes |
|-------|------|---------|-------------------|-------|
| `cache_ttl_secs` | `u64` | `0` | `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | Market-data cache TTL; 0 = server default |

## Media (`KaskMediaSettings`)

Research media model configuration — the media MCP server's model roles
(`kask/crates/kask_bridge/src/settings.rs`; the role models render on
Settings → Kask → Models per the operator's 2026-10-30 ruling). Defaults live
in `Default` impls only, sourced from shared inference constants
(`kask/crates/hkask-inference/src/model_constants.rs`).

| Field | Type | Default | Env var |
|-------|------|---------|---------|
| `stt_model` | `String` | `"OpenRouter/openai/whisper-large-v3-turbo"` | `HKASK_MEDIA_STT_MODEL` |
| `vision_model` | `String` | `"OpenRouter/openai/gpt-4o-mini"` | `HKASK_MEDIA_VISION_MODEL` |
| `sound_annotation_model` | `String` | `"OpenRouter/qwen/qwen3.8-omni-flash"` | `HKASK_MEDIA_SOUND_ANNOTATION_MODEL` |
| `tagging_model` | `String` | `"ollama/qwen3-vl:8b"` | `HKASK_MEDIA_TAGGING_MODEL` |

An explicitly empty value clears a default (the env var is not injected and
the pipeline fails visibly). The former creative model settings (`tts_model`,
`image_gen_model`, `video_model`) were removed with the creative ops — there
are no settings fields and no hidden fallbacks for them. The sound-annotation
model serves the audio passes only (per-call override → the setting →
NotConfigured — audio-only models never route via the shared
`HKASK_MEDIA_PASS_MODEL`); the tagging model serves the grounded tagging pass
(per-call override → the setting → the vision default).

## Models (`KaskModelsSettings`)

Kask-wide model configuration (`settings.rs`). Every model field carries a
**code default** (operator ruling 2026-09-04, superseding the former
no-hidden-models spec): the system works out of the box, and the settings
UI / settings.json overrides the defaults. The default values are the
operator's configured models, verbatim.[^ousterhout-models-settings]

| Field | Type | Default | Resolution |
|-------|------|---------|------------|
| `default_model` | `String` | `"OpenRouter/z-ai/glm-5.3"` | Injected as `HKASK_DEFAULT_MODEL` (`mcp_env.rs`); the zed-side inference stack resolves it from the registry, falling back to the zed default when the provider is not configured |
| `embedding_model` | `String` | `""` (see note) | `effective_embedding_model()` resolves `models.embedding_model` → `corpus.embedding_model` → empty; the **embedding default lives in `KaskCorpusSettings::default()`** (`"OpenRouter/qwen/qwen3-embedding-8b"` — MRL: every embed request carries `dimensions = embedding_dim`, default 1024, so width-bound vec0 stores keep receiving fitting vectors) so a models-layer default cannot shadow corpus overrides; injected as `HKASK_EMBEDDING_MODEL` |
| `classifier_model` | `String` | `"OpenRouter/z-ai/glm-5.2"` | Injected as `HKASK_CLASSIFIER_MODEL` (`mcp_env.rs`); consumed by corpus tagging, assertion extraction, and the memory write path's chunk tagging. glm-5.2 because the classifier must be non-thinking (or thinking-disable-able) — glm-5.3-flash cannot disable thinking |
| `qa_generation_model` | `String` | `""` | Explicit QA tool `model` > this setting (`HKASK_QA_GENERATION_MODEL`, corpus allowlist). Unset/invalid fails visibly; never the chat or classifier model |
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
children). The general/chat, embedding, classifier, QA-generation, and OCR defaults live in the settings `Default` implementations (`kask/crates/kask_bridge/src/settings.rs:650-718`; `kask/mcp-servers/hkask-mcp-corpus/src/settings.rs`). Shared constants remain where multiple settings layers consume one ratified value: media STT, media vision, and research reranking are defined in `kask/crates/hkask-inference/src/model_constants.rs:191-229`. TTS, video, and image-generation remain explicitly unconfigured when their media setting is empty.

## Keychain Architecture

There is ONE keychain namespace: zed's `CredentialsProvider` — keys stored
under `kask://credentials/<key>` (data services) or the provider's `api_url`
slot (inference providers — one key, one location,
D5).[^owasp-keychain-settings] The single
internal key is `hkask_db_passphrase` in the `kask://credentials/` namespace.

The composition root bridges the keychain to child processes: at MCP server
startup, `build_mcp_server_env` reads keys from the keychain and injects them
as env vars. MCP servers read API keys from env vars only — there is no
keychain fallback for API keys, and a missing credential surfaces as
`permission_denied` naming the env var.

### Restart-on-keychain-write

A keychain write or deletion through the settings UI (`write_credential` /
`delete_credential` in `crates/settings_ui/src/pages/kask_page.rs`) does **not**
by itself restart running MCP servers — their env was captured at spawn time.
After a successful keychain change, `credential_url_feeds_mcp_servers` checks
whether the URL occurs in `credential_urls_for_mcp`. Only then does
`nudge_mcp_servers(cx)` call `SettingsStore::notify_observers` directly (no
settings-file rewrite). The observer runs `sync_kask_mcp_runtime_servers`:
`build_mcp_server_env` re-reads the keychain, and affected servers restart when
their env differs. This covers data-service `kask://credentials/...` slots and
inference-provider `api_url` slots used by MCP children (for example,
OpenRouter/DeepInfra media keys). Native KiloCode chat has no MCP child-key
consumer, so rotating its provider slot does not nudge MCP servers. A failed
keychain write/delete does not trigger a restart.

### `HKASK_DB_PASSPHRASE` resolution

The SQLCipher passphrase is resolved through a canonical 2-tier helper,
`hkask_mcp_server::server::resolve_db_passphrase(&ctx.credentials)`:
`ctx.credentials.get("HKASK_DB_PASSPHRASE")` → `resolve_credential("HKASK_DB_PASSPHRASE")`
(env → hKask keychain). All five DB-passphrase-consuming servers (kata-kanban,
research, curator, corpus) use this helper, not inline
re-implementations. A miss
returns `McpToolError::permission_denied` naming the env var.

Corpus cannot read `ctx.credentials` from serde-default call sites, so it captures
the resolved passphrase at server construction into `static CORPUS_DB_PASSPHRASE:
OnceLock<Option<String>>` (`semantic/mod.rs`) via `set_corpus_db_passphrase`;
`default_corpus_passphrase()` reads the `OnceLock` first, falls back to
`resolve_credential`, giving the full 2-tier chain (creds → env → `hkask-keystore`
keychain) without changing serde-default signatures.

### First-run provisioning

`kask_bridge::identity::provision_db_passphrase` delegates to the one canonical `hkask_keystore::provision_db_passphrase_string` chain: environment override → existing `kask://credentials/hkask_db_passphrase` entry → first-run default (`kask/crates/kask_bridge/src/identity.rs:121-141`; `kask/crates/hkask-keystore/src/keychain.rs:367-429`). Governed MCP server launch calls that wrapper before building the server environment (`kask/crates/kask_bridge/src/mcp_servers.rs:536@build_mcp_server_env`). 

On first run, the DB passphrase defaults to `"allostery"`. There is ONE
passphrase for every SQLCipher database (curator, corpus,
kata-kanban, research — `kask/crates/kask_bridge/src/identity.rs:139-141`)
— no per-DB passphrases. The user can change it later
via the settings UI (Security page); the change is scheduled there and
applied by the next editor startup, before any database opens.

### Passphrase rotation

Changing a SQLCipher passphrase requires re-encrypting the entire database —
there is no in-place `PRAGMA rekey` that survives a crash. The rotation is handled by `hkask_storage::rotate_passphrase` (`kask/crates/hkask-storage/src/rotation.rs:115-302`), which:

1. Opens the source DB with the old passphrase (verifies it).
2. Creates `<db>.new` encrypted with the new passphrase.
3. Copies all user tables + `sqlite_sequence` in a single transaction.
4. Atomically renames: `<db>` → `<db>.old`, `<db>.new` → `<db>`.
   Deletes `.old` on success.

If any step fails, the original DB is untouched — the old passphrase remains
in effect. The caller writes the new passphrase to the keychain ONLY after
rotation returns `Ok(())`.

The bridge layer wraps this for the whole fleet (`kask/crates/kask_bridge/src/passphrase_rotation.rs`):

- `schedule_db_passphrase_rotation(new_passphrase, confirmed_inventory)` —
  the Security page's action: stores the new passphrase in the keychain
  pending slot (`hkask_db_passphrase_pending`) and the confirmed inventory's
  rotate paths in `maintenance/pending-db-rotation.json` (paths and the
  last error only — the passphrase never touches disk). Nothing is rotated
  at scheduling time.
- `run_pending_db_passphrase_rotation()` — called once at editor startup,
  before any database opens: each confirmed database is classified with
  `hkask_storage::verify_database_key` (old key → rotate; new key → already
  done, e.g. after a crash between rotation and the keychain write; neither
  → a surfaced manual-recovery error), the old-key set is rotated with
  rollback, and only after every database is consistent is the new
  passphrase written to the main keychain slot and the pending intent
  deleted. A failure records its error in the pending record, keeps the
  old passphrase everywhere, and retries at the next startup.

**From the settings UI:** the Security page schedules the change after a
confirmed inventory review and shows the scheduled state (with a cancel
action) until the next startup applies it
(`crates/settings_ui/src/pages/kask_page/security.rs`).


## Environment Variable Reference

All env vars can be set either via the settings UI (keychain) or via shell
environment. Shell env vars take precedence over keychain values. `KaskSettings::mcp_env` delegates to the environment builder at `kask/crates/kask_bridge/src/settings.rs:738` and `kask/crates/kask_bridge/src/mcp_env.rs`; settings defaults determine which optional values are emitted.
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
| `HKASK_EXA_API_KEY` | Exa |
| `HKASK_TAVILY_API_KEY` | Tavily |
| `HKASK_BRAVE_API_KEY` | Brave Search |
| `HKASK_SERPAPI_API_KEY` | SerpAPI |
| `HKASK_FIRECRAWL_API_KEY` | Firecrawl |
| `RUNPOD_API_KEY` | RunPod native provider (serverless OCR endpoint discovery) |

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

### Per-Server Config

| Env Var | Server | Source setting |
|---------|--------|----------------|

| `HKASK_TEMPLATE_ROOT` | corpus | `corpus.template_root` |
| `HKASK_SCENARIOS_DATA` | scenarios | derived from `data_dir` as `mcp/scenarios/` |
| `HKASK_PREDICTION_MARKETS_DATA` | prediction-markets | derived from `data_dir` as `mcp/prediction-markets/` |
| `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | prediction-markets | `prediction_markets.cache_ttl_secs` |
| `HKASK_DEFAULT_MODEL` | all | `models.default_model` |
| `HKASK_EMBEDDING_MODEL` | all | `models.embedding_model` / `corpus.embedding_model` |
| `HKASK_CLASSIFIER_MODEL` | all | `models.classifier_model` |
| `HKASK_QA_GENERATION_MODEL` | corpus | `models.qa_generation_model` (no default) |
| `HKASK_WEBID` | curator | mapped from `HKASK_CURATOR_WEBID` |
| `HKASK_CURATOR_DB` | curator | injected by deferred task |
| `HKASK_KANBAN_DB` | kata-kanban | Operator override for kanban DB path (default `mcp/kata-kanban/kanban.db`) |
| `HKASK_RESEARCH_DB` | research | Operator override for research DB path (default `mcp/research/research.db`) |

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
| `HKASK_MCP_STARTUP_TIMEOUT_SECS` | `60` | Deadline for a spawned server's handshake and tool discovery. Deliberately independent of the health-check interval (F6). |
| `HKASK_MCP_CALL_TIMEOUT_SECS` | `300` | Deadline for one already-dispatched tool call (F2/P1b). Firing reports `DispatchError::Interrupted` — a timeout proves no non-delivery, never a proven failure. |
| `HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS` | `60` | Interval between proactive MCP server health checks. |
| `HKASK_MCP_MAX_HEALTH_FAILURES` | `3` | Max consecutive health-check failures before the supervisor stops auto-healing. |
| `HKASK_MCP_MAX_READ_BYTES` | `33554432` (32 MiB) | Read size cap for `read_capped` (CWE-400). 0 is rejected (would block all reads). |
| `HKASK_MEMORY_LIFE_DAYS` | `90` | Memory retention in days (≈ a quarter). Controls decay constant in the Bayesian forgetting model. |
| `HKASK_MEMORY_FORGETTING_DAYS` | `7` | Distillation-gated hard-forgetting horizon in days (0 = disabled) — surfaced as the Forgetting Horizon field on Settings → Kask → Memory |
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


[^ousterhout-models-settings]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the two-layer default design (empty-string fields with effective_* fallback to constants) that avoids magic-number defaults.

[^owasp-keychain-settings]: OWASP. (2023). *OWASP Secrets Management Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the dual-namespace keychain design that separates settings-UI credentials from MCP-server credentials.
