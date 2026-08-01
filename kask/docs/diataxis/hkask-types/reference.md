---
title: "hkask-types — Reference"
audience: [developers, architects, agents]
last_updated: 2026-07-29
version: "0.1.1"
status: "Active"
domain: "Foundation"
mds_categories: [domain, composition]
---

# hkask-types — Reference

`hkask-types` is the foundation crate of the hKask workspace. It defines the
shared domain types, identifier newtypes, and hexagonal port traits that every
downstream kask crate depends on. The crate forbids `unsafe` code and exports
no implementations of its own port traits. It defines the abstractions;
implementations live in `hkask-storage`, `hkask-regulation`, `kask_bridge`,
and `hkask-templates`.

## Source citations

| Symbol | Location |
|--------|----------|
| Crate root, module list | `kask/crates/hkask-types/src/hkask_types.rs:1` |
| `pub use ports::*` re-export | `kask/crates/hkask-types/src/hkask_types.rs:72` |
| `InferencePort` trait | `kask/crates/hkask-types/src/ports/inference_port.rs:86` |
| `MemoryPort` trait | `kask/crates/hkask-types/src/ports/memory_port.rs:108` |
| `LedgerObserver` trait | `kask/crates/hkask-types/src/ports/regulation.rs:64` |
| `SkillRegistryIndex` trait | `kask/crates/hkask-types/src/ports/registry.rs:288` |
| `RegistryIndex` trait | `kask/crates/hkask-types/src/ports/registry.rs:311` |
| `Id<T: IdKind>` newtype | `kask/crates/hkask-types/src/id/core.rs:20` |
| `ToolPort` trait (in `hkask-capability`, not this crate) | `kask/crates/hkask-capability/src/tool_port.rs:47` |

## Port trait hierarchy

The `ports/` module defines six port traits organized into four clusters.
Each port is a `Send + Sync` trait that abstracts an infrastructure boundary.
Downstream crates implement these traits against concrete backends. The
`ToolPort` trait lives in `hkask-capability` rather than this crate because it
carries OCAP semantics; it is included here for context.

```mermaid
classDiagram
    class InferencePort {
        <<interface>>
        +generate(prompt, parameters, tools) Future
        +generate_with_model(prompt, parameters, model, tools) Future
        +generate_with_messages(messages, parameters, model, tools) Future
    }
    class MemoryPort {
        <<interface>>
        +ingest_turn(record) MemoryFuture
        +recall_context(query, limit) MemoryFuture
        +recall_thread(thread_id, limit) MemoryFuture
    }
    class LedgerObserver {
        <<interface>>
        +interest_mask() Vec~SpanNamespace~
        +on_event(event)
        +on_depletion(signal)
        +on_backpressure(signal)
    }
    class SkillRegistryIndex {
        <<interface>>
        +register_skill(skill) Result
        +get_skill(id) Option~Skill~
        +list_skills() Vec~Skill~
        +list_skills_visible_to(visibility) Vec~Skill~
    }
    class RegistryIndex {
        <<interface>>
        +list(domain_hint) Vec~RegistryEntry~
        +get(id) Result~RegistryEntry~
    }
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, token) ToolFuture
    }

    InferencePort <|.. LanguageModelInferencePort
    InferencePort <|.. GuardedInferencePort
    InferencePort <|.. MultiModelInferencePort
    MemoryPort <|.. LoggingMemoryPort
    MemoryPort <|.. RealMemoryPort
    LedgerObserver <|.. RegulationLedger
    SkillRegistryIndex <|.. Registry
    RegistryIndex <|.. Registry
    ToolPort <|.. BridgeToolPort
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-TYPES-001
verified_date: 2026-07-29
verified_against: kask/crates/hkask-types/src/ports/inference_port.rs:86; kask/crates/hkask-types/src/ports/memory_port.rs:108; kask/crates/hkask-types/src/ports/regulation.rs:64; kask/crates/hkask-types/src/ports/registry.rs:288,311; kask/crates/hkask-capability/src/tool_port.rs:47
status: VERIFIED
-->

## Port clusters

The six ports group into four functional clusters by the infrastructure
boundary they abstract. (The former Persistence cluster's port traits were
removed; `EmbeddingStore` and `EscalationQueue` are now used directly — see
the Persistence cluster section below.)

### Inference cluster

`InferencePort` (`ports/inference_port.rs:86`) abstracts LLM generation. The
trait exposes three methods: `generate` (single-prompt), `generate_with_model`
(with optional model override), and `generate_with_messages` (multi-turn with
explicit `ChatMessage` array). All return `Pin<Box<dyn Future + Send>>`. The
companion types `ModelEntry`, `InferenceStreamChunk`, `InferenceResult`,
`ChatMessage`, and `ChatToolDefinition` live in the same file and
`ports/inference_types.rs`. Implementors: `LanguageModelInferencePort` in
`kask_bridge/src/inference.rs:246` (wraps zed's `LanguageModel`),
`MultiModelInferencePort` in `kask_bridge/src/fusion_model.rs:338` (Fusion
multi-provider), and `GuardedInferencePort` in
`hkask-guard/src/guarded_inference.rs:56` (decorator that adds content
scanning).

### Memory cluster

`MemoryPort` (`ports/memory_port.rs:108`) abstracts turn ingestion and snippet
recall. The trait exposes `ingest_turn`, `recall_context` (semantic + episodic
recall by query), and `recall_thread` (recall by exact thread ID). The
companion types `TurnRecord`, `MemorySnippet`, `MemoryError`, and the
`MemoryFuture` type alias live in the same file. Implementors:
`LoggingMemoryPort` in `kask_bridge/src/memory.rs:49` (no-op placeholder when
no DB is configured), `RealMemoryPort` in `kask_bridge/src/memory.rs:695`
(SQLite-backed), and `BridgeMemoryPort` in `kask_bridge/src/memory.rs:1484`
(adapts `MemoryPort` to zed's `agent::ThreadMemoryPort` trait — note: this
adapter implements zed's trait, not hKask's).

### Regulation cluster

`LedgerObserver` (`ports/regulation.rs:64`) receives Regulation events via
`interest_mask`-filtered `on_event`/`on_depletion`/`on_backpressure` callbacks.
`RegulationArchive` (in `hkask-storage/src/regulation_store.rs:505`) persists and replays Regulation
records (`query_algedonic`, `replay_weighted`, `persist_cursor`,
`load_cursor`, `query_by_namespace`) and is used directly by consumers (no
port trait). `WalletManager` (in `hkask-regulation/src/wallet_manager.rs:176`)
manages gas-budget balance, encumbrance, and settlement (`gas_to_rjoules`,
`get_encumbrance`, `can_afford`, `consume`, `settle_rjoules`) and is used
directly by consumers (no port trait). `RegulationLedger` (as a
`LedgerObserver` subscriber bus) lives in `hkask-regulation/src/runtime.rs:405`.

### Persistence cluster

Two concrete storage backends are used directly (no port traits).
`EmbeddingStore` (in `hkask-storage/src/embeddings.rs:629`) stores, retrieves,
searches, and deletes vector embeddings. `EscalationQueue` (in
`hkask-storage/src/escalation.rs:402`) manages escalation records (`list_pending`,
`get`, `resolve`, `dismiss`, `persist_batch`, `add`). The runtime embedding
port is `LanguageModelEmbeddingPort` in `kask_bridge` (unchanged).

(`ConsentPort` / `ConsentStore` were removed — consent records are no longer persisted.)

### Registry cluster

`SkillRegistryIndex` (`ports/registry.rs:288`) and `RegistryIndex`
(`ports/registry.rs:311`) abstract the skill and template registry. The
companion types `Skill`, `RegistryEntry`, `SkillZone`, and `RegistryError`
live in the same file. Implementor: `Registry` in
`hkask-templates/src/registry.rs:523` (in-memory) and the SQLite-backed
`SqliteRegistry` in `hkask-templates/src/registry_sqlite.rs`.

## Identifier newtypes

The `id/` module defines a generic `Id<T: IdKind>` newtype
(`id/core.rs:20`) parameterized by a phantom `IdKind` marker. Each domain
entity gets a strongly typed identifier: `WebID`, `HMemId`, `GoalID`,
`TemplateID`, `TaskId`, `BoardId`, `WalletId`, `ApiKeyId`, and others. The
`IdKind` trait (`id/core.rs`) is implemented by empty marker enums
(`TemplateKind`, `BotKind`, `TripleKind`, etc.) that carry no runtime data.
This pattern prevents identifier confusion at compile time.

## Re-exports

The crate root (`hkask_types.rs:72`) re-exports the entire `ports` module via
`pub use ports::*`. Downstream crates depend on `hkask-types` and receive all
port traits, identifier newtypes, and domain types (`HMemEntry`, `WebID`,
`RJoule`, `ObservableSpan`, `RegulationRecord`, `VoiceDesign`,
and the wallet, curation, and loop types) through this single dependency.

## See also

- [hkask-types Explanation](./explanation.md): sequence diagram of how the
  port traits mediate between zed and kask at the composition root.
- [hkask-capability Reference](../hkask-capability/reference.md): the
  `ToolPort` trait and the capability-match gate in `McpRuntime::invoke`.
- [`kask/docs/architecture/zed-host-architecture-plan.md`](../../architecture/zed-host-architecture-plan.md):
  the D1–D10 integration seams that consume these port traits.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P5.4 dual-axis ontology (PKO + DC+BIBO) that grounds the domain types.

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The ports-and-adapters pattern that the `ports/` module implements: core logic depends on traits, infrastructure provides implementations.

[^newtype]: Rust Community. (2024). *Rust API Guidelines — Newtype Pattern.* <https://rust-lang.github.io/api-guidelines/type-conventions.html#c-newtype>. The `Id<T: IdKind>` newtype pattern that prevents identifier confusion at compile time.
