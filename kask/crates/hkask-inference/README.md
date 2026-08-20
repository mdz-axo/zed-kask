# hkask-inference

Multi-provider inference router for hKask — OpenRouter, Ollama, RunPod.

## Features

- **Provider dispatch** — route inference requests to the best available provider
- **Model selection** — fuzzy search, prefix-based routing (`OR/`, `OM/`, `RP/`)
- **Provider ID parsing** — `ProviderId` with model name resolution
- **Prompt validation** — never-panics input validation

## Configuration

| Variable | Description |
|----------|-------------|
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `RUNPOD_API_KEY` | RunPod API key (vision/OCR only) |
| `RUNPOD_TEMPLATE_ID` | RunPod serverless template ID (alternative to `RUNPOD_BASE_URL`) |
| `HKASK_DEFAULT_MODEL` | Default model (e.g., `OR/z-ai/glm-5.2`) |
| `HKASK_DEFAULT_PROVIDER` | Default provider code (OR, OM, RP; default: OR) |
