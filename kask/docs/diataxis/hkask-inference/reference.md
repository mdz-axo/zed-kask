---
title: "hkask-inference — Reference"
audience: [developers, architects, agents]
last_updated: 2026-07-29
version: "0.2.0"
status: "Active"
domain: "Inference"
mds_categories: [domain, composition]
---

# hkask-inference — Reference

`hkask-inference` provides the MCP-server-local inference routing layer for
hKask. It defines the `ProviderId` enum, the `InferenceConfig` struct, and the
`InferenceRouter` that dispatches chat, vision, and embedding requests to
provider-specific backends. The crate is used by MCP-server-internal inference
paths; user-facing inference goes through zed's `LanguageModelRegistry` via
`kask_bridge`. Long-term, the architecture plan replaces this with
`InferencePort` over zed's `LanguageModel`, but keeping it unblocks the MCP
servers immediately.

## Source citations

| Symbol | Location |
|--------|----------|
| `ProviderId` enum | `kask/crates/hkask-inference/src/config.rs:44` |
| `InferenceConfig` struct | `kask/crates/hkask-inference/src/config.rs:192` |
| `MediaRouter` struct | `kask/crates/hkask-inference/src/media_router.rs` |
| `InferenceIpcClient` struct | `kask/crates/hkask-inference/src/inference_ipc_client.rs` |
| `FusionOrchestrator` | `kask/crates/hkask-inference/src/fusion_orchestrator.rs` |
| `DeepInfraBackend` | `kask/crates/hkask-inference/src/deepinfra_backend.rs:21` |
| `FalBackend` | `kask/crates/hkask-inference/src/fal_backend.rs:26` |
| `OpenRouterBackend` | `kask/crates/hkask-inference/src/openrouter_backend.rs:22` |
| `RouterModelEntry` | `kask/crates/hkask-inference/src/hkask_inference.rs:79` |

## Provider model

The `ProviderId` enum (`config.rs:44`) identifies the inference provider. Each
variant carries a serde rename tag (a two-letter serialization code) and a
model-name prefix (registered in the `PREFIXES` const of `parse_from_model`).
The `InferenceConfig` struct (`config.rs:192`) holds the base URLs and API
keys for each provider, plus the `default_provider` field.

Embedding generation is handled by `kask_bridge::LanguageModelEmbeddingPort`
(in `kask/crates/kask_bridge/src/inference.rs`), which resolves credentials
directly from the `INFERENCE_PROVIDERS` table + env var and makes raw
`/embeddings` POSTs. The old `EmbeddingRouter` and `InferenceRouter` structs
have been deleted — they were dead code, never constructed.

```mermaid
classDiagram
    class ProviderId {
        <<enumeration>>
        DeepInfra
        Fal
        Together
        Runpod
        OpenRouter
        KiloCode
        Ollama
        Cline
    }
    class InferenceConfig {
        +default_provider: ProviderId
        +deepinfra_base_url: String
        +deepinfra_api_key: String
        +fal_base_url: String
        +fal_api_key: String
        +together_base_url: String
        +together_api_key: String
        +openrouter_base_url: String
        +openrouter_api_key: String
        +kilocode_base_url: String
        +kilocode_api_key: String
        +ollama_base_url: String
        +ollama_api_key: String
        +cline_base_url: String
        +cline_api_key: String
        +runpod_* : String
        +default_model: String
        +fusion: Option~FusionConfig~
    }
    class MediaRouter {
        -config: InferenceConfig
        -deepinfra: Option~DeepInfraBackend~
        -fal: Option~FalBackend~
        -openrouter: Option~OpenRouterBackend~
    }
    class InferenceIpcClient {
        +generate_with_model(model, prompt, params) Result
        +media_generate(op, params) Result
    }

    MediaRouter --> InferenceConfig : holds
    InferenceIpcClient --> MediaRouter : falls back to
    InferenceConfig --> ProviderId : defaults to
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-INF-001
verified_date: 2026-07-31
verified_against: kask/crates/hkask-inference/src/config.rs:44,192; kask/crates/hkask-inference/src/media_router.rs; kask/crates/hkask-inference/src/inference_ipc_client.rs
status: VERIFIED
-->

## Backend traits

The `ChatBackend` trait (`backend.rs:51`) defines the chat completion
interface. The `generate` method returns a non-streaming result; the
`generate_stream` method returns an SSE stream; the
`generate_with_messages` method accepts pre-formed message histories. All
three return pinned boxed futures because provider calls are asynchronous.

The `VisionBackend` trait (`backend.rs:101`) extends the interface for
image-input models with a single `generate_vision` method. Each provider
backend implements one or both traits — see the impl block at
`backend.rs:119` through `backend.rs:425`.

## Provider backends

Eight backends are defined, one per `ProviderId` variant:
`DeepInfraBackend` (`deepinfra_backend.rs:21`), `FalBackend`
(`fal_backend.rs:26`), `OpenRouterBackend` (`openrouter_backend.rs:22`),
`TogetherBackend` (`together_backend.rs:18`), `RunpodBackend`
(`runpod_backend.rs:16`), `KiloCodeBackend` (`kilocode_backend.rs:28`),
`OllamaBackend` (`ollama_backend.rs:51`), and `ClineBackend`
(`cline_backend.rs:23`). The `InferenceRouter` holds an `Option` for each
backend, constructing only those for which `Backend::new` succeeds (i.e.
API keys or base URLs are configured).

## See also

- [hkask-inference How-to](./how-to.md): configuring a new provider.
- [hkask-types Reference](../hkask-types/reference.md): the `InferencePort`
  trait that wraps this router in the bridge layer.
- [`kask/docs/explanation/fusion-mode.md`](../../explanation/fusion-mode.md):
  multi-model deliberation that uses this router.

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The backend-trait abstraction that allows multiple providers behind a single router.
