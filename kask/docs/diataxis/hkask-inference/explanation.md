---
title: "hkask-inference — Explanation: Provider Selection Rationale"
audience: [architects, developers]
last_updated: 2026-08-04
version: "0.3.1"
status: "Active"
domain: "Inference"
mds_categories: [trust, curation]
---

# hkask-inference — Explanation: Provider Selection Rationale

This crate is the MCP-server-local inference layer, not the primary
inference path for zed-kask user-facing chat. It offers two `InferencePort`
implementations selected at startup by `resolve_inference_port()`
(`hkask_inference.rs`):

- `InferenceIpcClient` — delegates chat, vision, embedding, tool dispatch,
  and skill execution to zed's `LanguageModelRegistry` over a Unix socket
  (`HKASK_INFERENCE_SOCKET`). This is the primary path in zed-kask; the zed
  process holds the API keys and the guard, the MCP server child process
  holds none.
- `MediaRouter` — media generation only (image/video/speech/transcription)
  via a `ProviderRegistry` of `MediaProvider` backends. This is the fallback
  when the IPC socket is unavailable; its `InferencePort` impl returns a
  clear error for chat/vision/embed.

Provider selection is prefix-based: a caller chooses the provider by
prefixing the model name (`DeepInfra/...`, `AtlasCloud/...`, `OpenRouter/...`).
`ProviderId::parse_from_model` parses the prefix; an unprefixed name uses
`default_provider`. This keeps the provider choice explicit and auditable
— a span that records the model name also records the provider.

The long-term plan is to replace this crate with an `InferencePort` adapter
over zed's `LanguageModel`, but keeping it unblocks the MCP servers
immediately.

## Source citations

| Symbol | Location |
|--------|----------|
| `ProviderId` enum | `kask/crates/hkask-inference/src/config.rs:36` |
| `from_prefix_segment` | `kask/crates/hkask-inference/src/config.rs:103` |
| `MediaRouter` struct | `kask/crates/hkask-inference/src/media_router.rs:47` |
| `InferenceIpcClient` struct | `kask/crates/hkask-inference/src/inference_ipc_client.rs` |
| `resolve_inference_port` | `kask/crates/hkask-inference/src/hkask_inference.rs` |

## Provider selection flow

```mermaid
stateDiagram-v2
    [*] --> ParsePrefix: receive model name
    ParsePrefix --> Route: prefix matches PREFIXES
    ParsePrefix --> Default: no recognized prefix
    Default --> Route: use default_provider
    Route --> [*]: InferenceIpcClient (chat/vision/embed) or MediaRouter (media)
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-INF-004
verified_date: 2026-08-11
verified_against: kask/crates/hkask-inference/src/config.rs:36,103; kask/crates/hkask-inference/src/media_router.rs:47; kask/crates/hkask-inference/src/inference_ipc_client.rs
status: STALE
note: The ParsePrefix→Route prefix-routing transition describes the removed in-process InferenceRouter (parse_from_model has no production caller); rewrite pending the parse_from_model dead-surface decision. The Reject state was removed with looks_like_prefix.
-->

## Why prefix-based selection

Prefix-based selection (`DeepInfra/model`, `OpenRouter/model`) makes the
provider choice visible in the model name string. This is auditable: a log
entry or span that records the model name also records the provider. A
configuration-based approach (where the provider is selected by a separate
setting) would hide the provider from the model name, making audit harder.

Unrecognized prefixes are not rejected — the model name is passed through to
zed's `LanguageModelRegistry` (via the IPC bridge), which does the actual
provider routing. `from_prefix_segment` classifies a model name's provider
prefix segment for model-listing labels; it does not gate routing.

## See also

- [hkask-inference Reference](./reference.md): class diagram and backends.
- [hkask-inference Tutorial](./tutorial.md): routing your first request.
- [hkask-types Reference](../hkask-types/reference.md): the `InferencePort`
  trait that both implementations satisfy.

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The `InferencePort` boundary that allows multiple providers behind a single port.