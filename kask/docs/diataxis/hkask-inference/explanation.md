---
title: "hkask-inference — Explanation: Provider Selection Rationale"
audience: [architects, developers]
last_updated: 2026-07-29
version: "0.2.0"
status: "Active"
domain: "Inference"
mds_categories: [trust, curation]
---

# hkask-inference — Explanation: Provider Selection Rationale

The `InferenceRouter` supports eight providers rather than a single
hardcoded backend. This design exists because different providers have
different cost, latency, and capability profiles. The router lets the
caller choose the provider by prefixing the model name, which keeps the
selection explicit and auditable.

This crate is the MCP-server-local inference router, not the primary
inference path for zed-kask user-facing chat. MCP servers that need their
own inference (media generation, skill execution) call this router
directly. User-facing chat goes through zed's `LanguageModelRegistry`
via `kask_bridge`. The long-term plan is to replace this crate with an
`InferencePort` adapter over zed's `LanguageModel`, but keeping it
unblocks the MCP servers immediately.

## Source citations

| Symbol | Location |
|--------|----------|
| `ProviderId` enum | `kask/crates/hkask-inference/src/config.rs:44` |
| `InferenceRouter` | `kask/crates/hkask-inference/src/inference_router/mod.rs:52` |
| `ChatBackend` trait | `kask/crates/hkask-inference/src/inference_router/backend.rs:51` |
| `parse_from_model` | `kask/crates/hkask-inference/src/config.rs:86` |
| `looks_like_prefix` | `kask/crates/hkask-inference/src/config.rs:128` |

## Provider selection flow

```mermaid
stateDiagram-v2
    [*] --> ParsePrefix: receive model name
    ParsePrefix --> Dispatch: prefix matches PREFIXES
    ParsePrefix --> Reject: looks_like_prefix but unknown
    ParsePrefix --> Default: no prefix shape
    Default --> Dispatch: use default_provider
    Reject --> [*]: return error
    Dispatch --> [*]: backend.generate(model, prompt)
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-INF-004
verified_date: 2026-07-29
verified_against: kask/crates/hkask-inference/src/config.rs:44,86,128; kask/crates/hkask-inference/src/inference_router/mod.rs:52; kask/crates/hkask-inference/src/inference_router/backend.rs:51
status: VERIFIED
-->

## Why prefix-based selection

Prefix-based selection (`DeepInfra/model`, `fal.ai/model`) makes the
provider choice visible in the model name string. This is auditable: a log
entry or span that records the model name also records the provider. A
configuration-based approach (where the provider is selected by a separate
setting) would hide the provider from the model name, making audit harder.

The router rejects unrecognized prefixes explicitly via
`looks_like_prefix` (`config.rs:128`) rather than silently routing them to
the default provider as a garbage model name. This is a fail-fast
property: a typo like `Deepinfra/model` (wrong casing) produces an error,
not a silent dispatch to the default.

## See also

- [hkask-inference Reference](./reference.md): class diagram of the router.
- [hkask-inference Tutorial](./tutorial.md): routing your first request.

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The backend-trait abstraction that allows multiple providers behind a single router.
