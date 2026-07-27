---
title: "hkask-inference — Explanation: Provider Selection Rationale"
audience: [architects, developers]
last_updated: 2026-07-27
version: "0.1.0"
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

## Source citations

| Symbol | Location |
|--------|----------|
| `ProviderId` enum | `kask/crates/hkask-inference/src/config.rs:43` |
| `InferenceRouter` | `kask/crates/hkask-inference/src/inference_router/mod.rs:52` |
| `ChatBackend` trait | `kask/crates/hkask-inference/src/inference_router/backend.rs:51` |

## Provider selection flow

```mermaid
stateDiagram-v2
    [*] --> ParsePrefix: receive model name
    ParsePrefix --> Dispatch: prefix matches ProviderId
    ParsePrefix --> Default: no prefix
    Default --> Dispatch: use default_provider
    Dispatch --> [*]: backend.generate(model, prompt)
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-INF-004
verified_date: 2026-07-27
verified_against: kask/crates/hkask-inference/src/config.rs:43; kask/crates/hkask-inference/src/inference_router/mod.rs:52; kask/crates/hkask-inference/src/inference_router/backend.rs:51
status: VERIFIED
-->

## Why prefix-based selection

Prefix-based selection (`DeepInfra/model`, `OpenRouter/model`) makes the
provider choice visible in the model name string. This is auditable: a log
entry or span that records the model name also records the provider. A
configuration-based approach (where the provider is selected by a separate
setting) would hide the provider from the model name, making audit harder.

## See also

- [hkask-inference Reference](./reference.md): class diagram of the router.
- [hkask-inference Tutorial](./tutorial.md): routing your first request.

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The backend-trait abstraction that allows multiple providers behind a single router.
