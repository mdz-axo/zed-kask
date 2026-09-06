# hkask-inference

Multi-provider inference router for hKask — OpenRouter, Ollama, RunPod.

## Features

- **Provider dispatch** — route inference requests to the best available provider
- **Model selection** — fuzzy search, prefix-based routing (`OR/`, `OM/`, `RP/`)
- **Provider ID parsing** — `ProviderId` with model name resolution
- **Prompt validation** — never-panics input validation

## QA transport contracts

- `LazyInferencePort` tries IPC first. If the bridge cannot be reached, direct
  chat uses the explicit model or `HKASK_DEFAULT_MODEL`; an unset default is
  `NotConfigured`, not a hidden replacement model. A response error from a
  connected bridge is returned, not retried through another provider.
- Direct chat preserves the supplied `ChatMessage` array (roles, order, and
  content). String prompts delegate to that same HTTP implementation as one
  user message. Direct tool-definition forwarding remains unsupported.
- Raw provider batch output is normalized by `batch::parse_batch_results`:
  duplicate `custom_id`s fail permanently, and a response accompanied by an
  error fails as malformed. Provider errors and no-choice failures remain
  errors. Counts reflect unique terminal map entries, not JSONL line counts.
  The map contains only returned identities; callers still detect missing or
  unsolicited IDs against their original prompts.
- The bridge maps normalized failures to `BatchResultEntry { text: None,
  error: Some(...), total_tokens: 0, .. }`. Corpus QA treats these as failed
  prompts, never successful QA rows. Parser/IPC-contract tests live in
  `src/batch.rs`; corpus consumption is pinned by
  `batch_response_failures_have_truthful_totals`. Offline HTTP capture tests
  in `src/hkask_inference.rs` exercise both direct and lazy fallback paths in
  an env-isolated subprocess with a loopback proxy, without live providers.

## Configuration

| Variable | Description |
|----------|-------------|
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `RUNPOD_API_KEY` | RunPod API key (vision/OCR only) |
| `RUNPOD_TEMPLATE_ID` | RunPod serverless template ID (alternative to `RUNPOD_BASE_URL`) |
| `HKASK_DEFAULT_MODEL` | Default model (e.g., `OR/z-ai/glm-5.2`) |
| `HKASK_DEFAULT_PROVIDER` | Default provider code (OR, OM, RP; default: OR) |

## Media routing policy — operator decision 2026-09-06

The selected model is authoritative: `params.model` when present, otherwise
that operation's configured environment model. Selectable operations require
`OpenRouter/<provider-local-model>` or `DeepInfra/<provider-local-model>`.
Full provider names are ASCII case-insensitive; the remainder may contain a
vendor slash and is preserved exactly. Bare models, short aliases (`OR/`,
`DI/`), blank overrides, and unknown providers are invalid.

**URL-safety clarification (2026-09-06):** provider-local model components
must be nonempty and cannot be `.` or `..`. Whitespace, control characters,
`?`, `#`, backslash, and `%` (including URL escapes) are rejected as `Model`
before any request, for explicit and env-selected models alike. Ordinary
dots, hyphens, underscores, version colons, and Unicode names remain valid.
The registry and direct adapters share this validation; native DeepInfra
URLs append model identifiers as URL path segments, preserving the
configured host/base path instead of parsing model text as URL syntax.

| Operation | Environment model when no override is supplied |
|---|---|
| `generate_image`, `image_to_image` | `HKASK_MEDIA_IMAGE_GEN_MODEL` |
| `generate_speech` | `HKASK_MEDIA_TTS_MODEL` |
| `transcribe` | `HKASK_MEDIA_STT_MODEL` |
| `generate_video`, `image_to_video` | `HKASK_MEDIA_VIDEO_MODEL` |
| `chat_audio` | `HKASK_MEDIA_AUDIO_CHAT_MODEL` |
| `chat_json` | `HKASK_MEDIA_STRUCTURED_PASS_MODEL` |

`ProviderRegistry::execute` resolves once, verifies the named provider is
registered and supports the operation, and calls exactly that provider with
only the provider-local model. No scoring, registration-order selection,
automatic cross-provider retry, or hidden default substitution remains.
The selected provider's errors return unchanged. Missing configuration or a
selected key names its environment variable (`NotConfigured`); invalid
selection is `Model`. Media MCP maps those to `permission_denied` and
`invalid_argument`, respectively; provider 401/403 remains `Auth` →
`permission_denied`.

`remove_background` and `upscale` use the fixed DeepInfra native endpoints
`Bria/remove_background` and `latentconsistency/upscale`. They need
`DEEPINFRA_API_KEY`, not a model env var, and reject model overrides.
OpenRouter does not serve image-to-image or TTS in these adapters;
DeepInfra does not serve `chat_audio` or `chat_json`.

This decision supersedes `d660f3b754` (scoring/automatic fallback) and
`8cc79c797e` (registration-order routing), not `f86cf19a70` (child-local
media). `LazyInferencePort::media_generate` still uses the child-local
`MediaRouter`; foreground media behavior and chat/vision/embed/IPC routes
are unchanged. The router is now media-only, with one inherent
`media_generate` entry point rather than a fake chat `InferencePort` impl.

**Migration:** qualify existing bare/short-alias media model settings and
replay overrides with the intended full provider name; install that
provider's key. Remove overrides from fixed-model operations. Settings
remain the source of defaults; standalone callers must configure the env
model or pass an override. The current STT settings default remains
`OpenRouter/openai/whisper-large-v3-turbo`; image, video, and TTS settings
have no default. No compatibility fallback is provided.

Offline pins: `media_routing_tests` exercises all eight selectable ops,
invalid selections, absent keys, fixed ops, typed failures, real adapter
HTTP payloads, and child-local `LazyInferencePort` with zero IPC calls.
`hkask-mcp-media/src/error.rs` pins real router 401/403 → tool
`permission_denied` and invalid model → `invalid_argument`.
