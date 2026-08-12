---
title: "hKask Capability — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-12
version: "2.0.0"
status: "Active"
domain: "Trust"
mds_categories: [trust]
---

# hKask Capability — Class Diagram

Type system after the 2026-08-12 removal of the per-call capability gate
(RR-0056). The crate now holds the dispatch port and the FIDES taint lattice
only — no tokens, no authorization check. `McpRuntime::invoke` meters the call
and dispatches it; the only pre-dispatch refusal is the runaway-loop breaker.

```mermaid
classDiagram
    class ToolPort {
        <<interface>>
        +invoke(server, tool, args, agent) ToolFuture
        +discover_tools() ToolFuture~Vec~String~~
        +get_tool_info(name) ToolFuture~Option~ToolInfo~~
    }

    class ToolInfo {
        +name: String
        +description: String
        +input_schema: Value
        +server_id: String
        +taint: ToolTaint
    }

    class ToolPortError {
        <<enumeration>>
        +EnergyBudgetExceeded(String)
        +NotFound(NotFound)
        +Unavailable(String)
        +InvocationFailed(String)
        +is_retryable() bool
    }

    class ToolTaint {
        <<enumeration>>
        Source
        Sink
        Pure
        Endorser
        +can_flow_to(target) bool
    }

    class McpRuntime {
        -servers: HashMap
        -tool_registry: HashMap
        -connections: HashMap
        -governance: Option
        +with_governance(cybernetics, sink) McpRuntime
        +register_server(server)
    }

    class CallCapManager {
        +charge_metered(agent) CallMeterOutcome
    }

    class CallMeterOutcome {
        <<enumeration>>
        Charged
        AutoRegistered
        CeilingReached
    }

    ToolPort ..> ToolInfo : returns
    ToolPort ..> ToolPortError : returns
    ToolInfo --> ToolTaint
    McpRuntime ..|> ToolPort : implements
    McpRuntime ..> CallCapManager : charges via CyberneticsLoop
    CallCapManager ..> CallMeterOutcome : returns
```

## What was removed (2026-08-12, RR-0056 / RR-0057)

- The per-call capability gate in `McpRuntime::invoke`. All three production mint
  sites derived the token's `resource_id` from the same tool name they passed to
  `invoke`, so `is_valid_for` compared a caller-supplied value against itself and
  could not deny.
- `DelegationToken` (with `new` / `is_valid_for`), `panel_default_token`,
  `capabilities_match`, `capability_from_server_id`, `CapabilitySpec`,
  `CapabilityParseError`, `DelegationResource`, `DelegationAction`
- `ToolPortError::CapabilityDenied`, `McpRuntime::verify_capability_domain`,
  `McpRuntime::required_capability_for`, the `ToolInfo::required_capability` field
- The `src/auth.rs` and `src/resources.rs` modules
- The fail-closed branch on an *absent* call ceiling — the meter now auto-registers
  an unseeded agent at `DEFAULT_RUNAWAY_CALL_CEILING` and logs the wiring gap
  (RR-0057)

## What remains

- `ToolPort` — `invoke` / `discover_tools` / `get_tool_info`, dyn-compatible via
  `ToolFuture`, implemented by `McpRuntime`. `invoke`'s `agent: WebID` is an
  accounting identity, not a credential.
- `ToolPortError` — `EnergyBudgetExceeded` / `NotFound` / `Unavailable` /
  `InvocationFailed`, plus `is_retryable` (true only for `Unavailable`)
- `ToolInfo` — tool metadata carrying the FIDES taint label
- `ToolTaint` — the FIDES lattice; `can_flow_to` blocks `Source`→`Sink` and
  nothing else
- `SYSTEM_MAX_RECURSION` — cascade depth limit (matryoshka), used by the manifest
  executor and the registry bootstrap; a recursion breaker, not an authority limit

Authority itself lives outside this crate: the per-request `tool_allowlist` on
the inference IPC dispatch, each swarm card's `mcp_tools` allowlist, and the
per-server MCP env/credential allowlists.

## Related

- [MCP Runtime Invoke Flow](./flowchart-mcp-runtime-invoke.md) — the metering path
- [Architecture Principles](../architecture/core/PRINCIPLES.md) — P4 Clear Boundaries
- [MDS](../architecture/core/MDS.md) — Trust category

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-001
verified_date: 2026-08-12
verified_against: kask/crates/hkask-capability/src/tool_port.rs; kask/crates/hkask-capability/src/tool_taint.rs; kask/crates/hkask-capability/src/token_types.rs; kask/crates/hkask-mcp/src/runtime.rs; kask/crates/hkask-regulation/src/energy.rs
status: VERIFIED
-->
