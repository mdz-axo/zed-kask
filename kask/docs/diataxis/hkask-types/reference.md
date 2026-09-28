---
title: "hkask-types — Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-28
version: "2.3.0"
status: "Active"
domain: "Foundation"
mds_categories: [domain, trust]
---

# hkask-types — Reference

`hkask-types` defines shared value types, identifiers, ports, paths, event and IPC
envelopes, resource limits, and cross-process contracts. The crate forbids unsafe
code at `kask/crates/hkask-types/src/hkask_types.rs:1` and follows the ports and
adapters pattern.[^hexagonal] Its generic identifiers use Rust's newtype discipline
to prevent cross-domain identifier confusion.[^newtype]

## Module inventory

The crate root is authoritative at
`kask/crates/hkask-types/src/hkask_types.rs:6-51`. The root also defines
`ExpectProposal`, the proposal template for a contract missing its user-facing
`expect:` annotation (`kask/crates/hkask-types/src/hkask_types.rs:76-90`).

```mermaid
classDiagram
    class hkask_types
    class agent_paths
    class block_provenance
    class corpus
    class curator
    class document
    class error
    class event
    class hmem_ontology
    class id
    class inference_ipc
    class json_extract
    class kanban_status
    class kanban_wire
    class media_limits
    class ocr_health
    class ytdlp
    class maybe_string
    class regulation
    class secret
    class server_env
    class spreadsheet
    class template
    class ports
    class process_global
    class time
    class tool_response
    class tool_schema
    class voice
    class sql_impls
    class url_utils
    class visibility
    hkask_types o-- agent_paths
    hkask_types o-- block_provenance
    hkask_types o-- corpus
    hkask_types o-- curator
    hkask_types o-- document
    hkask_types o-- error
    hkask_types o-- event
    hkask_types o-- hmem_ontology
    hkask_types o-- id
    hkask_types o-- inference_ipc
    hkask_types o-- json_extract
    hkask_types o-- kanban_status
    hkask_types o-- kanban_wire
    hkask_types o-- media_limits
    hkask_types o-- ocr_health
    hkask_types o-- ytdlp
    hkask_types o-- maybe_string
    hkask_types o-- regulation
    hkask_types o-- secret
    hkask_types o-- server_env
    hkask_types o-- spreadsheet
    hkask_types o-- template
    hkask_types o-- ports
    hkask_types o-- process_global
    hkask_types o-- time
    hkask_types o-- tool_response
    hkask_types o-- tool_schema
    hkask_types o-- voice
    hkask_types o-- sql_impls
    hkask_types o-- url_utils
    hkask_types o-- visibility
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-004
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/hkask_types.rs:6-51
status: VERIFIED
-->

`error`, `hmem_ontology`, and `kanban_status` are crate-private modules whose common
items are re-exported; `sql_impls` is feature-gated
(`kask/crates/hkask-types/src/hkask_types.rs:12,15,20,48-49,55-74`).

## Foundation modules

| Module | Principal surface | Evidence |
|---|---|---|
| `agent_paths` | internal-data and user-artifact roots, relative layout helpers, name sanitation | `kask/crates/hkask-types/src/agent_paths.rs:12-26,63-265` |
| `block_provenance` | `BlockProvenance` — server-authoritative widget-dispatch provenance wire contract | `kask/crates/hkask-types/src/block_provenance.rs:1-30,33` |
| `corpus` | `ExpertiseLevel`, `ChunkOntology`, `TaggedChunk` | `kask/crates/hkask-types/src/corpus.rs:60-326` |
| `curator` | escalation severity, directives, schema evolution, threshold config | `kask/crates/hkask-types/src/curator.rs:20-229` |
| `document` | `DocStructure`, `Page`, `Block` | `kask/crates/hkask-types/src/document.rs:21-98` |
| `error` | `DatabaseErrorKind`, `DbError`, `InfrastructureError`, `McpErrorKind`, `NotFound` | `kask/crates/hkask-types/src/error.rs:26-74,116-203,231-318` |
| `hmem_ontology` | `HMemOntology` | `kask/crates/hkask-types/src/hmem_ontology.rs:35-75` |
| `id` | sealed `Id<T>` aliases and `WebID` | `kask/crates/hkask-types/src/id/core.rs:8-183`; `kask/crates/hkask-types/src/id/webid.rs:9-107` |
| `json_extract` | balanced JSON extraction from model output; property tests and Kani proofs (2026-09-16) cover the extraction core | `kask/crates/hkask-types/src/json_extract.rs:47,187-269` |
| `kanban_status` / `kanban_wire` | task lifecycle and server/tool wire constants | `kask/crates/hkask-types/src/kanban_status.rs:11-71`; `kask/crates/hkask-types/src/kanban_wire.rs:17-36` |
| `maybe_string` | numeric-string-tolerant serde deserializers for `Option<u64>`, `Option<u32>`, `Option<f64>` | `kask/crates/hkask-types/src/maybe_string.rs:20,47,74` |
| `secret` | `SecretRef` — declarative env/keychain secret-source reference | `kask/crates/hkask-types/src/secret.rs:14-35` |
| `spreadsheet` | `AnalyticalTable`, `SpreadsheetArtifactRef`, `CellEdit`, `EditTransaction` — spreadsheet wire contracts | `kask/crates/hkask-types/src/spreadsheet.rs:78-113,338,444-493` |
| `template` | `LLMParameters` | `kask/crates/hkask-types/src/template.rs:14-85` |
| `time` | RFC 3339 timestamp helpers | `kask/crates/hkask-types/src/time.rs:18-44` |
| `tool_response` | tool envelopes, parsing, error classification, display hints | `kask/crates/hkask-types/src/tool_response.rs:30-196` |
| `tool_schema` | `AnyJsonValue` and boolean-schema inspection | `kask/crates/hkask-types/src/tool_schema.rs:46-147` |
| `url_utils` | YouTube ID extraction | `kask/crates/hkask-types/src/url_utils.rs:13-35` |
| `visibility` | visibility, access control, confidence, and 5W1H dimension | `kask/crates/hkask-types/src/visibility.rs:34-269` |
| `voice` | `VoiceDesign` | `kask/crates/hkask-types/src/voice.rs:15-63` |
| `ytdlp` | yt-dlp candidate paths, version parsing, and selection policy | `kask/crates/hkask-types/src/ytdlp.rs:3-25` |

## Shared invariant modules

| Module | Contract | Evidence |
|---|---|---|
| `media_limits` | concat, sequence, frame, variant, job-list, and workflow admission limits; reject rather than truncate | `kask/crates/hkask-types/src/media_limits.rs:1-24` |
| `ocr_health` | bounded `OcrHealthSnapshot` and canonical health-file path | `kask/crates/hkask-types/src/ocr_health.rs:11-71` |
| `server_env` | canonical MCP child environment wrapper | `kask/crates/hkask-types/src/server_env.rs:19-59` |
| `process_global` | re-settable clone-out-of-lock, poison-recovering hook slot | `kask/crates/hkask-types/src/process_global.rs:35-77` |

## Port surface

The `ports` module re-exports the current port and companion types at
`kask/crates/hkask-types/src/ports.rs:7-23`; the crate root re-exports the module at
`kask/crates/hkask-types/src/hkask_types.rs:74`.

```mermaid
classDiagram
    class InferencePort {
        <<interface>>
        +generate()
        +generate_with_model()
        +generate_with_messages()
        +generate_stream_with_messages()
        +generate_n()
        +generate_stream()
        +generate_stream_with_model()
        +generate_vision()
        +embed()
        +embed_with_identity()
        +rerank()
        +list_models()
        +list_vision_models()
        +media_generate()
    }
    class ToolDispatchPort {
        <<interface>>
        +invoke_tool()
    }
    class WorktreeSpawnPort {
        <<interface>>
        +create_worktree_thread()
    }
    class MemoryPort {
        <<interface>>
        +ingest_turn()
        +recall_context()
        +recall_thread()
    }
    class RegulationSink {
        <<interface>>
        +persist()
        +persist_if_absent()
    }
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-006
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/ports/inference_port.rs:115-183,184-431; kask/crates/hkask-types/src/ports/memory_port.rs:138-180; kask/crates/hkask-types/src/event.rs:497-515
status: VERIFIED
-->

| Cluster | Types | Evidence |
|---|---|---|
| Inference values | `ChatMessage`, `InferenceError`, `InferenceUsage` (with the `reported` flag), `ChatToolDefinition`, `ChatToolFunction`, `StructuredToolCall`, `InferenceResult`, `InferenceStreamChunk` | `kask/crates/hkask-types/src/ports/inference_types.rs:15-164` |
| Inference port | `EmbedFuture`, `EmbedWithIdentityFuture`, `MediaFuture`, `RerankFuture`, `MediaGenerateParams`, `ModelEntry`, `InferencePort` | `kask/crates/hkask-types/src/ports/inference_port.rs:11-113,184-431` |
| Tool/worktree ports | `ToolDispatchPort`, `WorktreeSpawnPort` | `kask/crates/hkask-types/src/ports/inference_port.rs:115-183` |
| Memory | `TurnRecord`, `GoalEvent`, `MemorySnippet`, `MemoryError`, `MemoryPort` | `kask/crates/hkask-types/src/ports/memory_port.rs:19-180` |
| Embedding | `EmbeddingBatch`, `EmbeddingGenerationError` | `kask/crates/hkask-types/src/ports/embedding.rs:3-26` |
| Consolidation | `ConsolidationRequest`, `ConsolidationOutcome` | `kask/crates/hkask-types/src/ports/regulation.rs:3-27` |

## Event and Regulation surface

| Type | Contract | Evidence |
|---|---|---|
| `RegulationRecord` | observer, span, phase, observation, optional regulation/outcome, parent, visibility | `kask/crates/hkask-types/src/event.rs:15-28` |
| `SpanNamespace` | validated namespace wrapper | `kask/crates/hkask-types/src/event.rs:60-65` |
| `SpanCategory` | Cybernetics, Curation, Inference, Memory, Skill, Unknown | `kask/crates/hkask-types/src/event.rs:258-330` |
| `Span` | validated namespace plus full path | `kask/crates/hkask-types/src/event.rs:359-409` |
| `SpanKind` | canonical reusable event pairs, including inference transition/recovery | `kask/crates/hkask-types/src/event.rs:411-459` |
| `CyclePhase` | Sense, Compute, Compare, Act | `kask/crates/hkask-types/src/event.rs:462-495` |
| `RegulationSink` | persistence and source-ID dedup port | `kask/crates/hkask-types/src/event.rs:497-515` |
| `LedgerHealth`, `RegulationHealth`, `RegulationSpan` | Regulation measurements and typed namespace source | `kask/crates/hkask-types/src/regulation.rs:25-155` |

## Inference IPC surface

The protocol is newline-delimited JSON between MCP child processes and the Zed
inference bridge (`kask/crates/hkask-types/src/inference_ipc.rs:1-47`).

```mermaid
sequenceDiagram
    participant Child as MCP child
    participant Request as InferenceRequest
    participant Zed as Zed inference bridge
    participant Response as InferenceResponse
    Child->>Request: id + method + params
    Request->>Zed: one JSON line
    Zed->>Response: same id + outcome
    Response-->>Child: one JSON line
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-010
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/inference_ipc.rs:1-47,53-120,122-298
status: VERIFIED
-->

| Surface | Current members | Evidence |
|---|---|---|
| Environment | `INFERENCE_SOCKET_ENV`, `TOOL_GRANT_ENV`, `INFERENCE_TIMEOUT_ENV` | `kask/crates/hkask-types/src/inference_ipc.rs:53-73` |
| Request envelope | `InferenceRequest { id, method, params }` | `kask/crates/hkask-types/src/inference_ipc.rs:77-87` |
| Methods | Generate, GenerateWithModel, GenerateWithMessages, GenerateVision, Embed, ListModels, ToolInvoke, ToolDefinition, CreateWorktreeThread, Rerank | `kask/crates/hkask-types/src/inference_ipc.rs:89-120` |
| Parameters | prompts/messages/images/model/tools, embedding, governed tool, worktree, and rerank fields | `kask/crates/hkask-types/src/inference_ipc.rs:122-179` |
| Response envelope | `InferenceResponse { id, outcome }` | `kask/crates/hkask-types/src/inference_ipc.rs:185-194` |
| Outcomes | Result, Embeddings, ModelList, ToolResult, ToolDefinition, WorktreeThread, RerankScores, Error | `kask/crates/hkask-types/src/inference_ipc.rs:196-252` |
| Auxiliary payloads | `RerankScoreEntry`, `ModelListEntry`, `WorktreeThreadInfo`, `InferenceErrorPayload` | `kask/crates/hkask-types/src/inference_ipc.rs:254-298` |

There are no batch companion envelopes in the current IPC surface. Multi-text
embedding and multi-document reranking are fields on `InferenceParams`, preserving
one correlation identity per request
(`kask/crates/hkask-types/src/inference_ipc.rs:134-137,170-180`).

## Filesystem path model

```mermaid
flowchart TD
    D[resolve_data_dir] --> A[agents]
    D --> M[mcp]
    D --> S[skills]
    D --> T[threads]
    U[resolve_artifacts_dir] --> V[visible server artifact directories]
    A --> AD[agent_dir and agent_db]
    M --> MD[mcp_server_db and mcp_server_subdir]
    V --> MA[mcp_artifacts_subdir]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-005
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/agent_paths.rs:12-26,42-46,63-265
status: VERIFIED
-->

`resolve_data_dir` and `resolve_artifacts_dir` are the two root regulators;
relative layout helpers compose beneath them
(`kask/crates/hkask-types/src/agent_paths.rs:63-265`). User-controlled segments
pass through `sanitize_name` at
`kask/crates/hkask-types/src/agent_paths.rs:238-265`.

## Identifier model

`Id<T: IdKind>` is a sealed phantom-generic newtype
(`kask/crates/hkask-types/src/id/core.rs:8-36`). Current aliases are declared at
`kask/crates/hkask-types/src/id/core.rs:170-183`. `WebID` supplies the stable
agent identity and deterministic `for_agent_name` derivation
(`kask/crates/hkask-types/src/id/webid.rs:9-64`).

## Procedures

Use these procedures when several kask crates must share a path, port, wire envelope,
resource limit, health snapshot, child-process environment, or process-global hook
without depending on one another's implementations. They apply the
ports and adapters pattern at the workspace boundary.[^hexagonal]

### Choose the smallest owning surface

| Need | Extend | Current examples |
|---|---|---|
| Relative data/artifact path | `agent_paths` | `kask/crates/hkask-types/src/agent_paths.rs:157-235` |
| Infrastructure behavior contract | `ports` | `kask/crates/hkask-types/src/ports.rs:7-23` |
| Parent/child inference wire shape | `inference_ipc` | `kask/crates/hkask-types/src/inference_ipc.rs:77-298` |
| Regulation event shape | `event` / `regulation` | `kask/crates/hkask-types/src/event.rs:15-28,258-515` |
| Shared media admission cap | `media_limits` | `kask/crates/hkask-types/src/media_limits.rs:1-24` |
| Cross-process OCR health | `ocr_health` | `kask/crates/hkask-types/src/ocr_health.rs:11-71` |
| Canonical MCP child environment | `server_env` | `kask/crates/hkask-types/src/server_env.rs:19-59` |
| Re-settable cross-crate hook | `process_global` | `kask/crates/hkask-types/src/process_global.rs:35-77` |

Do not create a generic companion type merely because one operation accepts
multiple values. Add a type only when it carries an independent invariant or wire
identity.

### Procedure A: Add a path helper

```mermaid
flowchart TD
    A[Choose internal-data or user-artifact root] --> B[Compose existing class directories]
    B --> C[Sanitize user-controlled segments]
    C --> D[Return relative PathBuf]
    D --> E[Resolve through the matching root helper]
    E --> F[Pin layout and hostile-name behavior]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-002
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/agent_paths.rs:12-26,63-154,157-235,238-265,312-443
status: VERIFIED
-->

1. Choose internal app data (`resolve_data_dir`) or visible user artifacts
   (`resolve_artifacts_dir`) at
   `kask/crates/hkask-types/src/agent_paths.rs:63-154`.
2. Reuse the existing class layout and helpers at
   `kask/crates/hkask-types/src/agent_paths.rs:157-235`.
3. Pass every user-controlled path segment through `sanitize_name`
   (`kask/crates/hkask-types/src/agent_paths.rs:238-265`).
4. Return a relative `PathBuf`; let the caller resolve it through
   `resolve_under_data_dir` or `resolve_under_artifacts_dir`.
5. Add a layout test alongside the current tests at
   `kask/crates/hkask-types/src/agent_paths.rs:312-443`.

### Procedure B: Add or extend a port

```mermaid
flowchart TD
    A[Define a Send + Sync trait in ports] --> B[Use named boxed future aliases when needed]
    B --> C[Re-export through ports.rs]
    C --> D[Implement in a downstream adapter]
    D --> E[Wire at the composition root]
    E --> F[Test object safety, delegation, and surfaced degradation]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TYPES-003
verified_date: 2026-09-28
verified_against: kask/crates/hkask-types/src/ports.rs:7-23; kask/crates/hkask-types/src/ports/inference_port.rs:17-40,115-183; kask/crates/hkask-types/src/ports/memory_port.rs:125-180; kask/crates/hkask-types/src/hkask_types.rs:74
status: VERIFIED
-->

1. Put the trait in the appropriate `ports/` module. Current public traits are
   `ToolDispatchPort`, `WorktreeSpawnPort`, `InferencePort`, and `MemoryPort`
   (`kask/crates/hkask-types/src/ports/inference_port.rs:115-183`;
   `kask/crates/hkask-types/src/ports/memory_port.rs:138-180`).
2. Use explicit `Pin<Box<dyn Future + Send>>` or a named alias when the trait must
   remain object-safe. Current aliases are `EmbedFuture`, `EmbedWithIdentityFuture`,
   `MediaFuture`, `RerankFuture`, and the crate-private `MemoryFuture`
   (`kask/crates/hkask-types/src/ports/inference_port.rs:17-40`;
   `kask/crates/hkask-types/src/ports/memory_port.rs:125`).
3. Re-export through `kask/crates/hkask-types/src/ports.rs:7-23`; the crate root
   already re-exports `ports::*` at
   `kask/crates/hkask-types/src/hkask_types.rs:74`.
4. Implement the port in the crate that owns the concrete backend and wire that
   adapter in the composition root.
5. Test object safety, delegation, and the unavailable path. Degradation must be
   surfaced as an error or explicit status, not disguised as successful empty data.

### Procedure C: Change an IPC or event contract

1. Update both sides of the boundary in the same change. For inference IPC, the
   request side is `InferenceRequest` / `InferenceMethod` / `InferenceParams` at
   `kask/crates/hkask-types/src/inference_ipc.rs:77-179`; the response side is
   `InferenceResponse` / `InferenceOutcome` and auxiliary payloads at
   `kask/crates/hkask-types/src/inference_ipc.rs:185-298`.
2. Preserve one request correlation ID and one response correlation ID. Do not add
   removed batch companion envelopes; multi-item operation inputs belong in the
   method parameters, as `embed_texts` and `rerank_documents` do at
   `kask/crates/hkask-types/src/inference_ipc.rs:134-137,170-180`.
3. For Regulation events, construct a validated namespace and add a `SpanKind`
   only when a canonical pair is reused across emitters
   (`kask/crates/hkask-types/src/event.rs:60-65,359-459`).
4. Add a serialization round-trip and an integration test that exercises the
   writer/reader or parent/child seam.

### Procedure D: Add a shared invariant module

Use a dedicated module when the shared item is not an infrastructure port:

- Add admission caps to `media_limits`; callers must reject rather than truncate
  (`kask/crates/hkask-types/src/media_limits.rs:1-24`).
- Extend `OcrHealthSnapshot` only with fields both file writer and reader can
  support, and preserve the canonical path helper
  (`kask/crates/hkask-types/src/ocr_health.rs:18-71`).
- Extend `ServerEnv` only for child-environment behavior that remains composed by
  the canonical builder (`kask/crates/hkask-types/src/server_env.rs:21-59`).
- Use `ProcessGlobal<T>` only for re-settable `Mutex<Option<T>>` hooks; use a
  set-once startup mechanism for immutable startup configuration
  (`kask/crates/hkask-types/src/process_global.rs:37-77`).

### Validation

Run the focused crate tests, then the project clippy wrapper:

```sh
cargo test -p hkask-types
./script/clippy
```

Also run the tests for every downstream writer, reader, or adapter changed with the
contract. A foundation type that serializes locally but is not consumed by the
other side has not validated the seam.

## See also

- [Why hkask-types splits boundary contracts by failure domain](./explanation.md)
- [Architecture principles](../../architecture/core/PRINCIPLES.md)

---

[^hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture.* <https://alistair.cockburn.us/hexagonal-architecture/>.
[^newtype]: Rust Community. (2024). *Rust API Guidelines — Newtype Pattern.* <https://rust-lang.github.io/api-guidelines/type-conventions.html#c-newtype>.
