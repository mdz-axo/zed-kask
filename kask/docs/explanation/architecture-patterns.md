---
title: "Architecture Patterns — Hexagonal, Loom-and-Thread, Good Regulator, VSM, Dual-Axis Ontology"
audience: [architects, developers]
last_updated: 2026-07-12
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, curation]
---

# Architecture Patterns

This document consolidates five architectural patterns that define hKask's structural identity and the API surface that exposes them. Each pattern exists because a specific project constraint demands it — not for aesthetic or conventional reasons. The patterns are: hexagonal ports and adapters, the loom-and-thread separation, the Good Regulator theorem, the Viable System Model mapping, and the dual-axis ontology. The final section documents the API surface equivalence (P3) that makes every architectural boundary equally accessible from CLI, API, and MCP.

---

## 1. Hexagonal Ports and Adapters

### Statement

The hexagonal architecture in hKask is not a pattern adopted for aesthetics. It exists because the system's core regulatory logic — the Regulation, the Curator, the Inference loop — must function identically whether it runs against a local SQLite database on a developer's laptop. More importantly, it must be testable without any of those backends at all.

### Evidence

In standard hexagonal architecture, the domain core is surrounded by ports (interfaces the core defines) and adapters (implementations that satisfy those interfaces). In Rust, ports are **traits**, and adapters are **structs that implement those traits**. The rule is simple: domain crates define the traits; infrastructure crates provide the implementations.

This design exists because hKask's dependency graph imposes a strict Authority DAG. Domain crates (`hkask-regulation`, `hkask-pods`, `hkask-inference`) must not depend on infrastructure crates (`hkask-storage`, `hkask-mcp`). The port traits in `hkask-types` are the only shared dependency — every domain crate imports from `hkask-types`, and every infrastructure crate implements against it. There is no other coupling path.

As the crate-level documentation in `crates/hkask-types/src/lib.rs` states: "Port traits that enable crates to depend on abstractions rather than concrete implementations. Per the Authority DAG, domain crates depend on these port traits (not on each other)."

#### The 17 Port Traits

The `hkask-types` crate defines **17 trait contracts**, each guarding a distinct architectural boundary. They group into five concerns:

| Concern | Traits |
|---------|--------|
| Regulation regulation | `CircuitBreakerPort`, `LedgerStoragePort`, `LedgerObserver` |
| Inference and tools | `InferencePort`, `ToolPort` |
| Governance and pipelines | `ConsentPort`, `EscalationPort`, `WalletBudgetPort`, `StepExecutor` |

The eight primary infrastructure boundaries are documented in detail below. The remaining nine are listed in §1.4.

**`InferencePort`** (`crates/hkask-types/src/inference_port.rs`) — The LLM invocation boundary. This is the most heavily used port — every agent pod, every Curator reflection, every template cascade eventually calls `generate()`. It uses `Pin<Box<dyn Future>>` rather than `async_trait` for object safety, enabling `Arc<dyn InferencePort>` dispatch at construction time. The trait provides default implementations for `generate_n()`, `generate_stream()`, `generate_with_model()`, and `generate_vision()` — all fall back to `generate()`, so a new backend only needs to implement one method. The concrete implementor is the `InferenceRouter` in `hkask-inference`, which multiplexes across multiple providers (DeepSeek, Anthropic, Groq, OpenAI, DeepInfra, Together AI, fal.ai, OpenRouter, KiloCode) with model routing, failover, and concurrency control.

**`ToolPort`** (`crates/hkask-types/src/tool.rs`) — The governance membrane for MCP tool invocation. Unlike `InferencePort`, this port has an authentication asymmetry: `discover_tools()` and `get_tool_info()` are intentionally unauthenticated — tool schemas are public metadata — but `invoke()` requires a `DelegationToken`. OCAP enforcement applies at the actuator boundary, not the sensor boundary. The concrete implementor is `McpDispatcher` in `hkask-mcp`. The error type, `ToolPortError`, encodes the governance envelope directly: `CapabilityDenied` (OCAP rejection), `EnergyBudgetExceeded` (gas depletion), `NotFound`, and `InvocationFailed`.

**`CircuitBreakerPort`** (`crates/hkask-types/src/regulation.rs`) — The circuit breaker boundary for the Cybernetics membrane. A minimal trait — `allow_request()`, `record_success()`, `record_failure()`, `state()` — that allows the Inference loop to use circuit breaking without depending on `hkask-regulation`. The concrete implementor is `CircuitBreaker` in `hkask-regulation`. When the Regulation detects elevated error rates above the `error_rate_max` set-point (default: 30%), it opens the circuit and the inference loop stops sending requests.

**`LedgerStoragePort`** (`crates/hkask-types/src/regulation.rs`) — Storage abstraction for Regulation event queries. While `CircuitBreakerPort` is the actuator boundary, `LedgerStoragePort` is the memory boundary — it abstracts the `RegulationArchive` behind a trait so the cybernetic regulation layer (`GasReport`, `CalibratedEnergyEstimator`, `WalletGasCalibrator`) can be tested without a real SQLite database. It provides `query_algedonic()` for alert retrospectives, `replay_weighted()` for temporal decay-weighted event replay, and `persist_cursor()`/`load_cursor()` for crash recovery.

**`LedgerObserver`** (`crates/hkask-types/src/regulation.rs`) — The subscriber interface for Regulation events. Observers declare an `interest_mask()` of `SpanNamespace` values they care about, then receive `on_event()`, `on_depletion()`, and `on_backpressure()` callbacks. The concrete implementor in `hkask-inference` uses this to react to throttle and circuit-break signals.

**`ConsentPort`** (`crates/hkask-types/src/consent_port.rs`) — Decouples agent pods from the concrete `ConsentStore` in `hkask-storage`. A CRUD trait for consent records — `initialize_schema()`, `store()`, `list_active()` — that ensures the Affirmative Consent (P2) verification layer can be tested independently of the database schema.

**`EmbeddingPort`** (`crates/hkask-types/src/embedding_port.rs`) — The vector embedding storage boundary. Abstracts the concrete `EmbeddingStore` in `hkask-storage`. Provides `store()`, `get()`, `search()` (cosine similarity), and `delete()` — the four operations needed by the semantic memory loop to anchor triples in embedding space.

