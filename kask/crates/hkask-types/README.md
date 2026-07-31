# hkask-types — Foundation Types

Foundation type system for the hKask agent platform. Provides canonical ID types, error infrastructure, event model, agent definitions, and visibility primitives used by all downstream crates.

## Public Modules

| Module | Purpose |
|--------|---------|
| `id` | Strongly-typed IDs: `WebID`, `BotID`, `HMemId`, `GoalID`, `TemplateID` |
| `error` | `InfrastructureError`, `McpErrorKind`, `DatabaseErrorKind` |
| `event` | `NuEvent`, `NuEventSink` — Regulation event model |
| `agent_registry` | `AgentDefinition`, `Charter`, `RegisteredAgent`, `UserProfile` |
| `regulation` | `LedgerHealth`, `RegulationHealth` |
| `curator` | `CuratorDirective`, `CuratorHandle`, `CurationThresholdConfig` |
| `observable_span` | `ObservableSpan` trait and domain span enums |
| `macros` | Shared `enum_str_ops!` macro (canonical location) |
| `visibility` | `Visibility`, `Confidence`, `Dimension` |
| `template` | `LLMParameters` |
| `tool_taint` | `ToolTaint` — FIDES information flow control labels (Source/Sink/Pure/Endorser) |
| `crypto` | `Ed25519PublicKey` |
| `time` | `now_rfc3339` utility |

## Key Types

| Type | Description |
|------|-------------|
| `WebID` | Universal agent identifier |
| `HMemId` | Memory triple identifier |
| `GoalID` / `TemplateID` / `BotID` | Domain-specific typed IDs |
| `InfrastructureError` | Universal error type for all infrastructure failures |
| `NuEvent` | Regulation event with namespace, category, observation |
| `AgentDefinition` | Agent charter, rights, responsibilities, contacts |
| `CuratorDirective` | Curator-issued instruction (budget, escalation) |
| `ObservableSpan` | Trait for domain spans that emit Regulation events |
| `LLMParameters` | Temperature, top_p, max_tokens configuration |
| `ToolTaint` | FIDES IFC label for MCP tools (Source/Sink/Pure/Endorser) |

## Usage

```rust
use hkask_types::{WebID, InfrastructureError, NuEvent, GoalID};

let webid = WebID::from_persona(b"curator");
let goal = GoalID::new();
```

## Dependencies

- `serde`, `chrono`, `uuid`, `thiserror`, `rand`
- `enum_str_ops!` macro (canonical, shared by all crates)
