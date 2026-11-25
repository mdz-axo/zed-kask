---
title: "hkask-types — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-11-18
version: "2.2.0"
status: "Active"
domain: "Foundation"
mds_categories: [trust, curation]
---

# hkask-types — Explanation

`hkask-types` owns the contracts that cross kask crate and process boundaries. It
keeps domain crates independent of Zed internals, storage implementations, and the
tool-port crate that already depends on these foundation types. This is the ports
and adapters pattern: consumers depend on stable contracts, while the composition
root supplies implementations.[^cockburn]

The crate forbids unsafe code and declares its complete module surface at
`kask/crates/hkask-types/src/hkask_types.rs:1-52`. Common cross-crate items and all
ports are re-exported at `kask/crates/hkask-types/src/hkask_types.rs:34-75`.

## Why boundaries are split by failure domain

The crate does not place every shared value in one undifferentiated module. Each
boundary has a contract shaped by how drift or failure would appear:

- `ports` defines object-safe inference, tool-dispatch, worktree-spawn, memory,
  embedding-error, and consolidation contracts
  (`kask/crates/hkask-types/src/ports.rs:7-23`).
- `event` and `regulation` define the typed observability substrate
  (`kask/crates/hkask-types/src/event.rs:15-28,258-330,359-515`;
  `kask/crates/hkask-types/src/regulation.rs:25-155`).
- `inference_ipc` defines the newline-delimited request/response protocol used by
  MCP child processes (`kask/crates/hkask-ipc-wire/src/inference_ipc.rs:1-47,53-120,122-250`).
- `agent_paths` centralizes internal-data and user-artifact path resolution
  (`kask/crates/hkask-types/src/agent_paths.rs:12-26,63-265`).
- `closed_vocab`, `ocr_health`, `server_env`, and `process_global` encode shared
  emission-noise normalization, cross-process health, child-environment, and hook-slot invariants
  (`kask/crates/hkask-types/src/closed_vocab.rs:1-40`;
  `kask/crates/hkask-types/src/ocr_health.rs:1-71`;
  `kask/crates/hkask-types/src/server_env.rs:1-59`;
  `kask/crates/hkask-types/src/process_global.rs:1-77`).
- `kanban_wire`, `media_wire`, and `nostr_wire` carry server/tool name constants so UI panels can invoke built-in MCP tools without linking the server crates (`kask/crates/hkask-types/src/hkask_types.rs:23-27`).

A boundary type belongs here when several crates must agree on its wire shape or
invariant without importing one another's implementation; domain-specific media admission caps (`media_limits`) and `ytdlp` helper types live in `hkask-media` (`kask/crates/hkask-media/src/hkask_media.rs:13,21`).

## Why closed vocabularies normalize emission noise before rejecting

`closed_vocab` centralizes the canonical emission-noise normalizer (`normalize_closed_vocab`) shared across MCP request decoders: surrounding whitespace, wrapping quotes, and ASCII case are normalized before matching against a closed enum set, while any value outside the declared vocabulary is rejected rather than coerced (`kask/crates/hkask-types/src/closed_vocab.rs:1-40`).

## Why OCR health is a file contract

The corpus MCP server is a child process, so its in-memory tracing does not enter
the host's Regulation ledger directly. `OcrHealthSnapshot` carries bounded silent-
failure timestamps, breaker state, and update time; `ocr_health_path()` resolves
one canonical shared file
(`kask/crates/hkask-types/src/ocr_health.rs:11-71`). The writer can update the file
atomically and the Zed-side reader can sense it on a Regulation tick without either
process depending on the other's implementation.

```mermaid
sequenceDiagram
    participant Corpus as Corpus child process
    participant File as ocr-health.json
    participant Bridge as Zed health source
    participant Regulation as CyberneticsLoop
    Corpus->>File: write OcrHealthSnapshot atomically
    Bridge->>File: read canonical snapshot path
    Bridge->>Regulation: report recent failures and breaker state
    Regulation->>Regulation: compare with set-point
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-008
verified_date: 2026-11-18
verified_against: kask/crates/hkask-types/src/ocr_health.rs:1-71; kask/crates/hkask-regulation/src/sensor_provider.rs:334-361@OcrHealthSensor
status: VERIFIED
-->

## Why `ServerEnv` and `ProcessGlobal` are types

`ServerEnv` makes the canonical MCP child-environment path visible in function
signatures. It wraps the composed map and exposes read/consume access, while its
documented construction path is `build_mcp_server_env`
(`kask/crates/hkask-types/src/server_env.rs:1-59`). This does not make canonical
construction type-system-private—the constructor crosses crates—but it makes a raw
map at a spawn boundary reviewable as a bypass.

`ProcessGlobal<T>` is the reusable re-settable hook slot. It stores
`Mutex<Option<T>>`, replaces or clears values through `set`, returns an owned clone
through `get`, and recovers poisoned locks
(`kask/crates/hkask-types/src/process_global.rs:35-77`). Clone-out-of-lock prevents
a caller from retaining the mutex across an await. This type is not for set-once
`OnceLock` startup hooks.

## Why the event substrate is shared

`RegulationRecord` is the audit event carried across loops and stores. `Span`
combines a validated `SpanNamespace` with a path; `SpanKind` supplies canonical
frequently-used pairs; `SpanCategory` supplies a typed dispatch classification;
`CyclePhase` identifies sense, compute, compare, or act
(`kask/crates/hkask-types/src/event.rs:15-28,258-459`).
`RegulationSink` is the persistence port at
`kask/crates/hkask-types/src/event.rs:497-515`.

```mermaid
flowchart TD
    K[SpanKind] --> S[Span::from_kind]
    N[Validated SpanNamespace] --> S
    S --> R[RegulationRecord]
    R --> P[RegulationSink]
    S --> C[SpanCategory dispatch]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-009
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/event.rs:15-28,60-65,258-330,359-515
status: VERIFIED
-->

The canonical namespace registry is private at
`kask/crates/hkask-types/src/event.rs:75-173`; construction validates names before
they become spans. The event surface includes inference circuit transition and
observed-recovery kinds at `kask/crates/hkask-types/src/event.rs:411-459`.

## Why IPC uses one request and one response envelope

`InferenceRequest` pairs a correlation ID, `InferenceMethod`, and
`InferenceParams`; `InferenceResponse` pairs the same ID with an untagged
`InferenceOutcome`
(`kask/crates/hkask-ipc-wire/src/inference_ipc.rs:87-156,159-345`). Current methods
cover generation, messages, vision, embedding, model listing, worktree-thread creation, reranking, System One decisions, and editor-side Nostr/Blossom signing. Current
outcomes cover a generation result, embeddings, model list, worktree thread, rerank scores, decision answers, signed Nostr event, or a typed error.

The protocol uses no separate batch envelope. Embedding already carries multiple
input texts in `InferenceParams::embed_texts`
(`kask/crates/hkask-ipc-wire/src/inference_ipc.rs:134-137`), while each IPC envelope
retains one correlation identity.

## See also

- [hkask-types reference — API surface and procedures](./reference.md)
- [Architecture principles](../../architecture/core/PRINCIPLES.md)

---

[^cockburn]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>.
