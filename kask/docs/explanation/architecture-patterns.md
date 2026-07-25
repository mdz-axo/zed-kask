---
title: "Architecture Patterns — Hexagonal, Loom-and-Thread, Good Regulator, VSM, Dual-Axis Ontology"
audience: [architects, developers]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, curation]
---

# Architecture Patterns

This document consolidates five architectural patterns that define hKask's structural identity. Each pattern exists because a specific project constraint demands it — not for aesthetic or conventional reasons. The patterns are: hexagonal ports and adapters, the loom-and-thread separation, the Good Regulator theorem, the Viable System Model mapping, and the dual-axis ontology.

## In-process architecture

zed-kask is a fork of the Zed editor with hKask compiled in-process. There is no standalone user-facing `kask` CLI, no HTTP API, and no daemon. The hKask domain crates (`hkask-regulation`, `hkask-pods`, `hkask-capability`, `hkask-types`, `hkask-guard`, `hkask-templates`, etc.) define port traits and pure regulatory logic; the single bridge crate `kask_bridge` (D8) implements every adapter over zed's native types, and the composition root in `zed/src/main.rs` wires them together. The governing invariant — enforced in CI by `kask/scripts/check-hkask-no-zed-deps.sh` — is that **hKask crates never depend on zed-kask; zed-kask depends on hKask crates.**

The user-facing surfaces are all in-process: the Agent Panel (zed native, drives the Curator agent D2 and skills via `ManifestExecutor` D1), the Kask Panel (D10, per-MCP-server one-on-one window), the `magna-carta-verifier` skill, and a slim `kask` admin CLI for backup/wallet/repair/admin only. The old P3 "API surface equivalence" (CLI ≡ API ≡ MCP) is dead; the live equivalence is **MCP ≡ in-process surfaces**.

---

## 1. Hexagonal Ports and Adapters

### Statement

The hexagonal architecture in hKask is not a pattern adopted for aesthetics. It exists because the system's core regulatory logic — the Regulation, the Curator, the Inference loop — must function identically whether it runs against a local SQLite database on a developer's laptop or against zed's in-process thread store. More importantly, it must be testable without any of those backends at all.

### Evidence

In standard hexagonal architecture, the domain core is surrounded by ports (interfaces the core defines) and adapters (implementations that satisfy those interfaces). In Rust, ports are **traits**, and adapters are **structs that implement those traits**. The rule is simple: domain crates define the traits; infrastructure crates provide the implementations.

This design exists because hKask's dependency graph imposes a strict Authority DAG. Domain crates (`hkask-regulation`, `hkask-pods`, `hkask-inference`) must not depend on infrastructure crates (`hkask-storage`, `hkask-mcp`). The port traits in `hkask-types` are the only shared dependency — every domain crate imports from `hkask-types`, and every infrastructure crate implements against it. There is no other coupling path.

As the crate-level documentation in `crates/hkask-types/src/lib.rs` states: "Port traits that enable crates to depend on abstractions rather than concrete implementations. Per the Authority DAG, domain crates depend on these port traits (not on each other)."

### The port traits

The `hkask-types` crate defines the trait contracts that guard each architectural boundary. The table below lists the ports documented here. Other ports exist in the crate (e.g. `EscalationPort`, `StepExecutor`, `WalletBudgetPort`); this document covers the eight primary infrastructure boundaries that the bridge crate implements.

| Concern | Traits |
|---------|--------|
| Regulation regulation | `CircuitBreakerPort`, `LedgerStoragePort`, `LedgerObserver` |
| Inference and tools | `InferencePort`, `ToolPort` |
| Governance and pipelines | `ConsentPort`, `EmbeddingPort`, `MemoryPort` |

**`InferencePort`** (`crates/hkask-types/src/inference_port.rs`) — The LLM invocation boundary. This is the most heavily used port — every agent pod, every Curator reflection, every template cascade eventually calls `generate()`. It uses `Pin<Box<dyn Future>>` rather than `async_trait` for object safety, enabling `Arc<dyn InferencePort>` dispatch at construction time. The trait provides default implementations for `generate_n()`, `generate_stream()`, `generate_with_model()`, and `generate_vision()` — all fall back to `generate()`, so a new backend only needs to implement one method.

The concrete implementor in zed-kask is **`LanguageModelInferencePort`** in `kask_bridge`, which adapts zed's `LanguageModelRegistry` (the upstream inference router in `crates/language_model*`) to the hKask port. It is wrapped by **`GuardedInferencePort`** (D4) in `hkask-guard`, which applies the mandatory content-safety membrane (P3.1) to every call. The composition root in `zed/src/main.rs` constructs `LanguageModelInferencePort::new(model, async_cx)`, wraps it in `GuardedInferencePort::new(...)`, and injects the result into `ManifestExecutor` (D1), the Curator (D2), and the kask panel's `PanelScopedInference` (D10).

> **Note:** The old `InferenceRouter` in `hkask-inference` (multiplexing DeepSeek, Anthropic, Groq, OpenAI, DeepInfra, Together AI, fal.ai, OpenRouter, KiloCode) is **not** the user-surface inference path in zed-kask. `hkask-inference` is kept only for MCP-server-internal use. The user surface is zed's `LanguageModelRegistry` adapted through `kask_bridge`.

**`ToolPort`** (`crates/hkask-types/src/tool.rs`) — The governance membrane for MCP tool invocation. Unlike `InferencePort`, this port has an authentication asymmetry: `discover_tools()` and `get_tool_info()` are intentionally unauthenticated — tool schemas are public metadata — but `invoke()` requires a `DelegationToken`. OCAP enforcement applies at the actuator boundary, not the sensor boundary. The error type, `ToolPortError`, encodes the governance envelope directly: `CapabilityDenied` (OCAP rejection), `EnergyBudgetExceeded` (gas depletion), `NotFound`, and `InvocationFailed`.

The concrete implementor in zed-kask is **`BridgeToolPort`** in `kask_bridge`, which adapts zed's `McpRuntime` (the in-process MCP tool registry, D3) to the hKask port. The composition root constructs `BridgeToolPort::new(mcp_runtime.clone())` and injects it into `ManifestExecutor` and the kask panel's `PanelToolInvoker` (which mints a `DelegationToken` from the `a2a_secret` resolved at startup).

> **Note:** The old `McpDispatcher` in `hkask-mcp` is **not** the user-surface tool path in zed-kask. `BridgeToolPort` over `McpRuntime` is.

**`MemoryPort`** — The memory ingestion boundary. The concrete implementor in zed-kask is **`BridgeMemoryPort`** in `kask_bridge`, which adapts zed's `ThreadMemoryPort` (thread completion path) to the hKask port. The composition root installs a `LoggingMemoryPort` early (so the global hook is set before any thread completes a turn) and upgrades it to a real `BridgeMemoryPort` once the Zed user resolves (D6). This replaces the deleted `DaemonHandler`/`DaemonClient` memory-ingestion path — there is no daemon in zed-kask.

**`CircuitBreakerPort`** (`crates/hkask-types/src/regulation.rs`) — The circuit breaker boundary for the Cybernetics membrane. A minimal trait — `allow_request()`, `record_success()`, `record_failure()`, `state()` — that allows the Inference loop to use circuit breaking without depending on `hkask-regulation`. The concrete implementor is `CircuitBreaker` in `hkask-regulation`. When the Regulation detects elevated error rates above the `error_rate_max` set-point (default: 30%), it opens the circuit and the inference loop stops sending requests.

**`LedgerStoragePort`** (`crates/hkask-types/src/regulation.rs`) — Storage abstraction for Regulation event queries. While `CircuitBreakerPort` is the actuator boundary, `LedgerStoragePort` is the memory boundary — it abstracts the `RegulationArchive` behind a trait so the cybernetic regulation layer (`GasReport`, `CalibratedEnergyEstimator`, `WalletGasCalibrator`) can be tested without a real SQLite database. It provides `query_algedonic()` for alert retrospectives, `replay_weighted()` for temporal decay-weighted event replay, and `persist_cursor()`/`load_cursor()` for crash recovery.

**`LedgerObserver`** (`crates/hkask-types/src/regulation.rs`) — The subscriber interface for Regulation events. Observers declare an `interest_mask()` of `SpanNamespace` values they care about, then receive `on_event()`, `on_depletion()`, and `on_backpressure()` callbacks. The concrete implementor in `hkask-inference` uses this to react to throttle and circuit-break signals.

**`ConsentPort`** (`crates/hkask-types/src/consent_port.rs`) — Decouples agent pods from the concrete `ConsentStore` in `hkask-storage`. A CRUD trait for consent records — `initialize_schema()`, `store()`, `list_active()` — that ensures the Affirmative Consent (P2) verification layer can be tested independently of the database schema.

**`EmbeddingPort`** (`crates/hkask-types/src/embedding_port.rs`) — The vector embedding storage boundary. Abstracts the concrete `EmbeddingStore` in `hkask-storage`. Provides `store()`, `get()`, `search()` (cosine similarity), and `delete()` — the four operations needed by the semantic memory loop to anchor triples in embedding space.

### Implications

The hexagonal pattern is what makes the in-process architecture possible. Because every boundary is a port trait, zed-kask can implement all adapters in a single bridge crate (`kask_bridge`) without hKask crates ever learning that zed exists. The composition root is the only place that knows both sides. This is also what makes the deleted surfaces deletable: when `hkask-api` (HTTP) and `hkask-cli` (user CLI) were removed, no domain crate changed — only the surface adapters disappeared.

---

## 2. Loom-and-Thread Separation

### Statement

The loom-and-thread separation distinguishes the fixed regulatory machinery (the loom) from the declarative YAML manifests that drive behavior (the threads). The loom cannot be modified by manifests; manifests can only be woven through it.

### Evidence

The loom is the compiled Rust code: `GovernedTool`, `GovernedInference`, `CyberneticsLoop`, `ManifestExecutor`, the OCAP verification chain, the guard pipeline. These enforce invariants structurally — a manifest that declares `"gas_bypass": true` is a parse error; a manifest cannot reorder, skip, or bypass any step in the `GovernedTool` membrane.

The threads are the YAML manifests in `kask/registry/manifests/` (FlowDef cascades, kata definitions, QA scripts) and the skill registry in `.agents/skills/`. They declare *what* to do (steps, templates, gas caps, convergence thresholds); the loom decides *how* it is allowed to happen.

### Implications

This separation is why skills execute safely via `ManifestExecutor` (D1) from the Agent Panel without a separate `kask skill execute` CLI. The manifest is the thread; the executor is part of the loom. The same separation applies to kata execution via the kata-kanban MCP server (in-process) — there is no `kask kata start` CLI.

---

## 3. Good Regulator Theorem

### Statement

Conant and Ashby's Good Regulator theorem states that "every good regulator of a system must be a model of that system." hKask takes this literally: the Regulation must maintain an internal model of the system it regulates, or its control actions will be blind.

### Evidence

The Regulation's model is the `RegulationArchive` — a persistent, queryable record of every ν-event the system has emitted. `CyberneticsLoop::verify_impact()` re-senses the targeted metric after an action and compares pre- and post-action values, producing an `ImpactReport` with an `ActionDecision` (`Accept`, `Stage`, `Block`). The `SetPointCalibrator` queries the archive periodically and adjusts thresholds within bounds. The Curator's `MetacognitionLoop::sense()` reads via `query_algedonic` to build its model of system state.

### Implications

A regulator without a model is a thermostat in a sealed room — it acts on stale or absent information. The Regulation's model is what allows it to distinguish "action worked" from "action failed silently," and to escalate rather than repeat ineffective actions (the substitution ladder and stagnation detection).

---

## 4. Viable System Model (VSM) Mapping

### Statement

hKask's operational structure maps onto Stafford Beer's Viable System Model: five systems (S1–S5) that must all be present for an organization to be viable.

### Evidence

| VSM System | hKask Component | Role |
|------------|-----------------|------|
| S1 — Operations | Agent pods, `ManifestExecutor`, MCP servers | Do the work |
| S2 — Coordination | `CyberneticsLoop`, gas budgets, circuit breakers | Prevent oscillation |
| S3 — Control | `RegulationLedger`, `SetPoints`, variety monitors | Audit and optimize S1 |
| S3* — Audit | `SeamWatcher`, `SloManager`, `StorageGuardLoop` | Independent audit channel |
| S4 — Intelligence | Curator agent (D2), `MetacognitionLoop` | Scan environment, plan |
| S5 — Policy | Magna Carta (P1–P4), `CuratorHandle::system()` singleton | Identity and policy |

### Implications

The Curator is S4, not S5. It can recommend, calibrate, and escalate, but it cannot override sovereignty boundaries (P1–P4) — that is S5's role, embodied in the Magna Carta and the singleton `CuratorHandle`. The Curator's `issue_directive()` verifies `handle.can_write(&DataCategory::Public)` before acting; it never bypasses OCAP.

---

## 5. Dual-Axis Ontology

### Statement

hKask does not invent ontologies; it bridges to existing ones. The dual-axis ontology anchors every fact to two axes: a domain ontology (what kind of thing is this?) and a provenance ontology (where did this fact come from, and how certain is it?).

### Evidence

The domain axis bridges to established ontologies: Dublin Core (documents), BIBO (bibographic), FIBO/GOLEM/ESO (financial/economic), PKO (process), 5W1H (journalistic dimensions). The provenance axis classifies every statement by certainty level (IS / OUGHT / probabilistic / subjunctive) and provenance tier (Core / Dual-Axis / Domain Supplement). Conflict resolution uses ontology-anchored OT ranking.

### Implications

This is why hKask's guard and state diagrams anchor to OWASP LLM Top 10 rather than inventing their own threat taxonomy, and why the Curator's `condenser/condenser_score_saliency` scores event relevance via ontology graph proximity rather than ad-hoc heuristics. Bridging to existing ontologies keeps the system legible to external tools and auditable against external standards.

---

## References

- Conant, R. C., & Ashby, W. R. (1970). "Every good regulator of a system must be a model of that system." *International Journal of Systems Science*, 1(2), 89–97.
- Beer, S. (1979). *The Heart of Enterprise*. Wiley. Viable System Model.
- Ousterhout, J. (2018). *A Philosophy of Software Design*. Deep modules, interface minimalism.
- [zed-kask Architecture Plan](../architecture/zed-host-architecture-plan.md) — D1–D10 integration seams, bridge crate, composition root
- [zed-kask Architecture](../architecture/zed-kask-architecture.md) — composition root
- [Sovereignty and OCAP](sovereignty-and-ocap.md) — OCAP dispatch membrane, delegation token attenuation
- [Regulation and Loops](regulation-and-loops.md) — Cybernetic loop, set points, variety engineering
