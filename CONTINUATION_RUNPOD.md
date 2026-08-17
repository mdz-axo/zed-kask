# Continuation: Wire RunPod Inference Backend into zed-kask

## Project

**zed-kask** — a fork of Zed (NOT Zed itself). Binary: `zed-kask`, config dir: `~/.config/zed-kask/`, app name: `Zed-Kask`. `DIVERGENCE.md` tracks every deviation from upstream Zed. Code in `kask/` is fork-owned; code in `crates/` is upstream Zed and needs D-seam entries + tests for any modifications.

## What's been done so far

### Blocker 1: Self-healing MCP — ✅ DONE, compiles and committed

The self-healing MCP code builds cleanly. Commits:
- `4f468148ae` — `watch_transport_shutdown` in `context_server_store.rs`
- `aba2ccfb9c` — `ContextServerTool::run` on-demand restart
- `6a8e8753d0` — transport-death retry for MCP tool calls + `trigger_server_maintenance` public method
- `91795702d8` — early-return when context server was never started (the only uncommitted change, now committed)

**No further work needed on Blocker 1.**

### Blocker 2: RunPod inference backend — PARTIALLY STARTED, needs completion

The architecture decision is made: **Option A — register RunPod as a dedicated Zed `LanguageModelProvider`** (like DeepSeek, Google, etc.), NOT via the `openai_compatible` settings map.

**Why Option A:** The IPC bridge path (`InferenceIpcClient` → `LanguageModelInferencePort` → `LanguageModelRegistry`) is the *only* path that works for the corpus server when launched by zed-kask. The `MediaRouter` fallback can't handle vision/OCR. So RunPod must be a registered provider in the `LanguageModelRegistry`.

**Key difference from standard OpenAI-compatible providers:** RunPod serverless endpoints each have their own API URL (`https://api.runpod.io/v2/{endpoint_id}/openai/v1`). The standard `OpenAiCompatibleLanguageModel` reads a single `api_url` from provider state. So RunPod needs a dedicated provider where each model carries its own endpoint URL.

## RunPod endpoint discovery

The RunPod `kask-ocr` endpoint exists:
- Endpoint ID: `hsldzov6932wf5`
- Name: `kask-ocr`
- Type: `QB` (Queue-Based serverless endpoint)
- Template ID: `53ganjs1i2`
- Model: `allenai/olmOCR-2-7B-1025` (vLLM with `RAW_OPENAI_OUTPUT=true`)
- API key env var: `RUNPOD_API_KEY` (already set in env)

**Discovery via RunPod GraphQL API:**
```bash
curl -s -H "Authorization: Bearer $RUNPOD_API_KEY" \
  "https://api.runpod.io/graphql" -X POST -H "Content-Type: application/json" \
  -d '{"query":"query { myself { endpoints { id name type env { key value } } } }"}'
```

Returns:
```json
{
  "data": {
    "myself": {
      "endpoints": [
        {
          "id": "hsldzov6932wf5",
          "name": "kask-ocr",
          "type": "QB",
          "env": [
            {"key": "MODEL_NAME", "value": "allenai/olmOCR-2-7B-1025"},
            {"key": "RAW_OPENAI_OUTPUT", "value": "true"},
            ...
          ]
        }
      ]
    }
  }
}
```

Each endpoint's OpenAI-compatible API URL is: `https://api.runpod.io/v2/{endpoint_id}/openai/v1`

**IMPORTANT:** The endpoint currently has zero active workers and returns 404 on all REST paths. This is a deployment issue — the user needs to deploy the endpoint via the RunPod console. The code should be written to work once the endpoint is deployed. The discovery should still list the endpoint even if it's not deployed (the GraphQL API returns endpoint metadata regardless of worker status).

## Changes already made (in working tree, uncommitted)

### 1. `crates/settings_content/src/language_model.rs` — ✅ DONE

Added `RunpodSettingsContent` and `RunpodAvailableModel` structs after `DeepseekAvailableModel`. Added `pub runpod: Option<RunpodSettingsContent>` to `AllLanguageModelSettingsContent`.

The `RunpodSettingsContent` struct:
```rust
pub struct RunpodSettingsContent {
    pub api_url: Option<String>,           // default: https://api.runpod.io
    pub auto_discover: Option<bool>,       // default: true
    pub available_models: Option<Vec<RunpodAvailableModel>>,
    pub custom_headers: Option<HashMap<String, String>>,
}

pub struct RunpodAvailableModel {
    pub name: String,                       // endpoint name (model id)
    pub display_name: Option<String>,
    pub endpoint_id: String,                // RunPod serverless endpoint ID
    pub max_tokens: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_images: bool,              // default false
}
```

### 2. `crates/language_models/src/settings.rs` — PARTIALLY DONE

Added `runpod::RunpodSettings` to the import list and `pub runpod: RunpodSettings` to `AllLanguageModelSettings`.

**Still needed:** Add `let runpod = language_models.runpod.unwrap();` to the `from_settings` destructuring, and add the `runpod: RunpodSettings { ... }` construction in the `Self { ... }` block. Pattern it after the DeepSeek settings construction (around line 113-117 in the original file).

## Changes still needed

### 3. Create `crates/language_models/src/provider/runpod.rs` — NOT STARTED

This is the main provider file. Model it after `crates/language_models/src/provider/deepseek.rs` but with these key differences:

1. **Provider ID:** `LanguageModelProviderId::new("runpod")`
2. **Provider name:** `LanguageModelProviderName::new("RunPod")`
3. **API key env var:** `RUNPOD_API_KEY` (use the D12 `api_key_env_var_name_for` pattern, or hardcode since it's a dedicated provider)
4. **Default API URL:** `https://api.runpod.io`
5. **Each model carries its own endpoint URL** — the `RunpodLanguageModel` struct should have an `endpoint_url: String` field (e.g. `https://api.runpod.io/v2/hsldzov6932wf5/openai/v1`)
6. **Discovery:** When `auto_discover` is true and the API key is set, query the RunPod GraphQL API at `{api_url}/graphql` with the query:
   ```graphql
   query { myself { endpoints { id name type env { key value } } } }
   ```
   Parse the response, filter for endpoints (optionally filter by type `QB`), and create a model for each. The model name is the endpoint name, the endpoint URL is `{api_url}/v2/{endpoint_id}/openai/v1`.
7. **Inference:** Use the OpenAI-compatible streaming logic from the `open_ai` crate (like DeepSeek does with `deepseek::stream_completion`). The endpoint URL is per-model, not per-provider. Use `open_ai::stream_completion` with the model's endpoint URL.
8. **Vision support:** The `supports_images` flag from the model config determines `supports_images()`. For OLMOCR-2, this should be `true`.
9. **telemetry_id:** `format!("runpod/{}", self.model_name)` — this is what `resolve_model` in `kask/crates/kask_bridge/src/model_resolution.rs` matches against. The model id should be the endpoint name (e.g. `kask-ocr`), so `RunPod/kask-ocr` resolves via case-insensitive provider ID lookup + `model.id()` match.
10. **Settings view:** Use `ProviderSettingsView::ApiKey(ApiKeyConfiguration::new(...))` like DeepSeek, with a link to `https://www.runpod.io/console/serverless` for API key management.

**Key implementation details for the model struct:**
```rust
pub struct RunpodLanguageModel {
    id: LanguageModelId,           // endpoint name (e.g. "kask-ocr")
    display_name: String,
    endpoint_url: String,          // per-model: https://api.runpod.io/v2/{id}/openai/v1
    supports_images: bool,
    max_tokens: u64,
    max_output_tokens: Option<u64>,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}
```

The `stream_completion` method should read the API key from state, then call `open_ai::stream_completion` with the model's `endpoint_url` (NOT the provider-level api_url).

**For the discovery task:** Spawn a GPUI task on authenticate/settings change that:
1. Reads the API key from state
2. POSTs to `{api_url}/graphql` with the GraphQL query
3. Parses the response into a list of endpoints
4. For each endpoint, creates a `RunpodAvailableModel` (name = endpoint name, endpoint_id = endpoint id)
5. Stores the discovered models in a `discovery_state` entity (like `OpenAiCompatibleLanguageModelProvider`'s `DiscoveryState`)
6. Merges with statically configured `available_models` in `provided_models()`

### 4. Add `pub mod runpod;` to `crates/language_models/src/provider.rs`

Add after `pub mod opencode;` (line 21).

### 5. Register the provider in `crates/language_models/src/language_models.rs`

In `register_language_model_providers` (around line 216), add:
```rust
registry.register_provider(
    Arc::new(RunpodLanguageModelProvider::new(
        client.http_client(),
        credentials_provider.clone(),
        cx,
    )),
    cx,
);
```
Add it after the DeepSeek provider registration (around line 278). Import `RunpodLanguageModelProvider` from `crate::provider::runpod`.

### 6. Add an icon — `crates/icons/src/icons.rs`

Add `AiRunpod,` to the `IconName` enum (after `AiOpenRouter` or similar). Note: adding an icon requires adding an SVG asset. If that's too complex, use `IconOrSvg::Icon(IconName::AiOpenAiCompat)` as a placeholder (RunPod uses OpenAI-compatible APIs).

### 7. Update `DIVERGENCE.md`

Add a new D-seam entry (D29 or next available) documenting the RunPod provider addition. The entry should note:
- File: `crates/language_models/src/provider/runpod.rs` + `crates/language_models/src/settings.rs` + `crates/language_models/src/language_models.rs` + `crates/settings_content/src/language_model.rs` + `crates/language_models/src/provider.rs`
- What: RunPod serverless endpoint provider with GraphQL-based endpoint discovery
- Why: RunPod is not a standard OpenAI-compatible provider — each endpoint has its own API URL, and endpoint discovery uses the RunPod GraphQL API, not `/v1/models`
- Pin with a test that verifies `RunPod/kask-ocr` resolves through `resolve_model_names`

### 8. Update `~/.config/zed-kask/settings.json`

Add a `runpod` entry under `language_models`:
```json
"runpod": {
  "api_url": "https://api.runpod.io",
  "auto_discover": true,
  "available_models": [
    {
      "name": "kask-ocr",
      "display_name": "kask-ocr (OLMOCR-2)",
      "endpoint_id": "hsldzov6932wf5",
      "max_tokens": 32768,
      "supports_images": true
    }
  ]
}
```

The statically configured `kask-ocr` model ensures it's always available even if discovery fails (e.g. endpoint not deployed). The `auto_discover: true` will also query the GraphQL API for any other endpoints.

### 9. Build and test

Build with:
```bash
cargo build --release -p agent -p project -p language_models -p settings_content
```

Then build the full zed-kask binary:
```bash
cargo build --release -p zed --bin zed-kask
```

Then rebuild MCP servers (they don't need the RunPod provider directly — they route through the IPC bridge which goes through the zed-kask registry).

## Key files to reference

| File | Role |
|------|------|
| `crates/language_models/src/provider/deepseek.rs` | Template for a dedicated provider with API key state, settings, and OpenAI-compatible streaming |
| `crates/language_models/src/provider/open_ai_compatible.rs` | Template for discovery state pattern (`DiscoveryState` with `restart_fetch_models_task`) |
| `crates/language_models/src/provider/api_compatible.rs` | D12 `api_key_env_var_name_for` function and `ApiCompatibleProviderState` |
| `crates/language_models/src/settings.rs` | Where `RunpodSettings` is added (partially done) |
| `crates/language_models/src/language_models.rs` | Where the provider is registered (line ~278) |
| `crates/language_models/src/provider.rs` | Where `pub mod runpod;` is added |
| `crates/settings_content/src/language_model.rs` | Settings content schema (done) |
| `crates/icons/src/icons.rs` | Icon enum (optional icon addition) |
| `kask/crates/kask_bridge/src/model_resolution.rs` | `resolve_model_names` — must resolve `RunPod/kask-ocr` |
| `kask/crates/hkask-inference/src/model_constants.rs` | `DEFAULT_OCR_MODEL = "RunPod/kask-ocr"` |
| `DIVERGENCE.md` | Add D-seam entry |

## How model resolution works (critical for verification)

The corpus server calls `generate_vision(prompt, images, params, Some("RunPod/kask-ocr"))` via the IPC bridge. On the zed-kask side:

1. `LanguageModelInferencePort::resolve_model` in `kask/crates/kask_bridge/src/inference.rs:219` looks up `RunPod/kask-ocr` in the `LanguageModelRegistry`
2. `resolve_model_names` in `kask/crates/kask_bridge/src/model_resolution.rs:35` splits on `/` → provider_id=`RunPod`, model_id=`kask-ocr`
3. Case-insensitive provider lookup finds the registered `runpod` provider
4. Searches the provider's models for `model.id() == "kask-ocr"` — this must match the `LanguageModelId` set from the endpoint name
5. Returns the `Arc<dyn LanguageModel>` for inference

The `list_vision_models` path (used by `resolve_ocr_model` in the corpus server) enumerates all models from all providers via the IPC bridge. The IPC server in `kask/crates/kask_bridge/src/inference_ipc_server.rs:345` builds `ModelListEntry` with `name: format!("{}/{}", provider_id, model.name())` and `supports_vision: model.supports_images()`. So the RunPod model must have `supports_images() == true` for `kask-ocr` to appear in the vision model list.

## Constraints

- No `unwrap()` — use `?` to propagate errors
- No `mod.rs` files — use `src/module.rs` instead
- No `block_on` on the foreground thread
- `AsyncApp` is not `Send` — use `tokio::sync::mpsc` channels with a foreground drainer for background tasks that need `AsyncApp`
- `cx.background_spawn` panics on tokio-dependent futures (reqwest) — use `gpui_tokio::Tokio::spawn(&*cx, ...)` for HTTP requests in background tasks. Look at how `OpenAiCompatibleLanguageModelProvider`'s `DiscoveryState::restart_fetch_models_task` handles this — it uses `cx.spawn` (GPUI task) with the `http_client` (which is `Arc<dyn HttpClient>` and works on GPUI tasks).
- Build: use `./script/clippy` instead of `cargo clippy`
- DIVERGENCE.md tracks deviations from upstream Zed. Code in `crates/` needs D-seam entries.

## Nebius (Blocker 3) — defer

Nebius inference is not needed for the current pipeline. The `NebiusHost` exists in `kask/mcp-servers/hkask-mcp-training/src/providers/nebius.rs` for training. When there's a use case, follow the same pattern as RunPod but with Nebius's API.

## After the code changes

1. Build the zed-kask binary: `cargo build --release -p zed --bin zed-kask`
2. Build MCP servers: `cargo build --release -p hkask-mcp-corpus -p hkask-mcp-training` (and others as needed)
3. Install MCP server binaries to `~/.local/bin/hkask-mcp-*`
4. Update `~/.config/zed-kask/settings.json` with the RunPod provider config
5. Restart zed-kask to launch MCP servers with the self-healing code and RunPod provider
6. The user needs to deploy the `kask-ocr` RunPod endpoint (scale to ≥1 worker) via the RunPod console
7. Verify OCR works by calling `corpus_convert` on a scanned PDF
8. Resume the corpus pipeline from Phase 2a (re-run tagging with working inference)
