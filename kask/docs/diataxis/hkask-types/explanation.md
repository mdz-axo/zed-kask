---
title: "hkask-types — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Foundation"
mds_categories: [trust, curation]
---

# hkask-types — Explanation

`hkask-types` exists to solve a boundary problem. hKask is compiled in-process
inside zed-kask, but the kask crates must not depend on zed's internal types,
on concrete storage backends, or on `hkask-tool-port` (which would create a
dependency cycle). The foundation crate defines the contracts that mediate
between these worlds: kask crates depend on abstractions, and `kask_bridge`,
`hkask-storage`, and `hkask-regulation` provide the
adapters. This is the hexagonal architecture pattern applied at the crate
boundary.[^cockburn]

## Source citations

| Symbol | Location |
|--------|----------|
| Crate root, `forbid(unsafe_code)`, module list | `kask/crates/hkask-types/src/hkask_types.rs:1,6-38` |
| `pub use ports::*` re-export | `kask/crates/hkask-types/src/hkask_types.rs:60` |
| Cycle-prevention note (`must NOT depend on hkask-tool-port`) | `kask/crates/hkask-types/Cargo.toml:13` |
| `resolve_data_dir` (internal-data regulator) | `kask/crates/hkask-types/src/agent_paths.rs:63` |
| `resolve_under_data_dir` (delegates to regulator) | `kask/crates/hkask-types/src/agent_paths.rs:99` |
| `resolve_artifacts_dir` (user-artifacts regulator) | `kask/crates/hkask-types/src/agent_paths.rs:120` |
| `agent_db` (renamed from `agent_pod_db`) | `kask/crates/hkask-types/src/agent_paths.rs:198` |
| `sanitize_name` (path-traversal guard) | `kask/crates/hkask-types/src/agent_paths.rs:209` |
| `InferencePort` trait | `kask/crates/hkask-types/src/ports/inference_port.rs:147` |
| `MemoryPort` trait | `kask/crates/hkask-types/src/ports/memory_port.rs:111` |
| `RegulationSink` trait | `kask/crates/hkask-types/src/event.rs:748` |
| `SpanNamespace` (validated namespace) | `kask/crates/hkask-types/src/event.rs:65` |
| `CANONICAL_NAMESPACES` (single source of truth, private) | `kask/crates/hkask-types/src/event.rs:75` |
| `Id<T: IdKind>` sealed-marker pattern | `kask/crates/hkask-types/src/id/core.rs:13,20` |
| `WebID::for_agent_name` (canonical derivation) | `kask/crates/hkask-types/src/id/webid.rs:42` |
| `InfrastructureError` (no catch-all variant) | `kask/crates/hkask-types/src/error.rs:117` |
| `McpErrorKind::is_retryable` | `kask/crates/hkask-types/src/error.rs:261` |
| `AnyJsonValue` (Ollama/Gemini strict-schema fix) | `kask/crates/hkask-types/src/tool_schema.rs:46` |

## Why the foundation crate is structured this way

The kask workspace compiles as a set of library crates. These crates need to
call LLM APIs, persist turns to memory, invoke tools, emit Regulation events,
and resolve per-agent storage paths. If the kask crates depended directly on
zed's `LanguageModel` or thread-memory types, the kask workspace would become
unbuildable outside zed-kask, and every zed internal change would break kask.
If they depended directly on `rusqlite`, every transitive consumer would
inherit heavy native deps.

`hkask-types` breaks this coupling three ways:

1. **Port traits** (`ports/`) define *what* an infrastructure backend does
   without naming a backend. `InferencePort` (`inference_port.rs:147`) defines
   generation, streaming, embedding, and media. `MemoryPort`
   (`memory_port.rs:111`) defines turn ingestion and snippet recall. The kask
   crates depend on these traits; `kask_bridge` provides the concrete
   adapters that implement them against zed's types.
2. **Value types** (`error.rs`, `visibility.rs`, `document.rs`, `corpus.rs`,
   `hmem_ontology.rs`, `secret.rs`) carry domain semantics with zero heavy
   deps. `InfrastructureError` (`error.rs:117`) is
   `Clone + PartialEq + Eq + Serialize + Deserialize` so it can cross crate
   boundaries without dragging `rusqlite` (the `From<rusqlite::Error>` impl
   is behind the opt-in `sql` feature at `error.rs:202-203`).
3. **Path helpers** (`agent_paths.rs`) centralize the data-dir and
   artifacts-dir fallback chains in two regulators (`resolve_data_dir` at
   `agent_paths.rs:63`, `resolve_artifacts_dir` at `agent_paths.rs:120`) so
   class directories cannot drift apart across helpers.

The crate forbids `unsafe` code (`hkask_types.rs:1`) and must not depend on
`hkask-tool-port` (`Cargo.toml:13`) — that would create a cycle, since
`hkask-tool-port` needs the foundation identifiers (`tool_port.rs:4` imports
`hkask_types::NotFound`).

## The single-regulator principle for paths

`resolve_data_dir` is the single regulator for the internal data-dir fallback
chain: `HKASK_DATA_DIR` (honored only when absolute or `.`-prefixed — a
relative value is treated as misconfig and falls through) →
`$XDG_DATA_HOME/zed-kask` → `$HOME/.local/share/zed-kask` → CWD with a
`warn!` (`agent_paths.rs:63-89`). `resolve_under_data_dir`
(`agent_paths.rs:99`) delegates to it rather than duplicating the chain.
This is a direct fix for F4: the two helpers previously duplicated the chain
but disagreed on whether to honor a relative `HKASK_DATA_DIR`, which could
split agent DBs across two trees (recorded at `agent_paths.rs:93-97`).

A second regulator, `resolve_artifacts_dir` (`agent_paths.rs:120`), governs
the user-facing artifacts tree (`zk-data` under the documents dir). It exists
because reports and exports should live in a visible, intuitive location —
not buried in a hidden XDG data directory (`agent_paths.rs:106-110`). It
applies the same misconfig discipline to `HKASK_ARTIFACTS_DIR`.

```mermaid
stateDiagram-v2
    [*] --> CheckHKASK
    CheckHKASK --> AbsoluteOrDot: HKASK_DATA_DIR set + absolute/.-prefixed
    CheckHKASK --> CheckXDG: relative or unset
    CheckXDG --> UseXDG: XDG_DATA_HOME set
    CheckXDG --> CheckHome: unset
    CheckHome --> UseHome: HOME set
    CheckHome --> UseCWD: unset
    AbsoluteOrDot --> [*]: use HKASK_DATA_DIR
    UseXDG --> [*]: $XDG_DATA_HOME/zed-kask
    UseHome --> [*]: $HOME/.local/share/zed-kask
    UseCWD --> [*]: CWD + warn!
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-008
verified_date: 2026-08-28
verified_against: kask/crates/hkask-types/src/agent_paths.rs:63-89,99-101
status: VERIFIED
-->

`sanitize_name` (`agent_paths.rs:209`) is the path-traversal guard every
user-controlled path segment passes through. It replaces `/ \ : * ? " < > | ( )` and space with hyphens, collapses consecutive dashes, trims
leading/trailing dashes, and substitutes `"unnamed"` for names that sanitize
to `.` or `..`. `agent_db` (`agent_paths.rs:198`) produces `{name}.db` (e.g.
`agents/curator/curator.db`), not `pod.db` — the "pod" concept was
deprecated and the helper renamed (`agent_paths.rs:195-197`).

## Why port traits use `Pin<Box<dyn Future>>` instead of `async_trait`

The `InferencePort` trait (`inference_port.rs:147`) returns
`Pin<Box<dyn Future<Output = ...> + Send + 'a>>` from every async method
rather than using the `async_trait` macro. `async_trait` desugars to
`Pin<Box<dyn Future>>` anyway, but declaring it explicitly keeps the trait
object-safe without a macro dependency and lets callers hold
`Arc<dyn InferencePort>` directly. The blanket impl for
`Arc<dyn InferencePort>` at `inference_port.rs:386` delegates every method
to `self.as_ref()`, so consumers can hold a shared handle without paying
for the indirection at every call site.

When a return type grows complex, the crate extracts a named future alias —
`EmbedFuture` (`inference_port.rs:17`), `MediaFuture`
(`inference_port.rs:24`), `MemoryFuture` (`memory_port.rs:98`) — to stay
under clippy's `type_complexity` threshold. This is the established pattern
for any new port trait with a non-trivial async return.

## Why the Regulation event substrate is a shared observability layer

`RegulationRecord` (`event.rs:16`) is the cybernetic audit trail emitted by
all loops. It is not owned by any single loop — it is the shared observability
substrate that the Regulation loop senses and the Curator audits. The
`SpanNamespace` (`event.rs:65`) is constructed via `SpanNamespace::new()`
(`event.rs:455`) or `parse()` (`event.rs:471`), both of which validate
against `CANONICAL_NAMESPACES` (`event.rs:75`), the private single source of
truth for canonical Regulation spans. The `reg.*` prefix is reserved: every
`reg.*` tracing target MUST be registered (module doc, `regulation.rs:7-11`).
Typed domain enums bridge onto the same validation path via
`TryFrom<RegulationSpan> for SpanNamespace` (`event.rs:604-617`), which
routes through the shared `from_str_validated` constructor
(`event.rs:596-599`).

`SpanKind` (`event.rs:670`) enumerates canonical (namespace, path) pairs so
construction sites use `Span::from_kind()` instead of string literals, and
`SpanCategory` (`event.rs:529`) is the typed dispatch key for
span-category-dependent logic such as decay configuration. This keeps the
canonical namespace set as the single validator while decoupling domain
callers from stringly-typed spans.

```mermaid
sequenceDiagram
    participant Loop as Regulation Loop
    participant Span as typed span enum
    participant NS as SpanNamespace
    participant Sink as RegulationSink
    participant Archive as RegulationArchive

    Loop->>Span: construct variant
    Span->>NS: TryFrom<RegulationSpan> → from_str_validated
    NS->>NS: validate against CANONICAL_NAMESPACES
    alt canonical
        NS-->>Span: Ok(SpanNamespace)
        Span->>Span: Span::from_kind / Span::new
        Span->>Sink: persist(RegulationRecord)
        Sink->>Archive: write record
        Archive-->>Sink: Ok
    else non-canonical
        NS-->>Span: Err("not registered in CANONICAL_NAMESPACES")
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-009
verified_date: 2026-08-28
verified_against: kask/crates/hkask-types/src/event.rs:65,75,455,471,596-617,670,748; kask/crates/hkask-types/src/regulation.rs:7-11,108
status: VERIFIED
-->

## Why identifiers are a sealed phantom-generic newtype

`Id<T: IdKind>` (`id/core.rs:20`) wraps a `Uuid` with a phantom `T` marker.
The `IdKind` trait (`id/core.rs:13`) is sealed by a private `Sealed`
supertrait (`id/core.rs:8`), so external crates cannot introduce new kinds.
Each domain entity gets a strongly typed alias (`TemplateID`, `BotID`,
`HMemId`, `EventID`, `GoalID`, `TaskId`, etc. — `id/core.rs:177-192`;
`GoalID` and `UserID` are behind the `sql` feature). This prevents identifier
confusion at compile time: you cannot pass a `BotID` where a `TemplateID` is
expected, even though both wrap a `Uuid`.

`WebID` (`id/webid.rs:9`) is the agent identifier. `for_agent_name`
(`id/webid.rs:42`) is the canonical agent-name → WebID derivation used
across CLI, API, REPL, and `AgentService` — it delegates to `from_persona`
(`id/webid.rs:55`), a UUID v5 derivation with a fixed namespace. Same agent
name → same WebID, deterministically. `redacted_display`
(`id/webid.rs:78`) shows only the first 8 hex chars for INFO-level logging,
preventing full UUID leakage in logs. `From<BotID> for WebID`
(`id/webid.rs:103`) bridges the two identifier types without re-deriving.

## Why the error taxonomy has no catch-all

`InfrastructureError` (`error.rs:117`) is `#[non_exhaustive]` and has no
`Other(String)` or `Internal(String)` variant — every variant is a distinct
recovery category. This forces downstream code to pattern-match on the
actual failure mode rather than collapsing everything into a string.
`McpErrorKind` (`error.rs:231`) follows the same discipline:
`is_retryable` (`error.rs:261`) returns true only for `Unavailable`,
`Timeout`, `RateLimited`. These predicates let the Regulation loop make
automated retry decisions without re-parsing error strings; escalation
policy is decided by consumers from the typed variants
(`PermissionDenied`, `FailedPrecondition`) rather than a
`requires_intervention` predicate, which no longer exists in this crate.

`DbError` (`error.rs:57`) is a pure type with no external deps beyond
`thiserror` + `serde`, both already in `hkask-types`. The
`From<rusqlite::Error>` impl is behind the opt-in `sql` feature
(`error.rs:202-203`) so downstream crates without `rusqlite` can still
construct `InfrastructureError::database(String)` via the `database`
constructor (`error.rs:139`).

## Why `AnyJsonValue` exists

`schemars`'s `impl JsonSchema for serde_json::Value` returns the bare boolean
`true` (valid JSON Schema meaning "accept any value"). Ollama's Go API decodes
each tool's `parameters.properties` as a struct and rejects boolean schemas
with a `400 cannot unmarshal bool into ... of type api.ToolProperty`. Google
Gemini's protobuf `Schema` has the same class of failure. One
`serde_json::Value`-typed field in any enabled tool's schema makes Ollama
fail the entire chat-completion request.

`AnyJsonValue` (`tool_schema.rs:46`) is a transparent `Value` wrapper whose
`JsonSchema` emits the empty object `{}` — equally permissive but
JSON-object-shaped so strict tool-schema decoders accept it. The wire value
is unchanged (any JSON); only the generated tool input schema differs. The
type lives in `hkask-types` (rather than `hkask-mcp-server`) so pure domain
crates can use it without pulling in heavy transitive deps.
`find_boolean_schema_positions` (`tool_schema.rs:107`) scans a generated
schema for bare booleans in schema-valued positions so MCP server
tool-input tests can assert the result is empty at CI before a future
`serde_json::Value`-typed input breaks any strict provider.

## See also

- [hkask-types Reference](./reference.md): class diagram of every port and
  companion type.
- [hkask-types Tutorial](./tutorial.md): reading the foundation crate.
- [hkask-types How-to](./how-to.md): adding a new path helper or port trait.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P4 (clear boundaries), P5.4 (dual-axis ontology), P9 (feedback loops) that
  the foundation crate enforces.

---

[^cockburn]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>. The ports-and-adapters pattern: core logic depends on traits, infrastructure provides implementations, and the composition root wires them together.
