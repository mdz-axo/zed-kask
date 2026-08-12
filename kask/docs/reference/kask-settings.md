---
title: "Kask Settings Reference"
audience: [developers, operators, agents]
last_updated: 2026-08-05
version: "0.32.3"
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

`KaskSettings` (settings.rs:36-85) has 16 subsections:

| Field | Type | Default source |
|-------|------|---------------|
| `mcp` | `KaskMcpSettings` | `Default` |
| `data_services` | `KaskDataServiceSettings` | derived `Default` (all false) |
| `curator` | `KaskCuratorSettings` | `Default` |
| `memory` | `KaskMemorySettings` | `Default` |
| `condenser` | `KaskCondenserSettings` | `Default` |
| `codegraph` | `KaskCodegraphSettings` | derived `Default` |
| `companies` | `KaskCompaniesSettings` | derived `Default` |
| `corpus` | `KaskCorpusSettings` | `Default` |
| `media` | `KaskMediaSettings` | derived `Default` |
| `scenarios` | `KaskScenariosSettings` | derived `Default` |
| `prediction_markets` | `KaskPredictionMarketsSettings` | derived `Default` |
| `swarm` | `KaskSwarmSettings` | `Default` |
| `training` | `KaskTrainingSettings` | derived `Default` |
| `models` | `KaskModelsSettings` | derived `Default` |
| `inference_providers` | `KaskInferenceProvidersSettings` | derived `Default` (all false) |
| `collab` | `KaskCollabSettings` | `Default` (enabled, localhost:3000, sqlite) |

## MCP Servers (`KaskMcpSettings`)

Toggle which of the 12 built-in kask MCP servers are loaded.[^mcp-spec-settings]
The 12 servers (settings.rs:37 comment still says 11 — the `Default` and the
`BUILT_IN_MCP_SERVERS_IDS` constant in `kask/crates/kask_bridge/src/mcp_servers.rs:323-336`
are authoritative): `codegraph`, `companies`, `condenser`, `corpus`, `curator`,
`kata-kanban`, `media`, `research`, `scenarios`, `prediction-markets`, `swarm`,
`training`. The crates live under `kask/mcp-servers/` (12 `hkask-mcp-*` crates).

| Field | Type | Default |
|-------|------|---------|
| `load_default` | `bool` | `true` — load all 12 servers |
| `overrides` | `HashMap<String, bool>` | empty — per-server overrides (e.g. `"curator": false`) |

The master `load_default` toggle controls all servers; individual `overrides`
take precedence. Set `load_default: false` to disable all kask MCP servers.

## Data Services (`KaskDataServiceSettings`)

API key toggles for data services. Keys are stored in the system keychain under
`kask://credentials/<key>`, not in settings.json. When MCP servers start, the
composition root reads keys from the keychain and injects them as environment
variables into the MCP server child process.[^owasp-secrets-settings]

| Field | Default | Keychain key | Env var injected |
|-------|---------|--------------|-------------------|
| `eodhd_enabled` | `false` | `kask://credentials/hkask_eodhd_api_key` | `HKASK_EODHD_API_KEY` |
| `fmp_enabled` | `false` | `kask://credentials/hkask_fmp_api_key` | `HKASK_FMP_API_KEY` |
| `exa_enabled` | `false` | `kask://credentials/hkask_exa_api_key` | `HKASK_EXA_API_KEY` |
| `tavily_enabled` | `false` | `kask://credentials/hkask_tavily_api_key` | `HKASK_TAVILY_API_KEY` |
| `brave_enabled` | `false` | `kask://credentials/hkask_brave_api_key` | `HKASK_BRAVE_API_KEY` |
| `runpod_enabled` | `false` | `kask://credentials/runpod_api_key` | `RUNPOD_API_KEY` |
| `nebius_enabled` | `false` | `kask://credentials/nebius_project_id` | `NEBIUS_PROJECT_ID` |

**To configure**: Toggle a service on, then enter the API key. The key is
written to the keychain immediately. Alternatively, set the corresponding env
var and restart Zed.

## Inference Providers (`KaskInferenceProvidersSettings`)

API key toggles for OpenAI-compatible inference providers. When a provider is
enabled:[^openai-compatible-settings]

1. An `openai_compatible.<provider_id>` entry is written to settings.json with
   the provider's API URL and an empty `available_models` list.
2. The provider appears in **Settings → AI → LLM Providers** and in the agent
   model picker.
3. The API key is stored in the keychain under the provider's `api_url` (so
   zed's OpenAI-compatible provider finds it) and mirrored to
   `kask://credentials/<key>` (for MCP server env injection).

| Field | Default | Env var auto-enable check |
|-------|---------|---------------------------|
| `deepinfra_enabled` | `false` | `DEEPINFRA_API_KEY` set |
| `openrouter_enabled` | `false` | `OPENROUTER_API_KEY` set |
| `atlascloud_enabled` | `false` | `ATLASCLOUD_API_KEY` set |

fal.ai is not an inference provider here — it is not OpenAI-compatible
(`/v1/chat/completions` returns 404; `/v1/models` uses `Authorization: Key`).
Its `FALAI_API_KEY` is managed as a data-service credential (see Data
Services) and consumed by the media and corpus MCP servers. Cline was removed
from the kask provider set.

`Default` returns all-false (pure, no side effects). The env-var-based
auto-enable logic lives in `From<KaskInferenceProvidersSettingsContent>` and
`KaskInferenceProvidersSettings::from_env()` (settings.rs:175-184), which
auto-enable a provider when its API key env var is set and the user hasn't
explicitly toggled it.

**To add models**: After enabling a provider, go to Settings → AI → LLM
Providers, find the provider, and add models via its configuration sub-page.

## Collab (`KaskCollabSettings`)

Local collab server configuration (settings.rs:199-227). When `enabled` is
true, zed-kask launches a local `collab serve api` process at startup so the
kask extensions panel can fetch `/api/kask-skills` without depending on the
deployed `zed.dev` server. The server uses SQLite (no Postgres/S3 needed)
for local dev; S3 is only required for publish/download/vote.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `enabled` | `bool` | `true` | Auto-launch the local collab server at startup |
| `database_url` | `String` | `"sqlite:kask_marketplace.db?mode=rwc"` | SQLite connection string |
| `http_port` | `u16` | `3000` | HTTP port the collab server listens on |
| `zed_environment` | `String` | `"development"` | `development`, `staging`, or `production` |
| `marketplace_url` | `String` | `"http://localhost:3000"` | Marketplace base URL the extensions panel uses; when set and non-empty, overrides `server_url`-based resolution |

## Curator (`KaskCuratorSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `always_on` | `bool` | `true` | Curator agent runs regulation loops in background |
| `algedonic_threshold` | `f64` | `0.8` | Algedonic signal threshold (0.0–1.0) |
| `email` | `KaskCuratorEmailSettings` | `Default` | Outbound algedonic alerts via MXroute |

### Curator Email (`KaskCuratorEmailSettings`)

Non-secret fields. The SMTP password is stored in the OS keychain under
`kask://credentials/hkask_smtp_password`, not here. The composition root reads
it from the keychain and injects it as `HKASK_SMTP_PASSWORD` into MCP server
child processes.

| Field | Type | Default | Env var injected |
|-------|------|---------|-------------------|
| `mxroute_server` | `String` | `""` | `HKASK_MXROUTE_SERVER` |
| `smtp_username` | `String` | `""` | `HKASK_SMTP_USERNAME` |
| `curator_email` | `String` | `""` | `HKASK_CURATOR_EMAIL` (defaults to `HKASK_SMTP_USERNAME`) |
| `alert_email` | `String` | `""` | `HKASK_ALERT_EMAIL` (defaults to `HKASK_SMTP_USERNAME`) |
| `authorized_emails` | `Vec<String>` | `[]` | `HKASK_AUTHORIZED_EMAILS` (comma-joined) |
| `inbox_poll_interval_secs` | `u64` | `0` | `HKASK_INBOX_POLL_INTERVAL_SECS` (0 = disabled; reserved for future IMAP) |
| `digest_interval_secs` | `u64` | `0` | `HKASK_DIGEST_INTERVAL_SECS` (0 = disabled; reserved for future digest) |

When `email` is `None` or unconfigured, the alert email sink falls back to the
log-only sink (`LogAlertEmailSink` in `crates/zed/src/main.rs`).

## Guard

There is no `KaskGuardSettings` struct. Direct chat is unguarded (provider-side safety + refusal fallback); the guard only wraps the skill cascade path. There is no configurable `direct_chat_strategy` — the `cascade_only` behavior is hardcoded. See DIVERGENCE.md D4.[^owasp-llm-guard-settings]

## Memory (`KaskMemorySettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `consolidation_cadence_secs` | `u64` | `300` | 0 = disabled |
| `confidence_floor` | `f64` | `0.3` | Memory retention floor (0.0–1.0) |
| `recall_limit` | `u32` | `5` | Max snippets retrieved for context injection |
| `recall_min_confidence` | `f64` | `0.3` | Min confidence for injection (0.0–1.0) |
| `auto_inject` | `bool` | `true` | Auto-inject recalled memories into prompts |

## Condenser (`KaskCondenserSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `profile` | `String` | `"normal"` | `"heavy"` (10% retention, 30 max lines), `"normal"` (20%, 80), `"soft"` (60%, 200), `"light"` (95%, no max) |
| `auto_compress_tool_results` | `bool` | `false` | Compress tool results before message history |
| `persona_keywords` | `Vec<String>` | `[]` | Saliency scoring keywords |
| `saliency_window` | `u32` | `5` | Max tokens budget: `saliency_window * 100`, clamped [150, 2000] |

## Codegraph (`KaskCodegraphSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `db_path` | `String` | `""` | Database path; empty = in-memory |

## Companies (`KaskCompaniesSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `chronic_staleness_days` | `u32` | `0` | 0 = use hardcoded default (90); >0 = override |
| `fermi_defaults` | `String` | `""` | JSON with `growth` + `margin` arrays; empty = hardcoded defaults |
| `transactions_dir` | `String` | `""` | Portfolio transaction files; empty = `<kask_data_dir>/transactions/` |

## Corpus (`KaskCorpusSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `embedding_dim` | `u32` | `1024` | Must match embedding model output |
| `embedding_model` | `String` | `default_embedding_model()` | Defaults to `hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL` |
| `ocr_concurrency` | `u32` | `4` | Pages sent to vision model in parallel |
| `ocr_simple_max` | `f64` | `0.05` | Pages below this processed simply |
| `ocr_moderate_max` | `f64` | `0.15` | Pages above simple but below this = moderate pipeline |
| `ocr_sample_rate` | `f64` | `0.10` | Fraction of moderate pages sampled |
| `ocr_tuneable` | `bool` | `true` | OCR tuneable mode enabled |
| `template_root` | `String` | `"registry"` | Jinja2 template root directory |

## Media (`KaskMediaSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `tts_model` | `String` | `""` | TTS model override (e.g., `"DeepInfra/hexgrad/Kokoro-82M"`) |
| `stt_model` | `String` | `""` | STT model override (e.g., `"DeepInfra/whisper-large-v3"`) |
| `vision_model` | `String` | `""` | Vision model override |
| `image_gen_model` | `String` | `""` | Image generation model override (e.g., `"DeepInfra/black-forest-labs/FLUX-2-klein-4b"`) |

## Scenarios (`KaskScenariosSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `data_dir` | `String` | `""` | Scenario persistence directory; empty = in-memory |

## Prediction Markets (`KaskPredictionMarketsSettings`)

Prediction-markets data-service configuration (settings.rs:451-458).

| Field | Type | Default | Env var injected | Notes |
|-------|------|---------|-------------------|-------|
| `data_dir` | `String` | `""` | `HKASK_PREDICTION_MARKETS_DATA` | Calibration journal directory; empty = in-memory |
| `cache_ttl_secs` | `u64` | `0` | `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | Market-data cache TTL; 0 = server default |
| `base_events` | `String` | `""` | `HKASK_PREDICTION_MARKETS_BASE_EVENTS` | Base-event registry: `"domain:series,..."` pairs for CMP construction |

## Swarm (`KaskSwarmSettings`)

Agent Bestiary World (ABW) swarm integration (added 2026-08-01). See `plans/abw-swarm-intelligence.md`.[^reynolds-swarm-settings]

| Field | Type | Default | Env var injected | Notes |
|-------|------|---------|-------------------|-------|
| `mode` | `SwarmModeConfig` | `"abw"` | `HKASK_SWARM_MODE` | `"abw"` (Agent Bestiary World, v1) or `"local"` (local substrate crates, v2 §15) |
| `api_url` | `String` | `""` | `HKASK_ABW_API_URL` | ABW API base URL override; empty = `https://agent-bestiary.world` |
| `max_credits_per_dispatch` | `u32` | `50` | `HKASK_ABW_MAX_CREDITS` | Per-dispatch credit ceiling (S3 budget gate); dispatches above this are refused pre-spend |
| `curator_consent_default` | `bool` | `false` | `HKASK_ABW_CURATOR_CONSENT_DEFAULT` | When `false`, `swarm_xaman` requires a per-call `consent_token`; `true` = operator globally opted in |
| `local_agents_dir` | `String` | `""` | `HKASK_LOCAL_AGENTS_DIR` | Directory for local agent cards (`<id>/agent_card.json`) in `local` mode; empty = `agents/local/curated` |
| `local_swarms_dir` | `String` | `""` | `HKASK_LOCAL_SWARMS_DIR` | Directory for local swarms (`<id>/swarm.json`), read/written by `LocalSwarmRegistry`; empty = `agents/local/swarms` |

The ABW API key is a secret — it lives in the keychain under
`kask://credentials/hkask_abw_api_key`, injected as `HKASK_ABW_API_KEY` by
`mcp_env_with_credentials`, not by `mcp_env()`. The bridge `Default` impl
(settings.rs:536-557) MUST stay in sync with `SwarmConfig::default()` in
`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs` — the two impls are
deliberately duplicated across the crate boundary to avoid a circular
dependency.

## Training (`KaskTrainingSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `host` | `String` | `""` | `"deepinfra"`, `"nebius"`, or `"runpod"`; empty = auto-detect from API keys |
| `cache_dir` | `String` | `""` | Dataset pipeline cache; empty = agent adapters directory |

## Models (`KaskModelsSettings`)

Kask-wide model configuration. Two-layer default design: fields default to
empty strings; `effective_*` methods fall back to the `DEFAULT_*_MODEL`
constants, which are `const` references to the single source of truth in
`hkask_inference::model_constants`.[^ousterhout-models-settings]

| Field | Type | Default | Effective fallback |
|-------|------|---------|-------------------|
| `default_model` | `String` | `""` | `DEFAULT_INFERENCE_MODEL` = `DEFAULT_FALLBACK_MODEL` = `"OpenRouter/z-ai/glm-5.2"` (model_constants.rs:35) |
| `embedding_model` | `String` | `""` | `DEFAULT_EMBEDDING_MODEL` = `"DeepInfra/Qwen/Qwen3-Embedding-0.6B"` (model_constants.rs:26) |
| `classifier_model` | `String` | `""` | `DEFAULT_CLASSIFIER_MODEL` = `"OpenRouter/deepseek/deepseek-v4-flash"` (model_constants.rs:23) |

`model_constants.rs` also defines `DEFAULT_OCR_MODEL` (`"RunPod/kask-ocr"`),
env `HKASK_OCR_MODEL`), `DEFAULT_TTS_MODEL` (`"DeepInfra/hexgrad/Kokoro-82M"`),
`DEFAULT_STT_MODEL` (`"DeepInfra/whisper-large-v3"`), `DEFAULT_VISION_MODEL`
(`"OpenRouter/Qwen/Qwen3-VL-235B-A22B-Instruct"`), and `DEFAULT_IMAGE_GEN_MODEL`
(`"DeepInfra/black-forest-labs/FLUX-2-klein-4b"`). Every constant has an env-var accessor (e.g.
`classifier_model()` reads `HKASK_CLASSIFIER_MODEL` first) so operators can
override without recompiling.

## Keychain Architecture

There are two keychain namespaces:[^owasp-keychain-settings]

1. **Zed's `CredentialsProvider`** — used by the settings UI. Keys are stored
   under `kask://credentials/<key>` (for data services) or the provider's
   `api_url` (for inference providers).
2. **hKask's `Keychain`** (service "hkask") — used by MCP servers via
   `resolve_credential` → `Keychain::retrieve_by_key(env_var)`.

The composition root bridges these namespaces: at MCP server startup, it reads
keys from zed's keychain and injects them as env vars into the child process.
MCP servers then find the keys via `std::env::var` or their own keychain
fallback.

## Storage Backend

hKask supports two storage backends, selected at startup via environment
variables. The `DatabaseDriver` trait abstracts the backend so all stores
(consent, goals, embeddings, wallet, kata, regulation, etc.) work with
either provider without code changes.[^sqlcipher-settings][^pgvector-settings]

### SQLite (default)

Per-agent SQLCipher-encrypted databases at `~/.local/share/hkask/agents/{name}/`.
Zero configuration — the default for local, single-user deployments. Uses
`sqlite-vec` for vector similarity search.

### PostgreSQL

Connects to a PostgreSQL database with `pgvector` for vector similarity
search. Use when memory or embedding collections outgrow SQLite's
single-writer model, or for multi-user / remote deployments.

```bash
# SQLite (default)
HKASK_DB_PROVIDER=sqlite

# PostgreSQL
HKASK_DB_PROVIDER=postgres \
  HKASK_DATABASE_URL=postgres://user:pass@localhost/hkask
```

The `PostgresDriver` uses a dedicated worker thread to bridge async `sqlx`
to the sync `DatabaseDriver` trait — safe from any calling context
including the GPUI foreground thread. Encryption at rest is the operator's
responsibility (TLS to a remote Postgres + disk encryption).

### `ServiceConfig::open_driver()`

The canonical entry point for driver construction. Dispatches on
`db_provider`:

- `Sqlite` → opens a SQLCipher database at `db_path` with `db_passphrase`.
- `Postgres` → connects to `HKASK_DATABASE_URL` and initializes the
  pgvector schema (`schema_pg.sql`).

Returns `Arc<dyn DatabaseDriver>` ready for any store's `from_driver()`
constructor.

## Environment Variable Reference

All env vars can be set either via the settings UI (keychain) or via shell
environment. Shell env vars take precedence over keychain values. The
`mcp_env()` method (settings.rs:668-970) translates settings into env vars for
MCP server child processes; only non-empty/non-default values are emitted.
`mcp_env()` also unconditionally injects `HKASK_MCP_SERVER_IDS` (the
comma-joined `BUILT_IN_MCP_SERVERS_IDS`, consumed only by the swarm server's
`config_env` allowlist) and passes through `HKASK_DATA_DIR` and
`HKASK_CURATOR_WEBID` → `HKASK_WEBID` when set in the parent environment.

### Storage

| Env Var | Service | Notes |
|---------|--------|-------|
| `HKASK_DB_PROVIDER` | Storage backend | `sqlite` (default) or `postgres` |
| `HKASK_DATABASE_URL` | PostgreSQL URL | Required when `HKASK_DB_PROVIDER=postgres` |
| `HKASK_DB_PATH` | SQLite path | |
| `HKASK_DB_PASSPHRASE` | SQLite passphrase | SQLCipher encryption |
| `HKASK_EMBEDDING_DIM` | Embedding dimension | Default 1024 (from `KaskCorpusSettings::default()`) |
| `HKASK_DATA_DIR` | hKask data directory | Passed to all MCP servers for path resolution |

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
| `HKASK_BROWSERBASE_API_KEY` | Browserbase |
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
| `DEEPINFRA_API_KEY` | DeepInfra |
| `OPENROUTER_API_KEY` | OpenRouter |
| `ATLASCLOUD_API_KEY` | AtlasCloud |

`FALAI_API_KEY` is listed under Data Services (fal.ai is a media platform,
not an OpenAI-compatible chat endpoint).

### Curator Email

| Env Var | Service |
|---------|--------|
| `HKASK_MXROUTE_SERVER` | MXroute server hostname |
| `HKASK_SMTP_USERNAME` | SMTP auth + From header |
| `HKASK_SMTP_PASSWORD` | SMTP password (keychain only) |
| `HKASK_CURATOR_EMAIL` | From address (defaults to `HKASK_SMTP_USERNAME`) |
| `HKASK_ALERT_EMAIL` | Alert recipient (defaults to `HKASK_SMTP_USERNAME`) |
| `HKASK_AUTHORIZED_EMAILS` | Authorized sender allowlist (comma-separated) |
| `HKASK_INBOX_POLL_INTERVAL_SECS` | Inbox poll interval (0 = disabled) |
| `HKASK_DIGEST_INTERVAL_SECS` | Digest interval (0 = disabled) |

### Per-Server Config

| Env Var | Server | Source setting |
|---------|--------|----------------|
| `HKASK_CODEGRAPH_DB` | codegraph | `codegraph.db_path` |
| `HKASK_CHRONIC_STALENESS_DAYS` | companies | `companies.chronic_staleness_days` |
| `HKASK_FERMI_DEFAULTS` | companies | `companies.fermi_defaults` |
| `HKASK_TRANSACTIONS_DIR` | companies | `companies.transactions_dir` |
| `HKASK_CONDENSER_PERSONA_KEYWORDS` | condenser | `condenser.persona_keywords` |
| `HKASK_CONDENSE_SALIENCY_WINDOW` | condenser | `condenser.saliency_window` |
| `HKASK_OCR_CONCURRENCY` | corpus | `corpus.ocr_concurrency` |
| `HKASK_OCR_SIMPLE_MAX` | corpus | `corpus.ocr_simple_max` |
| `HKASK_OCR_MODERATE_MAX` | corpus | `corpus.ocr_moderate_max` |
| `HKASK_OCR_SAMPLE_RATE` | corpus | `corpus.ocr_sample_rate` |
| `HKASK_OCR_TUNEABLE` | corpus | `corpus.ocr_tuneable` |
| `HKASK_TEMPLATE_ROOT` | corpus | `corpus.template_root` |
| `HKASK_MEDIA_TTS_MODEL` | media | `media.tts_model` |
| `HKASK_MEDIA_STT_MODEL` | media | `media.stt_model` |
| `HKASK_MEDIA_VISION_MODEL` | media | `media.vision_model` |
| `HKASK_MEDIA_IMAGE_GEN_MODEL` | media | `media.image_gen_model` |
| `HKASK_SCENARIOS_DATA` | scenarios | `scenarios.data_dir` |
| `HKASK_PREDICTION_MARKETS_DATA` | prediction-markets | `prediction_markets.data_dir` |
| `HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS` | prediction-markets | `prediction_markets.cache_ttl_secs` |
| `HKASK_PREDICTION_MARKETS_BASE_EVENTS` | prediction-markets | `prediction_markets.base_events` |
| `HKASK_SWARM_MODE` | swarm | `swarm.mode` |
| `HKASK_ABW_API_URL` | swarm | `swarm.api_url` |
| `HKASK_ABW_MAX_CREDITS` | swarm | `swarm.max_credits_per_dispatch` |
| `HKASK_ABW_CURATOR_CONSENT_DEFAULT` | swarm | `swarm.curator_consent_default` |
| `HKASK_LOCAL_AGENTS_DIR` | swarm | `swarm.local_agents_dir` |
| `HKASK_LOCAL_SWARMS_DIR` | swarm | `swarm.local_swarms_dir` |
| `HKASK_ABW_API_KEY` | swarm | ABW API key (keychain only) |
| `HKASK_TRAINING_HOST` | training | `training.host` |
| `HKASK_TRAINING_CACHE_DIR` | training | `training.cache_dir` |
| `HKASK_DEFAULT_MODEL` | all | `models.default_model` |
| `HKASK_EMBEDDING_MODEL` | all | `models.embedding_model` / `corpus.embedding_model` |
| `HKASK_CLASSIFIER_MODEL` | all | `models.classifier_model` |
| `HKASK_WEBID` | curator | mapped from `HKASK_CURATOR_WEBID` |
| `HKASK_MCP_SERVER_IDS` | swarm | `BUILT_IN_MCP_SERVERS_IDS` joined (unconditional) |
| `HKASK_CURATOR_DB` | curator | injected by deferred task |

The collab server is launched directly by zed-kask (not via `mcp_env()`), so
there are no `collab.*`-derived env vars.

## Footnotes

[^mcp-spec-settings]: Anthropic. (2024). *Model Context Protocol Specification*. Anthropic PBC. https://modelcontextprotocol.io/specification
    Cited for the MCP server model the `load_default`/`overrides` toggle controls.

[^owasp-secrets-settings]: OWASP. (2023). *OWASP Secrets Management Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the secrets-in-keychain-not-in-config principle the data services settings follow.

[^openai-compatible-settings]: OpenAI. (2024). *OpenAI API Reference — Models*. OpenAI. https://platform.openai.com/docs/api-reference/models
    Cited for the OpenAI-compatible provider model that the inference provider toggles configure.

[^owasp-llm-guard-settings]: OWASP. (2025). *OWASP Top 10 for Large Language Model Applications*. OWASP Foundation. https://owasp.org/www-project-top-10-for-large-language-model-applications/
    Cited for the LLM-specific security model the guard layer (D4) wraps the skill cascade with.

[^reynolds-swarm-settings]: Reynolds, C. W. (1987). Flocks, herds and schools: A distributed behavioral model. *ACM SIGGRAPH Computer Graphics*, 21(4), 25–34. https://doi.org/10.1145/37402.37406
    Cited for the swarm-coordination model the ABW swarm settings configure.

[^ousterhout-models-settings]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yakny Press.
    Cited for the two-layer default design (empty-string fields with effective_* fallback to constants) that avoids magic-number defaults.

[^owasp-keychain-settings]: OWASP. (2023). *OWASP Secrets Management Cheat Sheet*. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the dual-namespace keychain design that separates settings-UI credentials from MCP-server credentials.

[^sqlcipher-settings]: Zetetic LLC. (2024). *SQLCipher: Full Database Encryption for SQLite*. https://www.zetetic.net/sqlcipher/
    Cited for the SQLCipher-encrypted SQLite backend the default storage uses.

[^pgvector-settings]: pgvector. (2024). *pgvector: Open-source vector similarity search for PostgreSQL*. GitHub. https://github.com/pgvector/pgvector
    Cited for the pgvector extension the PostgreSQL storage backend uses for vector similarity search.
