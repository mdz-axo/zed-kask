# hkask-types — Foundation Types

Foundation type system for the hKask agent platform. Provides canonical ID
types, error infrastructure, the Regulation event/span model, hexagonal port
traits, visibility primitives, and shared domain types (loops, regulation,
curator, wallet, transcript, corpus, document, templates) used by all
downstream hKask crates.

`hkask-types` is the dependency root of the hKask crate tree: it must not
depend on `hkask-capability` (would create a cycle). Domain types that need
capability types live in their owning domain crates.

## Public Modules

| Module | Purpose |
|--------|---------|
| `id` | Strongly-typed IDs: `WebID`, `Id<T>` family (`BotID`, `HMemId`, `GoalID`, `TemplateID`, `PodID`, `UserID`, `WalletId`, `ApiKeyId`, …) |
| `error` | `InfrastructureError`, `McpErrorKind`, `DatabaseErrorKind`, `DbError`, `DbProvider`, `NotFound`, `CapabilityDenied` |
| `event` | `RegulationRecord`, `Span`, `SpanKind`, `SpanNamespace`, `SpanCategory`, `CyclePhase`, `RegulationSink` trait |
| `observable_span` | `ObservableSpan` trait — domain span enums implement this to emit Regulation events |
| `regulation` | `LedgerHealth`, `RegulationHealth`, `QueueDepth`, `RegulationSpan`, `ToolSubsystem` |
| `loops` | Loop type system: `LoopId`, `Signal`, `Deviation`, `ActionType`, `RegulatoryAction`, `LoopMetrics`, `ImpactReport` (moved from `hkask-regulation` to break a cycle) |
| `ports` | Hexagonal port traits: `InferencePort`, `ToolDispatchPort`, `SkillExecPort`, `MemoryPort`, `SkillRegistryIndex`, `RegistryIndex`, plus their request/result/error types (`ChatMessage`, `InferenceError`, `MemoryError`, …) |
| `curator` | `CuratorDirective`, `CuratorHandle`, `CurationThresholdConfig`, `EscalationSeverity` |
| `visibility` | `Visibility`, `Confidence`, `Dimension`, `AccessControl` |
| `template` / `template_type` | `LLMParameters`, `TemplateFile`, `TemplateCrate`, `TemplateType` |
| `tool_taint` | `ToolTaint` — FIDES information-flow labels (Source/Sink/Pure/Endorser) |
| `transcript` | `TimedWord`, `TranscriptSegment`, `TranscriptBundle` |
| `voice` | `VoiceDesign` |
| `corpus` | `TaggedChunk`, `ChunkOntology`, `ExpertiseLevel` |
| `document` | `DocStructure`, `Page`, `Block` |
| `crypto` / `secret` | `Ed25519PublicKey`, `Ed25519Signature`, `SecretRef`, `ZeroizingSecret` |
| `inference_ipc` | Unix-socket IPC envelope: `InferenceRequest`, `InferenceResponse`, `InferenceMethod`, `InferenceOutcome`, `INFERENCE_SOCKET_ENV` |
| `agent_paths` | Per-agent filesystem path helpers + data-dir resolution (`resolve_data_dir`, `resolve_under_data_dir`, `agent_dir`, `agent_pod_db`, …) and `DEFAULT_DB_PATH` |
| `server_config` | `ServerConfig`, `ServerRegistration`, `ServerConfigError` |
| `goal` | `GoalState` |
| `time` | `now_rfc3339` |
| `json_extract` | JSON-fence stripping / balanced-object extraction helpers |
| `keychain_keys` | Keychain key-name constants |
| `macros` | `enum_str_ops!`, `enum_snake_str!` (canonical, shared by all crates) |
| `sql_impls` | `FromSql`/`ToSql` impls for IDs and visibility types (`sql` feature) |

## Key Types

| Type | Description |
|------|-------------|
| `WebID` | Universal agent identifier (UUID-based, derived from persona) |
| `Id<T>` / `BotID` / `GoalID` / `HMemId` / … | Domain-specific typed IDs over `Uuid` with sealed `IdKind` markers |
| `InfrastructureError` | Universal error type for infrastructure failures |
| `McpErrorKind` | MCP tool error classification (retryable / requires-intervention) |
| `RegulationRecord` | Regulation event with namespace, category, observation |
| `ObservableSpan` | Trait for domain spans that emit Regulation events |
| `InferencePort` / `MemoryPort` / `SkillRegistryIndex` | Hexagonal port traits (implemented in downstream crates) |
| `ChatMessage` | Foundation inference message type (`role` + `content`) |
| `LLMParameters` | Temperature, top_p, max_tokens configuration |
| `ToolTaint` | FIDES IFC label for MCP tools (Source/Sink/Pure/Endorser) |
| `RJoule` | Energy/gas unit newtype over `u64` |

## Usage

```rust
use hkask_types::{WebID, InfrastructureError, RegulationRecord, GoalID};

let webid = WebID::from_persona(b"curator");
let goal = GoalID::new();
```

## Dependencies

`serde`, `serde_json`, `uuid`, `chrono`, `thiserror`, `dirs`, `zeroize`,
`hex`, `futures-util`, `async-trait`, `sha2`, `tracing`; optional `rusqlite`
(`sql` feature). The `enum_str_ops!` / `enum_snake_str!` macros are the
canonical string-enum helpers shared by all hKask crates.