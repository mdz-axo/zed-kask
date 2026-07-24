# hkask-inference

Multi-provider inference router for hKask — DeepInfra, Together AI, fal.ai, OpenRouter, KiloCode, Ollama, Cline, RunPod.

## Features

- **Provider dispatch** — route inference requests to the best available provider
- **Model selection** — fuzzy search, prefix-based routing (`DI/`, `FA/`, `TG/`, `OR/`, `KC/`, `OM/`, `CL/`, `RP/`)
- **Provider ID parsing** — `ProviderId` with model name resolution
- **Prompt validation** — never-panics input validation
- **Multi-model fusion** — provider-agnostic panel+judge deliberation (algo/LLM judge)

## Configuration

| Variable | Description |
|----------|-------------|
| `DI_API_KEY` | DeepInfra API key |
| `FA_API_KEY` | Fal.ai API key |
| `TG_API_KEY` | Together AI API key |
| `OR_API_KEY` | OpenRouter API key |
| `KC_API_KEY` | KiloCode API key |
| `CLINE_API_KEY` | Cline cloud gateway API key |
| `RUNPOD_API_KEY` | RunPod API key (vision/OCR only) |
| `RUNPOD_TEMPLATE_ID` | RunPod serverless template ID (alternative to `RUNPOD_BASE_URL`) |
| `HKASK_DEFAULT_MODEL` | Default model (e.g., `KC/z-ai/glm-5.2`) |
| `HKASK_DEFAULT_PROVIDER` | Default provider code (DI, FA, TG, OR, KC, OM, CL; default: DI) |
| `HKASK_FUSION_JUDGE_MODEL` | Fusion judge model (or `algo` for no-LLM merge) |
| `HKASK_FUSION_PANEL_MODELS` | Comma-separated fusion panel models |
| `HKASK_FUSION_MODE` | Fusion mode: synthesis, best-of-n, critique, deliberation, pi |
| `HKASK_FUSION_DISABLED` | Set to `1` to disable fusion |
