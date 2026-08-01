---
title: "Kask Settings Reference"
audience: [developers, operators, agents]
last_updated: 2026-08-01
version: "0.32.2"
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

`KaskSettings` (settings.rs:35) has 15 subsections:

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
| `swarm` | `KaskSwarmSettings` | `Default` |
| `training` | `KaskTrainingSettings` | derived `Default` |
| `fusion` | `KaskFusionSettings` | `Default` |
| `models` | `KaskModelsSettings` | derived `Default` |
| `inference_providers` | `KaskInferenceProvidersSettings` | derived `Default` (all false) |

## MCP Servers (`KaskMcpSettings`)

Toggle which of the 11 built-in kask MCP servers are loaded.

| Field | Type | Default |
|-------|------|---------|
| `load_default` | `bool` | `true` — load all 11 servers |
| `overrides` | `HashMap<String, bool>` | empty — per-server overrides (e.g. `"curator": false`) |

The master `load_default` toggle controls all servers; individual `overrides`
take precedence. Set `load_default: false` to disable all kask MCP servers.

## Data Services (`KaskDataServiceSettings`)

API key toggles for data services. Keys are stored in the system keychain under
`kask://credentials/<key>`, not in settings.json. When MCP servers start, the
composition root reads keys from the keychain and injects them as environment
variables into the MCP server child process.

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
enabled:

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
| `fal_enabled` | `false` | `FALAI_API_KEY` set |
| `together_enabled` | `false` | `TOGETHERAI_API_KEY` set |
| `openrouter_enabled` | `false` | `OPENROUTER_API_KEY` set |
| `kilocode_enabled` | `false` | `KILOCODE_API_KEY` set |
| `cline_enabled` | `false` | `CLINE_API_KEY` set |

`Default` returns all-false (pure, no side effects). The env-var-based
auto-enable logic lives in `From<KaskInferenceProvidersSettingsContent>` and
`KaskInferenceProvidersSettings::from_env()`, which auto-enable a provider
when its API key env var is set and the user hasn't explicitly toggled it.

**To add models**: After enabling a provider, go to Settings → AI → LLM
Providers, find the provider, and add models via its configuration sub-page.

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

There is no `KaskGuardSettings` struct. Direct chat is unguarded (provider-side safety + refusal fallback); the guard only wraps the skill cascade path. There is no configurable `direct_chat_strategy` — the `cascade_only` behavior is hardcoded. See DIVERGENCE.md D4.

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
| `auto_compress_tool_results` | `bool` | `true` | Compress tool results before message history |
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
| `tts_model` | `String` | `""` | TTS model override (e.g., `"fal.ai/qwen-3-tts"`) |
| `stt_model` | `String` | `""` | STT model override (e.g., `"fal.ai/wizper"`) |
| `vision_model` | `String` | `""` | Vision model override |
| `image_gen_model` | `String` | `""` | Image generation model override (e.g., `"fal.ai/flux-2"`) |

## Scenarios (`KaskScenariosSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `data_dir` | `String` | `""` | Scenario persistence directory; empty = in-memory |

## Swarm (`KaskSwarmSettings`)

Agent Bestiary World (ABW) swarm integration (added 2026-08-01). See `plans/abw-swarm-intelligence.md`.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `mode` | `SwarmModeConfig` | `"abw"` | `"abw"` (Agent Bestiary World, v1) or `"local"` (local substrate crates, v2 §15) |
| `api_url` | `String` | `""` | ABW API base URL override; empty = `https://agent-bestiary.world` |
| `max_credits_per_dispatch` | `u32` | `50` | Per-dispatch credit ceiling (S3 budget gate); dispatches above this are refused pre-spend |
| `curator_consent_default` | `bool` | `false` | When `false`, `swarm_xaman` requires a per-call `consent_token`; `true` = operator globally opted in |
| `local_agents_dir` | `String` | `""` | Directory for local agent cards (`<id>/agent_card.json`) in `local` mode; empty = `agents/local/curated` |

## Training (`KaskTrainingSettings`)

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `host` | `String` | `""` | `"deepinfra"`, `"nebius"`, or `"runpod"`; empty = auto-detect from API keys |
| `cache_dir` | `String` | `""` | Dataset pipeline cache; empty = agent adapters directory |

## Fusion (`KaskFusionSettings`)

Multi-model fusion inference configuration. Mirrors `hkask_types::FusionConfig`
but lives in the non-secret settings layer so users can edit it in the settings
UI. Two-layer default design (intentional): `judge_model` and `panel_models`
default to empty strings in `Default`. When empty, `to_fusion_config()` falls
back to `FusionConfig::kask_default()`, which reads env vars and falls back to
hardcoded model names.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `enabled` | `bool` | `false` | Master toggle |
| `judge_model` | `String` | `""` | Empty = `FusionConfig::kask_default()` |
| `panel_models` | `String` | `""` | Comma-separated; empty = kask default |
| `mode` | `String` | `"synthesis"` | `synthesis`/`best-of-n`/`critique`/`deliberation`/`pi`/`algo` |
| `algo_method` | `String` | `"merge"` | `merge`/`vote` (only when `mode == "algo"`) |
| `skills` | `String` | `""` | Comma-separated skill anchors (e.g., `"pragmatic-semantics,coding-guidelines"`) |
| `max_rounds` | `u32` | `5` | Max rounds for deliberation mode |
| `discovery_max_price` | `f64` | `2.0` | Max input price ($/M tokens) for AA discovery |
| `discovery_min_intelligence` | `f64` | `40.0` | Min AA Intelligence Index for discovery |
| `coherence_threshold` | `Option<f64>` | `None` | Not yet implemented — setting has no effect |
| `panel_sizing_enabled` | `bool` | `false` | Query-complexity-based panel sizing |
| `pressure_adaptive_enabled` | `bool` | `false` | Substrate-aware degradation under latency pressure |

## Models (`KaskModelsSettings`)

Kask-wide model configuration. Two-layer default design: fields default to
empty strings; `effective_*` methods fall back to the `DEFAULT_*_MODEL`
constants.

| Field | Type | Default | Effective fallback |
|-------|------|---------|-------------------|
| `default_model` | `String` | `""` | `DEFAULT_INFERENCE_MODEL` = `"openrouter/z-ai/glm-5.2"` |
| `embedding_model` | `String` | `""` | `DEFAULT_EMBEDDING_MODEL` = `"openrouter/z-ai/glm-5.2"` |
| `classifier_model` | `String` | `""` | `DEFAULT_CLASSIFIER_MODEL` = `"openrouter/z-ai/glm-5.2"` |

## Keychain Architecture

There are two keychain namespaces:

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
either provider without code changes.

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
environment. Shell env vars take precedence over keychain values.

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

### Inference Providers

| Env Var | Service |
|---------|--------|
| `DEEPINFRA_API_KEY` | DeepInfra |
| `FALAI_API_KEY` | fal.ai |
| `TOGETHERAI_API_KEY` | Together AI |
| `OPENROUTER_API_KEY` | OpenRouter |
| `KILOCODE_API_KEY` | KiloCode |
| `CLINE_API_KEY` | Cline |

### Fusion

| Env Var | Service | Default |
|---------|--------|---------|
| `HKASK_FUSION_JUDGE_MODEL` | Fusion judge model | `OpenRouter/z-ai/glm-5.2` |
| `HKASK_FUSION_PANEL_MODELS` | Comma-separated panel models | `OpenRouter/z-ai/glm-5.2,OpenRouter/qwen/qwen3-235b-a22b,OpenRouter/minimax/minimax3` |
| `HKASK_FUSION_MODE` | Deliberation mode | `synthesis` |
| `HKASK_FUSION_SKILLS` | Skill anchors | none |
| `HKASK_FUSION_MAX_ROUNDS` | Max rounds (deliberation) | `5` |
| `HKASK_FUSION_ALGO_METHOD` | Algo merge strategy | `merge` |
| `HKASK_FUSION_DISABLED` | Force-disable (`1`) | unset |

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
| `HKASK_TRAINING_HOST` | training | `training.host` |
| `HKASK_TRAINING_CACHE_DIR` | training | `training.cache_dir` |
| `HKASK_DEFAULT_MODEL` | all | `models.default_model` |
| `HKASK_EMBEDDING_MODEL` | all | `models.embedding_model` / `corpus.embedding_model` |
| `HKASK_CLASSIFIER_MODEL` | all | `models.classifier_model` |
| `HKASK_WEBID` | curator | mapped from `HKASK_CURATOR_WEBID` |
| `HKASK_CURATOR_DB` | curator | injected by deferred task |
