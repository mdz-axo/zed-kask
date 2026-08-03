# hKask Capability — Class Diagram

Simplified type system after the 2026-08-02 OCAP removal. The `DelegationToken` carries 6 fields (no expiry, no signature, no attenuation). The `McpRuntime::invoke` gate does a pure capability-match + gas gate — no token expiry, no signature verification.

```mermaid
classDiagram
    class DelegationToken {
        +id: String
        +resource: DelegationResource
        +resource_id: String
        +action: DelegationAction
        +delegated_from: WebID
        +delegated_to: WebID
        +new() DelegationToken
        +is_valid_for(resource, resource_id, action) bool
        -generate_id() String
    }

    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, token) ToolFuture
        +discover_tools() Vec~String~
    }

    class ToolPortError {
        <<enumeration>>
        +CapabilityDenied(String)
        +EnergyBudgetExceeded(String)
        +NotFound(NotFound)
        +InvocationFailed(String)
    }

    class DelegationResource {
        <<enumeration>>
        Tool
        Template
        Registry
        Key
    }

    class DelegationAction {
        <<enumeration>>
        Read
        Write
        Execute
    }

    class CapabilitySpec {
        +resource: DelegationResource
        +resource_id: String
        +action: DelegationAction
        +parse(capability) Result
    }

    class McpRuntime {
        -servers: HashMap
        -tool_registry: HashMap
        -connections: HashMap
        -governance: Option
        +invoke(server, tool, args, token) Result
        +discover_tools() Vec~String~
    }

    DelegationToken --> DelegationResource
    DelegationToken --> DelegationAction
    CapabilitySpec --> DelegationResource
    CapabilitySpec --> DelegationAction
    ToolPort ..> DelegationToken : token param
    ToolPort ..> ToolPortError : returns
    McpRuntime ..|> ToolPort : implements
    McpRuntime ..> DelegationToken : checks is_valid_for
```

## What was removed (2026-08-02)

- `OcapConfig` struct + `ocap:` manifest blocks (59 files)
- `DelegationToken.expires_at` field + `new_with_expiry` constructor + `is_expired` method + `is_valid_for_at` (expiry-aware variant)
- `TokenRegistry` trait + `TokenRegistryStore` impl + `delegation_tokens` SQL table
- `verification/` module (signature-verification vocabulary)
- `CapabilityAwareValidator` (registration-time OCAP gate — zero production call sites)
- `Caveat` struct + `attenuation_level`/`max_attenuation`/`context_nonce`/`caveats` fields
- `CapabilityError` enum (never constructed)
- `list_tokens` curator tool + `TokenListRequest` type
- `SYSTEM_MAX_ATTENUATION` const (OCAP alias)

## What remains

- `DelegationToken` — 6 fields, minted in-process via `new()`, checked by `is_valid_for()`
- `ToolPort` trait — `invoke()` + `discover_tools()`, implemented by `McpRuntime`
- `ToolPortError` — `CapabilityDenied` / `EnergyBudgetExceeded` / `NotFound` / `InvocationFailed`
- `DelegationResource` / `DelegationAction` — enums for capability matching
- `CapabilitySpec` — parses `"tool:domain:action"` capability strings
- `capabilities_match` — action hierarchy: Execute ⊇ Write ⊇ Read
- `SYSTEM_MAX_RECURSION` — cascade depth limit (matryoshka), still used by the manifest executor

## Related

- [MCP Runtime Invoke Flow](./flowchart-mcp-runtime-invoke.md) — the simplified gate
- [Architecture Principles](../architecture/core/PRINCIPLES.md) — P4 Clear Boundaries
- [MDS](../architecture/core/MDS.md) — Trust category