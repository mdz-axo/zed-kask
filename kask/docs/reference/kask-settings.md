# Kask Settings Reference

The **Settings → Kask** section configures hKask features in zed-kask. It has
sub-pages for data services, inference providers, MCP servers, and per-server
configuration.

## Data Services

API keys for data services (EODHD, FMP, Exa, Tavily, Brave, SerpAPI, Firecrawl,
Browserbase, RunPod, Nebius, HuggingFace). Keys are stored in the system
keychain under `kask://credentials/<key>`, not in settings.json.

When MCP servers start, the composition root reads keys from the keychain and
injects them as environment variables (e.g., `HKASK_EODHD_API_KEY`,
`RUNPOD_API_KEY`) into the MCP server child process.

**To configure**: Toggle a service on, then enter the API key. The key is
written to the keychain immediately. Alternatively, set the corresponding env
var and restart Zed.

## Inference Providers

API keys for OpenAI-compatible inference providers (DeepInfra, fal.ai, Together,
OpenRouter, KiloCode, Cline). When a provider is enabled:

1. An `openai_compatible.<provider_id>` entry is written to settings.json with
   the provider's API URL and an empty `available_models` list.
2. The provider appears in **Settings → AI → LLM Providers** and in the agent
   model picker.
3. The API key is stored in the keychain under the provider's `api_url` (so
   zed's OpenAI-compatible provider finds it) and mirrored to
   `kask://credentials/<key>` (for MCP server env injection).

**To add models**: After enabling a provider, go to Settings → AI → LLM
Providers, find the provider, and add models via its configuration sub-page.

**To configure**: Toggle a provider on, then enter the API key. The key is
written to both keychain locations immediately.

## MCP Servers

Toggle which of the 10 built-in kask MCP servers are loaded. The master
"Load Default MCP Servers" toggle controls all servers; individual overrides
take precedence.

## Per-Server Configuration

Sub-pages for Curator, Guard, Memory, Condenser, Codegraph, Companies, Corpus,
Media, Scenarios, Training, and Fusion provide per-server configuration. These
settings are stored in settings.json under the `kask.*` namespace.

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
HKASK_DB_PROVIDER=sqlite kask chat

# PostgreSQL
HKASK_DB_PROVIDER=postgres \
  HKASK_DATABASE_URL=postgres://user:pass@localhost/hkask \
  kask chat
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

| Env Var | Service | Source |
|---------|--------|--------|
| `HKASK_DB_PROVIDER` | Storage backend (`sqlite` or `postgres`) | Storage |
| `HKASK_DATABASE_URL` | PostgreSQL connection URL (required when `HKASK_DB_PROVIDER=postgres`) | Storage |
| `HKASK_DB_PATH` | SQLite database path | Storage |
| `HKASK_DB_PASSPHRASE` | SQLite SQLCipher encryption passphrase | Storage |
| `HKASK_EMBEDDING_DIM` | Embedding vector dimension (default 1024) | Storage |
| `HKASK_FMP_API_KEY` | FMP | Data Services |
| `HKASK_EXA_API_KEY` | Exa | Data Services |
| `HKASK_TAVILY_API_KEY` | Tavily | Data Services |
| `HKASK_BRAVE_API_KEY` | Brave Search | Data Services |
| `HKASK_SERPAPI_API_KEY` | SerpAPI | Data Services |
| `HKASK_FIRECRAWL_API_KEY` | Firecrawl | Data Services |
| `HKASK_BROWSERBASE_API_KEY` | Browserbase | Data Services |
| `RUNPOD_API_KEY` | RunPod | Data Services |
| `RUNPOD_S3_ACCESS_KEY` | RunPod S3 | Data Services |
| `RUNPOD_S3_SECRET` | RunPod S3 | Data Services |
| `NEBIUS_PROJECT_ID` | Nebius | Data Services |
| `NEBIUS_SUBNET_ID` | Nebius | Data Services |
| `HF_TOKEN` | HuggingFace | Data Services |

| `DEEPINFRA_API_KEY` | DeepInfra | Inference Providers |
| `FALAI_API_KEY` | fal.ai | Inference Providers |
| `TOGETHERAI_API_KEY` | Together AI | Inference Providers |
| `OPENROUTER_API_KEY` | OpenRouter | Inference Providers |
| `KILOCODE_API_KEY` | KiloCode | Inference Providers |
| `CLINE_API_KEY` | Cline | Inference Providers |
