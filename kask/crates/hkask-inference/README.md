# hkask-inference

Multi-provider inference router for hKask — DeepInfra, fal.ai, OpenRouter, KiloCode, Ollama, RunPod.

## Features

- **Provider dispatch** — route inference requests to the best available provider
- **Model selection** — fuzzy search, prefix-based routing (`DI/`, `FA/`, `TG/`, `OR/`, `KC/`, `OM/`, `CL/`, `RP/`)
- **Provider ID parsing** — `ProviderId` with model name resolution
- **Prompt validation** — never-panics input validation

## Configuration

| Variable | Description |
|----------|-------------|
| `DEEPINFRA_API_KEY` | DeepInfra API key |
| `ATLASCLOUD_API_KEY` | AtlasCloud API key (media generation) |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `KILOCODE_API_KEY` | KiloCode API key |
| `RUNPOD_API_KEY` | RunPod API key (vision/OCR only) |
| `RUNPOD_TEMPLATE_ID` | RunPod serverless template ID (alternative to `RUNPOD_BASE_URL`) |
| `HKASK_DEFAULT_MODEL` | Default model (e.g., `KC/z-ai/glm-5.2`) |
| `HKASK_DEFAULT_PROVIDER` | Default provider code (DI, OR, KC, OM; default: DI) |
