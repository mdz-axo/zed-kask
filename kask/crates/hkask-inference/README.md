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
