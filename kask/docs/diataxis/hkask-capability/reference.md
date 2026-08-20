---
title: "hkask-capability — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-13
version: "1.0.0"
status: "Active"
domain: "Sovereignty"
mds_categories: [domain, trust]
---

# hkask-capability — Reference

`hkask-capability` defines the tool dispatch port: the `ToolPort` trait, its
`ToolInfo` metadata, the `ToolFuture` alias, the `ToolPortError` taxonomy, and
the `SYSTEM_MAX_RECURSION` structural bound. That is the entire surface. It
contains **no capability tokens, no per-call authorization check, and no
information-flow labels**. Tool authority is enforced outside this crate, at
the allowlist boundaries listed under [Where authority is enforced](#where-authority-is-enforced).

## Source citations

| Symbol                       | Location                                                                                       |
| ---------------------------- | ---------------------------------------------------------------------------------------------- |
| `ToolPort` trait             | `kask/crates/hkask-capability/src/tool_port.rs:89-115`                                         |
| `ToolPortError` enum         | `kask/crates/hkask-capability/src/tool_port.rs:8-38`                                           |
| `ToolPortError::is_retryable`| `kask/crates/hkask-capability/src/tool_port.rs:40-53`                                          |
| `ToolFuture` type alias      | `kask/crates/hkask-capability/src/tool_port.rs:62`                                            |
| `ToolInfo` struct            | `kask/crates/hkask-capability/src/tool_port.rs:118-124`                                       |
| `SYSTEM_MAX_RECURSION` const | `kask/crates/hkask-capability/src/token_types.rs:7`                                            |
| Crate lib root               | `kask/crates/hkask-capability/src/hkask_capability.rs:1-27`                                    |
| `ToolPort` implementor      | `kask/crates/hkask-mcp/src/runtime.rs:969-1067` (`impl hkask_capability::ToolPort for McpRuntime`) |
| `McpRuntime::invoke` body    | `kask/crates/hkask-mcp/src/runtime.rs:969-1057`                                                |
| `McpRuntime::with_governance`| `kask/crates/hkask-mcp/src/runtime.rs:271-280`                                                 |
| `CallMeterOutcome` enum      | `kask/crates/hkask-regulation/src/energy.rs:35-45`                                             |
| `CallCapManager::charge_metered` | `kask/crates/hkask-regulation/src/energy.rs:217-243`                                       |
| `DEFAULT_RUNAWAY_CALL_CEILING` | `kask/crates/hkask-regulation/src/energy.rs:29`                                              |
| `CyberneticsLoop::charge_call_metered` | `kask/crates/hkask-regulation/src/cybernetics_loop.rs:617-621`                       |
| Per-request `tool_allowlist` gate | `kask/crates/kask_bridge/src/inference_ipc_server.rs:724-747`                              |
| Per-agent `mcp_tools` allowlist | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:236-346`                            |
| Per-server credential allowlist | `kask/crates/kask_bridge/src/mcp_servers.rs:26-43` (`BuiltinMcpServer.credentials`)         |
| `CapabilityTier::detect`     | `kask/crates/hkask-mcp-server/src/server/context.rs:94-104`                                   |

The crate's `src/` directory holds exactly three files: `hkask_capability.rs`
(lib root), `token_types.rs`, and `tool_port.rs`.

## Type surface

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
    }
    class ToolPortError {
        <<enumeration>>
        EnergyBudgetExceeded(String)
        NotFound(NotFound)
        Unavailable(String)
        Interrupted(String)
        InvocationFailed(String)
        +is_retryable() bool
    }
    class ToolFuture {
        <<type alias>>
        Pin~Box~dyn Future + Send + '_~~
    }
    class McpRuntime {
        -servers: HashMap
        -tool_registry: HashMap
        -connections: HashMap
        -governance: Option
    }

    ToolPort ..> ToolInfo : returns
    ToolPort ..> ToolPortError : returns
    ToolPort ..> ToolFuture : via
    McpRuntime ..|> ToolPort : implements
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-002
verified_date: 2026-08-13
verified_against: kask/crates/hkask-capability/src/tool_port.rs:62-124 (ToolFuture, ToolPort, ToolInfo); kask/crates/hkask-capability/src/tool_port.rs:8-53 (ToolPortError); kask/crates/hkask-mcp/src/runtime.rs:969-1067 (impl ToolPort for McpRuntime)
status: VERIFIED
-->

## `ToolPort` trait

`ToolPort` is the dispatch boundary for MCP tool invocation. Every method
returns `ToolFuture` (`Pin<Box<dyn Future + Send + '_>>`), so the trait is
object-safe and `Arc<dyn ToolPort>` works — this is why no adapter layer wraps
`McpRuntime` any more.

| Method            | Signature                                                                                     |
| ----------------- | --------------------------------------------------------------------------------------------- |
| `invoke`          | `(&self, server: &str, tool: &str, args: Value, agent: WebID) -> Result<Value, ToolPortError>` |
| `discover_tools`  | `(&self) -> Vec<String>`                                                                       |
| `get_tool_info`   | `(&self, tool_name: &str) -> Option<ToolInfo>`                                                 |

`agent` is the accounting identity for the call meter. `discover_tools` and
`get_tool_info` take no identity at all, because tool schemas are public per
the MCP protocol design (`tools/list` is an unauthenticated handshake).

## `ToolPortError` taxonomy

Five variants and one predicate, `is_retryable`, which is true only for
`Unavailable` — the call provably never reached the tool, so a retry cannot
duplicate a side effect.

| Variant                  | Meaning                                                                                       | Retryable |
| ------------------------ | --------------------------------------------------------------------------------------------- | --------- |
| `EnergyBudgetExceeded`   | The runaway-loop breaker tripped: the agent exhausted its per-tick call ceiling.             | No — needs a new regulation tick |
| `NotFound`               | The tool is not registered.                                                                    | No — would fail identically |
| `Unavailable`            | No live connection accepted the request; the tool provably never ran.                         | **Yes** |
| `Interrupted`            | A live peer accepted the request and the connection then dropped. The outcome is unknown.   | No — a retry could duplicate a side effect |
| `InvocationFailed`       | The call reached the tool and the tool failed.                                                | No — repeats the failure |

The `Unavailable` / `Interrupted` split is forced by `rmcp`, which reports both
a failed send and a dropped response channel as the same
`ServiceError::TransportClosed`. Once a request has reached a live peer, a
transport loss cannot be read as proof of non-delivery, so `Interrupted` is
never auto-retried at any layer.

## `ToolInfo` struct

Canonical tool metadata. Four descriptive fields, nothing that decides
anything:

```rust
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server_id: String,
}
```

`server_id` is how the MCP runtime's tool dispatch resolves which server
to dispatch to.

## `SYSTEM_MAX_RECURSION` structural bound

`SYSTEM_MAX_RECURSION` (7) is the shared bound for recursion depth and subgoal
nesting, consulted by the MCP runtime and the registry bootstrap. It is a
runaway-recursion breaker, not an authorization limit — the same distinction
the call meter draws.

## What `invoke` does

`McpRuntime::invoke` performs, in order:

1. **Call metering** — when governance is wired (`with_governance`), charge one
   call against the agent's per-tick ceiling via
   `CyberneticsLoop::charge_call_metered`, which delegates to
   `CallCapManager::charge_metered`. Without governance, dispatch unmetered.
2. **Dispatch** — `call_tool_inner` checks for a live connection, reconnects
   once if the transport closed, and issues the JSON-RPC call.
3. **Span emission** — persist a `reg.gas.settled` `RegulationRecord` (target
   `reg.mcp`) carrying server, tool, call count, and success/failure status,
   through the wired `RegulationSink`.

There is no authorization step. The only way `invoke` returns an error before
dispatch is `EnergyBudgetExceeded`.

## Call metering

The meter is a runaway-loop breaker and a usage recorder, not a permission
check. `CallCapManager::charge_metered` returns `CallMeterOutcome`:

| Outcome                        | Behavior                                                                                    |
| ------------------------------ | ------------------------------------------------------------------------------------------- |
| `Charged`                      | Headroom remained; the call proceeds.                                                       |
| `AutoRegistered`               | The agent had no registered ceiling: register at `DEFAULT_RUNAWAY_CALL_CEILING` (10 000), charge, proceed, and log the wiring gap. |
| `CeilingReached { ceiling }`   | The per-tick ceiling is exhausted: refuse with `ToolPortError::EnergyBudgetExceeded`. Resets on the next regulation tick. |

Fail-open on an unregistered agent is deliberate: a missing registration is a
composition-root wiring omission, and refusing it fails live paths without
protecting anything.

## Where authority is enforced

A capability check is only a gate when the authority list is not chosen by the
caller being checked. Three boundaries satisfy that:

| Boundary                                                        | Location                                                       | Note                                        |
| --------------------------------------------------------------- | -------------------------------------------------------------- | ------------------------------------------- |
| Per-request delegated-tool `tool_allowlist` (fail-closed on missing/empty) | `kask/crates/kask_bridge/src/inference_ipc_server.rs:724-747` `tool_invoke` dispatch | Enforced before dispatch; pinned by `dispatch_tool_invoke_rejects_unallowed_tool` |
| Per-agent declared `mcp_tools` allowlist                        | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:236-346` | Restricts which tools a swarm agent may call |
| Per-server MCP env / credential allowlists                      | `kask/crates/kask_bridge/src/mcp_servers.rs:26-43`              | Scopes credentials per server (`BuiltinMcpServer.credentials`) |

There is no fourth gate. A FIDES `Source`→`Sink` information-flow check used to
be listed here; it was deleted — see [Information flow](#information-flow-absent-by-decision).

## Information flow: absent by decision

Nothing in this crate or on the invoke path inspects information flow. Defense
**Layer 5 (information flow control) is absent by decision**, recorded in the
same register as Layer 3 (instruction hierarchy): de-advertised rather than
deployed.

The FIDES lattice labelled each tool `Source` / `Sink` / `Pure` / `Endorser`
and blocked `Source → Sink`. It was deleted rather than repaired because the
gate consuming it could not decide anything: `McpRuntime::get_tool_info`
hardcoded `Pure` at the only `ToolInfo` construction site, so the `Sink` arm
never matched. An inert gate is worse than no gate — it invites reliance on a
protection that does not exist.

Governing entry: `kask/security/regressions/RR-0053.yaml`, rewritten as an
absence check that forbids re-adding the machinery in inert form and states the
bar a real IFC gate must clear. Full rationale:
[`DIVERGENCE.md`](../../../../DIVERGENCE.md) D4 and
`kask/security/regressions/RR-0053.yaml`.

## CapabilityTier (sibling crate)

`CapabilityTier` lives in the sibling `hkask-mcp-server` crate, not here. It is
the per-server startup probe that distinguishes **embedded** mode (launched by
the hKask runtime, non-anonymous WebID, keystore reachable, persistence
available) from **standalone** mode (anonymous WebID, keystore may be
unavailable, persistence unavailable). Detection compares the resolved WebID
against the anonymous persona (`WebID::from_persona(b"anonymous")`) — not the
credential map, because `HKASK_WEBID` is an identity injected via `config_env`,
not a credential.

```mermaid
classDiagram
    class CapabilityTier {
        +embedded: bool
        +keystore_available: bool
        +persistence_available: bool
        +detect(webid, resolved_credentials) CapabilityTier
        +reg_available() bool
    }
    class WebID {
        +from_persona(bytes) WebID
    }
    CapabilityTier ..> WebID : compares against anonymous
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-003
verified_date: 2026-08-13
verified_against: kask/crates/hkask-mcp-server/src/server/context.rs:76-104 (CapabilityTier::detect); kask/crates/hkask-mcp-server/src/server/context.rs:108-118 (reg_available)
status: VERIFIED
-->

## See also

- [hkask-capability Explanation](./explanation.md): why per-call gating was
  removed and separation kept.
- [hkask-capability Tutorial](./tutorial.md): dispatching through the seam.
- [`kask/docs/architecture/core/PRINCIPLES.md`](../../architecture/core/PRINCIPLES.md):
  P4 (Clear Boundaries).
- `kask/security/regressions/RR-0053.yaml`, `RR-0056.yaml`, `RR-0057.yaml`.

---

[^fides-cap]: Microsoft Research. (2025). _FIDES: Information flow control for LLM agents_ (arXiv:2505.23643). The Source/Sink/Pure/Endorser lattice and the Source→Sink endorsement rule. Retained as the academic source for a design this crate no longer implements — the lattice was deleted (RR-0053). Citing it is not a claim that information flow control is deployed.

[^miller-ocap]: Miller, M. S. (2006). _Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control._ Johns Hopkins University. <https://www.erights.org/talks/thesis/markm-thesis.pdf>. The Object Capability model, retained here as the source of the principle that authority must be *separated* by a list the caller cannot choose.
