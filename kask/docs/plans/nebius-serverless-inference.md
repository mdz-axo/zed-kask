# Future Plan: Nebius Serverless AI Inference Backend

## Status

**Deferred.** No current use case. This document captures the knowledge and context gained from wiring up the RunPod backend (D29) so a future implementation is fast and correct.

## Why defer

Nebius Serverless AI endpoints are architecturally simpler than RunPod serverless — each endpoint is a single OpenAI-compatible API with a single managed HTTPS URL and a per-endpoint token. The existing `openai_compatible` provider already handles this pattern without new code. A dedicated provider (like RunPod's D29) is only needed if Nebius adds account-wide API key auth and a discovery API in the future.

## What Nebius Serverless AI is

Nebius AI Cloud offers a Serverless AI service where you deploy a containerized workload (e.g. `vllm/vllm-openai`) as an **endpoint** that listens for requests and returns results immediately. The service handles resource provisioning, lifecycle, and per-second billing. You don't manage VMs.

Source: https://docs.nebius.com/serverless/overview

### Key characteristics

| Property | Nebius Serverless AI | RunPod Serverless (D29) |
|----------|---------------------|------------------------|
| **API shape** | OpenAI-compatible (`/v1/chat/completions`, `/v1/models`) | OpenAI-compatible (`/openai/v1/chat/completions`, `/openai/v1/models`) |
| **URL pattern** | Managed HTTPS URL per endpoint (e.g. `https://...nebius.com/...`) — NOT derivable from an endpoint ID | Predictable pattern: `https://api.runpod.ai/v2/{endpoint_id}/openai/v1` |
| **Auth** | Per-endpoint token (generated at creation, `--auth token --token "$AUTH_TOKEN"`) | Account-wide API key (`RUNPOD_API_KEY`) — same for all endpoints |
| **Discovery** | None — manage via Nebius CLI (`nebius ai endpoint list/create/get`) | GraphQL API at `https://api.runpod.io/graphql` — list all endpoints in the account |
| **Model per endpoint** | One model baked into the container command (`--model Qwen/Qwen3-0.6B`) | One model per endpoint (vLLM `MODEL_NAME` env var) |
| **Scaling** | Per-second billing, stop/start endpoints | `workersMin`/`workersMax` auto-scaling |
| **Domain split** | Single domain (managed HTTPS URL) | GraphQL at `api.runpod.io`, REST at `api.runpod.ai` |

### How to deploy a vLLM endpoint on Nebius

From the official tutorial (https://docs.nebius.com/serverless/tutorials/deploy-model.md):

```bash
# 1. Generate a per-endpoint auth token
export AUTH_TOKEN=$(openssl rand -hex 32)

# 2. Set the model ID (Hugging Face model identifier)
export MODEL_ID="Qwen/Qwen3-0.6B"

# 3. Get a subnet ID
export SUBNET_ID=$(nebius vpc subnet list --format jsonpath='{.items[0].metadata.id}')

# 4. Create the endpoint
nebius ai endpoint create \
  --name qs-vllm-chat \
  --image vllm/vllm-openai:v0.18.0-cu130 \
  --container-command "python3 -m vllm.entrypoints.openai.api_server" \
  --args "--model $MODEL_ID --host 0.0.0.0 --port 8000" \
  --platform gpu-l40s-a \
  --preset 1gpu-8vcpu-32gb \
  --public \
  --container-port 8000 \
  --auth token \
  --token "$AUTH_TOKEN" \
  --shm-size 16Gi \
  --subnet-id "$SUBNET_ID"

# 5. Get the endpoint's managed HTTPS URL
export ENDPOINT_URL=$(nebius ai endpoint get-by-name --name qs-vllm-chat --format json \
  | jq -r '.status.public_endpoints[] | select(startswith("https://"))' | head -1)

# 6. Test
curl "$ENDPOINT_URL/v1/models" -H "Authorization: Bearer $AUTH_TOKEN" | jq
curl "$ENDPOINT_URL/v1/chat/completions" \
  -H "Authorization: Bearer $AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"model\": \"$MODEL_ID\", \"messages\": [{\"role\":\"user\",\"content\":\"Hello\"}]}" | jq
```

## Implementation plan (when needed)

### Option A: `openai_compatible` (no code change — recommended for now)

Since Nebius endpoints use a single URL + per-endpoint token and support `/v1/models`, the existing `openai_compatible` provider works out of the box:

1. **Deploy the endpoint** via Nebius CLI or console (see above).
2. **Add to `settings.json`:**
   ```json
   "language_models": {
     "openai_compatible": {
       "Nebius": {
         "api_url": "https://<managed-https-url>",
         "available_models": [
           {
             "name": "Qwen/Qwen3-0.6B",
             "display_name": "Qwen3-0.6B (Nebius)",
             "max_tokens": 32768,
             "capabilities": {
               "tools": true,
               "images": false,
               "parallel_tool_calls": true,
               "prompt_cache_key": false,
               "chat_completions": true,
               "interleaved_reasoning": false,
               "max_tokens_parameter": true
             }
           }
         ],
         "auto_discover": true
       }
     }
   }
   ```
3. **Enter the API key** (the per-endpoint token) via Settings → AI → LLM Providers → OpenAI Compatible → Nebius.
4. **For MCP server env injection** (if needed): add a `NEBIUS_INFERENCE_API_KEY` env var to `DATA_SERVICES` or `INFERENCE_PROVIDERS` in `kask/crates/kask_bridge/src/inference_providers.rs`, with `credential_key: "nebius_inference"` and `ui_toggle: Some("nebius_inference")`. Add a `nebius_inference_enabled` field to `KaskDataServiceSettings` or `KaskInferenceProvidersSettings`. This is a settings schema change but no new provider code.

**Limitation:** The per-endpoint token means each Nebius endpoint needs its own `openai_compatible` entry (different URL + different token). If you have many endpoints, this is verbose. A dedicated provider would centralize this — but only if Nebius adds account-wide API key auth.

### Option B: Dedicated `nebius` provider (if account-wide auth is added)

If Nebius adds account-wide API key auth (like RunPod's `RUNPOD_API_KEY`) and/or a discovery API, follow the RunPod D29 pattern:

1. **Create `crates/language_models/src/provider/nebius.rs`** — model after `runpod.rs` (D29).
2. **Key differences from RunPod:**
   - No GraphQL discovery (unless Nebius adds one) — rely on `auto_discover` via `/v1/models` (the `openai_compatible` `DiscoveryState` pattern) or static `available_models` only.
   - No domain split — the managed HTTPS URL is the single API URL for both discovery and inference.
   - Per-endpoint tokens, not account-wide API key — each model carries its own token (like RunPod's per-model `endpoint_url`). Add a `token: String` field to `NebiusAvailableModel`.
   - The `model` field in OpenAI requests should match the Hugging Face model ID (e.g. `Qwen/Qwen3-0.6B`) — same as RunPod's `served_model_name`.
3. **Settings schema:** `NebiusSettingsContent` in `crates/settings_content/src/language_model.rs`, `NebiusSettings` in `crates/language_models/src/provider/nebius.rs`, wired through `settings.rs` and `provider.rs`.
4. **Register in `language_models.rs`** — `registry.register_provider(Arc::new(NebiusLanguageModelProvider::new(...)), cx)`.
5. **Credential bridge:** Add Nebius to `INFERENCE_PROVIDERS` in `kask/crates/kask_bridge/src/inference_providers.rs` with `api_url` set to... this is where it gets tricky. Nebius endpoints don't share a single `api_url` — each has its own managed HTTPS URL. The `ApiKeyState` keychain lookup is keyed by `api_url`. For a dedicated provider with per-endpoint tokens, the keychain URL would need to be per-model, not per-provider. This is a fundamental difference from RunPod (where the API key is account-wide and the `api_url` is the GraphQL base URL).

   **Possible approach:** Store the per-endpoint token in the `AvailableModel` config directly (not in the keychain). This is less secure but matches Nebius's per-endpoint token model. Alternatively, store tokens in the keychain under `kask://credentials/nebius_{endpoint_name}` and resolve them per-model.

6. **DIVERGENCE.md:** Add a D-seam entry (D30 or next available) documenting the provider addition.
7. **Pin tests:** `endpoint_url` format, `telemetry_id` format, case-insensitive provider ID match, model field match.

## Lessons learned from RunPod (D29)

These lessons apply directly to any future Nebius provider implementation:

### 1. Domain split trap
RunPod has two domains: `api.runpod.io` (GraphQL) and `api.runpod.ai` (REST). The initial implementation used `api.runpod.io` for the OpenAI-compatible REST API, which returned 404. **Always verify the actual API URL against the provider's official docs by making a live request.** Don't assume the GraphQL API domain and the REST API domain are the same.

For Nebius: there's no domain split — the managed HTTPS URL is the single API URL. But verify this by deploying an endpoint and curling it.

### 2. `model.name()` vs `model.id()` — the resolution key
The IPC server builds `ModelListEntry.name` as `{provider_id}/{model.name()}`. If `name()` returns a display name with a suffix (e.g. `"kask-ocr (OLMOCR-2)"`), the prefixed name can't match a provider-prefixed config string like `"RunPod/kask-ocr"` in `resolve_ocr_model`. **`name()` must return the resolution key (the endpoint name), not the display name.** The display name is for UI rendering only.

For Nebius: `name()` should return the Hugging Face model ID (e.g. `"Qwen/Qwen3-0.6B"`), which is both the resolution key and the `model` field in OpenAI requests.

### 3. `resolve_ocr_model` case sensitivity
The corpus `resolve_ocr_model` gate used case-sensitive `==` against `ModelEntry.prefixed_name`/`model`, but the IPC server produces lowercase provider IDs while config uses capitalized names. **Always use `eq_ignore_ascii_case` in model-name comparison gates.** This was fixed for RunPod (D29) but the fix applies to all providers.

For Nebius: already fixed by the D29 `eq_ignore_ascii_case` change in `convert.rs`.

### 4. Credential bridge — two keychain stores
The kask credential system stores keys under `kask://credentials/<key>` (for MCP server env injection). Zed's `ApiKeyState` reads from the system keychain keyed by the provider's `api_url`. These are separate stores. **A mirror function is needed** to copy the key from the kask store to the Zed keychain at the `api_url` the provider reads. Without it, a key set via the kask settings UI is invisible to the provider.

For Nebius: if using `openai_compatible` (Option A), the existing `openai_compatible` provider reads from the Zed keychain at the `api_url` — no mirror needed (the user enters the token via the provider's settings UI). If using a dedicated provider (Option B), a mirror function like `mirror_runpod_api_key` would be needed, but the per-endpoint token model complicates this (each model has its own token).

### 5. `INFERENCE_PROVIDERS` vs `DATA_SERVICES`
`INFERENCE_PROVIDERS` mirrors keys to BOTH the Zed keychain (at `api_url`) AND `kask://credentials/<key>`. `DATA_SERVICES` mirrors only to `kask://credentials/<key>`. If a provider needs the key in the Zed keychain (for `ApiKeyState`), it must be in `INFERENCE_PROVIDERS`. If it only needs MCP env injection, `DATA_SERVICES` suffices.

For Nebius: if using `openai_compatible` (Option A), no `INFERENCE_PROVIDERS` entry is needed — the `openai_compatible` provider handles its own keychain. If using a dedicated provider (Option B), add to `INFERENCE_PROVIDERS` — but the per-endpoint token model means the `api_url` field doesn't map cleanly (each endpoint has a different URL).

### 6. `supports_tools` — don't advertise what the model can't do
RunPod's OLMOCR-2 is a specialized OCR model that doesn't support tool calls. The initial implementation advertised `supports_tools: true`, which could cause the agent panel to send tool-call requests to it. **Set `supports_tools`/`supports_streaming_tools` to `false` for specialized models.** Make it configurable per-model if the provider hosts both tool-capable and non-tool models.

For Nebius: depends on the deployed model. vLLM with a tool-capable model (e.g. Qwen3) supports tools; vLLM with a non-tool model doesn't. Make it configurable.

### 7. `served_model_name` — vLLM expects `MODEL_NAME`
vLLM expects the OpenAI `model` field to match `MODEL_NAME` (or `--served-model-name`). Passing the endpoint name instead of the model name causes 400/404. **Always send the Hugging Face model ID as the `model` field**, not the endpoint name. Add a `served_model_name` field to the model config for cases where it differs.

For Nebius: the `model` field should be the Hugging Face model ID (e.g. `Qwen/Qwen3-0.6B`), which is the same value passed to `--model` in the container command. The `openai_compatible` provider already passes `model.name` as the `model` field, so set `name` to the model ID.

### 8. Warn storm — always-on providers
A dedicated provider is always registered (unlike `openai_compatible` which is opt-in). If `auto_discover` defaults to `true` and no API key is set, every `SettingsStore` change re-fires the observer and re-logs the "no API key" warning. **Either suppress repeated warnings (track `warned_no_api_key` state) or default `auto_discover` to `false` for dedicated providers.** The RunPod implementation initially had this bug — hundreds of identical warnings per startup.

For Nebius: if using `openai_compatible` (Option A), this isn't an issue (opt-in). If using a dedicated provider (Option B), apply the same fix.

### 9. `max_tokens` — verify against the actual model
The RunPod static config had `max_tokens: 32768` as a placeholder, but the actual model supported 128000 (discovered via the `/v1/models` response). **Always query `/v1/models` to get `max_model_len` and update the config.** Don't guess.

For Nebius: the `/v1/models` endpoint returns `max_model_len` — use it.

### 10. DIVERGENCE.md — document every `crates/` change
Every modification to upstream Zed files in `crates/` needs a D-seam entry in `DIVERGENCE.md` with pin tests. The RunPod implementation initially missed this. **Add the D-seam entry in the same PR as the code change.**

For Nebius: if using `openai_compatible` (Option A), no `crates/` changes are needed (settings-only). If using a dedicated provider (Option B), add a D-seam entry.

## Decision matrix

| Criterion | Option A (`openai_compatible`) | Option B (dedicated provider) |
|-----------|-------------------------------|-------------------------------|
| Code changes | None (settings-only) | New provider file + settings schema + registration |
| Auto-discovery | Via `/v1/models` (built-in) | Custom (no Nebius discovery API) |
| Per-endpoint tokens | One `openai_compatible` entry per endpoint | Per-model token field |
| MCP env injection | Add `INFERENCE_PROVIDERS`/`DATA_SERVICES` entry | Same |
| Keychain mirror | Not needed (provider UI handles it) | Needed (like `mirror_runpod_api_key`) |
| Multiple endpoints | Verbose (one entry per endpoint) | Centralized (one provider, multiple models) |
| DIVERGENCE.md | Not needed (no `crates/` changes) | Required (D-seam entry) |

**Recommendation:** Start with Option A. Only move to Option B if:
- You have many Nebius endpoints and the per-entry settings become unmanageable, OR
- Nebius adds account-wide API key auth and a discovery API (making the RunPod pattern applicable)
